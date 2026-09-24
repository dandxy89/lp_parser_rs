//! Diagnostics: syntax (tree-sitter ERROR/MISSING), index-based checks, semantic
//! (`lp_parser_rs` errors, only without syntax errors) and analysis issues.

use std::collections::HashSet;
use std::ops::Range;

use lp_parser_rs::analysis::{AnalysisIssue, IssueCategory, IssueSeverity, IssueSubject};
use lp_parser_rs::{EntityKind as UpstreamKind, LpParseError, VariableBounds};
use tower_lsp_server::ls_types::{Diagnostic, DiagnosticRelatedInformation, DiagnosticSeverity, DiagnosticTag, NumberOrString};
use tree_sitter::Node;

use crate::config::Config;
use crate::document::Document;
use crate::index::{Namespace, Occurrence, Role};
use crate::position::floor_char_boundary;
use crate::semantic::{Model, SemanticResult};
use crate::syntax::{self, kind};

/// Diagnostic `code`s. Code actions match on these.
pub mod codes {
    /// tree-sitter `ERROR` node.
    pub const SYNTAX_ERROR: &str = "syntax-error";
    /// tree-sitter `MISSING` node.
    pub const MISSING_TOKEN: &str = "missing-token";
    /// `lp_parser_rs` parse/assembly error.
    pub const PARSE_ERROR: &str = "parse-error";
    /// Duplicate constraint/objective/SOS name.
    pub const DUPLICATE_NAME: &str = "duplicate-name";
    /// Variable in more than one type section.
    pub const CONFLICTING_TYPE: &str = "conflicting-type";
    /// Bound or type declaration for a variable used nowhere.
    pub const UNUSED_DECLARATION: &str = "unused-declaration";
    /// Lower bound above upper bound.
    pub const CONFLICTING_BOUNDS: &str = "conflicting-bounds";
    /// `=<` / `=>` spelling.
    pub const OPERATOR_SPELLING: &str = "operator-spelling";
    /// Prefix for analysis issues: `analysis/<category>`.
    pub const ANALYSIS_PREFIX: &str = "analysis/";
}

/// Longest source excerpt quoted in a syntax-error message, in characters.
const EXCERPT_CHARS: usize = 40;

/// Bound magnitude from which upstream treats a value as infinite.
const INFINITE_BOUND: f64 = 1e30;

/// All diagnostics for `doc` under `config`.
#[must_use]
pub fn compute(doc: &Document, config: &Config) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    syntax_diagnostics(doc, &mut out);
    if !doc.has_syntax_errors() {
        semantic_error(doc, &mut out);
    }
    duplicate_names(doc, &mut out);
    conflicting_types(doc, &mut out);
    let unused = unused_declarations(doc, &mut out);
    let conflicting = conflicting_bounds(doc, &mut out);
    if config.analysis.enabled
        && let Some(model) = doc.semantic().and_then(SemanticResult::model)
    {
        for issue in &model.analysis.issues {
            // Skip issues the index-based checks already report at the source.
            let reported = match issue.category {
                IssueCategory::UnusedVariable => Some(&unused),
                IssueCategory::InvalidBounds => Some(&conflicting),
                _ => None,
            };
            let subject = issue.subject.as_ref().filter(|s| s.kind == UpstreamKind::Variable);
            if !reported.zip(subject).is_some_and(|(names, s)| names.contains(s.name.as_str())) {
                analysis_issue(issue, doc, model, &mut out);
            }
        }
    }
    out
}

fn diagnostic(doc: &Document, range: Range<usize>, severity: DiagnosticSeverity, code: &str, message: String) -> Diagnostic {
    Diagnostic {
        range: doc.range(range),
        severity: Some(severity),
        code: Some(NumberOrString::String(code.to_owned())),
        source: Some("lp".to_owned()),
        message,
        ..Diagnostic::default()
    }
}

fn related(doc: &Document, range: Range<usize>, message: impl Into<String>) -> DiagnosticRelatedInformation {
    DiagnosticRelatedInformation { location: doc.location(range), message: message.into() }
}

