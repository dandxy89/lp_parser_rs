//! Inlay hints: generated names, `_rng` partners, variable types, normalised RHS.
//!
//! Names come from the semantic model (so they are exactly what upstream
//! generates); types and folded right-hand sides come from the syntax tree, so
//! they are available before the first semantic pass. Only entities and name
//! sites overlapping the requested range are visited, which keeps hints cheap
//! on very large documents.

use std::ops::Range;

use lp_parser_rs::VariableBounds;
use tower_lsp_server::ls_types::{InlayHint, InlayHintKind, InlayHintLabel, InlayHintTooltip, Range as LspRange};
use tree_sitter::Node;

use crate::config::InlayHintSettings;
use crate::document::Document;
use crate::index::{EntityKind, Role, Symbol, Variable};
use crate::syntax::{self, kind};

/// Hints within `range`.
#[must_use]
pub fn hints(doc: &Document, range: LspRange, settings: &InlayHintSettings) -> Vec<InlayHint> {
    let bytes = doc.byte_range(range);
    debug_assert!(bytes.start <= bytes.end && bytes.end <= doc.text.len(), "byte range must lie within the document");
    let entities = overlapping_entities(doc, &bytes);

    let mut hints: Vec<(usize, InlayHint)> = Vec::new();
    if settings.generated_names || settings.range_partners {
        model_name_hints(doc, entities.clone(), settings, &mut hints);
    }
    if settings.normalised_rhs {
        for entity in entities {
            normalised_rhs_hint(doc, entity, &mut hints);
        }
    }
    if settings.variable_types {
        variable_type_hints(doc, &bytes, &mut hints);
    }

    hints.retain(|(offset, _)| bytes.contains(offset) || *offset == bytes.end);
    hints.sort_by_key(|(offset, _)| *offset);
    hints.into_iter().map(|(_, hint)| hint).collect()
}

/// Indices of entities overlapping `bytes`. Entities are disjoint and sorted
/// by start, so their ends are sorted too.
fn overlapping_entities(doc: &Document, bytes: &Range<usize>) -> Range<usize> {
    let entities = &doc.index().entities;
    let first = entities.partition_point(|e| e.range.end < bytes.start);
    let last = entities.partition_point(|e| e.range.start <= bytes.end);
    first..last.max(first)
}

fn hint(doc: &Document, offset: usize, label: String, tooltip: String, left: bool, right: bool) -> (usize, InlayHint) {
    typed_hint(doc, offset, label, tooltip, left, right, None)
}

fn typed_hint(
    doc: &Document,
    offset: usize,
    label: String,
    tooltip: String,
    left: bool,
    right: bool,
    kind: Option<InlayHintKind>,
) -> (usize, InlayHint) {
    let hint = InlayHint {
        position: doc.position(offset),
        label: InlayHintLabel::String(label),
        kind,
        text_edits: None,
        tooltip: Some(InlayHintTooltip::String(tooltip)),
        padding_left: Some(left),
        padding_right: Some(right),
        data: None,
    };
    (offset, hint)
}

/// Generated names and `_rng` partners, read from the current semantic model.
fn model_name_hints(doc: &Document, entities: Range<usize>, settings: &InlayHintSettings, out: &mut Vec<(usize, InlayHint)>) {
    if entities.is_empty() {
        return;
    }
    let Some(model) = doc.semantic().and_then(|s| s.model()) else { return };
    let problem = &model.problem;
    let window = doc.index().entities[entities.start].range.start..=doc.index().entities[entities.end - 1].range.end;

    // (entity, model name) in model order: a ranged constraint's lower half
    // (the written name) precedes its `_rng` partner.
    let objectives = problem.objectives.values().map(|o| (o.byte_offset, o.name));
    let constraints = problem.constraints.values().map(|c| (c.byte_offset(), c.name()));
    let mut names: Vec<(usize, &str)> = objectives
        .chain(constraints)
        .filter_map(|(offset, name)| {
            let offset = offset.filter(|o| window.contains(o))?;
            let entity = doc.index().entity_at(offset).filter(|e| entities.contains(e))?;
            Some((entity, problem.resolve(name)))
        })
        .collect();
    names.sort_by_key(|(entity, _)| *entity);

    for group in names.chunk_by(|a, b| a.0 == b.0) {
        let entity = &doc.index().entities[group[0].0];
        let generated = group[0].1;
        if settings.generated_names && entity.name.as_deref() != Some(generated) {
            match &entity.name_range {
                None => out.push(hint(
                    doc,
                    entity.range.start,
                    format!("{generated}:"),
                    format!("Unnamed {}: the parser names it `{generated}`", entity.kind.label()),
                    false,
                    true,
                )),
                Some(name_range) => out.push(hint(
                    doc,
                    name_range.end,
                    format!("→ {generated}"),
                    format!("Name already in use: the parser renames this {} to `{generated}`", entity.kind.label()),
                    true,
                    false,
                )),
            }
        }
        if settings.range_partners && entity.kind == EntityKind::Constraint {
            for &(_, partner) in &group[1..] {
                out.push(hint(
                    doc,
                    entity.range.end,
                    format!("& {partner}"),
                    format!("Ranged constraint: the upper bound becomes the separate constraint `{partner}`"),
                    true,
                    false,
                ));
            }
        }
    }
}

