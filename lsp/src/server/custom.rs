//! The custom `lp/*` requests: wiring of [`crate::requests`] into the server
//! (blocking pool, cancellation, `$/progress`, the solver helper).
//!
//! Handlers never block the request loop: pure work runs on the blocking pool
//! and the solver is a child process awaited asynchronously. Each request runs
//! in its own task, so a client's `$/cancelRequest` (which drops the request
//! future) flips a [`Cancel`] token through a drop guard and the task then
//! stops the work and closes its progress, instead of leaving it dangling.

use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use ls_types::notification::Notification;
use ls_types::{ProgressToken, Uri, WorkDoneProgressCancelParams};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::mpsc;
use tower_lsp_server::jsonrpc::{Error, ErrorCode, Result};
use tower_lsp_server::{Bounded, Cancellable, ClientSocket, LspService, OngoingProgress, Unbounded};

use super::{Backend, read, write};
use crate::requests::solve::{self, SolverSettings, SolverStatus, phase_name};
use crate::requests::{Cancel, CancelOnDrop, DocumentRef, PROGRESS_ROWS, RequestError, Source, diff, explain, graph, matrix, model};
use crate::solution::Solution;
use crate::workspace;

/// How often a running request's progress is sampled.
const PROGRESS_INTERVAL: Duration = Duration::from_millis(100);

/// Build the language service with every custom method registered.
#[must_use]
pub fn service() -> (LspService<Backend>, ClientSocket) {
    LspService::build(Backend::new)
        .custom_method("lp/model", Backend::lp_model)
        .custom_method("lp/explain", Backend::lp_explain)
        .custom_method("lp/graph", Backend::lp_graph)
        .custom_method("lp/matrix", Backend::lp_matrix)
        .custom_method("lp/diff", Backend::lp_diff)
        .custom_method("lp/solve", Backend::lp_solve)
        .custom_method("lp/solverInfo", Backend::lp_solver_info)
        // tower-lsp-server does not route the client's progress cancellation.
        .custom_method("window/workDoneProgress/cancel", Backend::work_done_progress_cancel)
        .finish()
}

/// `lp/solutionChanged`: a solve result was stored for a document.
pub struct SolutionChanged;

/// Params of [`SolutionChanged`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SolutionChangedParams {
    /// The solved document.
    pub uri: String,
    /// Its version at the time of the solve.
    pub version: Option<i32>,
    /// `HiGHS` model status.
    pub status: String,
}

impl Notification for SolutionChanged {
    type Params = SolutionChangedParams;
    const METHOD: &'static str = "lp/solutionChanged";
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Removes a work-done token's cancellation entry when the request ends.
struct TokenGuard {
    state: Arc<super::State>,
    token: ProgressToken,
}

impl Drop for TokenGuard {
    fn drop(&mut self) {
        lock(&self.state.progress_cancels).remove(&self.token);
    }
}

/// Holds a document's solve slot; frees it only if it is still this solve's.
struct SolveSlot {
    state: Arc<super::State>,
    key: Uri,
    cancel: Cancel,
}

impl Drop for SolveSlot {
    fn drop(&mut self) {
        let mut solves = lock(&self.state.solves);
        if solves.get(&self.key).is_some_and(|current| current.same(&self.cancel)) {
            solves.remove(&self.key);
        }
    }
}

fn internal(what: &str, error: &tokio::task::JoinError) -> Error {
    Error { code: ErrorCode::InternalError, message: format!("{what} failed: {error}").into(), data: None }
}

impl Backend {
    /// Version the client has the document open at; `None` for an indexed file.
    fn open_version(&self, uri: &Uri) -> Option<i32> {
        read(&self.state.open).get(uri).map(|doc| doc.version)
    }

    /// Route `window/workDoneProgress/cancel` to the request owning the token.
    fn register_token(&self, token: &ProgressToken, cancel: &Cancel) -> TokenGuard {
        lock(&self.state.progress_cancels).insert(token.clone(), cancel.clone());
        TokenGuard { state: Arc::clone(&self.state), token: token.clone() }
    }

