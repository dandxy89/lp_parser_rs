//! `lp/model`: the Model Explorer's view of a document.

use lp_parser_rs::analysis::IssueSeverity;
use lp_parser_rs::model::{Constraint, SOSType};
use lp_parser_rs::{LpProblem, NameId};
use ls_types as lsp;
use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};

use super::sites::Sites;
use super::{Analysis, Cancel, DocumentRef, RequestError, Source, Target, check_limit, constraint_class, finite, operator, variable_kind};
use crate::config::AnalysisSettings;
use crate::description;
use crate::document::Document;

/// Default page size of each list.
pub const DEFAULT_LIMIT: usize = 5000;
/// Largest page size of each list.
pub const MAX_LIMIT: usize = 50_000;

/// A part of the result a client may ask for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Section {
    /// Counts and diagnostics totals.
    Summary,
    /// Objectives.
    Objectives,
    /// Constraints.
    Constraints,
    /// Variables.
    Variables,
}

/// Params of `lp/model`.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Params {
    /// Document and progress token.
    #[serde(flatten)]
    pub target: Target,
    /// Sections to return (all when absent).
    #[serde(default)]
    pub include: Option<Vec<Section>>,
    /// First item of each list.
    #[serde(default)]
    pub offset: usize,
    /// Page size of each list.
    #[serde(default = "default_limit")]
    pub limit: usize,
}

const fn default_limit() -> usize {
    DEFAULT_LIMIT
}

/// Result of `lp/model`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Result {
    #[serde(flatten)]
    pub document: DocumentRef,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<Summary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub objectives: Option<Vec<ObjectiveItem>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub constraints: Option<Vec<ConstraintItem>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub variables: Option<Vec<VariableItem>>,
    pub truncated: Truncated,
}

/// Which lists were cut by the page size.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct Truncated {
    pub objectives: bool,
    pub constraints: bool,
    pub variables: bool,
}

/// Model totals.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    pub name: Option<String>,
    pub sense: String,
    pub objectives: usize,
    pub constraints: usize,
    pub variables: usize,
    pub nonzeros: usize,
    pub density: f64,
    pub quadratic_objective_terms: usize,
    pub quadratic_constraint_terms: usize,
    pub variables_by_kind: std::collections::BTreeMap<&'static str, usize>,
    pub constraints_by_kind: std::collections::BTreeMap<&'static str, usize>,
    pub diagnostics: DiagnosticCounts,
}

/// Diagnostics per severity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct DiagnosticCounts {
    pub error: usize,
    pub warning: usize,
    pub info: usize,
}

/// One objective.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ObjectiveItem {
    pub name: String,
    pub range: Option<lsp::Range>,
    pub selection_range: Option<lsp::Range>,
    pub terms: usize,
    pub quadratic_terms: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attributes: Option<Attributes>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// Multi-objective attributes that are set.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Attributes {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub priority: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub weight: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub abs_tol: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rel_tol: Option<f64>,
}

/// One constraint.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConstraintItem {
    pub name: String,
    pub generated: bool,
    pub kind: &'static str,
    pub class: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operator: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rhs: Option<Option<f64>>,
    pub terms: usize,
    pub range_partner: Option<String>,
    pub range: Option<lsp::Range>,
    pub selection_range: Option<lsp::Range>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// One variable.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VariableItem {
    pub name: String,
    pub kind: &'static str,
    pub bounds: Bounds,
    pub usage: Usage,
    pub unused: bool,
    pub free: bool,
    pub range: Option<lsp::Range>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// Effective bounds: `null` is unbounded on that side.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Bounds {
    pub lower: Option<f64>,
    pub upper: Option<f64>,
    pub declared_lower: bool,
    pub declared_upper: bool,
}

/// How many objectives and constraints use a variable.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct Usage {
    pub objectives: usize,
    pub constraints: usize,
    pub quadratic: usize,
}

impl Bounds {
    /// Effective bounds of `variable`.
    #[must_use]
    pub fn of(variable: &lp_parser_rs::model::Variable) -> Self {
        Self {
            lower: finite(variable.bounds.effective_lower(variable.kind)),
            upper: finite(variable.bounds.effective_upper(variable.kind)),
            declared_lower: variable.bounds.lower.is_some(),
            declared_upper: variable.bounds.upper.is_some(),
        }
    }
}

