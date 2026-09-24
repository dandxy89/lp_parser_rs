//! Selection ranges from the tree-sitter ancestor chain.

use tower_lsp_server::ls_types::{Position, SelectionRange};

use crate::document::Document;

/// One selection-range chain per position.
#[must_use]
pub const fn ranges(doc: &Document, positions: &[Position]) -> Vec<SelectionRange> {
    let _ = (doc, positions);
    Vec::new()
}
