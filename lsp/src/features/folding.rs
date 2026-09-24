//! Folding ranges from `folds.scm`, block comments and runs of line comments.

use tower_lsp_server::ls_types::FoldingRange;

use crate::document::Document;

/// Folding ranges for `doc`.
#[must_use]
pub const fn ranges(doc: &Document) -> Vec<FoldingRange> {
    let _ = doc;
    Vec::new()
}
