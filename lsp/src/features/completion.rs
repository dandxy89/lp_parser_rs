//! Context-aware completion.
//!
//! The context comes from the text around the cursor (the section header
//! above it and the tokens before it on its line) rather than the syntax
//! tree, which is full of `ERROR` nodes while a line is being typed. The tree
//! is only consulted to tell whether a position is inside a comment.

use std::ops::Range;

use serde_json::{Value, json};
use tower_lsp_server::ls_types::{
    CompletionItem, CompletionItemKind, CompletionItemLabelDetails, CompletionTextEdit, Documentation, InsertTextFormat, MarkupContent,
    MarkupKind, Position, TextEdit,
};

use super::docs;
use crate::document::Document;
use crate::index::{Role, Variable};
use crate::syntax::kind;

/// Completion items at `position`.
#[must_use]
pub fn complete(doc: &Document, position: Position) -> Vec<CompletionItem> {
    let offset = doc.offset(position);
    let text = &doc.text;
    let line_start = line_start(text, offset);
    let word = word_start(text, line_start, offset)..offset;
    let before = &text[line_start..word.start];
    if before.contains('\\') || in_comment(doc, offset) {
        return Vec::new();
    }
    let place = place(doc, word.start);
    let rest = lex(&text[place.rest_start..word.start]);
    let ends_with_space = text[..word.start].ends_with([' ', '\t']);
    let mut out = Completions { doc, replace: word.clone(), items: Vec::new() };

    if before.trim().is_empty() {
        out.section_keywords(place.section == Section::Start);
    }
    match place.section {
        Section::Start | Section::End => {}
        Section::Objective => {
            let after_label = match rest.as_slice() {
                [Tok::Name, Tok::Colon, tail @ ..] => Some(tail),
                _ => None,
            };
            if place.multi && after_label.is_some_and(only_attributes) {
                out.attributes();
            }
            out.variables();
        }
        Section::Constraints => constraint_context(&mut out, strip_indicator(strip_label(&rest)), ends_with_space),
        Section::General => match strip_label(&rest) {
            [] | [.., Tok::LParen | Tok::Comma] => out.variables(),
            [Tok::Name, Tok::Op("=")] => out.functions(),
            _ => {}
        },
        Section::Bounds => match rest.as_slice() {
            [] | [Tok::Num, Tok::Op(_)] | [Tok::Sign, Tok::Num, Tok::Op(_)] => out.variables(),
            [Tok::Name] => {
                out.keyword("free");
                out.operators();
            }
            [Tok::Num, Tok::Op(_), Tok::Name] | [Tok::Sign, Tok::Num, Tok::Op(_), Tok::Name] => out.operators(),
            _ => {}
        },
        Section::Types => out.variables(),
        Section::Sos => match rest.as_slice() {
            [Tok::Name, Tok::Colon] => {
                out.keyword("s1");
                out.keyword("s2");
            }
            [.., Tok::Colon | Tok::Name] => {}
            _ => out.variables(),
        },
    }
    out.items
}

/// Fill in documentation for a completion item.
#[must_use]
pub fn resolve(mut item: CompletionItem) -> CompletionItem {
    let markdown = item.data.as_ref().and_then(|d| d.get("doc")).and_then(Value::as_str).and_then(docs::describe);
    if let Some(value) = markdown {
        item.documentation = Some(Documentation::MarkupContent(MarkupContent { kind: MarkupKind::Markdown, value }));
    }
    item
}

/// Constraint sections: variables in expressions, operators after a complete
/// expression, nothing in a right-hand side.
fn constraint_context(out: &mut Completions<'_>, rest: &[Tok], ends_with_space: bool) {
    let ops: Vec<usize> = rest.iter().enumerate().filter(|(_, t)| matches!(t, Tok::Op(_))).map(|(i, _)| i).collect();
    let expression = match ops.as_slice() {
        [] => rest,
        // Flipped or ranged: a numeric left-hand side, then the expression.
        [op] if rest[..*op].iter().all(|t| matches!(t, Tok::Num | Tok::Sign)) => &rest[op + 1..],
        _ => return,
    };
    let can_close = ops.len() < 2 && ends_with_space;
    match expression.last() {
        Some(Tok::Name | Tok::Close) if can_close => out.operators(),
        Some(Tok::Num) => {
            out.variables();
            if can_close {
                out.operators();
            }
        }
        None | Some(Tok::Sign | Tok::Colon | Tok::Arrow) => out.variables(),
        _ => {}
    }
}