/// Handle `lp/model`.
///
/// # Errors
/// `parseError`, `-32602` for an out-of-range page, or cancellation.
pub fn handle(
    doc: &Document,
    version: Option<i32>,
    params: &Params,
    settings: &AnalysisSettings,
    cancel: &Cancel,
) -> std::result::Result<Result, RequestError> {
    check_limit("limit", params.limit, MAX_LIMIT)?;
    let wants = |section| params.include.as_ref().is_none_or(|include| include.contains(&section));
    let source = Source::acquire(doc)?;
    let problem = source.problem();
    cancel.set_total(problem.objectives.len() + problem.constraints.len() + problem.variables.len());
    let sites = Sites::new(doc, problem);
    let page = |len: usize| {
        let start = params.offset.min(len);
        (start..start.saturating_add(params.limit).min(len), start.saturating_add(params.limit) < len)
    };
    let mut truncated = Truncated::default();

    let summary = if wants(Section::Summary) { Some(summary(&source, settings, cancel)?) } else { None };

    let objectives = if wants(Section::Objectives) {
        let (range, cut) = page(problem.objectives.len());
        truncated.objectives = cut;
        let mut items = Vec::with_capacity(range.len());
        for (i, objective) in problem.objectives.values().enumerate().skip(range.start).take(range.len()) {
            cancel.tick(i)?;
            let site = sites.objective(objective.name);
            let attributes = (!objective.attributes.is_empty()).then(|| Attributes {
                priority: objective.attributes.priority,
                weight: objective.attributes.weight.and_then(finite),
                abs_tol: objective.attributes.abs_tol.and_then(finite),
                rel_tol: objective.attributes.rel_tol.and_then(finite),
            });
            items.push(ObjectiveItem {
                name: problem.resolve(objective.name).to_owned(),
                range: site.as_ref().map(|s| s.range),
                selection_range: site.as_ref().map(|s| s.selection),
                terms: objective.coefficients.len(),
                quadratic_terms: objective.quadratic.len(),
                attributes,
                description: site.as_ref().and_then(|s| description::entity(doc, s.entity)),
            });
        }
        Some(items)
    } else {
        None
    };

    let constraints = if wants(Section::Constraints) {
        let (range, cut) = page(problem.constraints.len());
        truncated.constraints = cut;
        let partners = range_partners(problem);
        let mut items = Vec::with_capacity(range.len());
        for (i, constraint) in problem.constraints.values().enumerate().skip(range.start).take(range.len()) {
            cancel.tick(i)?;
            items.push(constraint_item(doc, problem, &sites, constraint, &partners));
        }
        Some(items)
    } else {
        None
    };

    let variables = if wants(Section::Variables) {
        let (range, cut) = page(problem.variables.len());
        truncated.variables = cut;
        let usage = usage(problem, cancel)?;
        let mut items = Vec::with_capacity(range.len());
        for (i, variable) in problem.variables.values().enumerate().skip(range.start).take(range.len()) {
            cancel.tick(i)?;
            let usage = usage[i];
            items.push(VariableItem {
                name: problem.resolve(variable.name).to_owned(),
                kind: variable_kind(variable.kind),
                bounds: Bounds::of(variable),
                usage,
                unused: usage == Usage::default(),
                free: variable.bounds.is_free(),
                range: sites.variable(problem.resolve(variable.name)),
                description: doc.index().variable(problem.resolve(variable.name)).and_then(|v| description::variable(doc, v)),
            });
        }
        Some(items)
    } else {
        None
    };

    cancel.check()?;
    Ok(Result {
        document: DocumentRef::new(&params.target.text_document.uri, version),
        summary,
        objectives,
        constraints,
        variables,
        truncated,
    })
}

/// Wire name of a constraint's kind.
#[must_use]
pub const fn constraint_kind(constraint: &Constraint) -> &'static str {
    match constraint {
        Constraint::Standard { .. } => "linear",
        Constraint::Indicator { .. } => "indicator",
        Constraint::Quadratic { .. } => "quadratic",
        Constraint::SOS { sos_type: SOSType::S1, .. } => "sos1",
        Constraint::SOS { sos_type: SOSType::S2, .. } => "sos2",
        Constraint::General { .. } => "general",
    }
}