/// `ERROR` and `MISSING` nodes (one diagnostic per `ERROR`, its subtree
/// skipped) and `=<` / `=>` operator spellings, in one walk.
fn syntax_diagnostics(doc: &Document, out: &mut Vec<Diagnostic>) {
    let mut cursor = doc.tree.walk();
    loop {
        let node = cursor.node();
        let mut descend = true;
        if node.is_error() {
            out.push(unexpected(doc, node));
            descend = false;
        } else if node.is_missing() {
            let expected = if node.is_named() { node.kind().replace('_', " ") } else { format!("`{}`", node.kind()) };
            out.push(diagnostic(doc, node.byte_range(), DiagnosticSeverity::ERROR, codes::MISSING_TOKEN, format!("missing {expected}")));
        } else if node.kind() == kind::COMPARISON_OPERATOR {
            let preferred = match doc.node_text(node) {
                "=<" => Some("<="),
                "=>" => Some(">="),
                _ => None,
            };
            if let Some(preferred) = preferred {
                let message = format!("`{}` is a non-standard spelling of `{preferred}`", doc.node_text(node));
                out.push(diagnostic(doc, node.byte_range(), DiagnosticSeverity::INFORMATION, codes::OPERATOR_SPELLING, message));
            }
            descend = false;
        }
        if descend && cursor.goto_first_child() {
            continue;
        }
        loop {
            if cursor.goto_next_sibling() {
                break;
            }
            if !cursor.goto_parent() {
                return;
            }
        }
    }
}

/// "unexpected …" for an `ERROR` node, underlining only its first line.
fn unexpected(doc: &Document, node: Node<'_>) -> Diagnostic {
    debug_assert!(node.is_error());
    let start = node.start_byte();
    let line_end = doc.lines.line_range(&doc.text, doc.lines.line_of(start)).end;
    let end = node.end_byte().min(line_end.max(start));
    let first_line = doc.slice(start..end).trim();
    let message = if first_line.is_empty() {
        "unexpected input".to_owned()
    } else {
        let mut excerpt: String = first_line.chars().take(EXCERPT_CHARS).collect();
        if excerpt.len() < first_line.len() || end < node.end_byte() {
            excerpt.push('…');
        }
        format!("unexpected `{excerpt}`")
    };
    diagnostic(doc, start..end, DiagnosticSeverity::ERROR, codes::SYNTAX_ERROR, message)
}

/// The upstream parse error for the current version, if any.
fn semantic_error(doc: &Document, out: &mut Vec<Diagnostic>) {
    let Some(Err(error)) = doc.semantic().map(|r| &r.outcome) else { return };
    let message = match error {
        LpParseError::ParseError { message, .. } => message.clone(),
        other => other.to_string(),
    };
    let range = match error.position() {
        Some(position) => {
            let offset = floor_char_boundary(&doc.text, position.min(doc.text.len()));
            syntax::token_at(&doc.tree, offset).map_or(offset..offset, |token| token.byte_range())
        }
        None => doc.lines.line_range(&doc.text, 0),
    };
    out.push(diagnostic(doc, range, DiagnosticSeverity::ERROR, codes::PARSE_ERROR, message));
}

fn duplicate_names(doc: &Document, out: &mut Vec<Diagnostic>) {
    let entities = &doc.index().entities;
    for duplicate in &doc.index().duplicates {
        let (first, entity) = (&entities[duplicate.first], &entities[duplicate.duplicate]);
        debug_assert_eq!(first.name, entity.name, "duplicates share a name");
        debug_assert!(entity.name.is_some(), "only named entities can be duplicates");
        let Some(name) = entity.name.as_deref() else { continue };
        let message = format!("duplicate {} name `{name}`", entity.kind.label());
        let mut d = diagnostic(
            doc,
            entity.name_range.clone().unwrap_or_else(|| entity.range.clone()),
            DiagnosticSeverity::ERROR,
            codes::DUPLICATE_NAME,
            message,
        );
        let first_range = first.name_range.clone().unwrap_or_else(|| first.range.clone());
        d.related_information = Some(vec![related(doc, first_range, format!("first defined here as {}", first.kind.label()))]);
        out.push(d);
    }
}

/// Type section named by a type-declaration role.
const fn type_section(role: Role) -> &'static str {
    match role {
        Role::Generals => "generals",
        Role::Integers => "integers",
        Role::Binaries => "binaries",
        _ => "semi-continuous",
    }
}

