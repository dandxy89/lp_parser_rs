//! `lp/explain`: what an objective, constraint or variable means, in the
//! model's own terms.

use std::fmt::Write as _;
use std::ops::Range;

use lp_parser_rs::model::{Constraint, Objective, Variable, VariableKind};
use lp_parser_rs::{EntityKind as UpstreamKind, LpProblem, NameId};
use ls_types as lsp;
use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::model::{Bounds, constraint_kind, relation};
use super::sites::Sites;
use super::structure::{RowKind, Scope, Structure};
use super::{Analysis, Cancel, DocumentRef, RequestError, Source, Target, constraint_class, finite, variable_kind};
use crate::config::AnalysisSettings;
use crate::description;
use crate::document::Document;
use crate::index::{EntityKind as IndexKind, Role, Symbol};
use crate::solution::Solution;

/// Terms listed.
pub const MAX_TERMS: usize = 10_000;
/// Usages listed for a variable.
pub const MAX_USAGE: usize = 10_000;
/// Related rows listed.
pub const MAX_RELATED: usize = 20;
/// Terms spelled out in the markdown.
const MARKDOWN_TERMS: usize = 8;

/// What a target names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Kind {
    /// A constraint.
    Constraint,
    /// An objective.
    Objective,
    /// A variable.
    Variable,
}

impl Kind {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Constraint => "constraint",
            Self::Objective => "objective",
            Self::Variable => "variable",
        }
    }
}

/// A named entity.
#[derive(Debug, Clone, Deserialize)]
pub struct Entity {
    /// What `name` names.
    pub kind: Kind,
    /// Its name.
    pub name: String,
}

/// Params of `lp/explain`: a named `target`, or a `position` in the document.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Params {
    /// Document and progress token.
    #[serde(flatten)]
    pub target: Target,
    /// The entity to explain.
    #[serde(default, rename = "target")]
    pub entity: Option<Entity>,
    /// A position at or inside the entity to explain.
    #[serde(default)]
    pub position: Option<lsp::Position>,
}

/// Result of `lp/explain`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Result {
    #[serde(flatten)]
    pub document: DocumentRef,
    pub kind: &'static str,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub range: Option<lsp::Range>,
    pub markdown: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub terms: Option<Vec<Term>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub terms_truncated: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operator: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rhs: Option<Option<f64>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub activity: Option<Activity>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub related: Option<Vec<Related>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<Vec<UsageItem>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage_truncated: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bounds: Option<VariableBounds>,
    pub diagnostics: Vec<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub solution: Option<Value>,
}

/// A term of a row.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Term {
    pub variable: String,
    /// `null` for a bare reference (an indicator or quadratic-only variable).
    pub coefficient: Option<f64>,
    pub range: Option<lsp::Range>,
}

/// Bounds of a row's activity; `null` is infinite.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Activity {
    pub min: Option<f64>,
    pub max: Option<f64>,
}

/// A row sharing variables with the explained one.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Related {
    pub name: String,
    pub kind: &'static str,
    pub shared: usize,
}

/// One place a variable is used or declared.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct UsageItem {
    /// The objective or constraint, `null` for bounds and type declarations.
    pub entity: Option<String>,
    pub kind: Option<&'static str>,
    pub role: &'static str,
    pub coefficient: Option<f64>,
    pub range: lsp::Range,
}

/// A variable's effective bounds and kind.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VariableBounds {
    #[serde(flatten)]
    pub bounds: Bounds,
    pub variable_kind: &'static str,
}

/// Handle `lp/explain`. `solution` is the document's stored solve result, if any.
///
/// # Errors
/// `parseError`, `-32602` for an unknown or missing target, or cancellation.
pub fn handle(
    doc: &Document,
    version: Option<i32>,
    params: &Params,
    solution: Option<&Solution>,
    settings: &AnalysisSettings,
    cancel: &Cancel,
) -> std::result::Result<Result, RequestError> {
    let source = Source::acquire(doc)?;
    let problem = source.problem();
    let analysis = Analysis::new(&source, settings, cancel)?;
    cancel.set_total(problem.objectives.len() + problem.constraints.len());
    let sites = Sites::new(doc, problem);
    let context = Context { solution, analysis: &analysis };
    let entity = match (&params.entity, params.position) {
        (Some(entity), _) => entity.clone(),
        (None, Some(position)) => at_position(doc, &sites, position)?,
        (None, None) => return Err(RequestError::invalid("explain needs a `target` or a `position`")),
    };
    let document = DocumentRef::new(&params.target.text_document.uri, version);
    match entity.kind {
        Kind::Constraint => constraint(doc, problem, &sites, &entity.name, document, &context, cancel),
        Kind::Objective => objective(doc, problem, &sites, &entity.name, document, &context, cancel),
        Kind::Variable => variable(doc, problem, &sites, &entity.name, document, &context, cancel),
    }
}

/// The entity or variable under `position`.
fn at_position(doc: &Document, sites: &Sites, position: lsp::Position) -> std::result::Result<Entity, RequestError> {
    let offset = doc.offset(position);
    let index = doc.index();
    let entity_index = match index.symbol_at(offset) {
        Some((_, Symbol::Variable(variable, _))) => {
            return Ok(Entity { kind: Kind::Variable, name: index.variables[variable].name.clone() });
        }
        Some((_, Symbol::Entity(entity))) => Some(entity),
        Some((_, Symbol::Attribute(attribute))) => Some(index.attributes[attribute].objective),
        None => index.entity_at(offset),
    };
    let entity = entity_index.ok_or_else(|| RequestError::invalid("no entity or variable at that position"))?;
    let kind = match index.entities[entity].kind {
        IndexKind::Objective => Kind::Objective,
        IndexKind::Constraint | IndexKind::GeneralConstraint | IndexKind::Sos => Kind::Constraint,
    };
    let name = sites.entity_name(entity).ok_or_else(|| RequestError::invalid("the entity at that position is not in the parsed model"))?;
    Ok(Entity { kind, name })
}

