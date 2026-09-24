//! Signature help for `MAX`/`MIN`/`ABS`/`AND`/`OR`.

use tower_lsp_server::ls_types::{Position, SignatureHelp};

use crate::document::Document;

/// Signature help at `position`.
#[must_use]
pub const fn help(doc: &Document, position: Position) -> Option<SignatureHelp> {
    let _ = (doc, position);
    None
}
