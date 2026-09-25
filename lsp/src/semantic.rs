//! Semantic pass: a full `lp_parser_rs` parse plus analysis. Expensive, so the
//! server runs it debounced on a blocking task and discards stale results.

use lp_parser_rs::analysis::{AnalysisConfig, ProblemAnalysis};
use lp_parser_rs::{LpParseError, LpProblem};

/// Outcome of the semantic pass for one document version.
#[derive(Debug, Clone)]
pub struct SemanticResult {
    /// Document version the pass ran on.
    pub version: i32,
    /// Parsed model and analysis, or the parse error.
    pub outcome: Result<Model, LpParseError>,
}

/// A successfully parsed model.
#[derive(Debug, Clone)]
pub struct Model {
    /// The upstream model.
    pub problem: LpProblem,
    /// Its analysis.
    pub analysis: ProblemAnalysis,
}

impl SemanticResult {
    /// The parsed model, if parsing succeeded.
    #[must_use]
    pub fn model(&self) -> Option<&Model> {
        self.outcome.as_ref().ok()
    }
}

/// Parse and analyse `text`.
#[must_use]
pub fn run(text: &str, version: i32, config: &AnalysisConfig) -> SemanticResult {
    let outcome = LpProblem::parse(text).map(|problem| {
        let analysis = problem.analyze_with_config(config);
        Model { problem, analysis }
    });
    SemanticResult { version, outcome }
}
