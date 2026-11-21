//! Custom `lp/*` requests: model explorer, explanations, dependency graph,
//! coefficient matrix, mathematical diff and the solver.
//!
//! Every handler is a synchronous pure function over a [`Document`] (the
//! server runs them on the blocking pool), so each can be tested without a
//! client. Handlers poll a [`Cancel`] token every [`POLL_EVERY`] rows and
//! return [`RequestError::Cancelled`] when the request was cancelled, so a
//! `$/cancelRequest` stops the work instead of letting it run to the end.
//!
//! Wire conventions (see `docs/design/fleet.md`, section 2.1):
//!
//! * every result echoes the request's `uri` and the document `version` it
//!   describes (`null` for a file indexed from disk but not open);
//! * numbers are finite JSON numbers: a missing bound is `null`, never
//!   infinity or NaN;
//! * errors are `-32602` (bad params), `-32800` (cancelled) or `-32803` with
//!   `data.kind` one of `parseError`, `unsupportedModel`, `solverUnavailable`,
//!   `solverFailed` and `tooLarge`.

pub mod diff;
pub mod explain;
pub mod graph;
pub mod matrix;
pub mod model;
pub mod sites;
pub mod solve;
mod structure;

use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use lp_parser_rs::analysis::{IssueSeverity, ModelDiagnostic, ModelDiagnostics};
use lp_parser_rs::model::{ComparisonOp, ConstraintClass, VariableKind};
use lp_parser_rs::{LpParseError, LpProblem};
use ls_types::{Position, TextDocumentIdentifier, Uri};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
pub use solve::{SolverSettings, SolverStatus};

use crate::config::{AnalysisSettings, SeverityOverride};
use crate::document::Document;
use crate::semantic::SemanticResult;

/// Requests over this many rows report `begin`/`report`/`end` progress.
pub const PROGRESS_ROWS: usize = 10_000;

/// Handlers poll for cancellation every this many rows or entries.
pub const POLL_EVERY: usize = 1024;

/// The methods this server implements, in the order they are advertised.
pub const REQUESTS: [&str; 7] = ["lp/model", "lp/explain", "lp/graph", "lp/matrix", "lp/diff", "lp/solve", "lp/solverInfo"];

/// Notifications the server sends.
pub const NOTIFICATIONS: [&str; 1] = ["lp/solutionChanged"];

/// Version of the custom protocol, advertised in `capabilities.experimental.lp`.
pub const PROTOCOL: u32 = 1;

/// `capabilities.experimental` for the initialise result.
#[must_use]
pub fn experimental_capabilities(solver_available: bool) -> Value {
    json!({ "lp": { "protocol": PROTOCOL, "requests": REQUESTS, "notifications": NOTIFICATIONS, "solver": solver_available } })
}

/// Why a request did not produce a result.
#[derive(Debug, Clone, PartialEq)]
pub enum RequestError {
    /// Unknown document, malformed params or a cap exceeded in the params (`-32602`).
    InvalidParams(String),
    /// The request was cancelled (`-32800`).
    Cancelled,
    /// The request is well formed but could not be served (`-32803`).
    Failed(Failure),
    /// The server itself failed (`-32603`): a worker panicked or was lost.
    Internal(String),
}

/// The `data` of a `-32803 RequestFailed` error.
#[derive(Debug, Clone, PartialEq)]
pub struct Failure {
    /// Machine-readable class.
    pub kind: FailureKind,
    /// Human-readable detail.
    pub message: String,
    /// Further fields of `data` (`side`, `reason`, `stderrTail`, ...).
    pub extra: Map<String, Value>,
}

/// `data.kind` of a `-32803` error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailureKind {
    /// The model text does not parse.
    ParseError,
    /// The solver cannot model something in the file.
    UnsupportedModel,
    /// No usable solver helper.
    SolverUnavailable,
    /// The solver helper failed.
    SolverFailed,
    /// An input is over the size cap.
    TooLarge,
}

