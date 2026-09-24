//! Rename and prepareRename for variables and constraint/objective/SOS names.
//!
//! A new name must lex as a single identifier under the upstream Logos lexer
//! and must not be read as a keyword at any renamed site (see
//! `rust/src/lexer.rs`, `Lexer::resolve_keyword`). As a final safety net the
//! edits are applied to a copy of each document, which is reparsed and
//! reindexed; the rename is refused if that introduces syntax errors or
//! changes how the renamed symbol is read.

use std::cmp::Reverse;
use std::collections::HashMap;
use std::ops::Range;
use std::sync::Arc;

use tower_lsp_server::ls_types::{Position, PrepareRenameResponse, TextEdit, WorkspaceEdit};
use tree_sitter::Tree;

use crate::document::Document;
use crate::index::{EntityKind, Namespace, Role, Section, Symbol, SymbolIndex};
use crate::syntax::{self, kind};

/// Special characters allowed anywhere in a name (upstream identifier regex).
const NAME_SPECIALS: &str = "!#$%&(),.;?@{}~'[]";

/// Single-word section keywords: keywords only as the first token of a line
/// that does not continue an expression and is not followed by `:` / `::`.
const SECTION_WORDS: &[&str] = &[
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
];

/// Objective sense words: keywords only as the first token of the file.
const SENSE_WORDS: &[&str] = &["minimize", "minimise", "minimum", "min", "maximize", "maximise", "maximum", "max"];

/// Multi-word keywords as `(first word, second word prefixes)`. The upstream
/// lexer takes the longest match, so `gen consumption` lexes as `gen cons`
/// followed by `umption`: the second word only has to start with the prefix.
const MULTI_WORD: &[(&str, &[&str])] = &[
    ("subject", &["to"]),
    ("such", &["that"]),
    ("lazy", &["constraints"]),
    ("user", &["cuts"]),
    ("general", &["constr"]),
    ("gen", &["cons"]),
];

/// Tokens after which the upstream lexer expects an operand, so a following
/// word is never a section keyword.
const CONTINUES_EXPRESSION: &[&str] = &["+", "-", ":", "::", "<=", "=<", ">=", "=>", "<", ">", "=", "->", "[", "^", "*", "/"];

/// Leaf kinds the upstream lexer reads as `Token::Identifier`.
const NAME_KINDS: &[&str] =
    &[kind::IDENTIFIER, kind::OBJECTIVE_NAME, kind::ATTRIBUTE_NAME, kind::CONSTRAINT_NAME, kind::FUNCTION_NAME, kind::SOS_NAME];

/// What a rename applies to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Target {
    /// Every occurrence of a variable in the document.
    Variable,
    /// Every entity label with the name in a namespace, across the workspace.
    Entity(Namespace),
}

/// Range and placeholder of the renameable symbol at `position`.
///
/// # Errors
/// A user-facing message when the position is not renameable.
pub fn prepare(doc: &Document, position: Position) -> Result<Option<PrepareRenameResponse>, String> {
    let (range, _) = renameable_at(doc, doc.offset(position))?;
    let placeholder = doc.slice(range.clone()).to_owned();
    Ok(Some(PrepareRenameResponse::RangeWithPlaceholder { range: doc.range(range), placeholder }))
}

