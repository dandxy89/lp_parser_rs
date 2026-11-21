//! The `lp-highs` helper protocol (version 1).
//!
//! `lp-highs solve` reads one [`SolveRequest`] as JSON on stdin and writes one
//! [`SolveResponse`] as JSON on stdout; progress is streamed on stderr as
//! newline-delimited [`Event`]s. These types are always built, with or without
//! the `highs` feature, so a client (the language server) shares the wire
//! format without linking a solver.

use std::fmt;

use serde::{Deserialize, Serialize};

/// The only protocol version this crate speaks.
pub const PROTOCOL_VERSION: u32 = 1;

/// Largest request the helper reads from stdin (512 MiB).
pub const MAX_REQUEST_BYTES: usize = 512 * 1024 * 1024;

/// Largest solver log tail a response carries (1 MiB).
pub const MAX_LOG_BYTES: usize = 1024 * 1024;

/// A request was malformed or spoke another protocol version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProtocolViolation(pub String);

impl fmt::Display for ProtocolViolation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ProtocolViolation {}

/// Source format of [`SolveRequest::text`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Format {
    Lp,
    Mps,
}

/// Whether to diagnose an infeasible model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Diagnose {
    #[default]
    Auto,
    Never,
}

/// `HiGHS`'s `presolve` option.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Presolve {
    On,
    Off,
    Choose,
}

impl Presolve {
    /// The value `HiGHS` expects for its `presolve` option.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::On => "on",
            Self::Off => "off",
            Self::Choose => "choose",
        }
    }
}

/// Solver options a client may set.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct SolveOptions {
    /// Wall-clock limit in seconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time_limit: Option<f64>,
    /// Relative MIP gap at which to stop.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mip_rel_gap: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub presolve: Option<Presolve>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub threads: Option<u32>,
}

/// The one request `lp-highs solve` reads.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SolveRequest {
    pub protocol: u32,
    pub format: Format,
    pub text: String,
    #[serde(default)]
    pub options: SolveOptions,
    #[serde(default)]
    pub diagnose: Diagnose,
    #[serde(default)]
    pub include_log: bool,
}

impl SolveRequest {
    /// A version-1 request with default options.
    #[must_use]
    pub fn new(format: Format, text: impl Into<String>) -> Self {
        Self {
            protocol: PROTOCOL_VERSION,
            format,
            text: text.into(),
            options: SolveOptions::default(),
            diagnose: Diagnose::Auto,
            include_log: false,
        }
    }

    /// Check the protocol version and option ranges.
    ///
    /// # Errors
    ///
    /// Returns a [`ProtocolViolation`] when `protocol` is not
    /// [`PROTOCOL_VERSION`], or a numeric option is negative, NaN or infinite.
    pub fn validate(&self) -> Result<(), ProtocolViolation> {
        if self.protocol != PROTOCOL_VERSION {
            return Err(ProtocolViolation(format!("unsupported protocol {}, this helper speaks {PROTOCOL_VERSION}", self.protocol)));
        }
        for (name, value) in [("timeLimit", self.options.time_limit), ("mipRelGap", self.options.mip_rel_gap)] {
            if let Some(value) = value
                && !(value.is_finite() && value >= 0.0)
            {
                return Err(ProtocolViolation(format!("option {name} must be a non-negative finite number, got {value}")));
            }
        }
        Ok(())
    }

    /// Parse and validate a request from JSON.
    ///
    /// # Errors
    ///
    /// Returns a [`ProtocolViolation`] for malformed JSON or a failed [`Self::validate`].
    pub fn from_json(json: &str) -> Result<Self, ProtocolViolation> {
        let request: Self = serde_json::from_str(json).map_err(|error| ProtocolViolation(format!("malformed request: {error}")))?;
        request.validate()?;
        Ok(request)
    }
}

/// Answer to `lp-highs info`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HelperInfo {
    pub name: String,
    pub version: String,
    pub protocol: u32,
    pub highs: String,
}

/// Which solver produced a result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SolverId {
    pub name: String,
    pub version: String,
    /// `lp-highs <version>`.
    pub helper: String,
}

/// Stage timings in milliseconds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Timings {
    pub build_ms: u64,
    pub solve_ms: u64,
    pub extract_ms: u64,
}

/// A variable's value in the solution.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VariableSolution {
    pub name: String,
    pub value: f64,
    /// Omitted for models with integer, semi or SOS columns.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reduced_cost: Option<f64>,
}

/// A constraint's row in the solution.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConstraintSolution {
    pub name: String,
    pub activity: f64,
    /// Omitted for models with integer, semi or SOS columns.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dual: Option<f64>,
    pub slack: f64,
    pub binding: bool,
}