impl FailureKind {
    /// The wire name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ParseError => "parseError",
            Self::UnsupportedModel => "unsupportedModel",
            Self::SolverUnavailable => "solverUnavailable",
            Self::SolverFailed => "solverFailed",
            Self::TooLarge => "tooLarge",
        }
    }
}

impl RequestError {
    /// A `parseError`.
    #[must_use]
    pub fn parse(error: &LpParseError) -> Self {
        Self::failed(FailureKind::ParseError, error.to_string())
    }

    /// A `-32803` error of `kind`.
    #[must_use]
    pub fn failed(kind: FailureKind, message: impl Into<String>) -> Self {
        Self::Failed(Failure { kind, message: message.into(), extra: Map::new() })
    }

    /// `-32602` with `message`.
    #[must_use]
    pub fn invalid(message: impl Into<String>) -> Self {
        Self::InvalidParams(message.into())
    }

    /// Add a field to `data` (no effect on the other error classes).
    #[must_use]
    pub fn with(mut self, key: &str, value: impl Into<Value>) -> Self {
        if let Self::Failed(failure) = &mut self {
            failure.extra.insert(key.to_owned(), value.into());
        }
        self
    }

    /// The JSON `data` of a [`Self::Failed`] error.
    #[must_use]
    pub fn data(&self) -> Option<Value> {
        let Self::Failed(failure) = self else { return None };
        let mut data = failure.extra.clone();
        data.insert("kind".to_owned(), failure.kind.as_str().into());
        data.insert("message".to_owned(), failure.message.clone().into());
        Some(Value::Object(data))
    }
}

impl fmt::Display for RequestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidParams(message) => write!(f, "invalid params: {message}"),
            Self::Cancelled => f.write_str("request cancelled"),
            Self::Failed(failure) => write!(f, "{}: {}", failure.kind.as_str(), failure.message),
            Self::Internal(message) => write!(f, "internal error: {message}"),
        }
    }
}

impl std::error::Error for RequestError {}

#[cfg(feature = "server")]
impl From<RequestError> for tower_lsp_server::jsonrpc::Error {
    fn from(error: RequestError) -> Self {
        use tower_lsp_server::jsonrpc::{Error, ErrorCode};
        /// `RequestFailed` in the LSP specification.
        const REQUEST_FAILED: i64 = -32803;
        match &error {
            RequestError::InvalidParams(message) => Error::invalid_params(message.clone()),
            RequestError::Cancelled => Error::request_cancelled(),
            RequestError::Internal(message) => Error { code: ErrorCode::InternalError, message: message.clone().into(), data: None },
            RequestError::Failed(failure) => {
                Error { code: ErrorCode::ServerError(REQUEST_FAILED), message: failure.message.clone().into(), data: error.data() }
            }
        }
    }
}

/// Cooperative cancellation and progress for one request, shared between the
/// async side (which sets it) and the blocking handler (which polls it).
#[derive(Debug, Clone, Default)]
pub struct Cancel {
    inner: Arc<CancelState>,
}

#[derive(Debug, Default)]
struct CancelState {
    cancelled: AtomicBool,
    done: AtomicUsize,
    total: AtomicUsize,
}

impl Cancel {
    /// A token that is not cancelled.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether `other` is a clone of this token.
    #[must_use]
    pub fn same(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }

    /// Cancel the request. Idempotent.
    pub fn cancel(&self) {
        self.inner.cancelled.store(true, Ordering::Release);
    }

