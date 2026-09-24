//! Inlay hints: generated names, `_rng` partners, variable types, normalised RHS.

use tower_lsp_server::ls_types::{InlayHint, Range};

use crate::config::InlayHintSettings;
use crate::document::Document;

/// Hints within `range`.
#[must_use]
pub const fn hints(doc: &Document, range: Range, settings: &InlayHintSettings) -> Vec<InlayHint> {
    let _ = (doc, range, settings);
    Vec::new()
}