const fn strip_label(tokens: &[Tok]) -> &[Tok] {
    match tokens {
        [Tok::Name, Tok::Colon | Tok::DoubleColon, tail @ ..] => tail,
        _ => tokens,
    }
}

fn strip_indicator(tokens: &[Tok]) -> &[Tok] {
    match tokens {
        [Tok::Name, Tok::Op("="), Tok::Num, Tok::Arrow, tail @ ..] => tail,
        _ => tokens,
    }
}

/// `Priority=2 Weight=1 ...`: only attribute assignments so far.
fn only_attributes(tokens: &[Tok]) -> bool {
    let mut rest = tokens;
    loop {
        rest = match rest {
            [] => return true,
            [Tok::Name, Tok::Op("="), Tok::Sign, Tok::Num, tail @ ..] | [Tok::Name, Tok::Op("="), Tok::Num, tail @ ..] => tail,
            _ => return false,
        };
    }
}

struct Completions<'a> {
    doc: &'a Document,
    replace: Range<usize>,
    items: Vec<CompletionItem>,
}

impl Completions<'_> {
    fn push(&mut self, label: &str, kind: CompletionItemKind, insert: &str, snippet: bool, doc_key: Option<String>) -> &mut CompletionItem {
        let edit = TextEdit { range: self.doc.range(self.replace.clone()), new_text: insert.to_owned() };
        self.items.push(CompletionItem {
            label: label.to_owned(),
            kind: Some(kind),
            text_edit: Some(CompletionTextEdit::Edit(edit)),
            insert_text_format: Some(if snippet { InsertTextFormat::SNIPPET } else { InsertTextFormat::PLAIN_TEXT }),
            data: doc_key.map(|key| json!({ "doc": key })),
            ..CompletionItem::default()
        });
        self.items.last_mut().expect("an item was just pushed")
    }

    /// Section headers (the objective sense only before any section), each
    /// as a plain keyword and a snippet with an indented first line.
    fn section_keywords(&mut self, sense: bool) {
        let senses = ["minimize", "maximize"];
        for keyword in docs::KEYWORDS.iter().filter(|k| (k.snippet.is_some() || k.id == "end") && senses.contains(&k.id) == sense) {
            let key = format!("keyword:{}", keyword.id);
            self.push(keyword.label, CompletionItemKind::KEYWORD, keyword.label, false, Some(key.clone()));
            if let Some(snippet) = keyword.snippet {
                let item = self.push(keyword.label, CompletionItemKind::SNIPPET, snippet, true, Some(key));
                item.label_details = Some(CompletionItemLabelDetails { detail: None, description: Some("snippet".to_owned()) });
            }
        }
    }

    fn keyword(&mut self, id: &str) {
        let Some(keyword) = docs::keyword(id) else {
            debug_assert!(false, "unknown keyword id {id}");
            return;
        };
        let insert = if keyword.node_kind == kind::SOS_TYPE { format!("{} :: ", keyword.label) } else { keyword.label.to_owned() };
        self.push(keyword.label, CompletionItemKind::KEYWORD, &insert, false, Some(format!("keyword:{id}")));
    }

    fn variables(&mut self) {
        let doc = self.doc;
        for var in &doc.index.variables {
            // Skip the name being typed, which the index already holds.
            if var.occurrences.iter().all(|o| o.range == self.replace) {
                continue;
            }
            let item = self.push(&var.name, CompletionItemKind::VARIABLE, &var.name, false, None);
            item.detail = Some(variable_detail(var));
        }
    }

    fn functions(&mut self) {
        for function in docs::FUNCTIONS {
            let item = self.push(
                function.name,
                CompletionItemKind::FUNCTION,
                &format!("{} ( $0 )", function.name),
                true,
                Some(format!("function:{}", function.name)),
            );
            item.detail = Some(function.signature().0);
        }
    }

    fn attributes(&mut self) {
        for (name, _) in docs::ATTRIBUTES {
            self.push(name, CompletionItemKind::PROPERTY, &format!("{name}="), false, Some(format!("attribute:{name}")));
        }
    }

    fn operators(&mut self) {
        for (op, _) in docs::OPERATORS {
            self.push(op, CompletionItemKind::OPERATOR, op, false, Some(format!("operator:{op}")));
        }
    }
}