fn unknown(kind: Kind, name: &str) -> RequestError {
    RequestError::invalid(format!("unknown {} '{name}'", kind.as_str()))
}

/// A row's terms, merged by variable in order of first appearance.
struct Terms {
    /// `NAN` for a bare reference.
    items: Vec<(NameId, f64)>,
    /// Whether the row is a plain linear expression, so activity bounds apply.
    linear: bool,
    constant: f64,
    at: FxHashMap<NameId, usize>,
}

impl Terms {
    fn new(linear: bool, constant: f64) -> Self {
        Self { items: Vec::new(), linear, constant, at: FxHashMap::default() }
    }

    /// Add `value` to the variable's term, creating it on first sight.
    fn add(&mut self, name: NameId, value: f64) {
        if let Some(&at) = self.at.get(&name) {
            let existing = &mut self.items[at].1;
            *existing = if existing.is_nan() {
                value
            } else if value.is_nan() {
                *existing
            } else {
                *existing + value
            };
        } else {
            self.at.insert(name, self.items.len());
            self.items.push((name, value));
        }
    }

    /// Mention a variable without a coefficient.
    fn reference(&mut self, name: NameId) {
        self.add(name, f64::NAN);
    }

    fn from_constraint(constraint: &Constraint) -> Self {
        let mut terms = Self::new(matches!(constraint, Constraint::Standard { .. } | Constraint::Indicator { .. }), 0.0);
        match constraint {
            Constraint::Standard { coefficients, .. } => {
                for c in coefficients {
                    terms.add(c.name, c.value);
                }
            }
            Constraint::Indicator { variable, coefficients, .. } => {
                terms.reference(*variable);
                for c in coefficients {
                    terms.add(c.name, c.value);
                }
            }
            Constraint::Quadratic { coefficients, quadratic, .. } => {
                for c in coefficients {
                    terms.add(c.name, c.value);
                }
                for q in quadratic {
                    terms.reference(q.var1);
                    terms.reference(q.var2);
                }
            }
            Constraint::SOS { weights, .. } => {
                for c in weights {
                    terms.add(c.name, c.value);
                }
            }
            Constraint::General { resultant, function, .. } => {
                terms.reference(*resultant);
                for &variable in function.variables() {
                    terms.reference(variable);
                }
            }
        }
        terms
    }

    fn from_objective(objective: &Objective) -> Self {
        let mut terms = Self::new(objective.quadratic.is_empty(), objective.constant);
        for c in &objective.coefficients {
            terms.add(c.name, c.value);
        }
        for q in &objective.quadratic {
            terms.reference(q.var1);
            terms.reference(q.var2);
        }
        terms
    }
}

/// Domain of a variable for activity bounds (design 1.3).
fn domain(variable: &Variable, tolerance: f64) -> (f64, f64) {
    let (lower, upper) = (variable.bounds.effective_lower(variable.kind), variable.bounds.effective_upper(variable.kind));
    match variable.kind {
        VariableKind::SemiContinuous | VariableKind::SemiInteger => (lower.min(0.0), upper.max(0.0)),
        VariableKind::Integer | VariableKind::General | VariableKind::Binary => {
            let round = |bound: f64, up: bool| {
                if !bound.is_finite() {
                    return bound;
                }
                let eps = tolerance * bound.abs().max(1.0);
                if up { (bound - eps).ceil() } else { (bound + eps).floor() }
            };
            (round(lower, true), round(upper, false))
        }
        VariableKind::Continuous | VariableKind::Sos => (lower, upper),
    }
}

/// Bounds of `sum a_j x_j + constant` over the variables' domains. Infinite
/// contributions are counted, never added, so there is no `inf - inf`.
fn activity(problem: &LpProblem, terms: &Terms, tolerance: f64) -> Option<Activity> {
    if !terms.linear {
        return None;
    }
    let (mut low, mut high) = (terms.constant, terms.constant);
    let (mut low_infinite, mut high_infinite) = (false, false);
    for &(name, a) in &terms.items {
        if a.is_nan() || a == 0.0 {
            continue;
        }
        let Some(variable) = problem.variables.get(&name) else { continue };
        let (l, u) = domain(variable, tolerance);
        let (at_low, at_high) = if a > 0.0 { (l, u) } else { (u, l) };
        for (bound, sum, infinite) in [(at_low, &mut low, &mut low_infinite), (at_high, &mut high, &mut high_infinite)] {
            if bound.is_infinite() {
                *infinite = true;
            } else {
                *sum += a * bound;
            }
        }
    }
    Some(Activity { min: if low_infinite { None } else { finite(low) }, max: if high_infinite { None } else { finite(high) } })
}

