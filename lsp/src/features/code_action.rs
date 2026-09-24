//! Quick fixes, refactors and the organise-sections source action.
//!
//! Every action is a set of minimal byte-range edits, converted to LSP
//! `TextEdit`s in one `WorkspaceEdit` keyed by the document URI. Conditions
//! are detected from the tree and index; matching diagnostics from the
//! request context are attached to the quick fixes that resolve them.

use std::collections::{HashMap, HashSet};
use std::ops::Range;

use tower_lsp_server::ls_types::{
    CodeAction, CodeActionContext, CodeActionKind, CodeActionOrCommand, Diagnostic, NumberOrString, Range as LspRange, TextEdit,
    WorkspaceEdit,
};
use tree_sitter::Node;

use crate::config::Config;
use crate::document::Document;
use crate::features::diagnostics::codes;
use crate::index::{EntityKind, Namespace, Role, Section, Symbol};
use crate::syntax::{self, kind};

/// Source action kind for reordering sections canonically.
pub const ORGANIZE_SECTIONS: &str = "source.organizeSections";

/// Single-word section keywords (`src/scanner.c`): read as keywords when they
/// are the first token of a line.
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
];

/// Type-section roles with their section kinds and the header written when
/// the section has to be created.
const TYPE_SECTIONS: &[(Role, &str, &str)] = &[
    (Role::Generals, kind::GENERALS_SECTION, "Generals"),
    (Role::Integers, kind::INTEGERS_SECTION, "Integers"),
    (Role::Binaries, kind::BINARIES_SECTION, "Binaries"),
    (Role::SemiContinuous, kind::SEMI_CONTINUOUS_SECTION, "Semi-Continuous"),
];

/// A byte-range replacement.
type Edit = (Range<usize>, String);

/// Code actions for `range`.
#[must_use]
pub fn actions(doc: &Document, range: LspRange, context: &CodeActionContext, _config: &Config) -> Vec<CodeActionOrCommand> {
    // No setting affects code actions yet.
    let mut actions = Actions { doc, context, request: doc.byte_range(range), out: Vec::new() };
    debug_assert!(actions.request.start <= actions.request.end && actions.request.end <= doc.text.len());

    if actions.wants(&CodeActionKind::QUICKFIX) {
        actions.quadratic_divisors();
        actions.indicator_values();
        actions.duplicate_names();
        actions.operator_spellings();
        actions.declarations();
    }
    if actions.wants(&CodeActionKind::REFACTOR) {
        actions.add_bound();
        actions.move_type();
    }
    if actions.wants(&CodeActionKind::REFACTOR_REWRITE) {
        actions.name_constraints();
        actions.sort_type_section();
        actions.flipped_constraints();
    }
    let organise = CodeActionKind::new(ORGANIZE_SECTIONS);
    if actions.wants(&organise)
        && let Some(edit) = organise_sections(doc)
    {
        actions.push("Organise sections".to_owned(), organise, vec![edit], None, false);
    }
    actions.out
}

struct Actions<'a> {
    doc: &'a Document,
    context: &'a CodeActionContext,
    request: Range<usize>,
    out: Vec<CodeActionOrCommand>,
}

