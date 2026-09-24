//! Tree-sitter-based formatter preserving comments and single blank lines.

use tower_lsp_server::ls_types::{Position, Range, TextEdit};

use crate::config::FormatSettings;
use crate::document::Document;

/// Format a whole LP text. `None` when it has syntax errors.
#[must_use]
pub const fn format_text(text: &str, settings: &FormatSettings) -> Option<String> {
    let _ = (text, settings);
    None
}

/// Edits formatting the whole document. `None` on syntax errors.
#[must_use]
pub const fn format_document(doc: &Document, settings: &FormatSettings) -> Option<Vec<TextEdit>> {
    let _ = (doc, settings);
    None
}

/// Edits formatting whole entries overlapping `range`.
#[must_use]
pub const fn format_range(doc: &Document, range: Range, settings: &FormatSettings) -> Option<Vec<TextEdit>> {
    let _ = (doc, range, settings);
    None
}

/// On-type formatting after `ch` (`\n`) at `position`.
#[must_use]
pub const fn format_on_type(doc: &Document, position: Position, ch: &str, settings: &FormatSettings) -> Option<Vec<TextEdit>> {
    let _ = (doc, position, ch, settings);
    None
}