fn conflicting_types(doc: &Document, out: &mut Vec<Diagnostic>) {
    for variable in &doc.index().variables {
        let declarations: Vec<&Occurrence> = variable.occurrences.iter().filter(|o| o.role.is_type_declaration()).collect();
        let Some(first) = declarations.first() else { continue };
        if declarations.iter().all(|o| o.role == first.role) {
            continue;
        }
        let mut sections: Vec<&str> = declarations.iter().map(|o| type_section(o.role)).collect();
        sections.dedup();
        let message = format!("`{}` is declared in more than one type section: {}", variable.name, sections.join(", "));
        for occurrence in &declarations {
            let mut d = diagnostic(doc, occurrence.range.clone(), DiagnosticSeverity::WARNING, codes::CONFLICTING_TYPE, message.clone());
            let others = declarations
                .iter()
                .filter(|o| o.role != occurrence.role)
                .map(|o| related(doc, o.range.clone(), format!("also declared in {}", type_section(o.role))));
            d.related_information = Some(others.collect());
            out.push(d);
        }
    }
}

/// Warn on every declaration of a variable no objective or constraint uses.
/// Returns the names reported.
fn unused_declarations<'d>(doc: &'d Document, out: &mut Vec<Diagnostic>) -> HashSet<&'d str> {
    let mut reported = HashSet::new();
    for variable in doc.index().variables.iter().filter(|v| !v.is_used()) {
        let message = format!("`{}` is declared but not used in any objective or constraint", variable.name);
        for occurrence in &variable.occurrences {
            debug_assert!(occurrence.role.is_declaration(), "unused variables only have declarations");
            let mut d = diagnostic(doc, occurrence.range.clone(), DiagnosticSeverity::WARNING, codes::UNUSED_DECLARATION, message.clone());
            d.tags = Some(vec![DiagnosticTag::UNNECESSARY]);
            out.push(d);
        }
        reported.insert(variable.name.as_str());
    }
    reported
}

/// Warn where a variable's merged bound entries leave lower above upper.
/// Returns the names reported.
fn conflicting_bounds<'d>(doc: &'d Document, out: &mut Vec<Diagnostic>) -> HashSet<&'d str> {
    let mut reported = HashSet::new();
    for variable in &doc.index().variables {
        let declarations: Vec<(Range<usize>, VariableBounds)> = variable
            .occurrences
            .iter()
            .filter(|o| o.role == Role::Bound)
            .filter_map(|o| doc.node(o.range.clone(), kind::IDENTIFIER)?.parent())
            .filter(|n| n.kind() == kind::BOUND_DECLARATION)
            // Entries that do not parse as a bound are syntax errors, reported elsewhere.
            .filter_map(|n| Some((n.byte_range(), declared_bounds(n, &doc.text)?)))
            .collect();
        let merged = declarations.iter().fold(VariableBounds::unspecified(), |acc, (_, b)| acc.merge(*b));
        let (Some(lower), Some(upper)) = (merged.lower, merged.upper) else { continue };
        let Some((last, others)) = declarations.split_last().filter(|_| lower > upper) else { continue };
        let message = format!("`{}` has lower bound {lower} above upper bound {upper}", variable.name);
        let mut d = diagnostic(doc, last.0.clone(), DiagnosticSeverity::WARNING, codes::CONFLICTING_BOUNDS, message);
        d.related_information = Some(others.iter().map(|(range, _)| related(doc, range.clone(), "also bounded here")).collect());
        out.push(d);
        reported.insert(variable.name.as_str());
    }
    reported
}

