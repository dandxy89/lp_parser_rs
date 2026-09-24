//! Selection ranges from the tree-sitter ancestor chain.

use std::ops::Range;

use tower_lsp_server::ls_types::{Position, SelectionRange};

use crate::document::Document;
use crate::syntax;

/// One selection-range chain per position.
#[must_use]
pub fn ranges(doc: &Document, positions: &[Position]) -> Vec<SelectionRange> {
    positions.iter().map(|&position| chain(doc, doc.offset(position))).collect()
}

/// Smallest node at `offset` → its ancestors → the file, skipping ancestors
/// with the same range as their child.
fn chain(doc: &Document, offset: usize) -> SelectionRange {
    let root = doc.tree.root_node();
    let mut spans: Vec<Range<usize>> = Vec::new();
    let mut current = syntax::token_at(&doc.tree, offset).or(Some(root));
    while let Some(node) = current {
        if spans.last() != Some(&node.byte_range()) {
            spans.push(node.byte_range());
        }
        current = node.parent();
    }
    debug_assert!(!spans.is_empty(), "the chain always contains the root");
    debug_assert!(spans.windows(2).all(|w| w[1].start <= w[0].start && w[0].end <= w[1].end), "ancestors contain their children");

    // Build from the outermost range inwards.
    let mut selection: Option<SelectionRange> = None;
    for span in spans.into_iter().rev() {
        selection = Some(SelectionRange { range: doc.range(span), parent: selection.map(Box::new) });
    }
    selection.unwrap_or_else(|| SelectionRange { range: doc.full_range(), parent: None })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::position::Encoding;

    fn texts(doc: &Document, selection: &SelectionRange) -> Vec<String> {
        let mut out = vec![doc.slice(doc.byte_range(selection.range)).to_owned()];
        let mut parent = selection.parent.as_deref();
        while let Some(p) = parent {
            out.push(doc.slice(doc.byte_range(p.range)).to_owned());
            parent = p.parent.as_deref();
        }
        out
    }

    #[test]
    fn expands_from_term_to_file() {
        let text = "min\n obj: x\nst\n c1: 2 y + x >= 1\nend\n";
        let doc = Document::new("file:///t.lp".parse().unwrap(), text.to_owned(), 0, Encoding::Utf16);
        let at = doc.position(text.find("2 y").unwrap() + 2);
        let chain = ranges(&doc, &[at]);
        assert_eq!(chain.len(), 1);
        assert_eq!(texts(&doc, &chain[0]), ["y", "2 y", "2 y + x", "c1: 2 y + x >= 1", "st\n c1: 2 y + x >= 1", text]);
    }

    #[test]
    fn position_outside_any_token_selects_the_file() {
        let text = "min\n obj: x\n\n\nend\n";
        let doc = Document::new("file:///t.lp".parse().unwrap(), text.to_owned(), 0, Encoding::Utf16);
        let chain = ranges(&doc, &[Position::new(2, 0), Position::new(99, 0)]);
        assert_eq!(chain.len(), 2);
        for selection in &chain {
            assert_eq!(selection.parent, None);
        }
    }
}
