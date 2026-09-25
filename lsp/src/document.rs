//! Open-document store entry: text, syntax tree, line index and symbol index,
//! kept in sync under incremental edits.

use std::ops::Range;
use std::sync::{Arc, OnceLock};

use tower_lsp_server::ls_types::{self as lsp, TextDocumentContentChangeEvent, Uri};
use tree_sitter::{InputEdit, Node, Tree};

use crate::index::SymbolIndex;
use crate::position::{Encoding, LineIndex};
use crate::semantic::SemanticResult;
use crate::syntax;

/// One document's state.
#[derive(Debug, Clone)]
pub struct Document {
    /// Document URI.
    pub uri: Uri,
    /// Full text.
    pub text: String,
    /// Client version (0 for files indexed from disk).
    pub version: i32,
    /// Syntax tree for `text`.
    pub tree: Tree,
    /// Line starts for `text`.
    pub lines: LineIndex,
    /// Symbol index for `tree`, built on first use per version (see [`Document::index`]).
    index: OnceLock<Arc<SymbolIndex>>,
    /// Position encoding of the session.
    pub encoding: Encoding,
    /// Latest semantic pass result, if any. May be for an older version;
    /// check [`Document::semantic`].
    pub semantic_result: Option<Arc<SemanticResult>>,
}

impl Document {
    /// Parse and index a new document.
    #[must_use]
    pub fn new(uri: Uri, text: String, version: i32, encoding: Encoding) -> Self {
        let tree = syntax::parse(&text, None);
        let lines = LineIndex::new(&text);
        Self { uri, text, version, tree, lines, index: OnceLock::new(), encoding, semantic_result: None }
    }

    /// Apply LSP content changes in order, incrementally reparse and reindex.
    pub fn apply_changes(&mut self, changes: &[TextDocumentContentChangeEvent], version: i32) {
        let mut incremental = true;
        for change in changes {
            if let Some(range) = change.range {
                let bytes = self.lines.byte_range(&self.text, range, self.encoding);
                self.edit(bytes, &change.text);
            } else {
                self.text.clone_from(&change.text);
                self.lines = LineIndex::new(&self.text);
                incremental = false;
            }
        }
        self.tree = syntax::parse(&self.text, incremental.then_some(&self.tree));
        // Typing only pays for the reparse; the index is rebuilt when a feature needs it.
        self.index = OnceLock::new();
        self.version = version;
    }

    /// Replace `range` with `new_text`, editing the tree to match. The caller
    /// reparses afterwards.
    pub fn edit(&mut self, range: Range<usize>, new_text: &str) {
        debug_assert!(range.start <= range.end && range.end <= self.text.len(), "edit range out of bounds");
        debug_assert!(self.text.is_char_boundary(range.start) && self.text.is_char_boundary(range.end));
        let start_position = self.lines.point(range.start);
        let old_end_position = self.lines.point(range.end);
        self.text.replace_range(range.clone(), new_text);
        self.lines.edit(&self.text, range.clone(), new_text.len());
        let new_end_byte = range.start + new_text.len();
        self.tree.edit(&InputEdit {
            start_byte: range.start,
            old_end_byte: range.end,
            new_end_byte,
            start_position,
            old_end_position,
            new_end_position: self.lines.point(new_end_byte),
        });
    }

    /// Symbol index for the current tree, built on first use. Clones of the
    /// document share it.
    #[must_use]
    pub fn index(&self) -> &SymbolIndex {
        self.index.get_or_init(|| Arc::new(SymbolIndex::build(&self.tree, &self.text)))
    }

    /// Build the index now rather than on first use (for background loading).
    pub fn build_index(&self) {
        self.index.get_or_init(|| Arc::new(SymbolIndex::build(&self.tree, &self.text)));
    }

    /// Semantic result for the current version only.
    #[must_use]
    pub fn semantic(&self) -> Option<&SemanticResult> {
        self.semantic_result.as_deref().filter(|r| r.version == self.version)
    }

    /// Byte offset of an LSP position.
    #[must_use]
    pub fn offset(&self, position: lsp::Position) -> usize {
        self.lines.offset(&self.text, position, self.encoding)
    }

    /// LSP position of a byte offset.
    #[must_use]
    pub fn position(&self, offset: usize) -> lsp::Position {
        self.lines.position(&self.text, offset, self.encoding)
    }

    /// LSP range of a byte range.
    #[must_use]
    pub fn range(&self, range: Range<usize>) -> lsp::Range {
        self.lines.range(&self.text, range, self.encoding)
    }

    /// Byte range of an LSP range.
    #[must_use]
    pub fn byte_range(&self, range: lsp::Range) -> Range<usize> {
        self.lines.byte_range(&self.text, range, self.encoding)
    }

    /// LSP location of a byte range in this document.
    #[must_use]
    pub fn location(&self, range: Range<usize>) -> lsp::Location {
        lsp::Location::new(self.uri.clone(), self.range(range))
    }