impl Actions<'_> {
    /// Whether `kind` passes the client's `only` filter (prefix match on
    /// dot-separated kinds).
    fn wants(&self, kind: &CodeActionKind) -> bool {
        self.context.only.as_ref().is_none_or(|only| {
            only.iter().any(|o| {
                let (k, o) = (kind.as_str(), o.as_str());
                k == o || (k.starts_with(o) && k.as_bytes().get(o.len()) == Some(&b'.'))
            })
        })
    }

    const fn touches(&self, range: &Range<usize>) -> bool {
        touches(range, &self.request)
    }

    /// Context diagnostics with `code` overlapping `target`.
    fn diagnostics(&self, code: &str, target: &Range<usize>) -> Option<Vec<Diagnostic>> {
        let found: Vec<Diagnostic> = self
            .context
            .diagnostics
            .iter()
            .filter(|d| matches!(&d.code, Some(NumberOrString::String(c)) if c == code))
            .filter(|d| touches(&self.doc.byte_range(d.range), target))
            .cloned()
            .collect();
        (!found.is_empty()).then_some(found)
    }

    fn push(&mut self, title: String, kind: CodeActionKind, mut edits: Vec<Edit>, diagnostics: Option<Vec<Diagnostic>>, preferred: bool) {
        debug_assert!(!edits.is_empty(), "an action must edit something");
        edits.sort_by_key(|(range, _)| (range.start, range.end));
        debug_assert!(edits.windows(2).all(|w| w[0].0.end <= w[1].0.start), "edits must not overlap: {edits:?}");
        let edits: Vec<TextEdit> = edits.into_iter().map(|(range, text)| TextEdit::new(self.doc.range(range), text)).collect();
        let changes = HashMap::from([(self.doc.uri.clone(), edits)]);
        self.out.push(CodeActionOrCommand::CodeAction(CodeAction {
            title,
            kind: Some(kind),
            diagnostics,
            edit: Some(WorkspaceEdit::new(changes)),
            is_preferred: preferred.then_some(true),
            ..CodeAction::default()
        }));
    }

    /// Name sites overlapping the request.
    fn sites(&self) -> &[(Range<usize>, Symbol)] {
        let sites = self.doc.index().sites();
        let lo = sites.partition_point(|(range, _)| range.end < self.request.start);
        let hi = sites.partition_point(|(range, _)| range.start <= self.request.end);
        &sites[lo..hi.max(lo)]
    }

    // ---- quick fixes -------------------------------------------------------

    /// Objectives need `[ ... ] / 2`; constraints must not divide.
    fn quadratic_divisors(&mut self) {
        let doc = self.doc;
        for block in nodes_in(doc.tree.root_node(), &self.request, kind::QUADRATIC_BLOCK) {
            let Some(section) = syntax::section_of(block) else { continue };
            let slash = children(block).into_iter().find(|c| c.kind() == "/");
            let diagnostics = self.diagnostics(codes::PARSE_ERROR, &block.byte_range());
            let end = block.end_byte();
            if section.kind() == kind::OBJECTIVES_SECTION {
                if slash.is_some() {
                    continue;
                }
                let mut keep = scale_terms(doc, block, |v| v * 2.0);
                keep.push((end..end, " / 2".to_owned()));
                self.push(
                    "Append `/ 2` and double the coefficients (keeps values)".to_owned(),
                    CodeActionKind::QUICKFIX,
                    keep,
                    diagnostics.clone(),
                    true,
                );
                self.push(
                    "Append `/ 2` (halves the quadratic terms)".to_owned(),
                    CodeActionKind::QUICKFIX,
                    vec![(end..end, " / 2".to_owned())],
                    diagnostics,
                    false,
                );
            } else if let Some(slash) = slash {
                let Some(close) = slash.prev_sibling() else { continue };
                let divisor_node = slash.next_sibling().filter(|n| n.kind() == kind::NUMBER);
                let divisor_text = divisor_node.map_or("", |n| doc.node_text(n));
                let remove = (close.end_byte()..end, String::new());
                let divisor = divisor_node.and_then(|n| syntax::parse_number(doc.node_text(n))).filter(|d| d.is_finite() && *d > 0.0);
                if let Some(divisor) = divisor {
                    let mut keep = scale_terms(doc, block, |v| v / divisor);
                    keep.push(remove.clone());
                    let title = format!("Remove `/ {divisor_text}` and divide the coefficients by {divisor_text} (keeps values)");
                    self.push(title, CodeActionKind::QUICKFIX, keep, diagnostics.clone(), true);
                }
                self.push(format!("Remove `/ {divisor_text}`"), CodeActionKind::QUICKFIX, vec![remove], diagnostics, false);
            }
        }
    }

    /// `b = 2 -> ...`: the indicator value must be 0 or 1.
    fn indicator_values(&mut self) {
        let doc = self.doc;
        for indicator in nodes_in(doc.tree.root_node(), &self.request, kind::INDICATOR) {
            let Some(value) = children(indicator).into_iter().find(|c| c.kind() == kind::NUMBER) else { continue };
            // Upstream compares the literal exactly with 0 and 1.
            #[allow(clippy::float_cmp)]
            let valid = syntax::parse_number(doc.node_text(value)).is_some_and(|v| v == 0.0 || v == 1.0);
            if valid {
                continue;
            }
            let diagnostics = self.diagnostics(codes::PARSE_ERROR, &indicator.byte_range());
            for target in ["1", "0"] {
                let edit = (value.byte_range(), target.to_owned());
                self.push(format!("Set indicator value to {target}"), CodeActionKind::QUICKFIX, vec![edit], diagnostics.clone(), false);
            }
        }
    }

    /// Rename a later entity reusing a name to the first free `name_N`.
    fn duplicate_names(&mut self) {
        let index = self.doc.index();
        for duplicate in &index.duplicates {
            let entity = &index.entities[duplicate.duplicate];
            let (Some(name), Some(range)) = (entity.name.as_deref(), entity.name_range.clone()) else { continue };
            if !self.touches(&range) {
                continue;
            }
            let namespace = entity.kind.namespace();
            let taken: HashSet<&str> =
                index.entities.iter().filter(|e| e.kind.namespace() == namespace).filter_map(|e| e.name.as_deref()).collect();
            // `taken.len() + 1` candidates cannot all be taken.
            let Some(fresh) = (2..=taken.len() + 2).map(|n| format!("{name}_{n}")).find(|c| !taken.contains(c.as_str())) else { continue };
            let diagnostics = self.diagnostics(codes::DUPLICATE_NAME, &range);
            self.push(format!("Rename duplicate `{name}` to `{fresh}`"), CodeActionKind::QUICKFIX, vec![(range, fresh)], diagnostics, true);
        }
    }

    /// `=<` → `<=` and `=>` → `>=`, singly and across the file.
    fn operator_spellings(&mut self) {
        let doc = self.doc;
        let misspelt = |n: &Node<'_>| matches!(doc.node_text(*n), "=<" | "=>");
        let here: Vec<Node<'_>> =
            nodes_in(doc.tree.root_node(), &self.request, kind::COMPARISON_OPERATOR).into_iter().filter(misspelt).collect();
        if here.is_empty() {
            return;
        }
        let fix = |n: Node<'_>| (n.byte_range(), mirror_spelling(doc.node_text(n)).to_owned());
        for &op in &here {
            let (range, text) = fix(op);
            let diagnostics = self.diagnostics(codes::OPERATOR_SPELLING, &range);
            let title = format!("Replace `{}` with `{text}`", doc.node_text(op));
            self.push(title, CodeActionKind::QUICKFIX, vec![(range, text)], diagnostics, true);
        }
        let all: Vec<Node<'_>> =
            nodes_in(doc.tree.root_node(), &(0..doc.text.len()), kind::COMPARISON_OPERATOR).into_iter().filter(misspelt).collect();
        if all.len() > 1 {
            let diagnostics = self.diagnostics(codes::OPERATOR_SPELLING, &(0..doc.text.len()));
            let edits = all.into_iter().map(fix).collect();
            self.push("Normalise all `=<` / `=>` operators in file".to_owned(), CodeActionKind::QUICKFIX, edits, diagnostics, false);
        }
    }

    /// Unused bound/type declarations and repeated type declarations.
    fn declarations(&mut self) {
        let doc = self.doc;
        let sites: Vec<(usize, usize)> =
            self.sites().iter().filter_map(|(_, s)| if let Symbol::Variable(v, o) = *s { Some((v, o)) } else { None }).collect();
        for (v, o) in sites {
            let variable = &doc.index().variables[v];
            let occurrence = &variable.occurrences[o];
            let name = &variable.name;
            if !occurrence.role.is_declaration() {
                continue;
            }
            if !variable.is_used() {
                let (item, what) = if occurrence.role == Role::Bound {
                    let Some(bound) =
                        doc.node(occurrence.range.clone(), kind::IDENTIFIER).and_then(|n| syntax::ancestor(n, kind::BOUND_DECLARATION))
                    else {
                        continue;
                    };
                    (bound.byte_range(), "bound".to_owned())
                } else {
                    (occurrence.range.clone(), format!("`{}` entry", type_label(occurrence.role)))
                };
                let diagnostics = self.diagnostics(codes::UNUSED_DECLARATION, &item);
                let edit = deletion(&doc.text, &item);
                self.push(format!("Remove unused {what} for `{name}`"), CodeActionKind::QUICKFIX, vec![edit], diagnostics, true);
            }
            if !occurrence.role.is_type_declaration() {
                continue;
            }
            let earlier = variable.occurrences[..o]
                .iter()
                .map(|e| e.role)
                .filter(|r| r.is_type_declaration())
                .find(|&r| conflicts(r, occurrence.role));
            if let Some(first) = earlier {
                let label = type_label(occurrence.role);
                let title = if first == occurrence.role {
                    format!("Remove duplicate `{label}` entry for `{name}`")
                } else {
                    format!("Remove conflicting `{label}` entry for `{name}` (keeps `{}`)", type_label(first))
                };
                let diagnostics = self.diagnostics(codes::CONFLICTING_TYPE, &occurrence.range);
                let edit = deletion(&doc.text, &occurrence.range);
                self.push(title, CodeActionKind::QUICKFIX, vec![edit], diagnostics, true);
            }
        }
    }

    // ---- refactors ---------------------------------------------------------

    /// Variable under the cursor: `(variable, occurrence)`.
    fn variable_at_cursor(&self) -> Option<(usize, usize)> {
        match self.doc.index().symbol_at(self.request.start)? {
            (_, Symbol::Variable(v, o)) => Some((v, o)),
            _ => None,
        }
    }

    /// Add a placeholder bound `0 <= x <= 1e30` (the default bounds: upstream
    /// reads `1e30` as infinity) for a variable without one.
    fn add_bound(&mut self) {
        let Some((v, _)) = self.variable_at_cursor() else { return };
        let variable = &self.doc.index().variables[v];
        if variable.occurrences.iter().any(|o| o.role == Role::Bound) {
            return;
        }
        let entry = format!("0 <= {} <= 1e30", variable.name);
        let Some(edit) = insert_entry(self.doc, kind::BOUNDS_SECTION, "Bounds", &entry) else { return };
        self.push(format!("Add bound for `{}`", variable.name), CodeActionKind::REFACTOR, vec![edit], None, false);
    }

    /// Move a type-section entry to another type section.
    fn move_type(&mut self) {
        let Some((v, o)) = self.variable_at_cursor() else { return };
        let doc = self.doc;
        let variable = &doc.index().variables[v];
        let occurrence = &variable.occurrences[o];
        if !occurrence.role.is_type_declaration() {
            return;
        }
        for &(role, section, header) in TYPE_SECTIONS {
            if variable.occurrences.iter().any(|e| e.role == role) {
                continue;
            }
            let Some(insert) = insert_entry(doc, section, header, &variable.name) else { continue };
            let delete = deletion(&doc.text, &occurrence.range);
            if insert.0.start > delete.0.start && insert.0.start < delete.0.end {
                continue;
            }
            let title = format!("Move `{}` to `{}`", variable.name, type_label(role));
            self.push(title, CodeActionKind::REFACTOR, vec![delete, insert], None, false);
        }
    }

    /// Name every unnamed constraint with the name upstream would generate.
    fn name_constraints(&mut self) {
        let doc = self.doc;
        let index = doc.index();
        let constraint_sections =
            [kind::CONSTRAINTS_SECTION, kind::LAZY_CONSTRAINTS_SECTION, kind::USER_CUTS_SECTION, kind::GENERAL_CONSTRAINTS_SECTION];
        let in_section = index.sections.iter().any(|s| constraint_sections.contains(&s.kind) && self.touches(&s.range));
        let is_constraint = |k: EntityKind| matches!(k, EntityKind::Constraint | EntityKind::GeneralConstraint);
        if !in_section || !index.entities.iter().any(|e| is_constraint(e.kind) && e.name.is_none()) {
            return;
        }
        let edits = generated_names(doc).into_iter().map(|(at, name)| (at..at, format!("{name}: "))).collect::<Vec<_>>();
        let count = edits.len();
        let noun = if count == 1 { "constraint" } else { "constraints" };
        self.push(format!("Name {count} unnamed {noun}"), CodeActionKind::REFACTOR_REWRITE, edits, None, false);
    }

    /// Sort a type section's entries, keeping the layout between them.
    fn sort_type_section(&mut self) {
        let doc = self.doc;
        let spans: Vec<Range<usize>> = doc
            .index()
            .sections
            .iter()
            .filter(|s| TYPE_SECTIONS.iter().any(|(_, k, _)| *k == s.kind) && self.touches(&s.range))
            .map(|s| s.range.clone())
            .collect();
        for span in spans {
            let Some(section) = doc.tree.root_node().descendant_for_byte_range(span.start, span.end).and_then(syntax::section_of) else {
                continue;
            };
            let entries: Vec<Node<'_>> = children(section).into_iter().filter(|c| c.kind() == kind::IDENTIFIER).collect();
            let (Some(first), Some(last)) = (entries.first(), entries.last()) else { continue };
            let names: Vec<&str> = entries.iter().map(|&n| doc.node_text(n)).collect();
            let separators: Vec<&str> = entries.windows(2).map(|w| &doc.text[w[0].end_byte()..w[1].start_byte()]).collect();
            let interleaved = separators.iter().any(|s| !s.trim().is_empty());
            let keyword = names.iter().any(|n| is_line_start_keyword(n));
            if names.is_sorted() || interleaved || keyword {
                continue;
            }
            let mut sorted = names.clone();
            sorted.sort_unstable();
            let mut text = String::with_capacity(last.end_byte() - first.start_byte());
            for (i, name) in sorted.iter().enumerate() {
                text.push_str(name);
                if let Some(separator) = separators.get(i) {
                    text.push_str(separator);
                }
            }
            let header = section.child(0).map_or("section", |h| doc.node_text(h));
            let edit = (first.start_byte()..last.end_byte(), text);
            self.push(format!("Sort `{header}` entries"), CodeActionKind::REFACTOR_REWRITE, vec![edit], None, false);
        }
    }

    /// `10 >= x + y` → `x + y <= 10`.
    fn flipped_constraints(&mut self) {
        let doc = self.doc;
        for constraint in nodes_in(doc.tree.root_node(), &self.request, kind::CONSTRAINT) {
            let parts = children(constraint);
            let operators: Vec<Node<'_>> = parts.iter().copied().filter(|c| c.kind() == kind::COMPARISON_OPERATOR).collect();
            let [op] = operators[..] else { continue };
            let name = constraint.child_by_field_name("name");
            let Some(start) = parts.iter().find(|c| {
                Some(**c) != name && !matches!(c.kind(), ":" | "::" | kind::INDICATOR | kind::LINE_COMMENT | kind::BLOCK_COMMENT)
            }) else {
                continue;
            };
            let Some(expression) = op.next_named_sibling().filter(|n| n.kind() == kind::LINEAR_EXPRESSION) else { continue };
            if start.kind() == kind::LINEAR_EXPRESSION || start.start_byte() >= op.start_byte() {
                continue;
            }
            let replaced = start.start_byte()..expression.end_byte();
            let has_comment = parts.iter().any(|c| c.is_extra() && touches(&c.byte_range(), &replaced))
                || nodes_in(expression, &replaced, kind::LINE_COMMENT).len() + nodes_in(expression, &replaced, kind::BLOCK_COMMENT).len()
                    > 0;
            if has_comment {
                continue;
            }
            let value: String = doc.text[start.start_byte()..op.start_byte()].chars().filter(|c| !c.is_whitespace()).collect();
            let rewritten = format!("{} {} {value}", doc.node_text(expression), mirror_operator(doc.node_text(op)));
            let title = format!("Rewrite as `{rewritten}`");
            self.push(title, CodeActionKind::REFACTOR_REWRITE, vec![(replaced, rewritten)], None, false);
        }
    }
}