/// `(operator, rhs)` of constraints that have one. The rhs is `None` when not finite.
#[must_use]
pub fn relation(constraint: &Constraint) -> Option<(&'static str, Option<f64>)> {
    match constraint {
        Constraint::Standard { operator: op, rhs, .. }
        | Constraint::Indicator { operator: op, rhs, .. }
        | Constraint::Quadratic { operator: op, rhs, .. } => Some((operator(*op), finite(*rhs))),
        Constraint::SOS { .. } | Constraint::General { .. } => None,
    }
}

/// Number of terms a constraint lists.
#[must_use]
pub fn term_count(constraint: &Constraint) -> usize {
    match constraint {
        Constraint::Standard { coefficients, .. } | Constraint::Indicator { coefficients, .. } => coefficients.len(),
        Constraint::Quadratic { coefficients, quadratic, .. } => coefficients.len() + quadratic.len(),
        Constraint::SOS { weights, .. } => weights.len(),
        Constraint::General { function, .. } => function.variables().len() + 1,
    }
}

/// The other half of each ranged constraint (`c1` and `c1_rng` come from one
/// statement and share its byte offset).
fn range_partners(problem: &LpProblem) -> FxHashMap<NameId, NameId> {
    let mut by_offset: FxHashMap<usize, Vec<NameId>> = FxHashMap::default();
    for constraint in problem.constraints.values() {
        if let Constraint::Standard { name, byte_offset: Some(offset), .. } = constraint {
            by_offset.entry(*offset).or_default().push(*name);
        }
    }
    let mut partners = FxHashMap::default();
    for names in by_offset.values().filter(|names| names.len() == 2) {
        partners.insert(names[0], names[1]);
        partners.insert(names[1], names[0]);
    }
    partners
}

fn constraint_item(doc: &Document, problem: &LpProblem, sites: &Sites, constraint: &Constraint, partners: &FxHashMap<NameId, NameId>) -> ConstraintItem {
    let id = constraint.name();
    let site = sites.constraint(id);
    let relation = relation(constraint);
    ConstraintItem {
        name: problem.resolve(id).to_owned(),
        generated: site.as_ref().is_none_or(|s| !s.labelled),
        kind: constraint_kind(constraint),
        class: constraint_class(problem.constraint_class(id)),
        operator: relation.map(|(op, _)| op),
        rhs: relation.map(|(_, rhs)| rhs),
        terms: term_count(constraint),
        range_partner: partners.get(&id).map(|&p| problem.resolve(p).to_owned()),
        range: site.as_ref().map(|s| s.range),
        selection_range: site.as_ref().map(|s| s.selection),
        description: site.as_ref().and_then(|s| description::entity(doc, s.entity)),
    }
}

/// Number of objectives and constraints using each variable, by position.
fn usage(problem: &LpProblem, cancel: &Cancel) -> std::result::Result<Vec<Usage>, RequestError> {
    let mut usage = vec![Usage::default(); problem.variables.len()];
    // Last row (1-based) counted for a variable, per counter, so a row counts once.
    let mut last_linear = vec![0usize; usage.len()];
    let mut last_quadratic = vec![0usize; usage.len()];
    let mut row = 0usize;
    let mut count = |name: NameId, quadratic: bool, objective: bool, row: usize, usage: &mut [Usage]| {
        let Some(col) = problem.variables.get_index_of(&name) else { return };
        let (last, field) = if quadratic {
            (&mut last_quadratic[col], &mut usage[col].quadratic)
        } else if objective {
            (&mut last_linear[col], &mut usage[col].objectives)
        } else {
            (&mut last_linear[col], &mut usage[col].constraints)
        };
        if *last != row {
            *last = row;
            *field += 1;
        }
    };
    for objective in problem.objectives.values() {
        row += 1;
        cancel.tick(row)?;
        for c in &objective.coefficients {
            count(c.name, false, true, row, &mut usage);
        }
        for term in &objective.quadratic {
            count(term.var1, true, true, row, &mut usage);
            count(term.var2, true, true, row, &mut usage);
        }
    }
    for constraint in problem.constraints.values() {
        row += 1;
        cancel.tick(row)?;
        match constraint {
            Constraint::Quadratic { coefficients, quadratic, .. } => {
                for c in coefficients {
                    count(c.name, false, false, row, &mut usage);
                }
                for term in quadratic {
                    count(term.var1, true, false, row, &mut usage);
                    count(term.var2, true, false, row, &mut usage);
                }
            }
            other => other.for_each_variable(|name| count(name, false, false, row, &mut usage)),
        }
    }
    Ok(usage)
}