/// Bounds set by one `bound_declaration`, as the upstream grammar reads it;
/// `None` for shapes upstream rejects.
fn declared_bounds(node: Node<'_>, text: &str) -> Option<VariableBounds> {
    #[derive(Clone, Copy)]
    enum Item {
        Variable,
        Free,
        /// `Some(true)` for `<=`-like, `Some(false)` for `>=`-like, `None` for `=`.
        Operator(Option<bool>),
        Value(f64),
    }
    debug_assert_eq!(node.kind(), kind::BOUND_DECLARATION);
    let mut items = Vec::new();
    let mut negative = false;
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        let source = syntax::text(child, text);
        match child.kind() {
            "-" => negative = true,
            kind::IDENTIFIER => items.push(Item::Variable),
            kind::FREE_KEYWORD => items.push(Item::Free),
            kind::COMPARISON_OPERATOR => items.push(Item::Operator(match source {
                "<=" | "=<" | "<" => Some(true),
                ">=" | "=>" | ">" => Some(false),
                _ => None,
            })),
            kind::NUMBER | kind::INFINITY => {
                let value = syntax::parse_number(source)?;
                let value = if negative { -value } else { value };
                negative = false;
                items.push(Item::Value(if value >= INFINITE_BOUND {
                    f64::INFINITY
                } else if value <= -INFINITE_BOUND {
                    f64::NEG_INFINITY
                } else {
                    value
                }));
            }
            _ => {}
        }
    }
    let bounds = match items.as_slice() {
        [Item::Variable, Item::Free] => VariableBounds::free(),
        [Item::Variable, Item::Operator(le), Item::Value(v)] => match le {
            Some(true) => VariableBounds::upper(*v),
            Some(false) => VariableBounds::lower(*v),
            None => VariableBounds::range(*v, *v),
        },
        [Item::Value(v), Item::Operator(le), Item::Variable] => match le {
            Some(true) => VariableBounds::lower(*v),
            Some(false) => VariableBounds::upper(*v),
            None => VariableBounds::range(*v, *v),
        },
        [Item::Value(a), Item::Operator(Some(true)), Item::Variable, Item::Operator(Some(true)), Item::Value(b)] => {
            VariableBounds::range(*a, *b)
        }
        [Item::Value(a), Item::Operator(Some(false)), Item::Variable, Item::Operator(Some(false)), Item::Value(b)] => {
            VariableBounds::range(*b, *a)
        }
        _ => return None,
    };
    Some(bounds)
}

fn analysis_issue(issue: &AnalysisIssue, doc: &Document, model: &Model, out: &mut Vec<Diagnostic>) {
    let severity = match issue.severity {
        IssueSeverity::Error => DiagnosticSeverity::ERROR,
        IssueSeverity::Warning => DiagnosticSeverity::WARNING,
        IssueSeverity::Info => DiagnosticSeverity::INFORMATION,
    };
    let code = format!("{}{}", codes::ANALYSIS_PREFIX, issue.category.to_string().to_lowercase().replace(' ', "-"));
    let range = issue.subject.as_ref().and_then(|s| subject_range(doc, model, s)).unwrap_or_else(|| sense_range(doc));
    // Coefficient issues carry the location name as details; it is the subject already.
    let message = match &issue.details {
        Some(details) if issue.subject.as_ref().is_none_or(|s| s.variable.is_none()) => format!("{}\n{details}", issue.message),
        _ => issue.message.clone(),
    };
    let d = diagnostic(doc, range, severity, &code, message);
    // Both halves of a ranged constraint repeat the same coefficient issue.
    if out.last() != Some(&d) {
        out.push(d);
    }
}

/// Source range of an issue's subject: a variable's definition, or an
/// entity's label (whole entity if unnamed), narrowed to `variable` inside it.
fn subject_range(doc: &Document, model: &Model, subject: &IssueSubject) -> Option<Range<usize>> {
    let (namespace, offset) = match subject.kind {
        UpstreamKind::Variable => return doc.index().variable(&subject.name).map(|v| v.definition().range.clone()),
        UpstreamKind::Constraint => {
            let constraint = model.problem.name_id(&subject.name).and_then(|id| model.problem.constraints.get(&id));
            (Namespace::Constraint, constraint.and_then(lp_parser_rs::model::Constraint::byte_offset))
        }
        UpstreamKind::Objective => {
            let objective = model.problem.name_id(&subject.name).and_then(|id| model.problem.objectives.get(&id));
            (Namespace::Objective, objective.and_then(|o| o.byte_offset))
        }
    };
    let index = doc.index();
    let entity = offset
        .and_then(|o| index.entity_at(o))
        .filter(|&e| index.entities[e].kind.namespace() == namespace)
        .or_else(|| index.entities_named(&subject.name, namespace).next().map(|(e, _)| e))?;
    let in_entity = subject
        .variable
        .as_deref()
        .and_then(|name| index.variable(name))
        .and_then(|v| v.occurrences.iter().find(|o| o.entity == Some(entity)))
        .map(|o| o.range.clone());
    let entity = &index.entities[entity];
    Some(in_entity.or_else(|| entity.name_range.clone()).unwrap_or_else(|| entity.range.clone()))
}