/// Rename the symbol at `position` to `new_name`. `workspace` holds the other
/// indexed documents (for names shared across files).
///
/// # Errors
/// A user-facing message when the new name is invalid or collides.
pub fn rename(doc: &Document, workspace: &[Arc<Document>], position: Position, new_name: &str) -> Result<Option<WorkspaceEdit>, String> {
    let (range, target) = renameable_at(doc, doc.offset(position))?;
    let old_name = doc.slice(range);
    if new_name == old_name {
        return Ok(None);
    }
    validate_name(new_name)?;

    let mut changes = HashMap::new();
    match target {
        Target::Variable => {
            if doc.index.variable(new_name).is_some() {
                return Err(format!("A variable named `{new_name}` already exists"));
            }
            let Some(variable) = doc.index.variable(old_name) else {
                return Err(format!("Variable `{old_name}` is not indexed; try again once the document is parsed"));
            };
            let ranges: Vec<Range<usize>> = variable.occurrences.iter().map(|o| o.range.clone()).collect();
            changes.insert(doc.uri.clone(), document_edits(doc, &ranges, old_name, new_name, target)?);
        }
        Target::Entity(namespace) => {
            let others = workspace.iter().map(AsRef::as_ref).filter(|d: &&Document| d.uri != doc.uri);
            for d in std::iter::once(doc).chain(others) {
                let ranges: Vec<Range<usize>> =
                    d.index.entities_named(old_name, namespace).filter_map(|(_, e)| e.name_range.clone()).collect();
                if ranges.is_empty() {
                    continue;
                }
                if let Some((_, existing)) = d.index.entities_named(new_name, namespace).next() {
                    return Err(format!("A {} named `{new_name}` already exists in {}", existing.kind.label(), d.uri.as_str()));
                }
                changes.insert(d.uri.clone(), document_edits(d, &ranges, old_name, new_name, target)?);
            }
            debug_assert!(changes.contains_key(&doc.uri), "the label under the cursor is always renamed");
        }
    }
    Ok(Some(WorkspaceEdit { changes: Some(changes), ..WorkspaceEdit::default() }))
}

/// The renameable name site at `offset`, or why there is none.
fn renameable_at(doc: &Document, offset: usize) -> Result<(Range<usize>, Target), String> {
    match doc.index.symbol_at(offset) {
        Some((range, Symbol::Variable(..))) => Ok((range, Target::Variable)),
        Some((range, Symbol::Entity(entity))) => Ok((range, Target::Entity(doc.index.entities[entity].kind.namespace()))),
        Some((_, Symbol::Attribute(_))) => Err("Objective attribute names (such as `Priority`) cannot be renamed".to_owned()),
        None => Err(not_renameable(doc, offset)),
    }
}

/// Message for a position with no renameable symbol, naming what is there.
fn not_renameable(doc: &Document, offset: usize) -> String {
    const GENERIC: &str = "only variables and constraint, objective or SOS names can be renamed";
    let Some(token) = syntax::token_at(&doc.tree, offset) else {
        return format!("Nothing to rename here: {GENERIC}");
    };
    let text = doc.node_text(token);
    match token.kind() {
        kind::NUMBER => format!("The number `{text}` cannot be renamed: {GENERIC}"),
        kind::INFINITY => format!("`{text}` (infinity) cannot be renamed: {GENERIC}"),
        kind::LINE_COMMENT | kind::BLOCK_COMMENT => format!("Comments cannot be renamed: {GENERIC}"),
        kind::FUNCTION_NAME => format!("The general-constraint function `{text}` cannot be renamed: {GENERIC}"),
        kind::SENSE | kind::SOS_TYPE | kind::END_MARKER => format!("The keyword `{text}` cannot be renamed: {GENERIC}"),
        k if k.ends_with("_keyword") => format!("The keyword `{text}` cannot be renamed: {GENERIC}"),
        _ => format!("Nothing to rename here: {GENERIC}"),
    }
}

/// Check `name` against the upstream identifier regex and the words that
/// always lex as something else.
fn validate_name(name: &str) -> Result<(), String> {
    let Some(first) = name.chars().next() else {
        return Err("The new name must not be empty".to_owned());
    };
    if first.is_ascii_digit() {
        return Err(format!("`{name}` is not a valid LP name: names cannot start with a digit"));
    }
    identifier_error(name).map_or(Ok(()), |reason| Err(format!("`{name}` is not a valid LP name: {reason}")))?;
    if name.eq_ignore_ascii_case("inf") || name.eq_ignore_ascii_case("infinity") {
        return Err(format!("`{name}` is reserved for infinity and cannot be used as a name"));
    }
    if is_number_literal(name) {
        return Err(format!("`{name}` would be read as a number"));
    }
    if name == "[" || name == "]" {
        return Err(format!("`{name}` on its own is a quadratic-block bracket and cannot be used as a name"));
    }
    Ok(())
}

