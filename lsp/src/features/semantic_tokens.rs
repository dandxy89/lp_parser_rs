//! Semantic tokens (full, range, delta) from `HIGHLIGHTS_QUERY`.
//!
//! Capture → token type mapping (standard LSP types only):
//!
//! | capture                         | type        | modifiers                         |
//! |---------------------------------|-------------|-----------------------------------|
//! | `keyword.*`                     | `keyword`   |                                   |
//! | `operator`                      | `operator`  |                                   |
//! | `number`, `constant.builtin`    | `number`    | `readonly`                        |
//! | `variable`                      | `variable`  | `declaration` in bounds and type sections |
//! | `function.builtin`              | `function`  | `defaultLibrary`                  |
//! | `comment`                       | `comment`   |                                   |
//! | `type`                          | `type`      |                                   |
//! | `property`                      | `property`  |                                   |
//! | `label`                         | `namespace` | `declaration`                     |
//!
//! LSP has no standard `label` type. Objective, constraint and SOS names live
//! in their own name spaces (see [`crate::index::Namespace`]), so they map to
//! `namespace`, which every client colours distinctly from variables.
//! Punctuation captures produce no token.

use std::ops::Range;
use std::sync::OnceLock;

use streaming_iterator::StreamingIterator;
use tower_lsp_server::ls_types::{SemanticToken, SemanticTokenModifier, SemanticTokenType, SemanticTokensEdit, SemanticTokensLegend};
use tree_sitter::{Query, QueryCursor};

use crate::document::Document;
use crate::position::Encoding;
use crate::syntax::{self, kind};

/// Legend token types; a token's type is an index into this list.
const TYPES: [SemanticTokenType; 9] = [
    SemanticTokenType::KEYWORD,
    SemanticTokenType::OPERATOR,
    SemanticTokenType::NUMBER,
    SemanticTokenType::VARIABLE,
    SemanticTokenType::FUNCTION,
    SemanticTokenType::COMMENT,
    SemanticTokenType::TYPE,
    SemanticTokenType::PROPERTY,
    SemanticTokenType::NAMESPACE,
];

const KEYWORD: u32 = 0;
const OPERATOR: u32 = 1;
const NUMBER: u32 = 2;
const VARIABLE: u32 = 3;
const FUNCTION: u32 = 4;
const COMMENT: u32 = 5;
const TYPE: u32 = 6;
const PROPERTY: u32 = 7;
const NAMESPACE: u32 = 8;

/// Legend token modifiers; a token's modifiers are a bit set over this list.
const MODIFIERS: [SemanticTokenModifier; 3] =
    [SemanticTokenModifier::DECLARATION, SemanticTokenModifier::READONLY, SemanticTokenModifier::DEFAULT_LIBRARY];

const DECLARATION: u32 = 1 << 0;
const READONLY: u32 = 1 << 1;
const DEFAULT_LIBRARY: u32 = 1 << 2;

/// Token kind for a capture: legend type index and modifier bits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Kind {
    token_type: u32,
    modifiers: u32,
}

/// Compiled highlights query and the token kind of each capture index.
struct Highlights {
    query: Query,
    kinds: Vec<Option<Kind>>,
}

fn highlights() -> &'static Highlights {
    static HIGHLIGHTS: OnceLock<Highlights> = OnceLock::new();
    HIGHLIGHTS.get_or_init(|| {
        // The query ships with the vendored grammar; failing to compile it is a
        // build defect, covered by the tests below.
        let query = Query::new(&syntax::language(), tree_sitter_lp::HIGHLIGHTS_QUERY).expect("bundled highlights query compiles");
        let kinds = query.capture_names().iter().map(|name| capture_kind(name)).collect();
        Highlights { query, kinds }
    })
}

/// Map a highlights capture name to a token kind (`None`: no token).
fn capture_kind(name: &str) -> Option<Kind> {
    let (token_type, modifiers) = match name.split('.').next()? {
        "keyword" => (KEYWORD, 0),
        "operator" => (OPERATOR, 0),
        "number" | "constant" => (NUMBER, READONLY),
        "variable" => (VARIABLE, 0),
        "function" => (FUNCTION, DEFAULT_LIBRARY),
        "comment" => (COMMENT, 0),
        "type" => (TYPE, 0),
        "property" => (PROPERTY, 0),
        "label" => (NAMESPACE, DECLARATION),
        _ => return None,
    };
    Some(Kind { token_type, modifiers })
}

/// Sections whose variables are declarations: bound entries and type-section
/// entries (`generals`, `integers`, `binaries`, `semi-continuous`). Every
/// identifier in these sections is such an entry.
const DECLARING_SECTIONS: [&str; 5] =
    [kind::BOUNDS_SECTION, kind::GENERALS_SECTION, kind::INTEGERS_SECTION, kind::BINARIES_SECTION, kind::SEMI_CONTINUOUS_SECTION];

