//! `workspace/executeCommand` handlers.

use crate::config::Config;
use crate::document::Document;

/// Run the full analysis and return a markdown report.
pub const ANALYZE: &str = "lp.analyze";
/// Write an MPS file next to the source.
pub const CONVERT_TO_MPS: &str = "lp.convertToMps";
/// Summarise variables, constraints, nonzeros and types.
pub const SHOW_MODEL_STATS: &str = "lp.showModelStats";

/// Every server-side command.
pub const ALL: &[&str] = &[ANALYZE, CONVERT_TO_MPS, SHOW_MODEL_STATS];

/// Result of a command: a JSON value for the caller and a message to show.
#[derive(Debug, Clone, PartialEq)]
pub struct Output {
    /// Returned to the client.
    pub value: serde_json::Value,
    /// Shown via `window/showMessage`.
    pub message: String,
}

/// Execute `command` against `doc` (the document named by the first argument).
///
/// # Errors
/// A user-facing message for unknown commands or failures.
pub fn execute(command: &str, doc: &Document, config: &Config) -> Result<Output, String> {
    let _ = (doc, config);
    Err(format!("unknown command '{command}'"))
}
