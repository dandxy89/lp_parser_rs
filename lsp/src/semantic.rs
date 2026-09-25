//! Semantic pass: a full `lp_parser_rs` parse plus analysis. Expensive, so the
//! server runs it debounced on a blocking task and discards stale results.

use lp_parser_rs::analysis::{AnalysisConfig, ProblemAnalysis};
use lp_parser_rs::{LpParseError, LpProblem, NameId};

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
    /// Objective and constraint names with a source offset, sorted by offset
    /// (ties in model order), so a range of the document is found by binary
    /// search instead of a scan of the whole model.
    pub name_sites: Vec<NameSite>,
}

/// Where the model names an objective or constraint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NameSite {
    /// Byte offset of the objective or constraint in the source.
    pub offset: usize,
    /// Position in model order: objectives, then constraints.
    pub order: usize,
    /// The model's name for it.
    pub name: NameId,
}

impl SemanticResult {
    /// The parsed model, if parsing succeeded.
    #[must_use]
    pub fn model(&self) -> Option<&Model> {
        self.outcome.as_ref().ok()
    }
}

impl Model {
    /// Name sites whose offset lies in `range` (inclusive).
    #[must_use]
    pub fn name_sites_in(&self, range: std::ops::RangeInclusive<usize>) -> &[NameSite] {
        let first = self.name_sites.partition_point(|s| s.offset < *range.start());
        let last = self.name_sites.partition_point(|s| s.offset <= *range.end());
        &self.name_sites[first..last.max(first)]
    }
}

/// Parse and analyse `text`.
#[must_use]
pub fn run(text: &str, version: i32, config: &AnalysisConfig) -> SemanticResult {
    let outcome = LpProblem::parse(text).map(|problem| {
        let analysis = problem.analyze_with_config(config);
        let name_sites = name_sites(&problem);
        Model { problem, analysis, name_sites }
    });
    SemanticResult { version, outcome }
}

fn name_sites(problem: &LpProblem) -> Vec<NameSite> {
    let objectives = problem.objectives.values().map(|o| (o.byte_offset, o.name));
    let constraints = problem.constraints.values().map(|c| (c.byte_offset(), c.name()));
    let mut sites: Vec<NameSite> = objectives
        .chain(constraints)
        .enumerate()
        .filter_map(|(order, (offset, name))| Some(NameSite { offset: offset?, order, name }))
        .collect();
    // Stable: equal offsets keep model order.
    sites.sort_by_key(|s| s.offset);
    debug_assert!(sites.windows(2).all(|w| (w[0].offset, w[0].order) < (w[1].offset, w[1].order)), "sites sorted by offset, then order");
    sites
}
