use std::collections::HashMap;
use std::path::Path;

use lp_parser_rs::analysis::ProblemAnalysis;
use lp_parser_rs::interner::NameId;
use lp_parser_rs::parser::MappedFile;
use lp_parser_rs::problem::LpProblem;

use crate::line_index::LineIndex;

/// Parsed LP file: the problem, structural analysis, and a constraint→line-number map.
pub type ParsedLpFile = (LpProblem, ProblemAnalysis, HashMap<NameId, usize>);

/// Build a map from constraint name to 1-based line number using byte offsets
/// captured during parsing and a `LineIndex` built from the source text.
fn build_constraint_line_map(problem: &LpProblem, line_index: &LineIndex) -> HashMap<NameId, usize> {
    let mut map = HashMap::new();
    for (name_id, constraint) in &problem.constraints {
        if let Some(offset) = constraint.byte_offset()
            && let Some(line) = line_index.line_number(offset)
        {
            map.insert(*name_id, line);
        }
    }
    map
}

/// Parse an LP file, returning the problem, analysis, and a constraint→line-number map.
///
/// Uses memory-mapped I/O to avoid copying the file into a heap-allocated `String`.
pub fn parse_lp_file(path: &Path) -> Result<ParsedLpFile, Box<dyn std::error::Error + Send + Sync>> {
    let mapped = MappedFile::open(path).map_err(|e| format!("failed to read '{}': {e}", path.display()))?;
    let content = mapped.as_str();
    let problem = LpProblem::parse(content).map_err(|e| format!("failed to parse '{}': {e}", path.display()))?;
    let line_index = LineIndex::new(content);
    let line_map = build_constraint_line_map(&problem, &line_index);
    let analysis = problem.analyze();
    Ok((problem, analysis, line_map))
}
