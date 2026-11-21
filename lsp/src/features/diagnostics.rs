//! Diagnostics: syntax (tree-sitter ERROR/MISSING), index-based checks, semantic
//! (`lp_parser_rs` errors, only without syntax errors) and coded model
//! diagnostics (`lp_parser_rs::analysis::ModelDiagnostics`), whose names are
//! mapped to source ranges through the document index.

use std::collections::HashSet;
use std::ops::Range;

use lp_highs::protocol::{DiagnosisMethod, Side};
use lp_parser_rs::analysis::{Certainty, DiagnosticCode, DiagnosticSubject, IssueSeverity, ModelDiagnostic, SubjectSite};
use lp_parser_rs::{EntityKind as UpstreamKind, LpParseError, VariableBounds};
use ls_types::{Diagnostic, DiagnosticRelatedInformation, DiagnosticSeverity, DiagnosticTag, NumberOrString};
use tree_sitter::Node;

use crate::config::{AnalysisSettings, Config, SeverityOverride};
use crate::document::Document;
use crate::index::{Namespace, Occurrence, Role};
use crate::position::floor_char_boundary;
use crate::requests::sites::Sites;
use crate::semantic::{Model, SemanticResult};
use crate::solution::Solution;
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
}

/// `source` of every diagnostic the server publishes. Model diagnostics carry
/// their stable kebab-case `DiagnosticCode` as `code`.
pub const SOURCE: &str = "lp-lsp";

/// Codes shown faded as unnecessary code.
const UNNECESSARY: [DiagnosticCode; 6] = [
    DiagnosticCode::UnusedVariable,
    DiagnosticCode::DuplicateRow,
    DiagnosticCode::DominatedRow,
    DiagnosticCode::RowRedundantByBounds,
    DiagnosticCode::BinaryRedundantBounds,
    DiagnosticCode::ExplicitZeroCoefficient,
];

/// Longest source excerpt quoted in a syntax-error message, in characters.
const EXCERPT_CHARS: usize = 40;

/// All diagnostics for `doc` under `config`.
#[must_use]
pub fn compute(doc: &Document, config: &Config) -> Vec<Diagnostic> {
    compute_with(doc, config, None)
}

/// [`compute`], plus the members of the infeasibility `solution` diagnosed,
/// while it describes this version of `doc`. After an edit the members are
/// gone: the next publish no longer includes them.
#[must_use]
pub fn compute_with(doc: &Document, config: &Config, solution: Option<&Solution>) -> Vec<Diagnostic> {
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
        model_diagnostics(doc, model, &config.analysis, &unused, &conflicting, &mut out);
    }
    if let Some(solution) = solution.filter(|s| !s.is_stale(Some(doc.version))) {
        iis_members(doc, solution, &mut out);
    }
    out
}

/// `code` of a diagnostic for a constraint or bound in an infeasible core.
pub const IIS_MEMBER: &str = "solver/iis-member";

