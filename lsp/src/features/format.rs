//! Tree-sitter-based formatter preserving comments and single blank lines.
//!
//! The document is split into *units*: headers (the sense line, section
//! keywords, `end`), entries (objectives, constraints, bounds, type-section
//! names, SOS headers and entries) and standalone comments. Comments between
//! two tokens of one unit stay inside it; the rest become their own units,
//! kept on the line they trailed or on a line of their own.
//!
//! Every token is printed exactly as written, separated by a single space
//! (upstream names may contain `(`, `,`, `[`, `-`, ..., so tokens are never
//! joined unless the join is known to be lexically safe). Only whitespace,
//! keyword casing and the `=<` / `=>` aliases change.
//!
//! Safety: line starts are significant in LP (single-word section keywords
//! are keywords only as the first token of a line), so an entry starting with
//! such a word stays on the previous line. As a last line of defence the
//! output is reparsed and its token sequence compared with the input's; any
//! mismatch returns `None` instead of an edit.

use std::ops::Range;

use tower_lsp_server::ls_types::{Position, Range as LspRange, TextEdit};
use tree_sitter::{Node, Tree};

use crate::config::{FormatSettings, KeywordCase};
use crate::document::Document;
use crate::syntax::{self, kind};

/// Header units: printed at column 0 on a line of their own.
const HEADER_KINDS: &[&str] = &[
    kind::SENSE,
    kind::MULTI_OBJECTIVES_KEYWORD,
    kind::SUBJECT_TO_KEYWORD,
    kind::LAZY_CONSTRAINTS_KEYWORD,
    kind::USER_CUTS_KEYWORD,
    kind::GENERAL_CONSTRAINTS_KEYWORD,
    kind::BOUNDS_KEYWORD,
    kind::GENERALS_KEYWORD,
    kind::INTEGERS_KEYWORD,
    kind::BINARIES_KEYWORD,
    kind::SEMI_CONTINUOUS_KEYWORD,
    kind::SOS_KEYWORD,
    kind::END_MARKER,
];

/// Tokens whose casing follows `keyword_case`.
const CASED_KINDS: &[&str] = &[kind::FREE_KEYWORD, kind::SOS_TYPE, kind::INFINITY];

/// Words that become section keywords at the start of a line unless followed
/// by `:` (upstream `Lexer::resolve_keyword` and the external scanner).
const LINE_START_KEYWORDS: &[&str] = &[
    "bound",
    "bounds",
    "gen",
    "general",
    "generals",
    "integer",
    "integers",
    "bin",
    "binary",
    "binaries",
    "semi",
    "semis",
    "semi-continuous",
    "sos",
    "end",
    "genconstr",
    "genconstrs",
    "st",
    "s.t.",
];

/// Sections of linear constraints.
const CONSTRAINT_SECTIONS: &[&str] = &[kind::CONSTRAINTS_SECTION, kind::LAZY_CONSTRAINTS_SECTION, kind::USER_CUTS_SECTION];

/// Sections whose comparison operators `align_operators` lines up.
const ALIGNED_SECTIONS: &[&str] =
    &[kind::CONSTRAINTS_SECTION, kind::LAZY_CONSTRAINTS_SECTION, kind::USER_CUTS_SECTION, kind::BOUNDS_SECTION];

/// Unit key shared by the sense and a following `multi-objectives`.
const SENSE_KEY: usize = usize::MAX;

/// A token of the syntax tree (comments included).
#[derive(Debug, Clone, Copy)]
struct Leaf<'a> {
    kind: &'static str,
    named: bool,
    parent: &'static str,
    text: &'a str,
    start: usize,
    end: usize,
    /// Id of the unit node (the child of a section or of the root).
    unit: usize,
    /// Kind of the unit node.
    unit_kind: &'static str,
    /// Enclosing section kind (`source_file` outside sections).
    section: &'static str,
    /// Id of the enclosing section (0 outside sections).
    section_id: usize,
}

