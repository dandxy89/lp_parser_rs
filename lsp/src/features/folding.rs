//! Folding ranges from `folds.scm`, block comments and runs of line comments.

use std::sync::LazyLock;

use streaming_iterator::StreamingIterator;
use tower_lsp_server::ls_types::{FoldingRange, FoldingRangeKind};
use tree_sitter::{Node, Query, QueryCursor};

use crate::document::Document;
use crate::syntax::{self, kind};

/// Compiled `folds.scm`. The query ships with the grammar, so a failure to
/// compile is a build defect (covered by the tests), not a runtime condition.
static FOLDS_QUERY: LazyLock<Query> =
    LazyLock::new(|| Query::new(&syntax::language(), tree_sitter_lp::FOLDS_QUERY).expect("folds.scm must compile"));

static COMMENTS_QUERY: LazyLock<Query> =
    LazyLock::new(|| Query::new(&syntax::language(), "[(line_comment) (block_comment)] @comment").expect("comment query must compile"));

/// Folding ranges for `doc`.
#[must_use]
pub fn ranges(doc: &Document) -> Vec<FoldingRange> {
    let root = doc.tree.root_node();
    let mut out = Vec::new();

    for node in captures(&FOLDS_QUERY, root, &doc.text) {
        push(doc, &mut out, section_start(node), node.end_byte(), None);
    }

    // A run of standalone line comments on consecutive lines: (start, end, last line, count).
    let mut run: Option<(usize, usize, u32, usize)> = None;
    for node in captures(&COMMENTS_QUERY, root, &doc.text) {
        if node.kind() == kind::BLOCK_COMMENT {
            push(doc, &mut out, node.start_byte(), node.end_byte(), Some(FoldingRangeKind::Comment));
            continue;
        }
        debug_assert_eq!(node.kind(), kind::LINE_COMMENT);
        let line = doc.position(node.start_byte()).line;
        let line_start = doc.lines.line_start(line as usize);
        if !doc.text[line_start..node.start_byte()].trim().is_empty() {
            // Trailing comment after code: not part of a comment block.
            flush(doc, &mut out, run.take());
            continue;
        }
        run = match run {
            Some((start, _, last, count)) if last + 1 == line => Some((start, node.end_byte(), line, count + 1)),
            previous => {
                flush(doc, &mut out, previous);
                Some((node.start_byte(), node.end_byte(), line, 1))
            }
        };
    }
    flush(doc, &mut out, run);

    out.sort_by_key(|r| (r.start_line, r.end_line));
    out
}

/// Start of a section for folding and outlines: the objectives section begins
/// at its `Minimize`/`Maximize` sense (and `multi-objectives` keyword), which
/// the grammar keeps as preceding siblings.
pub(crate) fn section_start(node: Node<'_>) -> usize {
    let mut start = node.start_byte();
    if node.kind() != kind::OBJECTIVES_SECTION {
        return start;
    }
    let mut previous = node.prev_sibling();
    while let Some(sibling) = previous {
        match sibling.kind() {
            kind::SENSE | kind::MULTI_OBJECTIVES_KEYWORD => start = sibling.start_byte(),
            kind::LINE_COMMENT | kind::BLOCK_COMMENT => {}
            _ => break,
        }
        previous = sibling.prev_sibling();
    }
    start
}

/// Every node captured by `query` under `root`, in document order.
fn captures<'t>(query: &Query, root: Node<'t>, text: &str) -> Vec<Node<'t>> {
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(query, root, text.as_bytes());
    let mut nodes = Vec::new();
    while let Some(m) = matches.next() {
        nodes.extend(m.captures().iter().map(|c| c.node));
    }
    nodes
}

fn flush(doc: &Document, out: &mut Vec<FoldingRange>, run: Option<(usize, usize, u32, usize)>) {
    if let Some((start, end, _, _)) = run.filter(|&(_, _, _, count)| count >= 2) {
        push(doc, out, start, end, Some(FoldingRangeKind::Comment));
    }
}

/// Add a fold over the lines of `start..end` if it spans at least two lines.
fn push(doc: &Document, out: &mut Vec<FoldingRange>, start: usize, end: usize, kind: Option<FoldingRangeKind>) {
    debug_assert!(start <= end && end <= doc.text.len());
    let start_line = doc.position(start).line;
    let end_line = doc.position(end).line;
    if end_line > start_line {
        out.push(FoldingRange { start_line, end_line, kind, ..FoldingRange::default() });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::position::Encoding;

    fn folds(text: &str) -> Vec<(u32, u32, Option<FoldingRangeKind>)> {
        let doc = Document::new("file:///t.lp".parse().unwrap(), text.to_owned(), 0, Encoding::Utf16);
        ranges(&doc).into_iter().map(|r| (r.start_line, r.end_line, r.kind)).collect()
    }

    #[test]
    fn folds_sections_and_comments() {
        let text = "\\ a\n\\ b\nMaximize\n obj: 3 x + 2 y\n\\* block\n comment *\\\nSubject To\n c1: x + y <= 10\n c2: x >= 1\n\nsos\n s1: S1 :: x : 1\n y : 2\nEnd\n";
        let comment = Some(FoldingRangeKind::Comment);
        assert_eq!(
            folds(text),
            [
                (0, 1, comment.clone()),
                (2, 3, None),
                (4, 5, comment),
                // Ends on its last constraint, not on the blank line or the next header.
                (6, 8, None),
                (10, 12, None),
            ]
        );
    }

    #[test]
    fn single_lines_do_not_fold() {
        let text = "min obj: x \\ trailing\n\\ lone\nst c1: x >= 1 \\ one\n\\ two\nend\n";
        assert_eq!(folds(text), []);
    }
}