/// Inclusive overlap: ranges that merely touch count.
const fn touches(a: &Range<usize>, b: &Range<usize>) -> bool {
    a.start <= b.end && b.start <= a.end
}

/// All children (named and anonymous) of `node`.
fn children(node: Node<'_>) -> Vec<Node<'_>> {
    let mut cursor = node.walk();
    node.children(&mut cursor).collect()
}

/// Nodes of `kind` under `root` touching `range`, in document order. Does not
/// descend into a match.
fn nodes_in<'t>(root: Node<'t>, range: &Range<usize>, kind: &str) -> Vec<Node<'t>> {
    let mut out = Vec::new();
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        if !touches(&node.byte_range(), range) {
            continue;
        }
        if node.kind() == kind {
            out.push(node);
            continue;
        }
        let mut cursor = node.walk();
        stack.extend(node.children(&mut cursor));
    }
    out.sort_by_key(Node::start_byte);
    out
}

/// Rescale every coefficient in a quadratic block (inserting one where the
/// term has an implicit 1).
fn scale_terms(doc: &Document, block: Node<'_>, scale: impl Fn(f64) -> f64) -> Vec<Edit> {
    debug_assert_eq!(block.kind(), kind::QUADRATIC_BLOCK);
    let mut edits = Vec::new();
    for term in children(block).into_iter().filter(|c| c.kind() == kind::QUADRATIC_TERM) {
        let Some(head) = term.named_child(0) else { continue };
        if head.kind() == kind::NUMBER {
            let Some(value) = syntax::parse_number(doc.node_text(head)) else { continue };
            edits.push((head.byte_range(), format_number(scale(value))));
        } else {
            let at = head.start_byte();
            edits.push((at..at, format!("{} ", format_number(scale(1.0)))));
        }
    }
    edits
}

