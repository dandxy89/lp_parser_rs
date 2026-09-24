//! Document symbols (hierarchical) and workspace symbols (fuzzy).

use std::sync::Arc;

use tower_lsp_server::ls_types::{DocumentSymbol, WorkspaceSymbol};

use crate::document::Document;

/// Sections → objectives / constraints / general constraints / SOS sets.
#[must_use]
pub const fn document_symbols(doc: &Document) -> Vec<DocumentSymbol> {
    let _ = doc;
    Vec::new()
}

/// Fuzzy search over constraint, objective, SOS and variable names.
#[must_use]
pub const fn workspace_symbols(docs: &[Arc<Document>], query: &str) -> Vec<WorkspaceSymbol> {
    let _ = (docs, query);
    Vec::new()
}