/// The sense keyword, else the first line.
fn sense_range(doc: &Document) -> Range<usize> {
    let root = doc.tree.root_node();
    let mut cursor = root.walk();
    let sense = root.named_children(&mut cursor).find(|n| n.kind() == kind::SENSE);
    sense.map_or_else(|| doc.lines.line_range(&doc.text, 0), |n| n.byte_range())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use lp_parser_rs::analysis::AnalysisConfig;

    use super::*;
    use crate::position::Encoding;

    const CLEAN: &str = "Minimize\n obj: x + y + z\nSubject To\n c1: x + y + z >= 1\nBounds\n x <= 10\nGenerals\n y\nEnd\n";

    fn document(text: &str) -> Document {
        let mut doc = Document::new("file:///test.lp".parse().unwrap(), text.to_owned(), 1, Encoding::Utf16);
        doc.semantic_result = Some(Arc::new(crate::semantic::run(text, 1, &AnalysisConfig::default())));
        doc
    }

    fn without_analysis() -> Config {
        let mut config = Config::default();
        config.analysis.enabled = false;
        config
    }

    /// `(code, covered text, message)` for each diagnostic.
    fn summary(doc: &Document, config: &Config) -> Vec<(String, String, String)> {
        compute(doc, config)
            .into_iter()
            .map(|d| {
                let Some(NumberOrString::String(code)) = d.code else { panic!("string code expected") };
                assert_eq!(d.source.as_deref(), Some("lp"));
                (code, doc.slice(doc.byte_range(d.range)).to_owned(), d.message)
            })
            .collect()
    }

    fn only(doc: &Document, code: &str) -> Vec<Diagnostic> {
        compute(doc, &Config::default()).into_iter().filter(|d| d.code == Some(NumberOrString::String(code.to_owned()))).collect()
    }

    fn covered(doc: &Document, d: &Diagnostic) -> String {
        doc.slice(doc.byte_range(d.range)).to_owned()
    }

    #[test]
    fn clean_file_has_no_diagnostics() {
        let doc = document(CLEAN);
        assert!(doc.semantic().unwrap().model().is_some());
        assert_eq!(compute(&doc, &Config::default()), []);
    }

    #[test]
    fn error_node_reports_offending_text_once() {
        let doc = document("min\n obj: x\nst\n c1: x >= >= 1\nend\n");
        // The upstream error for the same input is suppressed.
        assert!(doc.semantic().unwrap().outcome.is_err());
        assert_eq!(summary(&doc, &without_analysis()), [(codes::SYNTAX_ERROR.into(), ">=".into(), "unexpected `>=`".into())]);
    }

    #[test]
    fn long_error_excerpt_is_truncated() {
        let long = format!("{} ", "c".repeat(60)).repeat(3);
        let doc = document(&format!("min\n obj: x\nst\n c1 x {long} >= 1\nend\n"));
        let errors = only(&doc, codes::SYNTAX_ERROR);
        assert!(errors.iter().all(|d| d.message.chars().count() <= EXCERPT_CHARS + "unexpected ``…".len()), "{errors:?}");
        assert!(errors.iter().any(|d| d.message.ends_with("…`")), "{errors:?}");
    }

    #[test]
    fn missing_node_names_expected_kind() {
        let doc = document("min\n obj: x\nst\n c1: x >=\nend\n");
        assert_eq!(summary(&doc, &without_analysis()), [(codes::MISSING_TOKEN.into(), String::new(), "missing number".into())]);
    }

    #[test]
    fn semantic_error_spans_token_without_snippet() {
        let doc = document("min\n obj: x\nst\n c1: [ x ^ 3 ] <= 4\nend\n");
        assert!(!doc.has_syntax_errors());
        assert_eq!(
            summary(&doc, &Config::default()),
            [(codes::PARSE_ERROR.into(), "3".into(), "only squares ('x ^ 2') are quadratic, not 'x ^ 3'".into())]
        );
    }

    #[test]
    fn semantic_error_without_position_uses_first_line() {
        let mut doc = document(CLEAN);
        doc.semantic_result = Some(Arc::new(SemanticResult { version: 1, outcome: Err(LpParseError::validation_error("bad model")) }));
        assert_eq!(
            summary(&doc, &Config::default()),
            [(codes::PARSE_ERROR.into(), "Minimize".into(), "Validation error: bad model".into())]
        );
    }

    #[test]
    fn stale_semantic_result_is_ignored() {
        let mut doc = document("min\n obj: x\nst\n c1: [ x ^ 3 ] <= 4\nend\n");
        doc.version = 2;
        assert_eq!(compute(&doc, &Config::default()), []);
    }

    #[test]
    fn duplicate_names_point_at_first_definition() {
        let doc = document("min\n c1: x + y\nst\n c1: x + y >= 1\n c2: x - y <= 3\nsos\n c1: S1 :: x : 1 y : 2\nend\n");
        let duplicates = only(&doc, codes::DUPLICATE_NAME);
        // The objective `c1` is in another namespace; only the SOS set clashes.
        assert_eq!(duplicates.len(), 1);
        let d = &duplicates[0];
        assert_eq!(d.severity, Some(DiagnosticSeverity::ERROR));
        assert_eq!(d.message, "duplicate SOS set name `c1`");
        let first = doc.text.find(" c1: x + y >=").unwrap() + 1;
        let related = d.related_information.as_ref().unwrap();
        assert_eq!(related.len(), 1);
        assert_eq!(doc.byte_range(related[0].location.range), first..first + 2);

        assert_eq!(only(&document(CLEAN), codes::DUPLICATE_NAME), []);
    }

    #[test]
    fn conflicting_types_flag_every_declaration() {
        let doc = document("min\n obj: x + y\nst\n c1: x + y >= 1\ngenerals\n x y\nbinaries\n x\nend\n");
        let conflicts = only(&doc, codes::CONFLICTING_TYPE);
        assert_eq!(conflicts.len(), 2);
        assert!(conflicts.iter().all(|d| covered(&doc, d) == "x"));
        assert_eq!(conflicts[0].message, "`x` is declared in more than one type section: generals, binaries");
        assert_eq!(conflicts[0].related_information.as_ref().unwrap()[0].message, "also declared in binaries");

        let single = document("min\n obj: x\nst\n c1: x >= 1\ngenerals\n x\nend\n");
        assert_eq!(only(&single, codes::CONFLICTING_TYPE), []);
    }

    #[test]
    fn unused_declarations_are_flagged_once() {
        let doc = document("min\n obj: x\nst\n c1: x >= 1\nbounds\n w <= 4\ngenerals\n w\nend\n");
        let found: Vec<(String, String, String)> =
            summary(&doc, &Config::default()).into_iter().filter(|(code, ..)| code.contains("unused")).collect();
        // Both declarations are flagged; the upstream unused-variable issue is not repeated.
        assert_eq!(found.len(), 2, "{found:?}");
        assert!(found.iter().all(|(code, text, _)| code == codes::UNUSED_DECLARATION && text == "w"));
        assert_eq!(only(&doc, codes::UNUSED_DECLARATION)[0].tags, Some(vec![DiagnosticTag::UNNECESSARY]));

        let used = document("min\n obj: x\nst\n c1: x >= 1\nsos\n s1: S1 :: w : 1\nbounds\n w <= 4\nend\n");
        assert_eq!(only(&used, codes::UNUSED_DECLARATION), []);
    }

    #[test]
    fn conflicting_bounds_merge_like_upstream() {
        let doc = document("min\n obj: x + y\nst\n c1: x + y >= 1\nbounds\n x >= 5\n x <= 2\n 3 >= y >= 1\nend\n");
        let conflicts = only(&doc, codes::CONFLICTING_BOUNDS);
        assert_eq!(conflicts.len(), 1);
        assert_eq!(covered(&doc, &conflicts[0]), "x <= 2");
        assert_eq!(conflicts[0].message, "`x` has lower bound 5 above upper bound 2");
        assert_eq!(conflicts[0].related_information.as_ref().unwrap().len(), 1);
        // The upstream invalid-bounds issue for `x` is not repeated.
        assert_eq!(only(&doc, "analysis/invalid-bounds"), []);

        let cases = [
            ("x = 5", false),
            ("x >= 5\n x free", false),
            ("x <= 2\n x >= 1", false),
            ("x >= 1e30\n x <= 1e31", false),
            ("-5 <= x <= -inf", true),
            ("x >= -3\n -4 >= x", true),
        ];
        for (bounds, conflicting) in cases {
            let text = format!("min\n obj: x\nst\n c1: x >= 1\nbounds\n {bounds}\nend\n");
            let conflicts = only(&document(&text), codes::CONFLICTING_BOUNDS);
            assert_eq!(conflicts.len(), usize::from(conflicting), "{bounds}");
        }
    }

    #[test]
    fn non_standard_operator_spelling_is_info() {
        let doc = document("min\n obj: x + y + z\nst\n c1: x + y + z =< 4\n c2: x + y + z => 1\nend\n");
        let spelling = only(&doc, codes::OPERATOR_SPELLING);
        assert_eq!(spelling.len(), 2);
        assert!(spelling.iter().all(|d| d.severity == Some(DiagnosticSeverity::INFORMATION)));
        assert_eq!(spelling[0].message, "`=<` is a non-standard spelling of `<=`");
        assert_eq!(only(&document(CLEAN), codes::OPERATOR_SPELLING), []);
    }

    #[test]
    fn analysis_issues_are_located_at_their_subject() {
        let text =
            "min\n obj: x + y + z\nst\n 2 <= 1e12 x + y + z <= 8\n c2: x + 1e-12 y + z >= 1\n c3: x + y - z <= 4\nbounds\n z = 5\nend\n";
        let doc = document(text);
        let diagnostics = compute(&doc, &Config::default());
        let at = |needle: &str| -> Vec<(Option<NumberOrString>, String)> {
            diagnostics.iter().filter(|d| d.message.contains(needle)).map(|d| (d.code.clone(), covered(&doc, d))).collect()
        };
        let code = |c: &str| Some(NumberOrString::String(c.to_owned()));
        // Unnamed ranged constraint (two generated halves): one diagnostic on the variable.
        assert_eq!(at("Large coefficient ("), [(code("analysis/numerical-scaling"), "x".to_owned())]);
        let small = diagnostics.iter().find(|d| d.message.contains("Small coefficient (")).unwrap();
        assert_eq!(doc.byte_range(small.range).start, text.find("1e-12 y").unwrap() + 6);
        assert_eq!(at("fixed"), [(code("analysis/fixed-variable"), "z".to_owned())]);
        // Problem-wide issues go on the sense keyword.
        let over = diagnostics.iter().find(|d| d.message.contains("over-constrained")).unwrap();
        assert_eq!((over.severity, covered(&doc, over)), (Some(DiagnosticSeverity::WARNING), "min".to_owned()));

        let disabled = compute(&doc, &without_analysis());
        assert!(disabled.iter().all(|d| !matches!(&d.code, Some(NumberOrString::String(c)) if c.starts_with(codes::ANALYSIS_PREFIX))));
    }

    #[test]
    fn subject_range_resolves_labels_and_variables() {
        let doc = document("min\n obj: x + y\nst\n c1: x + y >= 1\n e1: 0 x >= -1\nend\n");
        let model = doc.semantic().unwrap().model().unwrap();
        let range = subject_range(&doc, model, &IssueSubject::constraint("e1")).unwrap();
        assert_eq!(doc.slice(range), "e1");
        let objective = IssueSubject { kind: UpstreamKind::Objective, name: "obj".into(), variable: Some("y".into()) };
        let range = subject_range(&doc, model, &objective).unwrap();
        assert_eq!(range.start, doc.text.find("+ y").unwrap() + 2);
        assert_eq!(subject_range(&doc, model, &IssueSubject::constraint("nope")), None);
    }
}
