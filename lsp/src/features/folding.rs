//! Folding ranges for the `folds.scm` captures (sections), block comments
//! and runs of line comments.

use std::sync::OnceLock;

use tower_lsp_server::ls_types::{FoldingRange, FoldingRangeKind};
use tree_sitter::Node;

use crate::document::Document;
use crate::syntax::{self, kind};

/// Folding ranges for `doc`.
#[must_use]
pub fn ranges(doc: &Document) -> Vec<FoldingRange> {
    let (sections, comments) = fold_nodes(doc);
    let mut out = Vec::new();

    for node in sections {
        push(doc, &mut out, section_start(node), node.end_byte(), None);
    }

    // A run of standalone line comments on consecutive lines: (start, end, last line, count).
    let mut run: Option<(usize, usize, u32, usize)> = None;
    for node in comments {
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

/// Grammar symbols that fold as sections (the `folds.scm` captures) and that
/// are comments, as lookup tables by symbol id.
struct FoldKinds {
    section: Vec<bool>,
    comment: Vec<bool>,
}

impl FoldKinds {
    fn get() -> &'static Self {
        static KINDS: OnceLock<FoldKinds> = OnceLock::new();
        KINDS.get_or_init(|| {
            let count = syntax::language().node_kind_count();
            let table = |kinds: &[&str]| {
                let mut table = vec![false; count];
                for id in kinds.iter().flat_map(|k| syntax::kind_ids(k, true)) {
                    table[usize::from(id)] = true;
                }
                table
            };
            Self { section: table(syntax::SECTION_KINDS), comment: table(&[kind::LINE_COMMENT, kind::BLOCK_COMMENT]) }
        })
    }

    fn is(table: &[bool], node: Node<'_>) -> bool {
        table.get(usize::from(node.kind_id())).copied().unwrap_or(false)
    }
}

/// Section nodes and comment nodes, each in document order, without visiting
/// the whole tree. Sections are children of the root, or of an `ERROR`, so
/// below the root only subtrees with errors can hold one; every comment
/// starts with `\`, so only subtrees spanning a backslash can hold one.
fn fold_nodes(doc: &Document) -> (Vec<Node<'_>>, Vec<Node<'_>>) {
    let kinds = FoldKinds::get();
    let backslashes: Vec<usize> = doc.text.match_indices('\\').map(|(i, _)| i).collect();
    let spans_backslash = |node: Node<'_>| {
        let next = backslashes.partition_point(|&b| b < node.start_byte());
        backslashes.get(next).is_some_and(|&b| b < node.end_byte())
    };
    let (mut sections, mut comments) = (Vec::new(), Vec::new());
    let mut cursor = doc.tree.walk();
    let mut at_root = true;
    loop {
        let node = cursor.node();
        if FoldKinds::is(&kinds.section, node) {
            sections.push(node);
        } else if FoldKinds::is(&kinds.comment, node) {
            comments.push(node);
        }
        let descend = at_root || node.has_error() || spans_backslash(node);
        at_root = false;
        if descend && cursor.goto_first_child() {
            continue;
        }
        loop {
            if cursor.goto_next_sibling() {
                break;
            }
            if !cursor.goto_parent() {
                debug_assert!(sections.windows(2).all(|w| w[0].start_byte() <= w[1].start_byte()), "sections in document order");
                debug_assert!(comments.windows(2).all(|w| w[0].end_byte() <= w[1].start_byte()), "comments in document order");
                return (sections, comments);
            }
        }
    }
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
    use proptest::prelude::*;
    use streaming_iterator::StreamingIterator;
    use tree_sitter::{Query, QueryCursor};

    use super::*;
    use crate::position::Encoding;

    fn document(text: &str) -> Document {
        Document::new("file:///t.lp".parse().unwrap(), text.to_owned(), 0, Encoding::Utf16)
    }

    fn folds(text: &str) -> Vec<(u32, u32, Option<FoldingRangeKind>)> {
        ranges(&document(text)).into_iter().map(|r| (r.start_line, r.end_line, r.kind)).collect()
    }

    /// Every node captured by `source` under the root, in document order.
    fn captures<'t>(doc: &'t Document, source: &str) -> Vec<Node<'t>> {
        let query = Query::new(&syntax::language(), source).unwrap();
        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(&query, doc.tree.root_node(), doc.text.as_bytes());
        let mut nodes = Vec::new();
        while let Some(m) = matches.next() {
            nodes.extend(m.captures().iter().map(|c| c.node));
        }
        nodes
    }

    /// [`fold_nodes`] must find exactly what the queries capture.
    fn assert_matches_queries(text: &str) {
        let doc = document(text);
        let (sections, comments) = fold_nodes(&doc);
        assert_eq!(sections, captures(&doc, tree_sitter_lp::FOLDS_QUERY), "sections of {text:?}");
        assert_eq!(comments, captures(&doc, "[(line_comment) (block_comment)] @comment"), "comments of {text:?}");
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
        assert_matches_queries(text);
    }

    #[test]
    fn single_lines_do_not_fold() {
        let text = "min obj: x \\ trailing\n\\ lone\nst c1: x >= 1 \\ one\n\\ two\nend\n";
        assert_eq!(folds(text), []);
        assert_matches_queries(text);
    }

    #[test]
    fn broken_documents_match_queries() {
        for text in [
            "",
            "\\ only a comment",
            "Minimize\n obj: 3 x + \nSubject To\n c1: x + >= 1\n c2 x - y <= 4\n c1: [ x ^ 2 \nBounds\n x <= \nEnd\n",
            "Minimize\n obj: x\nBounds\n x <= 1\nSubject To\n c1: x >= 1 \\ c\nGenerals\n x\nBounds\n x >= 0\nEnd\n",
            "Maximize\n obj: x \\* open block\nSubject To\n Bounds Generals\n \\ c\n sos\n s1: S1 :: x : 1\nEnd\n",
            "st\n c1: x >= 1\n\\ misplaced\nMinimize\n obj: x\nEnd\n",
        ] {
            assert_matches_queries(text);
        }
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(256))]
        #[test]
        fn fold_nodes_match_queries(
            parts in prop::collection::vec(
                prop_oneof![
                    Just("Minimize\n obj: x + y\n".to_owned()),
                    Just("Subject To\n c1: x + y >= 1\n".to_owned()),
                    Just(" c2: x - y <= 4 \\ trailing\n".to_owned()),
                    Just("\\ line comment\n".to_owned()),
                    Just("\\* block\n comment *\\\n".to_owned()),
                    Just("Bounds\n x <= 10\n".to_owned()),
                    Just("Generals\n y\n".to_owned()),
                    Just("sos\n s1: S1 :: x : 1\n".to_owned()),
                    Just("General Constraints\n g: r = MAX(x, y)\n".to_owned()),
                    Just(" [ x ^ 2 ]".to_owned()),
                    Just(" >= <= :".to_owned()),
                    Just("End\n".to_owned()),
                    "[a-z0-9 :+<=\\\\\\n-]{0,8}",
                ],
                0..14,
            )
        ) {
            assert_matches_queries(&parts.concat());
        }
    }
}
