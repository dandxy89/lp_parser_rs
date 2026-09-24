//! Go to definition / declaration / type definition, references and document
//! highlights.

use tower_lsp_server::ls_types::{DocumentHighlight, Location, Position};

use crate::document::Document;

/// Variable: its `Bounds` entry, else first occurrence. Names: the label.
#[must_use]
pub const fn definition(doc: &Document, position: Position) -> Option<Location> {
    let _ = (doc, position);
    None
}

/// Variable: its type-section entry. Names: the label.
#[must_use]
pub const fn declaration(doc: &Document, position: Position) -> Option<Location> {
    let _ = (doc, position);
    None
}

/// Variable: its type-section entry, else its bound.
#[must_use]
pub const fn type_definition(doc: &Document, position: Position) -> Option<Location> {
    let _ = (doc, position);
    None
}

/// Every occurrence of the symbol at `position`.
#[must_use]
pub const fn references(doc: &Document, position: Position, include_declaration: bool) -> Vec<Location> {
    let _ = (doc, position, include_declaration);
    Vec::new()
}

/// Write for the definition, read for everything else.
#[must_use]
pub const fn highlights(doc: &Document, position: Position) -> Vec<DocumentHighlight> {
    let _ = (doc, position);
    Vec::new()
}
