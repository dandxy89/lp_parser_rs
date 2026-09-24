//! Diagnostics: syntax (tree-sitter ERROR/MISSING), index-based checks, semantic
//! (`lp_parser_rs` errors, only without syntax errors) and analysis issues.

use tower_lsp_server::ls_types::Diagnostic;

use crate::config::Config;
use crate::document::Document;

/// Diagnostic `code`s. Code actions match on these.
pub mod codes {
    /// tree-sitter `ERROR` node.
    pub const SYNTAX_ERROR: &str = "syntax-error";
    /// tree-sitter `MISSING` node.
    pub const MISSING_TOKEN: &str = "missing-token";
    /// `lp_parser_rs` parse/assembly error.
    pub const PARSE_ERROR: &str = "parse-error";
    /// Duplicate constraint/objective/SOS name.
    pub const DUPLICATE_NAME: &str = "duplicate-name";
    /// Variable in more than one type section.
    pub const CONFLICTING_TYPE: &str = "conflicting-type";
    /// Bound or type declaration for a variable used nowhere.
    pub const UNUSED_DECLARATION: &str = "unused-declaration";
    /// Lower bound above upper bound.
    pub const CONFLICTING_BOUNDS: &str = "conflicting-bounds";
    /// `=<` / `=>` spelling.
    pub const OPERATOR_SPELLING: &str = "operator-spelling";
    /// Prefix for analysis issues: `analysis/<category>`.
    pub const ANALYSIS_PREFIX: &str = "analysis/";
}

/// All diagnostics for `doc` under `config`.
#[must_use]
pub const fn compute(doc: &Document, config: &Config) -> Vec<Diagnostic> {
    let _ = (doc, config);
    Vec::new()
}