/// `solver/iis-member` diagnostics for the constraints and bounds an
/// infeasibility diagnosis names, at their source sites. Members without a
/// site in the document are skipped.
fn iis_members(doc: &Document, solution: &Solution, out: &mut Vec<Diagnostic>) {
    let Some(infeasibility) = &solution.infeasibility else { return };
    let proven = infeasibility.method == DiagnosisMethod::Iis;
    // An elastic relaxation is one minimum-violation repair, not a proof that
    // these rows belong to an infeasible core.
    let (certainty, severity) = if proven { ("proven", DiagnosticSeverity::WARNING) } else { ("heuristic", DiagnosticSeverity::HINT) };
    let side_label = |side: Side| match side {
        Side::Lower => "lower side",
        Side::Upper => "upper side",
        Side::Boxed => "both sides",
        Side::Free => "free",
    };
    let data = serde_json::json!({ "category": "solver", "certainty": certainty });
    let mut push = |range: Range<usize>, message: String| {
        let mut d = diagnostic(doc, range, severity, IIS_MEMBER, message);
        d.data = Some(data.clone());
        out.push(d);
    };
    let model = doc.semantic().and_then(SemanticResult::model);
    for member in &infeasibility.constraints {
        // The solver reports unnamed rows under generated names (`C1`), which the
        // index does not hold: resolve through the model's byte offsets first.
        let by_model = model.and_then(|m| m.problem.name_id(&member.name).and_then(|id| Sites::new(doc, &m.problem).constraint(id)));
        let entity = match by_model {
            Some(site) => &doc.index().entities[site.entity],
            None => match doc.index().entities_named(&member.name, Namespace::Constraint).next() {
                Some((_, entity)) => entity,
                None => continue,
            },
        };
        let violation = member.violation.map_or_else(String::new, |v| format!(", violated by {v}"));
        push(
            entity.name_range.clone().unwrap_or_else(|| entity.range.clone()),
            if proven {
                format!(
                    "`{}` ({}) is part of the irreducible infeasible subsystem that makes the model infeasible{violation}",
                    member.name,
                    side_label(member.side)
                )
            } else {
                format!(
                    "`{}` ({}) is relaxed by the elastic diagnosis to restore feasibility{violation}",
                    member.name,
                    side_label(member.side)
                )
            },
        );
    }
    for member in &infeasibility.bounds {
        let Some(variable) = doc.index().variable(&member.variable) else { continue };
        let site = variable.occurrences.iter().rev().find(|o| o.role == Role::Bound).unwrap_or_else(|| variable.definition());
        let gap = member.gap.map_or_else(String::new, |g| format!(", lower bound exceeds upper by {g}"));
        push(
            site.range.clone(),
            if proven {
                format!(
                    "The bound on `{}` ({}) is part of the irreducible infeasible subsystem that makes the model infeasible{gap}",
                    member.variable,
                    side_label(member.side)
                )
            } else {
                format!(
                    "The bound on `{}` ({}) is relaxed by the elastic diagnosis to restore feasibility{gap}",
                    member.variable,
                    side_label(member.side)
                )
            },
        );
    }
}

fn diagnostic(doc: &Document, range: Range<usize>, severity: DiagnosticSeverity, code: &str, message: String) -> Diagnostic {
    Diagnostic {
        range: doc.range(range),
        severity: Some(severity),
        code: Some(NumberOrString::String(code.to_owned())),
        source: Some(SOURCE.to_owned()),
        message,
        ..Diagnostic::default()
    }
}

fn related(doc: &Document, range: Range<usize>, message: impl Into<String>) -> DiagnosticRelatedInformation {
    DiagnosticRelatedInformation { location: doc.location(range), message: message.into() }
}