/// Shortest round-tripping decimal, in exponent form when very large or small.
fn format_number(value: f64) -> String {
    debug_assert!(value.is_finite());
    let magnitude = value.abs();
    if magnitude != 0.0 && !(1e-6..1e16).contains(&magnitude) { format!("{value:e}") } else { format!("{value}") }
}

/// `=<` → `<=`, `=>` → `>=`.
fn mirror_spelling(op: &str) -> &'static str {
    debug_assert!(matches!(op, "=<" | "=>"));
    if op == "=<" { "<=" } else { ">=" }
}

/// The operator with its sides swapped (`>=` becomes `<=`).
fn mirror_operator(op: &str) -> &'static str {
    match op {
        "<=" | "=<" => ">=",
        ">=" | "=>" => "<=",
        "<" => ">",
        ">" => "<",
        _ => "=",
    }
}

/// Section name for a type-declaration role.
const fn type_label(role: Role) -> &'static str {
    match role {
        Role::Generals => "generals",
        Role::Integers => "integers",
        Role::Binaries => "binaries",
        _ => "semi-continuous",
    }
}

/// Whether a later type declaration `later` repeats or contradicts an
/// earlier `first`. General/integer plus semi-continuous is legitimate
/// (semi-integer).
fn conflicts(first: Role, later: Role) -> bool {
    debug_assert!(first.is_type_declaration() && later.is_type_declaration());
    let semi_integer = |a: Role, b: Role| matches!(a, Role::Generals | Role::Integers) && b == Role::SemiContinuous;
    !(semi_integer(first, later) || semi_integer(later, first))
}

fn is_line_start_keyword(name: &str) -> bool {
    LINE_START_KEYWORDS.iter().any(|k| k.eq_ignore_ascii_case(name))
}

/// Line terminator used by the document.
fn eol(text: &str) -> &'static str {
    if text.contains("\r\n") { "\r\n" } else { "\n" }
}

/// Deletion of `item`: its whole line when nothing else is on it, else the
/// item and adjacent horizontal whitespace.
fn deletion(text: &str, item: &Range<usize>) -> Edit {
    debug_assert!(item.start < item.end && item.end <= text.len());
    let line_start = text[..item.start].rfind('\n').map_or(0, |i| i + 1);
    let line_end = text[item.end..].find('\n').map_or(text.len(), |i| item.end + i + 1);
    let before = &text[line_start..item.start];
    let after = &text[item.end..line_end];
    if before.trim().is_empty() && after.trim().is_empty() {
        return (line_start..line_end, String::new());
    }
    let trailing = after.len() - after.trim_start_matches([' ', '\t']).len();
    if trailing > 0 && !after.trim().is_empty() {
        return (item.start..item.end + trailing, String::new());
    }
    let leading = before.len() - before.trim_end_matches([' ', '\t']).len();
    (item.start - leading..item.end, String::new())
}