/// Counts of constructs the solver could not model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Skipped {
    pub sos: usize,
}

/// How an infeasibility was diagnosed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DiagnosisMethod {
    /// An irreducible infeasible subsystem.
    Iis,
    /// The cheapest set of rows to relax (elastic relaxation).
    Elastic,
}

/// Which side of a row or bound is implicated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Side {
    Lower,
    Upper,
    Boxed,
    Free,
}

/// A constraint implicated in an infeasibility.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InfeasibleConstraint {
    pub name: String,
    pub side: Side,
    /// Only for the elastic method.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub violation: Option<f64>,
}

/// A variable bound implicated in an infeasibility.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InfeasibleBound {
    pub variable: String,
    pub side: Side,
    /// Only for conflicting declared bounds (`lower - upper`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gap: Option<f64>,
}

/// Why a model is infeasible.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Infeasibility {
    pub method: DiagnosisMethod,
    pub constraints: Vec<InfeasibleConstraint>,
    pub bounds: Vec<InfeasibleBound>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_violation: Option<f64>,
}

/// A successful solve, as the helper reports it (the language server adds
/// `uri`, `version` and source ranges).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SolveOutcome {
    pub solver: SolverId,
    /// Debug name of `highs::HighsModelStatus`, e.g. `Optimal`.
    pub status: String,
    pub objective_value: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mip_gap: Option<f64>,
    pub timings: Timings,
    pub variables: Vec<VariableSolution>,
    pub constraints: Vec<ConstraintSolution>,
    pub skipped: Skipped,
    pub infeasibility: Option<Infeasibility>,
    pub log: Option<String>,
}

/// Failure classes of a response.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ErrorKind {
    ParseError,
    UnsupportedModel,
    SolverFailed,
    Protocol,
}

/// The error half of a response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProtocolError {
    pub kind: ErrorKind,
    pub message: String,
}

/// The one response `lp-highs solve` writes: `result` when `ok`, `error` otherwise.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SolveResponse {
    pub protocol: u32,
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<SolveOutcome>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<ProtocolError>,
}

impl SolveResponse {
    /// A successful response.
    #[must_use]
    pub const fn success(result: SolveOutcome) -> Self {
        Self { protocol: PROTOCOL_VERSION, ok: true, result: Some(result), error: None }
    }

    /// A failed response.
    #[must_use]
    pub fn failure(kind: ErrorKind, message: impl Into<String>) -> Self {
        Self { protocol: PROTOCOL_VERSION, ok: false, result: None, error: Some(ProtocolError { kind, message: message.into() }) }
    }

    /// Check the version and that exactly the half matching `ok` is present.
    ///
    /// # Errors
    ///
    /// Returns a [`ProtocolViolation`] on a version mismatch or an inconsistent body.
    pub fn validate(&self) -> Result<(), ProtocolViolation> {
        if self.protocol != PROTOCOL_VERSION {
            return Err(ProtocolViolation(format!("unsupported protocol {}, expected {PROTOCOL_VERSION}", self.protocol)));
        }
        match (self.ok, &self.result, &self.error) {
            (true, Some(_), None) | (false, None, Some(_)) => Ok(()),
            _ => Err(ProtocolViolation("response body does not match its `ok` flag".to_owned())),
        }
    }

    /// Parse and validate a response from JSON.
    ///
    /// # Errors
    ///
    /// Returns a [`ProtocolViolation`] for malformed JSON or a failed [`Self::validate`].
    pub fn from_json(json: &str) -> Result<Self, ProtocolViolation> {
        let response: Self = serde_json::from_str(json).map_err(|error| ProtocolViolation(format!("malformed response: {error}")))?;
        response.validate()?;
        Ok(response)
    }
}

/// A stage of a solve request, in the order they occur.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Phase {
    Parsing,
    Building,
    Solving,
    Extracting,
    Diagnosing,
}

/// A progress line on stderr.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Event {
    /// A phase began, `elapsed_ms` after the request was read.
    Phase { phase: Phase, elapsed_ms: u64 },
}

impl Event {
    /// The NDJSON line for this event (no trailing newline).
    ///
    /// # Panics
    ///
    /// Never in practice: the event contains only plain data.
    #[must_use]
    pub fn to_line(&self) -> String {
        serde_json::to_string(self).expect("an Event always serialises")
    }

    /// Parse a stderr line; `None` when it is not an event (it is then
    /// ordinary stderr text, kept for the client's `stderrTail`).
    #[must_use]
    pub fn from_line(line: &str) -> Option<Self> {
        serde_json::from_str(line.trim()).ok()
    }
}