/// Where each variable first occurs inside the entity written at `range`.
fn occurrences_in(doc: &Document, range: &Range<usize>) -> FxHashMap<String, lsp::Range> {
    let index = doc.index();
    let sites = index.sites();
    let first = sites.partition_point(|(r, _)| r.start < range.start);
    let mut found = FxHashMap::default();
    for (site, symbol) in &sites[first..] {
        if site.start >= range.end {
            break;
        }
        if let Symbol::Variable(variable, _) = symbol {
            found.entry(index.variables[*variable].name.clone()).or_insert_with(|| doc.range(site.clone()));
        }
    }
    found
}

/// Codes of the diagnostics that name the entity as subject or related.
fn diagnostic_codes(analysis: &Analysis<'_>, kind: UpstreamKind, name: &str) -> Vec<&'static str> {
    let mut codes: Vec<&'static str> = Vec::new();
    for diagnostic in analysis.iter() {
        let names_it = |s: &lp_parser_rs::analysis::DiagnosticSubject| s.kind == kind && s.name == name;
        let hit = diagnostic.subject.as_ref().is_some_and(names_it) || diagnostic.related.iter().any(|r| names_it(&r.subject));
        if hit && !codes.contains(&diagnostic.code.as_str()) {
            codes.push(diagnostic.code.as_str());
        }
    }
    codes
}

/// Up to [`MAX_RELATED`] other rows sharing variables with row `row`, most shared first.
fn related(problem: &LpProblem, structure: &Structure, row: usize, cancel: &Cancel) -> std::result::Result<Vec<Related>, RequestError> {
    let columns = structure.columns(cancel)?;
    let mut shared: FxHashMap<usize, usize> = FxHashMap::default();
    for (col, _) in structure.entries(row) {
        for &other in columns.rows(col) {
            if other != row {
                *shared.entry(other).or_insert(0) += 1;
            }
        }
    }
    let mut ranked: Vec<(usize, usize)> = shared.into_iter().collect();
    ranked.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    Ok(ranked
        .into_iter()
        .take(MAX_RELATED)
        .map(|(other, shared)| {
            let r = structure.rows[other];
            Related {
                name: problem.resolve(r.name).to_owned(),
                kind: if r.kind == RowKind::Objective { "objective" } else { "constraint" },
                shared,
            }
        })
        .collect())
}

fn number(value: f64) -> String {
    if value.is_infinite() {
        return if value > 0.0 { "∞".to_owned() } else { "-∞".to_owned() };
    }
    format!("{value}")
}

/// `3 x + 2 y - z` for the first terms of a row.
fn expression(problem: &LpProblem, terms: &Terms) -> String {
    let mut out = String::new();
    for (i, &(name, a)) in terms.items.iter().take(MARKDOWN_TERMS).enumerate() {
        let name = problem.resolve(name);
        let (sign, magnitude) = if a.is_nan() {
            ('+', None)
        } else if a < 0.0 {
            ('-', Some(-a))
        } else {
            ('+', Some(a))
        };
        let coefficient = magnitude.filter(|m| (*m - 1.0).abs() > f64::EPSILON).map(|m| format!("{} ", number(m)));
        let coefficient = coefficient.unwrap_or_default();
        match (i, sign) {
            (0, '+') => write!(out, "{coefficient}{name}"),
            (0, _) => write!(out, "-{coefficient}{name}"),
            (_, s) => write!(out, " {s} {coefficient}{name}"),
        }
        .expect("writing to a String cannot fail");
    }
    if terms.items.len() > MARKDOWN_TERMS {
        out.push_str(" + …");
    }
    out
}

/// Bullet list of the first terms with their bounds.
fn term_bullets(problem: &LpProblem, terms: &Terms, md: &mut String) {
    for &(name, a) in terms.items.iter().take(MARKDOWN_TERMS) {
        let text = problem.resolve(name);
        let range = problem.variables.get(&name).map(|v| (v.bounds.effective_lower(v.kind), v.bounds.effective_upper(v.kind)));
        let domain = range.map_or_else(String::new, |(l, u)| format!(", {} ≤ {text} ≤ {}", number(l), number(u)));
        if a.is_nan() {
            writeln!(md, "- `{text}` is referenced{domain}.").expect("writing to a String cannot fail");
        } else {
            writeln!(md, "- `{text}` contributes {} per unit{domain}.", number(a)).expect("writing to a String cannot fail");
        }
    }
    if terms.items.len() > MARKDOWN_TERMS {
        writeln!(md, "- … and {} more.", terms.items.len() - MARKDOWN_TERMS).expect("writing to a String cannot fail");
    }
}

fn activity_sentence(activity: Option<Activity>) -> String {
    match activity {
        Some(Activity { min, max }) => {
            let min = min.map_or_else(|| "-∞".to_owned(), number);
            let max = max.map_or_else(|| "∞".to_owned(), number);
            format!("Given the variable bounds its value can lie in [{min}, {max}].")
        }
        None => String::new(),
    }
}

/// Append the stored solution's line to an explanation.
fn push_solution(md: &mut String, line: Option<String>) {
    if let Some(line) = line {
        let trimmed = md.trim_end().len();
        md.truncate(trimmed);
        md.push_str("\n\n");
        md.push_str(&line);
    }
}

/// What the entity explanations add to the model: the stored solve result and
/// the published findings.
#[derive(Clone, Copy)]
struct Context<'a> {
    solution: Option<&'a Solution>,
    analysis: &'a Analysis<'a>,
}

fn truncate_terms(terms: &Terms) -> (&[(NameId, f64)], bool) {
    (&terms.items[..terms.items.len().min(MAX_TERMS)], terms.items.len() > MAX_TERMS)
}