/// Token legend declared in `initialize`.
#[must_use]
pub fn legend() -> SemanticTokensLegend {
    SemanticTokensLegend { token_types: TYPES.to_vec(), token_modifiers: MODIFIERS.to_vec() }
}

/// Encoded tokens for the whole document, or only those within `range`.
#[must_use]
pub fn tokens(doc: &Document, range: Option<Range<usize>>) -> Vec<SemanticToken> {
    debug_assert!(range.as_ref().is_none_or(|r| r.start <= r.end && r.end <= doc.text.len()), "token range out of bounds");
    let highlights = highlights();
    let mut cursor = QueryCursor::new();
    if let Some(range) = range {
        cursor.set_byte_range(range);
    }

    // (byte range, pattern index, kind). `matches` is much cheaper than
    // `captures` on large trees because it does not buffer to keep captures
    // ordered; sorting below restores document order.
    let mut spans: Vec<(Range<usize>, usize, Kind)> = Vec::new();
    let mut matches = cursor.matches(&highlights.query, doc.tree.root_node(), doc.text.as_bytes());
    while let Some(m) = matches.next() {
        for capture in m.captures {
            let Some(kind) = highlights.kinds[capture.index as usize] else { continue };
            spans.push((capture.node.byte_range(), m.pattern_index, kind));
        }
    }
    // Order by start, innermost (shortest) node first, then pattern index
    // (earliest pattern wins), so the most specific capture wins.
    spans.sort_unstable_by_key(|(r, pattern, _)| (r.start, r.end, *pattern));

    // Section spans are in document order, like `spans`, so one forward
    // pointer finds the declaring section (if any) around each variable.
    // (`Node::parent` is avoided: it descends from the root on every call.)
    let mut declaring = doc.index.sections.iter().filter(|s| DECLARING_SECTIONS.contains(&s.kind)).map(|s| s.range.clone()).peekable();

    let mut encoder = Encoder::new(doc);
    let mut covered = 0;
    for (span, _, mut kind) in spans {
        if span.start < covered || span.is_empty() {
            continue;
        }
        covered = span.end;
        if kind.token_type == VARIABLE {
            while declaring.next_if(|section| section.end <= span.start).is_some() {}
            if declaring.peek().is_some_and(|section| section.start <= span.start) {
                kind.modifiers |= DECLARATION;
            }
        }
        encoder.push(span, kind);
    }
    encoder.out
}

/// Relative token encoder: splits multi-line spans per line and measures
/// columns and lengths in the document's position encoding. Tokens must be
/// pushed in document order.
struct Encoder<'a> {
    doc: &'a Document,
    out: Vec<SemanticToken>,
    /// Line and start column of the previous token.
    previous: (u32, u32),
    /// Line of `cursor`, its byte offset and its column in the encoding.
    line: usize,
    cursor: usize,
    column: u32,
}

impl<'a> Encoder<'a> {
    fn new(doc: &'a Document) -> Self {
        Self { doc, out: Vec::new(), previous: (0, 0), line: 0, cursor: 0, column: 0 }
    }

    fn units(&self, range: Range<usize>) -> u32 {
        let s = &self.doc.text[range];
        let units = match self.doc.encoding {
            Encoding::Utf8 => s.len(),
            Encoding::Utf16 if s.is_ascii() => s.len(),
            Encoding::Utf16 => s.encode_utf16().count(),
        };
        u32::try_from(units).unwrap_or(u32::MAX)
    }

    fn push(&mut self, span: Range<usize>, kind: Kind) {
        debug_assert!(span.start >= self.cursor, "tokens must be pushed in document order");
        let lines = &self.doc.lines;
        let first = lines.line_of(span.start);
        let last = lines.line_of(span.end);
        for line in first..=last {
            let content = lines.line_range(&self.doc.text, line);
            let start = span.start.max(content.start);
            let end = span.end.min(content.end);
            if start >= end {
                continue;
            }
            if line != self.line {
                self.line = line;
                self.cursor = content.start;
                self.column = 0;
            }
            let column = self.column + self.units(self.cursor..start);
            let length = self.units(start..end);
            self.cursor = end;
            self.column = column + length;

            let line = u32::try_from(line).unwrap_or(u32::MAX);
            let (previous_line, previous_column) = self.previous;
            let delta_start = if line == previous_line { column - previous_column } else { column };
            self.out.push(SemanticToken {
                delta_line: line - previous_line,
                delta_start,
                length,
                token_type: kind.token_type,
                token_modifiers_bitset: kind.modifiers,
            });
            self.previous = (line, column);
        }
    }
}