impl Leaf<'_> {
    fn is_comment(&self) -> bool {
        self.kind == kind::LINE_COMMENT || self.kind == kind::BLOCK_COMMENT
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UnitKind {
    Header,
    Entry,
    Comment,
}

/// A line-level item of the output.
#[derive(Debug, Clone)]
struct Unit {
    kind: UnitKind,
    node_kind: &'static str,
    section: &'static str,
    section_id: usize,
    leaves: Range<usize>,
    /// Source byte span.
    start: usize,
    end: usize,
}

/// Placement of a unit in the formatted output.
#[derive(Debug, Clone)]
struct Placed {
    kind: UnitKind,
    /// Source byte span.
    src: Range<usize>,
    /// Output offset of the line the unit starts on.
    out_line_start: usize,
    /// Output offset just past the unit.
    out_end: usize,
    /// Indent of the unit's first line.
    indent: usize,
}

/// The formatted text plus where each unit landed.
#[derive(Debug)]
struct Layout {
    text: String,
    units: Vec<Placed>,
}

/// Format a whole LP text. `None` when it has syntax errors.
#[must_use]
pub fn format_text(text: &str, settings: &FormatSettings) -> Option<String> {
    let tree = syntax::parse(text, None);
    layout(text, &tree, settings).map(|l| l.text)
}

/// Edits formatting the whole document. `None` on syntax errors.
#[must_use]
pub fn format_document(doc: &Document, settings: &FormatSettings) -> Option<Vec<TextEdit>> {
    let formatted = layout(&doc.text, &doc.tree, settings)?.text;
    let (old, new) = (doc.text.as_bytes(), formatted.as_bytes());
    if old == new {
        return Some(vec![]);
    }
    // Trim the common prefix and suffix back to line boundaries so the edit
    // only covers changed lines.
    let mut prefix = old.iter().zip(new).take_while(|(a, b)| a == b).count();
    prefix = old[..prefix].iter().rposition(|&b| b == b'\n').map_or(0, |i| i + 1);
    let max_suffix = old.len().min(new.len()) - prefix;
    let mut suffix = old.iter().rev().zip(new.iter().rev()).take(max_suffix).take_while(|(a, b)| a == b).count();
    let tail = &old[old.len() - suffix..];
    suffix = tail.iter().position(|&b| b == b'\n').map_or(0, |i| suffix - i - 1);
    debug_assert!(prefix + suffix <= old.len() && prefix + suffix <= new.len(), "edit bounds overlap");
    let range = doc.range(prefix..old.len() - suffix);
    Some(vec![TextEdit { range, new_text: formatted[prefix..new.len() - suffix].to_owned() }])
}

/// Edits formatting whole entries overlapping `range`.
#[must_use]
pub fn format_range(doc: &Document, range: LspRange, settings: &FormatSettings) -> Option<Vec<TextEdit>> {
    let layout = layout(&doc.text, &doc.tree, settings)?;
    let bytes = doc.byte_range(range);
    let overlaps = |u: &Placed| {
        if bytes.is_empty() {
            u.src.start <= bytes.start && bytes.start <= u.src.end
        } else {
            u.src.start < bytes.end && bytes.start < u.src.end
        }
    };
    let Some(first) = layout.units.iter().position(overlaps) else { return Some(vec![]) };
    let last = layout.units.iter().rposition(overlaps).unwrap_or(first);
    Some(snapped_edit(doc, &layout, first, last).into_iter().collect())
}

/// On-type formatting after `ch` (`\n`) at `position`.
#[must_use]
pub fn format_on_type(doc: &Document, position: Position, ch: &str, settings: &FormatSettings) -> Option<Vec<TextEdit>> {
    if ch != "\n" || position.line == 0 {
        return None;
    }
    let layout = layout(&doc.text, &doc.tree, settings)?;
    let line = usize::try_from(position.line).ok()?;
    if line >= doc.lines.line_count() {
        return None;
    }
    let line_start = doc.lines.line_start(line);
    let prev = doc.lines.line_range(&doc.text, line - 1);
    let mut edits = Vec::new();

    // Reformat the entries on the previous line when they end before the new one.
    let on_prev = |u: &Placed| u.src.start < prev.end.max(prev.start + 1) && prev.start < u.src.end;
    if let Some(first) = layout.units.iter().position(on_prev) {
        let last = layout.units.iter().rposition(on_prev).unwrap_or(first);
        let (first, last) = expand_to_lines(doc, &layout.units, first, last);
        if layout.units[last].src.end < line_start {
            edits.extend(snapped_edit(doc, &layout, first, last));
        }
    }

    // Re-indent the new line.
    let current = doc.lines.line_range(&doc.text, line);
    let line_text = &doc.text[current.clone()];
    let width = line_text.len() - line_text.trim_start_matches([' ', '\t']).len();
    let content = current.start + width;
    let has_content = !line_text.trim().is_empty();
    let wanted = if has_content {
        match layout.units.iter().find(|u| u.src.start <= content && content < u.src.end.max(u.src.start + 1)) {
            Some(u) if u.src.start == content => Some(u.indent),
            Some(u) if u.kind == UnitKind::Entry => Some(u.indent + settings.indent),
            _ => None,
        }
    } else {
        let before = layout.units.iter().rev().find(|u| u.src.end <= current.start);
        Some(match before {
            None => 0,
            Some(u) if u.kind == UnitKind::Header => {
                if doc.text[u.src.clone()].eq_ignore_ascii_case("end") {
                    0
                } else {
                    settings.indent
                }
            }
            Some(u) => u.indent,
        })
    };
    if let Some(wanted) = wanted
        && line_text[..width] != *" ".repeat(wanted)
    {
        edits.push(TextEdit { range: doc.range(current.start..content), new_text: " ".repeat(wanted) });
    }
    Some(edits)
}

/// Widen `first..=last` to whole source lines: units sharing a line with the
/// selection are included, so the edit can start at a line start.
fn expand_to_lines(doc: &Document, units: &[Placed], mut first: usize, mut last: usize) -> (usize, usize) {
    debug_assert!(first <= last && last < units.len(), "invalid unit selection");
    while first > 0 && doc.lines.line_of(units[first].src.start) == doc.lines.line_of(units[first - 1].src.end) {
        first -= 1;
    }
    while last + 1 < units.len() && doc.lines.line_of(units[last + 1].src.start) == doc.lines.line_of(units[last].src.end) {
        last += 1;
    }
    (first, last)
}

/// The edit replacing units `first..=last` (widened to whole lines) with their
/// formatted text, or nothing when already formatted.
fn snapped_edit(doc: &Document, layout: &Layout, first: usize, last: usize) -> Option<TextEdit> {
    let (first, last) = expand_to_lines(doc, &layout.units, first, last);
    let src = doc.lines.line_start(doc.lines.line_of(layout.units[first].src.start))..layout.units[last].src.end;
    let out = layout.units[first].out_line_start..layout.units[last].out_end;
    debug_assert!(doc.text[src.start..layout.units[first].src.start].trim().is_empty(), "snapped range must start at a line start");
    let new_text = &layout.text[out];
    (doc.text[src.clone()] != *new_text).then(|| TextEdit { range: doc.range(src), new_text: new_text.to_owned() })
}

/// Format `text` (parsed as `tree`). `None` on syntax errors, or when the
/// result would not provably keep the token sequence.
fn layout(text: &str, tree: &Tree, settings: &FormatSettings) -> Option<Layout> {
    let root = tree.root_node();
    if root.has_error() || root.kind() != kind::SOURCE_FILE {
        return None;
    }
    let leaves = collect_leaves(root, text);
    let units = group_units(&leaves);
    let newline = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let layout = render(text, &leaves, &units, settings, newline)?;

    // Last line of defence: same tokens, same kinds, no errors.
    let reparsed = syntax::parse(&layout.text, None);
    if reparsed.root_node().has_error() {
        return None;
    }
    let after = collect_leaves(reparsed.root_node(), &layout.text);
    let same = leaves.len() == after.len() && leaves.iter().zip(&after).all(|(a, b)| a.kind == b.kind && canonical(a) == canonical(b));
    same.then_some(layout)
}

/// All tokens in document order. A `comparison_operator` counts as one token.
fn collect_leaves<'a>(root: Node<'_>, source: &'a str) -> Vec<Leaf<'a>> {
    let mut leaves = Vec::new();
    let mut cursor = root.walk();
    let mut path = vec![root];
    loop {
        let node = cursor.node();
        let is_leaf = node.child_count() == 0 || node.kind() == kind::COMPARISON_OPERATOR;
        if is_leaf {
            leaves.push(leaf(&path, source));
        }
        if !is_leaf && cursor.goto_first_child() {
            path.push(cursor.node());
            continue;
        }
        loop {
            if cursor.goto_next_sibling() {
                if let Some(top) = path.last_mut() {
                    *top = cursor.node();
                }
                break;
            }
            if !cursor.goto_parent() {
                return leaves;
            }
            path.pop();
        }
    }
}

/// The leaf at the end of `path` (root first).
fn leaf<'a>(path: &[Node<'_>], source: &'a str) -> Leaf<'a> {
    debug_assert!(!path.is_empty(), "path must contain the leaf");
    let node = path[path.len() - 1];
    let parent = if path.len() >= 2 { path[path.len() - 2].kind() } else { "" };
    let in_section = path.len() >= 3 && syntax::is_section(path[1]);
    let unit_node = if in_section { path[2] } else { path.get(1).copied().unwrap_or(node) };
    let unit =
        if unit_node.kind() == kind::SENSE || unit_node.kind() == kind::MULTI_OBJECTIVES_KEYWORD { SENSE_KEY } else { unit_node.id() };
    Leaf {
        kind: node.kind(),
        named: node.is_named(),
        parent,
        text: syntax::text(node, source),
        start: node.start_byte(),
        end: node.end_byte(),
        unit,
        unit_kind: unit_node.kind(),
        section: if in_section { path[1].kind() } else { kind::SOURCE_FILE },
        section_id: if in_section { path[1].id() } else { 0 },
    }
}