    /// Whether [`Self::cancel`] was called.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.inner.cancelled.load(Ordering::Acquire)
    }

    /// `Err(Cancelled)` once cancelled.
    ///
    /// # Errors
    /// [`RequestError::Cancelled`] when the request was cancelled.
    pub fn check(&self) -> Result<(), RequestError> {
        if self.is_cancelled() { Err(RequestError::Cancelled) } else { Ok(()) }
    }

    /// Cheap poll for loops: checks every [`POLL_EVERY`]th `index` and records
    /// it as the progress made.
    ///
    /// # Errors
    /// [`RequestError::Cancelled`] when the request was cancelled.
    pub fn tick(&self, index: usize) -> Result<(), RequestError> {
        if index.is_multiple_of(POLL_EVERY) {
            self.inner.done.store(index, Ordering::Relaxed);
            self.check()
        } else {
            Ok(())
        }
    }

    /// Declare how many rows the request works through (for progress).
    pub fn set_total(&self, total: usize) {
        self.inner.total.store(total, Ordering::Relaxed);
        self.inner.done.store(0, Ordering::Relaxed);
    }

    /// Rows declared with [`Self::set_total`].
    #[must_use]
    pub fn total(&self) -> usize {
        self.inner.total.load(Ordering::Relaxed)
    }

    /// Progress as a percentage, 0 until a total is declared.
    #[must_use]
    pub fn percentage(&self) -> u32 {
        let total = self.total();
        if total == 0 {
            return 0;
        }
        let done = self.inner.done.load(Ordering::Relaxed).min(total);
        u32::try_from(done * 100 / total).unwrap_or(100)
    }
}

/// Cancels its token when dropped. The server holds one for the lifetime of a
/// request future: tower-lsp drops the future on `$/cancelRequest`, which
/// stops a handler already running on the blocking pool.
#[derive(Debug)]
pub struct CancelOnDrop(pub Cancel);

impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        self.0.cancel();
    }
}

/// `textDocument` plus the optional `workDoneToken` every request accepts.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Target {
    /// The document the request is about.
    pub text_document: TextDocumentIdentifier,
    /// Client-supplied progress token.
    #[serde(default)]
    pub work_done_token: Option<ls_types::ProgressToken>,
}

/// `uri` and `version` echoed by every result.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DocumentRef {
    /// The URI the client asked about.
    pub uri: String,
    /// Document version the result describes; `null` for an unopened file.
    pub version: Option<i32>,
}

impl DocumentRef {
    /// Reference `uri` at `version`.
    #[must_use]
    pub fn new(uri: &Uri, version: Option<i32>) -> Self {
        Self { uri: uri.as_str().to_owned(), version }
    }
}

/// An LSP position in a request.
pub type At = Position;

/// The parsed model of a document: the cached semantic pass when it is current,
/// else a fresh parse.
#[derive(Debug)]
pub enum Source {
    /// The semantic pass for this document version (always a successful parse).
    Cached(Arc<SemanticResult>),
    /// Parsed for this request.
    Fresh(Box<LpProblem>),
}

impl Source {
    /// Model for `doc`.
    ///
    /// # Errors
    /// `parseError` when the text does not parse.
    pub fn acquire(doc: &Document) -> Result<Self, RequestError> {
        if let Some(result) = doc.semantic_result.as_ref().filter(|r| r.version == doc.version) {
            return match &result.outcome {
                Ok(_) => Ok(Self::Cached(Arc::clone(result))),
                Err(error) => Err(RequestError::parse(error)),
            };
        }
        LpProblem::parse(&doc.text).map(|problem| Self::Fresh(Box::new(problem))).map_err(|e| RequestError::parse(&e))
    }

    /// The model.
    #[must_use]
    pub fn problem(&self) -> &LpProblem {
        match self {
            Self::Cached(result) => match &result.outcome {
                Ok(model) => &model.problem,
                // `acquire` only builds `Cached` from a successful outcome.
                Err(_) => unreachable!("a cached source always holds a parsed model"),
            },
            Self::Fresh(problem) => problem,
        }
    }
}

/// The model diagnostics a request reports, as the Problems panel shows them:
/// the per-version cache (`Model::diagnostics`) when it was computed under the
/// current settings, else a fresh pass; then only what `lp.analysis` publishes.
#[derive(Debug)]
pub struct Analysis<'a> {
    findings: Findings<'a>,
    settings: &'a AnalysisSettings,
}

#[derive(Debug)]
enum Findings<'a> {
    Cached(&'a ModelDiagnostics),
    Fresh(ModelDiagnostics),
}

