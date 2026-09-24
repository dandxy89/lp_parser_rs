//! Semantic tokens (full, range, delta) following `HIGHLIGHTS_QUERY`.
//!
//! Every pattern in the grammar's `highlights.scm` captures a node by kind
//! alone, so tokens come from one cursor walk with a kind-id → token table
//! instead of the query engine (about ten times faster on large files). The
//! query remains the specification: a test checks the walk against it on every
//! fixture.
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

use tower_lsp_server::ls_types::{SemanticToken, SemanticTokenModifier, SemanticTokenType, SemanticTokensEdit, SemanticTokensLegend};

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

/// Compiled highlights query and the token kind of each capture index. Only
/// the tests use it, as the oracle for [`tokens`].
#[cfg(test)]
struct Highlights {
    query: tree_sitter::Query,
    kinds: Vec<Option<Kind>>,
}

#[cfg(test)]
fn highlights() -> &'static Highlights {
    static HIGHLIGHTS: OnceLock<Highlights> = OnceLock::new();
    HIGHLIGHTS.get_or_init(|| {
        let query =
            tree_sitter::Query::new(&syntax::language(), tree_sitter_lp::HIGHLIGHTS_QUERY).expect("bundled highlights query compiles");
        let kinds = query.capture_names().iter().map(|name| capture_kind(name)).collect();
        Highlights { query, kinds }
    })
}

/// Map a highlights capture name to a token kind (`None`: no token).
#[cfg(test)]
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

/// The token for a node kind, mirroring `highlights.scm`.
fn node_kind(name: &str, named: bool) -> Option<Kind> {
    let (token_type, modifiers) = if named {
        match name {
            "sense" | "end_marker" => (KEYWORD, 0),
            _ if name.ends_with("_keyword") => (KEYWORD, 0),
            kind::SOS_TYPE => (TYPE, 0),
            kind::COMPARISON_OPERATOR => (OPERATOR, 0),
            kind::NUMBER | kind::INFINITY => (NUMBER, READONLY),
            kind::OBJECTIVE_NAME | kind::CONSTRAINT_NAME | kind::SOS_NAME => (NAMESPACE, DECLARATION),
            kind::ATTRIBUTE_NAME => (PROPERTY, 0),
            kind::FUNCTION_NAME => (FUNCTION, DEFAULT_LIBRARY),
            kind::IDENTIFIER => (VARIABLE, 0),
            kind::LINE_COMMENT | kind::BLOCK_COMMENT => (COMMENT, 0),
            _ => return None,
        }
    } else {
        match name {
            "=" | "->" | "+" | "-" | "/" | "^" | "*" => (OPERATOR, 0),
            _ => return None,
        }
    };
    Some(Kind { token_type, modifiers })
}

/// Token kind per grammar symbol id, and whether a top-level section with
/// that id declares its variables.
struct Table {
    kinds: Vec<Option<Kind>>,
    declaring: Vec<bool>,
}

fn table() -> &'static Table {
    static TABLE: OnceLock<Table> = OnceLock::new();
    TABLE.get_or_init(|| {
        let language = syntax::language();
        let count = u16::try_from(language.node_kind_count()).unwrap_or(u16::MAX);
        let (mut kinds, mut declaring) = (Vec::new(), Vec::new());
        for id in 0..count {
            let name = language.node_kind_for_id(id).unwrap_or("");
            kinds.push(node_kind(name, language.node_kind_is_named(id)));
            declaring.push(DECLARING_SECTIONS.contains(&name));
        }
        Table { kinds, declaring }
    })
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
    match range {
        Some(range) => walk(doc, range, false).out,
        None => match crate::index::workers(doc.text.len()) {
            1 => walk(doc, 0..doc.text.len(), false).out,
            threads => parallel(doc, threads),
        },
    }
}

/// Tokenise with `threads` workers, each encoding the tokens that start in
/// its slice of the text, then re-base each chunk's first token on the
/// previous chunk's last one.
fn parallel(doc: &Document, threads: usize) -> Vec<SemanticToken> {
    let len = doc.text.len();
    let bounds: Vec<usize> = (0..=threads).map(|i| len * i / threads).collect();
    let chunks: Vec<Encoder<'_>> = std::thread::scope(|scope| {
        let workers: Vec<_> = bounds
            .windows(2)
            .map(|w| {
                let window = w[0]..w[1];
                scope.spawn(move || walk(doc, window, true))
            })
            .collect();
        // A worker panic is a bug in the walk; surface it unchanged.
        workers.into_iter().map(|w| w.join().unwrap_or_else(|e| std::panic::resume_unwind(e))).collect()
    });
    let mut out = Vec::with_capacity(chunks.iter().map(|c| c.out.len()).sum());
    let mut previous = (0, 0);
    for chunk in chunks {
        let Some(first) = chunk.out.first() else { continue };
        // Encoded against (0, 0), the first token holds its absolute position.
        let (line, column) = (first.delta_line, first.delta_start);
        let rebased = SemanticToken {
            delta_line: line - previous.0,
            delta_start: if line == previous.0 { column - previous.1 } else { column },
            ..*first
        };
        out.push(rebased);
        out.extend_from_slice(&chunk.out[1..]);
        previous = chunk.previous;
    }
    out
}

