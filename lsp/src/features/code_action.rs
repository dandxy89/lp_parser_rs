//! Quick fixes, refactors and the organise-sections source action.

use tower_lsp_server::ls_types::{CodeActionContext, CodeActionOrCommand, Range};

use crate::config::Config;
use crate::document::Document;

/// Source action kind for reordering sections canonically.
pub const ORGANIZE_SECTIONS: &str = "source.organizeSections";

/// Code actions for `range`.
#[must_use]
pub const fn actions(doc: &Document, range: Range, context: &CodeActionContext, config: &Config) -> Vec<CodeActionOrCommand> {
    let _ = (doc, range, context, config);
    Vec::new()
}
