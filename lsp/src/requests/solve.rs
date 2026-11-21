//! `lp/solve` and `lp/solverInfo`: solving through the `lp-highs` helper
//! process.
//!
//! The language server never links `HiGHS` (design decision D3). It spawns the
//! helper once per solve, speaks the versioned JSON protocol of
//! [`lp_highs::protocol`] over its stdio, and kills it on cancellation, so a
//! crash, an out-of-memory MIP or a runaway presolve cannot take the server
//! down. Without a helper every other request works and `lp/solve` answers
//! `solverUnavailable`.
//!
//! The pure parts (settings, request building, response mapping and result
//! assembly) are plain functions; spawning and the event loop are async and
//! need the `server` feature.

use std::path::PathBuf;
use std::time::Duration;

use lp_highs::protocol::{
    Diagnose, ErrorKind, Format, HelperInfo, MAX_REQUEST_BYTES, PROTOCOL_VERSION, Phase, SolveOptions, SolveOutcome, SolveRequest,
    SolveResponse,
};
use lp_parser_rs::LpProblem;
use ls_types::Uri;
use serde::Deserialize;
use serde_json::{Value, json};

use super::sites::Sites;
use super::{DocumentRef, FailureKind, RequestError, Target};
use crate::document::Document;

/// Extra wall-clock time on top of the solver's own time limit before the
/// helper is killed.
pub const HARD_LIMIT_GRACE: Duration = Duration::from_secs(10);

/// Longest wall-clock limit honoured (a week), so absurd settings cannot overflow a deadline.
const MAX_LIMIT_SECONDS: f64 = 604_800.0;

/// How long the helper may take to answer `info`.
pub const PROBE_TIMEOUT: Duration = Duration::from_secs(3);

/// Largest helper response read from stdout.
pub const MAX_RESPONSE_BYTES: usize = 256 * 1024 * 1024;

/// How much of the helper's stderr is kept for error reports.
pub const STDERR_TAIL_BYTES: usize = 8 * 1024;

/// What `lp/solverInfo` says when no helper is found.
pub const NOT_FOUND: &str = "lp-highs not found next to lp-lsp, on PATH or at lp.solver.path";

/// What `lp/solverInfo` says when no helper is found in restricted mode.
pub const NOT_FOUND_RESTRICTED: &str = "lp-highs not found next to lp-lsp (restricted mode ignores lp.solver.path and PATH)";

/// `lp.solver`: where the helper is and how long a solve may take.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct SolverSettings {
    /// Path of the `lp-highs` binary; empty or absent means discover it.
    pub path: Option<String>,
    /// Solver time limit in seconds when a request names none.
    pub time_limit: f64,
}

impl Default for SolverSettings {
    fn default() -> Self {
        Self { path: None, time_limit: 60.0 }
    }
}

impl SolverSettings {
    /// Read `lp.solver` and `lp.solution` out of the client's `lp` settings.
    ///
    /// # Errors
    /// A message naming the setting that is wrong.
    pub fn from_value(value: &Value) -> Result<(Self, crate::solution::SolutionSettings), String> {
        let section = |key: &str| value.get(key).filter(|v| !v.is_null());
        let solver: Self = section("solver")
            .map_or_else(|| Ok(Self::default()), |v| serde_json::from_value(v.clone()))
            .map_err(|e| format!("invalid lp.solver settings: {e}"))?;
        if !(solver.time_limit.is_finite() && solver.time_limit > 0.0) {
            return Err(format!("lp.solver.timeLimit must be a positive number of seconds, got {}", solver.time_limit));
        }
        let solution = section("solution")
            .map_or_else(|| Ok(crate::solution::SolutionSettings::default()), |v| serde_json::from_value(v.clone()))
            .map_err(|e| format!("invalid lp.solution settings: {e}"))?;
        Ok((solver, solution))
    }

    /// The configured path, if it is not blank.
    #[must_use]
    pub fn configured_path(&self) -> Option<&str> {
        self.path.as_deref().map(str::trim).filter(|p| !p.is_empty())
    }
}

/// A usable helper.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Helper {
    /// The binary.
    pub path: PathBuf,
    /// Its `info` answer.
    pub info: HelperInfo,
}

/// Whether a helper is available.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SolverStatus {
    /// Found and speaking our protocol.
    Available(Helper),
    /// Not usable, and why.
    Unavailable(String),
}