fn term_list(
    problem: &LpProblem,
    terms: &[(NameId, f64)],
    ranges: &FxHashMap<String, lsp::Range>,
    cancel: &Cancel,
) -> std::result::Result<Vec<Term>, RequestError> {
    let mut out = Vec::with_capacity(terms.len());
    for (i, &(name, a)) in terms.iter().enumerate() {
        cancel.tick(i)?;
        let variable = problem.resolve(name).to_owned();
        out.push(Term { range: ranges.get(&variable).copied(), variable, coefficient: finite(a) });
    }
    Ok(out)
}

fn constraint(
    doc: &Document,
    problem: &LpProblem,
    sites: &Sites,
    name: &str,
    document: DocumentRef,
    context: &Context<'_>,
    cancel: &Cancel,
) -> std::result::Result<Result, RequestError> {
    let Context { solution, analysis } = *context;
    let current = document.version;
    let id = problem.name_id(name).filter(|id| problem.constraints.contains_key(id)).ok_or_else(|| unknown(Kind::Constraint, name))?;
    let (index, _, constraint) = problem.constraints.get_full(&id).ok_or_else(|| unknown(Kind::Constraint, name))?;
    let site = sites.constraint(id);
    let terms = Terms::from_constraint(constraint);
    let ranges = site.as_ref().map_or_else(FxHashMap::default, |s| occurrences_in(doc, &doc.index().entities[s.entity].range));
    let (shown, truncated) = truncate_terms(&terms);
    let activity = activity(problem, &terms, analysis.equality_tolerance());
    let relation = relation(constraint);
    let structure = Structure::build(problem, Scope::All, true, cancel)?;
    let row = structure.rows.iter().position(|r| r.kind == RowKind::Constraint && r.index == index);
    let related = row.map(|row| related(problem, &structure, row, cancel)).transpose()?.unwrap_or_default();

    let mut md = format!("### {name}\n");
    let class = constraint_class(problem.constraint_class(id));
    let label = match constraint_kind(constraint) {
        "linear" => "Linear constraint".to_owned(),
        "indicator" => "Indicator constraint".to_owned(),
        "quadratic" => "Quadratic constraint".to_owned(),
        "sos1" => "SOS1 set".to_owned(),
        "sos2" => "SOS2 set".to_owned(),
        _ => "General constraint".to_owned(),
    };
    let class_note = if class == "normal" { String::new() } else { format!(" ({class})") };
    match relation {
        Some((op, rhs)) => {
            writeln!(md, "{label}{class_note}: `{} {op} {}`.\n", expression(problem, &terms), rhs.map_or_else(|| "?".to_owned(), number))
        }
        None => writeln!(md, "{label}{class_note} over {} variable(s).\n", terms.items.len()),
    }
    .expect("writing to a String cannot fail");
    term_bullets(problem, &terms, &mut md);
    if let Some((op, Some(rhs))) = relation {
        let sentence = match op {
            "<=" | "<" => format!("The left-hand side cannot exceed {}.", number(rhs)),
            ">=" | ">" => format!("The left-hand side must be at least {}.", number(rhs)),
            _ => format!("The left-hand side must equal {}.", number(rhs)),
        };
        writeln!(md, "\n{sentence} {}", activity_sentence(activity)).expect("writing to a String cannot fail");
    }
    push_solution(&mut md, solution.and_then(|s| s.constraint_markdown(name, current)));

    Ok(Result {
        document,
        kind: "constraint",
        name: name.to_owned(),
        description: site.as_ref().and_then(|s| description::entity(doc, s.entity)),
        range: site.as_ref().map(|s| s.range),
        markdown: md.trim_end().to_owned(),
        terms: Some(term_list(problem, shown, &ranges, cancel)?),
        terms_truncated: Some(truncated),
        operator: relation.map(|(op, _)| op),
        rhs: relation.map(|(_, rhs)| rhs),
        activity,
        related: Some(related),
        usage: None,
        usage_truncated: None,
        bounds: None,
        diagnostics: diagnostic_codes(analysis, UpstreamKind::Constraint, name),
        solution: solution.and_then(|s| s.constraint_json(name, current)),
    })
}

fn objective(
    doc: &Document,
    problem: &LpProblem,
    sites: &Sites,
    name: &str,
    document: DocumentRef,
    context: &Context<'_>,
    cancel: &Cancel,
) -> std::result::Result<Result, RequestError> {
    let Context { solution, analysis } = *context;
    let current = document.version;
    let id = problem.name_id(name).filter(|id| problem.objectives.contains_key(id)).ok_or_else(|| unknown(Kind::Objective, name))?;
    let (index, _, objective) = problem.objectives.get_full(&id).ok_or_else(|| unknown(Kind::Objective, name))?;
    let site = sites.objective(id);
    let terms = Terms::from_objective(objective);
    let ranges = site.as_ref().map_or_else(FxHashMap::default, |s| occurrences_in(doc, &doc.index().entities[s.entity].range));
    let (shown, truncated) = truncate_terms(&terms);
    let activity = activity(problem, &terms, analysis.equality_tolerance());
    let structure = Structure::build(problem, Scope::All, true, cancel)?;
    let row = structure.rows.iter().position(|r| r.kind == RowKind::Objective && r.index == index);
    let related = row.map(|row| related(problem, &structure, row, cancel)).transpose()?.unwrap_or_default();

    let verb = if problem.sense == lp_parser_rs::model::Sense::Maximize { "Maximise" } else { "Minimise" };
    let mut md = format!("### {name}\n{verb} `{}`", expression(problem, &terms));
    if objective.constant != 0.0 {
        write!(md, " (plus the constant {})", number(objective.constant)).expect("writing to a String cannot fail");
    }
    md.push_str(".\n\n");
    term_bullets(problem, &terms, &mut md);
    let note = activity_sentence(activity);
    if !note.is_empty() {
        write!(md, "\n{note}").expect("writing to a String cannot fail");
    }
    push_solution(&mut md, solution.and_then(|s| s.objective_markdown(current)));

    Ok(Result {
        document,
        kind: "objective",
        name: name.to_owned(),
        description: site.as_ref().and_then(|s| description::entity(doc, s.entity)),
        range: site.as_ref().map(|s| s.range),
        markdown: md.trim_end().to_owned(),
        terms: Some(term_list(problem, shown, &ranges, cancel)?),
        terms_truncated: Some(truncated),
        operator: None,
        rhs: None,
        activity,
        related: Some(related),
        usage: None,
        usage_truncated: None,
        bounds: None,
        diagnostics: diagnostic_codes(analysis, UpstreamKind::Objective, name),
        solution: solution.and_then(|s| s.objective_json(current)),
    })
}