fn variable_detail(var: &Variable) -> String {
    let has = |role: Role| var.occurrences.iter().any(|o| o.role == role);
    let kind = if has(Role::Binaries) {
        "binary"
    } else if has(Role::Generals) || has(Role::Integers) {
        "integer"
    } else if has(Role::SemiContinuous) {
        "semi-continuous"
    } else {
        "continuous"
    };
    format!("{kind} variable")
}

/// Section the cursor is in, as far as completion cares.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Section {
    /// Before the objective sense.
    Start,
    /// Objective(s).
    Objective,
    /// `Subject To`, `Lazy Constraints`, `User Cuts`.
    Constraints,
    /// `General Constraints`.
    General,
    /// `Bounds`.
    Bounds,
    /// `Generals`, `Integers`, `Binaries`, `Semi-Continuous`.
    Types,
    /// `SOS`.
    Sos,
    /// After `End`.
    End,
}

/// Where a position sits.
pub(crate) struct Place {
    /// Enclosing section.
    pub section: Section,
    /// Whether the objective is a multi-objective one.
    pub multi: bool,
    /// Start of the position's line, or the end of a section header on it.
    pub rest_start: usize,
}

/// Section containing `offset`, from the nearest header at the start of a
/// line at or above it. Headers inside comments are skipped.
pub(crate) fn place(doc: &Document, offset: usize) -> Place {
    let text = &doc.text;
    let start = line_start(text, offset);
    let found = |keyword: &docs::Keyword, line: &str, rest_start: usize| {
        let section = match keyword.id {
            "minimize" | "maximize" => Section::Objective,
            "subject_to" | "lazy_constraints" | "user_cuts" => Section::Constraints,
            "general_constraints" => Section::General,
            "bounds" => Section::Bounds,
            "sos" => Section::Sos,
            "end" => Section::End,
            _ => Section::Types,
        };
        let multi = section == Section::Objective
            && (line.to_ascii_lowercase().contains("multi-objective")
                || doc.tree.root_node().children(&mut doc.tree.walk()).any(|c| c.kind() == kind::MULTI_OBJECTIVES_KEYWORD));
        Place { section, multi, rest_start }
    };
    let current = &text[start..offset];
    if let Some((keyword, len)) = docs::section_header(current)
        && !in_comment(doc, start + len)
    {
        return found(keyword, current, start + len);
    }
    let mut end = start;
    while end > 0 {
        let line_begin = line_start(text, end - 1);
        let line = &text[line_begin..end - 1];
        let code = line.split('\\').next().unwrap_or_default();
        if let Some((keyword, len)) = docs::section_header(code)
            && !in_comment(doc, line_begin + len)
        {
            return found(keyword, line, start);
        }
        end = line_begin;
    }
    Place { section: Section::Start, multi: false, rest_start: start }
}

/// Whether `offset` is inside (or at the end of) a comment.
pub(crate) fn in_comment(doc: &Document, offset: usize) -> bool {
    let root = doc.tree.root_node();
    [offset, offset.saturating_sub(1)].into_iter().any(|at| {
        let mut node = root.descendant_for_byte_range(at, at);
        while let Some(n) = node {
            let inside = match n.kind() {
                kind::LINE_COMMENT => n.start_byte() < offset && offset <= n.end_byte(),
                kind::BLOCK_COMMENT => n.start_byte() < offset && offset < n.end_byte(),
                _ => false,
            };
            if inside {
                return true;
            }
            node = n.parent();
        }
        false
    })
}

/// Byte offset of the start of the line containing `offset`.
pub(crate) fn line_start(text: &str, offset: usize) -> usize {
    debug_assert!(offset <= text.len());
    text[..offset].rfind('\n').map_or(0, |i| i + 1)
}

/// Start of the name-like word ending at `offset` (not before `floor`).
pub(crate) fn word_start(text: &str, floor: usize, offset: usize) -> usize {
    debug_assert!(floor <= offset && offset <= text.len());
    let bytes = text.as_bytes();
    let mut i = offset;
    while i > floor {
        let b = bytes[i - 1];
        // `-` belongs to a name only between two name characters.
        let part = if b == b'-' { i < offset && i - 1 > floor && is_word_byte(bytes[i - 2]) } else { is_word_byte(b) };
        if !part {
            break;
        }
        i -= 1;
    }
    i
}