fn summary(source: &Source, settings: &AnalysisSettings, cancel: &Cancel) -> std::result::Result<Summary, RequestError> {
    let problem = source.problem();
    let mut variables_by_kind = std::collections::BTreeMap::new();
    for variable in problem.variables.values() {
        *variables_by_kind.entry(variable_kind(variable.kind)).or_insert(0) += 1;
    }
    let mut constraints_by_kind = std::collections::BTreeMap::new();
    let mut nonzeros = 0;
    let mut quadratic_constraint_terms = 0;
    for constraint in problem.constraints.values() {
        *constraints_by_kind.entry(constraint_kind(constraint)).or_insert(0) += 1;
        nonzeros += match constraint {
            Constraint::Standard { coefficients, .. }
            | Constraint::Indicator { coefficients, .. }
            | Constraint::Quadratic { coefficients, .. } => coefficients.len(),
            Constraint::SOS { weights, .. } => weights.len(),
            Constraint::General { .. } => 0,
        };
        if let Constraint::Quadratic { quadratic, .. } = constraint {
            quadratic_constraint_terms += quadratic.len();
        }
    }
    cancel.check()?;
    let diagnostics = Analysis::new(source, settings, cancel)?;
    let (constraints, variables) = (problem.constraints.len(), problem.variables.len());
    #[expect(clippy::cast_precision_loss, reason = "counts far below 2^53")]
    let density = if constraints == 0 || variables == 0 { 0.0 } else { nonzeros as f64 / (constraints as f64 * variables as f64) };
    Ok(Summary {
        name: problem.name.clone(),
        sense: problem.sense.to_string(),
        objectives: problem.objectives.len(),
        constraints,
        variables,
        nonzeros,
        density,
        quadratic_objective_terms: problem.objectives.values().map(|o| o.quadratic.len()).sum(),
        quadratic_constraint_terms,
        variables_by_kind,
        constraints_by_kind,
        diagnostics: DiagnosticCounts {
            error: diagnostics.count(IssueSeverity::Error),
            warning: diagnostics.count(IssueSeverity::Warning),
            info: diagnostics.count(IssueSeverity::Info),
        },
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::super::testing::{MODEL, doc, json, target};
    use super::*;

    fn params() -> Params {
        Params { target: target(), include: None, offset: 0, limit: DEFAULT_LIMIT }
    }

    fn run(text: &str, params: &Params) -> serde_json::Value {
        json(&handle(&doc(text), Some(1), params, &AnalysisSettings::default(), &Cancel::new()).expect("model"))
    }

    #[test]
    fn describes_every_section() {
        let result = run(MODEL, &params());
        assert_eq!(result["uri"], "file:///t.lp");
        assert_eq!(result["version"], 1);
        let summary = &result["summary"];
        assert_eq!(summary["sense"], "Minimize");
        assert_eq!(
            (summary["objectives"].clone(), summary["constraints"].clone(), summary["variables"].clone()),
            (json!(1), json!(3), json!(3))
        );
        assert_eq!(summary["nonzeros"], 7);
        assert_eq!(summary["variablesByKind"], json!({ "continuous": 2, "general": 1 }));
        assert_eq!(summary["constraintsByKind"], json!({ "linear": 3 }));
        assert!(summary["diagnostics"]["error"].is_u64());

        let obj = &result["objectives"][0];
        assert_eq!((obj["name"].clone(), obj["terms"].clone()), (json!("obj"), json!(3)));
        assert!(obj["range"]["start"].is_object() && obj["selectionRange"]["start"].is_object());
        assert!(obj.get("attributes").is_none(), "attributes omitted when empty");

        let c2 = &result["constraints"][1];
        assert_eq!(c2["name"], "c2");
        assert_eq!(
            (c2["kind"].clone(), c2["class"].clone(), c2["operator"].clone(), c2["rhs"].clone()),
            (json!("linear"), json!("normal"), json!("<="), json!(100.0))
        );
        assert_eq!((c2["generated"].clone(), c2["rangePartner"].clone()), (json!(false), json!(null)));
        assert_eq!(c2["selectionRange"]["start"]["line"], 4);

        let x = &result["variables"][0];
        assert_eq!(x["name"], "x");
        assert_eq!(x["bounds"], json!({ "lower": 0.0, "upper": 10.0, "declaredLower": false, "declaredUpper": true }));
        assert_eq!(x["usage"], json!({ "objectives": 1, "constraints": 3, "quadratic": 0 }));
        assert_eq!((x["unused"].clone(), x["free"].clone()), (json!(false), json!(false)));
        assert_eq!(result["truncated"], json!({ "objectives": false, "constraints": false, "variables": false }));
    }

    #[test]
    fn descriptions_come_from_comments_above_entries() {
        let text = "Minimize\n \\ @description Total cost\n obj: x + y\nSubject To\n \\ @description Plant cap\n c1: x + y <= 4\n c2: x >= 1\nBounds\n \\ @description Width\n x <= 4\nEnd\n";
        let result = run(text, &params());
        assert_eq!(result["objectives"][0]["description"], "Total cost");
        assert_eq!(result["constraints"][0]["description"], "Plant cap");
        assert!(result["constraints"][1].get("description").is_none());
        assert_eq!(result["variables"][0]["description"], "Width");
        assert!(result["variables"][1].get("description").is_none());
    }

    #[test]
    fn unbounded_sides_are_null_and_binary_is_clamped() {
        let result = run("Minimize\n obj: a + b + c\nSubject To\n r: a + b + c >= 1\nBounds\n b free\nBinaries\n c\nEnd\n", &params());
        let vars = &result["variables"];
        assert_eq!(vars[0]["bounds"], json!({ "lower": 0.0, "upper": null, "declaredLower": false, "declaredUpper": false }));
        assert_eq!(vars[1]["bounds"]["lower"], json!(null), "free: lower is -inf");
        assert_eq!(vars[1]["free"], true);
        assert_eq!(vars[2]["bounds"], json!({ "lower": 0.0, "upper": 1.0, "declaredLower": false, "declaredUpper": false }));
        assert_eq!(vars[2]["kind"], "binary");
    }

    #[test]
    fn ranged_constraints_name_their_partner_and_generated_names_are_flagged() {
        let result = run("Minimize\n obj: x + y\nSubject To\n 2 <= x + y <= 8\n r: x - y >= -1\nEnd\n", &params());
        let constraints = result["constraints"].as_array().expect("constraints");
        assert_eq!(constraints.len(), 3);
        assert_eq!(
            (constraints[0]["generated"].clone(), constraints[1]["generated"].clone(), constraints[2]["generated"].clone()),
            (json!(true), json!(true), json!(false))
        );
        assert_eq!(constraints[0]["rangePartner"], constraints[1]["name"]);
        assert_eq!(constraints[1]["rangePartner"], constraints[0]["name"]);
        assert_eq!(constraints[2]["rangePartner"], json!(null));
        assert_eq!(constraints[0]["range"], constraints[1]["range"], "both halves are written in one place");
    }

    #[test]
    fn special_constraints_have_their_kind_and_no_relation() {
        let text = "Minimize\n obj: x + y\nSubject To\n q: x + [ x ^ 2 ] <= 4\n ind: b = 1 -> x + y <= 3\nBounds\n x <= 5\nBinaries\n b\nSOS\n s1: S1:: x:1 y:2\n s2: S2:: x:1 y:2 b:3\nEnd\n";
        let result = run(text, &params());
        let kinds: Vec<_> =
            result["constraints"].as_array().expect("constraints").iter().map(|c| c["kind"].as_str().unwrap_or("").to_owned()).collect();
        assert_eq!(kinds, ["quadratic", "indicator", "sos1", "sos2"]);
        let sos = &result["constraints"][2];
        assert!(sos.get("operator").is_none() && sos.get("rhs").is_none(), "SOS has no operator or rhs: {sos}");
        assert_eq!(result["summary"]["quadraticConstraintTerms"], 1);
        assert_eq!(result["variables"][0]["usage"]["quadratic"], 1);
    }

    #[test]
    fn lazy_constraints_and_objective_attributes() {
        let text = "Maximize multi-objectives\n obj: Priority=2 Weight=0.5\n  x + y\n plain:\n  x - y\nSubject To\n c1: x + y <= 4\nLazy Constraints\n lz: x - y <= 1\nEnd\n";
        let result = run(text, &params());
        assert_eq!(result["objectives"][0]["attributes"], json!({ "priority": 2, "weight": 0.5 }));
        assert!(result["objectives"][1].get("attributes").is_none());
        assert_eq!(result["constraints"][1]["class"], "lazy");
    }

    #[test]
    fn unused_variables_are_flagged() {
        let result = run("Minimize\n obj: x\nSubject To\n c1: x >= 1\nBounds\n w <= 3\nEnd\n", &params());
        let w = result["variables"].as_array().expect("variables").iter().find(|v| v["name"] == "w").expect("w").clone();
        assert_eq!(w["unused"], true);
        assert_eq!(w["usage"], json!({ "objectives": 0, "constraints": 0, "quadratic": 0 }));
    }

    #[test]
    fn include_selects_sections_and_pages_truncate() {
        let mut p = params();
        p.include = Some(vec![Section::Constraints]);
        p.limit = 2;
        p.offset = 1;
        let result = run(MODEL, &p);
        assert!(result.get("summary").is_none() && result.get("objectives").is_none() && result.get("variables").is_none());
        let names: Vec<_> = result["constraints"].as_array().expect("constraints").iter().map(|c| c["name"].clone()).collect();
        assert_eq!(names, [json!("c2"), json!("c3")]);
        assert_eq!(result["truncated"]["constraints"], false, "the page reaches the end");
        p.offset = 0;
        p.limit = 1;
        assert_eq!(run(MODEL, &p)["truncated"]["constraints"], true);
        p.offset = 99;
        let beyond = run(MODEL, &p);
        assert_eq!(beyond["constraints"], json!([]));
        assert_eq!(beyond["truncated"]["constraints"], false);
    }

    #[test]
    fn summary_diagnostics_follow_the_analysis_settings() {
        let text = "Minimize\n obj: x + y\nSubject To\n c1: x + y >= 2\n c2: 2 x + 2 y >= 4\nEnd\n";
        let total = |settings: &AnalysisSettings| {
            let result = json(&handle(&doc(text), Some(1), &params(), settings, &Cancel::new()).expect("model"));
            let d = &result["summary"]["diagnostics"];
            ["error", "warning", "info"].iter().map(|k| d[*k].as_u64().expect("count")).sum::<u64>()
        };
        let default = total(&AnalysisSettings::default());
        assert!(default > 0, "the proportional rows are reported by default");
        let off: AnalysisSettings =
            serde_json::from_value(json!({ "severityOverrides": { "proportional-row": "off" } })).expect("settings");
        assert!(total(&off) < default, "an `off` override drops the finding from the counts");
        let disabled: AnalysisSettings = serde_json::from_value(json!({ "enabled": false })).expect("settings");
        assert_eq!(total(&disabled), 0);
    }

    #[test]
    fn rejects_bad_pages_parse_errors_and_cancellation() {
        let mut p = params();
        p.limit = MAX_LIMIT + 1;
        assert!(matches!(
            handle(&doc(MODEL), Some(1), &p, &AnalysisSettings::default(), &Cancel::new()),
            Err(RequestError::InvalidParams(_))
        ));
        p.limit = 0;
        assert!(matches!(
            handle(&doc(MODEL), Some(1), &p, &AnalysisSettings::default(), &Cancel::new()),
            Err(RequestError::InvalidParams(_))
        ));
        assert!(matches!(
            handle(
                &doc("Minimize\n obj: 3 x +\nSubject To\n c1: >= \nEnd\n"),
                Some(1),
                &params(),
                &AnalysisSettings::default(),
                &Cancel::new()
            ),
            Err(RequestError::Failed(_))
        ));
        let cancel = Cancel::new();
        cancel.cancel();
        assert_eq!(handle(&doc(MODEL), Some(1), &params(), &AnalysisSettings::default(), &cancel).err(), Some(RequestError::Cancelled));
    }

    #[test]
    fn unopened_files_report_a_null_version() {
        let result = handle(&doc(MODEL), None, &params(), &AnalysisSettings::default(), &Cancel::new()).expect("model");
        assert_eq!(json(&result)["version"], json!(null));
    }
}
