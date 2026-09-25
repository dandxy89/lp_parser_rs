//! LSP features. Each module is a set of pure functions over a [`Document`]
//! (and, where needed, the workspace), so it can be tested without a client.
//!
//! [`Document`]: crate::document::Document

pub mod code_action;
pub mod code_lens;
pub mod commands;
pub mod completion;
pub mod diagnostics;
mod docs;
pub mod folding;
pub mod format;
pub mod hierarchy;
pub mod hover;
pub mod inlay;
pub mod navigation;
pub mod rename;
pub mod selection;
pub mod semantic_tokens;
pub mod signature;
pub mod symbols;