impl Default for SolverStatus {
    fn default() -> Self {
        Self::Unavailable("not probed yet".to_owned())
    }
}

impl SolverStatus {
    /// Whether a helper is available.
    #[must_use]
    pub const fn is_available(&self) -> bool {
        matches!(self, Self::Available(_))
    }

    /// The `lp/solverInfo` result.
    #[must_use]
    pub fn info_json(&self) -> Value {
        match self {
            Self::Available(helper) => json!({
                "available": true,
                "path": helper.path.display().to_string(),
                "helperVersion": helper.info.version,
                "highsVersion": helper.info.highs,
                "protocol": helper.info.protocol,
            }),
            Self::Unavailable(reason) => json!({ "available": false, "reason": reason }),
        }
    }

    /// The helper, or `solverUnavailable`.
    ///
    /// # Errors
    /// `solverUnavailable` with `data.reason`.
    pub fn helper(&self) -> Result<&Helper, RequestError> {
        match self {
            Self::Available(helper) => Ok(helper),
            Self::Unavailable(reason) => {
                Err(RequestError::failed(FailureKind::SolverUnavailable, format!("no solver available: {reason}"))
                    .with("reason", reason.as_str()))
            }
        }
    }
}

/// Check an `info` answer: parse it and compare protocols.
///
/// # Errors
/// The reason the helper is not usable.
pub fn parse_info(stdout: &[u8]) -> Result<HelperInfo, String> {
    let info: HelperInfo = serde_json::from_slice(stdout).map_err(|e| format!("`lp-highs info` printed invalid JSON: {e}"))?;
    if info.protocol != PROTOCOL_VERSION {
        return Err(format!(
            "{} {} speaks protocol {}, but this lp-lsp expects protocol {PROTOCOL_VERSION}",
            info.name, info.version, info.protocol
        ));
    }
    Ok(info)
}

/// Params of `lp/solve`.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Params {
    /// Document and progress token.
    #[serde(flatten)]
    pub target: Target,
    /// Solver options.
    #[serde(default)]
    pub options: Option<SolveOptions>,
    /// Whether to diagnose an infeasible model (default `auto`).
    #[serde(default)]
    pub diagnose: Option<Diagnose>,
    /// Return the solver log (default false).
    #[serde(default)]
    pub include_log: bool,
}

/// The helper request for `text`, with `settings` filling in the time limit.
///
/// # Errors
/// `-32602` for an invalid option, `tooLarge` when the text is over the protocol limit.
pub fn build_request(text: &str, params: &Params, settings: &SolverSettings) -> Result<SolveRequest, RequestError> {
    if text.len() > MAX_REQUEST_BYTES {
        return Err(RequestError::failed(
            FailureKind::TooLarge,
            format!("the model is {} bytes; the solver limit is {MAX_REQUEST_BYTES}", text.len()),
        ));
    }
    let mut request = SolveRequest::new(Format::Lp, text);
    request.options = params.options.clone().unwrap_or_default();
    request.options.time_limit.get_or_insert(settings.time_limit);
    request.diagnose = params.diagnose.unwrap_or_default();
    request.include_log = params.include_log;
    request.validate().map_err(|e| RequestError::invalid(e.to_string()))?;
    if let Some(threads) = request.options.threads
        && threads == 0
    {
        return Err(RequestError::invalid("threads must be at least 1"));
    }
    Ok(request)
}

/// Wall-clock limit for the helper: its time limit plus a grace period.
#[must_use]
pub fn hard_limit(request: &SolveRequest) -> Duration {
    let seconds = request.options.time_limit.unwrap_or(0.0).clamp(0.0, MAX_LIMIT_SECONDS);
    Duration::from_secs_f64(seconds) + HARD_LIMIT_GRACE
}