/// One syntactic piece of a constraint body.
#[derive(Debug, Clone, Copy)]
enum Part<'t> {
    Expression(Node<'t>),
    Operator(&'static str),
    Number(f64),
}

/// The pieces of a `constraint`, with signs applied to
/// numbers and operator aliases normalised.
fn parts<'t>(node: Node<'t>, text: &str) -> Vec<Part<'t>> {
    let mut out = Vec::new();
    let mut sign = 1.0;
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "-" => sign = -1.0,
            "+" => sign = 1.0,
            kind::NUMBER | kind::INFINITY => {
                if let Some(value) = syntax::parse_number(syntax::text(child, text)) {
                    out.push(Part::Number(sign * value));
                }
                sign = 1.0;
            }
            kind::COMPARISON_OPERATOR => out.push(Part::Operator(syntax::canonical_operator(syntax::text(child, text)))),
            kind::LINEAR_EXPRESSION => out.push(Part::Expression(child)),
            _ => {}
        }
    }
    out
}

/// Sum of the signed constant terms of a `linear_expression`, or `None` when
/// it has none.
fn expression_constant(expression: Node<'_>, text: &str) -> Option<f64> {
    let mut sign = 1.0;
    let mut total: Option<f64> = None;
    let mut cursor = expression.walk();
    for child in expression.children(&mut cursor) {
        match child.kind() {
            "-" => sign = -1.0,
            "+" => sign = 1.0,
            kind::TERM => {
                let constant = child.named_child(0).filter(|c| child.named_child_count() == 1 && c.kind() != kind::IDENTIFIER);
                if let Some(value) = constant.and_then(|c| syntax::parse_number(syntax::text(c, text))) {
                    total = Some(total.unwrap_or(0.0) + sign * value);
                }
                sign = 1.0;
            }
            _ => sign = 1.0,
        }
    }
    total
}

fn format_value(value: f64) -> String {
    if value == f64::INFINITY {
        "inf".to_owned()
    } else if value == f64::NEG_INFINITY {
        "-inf".to_owned()
    } else {
        format!("{value}")
    }
}

/// `= 7` after a constraint whose expression carries constants, folded into
/// the right-hand side as upstream does (`assemble.rs`).
fn normalised_rhs_hint(doc: &Document, entity_id: usize, out: &mut Vec<(usize, InlayHint)>) {
    let entity = &doc.index().entities[entity_id];
    if entity.kind != EntityKind::Constraint {
        return;
    }
    let Some(node) = doc.node(entity.range.clone(), kind::CONSTRAINT) else { return };
    let body = parts(node, &doc.text);
    let folded = match body.as_slice() {
        // `expr op rhs`
        [Part::Expression(e), Part::Operator(op), Part::Number(rhs)] => {
            expression_constant(*e, &doc.text).map(|c| (format!("= {}", format_value(rhs - c)), format!("{op} {}", format_value(rhs - c))))
        }
        // `lhs op expr`: upstream flips the operator.
        [Part::Number(lhs), Part::Operator(op), Part::Expression(e)] => expression_constant(*e, &doc.text)
            .map(|c| (format!("= {}", format_value(lhs - c)), format!("{} {}", syntax::flip_operator(op), format_value(lhs - c)))),
        // `lo op expr op hi`
        [Part::Number(lo), Part::Operator(op1), Part::Expression(e), Part::Operator(op2), Part::Number(hi)] => {
            expression_constant(*e, &doc.text).map(|c| {
                let (lo, hi) = (format_value(lo - c), format_value(hi - c));
                (format!("= {lo} .. {hi}"), format!("{} {lo} and {op2} {hi}", syntax::flip_operator(op1)))
            })
        }
        _ => None,
    };
    let Some((label, normalised)) = folded else { return };
    if label.contains("NaN") {
        return;
    }
    out.push(hint(
        doc,
        entity.range.end,
        label,
        format!("Left-hand-side constants folded into the right-hand side: expression {normalised}"),
        true,
        false,
    ));
}

