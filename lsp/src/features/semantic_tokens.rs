//! Semantic tokens (full, range, delta) from `HIGHLIGHTS_QUERY`.

use std::ops::Range;

use tower_lsp_server::ls_types::{SemanticToken, SemanticTokensEdit, SemanticTokensLegend};

use crate::document::Document;

/// Token legend declared in `initialize`.
#[must_use]
pub fn legend() -> SemanticTokensLegend {
    SemanticTokensLegend::default()
}

/// Encoded tokens for the whole document, or only those within `range`.
#[must_use]
pub const fn tokens(doc: &Document, range: Option<Range<usize>>) -> Vec<SemanticToken> {
    let _ = (doc, range);
    Vec::new()
}

/// Edits turning `old` into `new`.
#[must_use]
pub const fn delta(old: &[SemanticToken], new: &[SemanticToken]) -> Vec<SemanticTokensEdit> {
    let _ = (old, new);
    Vec::new()
}