/// The outcome of a helper response, or the error it stands for.
///
/// # Errors
/// `parseError`, `unsupportedModel` or `solverFailed` per the response's error kind.
pub fn outcome(response: SolveResponse) -> Result<SolveOutcome, RequestError> {
    if let Err(violation) = response.validate() {
        return Err(RequestError::failed(FailureKind::SolverFailed, format!("the solver helper broke the protocol: {violation}")));
    }
    match (response.result, response.error) {
        (Some(result), _) if response.ok => Ok(result),
        (_, Some(error)) => {
            let kind = match error.kind {
                ErrorKind::ParseError => FailureKind::ParseError,
                ErrorKind::UnsupportedModel => FailureKind::UnsupportedModel,
                ErrorKind::SolverFailed | ErrorKind::Protocol => FailureKind::SolverFailed,
            };
            Err(RequestError::failed(kind, error.message))
        }
        _ => Err(RequestError::failed(FailureKind::SolverFailed, "the solver helper sent an empty response")),
    }
}

/// Name of a phase, as shown in progress messages.
#[must_use]
pub const fn phase_name(phase: Phase) -> &'static str {
    match phase {
        Phase::Parsing => "parsing",
        Phase::Building => "building",
        Phase::Solving => "solving",
        Phase::Extracting => "extracting",
        Phase::Diagnosing => "diagnosing",
    }
}

/// The `lp/solve` result: the helper's outcome plus `uri`, `version` and
/// source ranges for every variable, constraint and implicated bound.
///
/// # Panics
/// Never in practice: a solve outcome always serialises.
#[must_use]
pub fn result_json(doc: &Document, problem: &LpProblem, document: &DocumentRef, outcome: &SolveOutcome) -> Value {
    let sites = Sites::new(doc, problem);
    let constraint_range = |name: &str| problem.name_id(name).and_then(|id| sites.constraint(id)).map(|s| s.range);
    let range_json = |range: Option<ls_types::Range>| serde_json::to_value(range).unwrap_or(Value::Null);

    let mut value = serde_json::to_value(outcome).expect("a solve outcome always serialises");
    value["uri"] = json!(document.uri);
    value["version"] = json!(document.version);
    let annotate = |list: &mut Value, key: &str, range_of: &dyn Fn(&str) -> Value| {
        if let Some(items) = list.as_array_mut() {
            for item in items {
                let range = item.get(key).and_then(Value::as_str).map_or(Value::Null, range_of);
                item["range"] = range;
            }
        }
    };
    annotate(&mut value["variables"], "name", &|n| range_json(sites.variable(n)));
    annotate(&mut value["constraints"], "name", &|n| range_json(constraint_range(n)));
    if let Some(infeasibility) = value.get_mut("infeasibility").filter(|v| v.is_object()) {
        annotate(&mut infeasibility["constraints"], "name", &|n| range_json(constraint_range(n)));
        annotate(&mut infeasibility["bounds"], "variable", &|n| range_json(sites.variable(n)));
    }
    value
}

/// The folder a solve runs in, so a `highs.opt` next to the model applies.
#[must_use]
pub fn working_directory(uri: &Uri) -> Option<PathBuf> {
    uri.to_file_path().and_then(|p| p.parent().map(std::path::Path::to_path_buf)).filter(|dir| dir.is_dir())
}

#[cfg(feature = "server")]
pub use process::{probe, run_helper};

#[cfg(feature = "server")]
mod process {
    //! Spawning the helper.

    use std::path::{Path, PathBuf};
    use std::process::Stdio;
    use std::time::Instant;

    use lp_highs::protocol::{Event, Phase, SolveRequest, SolveResponse};
    use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
    use tokio::process::Command;
    use tokio::sync::mpsc::UnboundedSender;

    use super::{
        FailureKind, Helper, MAX_RESPONSE_BYTES, NOT_FOUND, NOT_FOUND_RESTRICTED, PROBE_TIMEOUT, RequestError, STDERR_TAIL_BYTES,
        SolverSettings, SolverStatus, hard_limit, parse_info,
    };
    use crate::requests::Cancel;

    /// Name of the helper binary.
    const BINARY: &str = if cfg!(windows) { "lp-highs.exe" } else { "lp-highs" };

    /// Locate the helper (setting, then next to this executable, then `PATH`;
    /// only next to this executable when `restricted`) and ask it for `info`.
    pub async fn probe(settings: &SolverSettings, restricted: bool) -> SolverStatus {
        let path = match locate(settings, restricted) {
            Ok(path) => path,
            Err(reason) => return SolverStatus::Unavailable(reason),
        };
        match info(&path).await {
            Ok(info) => SolverStatus::Available(Helper { path, info }),
            Err(reason) => SolverStatus::Unavailable(reason),
        }
    }