/// Edits turning `old` into `new`: at most one edit replacing the differing
/// middle. `start` and `delete_count` count `u32`s (5 per token).
#[must_use]
pub fn delta(old: &[SemanticToken], new: &[SemanticToken]) -> Vec<SemanticTokensEdit> {
    let prefix = old.iter().zip(new).take_while(|(a, b)| a == b).count();
    let (old_rest, new_rest) = (&old[prefix..], &new[prefix..]);
    let suffix = old_rest.iter().rev().zip(new_rest.iter().rev()).take_while(|(a, b)| a == b).count();
    let deleted = old_rest.len() - suffix;
    let inserted = &new_rest[..new_rest.len() - suffix];
    if deleted == 0 && inserted.is_empty() {
        return Vec::new();
    }
    let to_u32 = |tokens: usize| u32::try_from(tokens * 5).unwrap_or(u32::MAX);
    vec![SemanticTokensEdit {
        start: to_u32(prefix),
        delete_count: to_u32(deleted),
        data: (!inserted.is_empty()).then(|| inserted.to_vec()),
    }]
}

#[cfg(test)]
mod tests {
    use tower_lsp_server::ls_types::Uri;

    use super::*;

    fn doc(text: &str, encoding: Encoding) -> Document {
        Document::new("file:///t.lp".parse::<Uri>().unwrap(), text.to_owned(), 1, encoding)
    }

    /// Decode relative tokens into absolute (line, column, length, type, modifiers).
    fn absolute(tokens: &[SemanticToken]) -> Vec<(u32, u32, u32, u32, u32)> {
        let (mut line, mut column) = (0, 0);
        tokens
            .iter()
            .map(|t| {
                if t.delta_line > 0 {
                    column = 0;
                }
                line += t.delta_line;
                column += t.delta_start;
                (line, column, t.length, t.token_type, t.token_modifiers_bitset)
            })
            .collect()
    }

    /// (text, type, modifiers) of each token, for single-line tokens.
    fn described(doc: &Document, tokens: &[SemanticToken]) -> Vec<(String, u32, u32)> {
        absolute(tokens)
            .into_iter()
            .map(|(line, column, length, ty, mods)| {
                let start = doc.offset(tower_lsp_server::ls_types::Position::new(line, column));
                let end = doc.offset(tower_lsp_server::ls_types::Position::new(line, column + length));
                (doc.text[start..end].to_owned(), ty, mods)
            })
            .collect()
    }

    fn flatten(tokens: &[SemanticToken]) -> Vec<u32> {
        tokens.iter().flat_map(|t| [t.delta_line, t.delta_start, t.length, t.token_type, t.token_modifiers_bitset]).collect()
    }

    const SAMPLE: &str = "Maximize\n obj: 3 x + 2 y\nSubject To\n c1: x + y <= inf\nGeneral Constraints\n g: r = MAX ( x , y )\nBounds\n x free\nGenerals\n y\nSOS\n s1: S1 :: x: 1\nEnd\n";

    #[test]
    fn legend_matches_indices() {
        let legend = legend();
        assert_eq!(legend.token_types[VARIABLE as usize], SemanticTokenType::VARIABLE);
        assert_eq!(legend.token_types[NAMESPACE as usize], SemanticTokenType::NAMESPACE);
        assert_eq!(legend.token_modifiers[2], SemanticTokenModifier::DEFAULT_LIBRARY);
        // Every capture in the query is either mapped or punctuation.
        for name in highlights().query.capture_names() {
            assert!(capture_kind(name).is_some() || name.starts_with("punctuation"), "unmapped capture {name}");
        }
    }

    #[test]
    fn types_and_modifiers() {
        let d = doc(SAMPLE, Encoding::Utf16);
        let got = described(&d, &tokens(&d, None));
        let s = |t: &str, ty, mods| (t.to_owned(), ty, mods);
        assert_eq!(
            got,
            [
                s("Maximize", KEYWORD, 0),
                s("obj", NAMESPACE, DECLARATION),
                s("3", NUMBER, READONLY),
                s("x", VARIABLE, 0),
                s("+", OPERATOR, 0),
                s("2", NUMBER, READONLY),
                s("y", VARIABLE, 0),
                s("Subject To", KEYWORD, 0),
                s("c1", NAMESPACE, DECLARATION),
                s("x", VARIABLE, 0),
                s("+", OPERATOR, 0),
                s("y", VARIABLE, 0),
                s("<=", OPERATOR, 0),
                s("inf", NUMBER, READONLY),
                s("General Constraints", KEYWORD, 0),
                s("g", NAMESPACE, DECLARATION),
                s("r", VARIABLE, 0),
                s("=", OPERATOR, 0),
                s("MAX", FUNCTION, DEFAULT_LIBRARY),
                s("x", VARIABLE, 0),
                s("y", VARIABLE, 0),
                s("Bounds", KEYWORD, 0),
                s("x", VARIABLE, DECLARATION),
                s("free", KEYWORD, 0),
                s("Generals", KEYWORD, 0),
                s("y", VARIABLE, DECLARATION),
                s("SOS", KEYWORD, 0),
                s("s1", NAMESPACE, DECLARATION),
                s("S1", TYPE, 0),
                s("x", VARIABLE, 0),
                s("1", NUMBER, READONLY),
                s("End", KEYWORD, 0),
            ]
        );
    }