fn is_name_start(c: char) -> bool {
    c.is_ascii_alphabetic() || c == '_' || NAME_SPECIALS.contains(c)
}

fn is_name_continue(c: char) -> bool {
    is_name_start(c) || c.is_ascii_digit() || c == '|'
}

fn is_after_gt(c: char) -> bool {
    is_name_continue(c) && !c.is_ascii_digit() && c != '.'
}

/// Why `name` does not match the upstream identifier regex
/// `START (CONT | -CONT | >AFTER_GT)*`, or `None` when it does.
fn identifier_error(name: &str) -> Option<String> {
    let mut chars = name.chars().peekable();
    let first = chars.next()?;
    if !is_name_start(first) {
        return Some(format!("{first:?} cannot start a name"));
    }
    while let Some(c) = chars.next() {
        match c {
            '-' if chars.peek().is_some_and(|&n| is_name_continue(n)) => {}
            '-' => return Some("`-` must be followed by a letter, digit or name symbol".to_owned()),
            '>' if chars.peek().is_some_and(|&n| is_after_gt(n)) => {}
            '>' => return Some("`>` must be followed by a letter or name symbol (not a digit or `.`)".to_owned()),
            c if is_name_continue(c) => continue,
            c => return Some(format!("{c:?} is not allowed in a name")),
        }
        // The character after `-` / `>` was checked above; consume it.
        debug_assert!(chars.peek().is_some());
        chars.next();
    }
    None
}

/// Whether all of `s` matches the upstream number regex
/// `([0-9]+\.?[0-9]*|[0-9]*\.[0-9]+)([eE][+-]?[0-9]+)?`. Such a name (e.g.
/// `.5`) also matches the identifier regex, but the number token wins.
fn is_number_literal(s: &str) -> bool {
    let bytes = s.as_bytes();
    let digits = |from: usize| bytes.get(from..).map_or(0, |rest| rest.iter().take_while(|b| b.is_ascii_digit()).count());
    let whole = digits(0);
    let mut end = whole;
    let mut fraction = 0;
    if bytes.get(end) == Some(&b'.') {
        fraction = digits(end + 1);
        end += 1 + fraction;
    }
    if whole == 0 && fraction == 0 {
        return false;
    }
    if matches!(bytes.get(end), Some(b'e' | b'E')) {
        let sign = usize::from(matches!(bytes.get(end + 1), Some(b'+' | b'-')));
        let exponent = digits(end + 1 + sign);
        if exponent > 0 {
            end += 1 + sign + exponent;
        }
    }
    end == bytes.len()
}

/// Validate the renamed sites of one document and build its edits.
fn document_edits(
    doc: &Document,
    ranges: &[Range<usize>],
    old_name: &str,
    new_name: &str,
    target: Target,
) -> Result<Vec<TextEdit>, String> {
    debug_assert!(!ranges.is_empty(), "a renamed document has at least one site");
    debug_assert!(ranges.iter().all(|r| &doc.text[r.clone()] == old_name), "every site spells the old name");
    check_keyword_sites(doc, ranges, new_name)?;
    check_reparse(doc, ranges, old_name, new_name, target)?;
    Ok(ranges.iter().map(|r| TextEdit::new(doc.range(r.clone()), new_name.to_owned())).collect())
}

/// Significant leaf tokens (no comments, no zero-width `MISSING` nodes) as
/// `(kind, byte range)`, in document order.
fn leaves(tree: &Tree) -> Vec<(&'static str, Range<usize>)> {
    let mut out = Vec::new();
    let mut cursor = tree.walk();
    loop {
        if cursor.goto_first_child() {
            continue;
        }
        let node = cursor.node();
        if node.start_byte() < node.end_byte() && !matches!(node.kind(), kind::LINE_COMMENT | kind::BLOCK_COMMENT) {
            out.push((node.kind(), node.byte_range()));
        }
        while !cursor.goto_next_sibling() {
            if !cursor.goto_parent() {
                return out;
            }
        }
    }
}