impl<'a> Analysis<'a> {
    /// The findings for `source` under `settings`.
    ///
    /// # Errors
    /// Cancellation around a fresh pass, or `parseError` when `settings` are
    /// not a valid analysis configuration.
    pub fn new(source: &'a Source, settings: &'a AnalysisSettings, cancel: &Cancel) -> Result<Self, RequestError> {
        let config = settings.diagnostics_config();
        let cached = if let Source::Cached(result) = source {
            result.model().filter(|model| model.config == config).map(|model| &model.diagnostics)
        } else {
            None
        };
        let findings = if let Some(diagnostics) = cached {
            Findings::Cached(diagnostics)
        } else {
            cancel.check()?;
            let fresh = source.problem().diagnostics_with_config(&config).map_err(|e| RequestError::parse(&e))?;
            cancel.check()?;
            Findings::Fresh(fresh)
        };
        Ok(Self { findings, settings })
    }

    /// Published findings: none when analysis is disabled.
    pub fn iter(&self) -> impl Iterator<Item = &ModelDiagnostic> {
        let all: &[ModelDiagnostic] = match &self.findings {
            Findings::Cached(d) => &d.diagnostics,
            Findings::Fresh(d) => &d.diagnostics,
        };
        let settings = self.settings;
        all.iter().filter(move |d| settings.enabled && settings.publishes(d.code))
    }

    /// Published findings of one severity, after `severityOverrides`.
    #[must_use]
    pub fn count(&self, severity: IssueSeverity) -> usize {
        self.iter().filter(|d| self.severity(d) == severity).count()
    }

    fn severity(&self, finding: &ModelDiagnostic) -> IssueSeverity {
        match self.settings.severity_override(finding.code) {
            Some(SeverityOverride::Error) => IssueSeverity::Error,
            Some(SeverityOverride::Warning) => IssueSeverity::Warning,
            Some(SeverityOverride::Information | SeverityOverride::Hint) => IssueSeverity::Info,
            Some(SeverityOverride::Off) | None => finding.severity,
        }
    }

    /// Tolerance the analysis treats integer bounds with.
    #[must_use]
    pub fn equality_tolerance(&self) -> f64 {
        self.settings.equality_tolerance
    }
}

/// `Some(x)` for finite `x`, else `None` (serialised as `null`).
#[must_use]
pub fn finite(x: f64) -> Option<f64> {
    x.is_finite().then_some(x)
}

/// Wire name of a variable kind.
#[must_use]
pub const fn variable_kind(kind: VariableKind) -> &'static str {
    match kind {
        VariableKind::Continuous => "continuous",
        VariableKind::Integer => "integer",
        VariableKind::General => "general",
        VariableKind::Binary => "binary",
        VariableKind::SemiContinuous => "semiContinuous",
        VariableKind::SemiInteger => "semiInteger",
        VariableKind::Sos => "sos",
    }
}

/// Wire name of a constraint class.
#[must_use]
pub const fn constraint_class(class: ConstraintClass) -> &'static str {
    match class {
        ConstraintClass::Normal => "normal",
        ConstraintClass::Lazy => "lazy",
        ConstraintClass::UserCut => "userCut",
    }
}

/// Wire name of a comparison operator.
#[must_use]
pub const fn operator(op: ComparisonOp) -> &'static str {
    match op {
        ComparisonOp::LT => "<",
        ComparisonOp::LTE => "<=",
        ComparisonOp::EQ => "=",
        ComparisonOp::GTE => ">=",
        ComparisonOp::GT => ">",
    }
}

/// Validate a client-supplied page size: at least 1 and at most `max`.
///
/// # Errors
/// `-32602` when out of range.
pub fn check_limit(name: &str, value: usize, max: usize) -> Result<usize, RequestError> {
    if value == 0 || value > max {
        return Err(RequestError::invalid(format!("{name} must be between 1 and {max}, got {value}")));
    }
    Ok(value)
}