    async fn work_done_progress_cancel(&self, params: WorkDoneProgressCancelParams) {
        let cancel = lock(&self.state.progress_cancels).get(&params.token).cloned();
        match cancel {
            Some(cancel) => cancel.cancel(),
            None => self.log(ls_types::MessageType::LOG, format!("cancel for unknown progress token {:?}", params.token)).await,
        }
    }

    /// Run `work` on the blocking pool as a request: cancellable, with
    /// progress when the client sent a `workDoneToken` and the model has more
    /// than [`PROGRESS_ROWS`] rows.
    async fn request<R: Send + 'static>(
        &self,
        token: Option<ProgressToken>,
        title: &'static str,
        work: impl FnOnce(&Cancel) -> std::result::Result<R, RequestError> + Send + 'static,
    ) -> Result<R> {
        let cancel = Cancel::new();
        // Dropped with this future on `$/cancelRequest`; the task below then stops.
        let guard = CancelOnDrop(cancel.clone());
        let this = self.clone();
        let task = tokio::spawn(async move { this.drive(token, title, cancel, work).await });
        let outcome = task.await.map_err(|e| internal(title, &e))?;
        drop(guard);
        outcome.map_err(Error::from)
    }

    async fn drive<R: Send + 'static>(
        &self,
        token: Option<ProgressToken>,
        title: &'static str,
        cancel: Cancel,
        work: impl FnOnce(&Cancel) -> std::result::Result<R, RequestError> + Send + 'static,
    ) -> std::result::Result<R, RequestError> {
        let _registration = token.as_ref().map(|t| self.register_token(t, &cancel));
        let worker_cancel = cancel.clone();
        let mut worker = tokio::task::spawn_blocking(move || work(&worker_cancel));
        let mut progress: Option<OngoingProgress<Bounded, Cancellable>> = None;
        let mut last = 0;
        let mut tick = tokio::time::interval(PROGRESS_INTERVAL);
        let joined = loop {
            tokio::select! {
                joined = &mut worker => break joined,
                _ = tick.tick(), if token.is_some() => {
                    if cancel.total() > PROGRESS_ROWS {
                        let began = self.begin_bounded(token.as_ref(), title, &mut progress).await;
                        let percentage = cancel.percentage();
                        if let (Some(p), true) = (began, percentage != last) {
                            p.report(percentage, None).await;
                            last = percentage;
                        }
                    }
                }
            }
        };
        // A big request always gets its begin/end, however fast it was.
        if cancel.total() > PROGRESS_ROWS {
            self.begin_bounded(token.as_ref(), title, &mut progress).await;
        }
        if let Some(progress) = progress {
            progress.finish().await;
        }
        joined.map_err(|e| RequestError::Internal(format!("{title} worker failed: {e}")))?
    }

    /// Start (once) progress for `token`; returns the running progress.
    async fn begin_bounded<'a>(
        &self,
        token: Option<&ProgressToken>,
        title: &'static str,
        progress: &'a mut Option<OngoingProgress<Bounded, Cancellable>>,
    ) -> Option<&'a OngoingProgress<Bounded, Cancellable>> {
        if progress.is_none()
            && let Some(token) = token
        {
            *progress = Some(self.client.progress(token.clone(), title).with_percentage(0).with_cancel_button().begin().await);
        }
        progress.as_ref()
    }

    async fn lp_model(&self, params: model::Params) -> Result<model::Result> {
        let uri = params.target.text_document.uri.clone();
        let doc = self.document_or_error(&uri)?;
        let (version, token) = (self.open_version(&uri), params.target.work_done_token.clone());
        let settings = self.config().analysis;
        self.request(token, "Reading the model", move |cancel| model::handle(&doc, version, &params, &settings, cancel)).await
    }

    async fn lp_explain(&self, params: explain::Params) -> Result<explain::Result> {
        let uri = params.target.text_document.uri.clone();
        let doc = self.document_or_error(&uri)?;
        let (version, token) = (self.open_version(&uri), params.target.work_done_token.clone());
        let solution = read(&self.state.solutions).get(&workspace::key(&uri)).cloned();
        let settings = self.config().analysis;
        self.request(token, "Explaining", move |cancel| explain::handle(&doc, version, &params, solution.as_deref(), &settings, cancel))
            .await
    }

    async fn lp_graph(&self, params: graph::Params) -> Result<graph::Result> {
        let uri = params.target.text_document.uri.clone();
        let doc = self.document_or_error(&uri)?;
        let (version, token) = (self.open_version(&uri), params.target.work_done_token.clone());
        self.request(token, "Building the dependency graph", move |cancel| graph::handle(&doc, version, &params, cancel)).await
    }

    async fn lp_matrix(&self, params: matrix::Params) -> Result<matrix::Result> {
        let uri = params.target.text_document.uri.clone();
        let doc = self.document_or_error(&uri)?;
        let (version, token) = (self.open_version(&uri), params.target.work_done_token.clone());
        self.request(token, "Building the coefficient matrix", move |cancel| matrix::handle(&doc, version, &params, cancel)).await
    }

    async fn lp_diff(&self, params: diff::Params) -> Result<diff::Result> {
        let uri = params.target.text_document.uri.clone();
        let doc = self.document_or_error(&uri)?;
        let (version, token) = (self.open_version(&uri), params.target.work_done_token.clone());
        // The old side may be another open or indexed document: resolve it now,
        // from the stores, and read the file from disk on the worker.
        let other = match &params.against {
            diff::Against::Uri { uri } => {
                let other: Uri = uri.parse().map_err(|e| Error::invalid_params(format!("invalid `against.uri`: {e}")))?;
                Some((self.document(&other), other))
            }
            diff::Against::Text { .. } => None,
        };
        self.request(token, "Comparing models", move |cancel| {
            params.validate()?;
            let against = match (&params.against, other) {
                (diff::Against::Text { text, label }, _) => diff::Resolved { text: text.clone(), label: label.clone() },
                (diff::Against::Uri { uri }, Some((Some(open), _))) => diff::Resolved { text: open.text.clone(), label: Some(uri.clone()) },
                (diff::Against::Uri { uri }, Some((None, other))) => {
                    let path = other
                        .to_file_path()
                        .ok_or_else(|| RequestError::invalid(format!("`against.uri` {uri} is neither open nor a file")))?;
                    let size =
                        std::fs::metadata(&path).map_err(|e| RequestError::invalid(format!("cannot read {}: {e}", path.display())))?.len();
                    diff::check_size(usize::try_from(size).unwrap_or(usize::MAX))?;
                    let text = std::fs::read_to_string(&path)
                        .map_err(|e| RequestError::invalid(format!("cannot read {}: {e}", path.display())))?;
                    diff::Resolved { text, label: Some(uri.clone()) }
                }
                (diff::Against::Uri { .. }, None) => return Err(RequestError::Internal("`against.uri` was not resolved".to_owned())),
            };
            diff::handle(&doc, version, &params, &against, cancel)
        })
        .await
    }

    async fn lp_solver_info(&self, _params: Value) -> Result<Value> {
        let status = self.refresh_solver().await;
        Ok(status.info_json())
    }

    /// Probe for the helper with the current settings and remember the answer.
    pub(super) async fn refresh_solver(&self) -> SolverStatus {
        let settings = read(&self.state.solver_settings).clone();
        let status = solve::probe(&settings, self.state.restricted.load(Ordering::Relaxed)).await;
        *write(&self.state.solver) = status.clone();
        status
    }

    /// Apply the `lp.solver` and `lp.solution` settings; returns an error message for bad ones.
    pub(super) fn apply_solver_settings(&self, value: &Value) -> std::result::Result<(), String> {
        let (solver, solution) = SolverSettings::from_value(value)?;
        *write(&self.state.solver_settings) = solver;
        *write(&self.state.solution_settings) = solution;
        Ok(())
    }

    /// Whether a solver helper was found at the last probe.
    pub(super) fn solver_available(&self) -> bool {
        read(&self.state.solver).is_available()
    }

    /// Forget the solve state of a closed document, cancelling a running solve.
    pub(super) fn forget_solves(&self, uri: &Uri) {
        let key = workspace::key(uri);
        write(&self.state.solutions).remove(&key);
        if let Some(running) = lock(&self.state.solves).remove(&key) {
            running.cancel();
        }
    }

    async fn lp_solve(&self, params: solve::Params) -> Result<Value> {
        let cancel = Cancel::new();
        let guard = CancelOnDrop(cancel.clone());
        let this = self.clone();
        let task = tokio::spawn(async move { this.solve(params, cancel).await });
        let outcome = task.await.map_err(|e| internal("solve", &e))?;
        drop(guard);
        outcome.map_err(Error::from)
    }

    /// One solve: supersede any earlier solve of the document, run the helper,
    /// store and announce the result.
    async fn solve(&self, params: solve::Params, cancel: Cancel) -> std::result::Result<Value, RequestError> {
        let uri = params.target.text_document.uri.clone();
        let doc = self.document(&uri).ok_or_else(|| RequestError::invalid(format!("unknown document {}", uri.as_str())))?;
        let version = self.open_version(&uri);
        let key = workspace::key(&uri);
        let _registration = params.target.work_done_token.as_ref().map(|t| self.register_token(t, &cancel));
        if let Some(previous) = lock(&self.state.solves).insert(key.clone(), cancel.clone()) {
            previous.cancel();
        }
        let _slot = SolveSlot { state: Arc::clone(&self.state), key: key.clone(), cancel: cancel.clone() };

        let progress = match &params.target.work_done_token {
            Some(token) => {
                let name = uri.as_str().rsplit('/').next().unwrap_or("model");
                Some(self.client.progress(token.clone(), format!("Solving {name}")).with_cancel_button().begin().await)
            }
            None => None,
        };
        let result = self.solve_with_progress(&params, &doc, version, &uri, &cancel, progress.as_ref()).await;
        if let Some(progress) = progress {
            progress.finish().await;
        }
        let (value, solution) = result?;
        // A newer solve or a close that arrived while the result was assembled wins.
        cancel.check()?;
        let status = solution.status.clone();
        write(&self.state.solutions).insert(key, Arc::new(solution));
        self.client.send_notification::<SolutionChanged>(SolutionChangedParams { uri: uri.as_str().to_owned(), version, status }).await;
        // An infeasibility diagnosis is published as `solver/iis-member` diagnostics.
        self.publish(&uri).await;
        self.refresh_diagnostics().await;
        Ok(value)
    }

    async fn solve_with_progress(
        &self,
        params: &solve::Params,
        doc: &Arc<crate::document::Document>,
        version: Option<i32>,
        uri: &Uri,
        cancel: &Cancel,
        progress: Option<&OngoingProgress<Unbounded, Cancellable>>,
    ) -> std::result::Result<(Value, Solution), RequestError> {
        cancel.check()?;
        let mut status = read(&self.state.solver).clone();
        if !status.is_available() {
            // The helper may have been installed since the last probe.
            status = self.refresh_solver().await;
        }
        let helper = status.helper()?.clone();
        let settings = read(&self.state.solver_settings).clone();

        let (source, request) = {
            let (doc, params) = (Arc::clone(doc), params.clone());
            self.blocking(move || {
                let source = Source::acquire(&doc)?;
                let request = solve::build_request(&doc.text, &params, &settings)?;
                Ok((source, request))
            })
            .await?
        };
        cancel.check()?;

        let (events, mut received) = mpsc::unbounded_channel();
        let forward = async {
            while let Some((phase, elapsed_ms)) = received.recv().await {
                if let Some(progress) = progress {
                    #[expect(clippy::cast_precision_loss, reason = "a progress message")]
                    let seconds = elapsed_ms as f64 / 1000.0;
                    progress.report_with_message(format!("{} ({seconds:.1} s)", phase_name(phase)), Some(true)).await;
                }
            }
        };
        let cwd = solve::working_directory(uri);
        let (response, ()) = tokio::join!(solve::run_helper(&helper, &request, cwd, cancel, events), forward);
        let outcome = solve::outcome(response?)?;

        let document = DocumentRef::new(uri, version);
        let doc = Arc::clone(doc);
        self.blocking(move || {
            let value = solve::result_json(&doc, source.problem(), &document, &outcome);
            Ok((value, Solution::new(version, &outcome)))
        })
        .await
    }

    /// Run `work` on the blocking pool.
    async fn blocking<R: Send + 'static>(
        &self,
        work: impl FnOnce() -> std::result::Result<R, RequestError> + Send + 'static,
    ) -> std::result::Result<R, RequestError> {
        tokio::task::spawn_blocking(work).await.map_err(|e| RequestError::Internal(format!("worker failed: {e}")))?
    }
}