    pub(super) fn locate(settings: &SolverSettings, restricted: bool) -> Result<PathBuf, String> {
        let beside = std::env::current_exe().ok().and_then(|exe| exe.parent().map(|dir| dir.join(BINARY)));
        if restricted {
            // Untrusted workspace: neither the setting nor `PATH` may pick the executable.
            return beside.filter(|candidate| candidate.is_file()).ok_or_else(|| NOT_FOUND_RESTRICTED.to_owned());
        }
        if let Some(configured) = settings.configured_path() {
            let path = PathBuf::from(configured);
            return if path.is_file() { Ok(path) } else { Err(format!("lp.solver.path '{configured}' is not a file")) };
        }
        let on_path = std::env::var_os("PATH")
            .into_iter()
            .flat_map(|paths| std::env::split_paths(&paths).collect::<Vec<_>>())
            .map(|dir| dir.join(BINARY));
        beside.into_iter().chain(on_path).find(|candidate| candidate.is_file()).ok_or_else(|| NOT_FOUND.to_owned())
    }

    async fn info(path: &Path) -> Result<lp_highs::protocol::HelperInfo, String> {
        let output = tokio::time::timeout(
            PROBE_TIMEOUT,
            Command::new(path).arg("info").stdin(Stdio::null()).stderr(Stdio::null()).kill_on_drop(true).output(),
        )
        .await
        .map_err(|_| format!("{} did not answer `info` within {} s", path.display(), PROBE_TIMEOUT.as_secs()))?
        .map_err(|e| format!("cannot run {}: {e}", path.display()))?;
        if !output.status.success() {
            return Err(format!("`{} info` failed with {}", path.display(), output.status));
        }
        parse_info(&output.stdout)
    }

    /// Run one solve: write the request, stream phase events to `events`, read
    /// the response. Killed when the future is dropped, on cancellation, or
    /// after the time limit plus a grace period.
    ///
    /// # Errors
    /// `solverFailed` (spawn failure, crash, timeout, invalid output) or cancellation.
    pub async fn run_helper(
        helper: &Helper,
        request: &SolveRequest,
        cwd: Option<PathBuf>,
        cancel: &Cancel,
        events: UnboundedSender<(Phase, u64)>,
    ) -> Result<SolveResponse, RequestError> {
        let failed = |message: String, tail: &str| {
            let error = RequestError::failed(FailureKind::SolverFailed, message);
            if tail.is_empty() { error } else { error.with("stderrTail", tail) }
        };
        let body = serde_json::to_vec(request).map_err(|e| failed(format!("cannot encode the solve request: {e}"), ""))?;

        let mut command = Command::new(&helper.path);
        command.arg("solve").stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);
        if let Some(cwd) = cwd {
            command.current_dir(cwd);
        }
        let mut child = command.spawn().map_err(|e| failed(format!("cannot start {}: {e}", helper.path.display()), ""))?;
        let (Some(mut stdin), Some(mut stdout), Some(stderr)) = (child.stdin.take(), child.stdout.take(), child.stderr.take()) else {
            return Err(failed("the solver helper's pipes were not created".to_owned(), ""));
        };