/// `: int`, `: bin`, `: semi`, `: free` after each variable's first use.
fn variable_type_hints(doc: &Document, bytes: &Range<usize>, out: &mut Vec<(usize, InlayHint)>) {
    let sites = doc.index().sites();
    let start = sites.partition_point(|(range, _)| range.end < bytes.start);
    for (range, symbol) in sites[start..].iter().take_while(|(range, _)| range.start <= bytes.end) {
        let Symbol::Variable(var, occurrence) = *symbol else { continue };
        let variable = &doc.index().variables[var];
        let first_use = variable.occurrences.iter().position(|o| !o.role.is_declaration());
        if first_use != Some(occurrence) {
            continue;
        }
        let Some(label) = variable_type(doc, variable) else { continue };
        out.push(typed_hint(
            doc,
            range.end,
            format!(": {label}"),
            format!("`{}` is {}", variable.name, describe(label)),
            false,
            false,
            Some(InlayHintKind::TYPE),
        ));
    }
}

fn describe(label: &str) -> &'static str {
    match label {
        "bin" => "binary",
        "int" => "integer",
        "semi" => "semi-continuous",
        "semi-int" => "semi-integer",
        _ => "free (unbounded below)",
    }
}

/// Type label from the variable's type-section entries and bounds; `None` for
/// a continuous variable with a finite (or default) lower bound.
fn variable_type(doc: &Document, variable: &Variable) -> Option<&'static str> {
    let has = |role: Role| variable.occurrences.iter().any(|o| o.role == role);
    let integer = has(Role::Generals) || has(Role::Integers);
    if has(Role::Binaries) {
        return Some("bin");
    }
    if has(Role::SemiContinuous) {
        return Some(if integer { "semi-int" } else { "semi" });
    }
    if integer {
        return Some("int");
    }
    let bounds = variable
        .occurrences
        .iter()
        .filter(|o| o.role == Role::Bound)
        .filter_map(|o| doc.node(o.range.clone(), kind::IDENTIFIER)?.parent())
        .filter(|n| n.kind() == kind::BOUND_DECLARATION)
        .filter_map(|n| syntax::declared_bounds(n, &doc.text))
        .fold(VariableBounds::unspecified(), VariableBounds::merge);
    let free = bounds.lower == Some(f64::NEG_INFINITY) && bounds.upper.is_none_or(|u| u == f64::INFINITY);
    free.then_some("free")
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use lp_parser_rs::analysis::AnalysisConfig;
    use tower_lsp_server::ls_types::Position;

    use super::*;
    use crate::position::Encoding;
    use crate::semantic;

    fn doc(text: &str) -> Document {
        let mut doc = Document::new("file:///t.lp".parse().unwrap(), text.to_owned(), 1, Encoding::Utf16);
        doc.semantic_result = Some(Arc::new(semantic::run(text, 1, &AnalysisConfig::default())));
        doc
    }

    fn labels(doc: &Document, settings: &InlayHintSettings) -> Vec<(Position, String)> {
        hints(doc, doc.full_range(), settings)
            .into_iter()
            .map(|h| {
                let InlayHintLabel::String(label) = h.label else { unreachable!("labels are plain strings") };
                (h.position, label)
            })
            .collect()
    }

    /// Settings from `[generated_names, range_partners, variable_types, normalised_rhs]`.
    fn only([generated_names, range_partners, variable_types, normalised_rhs]: [bool; 4]) -> InlayHintSettings {
        InlayHintSettings { generated_names, range_partners, variable_types, normalised_rhs }
    }

    fn at(doc: &Document, needle: &str, delta: usize) -> Position {
        doc.position(doc.text.find(needle).unwrap() + delta)
    }

    const MODEL: &str = "Minimize\n 3 x + 2 y\nSubject To\n x + y >= 1\n c1: 2 <= x + z <= 8\n x - y <= 4\nEnd\n";

    #[test]
    fn generated_names_from_model() {
        let d = doc(MODEL);
        assert!(d.semantic().unwrap().model().is_some());
        let got = labels(&d, &only([true, false, false, false]));
        assert_eq!(
            got,
            [(at(&d, "3 x", 0), "OBJ1:".to_owned()), (at(&d, "x + y >=", 0), "C1:".to_owned()), (at(&d, "x - y", 0), "C2:".to_owned()),]
        );
        assert_eq!(labels(&d, &only([false, false, false, false])), []);
    }

    #[test]
    fn generated_names_need_current_semantic_result() {
        let mut d = doc(MODEL);
        d.version = 2;
        assert_eq!(labels(&d, &only([true, true, false, false])), []);
    }

    #[test]
    fn range_partners() {
        let d = doc(MODEL);
        let got = labels(&d, &only([false, true, false, false]));
        assert_eq!(got, [(at(&d, "8\n", 1), "& c1_rng".to_owned())]);
        assert_eq!(labels(&d, &only([false, false, true, true])).iter().filter(|(_, l)| l.contains("rng")).count(), 0);

        // Unnamed ranged constraint: both halves are generated.
        let d = doc("min\n x\nst\n 1 <= x <= 2\nend\n");
        let got = labels(&d, &only([true, true, false, false]));
        assert_eq!(
            got,
            [(at(&d, "x\nst", 0), "OBJ1:".to_owned()), (at(&d, "1 <=", 0), "C1:".to_owned()), (at(&d, "2\nend", 1), "& C2".to_owned())]
        );
    }

    #[test]
    fn variable_types() {
        let text = "min\n obj: x + y + z + w + s + v\nst\n c: x + y + z + w + s + v >= 1\nbounds\n y free\n w >= -1e30\n v >= -2\nbinaries\n x\ngenerals\n z\nsemi\n s\nend\n";
        let d = doc(text);
        let got = labels(&d, &only([false, false, true, false]));
        assert_eq!(
            got,
            [
                (at(&d, "x +", 1), ": bin".to_owned()),
                (at(&d, "y +", 1), ": free".to_owned()),
                (at(&d, "z +", 1), ": int".to_owned()),
                (at(&d, "w +", 1), ": free".to_owned()),
                (at(&d, "s +", 1), ": semi".to_owned()),
            ]
        );
        assert_eq!(labels(&d, &only([false, false, false, false])), []);
    }

    #[test]
    fn normalised_rhs() {
        let text = "min\n obj: x\nst\n a: x + 3 <= 10\n b: 10 >= x - 2 + 1\n c: 2 <= x + 1 <= 8\n d: x <= 5\nend\n";
        let d = doc(text);
        let got = labels(&d, &only([false, false, false, true]));
        assert_eq!(
            got,
            [(at(&d, "10\n", 2), "= 7".to_owned()), (at(&d, "+ 1\n", 3), "= 11".to_owned()), (at(&d, "8\n", 1), "= 1 .. 7".to_owned()),]
        );
        // Matches upstream folding.
        let problem = &d.semantic().unwrap().model().unwrap().problem;
        let rhs = |name: &str| problem.constraints[&problem.name_id(name).unwrap()].linear_row().unwrap().2;
        assert!((rhs("a") - 7.0).abs() < f64::EPSILON);
        assert!((rhs("b") - 11.0).abs() < f64::EPSILON);
        assert!((rhs("c") - 1.0).abs() < f64::EPSILON);
        assert!((rhs("c_rng") - 7.0).abs() < f64::EPSILON);
        assert_eq!(labels(&d, &only([true, true, true, false])).iter().filter(|(_, l)| l.starts_with('=')).count(), 0);
    }

    #[test]
    fn only_hints_inside_range() {
        let d = doc(MODEL);
        let line = |n: u32| LspRange::new(Position::new(n, 0), Position::new(n + 1, 0));
        let settings = InlayHintSettings::default();
        let lines = |n: u32| hints(&d, line(n), &settings).into_iter().map(|h| h.position.line).collect::<Vec<_>>();
        assert_eq!(lines(3), [3]);
        assert_eq!(lines(4), [4]);
        assert_eq!(hints(&d, line(2), &settings).iter().map(|h| h.position).collect::<Vec<_>>(), []);
        let all = hints(&d, d.full_range(), &settings);
        assert!(all.windows(2).all(|w| (w[0].position.line, w[0].position.character) <= (w[1].position.line, w[1].position.character)));
        assert_eq!(all.len(), 4);
    }

    #[test]
    fn no_panic_on_broken_documents() {
        let d = doc("min\n x +\nst\n : <= \n 3 >= \nbounds\n <= x\nend");
        let _hints = hints(&d, d.full_range(), &InlayHintSettings::default());
        let d = doc("");
        assert_eq!(labels(&d, &InlayHintSettings::default()), []);
    }
}
