//! Code lenses: variable counts on labels, usage counts on variable definitions.

use tower_lsp_server::ls_types::CodeLens;

use crate::document::Document;

/// Client-side command the usage lens runs: arguments are the document URI,
/// the position and the reference locations.
pub const SHOW_REFERENCES: &str = "lp.showReferences";

/// Lenses for `doc`.
#[must_use]
pub const fn lenses(doc: &Document) -> Vec<CodeLens> {
    let _ = doc;
    Vec::new()
}