/// Reject `new_name` if the upstream lexer would read it as a keyword at any
/// of `ranges` (mirrors `Lexer::resolve_keyword`).
fn check_keyword_sites(doc: &Document, ranges: &[Range<usize>], new_name: &str) -> Result<(), String> {
    let leaves = leaves(&doc.tree);
    let lower = new_name.to_ascii_lowercase();
    let first_header = leaves.iter().find(|(k, _)| *k == kind::SUBJECT_TO_KEYWORD).map_or(usize::MAX, |(_, r)| r.start);

    for range in ranges {
        let before = leaves.partition_point(|(_, r)| r.start < range.start);
        let after = leaves.partition_point(|(_, r)| r.start < range.end);
        let prev = before.checked_sub(1).map(|i| &leaves[i]);
        let next = leaves.get(after);
        // Only whitespace and comments lie in the gaps, and upstream counts a
        // line break inside a block comment too.
        let at_line_start = prev.is_none_or(|(_, r)| doc.text[r.end..range.start].contains('\n'));
        let prev_kind = prev.map(|(k, _)| *k);
        let next_kind = next.filter(|(_, r)| !doc.text[range.end..r.start].contains('\n')).map(|(k, _)| *k);
        let line_keyword = at_line_start && !prev_kind.is_some_and(|k| CONTINUES_EXPRESSION.contains(&k));

        let reading = match lower.as_str() {
            w if SECTION_WORDS.contains(&w) && line_keyword && !matches!(next_kind, Some(":" | "::")) => Some("a section header"),
            w if SENSE_WORDS.contains(&w) && prev.is_none() => Some("the objective sense"),
            "multi-objective" | "multi-objectives" if prev_kind == Some(kind::SENSE) => Some("the multi-objectives marker"),
            "st" | "s.t." if line_keyword && range.start < first_header => Some("the `Subject To` header"),
            "free" if !at_line_start && prev_kind.is_some_and(|k| NAME_KINDS.contains(&k)) => Some("the `free` bound keyword"),
            "s1" | "s2" if prev_kind == Some(":") && next_kind == Some("::") => Some("an SOS type"),
            _ => None,
        };
        let multi_word = || {
            let joins = |gap: Range<usize>, first: &str, second: &str| {
                !gap.is_empty() && doc.text[gap].bytes().all(|b| b == b' ' || b == b'\t') && forms_multi_word(first, second)
            };
            prev.is_some_and(|(_, r)| joins(r.end..range.start, &doc.text[r.clone()], new_name))
                || next.is_some_and(|(_, r)| joins(range.end..r.start, new_name, &doc.text[r.clone()]))
        };
        let reading = reading.or_else(|| multi_word().then_some("part of a multi-word section header"));
        if let Some(reading) = reading {
            let line = doc.position(range.start).line + 1;
            return Err(format!("`{new_name}` would be read as {reading} on line {line} of {}; choose another name", doc.uri.as_str()));
        }
    }
    Ok(())
}

/// Whether `first second` lexes as a multi-word keyword upstream.
fn forms_multi_word(first: &str, second: &str) -> bool {
    MULTI_WORD.iter().any(|(word, prefixes)| {
        first.eq_ignore_ascii_case(word) && prefixes.iter().any(|p| second.get(..p.len()).is_some_and(|head| head.eq_ignore_ascii_case(p)))
    })
}

/// Number of `ERROR` and `MISSING` nodes in `tree`.
fn error_count(tree: &Tree) -> usize {
    let mut count = 0;
    let mut cursor = tree.walk();
    loop {
        let node = cursor.node();
        count += usize::from(node.is_error() || node.is_missing());
        if node.has_error() && cursor.goto_first_child() {
            continue;
        }
        while !cursor.goto_next_sibling() {
            if !cursor.goto_parent() {
                return count;
            }
        }
    }
}