/// Name characters, less the punctuation used around general-constraint
/// arguments.
const fn is_word_byte(b: u8) -> bool {
    docs::is_name_byte(b) && !matches!(b, b'(' | b')' | b',' | b'-')
}

/// A token of the line before the cursor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tok {
    Name,
    Num,
    Op(&'static str),
    Sign,
    Colon,
    DoubleColon,
    Arrow,
    LParen,
    Comma,
    /// `)` or `]`: closes something that can end an expression.
    Close,
    Other,
}

/// Rough tokeniser for the text before the cursor on one line.
fn lex(text: &str) -> Vec<Tok> {
    const PUNCT: &[(&str, Tok)] = &[
        ("->", Tok::Arrow),
        ("::", Tok::DoubleColon),
        ("<=", Tok::Op("<=")),
        ("=<", Tok::Op("<=")),
        (">=", Tok::Op(">=")),
        ("=>", Tok::Op(">=")),
        (":", Tok::Colon),
        ("<", Tok::Op("<")),
        (">", Tok::Op(">")),
        ("=", Tok::Op("=")),
        ("+", Tok::Sign),
        ("-", Tok::Sign),
        ("(", Tok::LParen),
        (",", Tok::Comma),
        (")", Tok::Close),
        ("]", Tok::Close),
    ];
    let bytes = text.as_bytes();
    let mut tokens = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b.is_ascii_whitespace() {
            i += 1;
        } else if let Some((p, tok)) = PUNCT.iter().find(|(p, _)| text[i..].starts_with(p)) {
            tokens.push(*tok);
            i += p.len();
        } else if b.is_ascii_digit() || (b == b'.' && bytes.get(i + 1).is_some_and(u8::is_ascii_digit)) {
            i += 1;
            while i < bytes.len()
                && (bytes[i].is_ascii_alphanumeric()
                    || bytes[i] == b'.'
                    || (matches!(bytes[i], b'+' | b'-') && matches!(bytes[i - 1], b'e' | b'E')))
            {
                i += 1;
            }
            tokens.push(Tok::Num);
        } else if is_word_byte(b) && b != b'[' {
            let start = i;
            while i < bytes.len() && (is_word_byte(bytes[i]) || (bytes[i] == b'-' && bytes.get(i + 1).copied().is_some_and(is_word_byte))) {
                i += 1;
            }
            let word = &text[start..i];
            let infinity = word.eq_ignore_ascii_case("inf") || word.eq_ignore_ascii_case("infinity");
            tokens.push(if infinity { Tok::Num } else { Tok::Name });
        } else {
            tokens.push(Tok::Other);
            // Step over one whole character.
            i += text[i..].chars().next().map_or(1, char::len_utf8);
        }
    }
    tokens
}

#[cfg(test)]
mod tests {
    use tower_lsp_server::ls_types::Uri;

    use super::*;
    use crate::position::Encoding;

    const BASE: &str = "Maximize multi-objectives\n o1: 3 x + 2 y\nSubject To\n c1: x + y <= 10\nGeneral Constraints\n g1: r = MAX ( x , y )\nBounds\n x <= 4\nGenerals\n y\nSOS\n s1: S1 :: x : 1\nEnd\n";

    /// Completion labels with `|` marking the cursor in `text`.
    fn labels(text: &str) -> Vec<String> {
        let at = text.find('|').expect("cursor marker");
        let source = text.replacen('|', "", 1);
        let doc = Document::new("file:///t.lp".parse::<Uri>().unwrap(), source, 1, Encoding::Utf16);
        complete(&doc, doc.position(at)).into_iter().map(|i| i.label).collect()
    }

    /// `BASE` with `line` inserted before the line starting with `anchor`.
    fn with_line(anchor: &str, line: &str) -> String {
        let at = BASE.find(anchor).unwrap();
        format!("{}{line}\n{}", &BASE[..at], &BASE[at..])
    }

    fn has(labels: &[String], label: &str) -> bool {
        labels.iter().any(|l| l == label)
    }

