//! Markdown hover for variables, constraints, objectives, keywords and functions.

use tower_lsp_server::ls_types::{Hover, Position};

use crate::document::Document;

/// Hover at `position`.
#[must_use]
pub const fn hover(doc: &Document, position: Position) -> Option<Hover> {
    let _ = (doc, position);
    None
}