/// Wire role and entity kind of an occurrence.
const fn role_name(role: Role) -> &'static str {
    match role {
        Role::ObjectiveTerm | Role::ConstraintTerm => "linear",
        Role::QuadraticTerm => "quadratic",
        Role::Bound => "bound",
        Role::Generals | Role::Integers | Role::Binaries | Role::SemiContinuous => "type",
        Role::SosEntry => "sos",
        Role::Indicator => "indicator",
        Role::Resultant | Role::GeneralArgument => "general",
    }
}

fn variable(
    doc: &Document,
    problem: &LpProblem,
    sites: &Sites,
    name: &str,
    document: DocumentRef,
    context: &Context<'_>,
    cancel: &Cancel,
) -> std::result::Result<Result, RequestError> {
    let Context { solution, analysis } = *context;
    let current = document.version;
    let id = problem.name_id(name).ok_or_else(|| unknown(Kind::Variable, name))?;
    let var = problem.variables.get(&id).ok_or_else(|| unknown(Kind::Variable, name))?;
    let index = doc.index();
    let mut usage = Vec::new();
    let (mut objectives, mut constraints) = (0usize, 0usize);
    let mut counted: Vec<usize> = Vec::new();
    let mut truncated = false;
    if let Some(entry) = index.variable(name) {
        for (i, occurrence) in entry.occurrences.iter().enumerate() {
            cancel.tick(i)?;
            if let Some(e) = occurrence.entity.filter(|e| !counted.contains(e)) {
                counted.push(e);
                if index.entities[e].kind == IndexKind::Objective { objectives += 1 } else { constraints += 1 }
            }
            if usage.len() >= MAX_USAGE {
                truncated = true;
                continue;
            }
            let entity = occurrence.entity;
            usage.push(UsageItem {
                entity: entity.and_then(|e| sites.entity_name(e)),
                kind: entity.map(|e| if index.entities[e].kind == IndexKind::Objective { "objective" } else { "constraint" }),
                role: role_name(occurrence.role),
                coefficient: occurrence.coefficient.and_then(finite),
                range: doc.range(occurrence.range.clone()),
            });
        }
    }

    let bounds = Bounds::of(var);
    let mut md = format!("### {name}\n{} variable", var.kind);
    let (lower, upper) = (var.bounds.effective_lower(var.kind), var.bounds.effective_upper(var.kind));
    write!(md, ", {} ≤ {name} ≤ {}", number(lower), number(upper)).expect("writing to a String cannot fail");
    let declared = match (var.bounds.lower.is_some(), var.bounds.upper.is_some()) {
        (true, true) => "both bounds declared",
        (true, false) => "only the lower bound declared",
        (false, true) => "only the upper bound declared",
        (false, false) => "no bounds declared, so the default lower bound of 0 applies",
    };
    write!(md, " ({declared}).\n\n").expect("writing to a String cannot fail");
    let plural = |n: usize, word: &str| format!("{n} {word}{}", if n == 1 { "" } else { "s" });
    if objectives + constraints == 0 {
        md.push_str("It is not used by any objective or constraint.");
    } else {
        write!(md, "Used in {} and {}.", plural(constraints, "constraint"), plural(objectives, "objective"))
            .expect("writing to a String cannot fail");
    }
    push_solution(&mut md, solution.and_then(|s| s.variable_markdown(name, current)));

    Ok(Result {
        document,
        kind: "variable",
        name: name.to_owned(),
        description: index.variable(name).and_then(|v| description::variable(doc, v)),
        range: sites.variable(name),
        markdown: md,
        terms: None,
        terms_truncated: None,
        operator: None,
        rhs: None,
        activity: None,
        related: None,
        usage: Some(usage),
        usage_truncated: Some(truncated),
        bounds: Some(VariableBounds { bounds, variable_kind: variable_kind(var.kind) }),
        diagnostics: diagnostic_codes(analysis, UpstreamKind::Variable, name),
        solution: solution.and_then(|s| s.variable_json(name, current)),
    })
}

#[cfg(test)]
mod tests {
    use lp_highs::protocol::{ConstraintSolution, Skipped, SolveOutcome, SolverId, Timings, VariableSolution};
    use serde_json::json;

    use super::super::testing::{MODEL, doc, json, target};
    use super::*;