/// Rank of a section kind in the canonical order.
fn rank(kind: &str) -> Option<usize> {
    syntax::SECTION_KINDS.iter().position(|k| *k == kind)
}

/// Whether `offset` starts a comment node.
fn starts_comment(doc: &Document, offset: usize) -> bool {
    doc.tree
        .root_node()
        .descendant_for_byte_range(offset, offset + 1)
        .is_some_and(|n| matches!(n.kind(), kind::LINE_COMMENT | kind::BLOCK_COMMENT) && n.start_byte() == offset)
}

/// Start of the line holding `offset`, extended upwards over directly
/// preceding comment-only lines. `None` when `offset` is not the first token
/// on its line.
fn attached_start(doc: &Document, offset: usize) -> Option<usize> {
    let lines = &doc.lines;
    let line = lines.line_of(offset);
    let mut start = lines.line_start(line);
    if !doc.text[start..offset].trim().is_empty() {
        return None;
    }
    let mut above = line;
    while above > 0 {
        above -= 1;
        let content = lines.line_range(&doc.text, above);
        let text = &doc.text[content.clone()];
        let indent = text.len() - text.trim_start().len();
        let comment_only = !text.trim().is_empty()
            && starts_comment(doc, content.start + indent)
            && doc
                .tree
                .root_node()
                .descendant_for_byte_range(content.start + indent, content.start + indent + 1)
                .is_some_and(|n| n.end_byte() >= content.end);
        if !comment_only {
            break;
        }
        start = lines.line_start(above);
    }
    Some(start)
}

/// Insert `entry` into the first section of `section_kind`, or create the
/// section (`header` then the entry) at its canonical position.
fn insert_entry(doc: &Document, section_kind: &str, header: &str, entry: &str) -> Option<Edit> {
    let eol = eol(&doc.text);
    let own_rank = rank(section_kind)?;
    if let Some(span) = doc.index().sections.iter().find(|s| s.kind == section_kind) {
        let section = doc.node(span.range.clone(), section_kind)?;
        let last = children(section).into_iter().rfind(|c| !c.is_extra())?;
        let line_end = doc.lines.line_range(&doc.text, doc.lines.line_of(last.end_byte())).end;
        let in_comment = doc
            .tree
            .root_node()
            .descendant_for_byte_range(line_end.saturating_sub(1), line_end)
            .is_some_and(|n| n.kind() == kind::BLOCK_COMMENT && n.end_byte() > line_end);
        // A keyword-named entry must not start a line.
        if in_comment || is_line_start_keyword(entry) {
            let at = last.end_byte();
            return Some((at..at, format!(" {entry}")));
        }
        let indent = if last.kind() == kind::IDENTIFIER || last.kind() == kind::BOUND_DECLARATION || last.parent() != Some(section) {
            let line = doc.lines.line_range(&doc.text, doc.lines.line_of(last.start_byte()));
            let text = &doc.text[line];
            &text[..text.len() - text.trim_start().len()]
        } else {
            " "
        };
        let indent = if indent.is_empty() { " " } else { indent };
        return Some((line_end..line_end, format!("{eol}{indent}{entry}")));
    }
    let root = doc.tree.root_node();
    let later = children(root).into_iter().find(|c| c.kind() == kind::END_MARKER || rank(c.kind()).is_some_and(|r| r > own_rank && r > 1));
    let body = if is_line_start_keyword(entry) { format!("{header} {entry}{eol}") } else { format!("{header}{eol} {entry}{eol}") };
    if let Some(node) = later {
        let at = attached_start(doc, node.start_byte())?;
        return Some((at..at, body));
    }
    let at = doc.text.len();
    let lead = if doc.text.is_empty() || doc.text.ends_with('\n') { "" } else { eol };
    Some((at..at, format!("{lead}{body}")))
}

/// Names upstream would give each unnamed constraint: `(insert offset, name)`.
///
/// Upstream numbers `C<n>` with one counter over `Subject To`, then general
/// constraints, then lazy constraints, then user cuts, skipping every
/// explicit constraint/SOS name. An unnamed ranged constraint expands into
/// two constraints and so consumes two numbers; once named it becomes
/// `C<n>` plus `C<n>_rng`.
fn generated_names(doc: &Document) -> Vec<(usize, String)> {
    let index = doc.index();
    let reserved: HashSet<&str> =
        index.entities.iter().filter(|e| e.kind.namespace() == Namespace::Constraint).filter_map(|e| e.name.as_deref()).collect();
    let order = |section: Section| match section {
        Section::SubjectTo => 0,
        Section::General => 1,
        Section::Lazy => 2,
        _ => 3,
    };
    let mut constraints: Vec<usize> = (0..index.entities.len())
        .filter(|&i| matches!(index.entities[i].kind, EntityKind::Constraint | EntityKind::GeneralConstraint))
        .collect();
    constraints.sort_by_key(|&i| order(index.entities[i].section));

    let mut counter: u32 = 0;
    let mut next = || loop {
        counter += 1;
        let candidate = format!("C{counter}");
        if !reserved.contains(candidate.as_str()) {
            return candidate;
        }
    };
    let mut out = Vec::new();
    for i in constraints {
        let entity = &index.entities[i];
        if entity.name.is_some() {
            continue;
        }
        out.push((entity.range.start, next()));
        let ranged = doc
            .node(entity.range.clone(), kind::CONSTRAINT)
            .is_some_and(|n| children(n).iter().filter(|c| c.kind() == kind::COMPARISON_OPERATOR).count() == 2);
        if ranged {
            next();
        }
    }
    out.sort_by_key(|(at, _)| *at);
    out
}

