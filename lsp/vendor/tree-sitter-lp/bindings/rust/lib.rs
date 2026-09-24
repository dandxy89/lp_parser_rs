//! LP (Linear Programming) file support for the [tree-sitter] parsing library.
//!
//! Pass [`LANGUAGE`] to a tree-sitter [`Parser`], then parse LP source:
//!
//! ```
//! let code = r#"
//! min
//!   x
//! subject to
//!   x >= 0
//! end
//! "#;
//! let mut parser = tree_sitter::Parser::new();
//! let language = tree_sitter_lp::LANGUAGE;
//! parser
//!     .set_language(&language.into())
//!     .expect("Error loading LP parser");
//! let tree = parser.parse(code, None).unwrap();
//! assert!(!tree.root_node().has_error());
//! ```
//!
//! [`Parser`]: https://docs.rs/tree-sitter/latest/tree_sitter/struct.Parser.html
//! [tree-sitter]: https://tree-sitter.github.io/

use tree_sitter_language::LanguageFn;

extern "C" {
    fn tree_sitter_lp() -> *const ();
}

/// The tree-sitter [`LanguageFn`] for this grammar.
///
/// Convert it with `.into()` to get a `tree_sitter::Language`.
pub const LANGUAGE: LanguageFn = unsafe { LanguageFn::from_raw(tree_sitter_lp) };

/// The content of the [`node-types.json`] file for this grammar.
///
/// [`node-types.json`]: https://tree-sitter.github.io/tree-sitter/using-parsers/6-static-node-types
pub const NODE_TYPES: &str = include_str!("../../src/node-types.json");

/// The syntax highlighting query for this grammar (`queries/highlights.scm`).
pub const HIGHLIGHTS_QUERY: &str = include_str!("../../queries/highlights.scm");

/// The local variable query for this grammar (`queries/locals.scm`).
pub const LOCALS_QUERY: &str = include_str!("../../queries/locals.scm");

#[cfg(test)]
mod tests {
    #[test]
    fn test_can_load_grammar() {
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(&super::LANGUAGE.into()).expect("Error loading LP parser");
    }
}