    fn params(kind: Kind, name: &str) -> Params {
        Params { target: target(), entity: Some(Entity { kind, name: name.to_owned() }), position: None }
    }

    fn run(text: &str, params: &Params) -> Value {
        json(&handle(&doc(text), Some(1), params, None, &AnalysisSettings::default(), &Cancel::new()).expect("explain"))
    }

    fn solution() -> Solution {
        let outcome = SolveOutcome {
            solver: SolverId { name: "HiGHS".into(), version: "1".into(), helper: "lp-highs 0.1.0".into() },
            status: "Optimal".into(),
            objective_value: Some(7.0),
            mip_gap: None,
            timings: Timings::default(),
            variables: vec![VariableSolution { name: "x".into(), value: 2.0, reduced_cost: Some(0.0) }],
            constraints: vec![ConstraintSolution { name: "c1".into(), activity: 2.0, dual: Some(1.5), slack: 0.0, binding: true }],
            skipped: Skipped::default(),
            infeasibility: None,
            log: None,
        };
        Solution::new(Some(1), &outcome)
    }

    #[test]
    fn explains_a_constraint_with_terms_activity_and_related_rows() {
        let result = run(MODEL, &params(Kind::Constraint, "c2"));
        assert_eq!((result["kind"].clone(), result["name"].clone()), (json!("constraint"), json!("c2")));
        assert_eq!((result["operator"].clone(), result["rhs"].clone()), (json!("<="), json!(100.0)));
        assert_eq!(result["terms"][0]["variable"], "x");
        assert_eq!(result["terms"][1]["coefficient"], 4.0);
        assert!(result["terms"][0]["range"]["start"].is_object());
        assert_eq!(result["termsTruncated"], false);
        // 2x + 4y with x in [0, 10] and y in [1, 5]: [4, 40].
        assert_eq!(result["activity"], json!({ "min": 4.0, "max": 40.0 }));
        let related = result["related"].as_array().expect("related");
        assert!(related.iter().any(|r| r["name"] == "c1" && r["shared"] == 2 && r["kind"] == "constraint"), "{related:?}");
        assert!(related.iter().any(|r| r["name"] == "obj" && r["kind"] == "objective"));
        let md = result["markdown"].as_str().expect("markdown");
        assert!(md.starts_with("### c2\n") && md.contains("2 x + 4 y <= 100") && md.contains("`y` contributes 4 per unit"), "{md}");
        assert!(md.contains("cannot exceed 100"), "{md}");
        assert!(result.get("solution").is_none() && result.get("usage").is_none());
    }

    #[test]
    fn infinite_activity_sides_are_null() {
        let result = run("Minimize\n obj: x\nSubject To\n c1: x - y >= 1\nEnd\n", &params(Kind::Constraint, "c1"));
        assert_eq!(result["activity"], json!({ "min": null, "max": null }), "x - y over [0, inf) x [0, inf) is unbounded both ways");
        let bounded = run("Minimize\n obj: x\nSubject To\n c1: x + y >= 1\nBounds\n x <= 3\nEnd\n", &params(Kind::Constraint, "c1"));
        assert_eq!(bounded["activity"], json!({ "min": 0.0, "max": null }));
    }

    #[test]
    fn integer_and_semi_domains_tighten_or_widen_activity() {
        let text = "Minimize\n obj: x\nSubject To\n c1: x + y + s >= 1\nBounds\n 0.5 <= x <= 3.5\n 1 <= s <= 4\nGenerals\n x\nSemi-Continuous\n s\nEnd\n";
        let result = run(text, &params(Kind::Constraint, "c1"));
        // x integral in [1, 3]; s semi-continuous in [0, 4]; y in [0, inf).
        assert_eq!(result["activity"], json!({ "min": 1.0, "max": null }));
        let all_finite = run(
            "Minimize\n obj: x\nSubject To\n c1: x + s >= 1\nBounds\n 0.5 <= x <= 3.5\n 1 <= s <= 4\nGenerals\n x\nSemi-Continuous\n s\nEnd\n",
            &params(Kind::Constraint, "c1"),
        );
        assert_eq!(all_finite["activity"], json!({ "min": 1.0, "max": 7.0 }));
    }

    #[test]
    fn explain_includes_the_description() {
        let text = "Minimize\n \\ @description Total cost\n obj: x + y\nSubject To\n \\ @description Plant cap\n c1: x + y <= 4\n c2: x >= 1\nBounds\n \\ @description Width\n x <= 4\nEnd\n";
        let described = [(Kind::Constraint, "c1", "Plant cap"), (Kind::Objective, "obj", "Total cost"), (Kind::Variable, "x", "Width")];
        for (kind, name, want) in described {
            let result = run(text, &params(kind, name));
            assert_eq!(result["description"], want, "{name}");
            assert!(!result["markdown"].as_str().unwrap().contains(want), "markdown is unchanged");
        }
        assert!(run(text, &params(Kind::Constraint, "c2")).get("description").is_none());
        assert!(run(text, &params(Kind::Variable, "y")).get("description").is_none());
    }