    #[test]
    fn no_overlaps_and_single_line() {
        let text = "min\n x\nst\n c1: x = 1\n c2: x => 2\n\\* block\r\n  comment *\\\nend\n";
        let d = doc(text, Encoding::Utf8);
        let abs = absolute(&tokens(&d, None));
        for pair in abs.windows(2) {
            let ((l0, c0, n0, ..), (l1, c1, ..)) = (pair[0], pair[1]);
            assert!(l1 > l0 || c1 >= c0 + n0, "overlap: {pair:?}");
        }
        let comments: Vec<_> = described(&d, &tokens(&d, None)).into_iter().filter(|t| t.1 == COMMENT).map(|t| t.0).collect();
        assert_eq!(comments, ["\\* block", "  comment *\\"]);
    }

    #[test]
    fn range_restriction() {
        let d = doc(SAMPLE, Encoding::Utf16);
        let start = SAMPLE.find("Bounds").unwrap();
        let end = SAMPLE.find("Generals").unwrap();
        let got = described(&d, &tokens(&d, Some(start..end)));
        assert_eq!(got, [("Bounds".to_owned(), KEYWORD, 0), ("x".to_owned(), VARIABLE, DECLARATION), ("free".to_owned(), KEYWORD, 0)]);
        // First token is absolute.
        assert_eq!(absolute(&tokens(&d, Some(start..end)))[0].0, 6);
    }

    #[test]
    fn utf16_lengths() {
        // Names are ASCII; multi-byte text appears in comments.
        let text = "min\n \\ café 😀\n \\* é😀 *\\ x + y\nst\n c: x >= 1\nend\n";
        let utf16 = doc(text, Encoding::Utf16);
        let abs = absolute(&tokens(&utf16, None));
        // `\ café 😀` and `\* é😀 *\` are 9 UTF-16 units (12 bytes) each.
        assert_eq!(abs[1], (1, 1, 9, COMMENT, 0));
        assert_eq!(abs[2], (2, 1, 9, COMMENT, 0));
        assert_eq!(abs[3], (2, 11, 1, VARIABLE, 0));
        assert_eq!(abs[4], (2, 13, 1, OPERATOR, 0));
        let utf8 = doc(text, Encoding::Utf8);
        let abs = absolute(&tokens(&utf8, None));
        assert_eq!(abs[1], (1, 1, 12, COMMENT, 0));
        assert_eq!(abs[2], (2, 1, 12, COMMENT, 0));
        assert_eq!(abs[3], (2, 14, 1, VARIABLE, 0));
        assert_eq!(abs[4], (2, 16, 1, OPERATOR, 0));
        assert_eq!(described(&utf16, &tokens(&utf16, None)), described(&utf8, &tokens(&utf8, None)));
    }

    fn apply(old: &[SemanticToken], edits: &[SemanticTokensEdit]) -> Vec<u32> {
        let mut data = flatten(old);
        for edit in edits {
            let start = edit.start as usize;
            let inserted = edit.data.as_deref().map(flatten).unwrap_or_default();
            data.splice(start..start + edit.delete_count as usize, inserted);
        }
        data
    }

    #[test]
    fn delta_round_trip() {
        let old = tokens(&doc(SAMPLE, Encoding::Utf16), None);
        for edited in [
            SAMPLE.replace("c1", "limit"),
            SAMPLE.replace(" x free\n", ""),
            SAMPLE.replace("Generals\n y\n", "Generals\n y\n z\n w\n"),
            "min\nst\nend\n".to_owned(),
        ] {
            let new = tokens(&doc(&edited, Encoding::Utf16), None);
            let edits = delta(&old, &new);
            assert_eq!(edits.len(), 1);
            assert_eq!(edits[0].start % 5, 0);
            assert_eq!(apply(&old, &edits), flatten(&new));
        }
        assert_eq!(delta(&old, &old), []);
        assert_eq!(apply(&[], &delta(&[], &old)), flatten(&old));
        assert_eq!(apply(&old, &delta(&old, &[])), Vec::<u32>::new());
    }
}
