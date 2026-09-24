//! Rename and prepareRename for variables and constraint/objective/SOS names.

use std::sync::Arc;

use tower_lsp_server::ls_types::{Position, PrepareRenameResponse, WorkspaceEdit};

use crate::document::Document;

/// Range and placeholder of the renameable symbol at `position`.
///
/// # Errors
/// A user-facing message when the position is not renameable.
pub const fn prepare(doc: &Document, position: Position) -> Result<Option<PrepareRenameResponse>, String> {
    let _ = (doc, position);
    Ok(None)
}

/// Rename the symbol at `position` to `new_name`. `workspace` holds the other
/// indexed documents (for names shared across files).
///
/// # Errors
/// A user-facing message when the new name is invalid or collides.
pub const fn rename(doc: &Document, workspace: &[Arc<Document>], position: Position, new_name: &str) -> Result<Option<WorkspaceEdit>, String> {
    let _ = (doc, workspace, position, new_name);
    Ok(None)
}