    #[test]
    fn keywords_only_at_line_start() {
        let start = labels(&with_line("End", "Bou|"));
        assert!(has(&start, "Bounds") && has(&start, "Subject To") && has(&start, "End"), "{start:?}");
        assert!(!has(&start, "Minimize"));
        let mid = labels(&with_line("Generals", " x + Bou|"));
        assert!(!has(&mid, "Bounds") && !has(&mid, "End"), "{mid:?}");
        let sense = labels("Ma|");
        assert!(has(&sense, "Maximize") && !has(&sense, "Bounds"), "{sense:?}");
    }

    #[test]
    fn snippets_have_snippet_format() {
        let source = with_line("End", "");
        let at = source.find("\nEnd").unwrap() + 1;
        let doc = Document::new("file:///t.lp".parse::<Uri>().unwrap(), source, 1, Encoding::Utf8);
        let items = complete(&doc, doc.position(at));
        let snippet = items.iter().find(|i| i.label == "Subject To" && i.kind == Some(CompletionItemKind::SNIPPET)).unwrap();
        assert_eq!(snippet.insert_text_format, Some(InsertTextFormat::SNIPPET));
        let Some(CompletionTextEdit::Edit(edit)) = &snippet.text_edit else { panic!("text edit expected") };
        assert_eq!(edit.new_text, "Subject To\n  ${1:c1}: $0");
    }

    #[test]
    fn variables_in_expression_bound_type_and_sos_contexts() {
        for (anchor, line) in [
            ("General", " c2: 2 x + |"),
            ("General", " c2: |"),
            ("General", " c3: 2 <= x + |"),
            ("Generals", " -5 <= |"),
            ("SOS", " |"),
            ("End", " s2: S2 :: x : 1 |"),
        ] {
            let got = labels(&with_line(anchor, line));
            assert!(has(&got, "x") && has(&got, "y"), "{line}: {got:?}");
        }
        let got = labels(&BASE.replace(" y\nSOS", " y |\nSOS"));
        assert!(has(&got, "x"), "{got:?}");
    }

    #[test]
    fn no_variables_in_comments_or_rhs() {
        assert_eq!(labels(&with_line("General", " \\ note x|")), Vec::<String>::new());
        assert_eq!(labels(&with_line("General", " \\* x + | *\\")), Vec::<String>::new());
        assert_eq!(labels(&with_line("General", " c2: x + y <= |")), Vec::<String>::new());
        let block = labels(&with_line("General", "\\* a\n x| *\\"));
        assert_eq!(block, Vec::<String>::new());
    }

    #[test]
    fn sos_type_after_label() {
        let got = labels(&with_line("End", " s2: |"));
        assert_eq!(got, ["S1", "S2"]);
    }

    #[test]
    fn functions_after_resultant() {
        let got = labels(&with_line("Bounds", " g2: z = M|"));
        assert_eq!(got, ["MAX", "MIN", "ABS", "AND", "OR"]);
        let args = labels(&with_line("Bounds", " g2: z = MIN ( x , |"));
        assert!(has(&args, "y"), "{args:?}");
    }

    #[test]
    fn attributes_after_objective_label() {
        let got = labels(&BASE.replace(" o1: 3 x", " o1: Priority=1 | 3 x"));
        assert!(has(&got, "Weight") && has(&got, "RelTol") && has(&got, "x"), "{got:?}");
        let single = labels("min\n obj: |\nst\n c: x >= 1\nend\n");
        assert!(!has(&single, "Priority") && has(&single, "x"), "{single:?}");
    }

    #[test]
    fn operators_after_expression_and_free_in_bounds() {
        let ops = labels(&with_line("General", " c2: x + y |"));
        assert!(has(&ops, "<=") && has(&ops, "=") && !has(&ops, "x"), "{ops:?}");
        let free = labels(&with_line("Generals", " y |"));
        assert!(has(&free, "free") && has(&free, ">="), "{free:?}");
    }

    #[test]
    fn resolve_adds_documentation() {
        let source = with_line("End", "");
        let at = source.find("\nEnd").unwrap() + 1;
        let doc = Document::new("file:///t.lp".parse::<Uri>().unwrap(), source, 1, Encoding::Utf8);
        let item = complete(&doc, doc.position(at)).into_iter().find(|i| i.label == "Bounds").unwrap();
        assert!(item.documentation.is_none());
        let Some(Documentation::MarkupContent(markup)) = resolve(item).documentation else { panic!("markdown expected") };
        assert!(markup.value.contains("`bound`"));
    }
}