/// Apply the edits to a copy, reparse and reindex; reject if syntax errors
/// appear or the renamed symbol (or the overall shape) is read differently.
fn check_reparse(doc: &Document, ranges: &[Range<usize>], old_name: &str, new_name: &str, target: Target) -> Result<(), String> {
    let mut text = doc.text.clone();
    let mut sorted = ranges.to_vec();
    sorted.sort_by_key(|r| Reverse(r.start));
    debug_assert!(sorted.windows(2).all(|w| w[1].end <= w[0].start), "renamed sites never overlap");
    for range in sorted {
        text.replace_range(range, new_name);
    }
    let tree = syntax::parse(&text, None);
    if error_count(&tree) > error_count(&doc.tree) {
        return Err(format!("Renaming to `{new_name}` would introduce syntax errors in {}", doc.uri.as_str()));
    }
    let index = SymbolIndex::build(&tree, &text);
    let same_symbol = match target {
        Target::Variable => {
            let roles = |index: &SymbolIndex, name: &str| -> Option<Vec<Role>> {
                index.variable(name).map(|v| v.occurrences.iter().map(|o| o.role).collect())
            };
            roles(&doc.index, old_name) == roles(&index, new_name) && index.variable(old_name).is_none()
        }
        Target::Entity(namespace) => {
            let kinds = |index: &SymbolIndex, name: &str| -> Vec<(EntityKind, Section)> {
                index.entities_named(name, namespace).map(|(_, e)| (e.kind, e.section)).collect()
            };
            kinds(&doc.index, old_name) == kinds(&index, new_name) && index.entities_named(old_name, namespace).next().is_none()
        }
    };
    let same_shape = index.variables.len() == doc.index.variables.len()
        && index.entities.len() == doc.index.entities.len()
        && index.sections.len() == doc.index.sections.len();
    if !(same_symbol && same_shape) {
        return Err(format!("Renaming to `{new_name}` would change how {} is parsed; choose another name", doc.uri.as_str()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use tower_lsp_server::ls_types::Uri;

    use super::*;
    use crate::position::Encoding;

    fn doc_at(uri: &str, text: &str) -> Document {
        Document::new(uri.parse().unwrap(), text.to_owned(), 0, Encoding::Utf16)
    }

    fn doc(text: &str) -> Document {
        doc_at("file:///a.lp", text)
    }

    /// Position of the `nth` (0-based) match of `needle`, one byte in.
    fn at(doc: &Document, needle: &str, nth: usize) -> Position {
        let offset = doc.text.match_indices(needle).nth(nth).unwrap().0;
        doc.position(offset + 1)
    }

    /// Apply the edits for `doc` from `edit` and return the new text.
    fn applied(doc: &Document, edit: &WorkspaceEdit) -> String {
        let mut edits: Vec<(Range<usize>, String)> =
            edit.changes.as_ref().unwrap()[&doc.uri].iter().map(|e| (doc.byte_range(e.range), e.new_text.clone())).collect();
        edits.sort_by_key(|(r, _)| Reverse(r.start));
        let mut text = doc.text.clone();
        for (range, new) in edits {
            text.replace_range(range, &new);
        }
        text
    }

    fn rename_err(doc: &Document, position: Position, new_name: &str) -> String {
        rename(doc, &[], position, new_name).unwrap_err()
    }

    const MODEL: &str = "Minimize\n obj: 3 x + 2 y\nSubject To\n c1: x + y >= 1\n c2: [ x ^ 2 ] <= 4\nBounds\n x <= 10\nGenerals\n x\nSOS\n s1: S1 :: x : 1 y : 2\nEnd\n";

    #[test]
    fn prepare_returns_range_and_placeholder() {
        let d = doc(MODEL);
        let Some(PrepareRenameResponse::RangeWithPlaceholder { range, placeholder }) = prepare(&d, at(&d, "c1", 0)).unwrap() else {
            panic!("expected a range with placeholder");
        };
        assert_eq!(placeholder, "c1");
        assert_eq!(d.byte_range(range), MODEL.find("c1").unwrap()..MODEL.find("c1").unwrap() + 2);
    }

    #[test]
    fn prepare_rejects_numbers_keywords_and_attributes() {
        let d = doc(MODEL);
        let err = prepare(&d, at(&d, "10", 0)).unwrap_err();
        assert!(err.contains("number `10` cannot be renamed"), "{err}");
        let err = prepare(&d, at(&d, "Bounds", 0)).unwrap_err();
        assert!(err.contains("keyword `Bounds` cannot be renamed"), "{err}");
        let err = prepare(&d, at(&d, "Minimize", 0)).unwrap_err();
        assert!(err.contains("cannot be renamed"), "{err}");

        let m = doc("max multi-objectives\n o1: Priority=2 x\nst\n c: x <= 1\nend\n");
        let err = prepare(&m, at(&m, "Priority", 0)).unwrap_err();
        assert!(err.contains("attribute names"), "{err}");
    }

    #[test]
    fn renames_variable_across_roles() {
        let d = doc(MODEL);
        let edit = rename(&d, &[], at(&d, "x", 0), "flow").unwrap().unwrap();
        assert_eq!(edit.changes.as_ref().unwrap().len(), 1);
        assert_eq!(
            applied(&d, &edit),
            "Minimize\n obj: 3 flow + 2 y\nSubject To\n c1: flow + y >= 1\n c2: [ flow ^ 2 ] <= 4\nBounds\n flow <= 10\nGenerals\n flow\nSOS\n s1: S1 :: flow : 1 y : 2\nEnd\n"
        );
    }

    #[test]
    fn same_name_is_a_no_op() {
        let d = doc(MODEL);
        assert_eq!(rename(&d, &[], at(&d, "y", 0), "y"), Ok(None));
    }

    #[test]
    fn renames_constraint_name_only() {
        let d = doc("min\n c1: x\nst\n c1: x >= 1\n c2: x <= 2\nend\n");
        let edit = rename(&d, &[], at(&d, "c1", 1), "limit").unwrap().unwrap();
        // The objective `c1` lives in another namespace.
        assert_eq!(applied(&d, &edit), "min\n c1: x\nst\n limit: x >= 1\n c2: x <= 2\nend\n");
    }

    #[test]
    fn renames_objective_across_workspace() {
        let a = doc_at("file:///a.lp", "min\n cost: x\nst\n c: x >= 1\nend\n");
        let b = Arc::new(doc_at("file:///b.lp", "max\n cost: y\nst\n cost: y <= 1\nend\n"));
        let c = Arc::new(doc_at("file:///c.lp", "min\n other: z\nst\n cost: z >= 1\nend\n"));
        let edit = rename(&a, &[b.clone(), c.clone()], at(&a, "cost", 0), "spend").unwrap().unwrap();
        let changes = edit.changes.as_ref().unwrap();
        let mut uris: Vec<&Uri> = changes.keys().collect();
        uris.sort();
        assert_eq!(uris, [&a.uri, &b.uri]);
        assert_eq!(applied(&a, &edit), "min\n spend: x\nst\n c: x >= 1\nend\n");
        // Only the objective label changes, not the constraint named `cost`.
        assert_eq!(applied(&b, &edit), "max\n spend: y\nst\n cost: y <= 1\nend\n");
    }

    #[test]
    fn rejects_section_keyword_at_line_start() {
        let d = doc("min\n obj: x + y\nst\n c: x + y >= 1\ngenerals\n x\n y\nend\n");
        let err = rename_err(&d, at(&d, "y", 0), "bin");
        assert!(err.contains("section header on line 7"), "{err}");
        assert!(rename_err(&d, at(&d, "y", 0), "END").contains("section header"));
    }

    #[test]
    fn accepts_section_keyword_mid_line_or_as_label() {
        let d = doc("min\n obj: x + y\nst\n c: x + y >= 1\ngenerals\n x y\nend\n");
        let edit = rename(&d, &[], at(&d, "y", 0), "bin").unwrap().unwrap();
        assert_eq!(applied(&d, &edit), "min\n obj: x + bin\nst\n c: x + bin >= 1\ngenerals\n x bin\nend\n");

        // A label is followed by `:`, so it is never a section header.
        let edit = rename(&d, &[], at(&d, "c:", 0), "bounds").unwrap().unwrap();
        assert!(applied(&d, &edit).contains("\n bounds: x + y >= 1\n"));
    }

    #[test]
    fn rejects_context_keywords() {
        // `free` right after a name on the same line.
        let d = doc("min\n obj: x + y\nst\n c: x + y >= 1\ngenerals\n x y\nend\n");
        assert!(rename_err(&d, at(&d, "y", 0), "free").contains("`free` bound keyword"));
        // `st` at line start before the header.
        let d = doc("min\n obj: x\n + y\n\\ note\n z: w\nst\n c: x >= 1\nend\n");
        let edit = rename(&d, &[], at(&d, "y", 0), "st").unwrap().unwrap();
        assert!(applied(&d, &edit).contains(" + st\n"), "after `+` it continues the expression");
        assert!(rename_err(&d, at(&d, "z", 0), "ST").contains("`Subject To` header"));
        // Multi-word headers match by prefix of the second word.
        let d = doc("min\n obj: x + y + consumption\nst\n c: x >= 1\ngenerals\n x y consumption\nend\n");
        assert!(rename_err(&d, at(&d, "y", 0), "gen").contains("multi-word"));
    }

    #[test]
    fn rejects_invalid_names() {
        let d = doc(MODEL);
        let pos = at(&d, "y", 0);
        assert!(rename_err(&d, pos, "2x").contains("cannot start with a digit"));
        assert!(rename_err(&d, pos, "INF").contains("reserved for infinity"));
        assert!(rename_err(&d, pos, "infinity").contains("reserved for infinity"));
        assert!(rename_err(&d, pos, "a b").contains("' ' is not allowed"));
        assert!(rename_err(&d, pos, "a\\b").contains("is not allowed"));
        assert!(rename_err(&d, pos, "a-").contains("`-` must be followed"));
        assert!(rename_err(&d, pos, "a>1").contains("`>` must be followed"));
        assert!(rename_err(&d, pos, "|a").contains("cannot start a name"));
        assert!(rename_err(&d, pos, ".5e-3").contains("read as a number"));
        assert!(rename_err(&d, pos, "").contains("must not be empty"));
        // Valid unusual names from the upstream regex.
        for name in ["x[1]", "a-b", "ArcFlow%>%[0]", ".x", "_", "a|b", ".5x", "min"] {
            assert!(validate_name(name).is_ok(), "{name}");
        }
    }

    #[test]
    fn rejects_collisions() {
        let d = doc(MODEL);
        assert!(rename_err(&d, at(&d, "y", 0), "x").contains("variable named `x` already exists"));
        assert!(rename_err(&d, at(&d, "c1", 0), "s1").contains("SOS set named `s1` already exists"));
        // Objectives are a separate namespace, so this is fine.
        assert!(rename(&d, &[], at(&d, "c1", 0), "obj").is_ok());
    }

    #[test]
    fn rejects_collision_in_other_document() {
        let a = doc_at("file:///a.lp", "min\n cost: x\nst\n c: x >= 1\nend\n");
        let b = Arc::new(doc_at("file:///b.lp", "min\n multi: y\n cost: y\nst\n c: y >= 1\nend\n"));
        let err = rename(&a, &[b], at(&a, "cost", 0), "multi").unwrap_err();
        assert!(err.contains("objective named `multi` already exists in file:///b.lp"), "{err}");
    }

    #[test]
    fn reparse_catches_token_merges() {
        // `2x` renamed to `2e5` would merge into a number.
        let d = doc("min\n obj: 2x\nst\n c: x >= 1\nend\n");
        let err = rename_err(&d, at(&d, "x", 0), "e5");
        assert!(err.contains("would change how file:///a.lp is parsed"), "{err}");
    }
}
