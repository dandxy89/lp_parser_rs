//! Context-aware completion.

use tower_lsp_server::ls_types::{CompletionItem, Position};

use crate::document::Document;

/// Completion items at `position`.
#[must_use]
pub const fn complete(doc: &Document, position: Position) -> Vec<CompletionItem> {
    let _ = (doc, position);
    Vec::new()
}

/// Fill in documentation for a completion item.
#[must_use]
pub const fn resolve(item: CompletionItem) -> CompletionItem {
    item
}