#[cfg(test)]
pub(crate) mod testing {
    //! Shared fixtures for the handler tests.

    use super::*;
    use crate::position::Encoding;

    /// A document at version 1.
    pub fn doc(text: &str) -> Document {
        Document::new("file:///t.lp".parse().expect("valid uri"), text.to_owned(), 1, Encoding::Utf16)
    }

    pub fn target() -> Target {
        Target { text_document: TextDocumentIdentifier { uri: "file:///t.lp".parse().expect("valid uri") }, work_done_token: None }
    }

    /// Serialise a result for assertions on the wire shape.
    pub fn json<T: Serialize>(value: &T) -> Value {
        serde_json::to_value(value).expect("results serialise")
    }

    pub const MODEL: &str = "Minimize\n obj: 3 x + 2 y + z\nSubject To\n c1: x + y >= 2\n c2: 2 x + 4 y <= 100\n c3: x + y + z = 10\nBounds\n x <= 10\n 1 <= y <= 5\nGenerals\n z\nEnd\n";
}

#[cfg(test)]
mod tests {
    use super::testing::*;
    use super::*;

    #[test]
    fn cancel_ticks_only_every_poll_interval() {
        let cancel = Cancel::new();
        cancel.cancel();
        assert_eq!(cancel.tick(1), Ok(()), "between polls the flag is not read");
        assert_eq!(cancel.tick(POLL_EVERY), Err(RequestError::Cancelled));
        assert_eq!(cancel.check(), Err(RequestError::Cancelled));
    }

    #[test]
    fn progress_is_the_fraction_of_the_declared_total() {
        let cancel = Cancel::new();
        assert_eq!(cancel.percentage(), 0);
        cancel.set_total(4096);
        cancel.tick(2048).expect("not cancelled");
        assert_eq!(cancel.percentage(), 50);
    }

    #[test]
    fn dropping_the_guard_cancels() {
        let cancel = Cancel::new();
        drop(CancelOnDrop(cancel.clone()));
        assert!(cancel.is_cancelled());
    }

    #[test]
    fn failures_carry_kind_message_and_extras() {
        let error = RequestError::failed(FailureKind::SolverFailed, "boom").with("stderrTail", "oops");
        assert_eq!(error.data(), Some(json!({ "kind": "solverFailed", "message": "boom", "stderrTail": "oops" })));
        assert_eq!(RequestError::Cancelled.data(), None);
        assert_eq!(RequestError::invalid("x").with("ignored", 1).data(), None);
    }

    #[test]
    fn acquire_reports_parse_errors_and_prefers_the_current_semantic_pass() {
        let broken = doc("Minimize\n obj: 3 x +\nSubject To\n c1: >= \nEnd\n");
        assert!(matches!(Source::acquire(&broken), Err(RequestError::Failed(f)) if f.kind == FailureKind::ParseError));

        let mut current = doc(MODEL);
        assert!(matches!(Source::acquire(&current), Ok(Source::Fresh(_))));
        current.semantic_result = Some(Arc::new(crate::semantic::run(MODEL, 1, &lp_parser_rs::analysis::AnalysisConfig::default())));
        assert!(matches!(Source::acquire(&current), Ok(Source::Cached(_))));
        // A pass for an older version is not trusted.
        current.version = 2;
        assert!(matches!(Source::acquire(&current), Ok(Source::Fresh(_))));
    }

    #[test]
    fn limits_are_validated() {
        assert_eq!(check_limit("limit", 5, 10), Ok(5));
        assert!(check_limit("limit", 0, 10).is_err());
        assert!(check_limit("limit", 11, 10).is_err());
    }

    #[test]
    fn advertised_capabilities_list_every_request() {
        let caps = experimental_capabilities(true);
        assert_eq!(caps["lp"]["protocol"], 1);
        assert_eq!(caps["lp"]["solver"], true);
        assert_eq!(caps["lp"]["requests"].as_array().map(Vec::len), Some(REQUESTS.len()));
    }
}