    /// LSP range covering the whole document.
    #[must_use]
    pub fn full_range(&self) -> lsp::Range {
        self.range(0..self.text.len())
    }

    /// Source text of a byte range.
    #[must_use]
    pub fn slice(&self, range: Range<usize>) -> &str {
        &self.text[range]
    }

    /// Source text of a node.
    #[must_use]
    pub fn node_text(&self, node: Node<'_>) -> &str {
        syntax::text(node, &self.text)
    }

    /// The node of `kind` spanning exactly `range`, if any.
    #[must_use]
    pub fn node(&self, range: Range<usize>, kind: &str) -> Option<Node<'_>> {
        let mut node = self.tree.root_node().descendant_for_byte_range(range.start, range.end)?;
        loop {
            if node.kind() == kind && node.byte_range() == range {
                return Some(node);
            }
            if node.start_byte() < range.start || node.end_byte() > range.end {
                return None;
            }
            node = node.parent()?;
        }
    }

    /// Whether the tree contains `ERROR` or `MISSING` nodes.
    #[must_use]
    pub fn has_syntax_errors(&self) -> bool {
        self.tree.root_node().has_error()
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    const BASE: &str = "Minimize\n obj: 3 x + 2 y\nSubject To\n c1: x + y >= 1\n c2: x - y <= 4\nBounds\n x <= 10\nGenerals\n y\nEnd\n";

    fn uri() -> Uri {
        "file:///test.lp".parse().unwrap()
    }

    fn change(doc: &Document, range: Range<usize>, text: &str) -> TextDocumentContentChangeEvent {
        TextDocumentContentChangeEvent { range: Some(doc.range(range)), range_length: None, text: text.to_owned() }
    }

    #[test]
    fn incremental_edit_updates_index() {
        let mut doc = Document::new(uri(), BASE.to_owned(), 1, Encoding::Utf16);
        let at = BASE.find("c2").unwrap();
        let edit = change(&doc, at..at + 2, "limit");
        doc.apply_changes(&[edit], 2);
        assert_eq!(doc.version, 2);
        assert!(doc.text.contains("limit: x - y"));
        assert!(doc.index().entities.iter().any(|e| e.name.as_deref() == Some("limit")));
        assert_eq!(doc.tree.root_node().to_sexp(), syntax::parse(&doc.text, None).root_node().to_sexp());
    }

    #[test]
    fn full_replacement_resets() {
        let mut doc = Document::new(uri(), BASE.to_owned(), 1, Encoding::Utf8);
        doc.apply_changes(
            &[TextDocumentContentChangeEvent { range: None, range_length: None, text: "min\n z\nst\n z >= 1\nend\n".into() }],
            2,
        );
        assert!(doc.index().variable("x").is_none());
        assert!(doc.index().variable("z").is_some());
    }

    fn edit_strategy() -> impl Strategy<Value = Vec<(usize, usize, String)>> {
        let fragment = prop_oneof![
            Just(String::new()),
            Just("\n".to_owned()),
            Just("\r\n".to_owned()),
            Just(" + z".to_owned()),
            Just("c9: ".to_owned()),
            Just(">=".to_owned()),
            Just("é😀".to_owned()),
            Just("Bounds\n".to_owned()),
            Just("[ x ^ 2 ]".to_owned()),
            "[a-z0-9 :+<=-]{0,6}",
        ];
        prop::collection::vec((any::<usize>(), 0usize..8, fragment), 1..25)
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(128))]
        #[test]
        fn incremental_parse_matches_fresh_parse(edits in edit_strategy(), utf8 in any::<bool>()) {
            let encoding = if utf8 { Encoding::Utf8 } else { Encoding::Utf16 };
            let mut doc = Document::new(uri(), BASE.to_owned(), 0, encoding);
            let mut replay = BASE.to_owned();
            for (version, (start, len, text)) in edits.into_iter().enumerate() {
                let start = crate::position::floor_char_boundary(&replay, start % (replay.len() + 1));
                let end = crate::position::floor_char_boundary(&replay, (start + len).min(replay.len()));
                // Ranges must survive the LSP round trip (not split a CRLF).
                let lsp_range = doc.range(start..end);
                let bytes = doc.byte_range(lsp_range);
                replay.replace_range(bytes.clone(), &text);
                let event = TextDocumentContentChangeEvent { range: Some(lsp_range), range_length: None, text };
                doc.apply_changes(&[event], i32::try_from(version).unwrap() + 1);
                prop_assert_eq!(&doc.text, &replay);
                prop_assert_eq!(doc.tree.root_node().to_sexp(), syntax::parse(&replay, None).root_node().to_sexp());
                prop_assert_eq!(&doc.lines, &LineIndex::new(&replay));
            }
        }
    }
}
