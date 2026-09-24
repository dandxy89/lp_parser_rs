//! Language server for LP (linear programming) files.
//!
//! Syntax comes from the tree-sitter-lp grammar (incremental, error tolerant);
//! semantics from a debounced full `lp_parser_rs` parse and analysis.

// Settings and capability flags are naturally many independent booleans, and
// `LanguageServer` handlers must be `async` even when they never await.
#![allow(clippy::struct_excessive_bools, clippy::unused_async, clippy::unused_async_trait_impl)]

pub mod config;
pub mod document;
pub mod features;
pub mod index;
pub mod position;
pub mod semantic;
pub mod server;
pub mod syntax;
pub mod workspace;

pub use config::Config;
pub use document::Document;
pub use index::SymbolIndex;
pub use position::{Encoding, LineIndex};
pub use server::Backend;