    #[test]
    fn explains_an_objective() {
        let result = run(MODEL, &params(Kind::Objective, "obj"));
        assert_eq!(result["kind"], "objective");
        assert!(result.get("operator").is_none() && result.get("rhs").is_none());
        assert_eq!(result["terms"].as_array().map(Vec::len), Some(3));
        assert!(result["markdown"].as_str().is_some_and(|m| m.contains("Minimise `3 x + 2 y + z`")));
        assert!(result["related"].as_array().is_some_and(|r| r.iter().any(|r| r["name"] == "c3" && r["shared"] == 3)));
        let quadratic = run("Minimize\n obj: x + [ x ^ 2 ] / 2\nSubject To\n c1: x >= 1\nEnd\n", &params(Kind::Objective, "obj"));
        assert!(quadratic.get("activity").is_none(), "no activity bounds for a quadratic objective");
        assert_eq!(quadratic["terms"][0]["coefficient"], 1.0);
    }

    #[test]
    fn explains_a_variable_with_usage_and_bounds() {
        let result = run(MODEL, &params(Kind::Variable, "y"));
        assert_eq!(result["kind"], "variable");
        assert_eq!(
            result["bounds"],
            json!({ "lower": 1.0, "upper": 5.0, "declaredLower": true, "declaredUpper": true, "variableKind": "continuous" })
        );
        let usage = result["usage"].as_array().expect("usage");
        let roles: Vec<_> =
            usage.iter().map(|u| (u["entity"].as_str().unwrap_or("-").to_owned(), u["role"].as_str().unwrap_or("").to_owned())).collect();
        assert_eq!(
            roles,
            [
                ("obj".into(), "linear".into()),
                ("c1".into(), "linear".into()),
                ("c2".into(), "linear".into()),
                ("c3".into(), "linear".into()),
                ("-".into(), "bound".into())
            ]
        );
        assert_eq!(usage[2]["coefficient"], 4.0);
        assert_eq!(usage[4]["kind"], json!(null));
        assert!(result["markdown"].as_str().is_some_and(|m| m.contains("Used in 3 constraints and 1 objective")));
        let z = run(MODEL, &params(Kind::Variable, "z"));
        assert!(z["usage"].as_array().is_some_and(|u| u.iter().any(|u| u["role"] == "type")), "generals entry: {z}");
        assert_eq!(z["bounds"]["variableKind"], "general");
    }

    #[test]
    fn generated_names_appear_in_usage() {
        let result = run("Minimize\n obj: x\nSubject To\n x >= 1\nEnd\n", &params(Kind::Variable, "x"));
        let entities: Vec<_> = result["usage"].as_array().expect("usage").iter().map(|u| u["entity"].clone()).collect();
        assert!(
            entities.contains(&json!("C1")) || entities.iter().any(|e| e.as_str().is_some_and(|s| s.starts_with('C'))),
            "an unlabelled constraint is named by the model: {entities:?}"
        );
    }

    #[test]
    fn diagnostics_name_both_sides_of_a_finding() {
        let text = "Minimize\n obj: x + y\nSubject To\n c1: x + y >= 2\n c2: 2 x + 2 y >= 4\nEnd\n";
        let c2 = run(text, &params(Kind::Constraint, "c2"));
        let c1 = run(text, &params(Kind::Constraint, "c1"));
        assert!(c2["diagnostics"].as_array().is_some_and(|d| d.contains(&json!("proportional-row"))), "{c2}");
        assert!(
            c1["diagnostics"].as_array().is_some_and(|d| d.contains(&json!("proportional-row"))),
            "the representative is related: {c1}"
        );
        assert_eq!(
            run(MODEL, &params(Kind::Constraint, "c2"))["diagnostics"],
            json!(["row-redundant-by-bounds"]),
            "2x + 4y <= 100 always holds when x <= 10 and y <= 5"
        );
        assert_eq!(run(MODEL, &params(Kind::Constraint, "c1"))["diagnostics"], json!([]));
    }

    fn settings(value: Value) -> AnalysisSettings {
        serde_json::from_value(value).expect("analysis settings")
    }

    fn explain_with(text: &str, params: &Params, settings: &AnalysisSettings) -> Value {
        json(&handle(&doc(text), Some(1), params, None, settings, &Cancel::new()).expect("explain"))
    }

    #[test]
    fn diagnostics_follow_the_analysis_settings() {
        let text = "Minimize\n obj: x + y\nSubject To\n c1: x + y >= 2\n c2: 2 x + 2 y >= 4\nEnd\n";
        let p = params(Kind::Constraint, "c2");
        let off = settings(json!({ "severityOverrides": { "proportional-row": "off" } }));
        assert_eq!(explain_with(text, &p, &off)["diagnostics"], json!([]), "an `off` override hides the code");
        let no_analysis = settings(json!({ "enabled": false }));
        assert_eq!(explain_with(text, &p, &no_analysis)["diagnostics"], json!([]));
        let default = explain_with(text, &p, &AnalysisSettings::default());
        assert!(default["diagnostics"].as_array().is_some_and(|d| d.contains(&json!("proportional-row"))), "{default}");
    }

    #[test]
    fn integer_domains_round_with_the_configured_equality_tolerance() {
        // x in [0.0000001, 3]: the default tolerance (1e-9) rounds the lower bound up to 1;
        // a loose tolerance treats it as 0.
        let text = "Minimize\n obj: x\nSubject To\n c1: x >= 0\nBounds\n 0.0000001 <= x <= 3\nGenerals\n x\nEnd\n";
        let p = params(Kind::Constraint, "c1");
        let tight = explain_with(text, &p, &AnalysisSettings::default());
        assert_eq!(tight["activity"]["min"], 1.0, "{tight}");
        let loose = explain_with(text, &p, &settings(json!({ "equalityTolerance": 1e-3 })));
        assert_eq!(loose["activity"]["min"], 0.0, "{loose}");
    }