/// Walk the tree emitting tokens in `window`: tokens overlapping it, or with
/// `by_start`, only those starting in it (so windows never share a token).
fn walk(doc: &Document, Range { start, end }: Range<usize>, by_start: bool) -> Encoder<'_> {
    let table = table();
    let mut encoder = Encoder::new(doc);
    let mut cursor = doc.tree.walk();
    // Whether the current top-level section declares its variables.
    let mut declaring = false;
    // Tracked by hand: `TreeCursor::depth` walks the cursor stack, which is
    // deep inside sections with millions of children.
    let mut depth = 0usize;
    // Preorder walk: emit a token for a node with a kind (skipping its
    // subtree, as the query's outermost capture wins), else descend.
    loop {
        let node = cursor.node();
        if node.start_byte() >= end && node.start_byte() > start {
            break;
        }
        if depth == 1 {
            declaring = table.declaring.get(usize::from(node.kind_id())).copied().unwrap_or(false);
        }
        let overlaps = node.end_byte() > start || (node.start_byte() == start && start == end);
        let owned = !by_start || (start <= node.start_byte() && node.start_byte() < end);
        let token =
            if node.is_error() || node.is_missing() { None } else { table.kinds.get(usize::from(node.kind_id())).copied().flatten() };
        let descend = match token {
            Some(mut kind) if overlaps && owned => {
                if kind.token_type == VARIABLE && declaring {
                    kind.modifiers |= DECLARATION;
                }
                if node.start_byte() < node.end_byte() {
                    encoder.push(node.byte_range(), kind);
                }
                false
            }
            Some(_) => false,
            None => overlaps && node.child_count() > 0,
        };
        // Jump straight to the first child reaching `start` (large sections
        // have millions of children).
        if descend && (cursor.goto_first_child_for_byte(start).is_some() || cursor.goto_first_child()) {
            depth += 1;
            continue;
        }
        loop {
            if cursor.goto_next_sibling() {
                break;
            }
            if !cursor.goto_parent() {
                return encoder;
            }
            depth -= 1;
        }
    }
    encoder
}

/// Tokens straight from the highlights query: the reference [`tokens`] must
/// match.
#[cfg(test)]
fn query_tokens(doc: &Document) -> Vec<SemanticToken> {
    use streaming_iterator::StreamingIterator;
    let highlights = highlights();
    let mut cursor = tree_sitter::QueryCursor::new();
    let mut spans: Vec<(Range<usize>, usize, Kind)> = Vec::new();
    let mut matches = cursor.matches(&highlights.query, doc.tree.root_node(), doc.text.as_bytes());
    while let Some(m) = matches.next() {
        for capture in m.captures() {
            let Some(kind) = highlights.kinds[capture.index as usize] else { continue };
            spans.push((capture.node.byte_range(), m.pattern_index, kind));
        }
    }
    // Outermost node first, then earliest pattern: the widest capture wins.
    spans.sort_unstable_by_key(|(r, pattern, _)| (r.start, std::cmp::Reverse(r.end), *pattern));
    let declaring: Vec<Range<usize>> =
        doc.index().sections.iter().filter(|s| DECLARING_SECTIONS.contains(&s.kind)).map(|s| s.range.clone()).collect();
    let mut encoder = Encoder::new(doc);
    let mut covered = 0;
    for (span, _, mut kind) in spans {
        if span.start < covered || span.is_empty() {
            continue;
        }
        covered = span.end;
        if kind.token_type == VARIABLE && declaring.iter().any(|s| s.start <= span.start && span.end <= s.end) {
            kind.modifiers |= DECLARATION;
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
    /// Content range of `line` (terminator excluded): tokens inside it skip
    /// the line lookups.
    content: Range<usize>,
}

impl<'a> Encoder<'a> {
    const fn new(doc: &'a Document) -> Self {
        Self { doc, out: Vec::new(), previous: (0, 0), line: 0, cursor: 0, column: 0, content: 0..0 }
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
        // Fast path: the whole token sits on the current line.
        if self.content.start <= span.start && span.end <= self.content.end && self.cursor >= self.content.start {
            self.emit(self.line, span, kind);
            return;
        }
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
            if line != self.line || self.cursor < content.start {
                self.line = line;
                self.cursor = content.start;
                self.column = 0;
            }
            self.content = content;
            self.emit(line, start..end, kind);
        }
    }

    /// Append a token on `line` (the current line) spanning `span`.
    fn emit(&mut self, line: usize, Range { start, end }: Range<usize>, kind: Kind) {
        {
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
    fn walk_matches_highlights_query_on_every_fixture() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let mut checked = 0;
        for dir in ["rust/resources", "rust/tests"] {
            for entry in std::fs::read_dir(root.join(dir)).unwrap() {
                let path = entry.unwrap().path();
                if path.extension().is_none_or(|e| e != "lp") {
                    continue;
                }
                let d = doc(&std::fs::read_to_string(&path).unwrap(), Encoding::Utf16);
                assert_eq!(tokens(&d, None), query_tokens(&d), "{}", path.display());
                checked += 1;
            }
        }
        let d = doc(SAMPLE, Encoding::Utf8);
        assert_eq!(tokens(&d, None), query_tokens(&d));
        assert!(checked > 50, "only {checked} fixtures checked");
    }

    #[test]
    fn parallel_matches_sequential() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        for dir in ["rust/resources", "rust/tests"] {
            for entry in std::fs::read_dir(root.join(dir)).unwrap() {
                let path = entry.unwrap().path();
                if path.extension().is_none_or(|e| e != "lp") {
                    continue;
                }
                let d = doc(&std::fs::read_to_string(&path).unwrap(), Encoding::Utf16);
                let sequential = walk(&d, 0..d.text.len(), false).out;
                for threads in [2, 5] {
                    assert_eq!(parallel(&d, threads), sequential, "{} with {threads} threads", path.display());
                }
            }
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