/// Reorder the movable sections (everything after `Subject To`) canonically,
/// each moving with the comment lines directly above it.
fn organise_sections(doc: &Document) -> Option<Edit> {
    if doc.has_syntax_errors() {
        return None;
    }
    let root = doc.tree.root_node();
    let top = children(root);
    let sections: Vec<(usize, Node<'_>)> = top.iter().filter_map(|&c| rank(c.kind()).filter(|&r| r > 1).map(|r| (r, c))).collect();
    if sections.len() < 2 || sections.windows(2).all(|w| w[0].0 <= w[1].0) {
        return None;
    }
    let starts: Vec<usize> = sections.iter().map(|(_, s)| attached_start(doc, s.start_byte())).collect::<Option<_>>()?;
    let tail = match top.iter().find(|c| c.kind() == kind::END_MARKER) {
        Some(end) => attached_start(doc, end.start_byte())?,
        None => doc.text.len(),
    };
    if !starts.windows(2).all(|w| w[0] < w[1]) || starts.last().is_some_and(|&s| s > tail) {
        return None;
    }
    let mut chunks: Vec<(usize, &str)> =
        sections.iter().enumerate().map(|(i, (r, _))| (*r, &doc.text[starts[i]..starts.get(i + 1).copied().unwrap_or(tail)])).collect();
    chunks.sort_by_key(|(r, _)| *r);
    let eol = eol(&doc.text);
    let mut text = String::with_capacity(tail - starts[0] + eol.len());
    for (i, (_, chunk)) in chunks.iter().enumerate() {
        text.push_str(chunk);
        if i + 1 < chunks.len() && !chunk.ends_with('\n') {
            text.push_str(eol);
        }
    }
    Some(trim_edit(&doc.text, starts[0]..tail, text))
}

/// Shrink a replacement to the part that actually changes.
fn trim_edit(original: &str, mut range: Range<usize>, mut text: String) -> Edit {
    let old = &original[range.clone()];
    let mut prefix = old.bytes().zip(text.bytes()).take_while(|(a, b)| a == b).count();
    while !old.is_char_boundary(prefix) {
        prefix -= 1;
    }
    let max_suffix = old.len().min(text.len()) - prefix;
    let mut suffix = old.bytes().rev().zip(text.bytes().rev()).take(max_suffix).take_while(|(a, b)| a == b).count();
    while !old.is_char_boundary(old.len() - suffix) || !text.is_char_boundary(text.len() - suffix) {
        suffix -= 1;
    }
    text.truncate(text.len() - suffix);
    text.drain(..prefix);
    range = range.start + prefix..range.end - suffix;
    (range, text)
}

#[cfg(test)]
mod tests {
    use lp_parser_rs::LpProblem;
    use lp_parser_rs::diff::DiffOptions;
    use tower_lsp_server::ls_types::{DiagnosticSeverity, Uri};

    use super::*;
    use crate::position::Encoding;

    fn doc(text: &str) -> Document {
        let uri: Uri = "file:///test.lp".parse().unwrap();
        Document::new(uri, text.to_owned(), 1, Encoding::Utf16)
    }

    fn run_with(doc: &Document, at: Range<usize>, context: &CodeActionContext) -> Vec<CodeAction> {
        actions(doc, doc.range(at), context, &Config::default())
            .into_iter()
            .map(|a| match a {
                CodeActionOrCommand::CodeAction(action) => action,
                CodeActionOrCommand::Command(c) => panic!("unexpected command {c:?}"),
            })
            .collect()
    }

    /// Actions with the cursor at the first occurrence of `needle`.
    fn at(doc: &Document, needle: &str) -> Vec<CodeAction> {
        let offset = doc.text.find(needle).unwrap_or_else(|| panic!("`{needle}` not in text"));
        run_with(doc, offset..offset, &CodeActionContext::default())
    }

    fn titles(actions: &[CodeAction]) -> Vec<&str> {
        actions.iter().map(|a| a.title.as_str()).collect()
    }

    fn find(actions: &[CodeAction], prefix: &str) -> CodeAction {
        let exact = actions.iter().find(|a| a.title == prefix);
        exact
            .or_else(|| actions.iter().find(|a| a.title.starts_with(prefix)))
            .cloned()
            .unwrap_or_else(|| panic!("no `{prefix}` in {:?}", titles(actions)))
    }

    fn apply(doc: &Document, action: &CodeAction) -> String {
        let changes = action.edit.as_ref().unwrap().changes.as_ref().unwrap();
        assert_eq!(changes.len(), 1);
        let mut spans: Vec<(Range<usize>, &str)> =
            changes[&doc.uri].iter().map(|e| (doc.byte_range(e.range), e.new_text.as_str())).collect();
        spans.sort_by_key(|(r, _)| (r.start, r.end));
        assert!(spans.windows(2).all(|w| w[0].0.end <= w[1].0.start), "overlapping edits");
        let mut text = doc.text.clone();
        for (range, new) in spans.into_iter().rev() {
            text.replace_range(range, new);
        }
        text
    }

    fn assert_equivalent(before: &str, after: &str) {
        let a = LpProblem::parse(before).unwrap();
        let b = LpProblem::parse(after).unwrap();
        let diff = a.diff(&b, &DiffOptions::default());
        assert!(diff.is_empty(), "{diff:?}\n--- after ---\n{after}");
    }

    #[test]
    fn objective_quadratic_gets_divisor() {
        let d = doc("min\n obj: x + [ x ^ 2 + 3 x * y ]\nst\n c: x + y >= 1\nend\n");
        let actions = at(&d, "[");
        let keep = apply(&d, &find(&actions, "Append `/ 2` and double"));
        assert_eq!(keep, "min\n obj: x + [ 2 x ^ 2 + 6 x * y ] / 2\nst\n c: x + y >= 1\nend\n");
        let halve = apply(&d, &find(&actions, "Append `/ 2` (halves"));
        assert!(halve.contains("[ x ^ 2 + 3 x * y ] / 2"));
        assert!(LpProblem::parse(&keep).is_ok());

        let fine = doc("min\n obj: [ x ^ 2 ] / 2\nst\n c: x >= 1\nend\n");
        assert_eq!(titles(&at(&fine, "[")), Vec::<&str>::new());
    }

    #[test]
    fn constraint_quadratic_loses_divisor() {
        let d = doc("min\n obj: x\nst\n q: [ x ^ 2 + 4 x * y ] / 2 <= 4\nend\n");
        let actions = at(&d, "[");
        assert_eq!(apply(&d, &find(&actions, "Remove `/ 2` and divide")), "min\n obj: x\nst\n q: [ 0.5 x ^ 2 + 2 x * y ] <= 4\nend\n");
        let plain = apply(&d, &find(&actions, "Remove `/ 2`"));
        assert!(plain.contains("q: [ x ^ 2 + 4 x * y ] <= 4"));
        assert!(LpProblem::parse(&plain).is_ok());

        let fine = doc("min\n obj: x\nst\n q: [ x ^ 2 ] <= 4\nend\n");
        assert_eq!(titles(&at(&fine, "[")), Vec::<&str>::new());
    }

    #[test]
    fn indicator_value_is_set_to_zero_or_one() {
        let d = doc("min\n obj: x\nst\n i: b = 2 -> x <= 3\nend\n");
        let actions = at(&d, "2 ->");
        assert!(apply(&d, &find(&actions, "Set indicator value to 1")).contains("b = 1 ->"));
        assert!(apply(&d, &find(&actions, "Set indicator value to 0")).contains("b = 0 ->"));
        let fine = doc("min\n obj: x\nst\n i: b = 1 -> x <= 3\nend\n");
        assert_eq!(titles(&at(&fine, "1 ->")), Vec::<&str>::new());
    }

    #[test]
    fn duplicate_name_gets_first_free_suffix() {
        let d = doc("min\n obj: x\nst\n c1: x >= 1\n c1_2: x >= 0\n c1: x <= 2\nend\n");
        let offset = d.text.rfind("c1:").unwrap();
        let diagnostic = Diagnostic {
            range: d.range(offset..offset + 2),
            severity: Some(DiagnosticSeverity::ERROR),
            code: Some(NumberOrString::String(codes::DUPLICATE_NAME.to_owned())),
            message: "duplicate".to_owned(),
            ..Diagnostic::default()
        };
        let context = CodeActionContext { diagnostics: vec![diagnostic.clone()], ..CodeActionContext::default() };
        let actions = run_with(&d, offset..offset, &context);
        let rename = find(&actions, "Rename duplicate `c1` to `c1_3`");
        assert_eq!(rename.kind, Some(CodeActionKind::QUICKFIX));
        assert_eq!(rename.diagnostics, Some(vec![diagnostic]));
        assert!(apply(&d, &rename).ends_with(" c1_3: x <= 2\nend\n"));
        // The first definition is not a duplicate.
        assert!(!titles(&at(&d, "c1:")).iter().any(|t| t.starts_with("Rename")));
    }

    #[test]
    fn operator_spelling_single_and_all() {
        let d = doc("min\n obj: x\nst\n a: x =< 1\n b: x => 0\n c: x <= 5\nend\n");
        let actions = at(&d, "=<");
        assert!(apply(&d, &find(&actions, "Replace `=<` with `<=`")).contains("a: x <= 1\n b: x => 0"));
        assert_eq!(apply(&d, &find(&actions, "Normalise all")), "min\n obj: x\nst\n a: x <= 1\n b: x >= 0\n c: x <= 5\nend\n");
        assert!(!titles(&at(&d, "<= 5")).iter().any(|t| t.starts_with("Replace") || t.starts_with("Normalise")));
    }

    #[test]
    fn unused_declarations_are_removed() {
        let d = doc("min\n obj: x\nst\n c: x >= 1\nbounds\n x <= 4\n z <= 3\ngenerals\n x z\nend\n");
        let bound = apply(&d, &find(&at(&d, "z <="), "Remove unused bound for `z`"));
        assert_eq!(bound, "min\n obj: x\nst\n c: x >= 1\nbounds\n x <= 4\ngenerals\n x z\nend\n");
        let entry = apply(&d, &find(&at(&d, "z\nend"), "Remove unused `generals` entry for `z`"));
        assert!(entry.ends_with("generals\n x\nend\n"), "{entry}");
        assert!(!titles(&at(&d, "x <= 4")).iter().any(|t| t.starts_with("Remove")));
    }

    #[test]
    fn duplicate_type_declaration_is_removed() {
        let d = doc("min\n obj: x + y\nst\n c: x + y >= 1\ngenerals\n x y\nbinaries\n x\nsemi-continuous\n y\ngenerals\n y\nend\n");
        let conflict = find(&at(&d, "x\nsemi"), "Remove conflicting `binaries` entry for `x` (keeps `generals`)");
        assert!(apply(&d, &conflict).contains("binaries\nsemi-continuous"));
        let repeat = find(&at(&d, "y\nend"), "Remove duplicate `generals` entry for `y`");
        assert!(apply(&d, &repeat).ends_with("generals\nend\n"));
        // Semi-integer (generals + semi-continuous) is not a conflict.
        assert!(!titles(&at(&d, "y\ngenerals")).iter().any(|t| t.starts_with("Remove")));
    }

    #[test]
    fn adds_bound_to_existing_or_new_section() {
        let with = doc("min\n obj: x + y\nst\n c: x + y >= 1\nbounds\n  x <= 4\ngenerals\n y\nend\n");
        let added = apply(&with, &find(&at(&with, "y >="), "Add bound for `y`"));
        assert_eq!(added, "min\n obj: x + y\nst\n c: x + y >= 1\nbounds\n  x <= 4\n  0 <= y <= 1e30\ngenerals\n y\nend\n");
        // Numerically the default bounds; upstream records them as explicit.
        assert!(LpProblem::parse(&added).is_ok());
        assert!(!titles(&at(&with, "x +")).iter().any(|t| t.starts_with("Add bound")));

        let without = doc("min\n obj: x\nst\n c: x >= 1\n\\ integer part\ngenerals\n x\nend\n");
        let created = apply(&without, &find(&at(&without, "x >="), "Add bound for `x`"));
        assert_eq!(created, "min\n obj: x\nst\n c: x >= 1\nBounds\n 0 <= x <= 1e30\n\\ integer part\ngenerals\n x\nend\n");
    }

    #[test]
    fn moves_variable_between_type_sections() {
        let d = doc("min\n obj: x + y\nst\n c: x + y >= 1\ngenerals\n x y\nsos\n s1: S1 :: x : 1 y : 2\nend\n");
        let actions = at(&d, "y\nsos");
        let to_binaries = apply(&d, &find(&actions, "Move `y` to `binaries`"));
        assert_eq!(to_binaries, "min\n obj: x + y\nst\n c: x + y >= 1\ngenerals\n x\nBinaries\n y\nsos\n s1: S1 :: x : 1 y : 2\nend\n");
        assert!(LpProblem::parse(&to_binaries).is_ok());
        assert!(!titles(&actions).iter().any(|t| t.contains("to `generals`")));

        let existing = doc("min\n obj: x + y\nst\n c: x + y >= 1\ngenerals\n x y\nintegers\n z\nend\n");
        let moved = apply(&existing, &find(&at(&existing, "x y"), "Move `x` to `integers`"));
        assert!(moved.ends_with("generals\n y\nintegers\n z\n x\nend\n"), "{moved}");
        // Not a type entry: no move.
        assert!(!titles(&at(&d, "x + y >=")).iter().any(|t| t.starts_with("Move")));
    }

    #[test]
    fn names_unnamed_constraints_like_upstream() {
        let text = "min\n obj: x + y\nst\n x + y >= 1\n C2: x <= 4\n y <= 3\n c: 2 <= x <= 8\n x - y >= -5\nlazy constraints\n x + y <= 20\ngeneral constraints\n r = MAX ( x , y )\nend\n";
        let d = doc(text);
        let action = find(&at(&d, "x + y >= 1"), "Name 5 unnamed constraints");
        let named = apply(&d, &action);
        assert_eq!(
            named,
            "min\n obj: x + y\nst\n C1: x + y >= 1\n C2: x <= 4\n C3: y <= 3\n c: 2 <= x <= 8\n C4: x - y >= -5\nlazy constraints\n C6: x + y <= 20\ngeneral constraints\n C5: r = MAX ( x , y )\nend\n"
        );
        assert_equivalent(text, &named);

        let all_named = doc("min\n obj: x\nst\n c: x >= 1\nend\n");
        assert!(!titles(&at(&all_named, "c:")).iter().any(|t| t.starts_with("Name")));
    }

    #[test]
    fn sorts_type_section_entries() {
        let d = doc("min\n obj: x\nst\n c: x + y + z >= 1\ngenerals\n z x\n y\nend\n");
        let sorted = apply(&d, &find(&at(&d, "z x"), "Sort `generals` entries"));
        assert!(sorted.ends_with("generals\n x y\n z\nend\n"), "{sorted}");
        let commented = doc("min\n obj: x\nst\n c: x + y >= 1\ngenerals\n y \\ note\n x\nend\n");
        assert!(!titles(&at(&commented, "y \\")).iter().any(|t| t.starts_with("Sort")));
        let already = doc("min\n obj: x\nst\n c: x + y >= 1\ngenerals\n x y\nend\n");
        assert!(!titles(&at(&already, "x y")).iter().any(|t| t.starts_with("Sort")));
    }

    #[test]
    fn flipped_constraint_to_standard_form() {
        let text = "min\n obj: x\nst\n c: - 10 >= x + y\n r: 2 <= x <= 8\nend\n";
        let d = doc(text);
        let flipped = apply(&d, &find(&at(&d, "- 10"), "Rewrite as `x + y <= -10`"));
        assert!(flipped.contains(" c: x + y <= -10\n"));
        assert_equivalent(text, &flipped);
        assert!(!titles(&at(&d, "2 <=")).iter().any(|t| t.starts_with("Rewrite")));
        assert!(!titles(&at(&doc("min\n obj: x\nst\n c: x >= 1\nend\n"), "x >=")).iter().any(|t| t.starts_with("Rewrite")));
    }

    #[test]
    fn organises_sections_with_their_comments() {
        let text = "\\ model header\nmin\n obj: x + y + z\nst\n c: x + y + z >= 1\n\n\\ types\ngenerals\n x\n\nsos\n s1: S1 :: y : 1 z : 2\n\\ the bounds\n\\ more\nbounds\n x <= 4\nlazy constraints\n l: x <= 9\n\\ trailer\nend\n";
        let d = doc(text);
        let action = find(&at(&d, "obj"), "Organise sections");
        assert_eq!(action.kind, Some(CodeActionKind::new(ORGANIZE_SECTIONS)));
        let organised = apply(&d, &action);
        assert_eq!(
            organised,
            "\\ model header\nmin\n obj: x + y + z\nst\n c: x + y + z >= 1\n\nlazy constraints\n l: x <= 9\n\\ the bounds\n\\ more\nbounds\n x <= 4\n\\ types\ngenerals\n x\n\nsos\n s1: S1 :: y : 1 z : 2\n\\ trailer\nend\n"
        );
        assert_equivalent(text, &organised);
        assert!(organise_sections(&doc(&organised)).is_none());

        let broken = doc("min\n obj: x\nst\n c: x >= \ngenerals\n x\nbounds\n x <= 1\nend\n");
        assert!(organise_sections(&broken).is_none());
    }

    #[test]
    fn only_filter_limits_kinds() {
        let d = doc("min\n obj: x\nst\n c: 10 >= x =< 3\n d: 5 >= x\nbounds\n x <= 2\nsos\n s: S1 :: x : 1\ngenerals\n x\nend\n");
        let offset = d.text.find("5 >=").unwrap();
        let only = |kinds: Vec<CodeActionKind>| CodeActionContext { only: Some(kinds), ..CodeActionContext::default() };
        let quick = run_with(&d, offset..offset, &only(vec![CodeActionKind::QUICKFIX]));
        assert!(quick.iter().all(|a| a.kind == Some(CodeActionKind::QUICKFIX)));
        let refactor = run_with(&d, offset..offset, &only(vec![CodeActionKind::REFACTOR]));
        assert!(refactor.iter().any(|a| a.title.starts_with("Rewrite as")));
        assert!(refactor.iter().all(|a| a.kind.as_ref().is_some_and(|k| k.as_str().starts_with("refactor"))));
        let source = run_with(&d, offset..offset, &only(vec![CodeActionKind::SOURCE]));
        assert_eq!(titles(&source), ["Organise sections"]);
    }

    #[test]
    fn deletion_shapes() {
        let text = "a\n x y\n z\n";
        assert_eq!(deletion(text, &(3..4)), (3..5, String::new()));
        assert_eq!(deletion(text, &(5..6)), (4..6, String::new()));
        assert_eq!(deletion(text, &(8..9)), (7..10, String::new()));
    }
}