/// `ERROR` and `MISSING` nodes (one diagnostic per `ERROR`, its subtree
/// skipped) and `=<` / `=>` operator spellings, in one walk. Only subtrees
/// with errors (`has_error` holds on every ancestor of an `ERROR` or
/// `MISSING` node) or spanning one of those spellings are entered: nothing
/// else can produce a diagnostic.
fn syntax_diagnostics(doc: &Document, out: &mut Vec<Diagnostic>) {
    let mut spellings: Vec<usize> = doc.text.match_indices("=<").chain(doc.text.match_indices("=>")).map(|(i, _)| i).collect();
    spellings.sort_unstable();
    let spans_spelling = |node: Node<'_>| {
        let next = spellings.partition_point(|&s| s < node.start_byte());
        spellings.get(next).is_some_and(|&s| s + 2 <= node.end_byte())
    };
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
        if descend && (node.has_error() || spans_spelling(node)) && cursor.goto_first_child() {
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

fn conflicting_types(doc: &Document, out: &mut Vec<Diagnostic>) {
    for variable in &doc.index().variables {
        let declarations: Vec<&Occurrence> = variable.occurrences.iter().filter(|o| o.role.is_type_declaration()).collect();
        let Some(first) = declarations.first() else { continue };
        if declarations.iter().all(|o| o.role == first.role) {
            continue;
        }
        let mut sections: Vec<&str> = declarations.iter().map(|o| o.role.section_name()).collect();
        sections.dedup();
        let message = format!("`{}` is declared in more than one type section: {}", variable.name, sections.join(", "));
        for occurrence in &declarations {
            let mut d = diagnostic(doc, occurrence.range.clone(), DiagnosticSeverity::WARNING, codes::CONFLICTING_TYPE, message.clone());
            let others = declarations
                .iter()
                .filter(|o| o.role != occurrence.role)
                .map(|o| related(doc, o.range.clone(), format!("also declared in {}", o.role.section_name())));
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
    let entries = bound_entries(doc);
    let entry = |range: &Range<usize>| {
        let at = entries.binary_search_by_key(&range.start, |(identifier, ..)| identifier.start).ok()?;
        let (identifier, declaration, bounds) = &entries[at];
        (identifier == range).then_some((*declaration, *bounds))
    };
    for variable in &doc.index().variables {
        let declarations: Vec<(Range<usize>, VariableBounds)> = variable
            .occurrences
            .iter()
            .filter(|o| o.role == Role::Bound)
            .filter_map(|o| entry(&o.range))
            // Entries that do not parse as a bound are syntax errors, reported elsewhere.
            .filter_map(|(n, bounds)| Some((n.byte_range(), bounds?)))
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

/// Every identifier of a `bound_declaration` in a top-level `Bounds` section
/// (where the index finds [`Role::Bound`] occurrences) with its declaration
/// and the bounds it declares, in document order. One pass over each
/// declaration, rather than a lookup from the root per occurrence.
fn bound_entries(doc: &Document) -> Vec<(Range<usize>, Node<'_>, Option<VariableBounds>)> {
    let language = syntax::language();
    let bounds_section = language.id_for_node_kind(kind::BOUNDS_SECTION, true);
    let bound_declaration = language.id_for_node_kind(kind::BOUND_DECLARATION, true);
    let mut out = Vec::new();
    let root = doc.tree.root_node();
    let (mut sections, mut entries, mut parts) = (root.walk(), root.walk(), root.walk());
    for section in root.children(&mut sections).filter(|n| n.kind_id() == bounds_section) {
        for declaration in section.children(&mut entries).filter(|n| n.kind_id() == bound_declaration) {
            let first = out.len();
            let bounds =
                syntax::declared_bounds_with(declaration, &doc.text, &mut parts, |n| out.push((n.byte_range(), declaration, None)));
            for entry in &mut out[first..] {
                entry.2 = bounds;
            }
        }
    }
    debug_assert!(out.windows(2).all(|w| w[0].0.end <= w[1].0.start), "entries in document order");
    out
}

/// Publish the model's coded diagnostics that `settings` allow. Findings the
/// index-based checks already report at the source (`unused`, `conflicting`:
/// variable names) are not repeated.
fn model_diagnostics(
    doc: &Document,
    model: &Model,
    settings: &AnalysisSettings,
    unused: &HashSet<&str>,
    conflicting: &HashSet<&str>,
    out: &mut Vec<Diagnostic>,
) {
    for finding in &model.diagnostics.diagnostics {
        if !settings.publishes(finding.code) {
            continue;
        }
        let covered = match finding.code {
            DiagnosticCode::UnusedVariable => Some(unused),
            DiagnosticCode::InvalidBounds => Some(conflicting),
            _ => None,
        };
        let subject = finding.subject.as_ref().filter(|s| s.kind == UpstreamKind::Variable);
        if covered.zip(subject).is_some_and(|(names, s)| names.contains(s.name.as_str())) {
            continue;
        }
        out.push(model_diagnostic(finding, doc, model, settings));
    }
}

/// One [`ModelDiagnostic`] as an LSP diagnostic: `code`, `source`, severity
/// (error, warning, information; heuristic information is a hint), the
/// `UNNECESSARY` tag, `relatedInformation` for the other entities involved and
/// `data` = `{ category, certainty }`.
fn model_diagnostic(finding: &ModelDiagnostic, doc: &Document, model: &Model, settings: &AnalysisSettings) -> Diagnostic {
    let range = finding.subject.as_ref().and_then(|s| subject_range(doc, model, s)).unwrap_or_else(|| sense_range(doc));
    let message = match &finding.explanation {
        Some(explanation) => format!("{}\n{explanation}", finding.message),
        None => finding.message.clone(),
    };
    let related: Vec<DiagnosticRelatedInformation> =
        finding.related.iter().filter_map(|r| Some(related(doc, subject_range(doc, model, &r.subject)?, r.note.clone()))).collect();
    let mut d = diagnostic(doc, range, severity(finding, settings), finding.code.as_str(), message);
    d.related_information = (!related.is_empty()).then_some(related);
    d.tags = UNNECESSARY.contains(&finding.code).then(|| vec![DiagnosticTag::UNNECESSARY]);
    d.data = Some(serde_json::json!({ "category": finding.category, "certainty": finding.certainty }));
    d
}

/// Default mapping, unless `lp.analysis.severityOverrides` names the code.
fn severity(finding: &ModelDiagnostic, settings: &AnalysisSettings) -> DiagnosticSeverity {
    match settings.severity_override(finding.code) {
        Some(SeverityOverride::Error) => DiagnosticSeverity::ERROR,
        Some(SeverityOverride::Warning) => DiagnosticSeverity::WARNING,
        Some(SeverityOverride::Information) => DiagnosticSeverity::INFORMATION,
        Some(SeverityOverride::Hint) => DiagnosticSeverity::HINT,
        // `publishes` already dropped `off`; keep the default if it slips through.
        Some(SeverityOverride::Off) | None => match finding.severity {
            IssueSeverity::Error => DiagnosticSeverity::ERROR,
            IssueSeverity::Warning => DiagnosticSeverity::WARNING,
            IssueSeverity::Info if finding.certainty == Certainty::Heuristic => DiagnosticSeverity::HINT,
            IssueSeverity::Info => DiagnosticSeverity::INFORMATION,
        },
    }
}

/// Source range of a subject, found by name in the document index: a
/// variable's definition, bound or type declaration (by `site`), or an
/// entity's label (whole entity if unnamed), narrowed to `variable` inside it.
/// `None` when the name has no site in this document.
fn subject_range(doc: &Document, model: &Model, subject: &DiagnosticSubject) -> Option<Range<usize>> {
    let (namespace, offset) = match subject.kind {
        UpstreamKind::Variable => {
            let variable = doc.index().variable(&subject.name)?;
            let found = match subject.site {
                SubjectSite::Bound => variable.occurrences.iter().rev().find(|o| o.role == Role::Bound),
                SubjectSite::TypeDeclaration => variable.occurrences.iter().find(|o| o.role.is_type_declaration()),
                _ => None,
            };
            return Some(found.unwrap_or_else(|| variable.definition()).range.clone());
        }
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

    use lp_parser_rs::analysis::{AnalysisConfig, DiagnosticsConfig};

    use super::*;
    use crate::position::Encoding;

    const CLEAN: &str = "Minimize\n obj: x + y + z\nSubject To\n c1: x + y + z >= 1\nBounds\n x <= 10\nGenerals\n y\nEnd\n";

    fn document(text: &str) -> Document {
        let mut doc = Document::new("file:///test.lp".parse().unwrap(), text.to_owned(), 1, Encoding::Utf16);
        doc.semantic_result = Some(Arc::new(crate::semantic::run(text, 1, &AnalysisConfig::default())));
        doc
    }

    fn iis_member_diagnostics(method: DiagnosisMethod) -> Vec<Diagnostic> {
        iis_diagnostics(&document(CLEAN), method, &["c1"])
    }

    fn iis_diagnostics(doc: &Document, method: DiagnosisMethod, names: &[&str]) -> Vec<Diagnostic> {
        use lp_highs::protocol::{Infeasibility, InfeasibleConstraint, SolveOutcome, SolverId, Timings};

        let outcome = SolveOutcome {
            solver: SolverId { name: "HiGHS".into(), version: "1".into(), helper: "lp-highs".into() },
            status: "Infeasible".into(),
            objective_value: None,
            mip_gap: None,
            timings: Timings { build_ms: 0, solve_ms: 0, extract_ms: 0 },
            variables: Vec::new(),
            constraints: Vec::new(),
            skipped: lp_highs::protocol::Skipped::default(),
            infeasibility: Some(Infeasibility {
                method,
                constraints: names
                    .iter()
                    .map(|name| InfeasibleConstraint { name: (*name).into(), side: Side::Upper, violation: Some(2.0) })
                    .collect(),
                bounds: Vec::new(),
                total_violation: None,
            }),
            log: None,
        };
        let solution = Solution::new(Some(1), &outcome);
        let mut out = Vec::new();
        iis_members(doc, &solution, &mut out);
        out
    }

    #[test]
    fn iis_members_are_proven_and_elastic_members_are_heuristic() {
        let iis = iis_member_diagnostics(DiagnosisMethod::Iis);
        assert_eq!(iis.len(), 1);
        assert_eq!(iis[0].severity, Some(DiagnosticSeverity::WARNING));
        assert_eq!(iis[0].data.as_ref().unwrap()["certainty"], "proven");
        assert!(iis[0].message.contains("irreducible infeasible subsystem"), "{}", iis[0].message);

        let elastic = iis_member_diagnostics(DiagnosisMethod::Elastic);
        assert_eq!(elastic.len(), 1);
        assert_eq!(elastic[0].severity, Some(DiagnosticSeverity::HINT));
        assert_eq!(elastic[0].data.as_ref().unwrap()["certainty"], "heuristic");
        assert_eq!(elastic[0].message, "`c1` (upper side) is relaxed by the elastic diagnosis to restore feasibility, violated by 2");
    }

    #[test]
    fn unnamed_iis_members_resolve_through_their_generated_names() {
        let doc = document("Minimize\n obj: x\nSubject To\n x >= 3\n x <= 1\nEnd\n");
        for (method, certainty) in [(DiagnosisMethod::Iis, "proven"), (DiagnosisMethod::Elastic, "heuristic")] {
            let out = iis_diagnostics(&doc, method, &["C1", "C2"]);
            assert_eq!(out.len(), 2, "{certainty}: {out:?}");
            let covered: Vec<&str> = out.iter().map(|d| doc.slice(doc.byte_range(d.range))).collect();
            assert!(covered[0].contains("x >= 3") && covered[1].contains("x <= 1"), "{covered:?}");
            assert!(out.iter().all(|d| d.data.as_ref().unwrap()["certainty"] == certainty));
        }
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
                assert_eq!(d.source.as_deref(), Some(SOURCE));
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

    /// [`syntax_diagnostics`] visiting every node: the pruned walk must match.
    fn syntax_diagnostics_full_walk(doc: &Document) -> Vec<Diagnostic> {
        let mut out = Vec::new();
        let mut cursor = doc.tree.walk();
        loop {
            let node = cursor.node();
            let mut descend = true;
            if node.is_error() {
                out.push(unexpected(doc, node));
                descend = false;
            } else if node.is_missing() {
                let expected = if node.is_named() { node.kind().replace('_', " ") } else { format!("`{}`", node.kind()) };
                out.push(diagnostic(
                    doc,
                    node.byte_range(),
                    DiagnosticSeverity::ERROR,
                    codes::MISSING_TOKEN,
                    format!("missing {expected}"),
                ));
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
                    return out;
                }
            }
        }
    }

    proptest::proptest! {
        #![proptest_config(proptest::prelude::ProptestConfig::with_cases(256))]
        #[test]
        fn pruned_syntax_walk_matches_full_walk(
            parts in proptest::collection::vec(
                proptest::prop_oneof![
                    proptest::strategy::Just("Minimize\n obj: x + y\n".to_owned()),
                    proptest::strategy::Just("Subject To\n c1: x + y >= 1\n".to_owned()),
                    proptest::strategy::Just(" c2: x - y =< 4\n".to_owned()),
                    proptest::strategy::Just(" c3: 2 x => 1\n".to_owned()),
                    proptest::strategy::Just(" c4: b = 1 -> x + y <= 2\n".to_owned()),
                    proptest::strategy::Just("Bounds\n x <= 10\n -1 =< y =< 3\n".to_owned()),
                    proptest::strategy::Just("Generals\n y\n".to_owned()),
                    proptest::strategy::Just("General Constraints\n g: r = MAX(x, y)\n".to_owned()),
                    proptest::strategy::Just(" [ x ^ 2 ]".to_owned()),
                    proptest::strategy::Just("End\n".to_owned()),
                    "[a-z0-9 :+<=>\\n-]{0,8}",
                ],
                0..14,
            )
        ) {
            let doc = Document::new("file:///t.lp".parse().unwrap(), parts.concat(), 1, Encoding::Utf16);
            let mut pruned = Vec::new();
            syntax_diagnostics(&doc, &mut pruned);
            proptest::prop_assert_eq!(pruned, syntax_diagnostics_full_walk(&doc));
        }
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
    fn bound_entries_are_the_parents_of_bound_occurrences() {
        for text in [
            "min\n obj: x + y\nst\n c1: x + y >= 1\nbounds\n x >= 5\n x <= 2\n 3 >= y >= 1\n -inf <= z <= x\nend\n",
            "min\n obj: x\nbounds\n x <= \n y free\nst\n c1: x >= 1\nbounds\n 1 <= x <= 2 <= 3\n <= y\n w\nend\n",
            "min\n obj: x\nst\n c1: x >= 1\nbounds\n x >= 1 bounds x <= 2\n [ x ]\nend\n",
        ] {
            let doc = document(text);
            let entries = bound_entries(&doc);
            for occurrence in doc.index().variables.iter().flat_map(|v| &v.occurrences).filter(|o| o.role == Role::Bound) {
                let expected = doc
                    .node(occurrence.range.clone(), kind::IDENTIFIER)
                    .and_then(|n| n.parent())
                    .filter(|n| n.kind() == kind::BOUND_DECLARATION);
                let found = entries.iter().find(|(range, ..)| *range == occurrence.range).map(|(_, n, bounds)| (*n, *bounds));
                let expected = expected.map(|n| (n, syntax::declared_bounds(n, &doc.text)));
                assert_eq!(found, expected, "{text:?} at {:?}", occurrence.range);
            }
        }
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
        assert_eq!(only(&doc, "invalid-bounds"), []);

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

    const MODEL: &str = "min\n obj: x + y + z + w\nst\n c1: 2 x + 4 y <= 100\n c2: x + 2 y <= 50\n c3: x + 2 y <= 50\n c4: z + w >= 100\n c5: 1e12 x + y >= 1\nbounds\n z <= 50\n w <= 20\n -5 <= v <= 5\nbinaries\n b\nend\n";

    #[allow(clippy::unnecessary_wraps, reason = "compared with `Diagnostic::code`")]
    fn code(c: &str) -> Option<NumberOrString> {
        Some(NumberOrString::String(c.to_owned()))
    }

    #[test]
    fn model_diagnostics_map_to_lsp_diagnostics() {
        let doc = document(MODEL);
        let diagnostics = compute(&doc, &Config::default());
        let of = |c: &str| diagnostics.iter().filter(|d| d.code == code(c)).collect::<Vec<_>>();

        // Proven error: code, source, severity, range, related information and data.
        let [infeasible] = of("row-infeasible-by-bounds")[..] else { panic!("one row-infeasible-by-bounds: {diagnostics:?}") };
        assert_eq!(infeasible.source.as_deref(), Some(SOURCE));
        assert_eq!(infeasible.severity, Some(DiagnosticSeverity::ERROR));
        assert_eq!(covered(&doc, infeasible), "c4");
        assert!(infeasible.message.starts_with("Constraint 'c4' cannot hold"), "{}", infeasible.message);
        assert!(infeasible.message.contains('\n'), "the explanation follows the message");
        let related: Vec<(&str, &str)> = infeasible
            .related_information
            .as_ref()
            .unwrap()
            .iter()
            .map(|r| (doc.slice(doc.byte_range(r.location.range)), r.message.as_str()))
            .collect();
        // Each related variable is anchored at its definition.
        assert_eq!(related, [("z", "ranges over [0, 50]"), ("w", "ranges over [0, 20]")]);
        assert_eq!(infeasible.data, Some(serde_json::json!({ "category": "mathematical", "certainty": "proven" })));
        assert_eq!(infeasible.tags, None);

        // Related constraint is anchored at its label.
        let proportional = of("proportional-row");
        assert_eq!(proportional.len(), 2);
        assert_eq!(covered(&doc, proportional[0]), "c2");
        let first = &proportional[0].related_information.as_ref().unwrap()[0];
        assert_eq!(doc.slice(doc.byte_range(first.location.range)), "c1");
        assert_eq!(proportional[0].severity, Some(DiagnosticSeverity::WARNING));

        // Heuristic warning keeps its severity; heuristic information is a hint.
        let [large] = of("large-coefficient")[..] else { panic!("one large-coefficient") };
        assert_eq!(large.severity, Some(DiagnosticSeverity::WARNING));
        assert_eq!(large.data, Some(serde_json::json!({ "category": "numerical", "certainty": "heuristic" })));
        assert_eq!(covered(&doc, large), "x", "narrowed to the term inside the constraint");
        // Problem-wide findings sit on the sense keyword.
        let [range] = of("coefficient-range")[..] else { panic!("one coefficient-range") };
        assert_eq!((range.severity, covered(&doc, range)), (Some(DiagnosticSeverity::HINT), "min".to_owned()));

        // Retired prefix and duplicated checks are gone.
        assert!(diagnostics.iter().all(|d| !matches!(&d.code, Some(NumberOrString::String(c)) if c.starts_with("analysis/"))));
        assert_eq!(of("unused-variable"), Vec::<&Diagnostic>::new(), "deduplicated against unused-declaration");
        assert_eq!(of("unused-declaration").len(), 2);
    }

    #[test]
    fn unnecessary_tag_marks_redundant_findings() {
        let doc = document("min\n obj: x + y\nst\n c1: x + y >= 2\n c2: x + y >= 2\n z0: 0 x + y >= 0\nbounds\n x >= 0\nend\n");
        let diagnostics = compute(&doc, &Config::default());
        for finding in ["duplicate-row", "explicit-zero-coefficient", "row-redundant-by-bounds"] {
            let hits: Vec<&Diagnostic> = diagnostics.iter().filter(|d| d.code == code(finding)).collect();
            assert!(!hits.is_empty(), "{finding} fires: {diagnostics:?}");
            assert!(hits.iter().all(|d| d.tags == Some(vec![DiagnosticTag::UNNECESSARY])), "{finding}");
        }
        // Other codes are not faded.
        assert!(diagnostics.iter().filter(|d| d.code == code("empty-row")).all(|d| d.tags.is_none()));
    }

    #[test]
    fn variable_sites_choose_bound_type_or_definition() {
        // A binary with a bound outside [0, 1]: anchored at the bound entry.
        let doc = document("min\n obj: b + x\nst\n c1: b + x >= 1\nbinaries\n b\nbounds\n b <= 5\nend\n");
        let hits = only(&doc, "binary-bound-outside-unit");
        assert_eq!(hits.len(), 1, "{:?}", compute(&doc, &Config::default()));
        assert_eq!(doc.byte_range(hits[0].range).start, doc.text.find("b <= 5").unwrap());
        assert_eq!(hits[0].severity, Some(DiagnosticSeverity::WARNING));

        // An indicator on a continuous variable: anchored at its first use.
        let doc = document("min\n obj: x\nst\n c1: x >= 1\n c2: i = 1 -> x <= 2\nend\n");
        let hits = only(&doc, "indicator-not-binary");
        assert_eq!(hits.len(), 1, "{:?}", compute(&doc, &Config::default()));
        assert_eq!(hits[0].severity, Some(DiagnosticSeverity::ERROR));
        assert_eq!(covered(&doc, &hits[0]), "i");

        // Direct site resolution, including the fallback to the definition.
        let model = doc.semantic().unwrap().model().unwrap();
        let bound = DiagnosticSubject::variable("x", SubjectSite::Bound);
        assert_eq!(subject_range(&doc, model, &bound), Some(doc.index().variable("x").unwrap().definition().range.clone()));
        assert_eq!(subject_range(&doc, model, &DiagnosticSubject::variable("nope", SubjectSite::Definition)), None);
    }

    #[test]
    fn index_checks_still_win_over_duplicate_model_findings() {
        let doc = document("min\n obj: x\nst\n c1: x >= 1\nbounds\n x >= 5\n x <= 2\n w <= 4\nend\n");
        let diagnostics = compute(&doc, &Config::default());
        let codes: Vec<String> = diagnostics
            .iter()
            .filter_map(|d| match &d.code {
                Some(NumberOrString::String(c)) => Some(c.clone()),
                _ => None,
            })
            .collect();
        assert!(codes.contains(&"conflicting-bounds".to_owned()), "{codes:?}");
        assert!(!codes.contains(&"invalid-bounds".to_owned()), "{codes:?}");
        assert!(codes.contains(&"unused-declaration".to_owned()) && !codes.contains(&"unused-variable".to_owned()), "{codes:?}");
    }

    #[test]
    fn settings_select_and_reshape_model_diagnostics() {
        let doc = document(MODEL);
        let codes = |config: &Config| -> Vec<(String, Option<DiagnosticSeverity>)> {
            compute(&doc, config)
                .into_iter()
                .filter_map(|d| match d.code {
                    Some(NumberOrString::String(c)) => Some((c, d.severity)),
                    _ => None,
                })
                .collect()
        };
        let configured = |value: serde_json::Value| Config::from_value(serde_json::json!({ "analysis": value })).unwrap();
        let has = |found: &[(String, Option<DiagnosticSeverity>)], c: &str| found.iter().any(|(code, _)| code == c);

        let all = codes(&Config::default());
        assert!(has(&all, "large-coefficient") && has(&all, "proportional-row"));

        let off = codes(&without_analysis());
        assert!(!has(&off, "proportional-row") && !has(&off, "row-infeasible-by-bounds"));
        assert!(has(&off, "unused-declaration"), "index-based checks are not analysis");

        let no_numerical = codes(&configured(serde_json::json!({ "categories": { "numerical": false } })));
        assert!(!has(&no_numerical, "large-coefficient") && !has(&no_numerical, "coefficient-range"));
        assert!(has(&no_numerical, "proportional-row"));

        let no_heuristics = codes(&configured(serde_json::json!({ "heuristics": false })));
        assert!(!has(&no_heuristics, "large-coefficient") && has(&no_heuristics, "row-infeasible-by-bounds"));

        let overridden = codes(&configured(serde_json::json!({
            "heuristics": false,
            "severityOverrides": { "large-coefficient": "hint", "proportional-row": "off", "row-infeasible-by-bounds": "information" }
        })));
        assert!(overridden.contains(&("large-coefficient".into(), Some(DiagnosticSeverity::HINT))), "{overridden:?}");
        assert!(!has(&overridden, "proportional-row"));
        assert!(overridden.contains(&("row-infeasible-by-bounds".into(), Some(DiagnosticSeverity::INFORMATION))));
    }

    #[test]
    fn analysis_thresholds_and_truncation_reach_the_model_diagnostics() {
        let text = "min\n obj: x\nst\n c1: x >= 1\n c2: x >= 2\n c3: x >= 3\nend\n";
        let mut doc = Document::new("file:///t.lp".parse().unwrap(), text.to_owned(), 1, crate::position::Encoding::Utf16);
        let config = Config::from_value(serde_json::json!({ "analysis": { "maxPerCode": 1, "largeRhsThreshold": 1.5 } })).unwrap();
        doc.semantic_result = Some(Arc::new(crate::semantic::run_with(text, 1, &config.analysis.diagnostics_config())));
        let diagnostics = compute(&doc, &config);
        let large_rhs = diagnostics.iter().filter(|d| d.code == code("large-rhs")).count();
        assert_eq!(large_rhs, 1, "capped per code: {diagnostics:?}");
        assert!(diagnostics.iter().any(|d| d.code == code("diagnostics-truncated")), "{diagnostics:?}");
    }

    #[test]
    fn ranged_constraint_halves_are_not_reported_against_each_other() {
        let doc = document("min\n obj: x + y\nst\n c1: 2 <= x + y <= 8\nend\n");
        let rows: Vec<String> = compute(&doc, &Config::default())
            .into_iter()
            .filter_map(|d| match d.code {
                Some(NumberOrString::String(c))
                    if ["duplicate-row", "proportional-row", "dominated-row", "contradictory-rows", "rows-form-range"]
                        .contains(&c.as_str()) =>
                {
                    Some(c)
                }
                _ => None,
            })
            .collect();
        assert_eq!(rows, Vec::<String>::new());
    }

    #[test]
    fn subject_range_resolves_labels_and_variables() {
        let doc = document("min\n obj: x + y\nst\n c1: x + y >= 1\n e1: 0 x >= -1\nend\n");
        let model = doc.semantic().unwrap().model().unwrap();
        let range = subject_range(&doc, model, &DiagnosticSubject::constraint("e1")).unwrap();
        assert_eq!(doc.slice(range), "e1");
        let objective = DiagnosticSubject::objective("obj").with_variable("y");
        let range = subject_range(&doc, model, &objective).unwrap();
        assert_eq!(range.start, doc.text.find("+ y").unwrap() + 2);
        assert_eq!(subject_range(&doc, model, &DiagnosticSubject::constraint("nope")), None);
        // Generated names resolve through the model's byte offset.
        let unnamed = document("min\n obj: x\nst\n x + 0 y >= 1\nend\n");
        let model = unnamed.semantic().unwrap().model().unwrap();
        let generated = model.problem.constraints.values().next().unwrap().name();
        let name = model.problem.resolve(generated).to_owned();
        assert!(subject_range(&unnamed, model, &DiagnosticSubject::constraint(name)).is_some());
    }

    #[test]
    fn cached_analysis_is_current_only_for_its_version_and_settings() {
        let result = crate::semantic::run_with(CLEAN, 3, &DiagnosticsConfig::default());
        assert!(result.is_current(3, &DiagnosticsConfig::default()));
        assert!(!result.is_current(4, &DiagnosticsConfig::default()), "an edit invalidates it");
        let mut other = DiagnosticsConfig::default();
        other.max_per_code = 5;
        assert!(!result.is_current(3, &other), "a setting that feeds the analysis invalidates it");

        // A parse error does not depend on the analysis settings.
        let failed = crate::semantic::run_with("min\n obj: x\nst\n c1: [ x ^ 3 ] <= 4\nend\n", 3, &DiagnosticsConfig::default());
        assert!(failed.model().is_none() && failed.is_current(3, &other));
    }

    #[test]
    fn invalid_analysis_config_is_reported_not_ignored() {
        let mut bad = DiagnosticsConfig::default();
        bad.max_per_code = 0;
        let result = crate::semantic::run_with(CLEAN, 1, &bad);
        assert!(result.outcome.is_err());
    }
}