/// Group leaves into units. A comment between two tokens of the same unit
/// belongs to it; any other comment is a unit of its own.
fn group_units(leaves: &[Leaf<'_>]) -> Vec<Unit> {
    let mut prev_sig: Vec<Option<usize>> = Vec::with_capacity(leaves.len());
    let mut last = None;
    for leaf in leaves {
        prev_sig.push(last);
        if !leaf.is_comment() {
            last = Some(leaf.unit);
        }
    }
    let mut next_sig = vec![None; leaves.len()];
    let mut next = None;
    for (i, leaf) in leaves.iter().enumerate().rev() {
        next_sig[i] = next;
        if !leaf.is_comment() {
            next = Some(leaf.unit);
        }
    }

    let mut units: Vec<Unit> = Vec::new();
    let mut current: Option<usize> = None;
    for (i, leaf) in leaves.iter().enumerate() {
        let key = if leaf.is_comment() { prev_sig[i].filter(|&p| Some(p) == next_sig[i]) } else { Some(leaf.unit) };
        match (key, units.last_mut()) {
            (Some(k), Some(unit)) if current == Some(k) => {
                unit.leaves.end = i + 1;
                unit.end = leaf.end;
            }
            _ => {
                let kind = match key {
                    None => UnitKind::Comment,
                    Some(_) if HEADER_KINDS.contains(&leaf.unit_kind) => UnitKind::Header,
                    Some(_) => UnitKind::Entry,
                };
                units.push(Unit {
                    kind,
                    node_kind: leaf.unit_kind,
                    section: leaf.section,
                    section_id: leaf.section_id,
                    leaves: i..i + 1,
                    start: leaf.start,
                    end: leaf.end,
                });
            }
        }
        current = key;
    }
    units
}

/// A printable token with its spacing rules.
#[derive(Debug)]
struct Piece {
    text: String,
    /// No space before (`:`, `::`, an operand after a unary sign).
    glue: bool,
    /// A line may break before it (a binary `+` / `-`).
    break_before: bool,
    /// A line must break after it (a line comment).
    hard_break_after: bool,
    /// The first comparison operator (alignment anchor).
    align: bool,
}

fn pieces(leaves: &[Leaf<'_>], case: KeywordCase) -> Vec<Piece> {
    let mut out: Vec<Piece> = Vec::with_capacity(leaves.len());
    let mut prev_sig: Option<&Leaf<'_>> = None;
    let mut after_unary = false;
    let mut aligned = false;
    for leaf in leaves {
        if leaf.is_comment() {
            let hard = leaf.kind == kind::LINE_COMMENT;
            out.push(Piece { text: comment_text(leaf), glue: false, break_before: false, hard_break_after: hard, align: false });
            after_unary = false;
            continue;
        }
        let is_sign = !leaf.named && matches!(leaf.text, "+" | "-");
        let unary = is_sign && prev_sig.is_none_or(|p| p.kind == kind::COMPARISON_OPERATOR || (!p.named && !matches!(p.text, "]" | ")")));
        let colon = !leaf.named && matches!(leaf.text, ":" | "::");
        let is_op = leaf.kind == kind::COMPARISON_OPERATOR;
        out.push(Piece {
            text: token_text(leaf, case),
            glue: colon || (after_unary && leaf.named),
            break_before: is_sign && !unary && matches!(leaf.parent, kind::LINEAR_EXPRESSION | kind::QUADRATIC_BLOCK),
            hard_break_after: false,
            align: is_op && !aligned,
        });
        aligned |= is_op;
        after_unary = unary;
        prev_sig = Some(leaf);
    }
    out
}

/// Printed text of a non-comment token.
fn token_text(leaf: &Leaf<'_>, case: KeywordCase) -> String {
    if HEADER_KINDS.contains(&leaf.kind) || CASED_KINDS.contains(&leaf.kind) {
        return apply_case(&collapse_whitespace(leaf.text), case);
    }
    if leaf.kind == kind::COMPARISON_OPERATOR {
        return match leaf.text {
            "=<" => "<=".to_owned(),
            "=>" => ">=".to_owned(),
            other => other.to_owned(),
        };
    }
    leaf.text.to_owned()
}

/// Printed text of a comment: no trailing whitespace on line comments, `\n`
/// line breaks inside block comments (the output newline is applied later).
fn comment_text(leaf: &Leaf<'_>) -> String {
    if leaf.kind == kind::LINE_COMMENT { leaf.text.trim_end().to_owned() } else { leaf.text.replace("\r\n", "\n") }
}

/// `Subject   To :` -> `Subject To:`.
fn collapse_whitespace(text: &str) -> String {
    let joined = text.split_whitespace().collect::<Vec<_>>().join(" ");
    joined.replace(" :", ":")
}

fn apply_case(text: &str, case: KeywordCase) -> String {
    match case {
        KeywordCase::Preserve => text.to_owned(),
        KeywordCase::Lower => text.to_lowercase(),
        KeywordCase::Upper => text.to_uppercase(),
        KeywordCase::Title => {
            let mut out = String::with_capacity(text.len());
            let mut word_start = true;
            for c in text.chars() {
                if word_start {
                    out.extend(c.to_uppercase());
                } else {
                    out.extend(c.to_lowercase());
                }
                word_start = matches!(c, ' ' | '-');
            }
            out
        }
    }
}

/// Token identity for the safety check: keywords compared case-insensitively,
/// operator aliases and comment whitespace normalised.
fn canonical(leaf: &Leaf<'_>) -> String {
    if leaf.is_comment() {
        return comment_text(leaf);
    }
    token_text(leaf, KeywordCase::Lower)
}

fn width(text: &str) -> usize {
    text.chars().count()
}

/// Column after appending `text` at column `col`.
fn advance(col: usize, text: &str) -> usize {
    match text.rfind('\n') {
        Some(i) => width(&text[i + 1..]),
        None => col + width(text),
    }
}

/// Render pieces starting at column `indent`; continuation lines at
/// `continuation`. Returns the text (with `\n`) and the column of the first
/// comparison operator when it is on the first line.
fn render_pieces(pieces: &[Piece], indent: usize, continuation: usize, line_width: usize, align: Option<usize>) -> (String, Option<usize>) {
    debug_assert!(!pieces.is_empty(), "a unit has at least one token");
    let mut out = " ".repeat(indent);
    let mut col = indent;
    let mut first_line = true;
    let mut op_col = None;
    let mut i = 0;
    while i < pieces.len() {
        // A group runs to the next break opportunity: it is never split.
        let mut group = pieces[i].text.clone();
        let mut j = i + 1;
        while j < pieces.len() && !pieces[j].break_before && !pieces[j].align && !pieces[j - 1].hard_break_after {
            if !pieces[j].glue {
                group.push(' ');
            }
            group.push_str(&pieces[j].text);
            j += 1;
        }
        let head = &pieces[i];
        if i == 0 {
            out.push_str(&group);
            col = advance(col, &group);
        } else if pieces[i - 1].hard_break_after
            || (head.break_before && col > continuation && col + 1 + width(group.split('\n').next().unwrap_or("")) > line_width)
        {
            out.push('\n');
            out.push_str(&" ".repeat(continuation));
            out.push_str(&group);
            col = advance(continuation, &group);
            first_line = false;
        } else {
            let mut pad = usize::from(!head.glue);
            if head.align && first_line {
                if let Some(target) = align {
                    pad = pad.max(target.saturating_sub(col));
                }
                op_col = Some(col + pad);
            }
            out.push_str(&" ".repeat(pad));
            out.push_str(&group);
            col = advance(col + pad, &group);
        }
        if group.contains('\n') {
            first_line = false;
        }
        i = j;
    }
    (out, op_col)
}

/// Whether an entry starting a line would be read as a section keyword.
fn starts_like_keyword(leaves: &[Leaf<'_>]) -> bool {
    let mut sig = leaves.iter().filter(|l| !l.is_comment());
    let Some(first) = sig.next() else { return false };
    let labelled = sig.next().is_some_and(|l| !l.named && matches!(l.text, ":" | "::"));
    first.named && !labelled && LINE_START_KEYWORDS.iter().any(|k| first.text.eq_ignore_ascii_case(k))
}

/// Whether `leaves` continue the previous constraint `prev`: tree-sitter may
/// split `-3 <= x - y <= 8` after `x`, while upstream (newline-insensitive)
/// reads one ranged constraint. Keeping them on one line shows that reading.
fn continues_constraint(prev: &[Leaf<'_>], leaves: &[Leaf<'_>]) -> bool {
    let first = leaves.iter().find(|l| !l.is_comment());
    let last = prev.iter().rev().find(|l| !l.is_comment());
    first.is_some_and(|l| !l.named && matches!(l.text, "+" | "-"))
        && last.is_some_and(|l| matches!(l.parent, kind::TERM | kind::LINEAR_EXPRESSION) || l.text == "]")
}

fn render(source: &str, leaves: &[Leaf<'_>], units: &[Unit], settings: &FormatSettings, newline: &str) -> Option<Layout> {
    let case = settings.keyword_case;
    let mut unit_pieces: Vec<Vec<Piece>> = units.iter().map(|u| pieces(&leaves[u.leaves.clone()], case)).collect();

    let entry_indent = |u: &Unit| if u.node_kind == kind::SOS_ENTRY { settings.indent * 2 } else { settings.indent };
    let mut indents = vec![0; units.len()];
    let mut next_indent = 0;
    for (k, unit) in units.iter().enumerate().rev() {
        indents[k] = match unit.kind {
            UnitKind::Header => 0,
            UnitKind::Entry => entry_indent(unit),
            // Own-line comments take the indent of what follows them.
            UnitKind::Comment => next_indent,
        };
        if unit.kind != UnitKind::Comment {
            next_indent = indents[k];
        }
    }

    // Alignment target per section: the widest operator column.
    let mut targets: Vec<(usize, usize)> = Vec::new();
    if settings.align_operators {
        for (k, unit) in units.iter().enumerate() {
            if unit.kind != UnitKind::Entry || !ALIGNED_SECTIONS.contains(&unit.section) {
                continue;
            }
            let (_, col) = render_pieces(&unit_pieces[k], indents[k], indents[k] + settings.indent, settings.line_width, None);
            if let Some(col) = col {
                match targets.iter_mut().find(|(id, _)| *id == unit.section_id) {
                    Some((_, target)) => *target = (*target).max(col),
                    None => targets.push((unit.section_id, col)),
                }
            }
        }
    }

    let mut text = String::with_capacity(source.len() + source.len() / 8);
    let mut placed = Vec::with_capacity(units.len());
    let mut line_start = 0;
    for (k, unit) in units.iter().enumerate() {
        let continued = k > 0
            && unit.kind == UnitKind::Entry
            && units[k - 1].kind == UnitKind::Entry
            && units[k - 1].section_id == unit.section_id
            && CONSTRAINT_SECTIONS.contains(&unit.section)
            && continues_constraint(&leaves[units[k - 1].leaves.clone()], &leaves[unit.leaves.clone()]);
        if continued && let Some(operand) = unit_pieces[k].get_mut(1) {
            // The leading sign is binary here: `x - y`, not `x -y`.
            operand.glue = false;
        }
        let joined = continued || (k > 0 && unit.kind == UnitKind::Entry && starts_like_keyword(&leaves[unit.leaves.clone()]));
        if k > 0 {
            let prev = &units[k - 1];
            let gap = &source[prev.end..unit.start];
            if unit.kind == UnitKind::Comment && !gap.contains('\n') {
                text.push(' ');
            } else if joined {
                // Only a code token may precede it on its line: after a comment
                // it would still start a line.
                if prev.kind == UnitKind::Comment {
                    return None;
                }
                text.push(' ');
            } else {
                text.push_str(newline);
                if gap.matches('\n').count() >= 2 && prev.kind != UnitKind::Header {
                    text.push_str(newline);
                }
                line_start = text.len();
            }
        }
        let indent = if text.len() == line_start { indents[k] } else { 0 };
        let align = targets.iter().find(|(id, _)| *id == unit.section_id).map(|&(_, t)| t);
        let body = match unit.kind {
            UnitKind::Entry => {
                render_pieces(&unit_pieces[k], indent, indents[k] + settings.indent, settings.line_width, align.filter(|_| !joined)).0
            }
            UnitKind::Header | UnitKind::Comment => render_pieces(&unit_pieces[k], indent, indent, usize::MAX, None).0,
        };
        if newline == "\n" {
            text.push_str(&body);
        } else {
            text.push_str(&body.replace('\n', newline));
        }
        placed.push(Placed {
            kind: unit.kind,
            src: unit.start..unit.end,
            out_line_start: line_start,
            out_end: text.len(),
            indent: indents[k],
        });
        if let Some(i) = text[line_start..].rfind('\n') {
            line_start += i + 1;
        }
    }
    text.push_str(newline);
    Some(Layout { text, units: placed })
}