    #[test]
    fn position_picks_the_entity_or_variable_under_the_cursor() {
        let d = doc(MODEL);
        let at = |needle: &str, extra: usize| d.position(MODEL.find(needle).expect("needle") + extra);
        let by_position = |position| {
            let p = Params { target: target(), entity: None, position: Some(position) };
            json(&handle(&d, Some(1), &p, None, &AnalysisSettings::default(), &Cancel::new()).expect("explain"))
        };
        let on_label = by_position(at(" c2:", 2));
        assert_eq!((on_label["kind"].clone(), on_label["name"].clone()), (json!("constraint"), json!("c2")));
        let on_variable = by_position(at("2 x + 4 y", 2));
        assert_eq!((on_variable["kind"].clone(), on_variable["name"].clone()), (json!("variable"), json!("x")));
        let in_row = by_position(at("c1: x + y >= 2", 12));
        assert_eq!(in_row["name"], "c1", "inside the row but not on a name: the enclosing constraint");
        let objective = by_position(at("obj:", 1));
        assert_eq!(objective["kind"], "objective");
    }

    #[test]
    fn unlabelled_entities_resolve_to_their_generated_names() {
        let text = "Minimize\n obj: x\nSubject To\n x >= 1\nEnd\n";
        let d = doc(text);
        let p = Params { target: target(), entity: None, position: Some(d.position(text.find("x >= 1").expect("row") + 3)) };
        let result = json(&handle(&d, Some(1), &p, None, &AnalysisSettings::default(), &Cancel::new()).expect("explain"));
        assert_eq!(result["kind"], "constraint");
        assert!(result["name"].as_str().is_some_and(|n| n.starts_with('C')), "{result}");
    }

    #[test]
    fn solution_is_included_and_marked_stale_after_an_edit() {
        let s = solution();
        let fresh = json(
            &handle(&doc(MODEL), Some(1), &params(Kind::Constraint, "c1"), Some(&s), &AnalysisSettings::default(), &Cancel::new())
                .expect("explain"),
        );
        assert_eq!(fresh["solution"], json!({ "stale": false, "activity": 2.0, "slack": 0.0, "binding": true, "dual": 1.5 }));
        let stale = json(
            &handle(&doc(MODEL), Some(2), &params(Kind::Variable, "x"), Some(&s), &AnalysisSettings::default(), &Cancel::new())
                .expect("explain"),
        );
        assert_eq!(stale["solution"]["stale"], true);
        assert_eq!(stale["solution"]["value"], 2.0);
        let objective = json(
            &handle(&doc(MODEL), Some(1), &params(Kind::Objective, "obj"), Some(&s), &AnalysisSettings::default(), &Cancel::new())
                .expect("explain"),
        );
        assert_eq!(objective["solution"], json!({ "stale": false, "value": 7.0 }));
        let none = json(
            &handle(&doc(MODEL), Some(1), &params(Kind::Variable, "y"), Some(&s), &AnalysisSettings::default(), &Cancel::new())
                .expect("explain"),
        );
        assert!(none.get("solution").is_none(), "no solution entry for y");
    }

    #[test]
    fn rejects_unknown_targets_and_missing_selectors() {
        let d = doc(MODEL);
        let err = |p: Params| handle(&d, Some(1), &p, None, &AnalysisSettings::default(), &Cancel::new()).err();
        assert!(matches!(err(params(Kind::Constraint, "nope")), Some(RequestError::InvalidParams(m)) if m.contains("nope")));
        assert!(matches!(err(params(Kind::Constraint, "x")), Some(RequestError::InvalidParams(_))), "x is a variable");
        assert!(matches!(err(params(Kind::Variable, "c1")), Some(RequestError::InvalidParams(_))));
        assert!(matches!(err(params(Kind::Objective, "c1")), Some(RequestError::InvalidParams(_))));
        assert!(matches!(err(Params { target: target(), entity: None, position: None }), Some(RequestError::InvalidParams(_))));
        let blank = Params { target: target(), entity: None, position: Some(lsp::Position::new(0, 0)) };
        assert!(matches!(err(blank), Some(RequestError::InvalidParams(m)) if m.contains("position")), "the sense keyword is no entity");
        assert!(matches!(
            handle(
                &doc("min\n obj: x +\nst\n c: >= \nend\n"),
                None,
                &params(Kind::Variable, "x"),
                None,
                &AnalysisSettings::default(),
                &Cancel::new()
            ),
            Err(RequestError::Failed(_))
        ));
    }

    #[test]
    fn long_rows_are_capped_and_cancellation_is_honoured() {
        let mut text = String::from("Minimize\n obj: x0\nSubject To\n big: ");
        text.push_str(&(0..MAX_TERMS + 5).map(|i| format!("x{i}")).collect::<Vec<_>>().join(" + "));
        text.push_str(" >= 1\nEnd\n");
        let result = run(&text, &params(Kind::Constraint, "big"));
        assert_eq!(result["terms"].as_array().map(Vec::len), Some(MAX_TERMS));
        assert_eq!(result["termsTruncated"], true);
        assert!(result["markdown"].as_str().is_some_and(|m| m.contains("and 9997 more")), "markdown lists {MARKDOWN_TERMS} terms");

        let cancel = Cancel::new();
        cancel.cancel();
        assert_eq!(
            handle(&doc(&text), None, &params(Kind::Constraint, "big"), None, &AnalysisSettings::default(), &cancel).err(),
            Some(RequestError::Cancelled)
        );
    }
}