        // Writing, reading stdout and reading stderr run concurrently: a large
        // model would otherwise fill a pipe and deadlock.
        let writer = tokio::spawn(async move {
            stdin.write_all(&body).await?;
            stdin.shutdown().await
        });
        let reader = tokio::spawn(async move {
            let mut out = Vec::new();
            let limit = u64::try_from(MAX_RESPONSE_BYTES).unwrap_or(u64::MAX).saturating_add(1);
            (&mut stdout).take(limit).read_to_end(&mut out).await.map(|_| out)
        });
        let logger = tokio::spawn(async move {
            let mut tail = String::new();
            // Once nobody listens, events are dropped; stderr is still drained into the tail.
            let mut listening = true;
            let mut lines = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                match Event::from_line(&line) {
                    Some(Event::Phase { phase, elapsed_ms }) => {
                        if listening && events.send((phase, elapsed_ms)).is_err() {
                            listening = false;
                        }
                    }
                    None => {
                        tail.push_str(&line);
                        tail.push('\n');
                        if tail.len() > STDERR_TAIL_BYTES {
                            let mut cut = tail.len() - STDERR_TAIL_BYTES;
                            while !tail.is_char_boundary(cut) {
                                cut += 1;
                            }
                            tail.drain(..cut);
                        }
                    }
                }
            }
            tail
        });

        let started = Instant::now();
        let deadline = hard_limit(request);
        let mut tick = tokio::time::interval(std::time::Duration::from_millis(50));
        let status = loop {
            tokio::select! {
                status = child.wait() => break status,
                _ = tick.tick() => {
                    if cancel.is_cancelled() {
                        stop(&mut child).await.map_err(|e| failed(format!("cancelled, but the solver helper could not be stopped: {e}"), ""))?;
                        return Err(RequestError::Cancelled);
                    }
                    if started.elapsed() > deadline {
                        stop(&mut child).await.map_err(|e| failed(format!("the solver helper could not be stopped: {e}"), ""))?;
                        let tail = logger.await.map_err(|e| failed(format!("stderr reader failed: {e}"), ""))?;
                        return Err(failed(format!("the solver helper was killed after {} s (time limit plus grace)", deadline.as_secs()), &tail));
                    }
                }
            }
        };
        let tail = logger.await.map_err(|e| failed(format!("stderr reader failed: {e}"), ""))?;
        let status = status.map_err(|e| failed(format!("cannot wait for the solver helper: {e}"), &tail))?;
        let out = reader
            .await
            .map_err(|e| failed(format!("stdout reader failed: {e}"), &tail))?
            .map_err(|e| failed(format!("cannot read the solver helper's output: {e}"), &tail))?;
        // A helper that died early closes its stdin: a failed write is then the symptom, not the cause.
        let write = writer.await.map_err(|e| failed(format!("stdin writer failed: {e}"), &tail))?;
        if out.len() > MAX_RESPONSE_BYTES {
            return Err(failed(format!("the solver helper's response is over {MAX_RESPONSE_BYTES} bytes"), &tail));
        }
        let text = String::from_utf8_lossy(&out);
        match SolveResponse::from_json(&text) {
            Ok(response) if status.success() => Ok(response),
            Ok(response) if !response.ok => Ok(response),
            Ok(_) => Err(failed(format!("the solver helper exited with {status} after reporting success"), &tail)),
            Err(violation) if status.success() => {
                let note = write.err().map_or_else(String::new, |e| format!(" (writing the request failed: {e})"));
                Err(failed(format!("the solver helper sent an unusable response: {violation}{note}"), &tail))
            }
            Err(_) => {
                let note = write.err().map_or_else(String::new, |e| format!(" (writing the request failed: {e})"));
                Err(failed(format!("the solver helper exited with {status}{note}"), &tail))
            }
        }
    }

    /// Kill the helper and reap it. `start_kill` fails only when the process
    /// has already exited, which is the goal anyway.
    async fn stop(child: &mut tokio::process::Child) -> std::io::Result<()> {
        if child.start_kill().is_ok() {
            child.wait().await?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use lp_highs::protocol::{
        ConstraintSolution, DiagnosisMethod, Infeasibility, InfeasibleBound, InfeasibleConstraint, ProtocolError, Side, Skipped, SolverId,
        Timings, VariableSolution,
    };

    use super::super::testing::{MODEL, doc, target};
    use super::*;

    fn params() -> Params {
        Params { target: target(), options: None, diagnose: None, include_log: false }
    }

    fn sample_outcome() -> SolveOutcome {
        SolveOutcome {
            solver: SolverId { name: "HiGHS".into(), version: "1.11.0".into(), helper: "lp-highs 0.1.0".into() },
            status: "Optimal".into(),
            objective_value: Some(12.0),
            mip_gap: None,
            timings: Timings { build_ms: 1, solve_ms: 2, extract_ms: 3 },
            variables: vec![
                VariableSolution { name: "x".into(), value: 1.0, reduced_cost: None },
                VariableSolution { name: "nope".into(), value: 0.0, reduced_cost: None },
            ],
            constraints: vec![ConstraintSolution { name: "c1".into(), activity: 2.0, dual: None, slack: 0.0, binding: true }],
            skipped: Skipped::default(),
            infeasibility: None,
            log: None,
        }
    }

    #[test]
    fn requests_take_the_default_time_limit_and_validate_options() {
        let settings = SolverSettings::default();
        let request = build_request("min\nend", &params(), &settings).expect("valid");
        assert_eq!((request.protocol, request.format), (1, Format::Lp));
        assert_eq!(request.options.time_limit, Some(60.0));
        assert_eq!(request.diagnose, Diagnose::Auto);

        let mut p = params();
        p.options = Some(SolveOptions { time_limit: Some(5.0), mip_rel_gap: Some(0.01), presolve: None, threads: Some(2) });
        p.diagnose = Some(Diagnose::Never);
        p.include_log = true;
        let request = build_request("min\nend", &p, &settings).expect("valid");
        assert_eq!(
            (request.options.time_limit, request.options.threads, request.diagnose, request.include_log),
            (Some(5.0), Some(2), Diagnose::Never, true)
        );

        p.options = Some(SolveOptions { time_limit: Some(-1.0), ..SolveOptions::default() });
        assert!(matches!(build_request("min\nend", &p, &settings), Err(RequestError::InvalidParams(_))));
        p.options = Some(SolveOptions { threads: Some(0), ..SolveOptions::default() });
        assert!(matches!(build_request("min\nend", &p, &settings), Err(RequestError::InvalidParams(_))));
        p.options = Some(SolveOptions { mip_rel_gap: Some(f64::NAN), ..SolveOptions::default() });
        assert!(matches!(build_request("min\nend", &p, &settings), Err(RequestError::InvalidParams(_))));
    }

    #[test]
    fn the_hard_limit_is_the_time_limit_plus_grace_and_never_overflows() {
        let mut request = SolveRequest::new(Format::Lp, "");
        request.options.time_limit = Some(5.0);
        assert_eq!(hard_limit(&request), Duration::from_secs(15));
        request.options.time_limit = Some(f64::MAX);
        assert_eq!(hard_limit(&request), Duration::from_secs(604_800) + HARD_LIMIT_GRACE);
    }

    #[test]
    fn responses_map_to_outcomes_and_error_kinds() {
        let ok = outcome(SolveResponse::success(sample_outcome())).expect("ok");
        assert_eq!(ok.status, "Optimal");
        for (kind, expected) in [
            (ErrorKind::ParseError, FailureKind::ParseError),
            (ErrorKind::UnsupportedModel, FailureKind::UnsupportedModel),
            (ErrorKind::SolverFailed, FailureKind::SolverFailed),
            (ErrorKind::Protocol, FailureKind::SolverFailed),
        ] {
            let error = outcome(SolveResponse::failure(kind, "nope")).err();
            assert!(matches!(error, Some(RequestError::Failed(f)) if f.kind == expected && f.message == "nope"), "{kind:?}");
        }
        let inconsistent = SolveResponse {
            protocol: 1,
            ok: true,
            result: None,
            error: Some(ProtocolError { kind: ErrorKind::ParseError, message: "x".into() }),
        };
        assert!(matches!(outcome(inconsistent), Err(RequestError::Failed(f)) if f.kind == FailureKind::SolverFailed));
        let wrong_protocol = SolveResponse { protocol: 2, ..SolveResponse::success(sample_outcome()) };
        assert!(outcome(wrong_protocol).is_err());
    }

    #[test]
    fn info_is_checked_for_protocol_compatibility() {
        let good = br#"{"name":"lp-highs","version":"0.1.0","protocol":1,"highs":"1.11.0"}"#;
        assert_eq!(parse_info(good).map(|i| i.version), Ok("0.1.0".to_owned()));
        let future = br#"{"name":"lp-highs","version":"9.0.0","protocol":2,"highs":"1.11.0"}"#;
        let reason = parse_info(future).expect_err("mismatch");
        assert!(reason.contains("protocol 2") && reason.contains("expects protocol 1"), "{reason}");
        assert!(parse_info(b"not json").expect_err("garbage").contains("invalid JSON"));
    }

    #[cfg(feature = "server")]
    #[test]
    fn restricted_mode_ignores_the_configured_solver_path() {
        let existing = concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml");
        let settings = SolverSettings { path: Some(existing.to_owned()), time_limit: 1.0 };
        assert_eq!(process::locate(&settings, false), Ok(PathBuf::from(existing)));
        assert_ne!(process::locate(&settings, true), Ok(PathBuf::from(existing)));
    }

    #[test]
    fn status_renders_solver_info_and_unavailable_errors() {
        let info = HelperInfo { name: "lp-highs".into(), version: "0.1.0".into(), protocol: 1, highs: "1.11.0".into() };
        let available = SolverStatus::Available(Helper { path: PathBuf::from("/opt/lp-highs"), info });
        assert_eq!(
            available.info_json(),
            json!({ "available": true, "path": "/opt/lp-highs", "helperVersion": "0.1.0", "highsVersion": "1.11.0", "protocol": 1 })
        );
        let missing = SolverStatus::Unavailable(NOT_FOUND.to_owned());
        assert_eq!(missing.info_json(), json!({ "available": false, "reason": NOT_FOUND }));
        let error = missing.helper().err().and_then(|e| e.data());
        assert_eq!(error.as_ref().map(|d| d["kind"].clone()), Some(json!("solverUnavailable")));
        assert_eq!(error.map(|d| d["reason"].clone()), Some(json!(NOT_FOUND)));
        assert!(!missing.is_available() && available.is_available());
    }

    #[test]
    fn settings_come_from_the_lp_section() {
        let (solver, solution) = SolverSettings::from_value(
            &json!({ "solver": { "path": " /x/lp-highs ", "timeLimit": 30 }, "solution": { "showStale": true } }),
        )
        .expect("valid");
        assert_eq!((solver.configured_path(), solver.time_limit, solution.show_stale), (Some("/x/lp-highs"), 30.0, true));
        let (defaults, _) = SolverSettings::from_value(&Value::Null).expect("defaults");
        assert_eq!(defaults, SolverSettings::default());
        assert_eq!(SolverSettings { path: Some("  ".into()), time_limit: 1.0 }.configured_path(), None);
        assert!(SolverSettings::from_value(&json!({ "solver": { "timeLimit": 0 } })).is_err());
        assert!(SolverSettings::from_value(&json!({ "solver": { "timeLimit": "soon" } })).is_err());
    }

    #[test]
    fn results_carry_uri_version_and_ranges() {
        let d = doc(MODEL);
        let problem = LpProblem::parse(MODEL).expect("parses");
        let document = DocumentRef::new(&"file:///t.lp".parse().expect("uri"), Some(7));
        let mut o = sample_outcome();
        o.infeasibility = Some(Infeasibility {
            method: DiagnosisMethod::Iis,
            constraints: vec![InfeasibleConstraint { name: "c2".into(), side: Side::Upper, violation: None }],
            bounds: vec![InfeasibleBound { variable: "y".into(), side: Side::Lower, gap: None }],
            total_violation: None,
        });
        let value = result_json(&d, &problem, &document, &o);
        assert_eq!((value["uri"].clone(), value["version"].clone()), (json!("file:///t.lp"), json!(7)));
        assert_eq!(value["status"], "Optimal");
        assert_eq!(value["objectiveValue"], 12.0);
        assert!(value["variables"][0]["range"]["start"].is_object());
        assert_eq!(value["variables"][1]["range"], json!(null), "a name the document does not define has no range");
        assert!(value["constraints"][0]["range"]["start"]["line"].is_u64());
        assert!(value["infeasibility"]["constraints"][0]["range"]["start"].is_object());
        assert!(value["infeasibility"]["bounds"][0]["range"]["start"].is_object());
        assert_eq!(value["infeasibility"]["method"], "iis");
        assert_eq!(value["solver"]["name"], "HiGHS");
        assert_eq!(value["skipped"], json!({ "sos": 0 }));
        let no_infeasibility = result_json(&d, &problem, &document, &sample_outcome());
        assert_eq!(no_infeasibility["infeasibility"], json!(null));
    }

    #[test]
    fn solves_run_in_the_documents_folder() {
        let dir = std::env::temp_dir();
        let uri: Uri = format!("file://{}/model.lp", dir.display()).parse().expect("uri");
        assert_eq!(working_directory(&uri).and_then(|d| d.canonicalize().ok()), dir.canonicalize().ok());
        assert_eq!(working_directory(&"untitled:Untitled-1".parse().expect("uri")), None);
    }

    #[test]
    fn oversized_models_are_too_large() {
        let text = "x".repeat(MAX_REQUEST_BYTES + 1);
        assert!(
            matches!(build_request(&text, &params(), &SolverSettings::default()), Err(RequestError::Failed(f)) if f.kind == FailureKind::TooLarge)
        );
    }
}
