//! Property tests for model diagnostics.
//!
//! Two kinds of property:
//! - feasible models (built around a known point) never produce an error
//!   diagnostic, so proven findings have no false positives;
//! - every proven finding about rows or bounds is checked semantically against
//!   sampled integer points of the model.

// Generated bounds and points are small integers, so exact comparison is right.
#![allow(clippy::float_cmp)]

use std::collections::HashMap;

use lp_parser_rs::analysis::IssueSeverity;
use lp_parser_rs::model::{
    Coefficient, ComparisonOp, Constraint, Objective, ObjectiveAttributes, Sense, Variable, VariableBounds, VariableKind,
};
use lp_parser_rs::{DiagnosticCode, LpProblem, NameId};
use proptest::prelude::*;

const MAX_VARS: usize = 5;

#[derive(Debug, Clone)]
struct VarSpec {
    kind: u8,
    lower: Option<i32>,
    upper: Option<i32>,
    /// Feasible-model generator: distance of the bounds from the known point.
    point: i32,
}

#[derive(Debug, Clone)]
struct RowSpec {
    coeffs: [i32; MAX_VARS],
    op: u8,
    rhs: i32,
    slack: i32,
    /// `(earlier row, factor, loosening)`.
    copy_of: Option<(usize, i32, i32)>,
}

#[derive(Debug, Clone)]
struct Spec {
    n: usize,
    vars: Vec<VarSpec>,
    rows: Vec<RowSpec>,
    objective: [i32; MAX_VARS],
    maximise: bool,
    points: Vec<Vec<i32>>,
}

fn spec_strategy() -> impl Strategy<Value = Spec> {
    let var = (0u8..3, prop::option::of(-6i32..=6), prop::option::of(-6i32..=6), 0i32..=5)
        .prop_map(|(kind, lower, upper, point)| VarSpec { kind, lower, upper, point });
    let row = (
        prop::array::uniform5(-3i32..=3),
        0u8..3,
        -10i32..=10,
        0i32..=3,
        prop::option::weighted(0.45, (0usize..8, prop::sample::select(vec![1, 1, 2, -1, -2, 3]), 0i32..=2)),
    )
        .prop_map(|(coeffs, op, rhs, slack, copy_of)| RowSpec { coeffs, op, rhs, slack, copy_of });
    (
        2usize..=MAX_VARS,
        prop::collection::vec(var, MAX_VARS),
        prop::collection::vec(row, 1..8),
        prop::array::uniform5(-3i32..=3),
        any::<bool>(),
        prop::collection::vec(prop::collection::vec(-8i32..=8, MAX_VARS), 40),
    )
        .prop_map(|(n, vars, rows, objective, maximise, points)| Spec { n, vars, rows, objective, maximise, points })
}

fn op_of(code: u8) -> ComparisonOp {
    match code {
        0 => ComparisonOp::LTE,
        1 => ComparisonOp::GTE,
        _ => ComparisonOp::EQ,
    }
}

fn flip(op: ComparisonOp) -> ComparisonOp {
    match op {
        ComparisonOp::LTE => ComparisonOp::GTE,
        ComparisonOp::GTE => ComparisonOp::LTE,
        other => other,
    }
}

struct Built {
    problem: LpProblem,
    ids: Vec<NameId>,
    /// Known feasible point (feasible mode only).
    point: Vec<i32>,
}

/// Build the problem. In `feasible` mode every row is satisfied by `point` and
/// every bound contains it; otherwise right-hand sides and bounds are arbitrary.
fn build(spec: &Spec, feasible: bool) -> Built {
    let mut problem = LpProblem::new();
    let ids: Vec<NameId> = (0..spec.n).map(|i| problem.intern(&format!("x{i}"))).collect();
    let point: Vec<i32> = spec.vars.iter().take(spec.n).map(|v| if v.kind == 2 { v.point % 2 } else { v.point - 2 }).collect();

    for (i, id) in ids.iter().enumerate() {
        let v = &spec.vars[i];
        let kind = match v.kind {
            0 => VariableKind::Continuous,
            1 => VariableKind::Integer,
            _ => VariableKind::Binary,
        };
        let bounds = if feasible {
            if kind == VariableKind::Binary {
                VariableBounds::unspecified()
            } else {
                let x = point[i];
                let lower = match v.lower {
                    Some(d) => Some(f64::from(x - d.abs())),
                    None if x >= 0 => None,
                    None => Some(f64::from(x - 1)),
                };
                VariableBounds { lower, upper: v.upper.map(|d| f64::from(x + d.abs())) }
            }
        } else {
            VariableBounds { lower: v.lower.map(f64::from), upper: v.upper.map(f64::from) }
        };
        problem.add_variable(Variable::new(*id).with_kind(kind).with_bounds(bounds));
    }

    let dot = |coeffs: &[i32; MAX_VARS], p: &[i32]| -> i32 { coeffs.iter().zip(p).map(|(a, x)| a * x).sum() };
    let mut made: Vec<([i32; MAX_VARS], ComparisonOp, i32)> = Vec::new();
    for (k, row) in spec.rows.iter().enumerate() {
        let (mut coeffs, mut op, mut rhs) = (row.coeffs, op_of(row.op), row.rhs);
        for c in coeffs.iter_mut().skip(spec.n) {
            *c = 0;
        }
        if let Some((source, factor, loosen)) = row.copy_of.filter(|(s, _, _)| *s < made.len()) {
            let (c0, op0, rhs0) = made[source];
            coeffs = c0.map(|c| c * factor);
            op = if factor < 0 { flip(op0) } else { op0 };
            rhs = rhs0 * factor;
            match op {
                ComparisonOp::LTE => rhs += loosen,
                ComparisonOp::GTE => rhs -= loosen,
                _ => {}
            }
            if !feasible {
                rhs += loosen - 1;
            }
        } else if feasible {
            let activity = dot(&coeffs, &point);
            rhs = match op {
                ComparisonOp::LTE => activity + row.slack,
                ComparisonOp::GTE => activity - row.slack,
                _ => activity,
            };
        }
        made.push((coeffs, op, rhs));
        let name = problem.intern(&format!("c{k}"));
        let terms: Vec<Coefficient> =
            coeffs.iter().take(spec.n).enumerate().map(|(i, c)| Coefficient { name: ids[i], value: f64::from(*c) }).collect();
        problem.add_constraint(Constraint::Standard { name, coefficients: terms, operator: op, rhs: f64::from(rhs), byte_offset: None });
    }

    let obj = problem.intern("obj");
    let terms = spec
        .objective
        .iter()
        .take(spec.n)
        .enumerate()
        .filter(|(_, c)| **c != 0)
        .map(|(i, c)| Coefficient { name: ids[i], value: f64::from(*c) })
        .collect();
    problem.add_objective(Objective {
        name: obj,
        coefficients: terms,
        constant: 0.0,
        quadratic: Vec::new(),
        attributes: ObjectiveAttributes::default(),
        byte_offset: None,
    });
    problem.sense = if spec.maximise { Sense::Maximize } else { Sense::Minimize };
    Built { problem, ids, point }
}

fn row_holds(constraint: &Constraint, values: &HashMap<NameId, f64>) -> bool {
    let (coefficients, operator, rhs) = constraint.linear_row().expect("standard row");
    let activity: f64 = coefficients.iter().map(|c| c.value * values[&c.name]).sum();
    match operator {
        ComparisonOp::LTE | ComparisonOp::LT => activity <= rhs + 1e-9,
        ComparisonOp::GTE | ComparisonOp::GT => activity >= rhs - 1e-9,
        ComparisonOp::EQ => (activity - rhs).abs() <= 1e-9,
    }
}

fn within_bounds(problem: &LpProblem, values: &HashMap<NameId, f64>) -> bool {
    problem.variables.iter().all(|(id, v)| {
        let x = values[id];
        x >= v.bounds.effective_lower(v.kind) - 1e-9
            && x <= v.bounds.effective_upper(v.kind) + 1e-9
            && (!v.kind.is_integer() || x.fract() == 0.0)
    })
}

/// Integer points inside the variables' effective domains, from the sample.
fn domain_points(built: &Built, spec: &Spec) -> Vec<HashMap<NameId, f64>> {
    let mut out = Vec::new();
    'point: for raw in &spec.points {
        let mut values = HashMap::new();
        for (i, id) in built.ids.iter().enumerate() {
            let v = &built.problem.variables[id];
            let (lo, hi) = (v.bounds.effective_lower(v.kind), v.bounds.effective_upper(v.kind));
            let (lo_i, hi_i) = (if lo.is_finite() { lo.ceil() } else { -8.0 }, if hi.is_finite() { hi.floor() } else { 8.0 });
            if lo_i > hi_i {
                continue 'point;
            }
            values.insert(*id, f64::from(raw[i]).clamp(lo_i, hi_i));
        }
        debug_assert!(within_bounds(&built.problem, &values));
        out.push(values);
    }
    out
}

fn constraint<'a>(problem: &'a LpProblem, name: &str) -> &'a Constraint {
    &problem.constraints[&problem.name_id(name).expect("known name")]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(400))]

    /// A model that has a feasible point never gets an error diagnostic.
    #[test]
    fn feasible_models_have_no_errors(spec in spec_strategy()) {
        let built = build(&spec, true);
        let values: HashMap<NameId, f64> = built.ids.iter().zip(&built.point).map(|(id, x)| (*id, f64::from(*x))).collect();
        prop_assert!(within_bounds(&built.problem, &values), "generator produced an infeasible point");
        prop_assert!(built.problem.constraints.values().all(|c| row_holds(c, &values)));
        let result = built.problem.diagnostics();
        let errors: Vec<_> = result.diagnostics.iter().filter(|d| d.severity == IssueSeverity::Error).collect();
        prop_assert!(errors.is_empty(), "false positive on a feasible model: {errors:?}");
        prop_assert!(result.with_code(DiagnosticCode::NegativeUpperDefaultLower).next().is_none());
    }

    /// `no-effective-bound` is sound: a feasible point extends along a ray that improves the objective.
    #[test]
    fn unbounded_ray_is_real(spec in spec_strategy()) {
        let built = build(&spec, true);
        let result = built.problem.diagnostics();
        for found in result.with_code(DiagnosticCode::NoEffectiveBound) {
            let subject = found.subject.as_ref().unwrap();
            let id = built.problem.name_id(&subject.name).unwrap();
            let cost = built.problem.objectives[0].coefficients.iter().filter(|c| c.name == id).map(|c| c.value).sum::<f64>();
            prop_assert!(cost != 0.0);
            let up = (cost < 0.0) == (built.problem.sense == Sense::Minimize);
            let mut values: HashMap<NameId, f64> = built.ids.iter().zip(&built.point).map(|(id, x)| (*id, f64::from(*x))).collect();
            for steps in [1.0, 10.0, 1000.0] {
                values.insert(id, f64::from(built.point[built.ids.iter().position(|i| *i == id).unwrap()]) + if up { steps } else { -steps });
                prop_assert!(within_bounds(&built.problem, &values), "ray leaves the bounds of {}", subject.name);
                prop_assert!(built.problem.constraints.values().all(|c| row_holds(c, &values)), "ray violates a row for {}", subject.name);
            }
        }
    }

    /// Row findings agree with sampled points, on models that may be infeasible.
    #[test]
    fn row_findings_are_semantically_true(spec in spec_strategy()) {
        let built = build(&spec, false);
        let problem = &built.problem;
        let points = domain_points(&built, &spec);
        let result = problem.diagnostics();

        for d in &result.diagnostics {
            let Some(subject) = &d.subject else { continue };
            match d.code {
                DiagnosticCode::RowInfeasibleByBounds => {
                    let row = constraint(problem, &subject.name);
                    prop_assert!(points.iter().all(|p| !row_holds(row, p)), "{d}: a point satisfies the row");
                }
                DiagnosticCode::RowRedundantByBounds => {
                    let row = constraint(problem, &subject.name);
                    prop_assert!(points.iter().all(|p| row_holds(row, p)), "{d}: a point violates the row");
                }
                DiagnosticCode::EmptyRowInfeasible => {
                    let row = constraint(problem, &subject.name);
                    let zeros: HashMap<NameId, f64> = built.ids.iter().map(|id| (*id, 0.0)).collect();
                    let (coefficients, _, _) = row.linear_row().unwrap();
                    prop_assert!(coefficients.iter().all(|c| c.value == 0.0));
                    prop_assert!(!row_holds(row, &zeros), "{d}: the empty row holds");
                }
                DiagnosticCode::DuplicateRow | DiagnosticCode::ProportionalRow => {
                    let (a, b) = (constraint(problem, &subject.name), constraint(problem, &d.related[0].subject.name));
                    prop_assert!(points.iter().all(|p| row_holds(a, p) == row_holds(b, p)), "{d}: rows disagree");
                }
                DiagnosticCode::DominatedRow => {
                    let (redundant, tighter) = (constraint(problem, &subject.name), constraint(problem, &d.related[0].subject.name));
                    prop_assert!(points.iter().all(|p| !row_holds(tighter, p) || row_holds(redundant, p)), "{d}: not implied");
                }
                DiagnosticCode::ContradictoryRows => {
                    let (a, b) = (constraint(problem, &subject.name), constraint(problem, &d.related[0].subject.name));
                    prop_assert!(points.iter().all(|p| !(row_holds(a, p) && row_holds(b, p))), "{d}: a point satisfies both");
                }
                _ => {}
            }
        }
    }

    /// Variable findings agree with an independent oracle.
    #[test]
    fn variable_findings_match_oracle(spec in spec_strategy()) {
        let built = build(&spec, false);
        let result = built.problem.diagnostics();
        for (id, v) in &built.problem.variables {
            let name = built.problem.resolve(*id);
            let fired = |code| result.with_code(code).any(|d| d.subject.as_ref().is_some_and(|s| s.name == name));
            let invalid = matches!((v.bounds.lower, v.bounds.upper), (Some(l), Some(u)) if l > u);
            prop_assert_eq!(fired(DiagnosticCode::InvalidBounds), invalid);
            let (lo, hi) = (v.bounds.effective_lower(v.kind), v.bounds.effective_upper(v.kind));
            // An undeclared lower bound with a negative upper bound is left to the
            // heuristic `negative-upper-default-lower`, as dialects disagree on it.
            let default_lower_clash = v.bounds.lower.is_none() && v.bounds.upper.is_some_and(|u| u < 0.0) && v.kind != VariableKind::Binary;
            let no_integer = v.kind.is_integer() && !invalid && !default_lower_clash && lo.is_finite() && hi.is_finite() && lo.ceil() > hi.floor();
            prop_assert_eq!(fired(DiagnosticCode::IntegerNoFeasibleValues), no_integer);
            let fixed = matches!((v.bounds.lower, v.bounds.upper), (Some(l), Some(u)) if l == u);
            prop_assert_eq!(fired(DiagnosticCode::FixedVariable), fixed);
        }
    }

    /// Diagnostics are deterministic, sorted and well-formed on any model.
    #[test]
    fn output_is_stable(spec in spec_strategy(), feasible in any::<bool>()) {
        let built = build(&spec, feasible);
        let first = built.problem.diagnostics();
        prop_assert_eq!(&first, &built.problem.diagnostics());
        let rank = |d: &lp_parser_rs::ModelDiagnostic| match d.severity { IssueSeverity::Error => 0, IssueSeverity::Warning => 1, IssueSeverity::Info => 2 };
        prop_assert!(first.diagnostics.windows(2).all(|w| rank(&w[0]) <= rank(&w[1])));
        for d in &first.diagnostics {
            prop_assert_eq!(d.severity, d.code.default_severity());
        }
    }
}

/// Guards against vacuous properties: the generators must actually reach the
/// codes the properties above check.
#[test]
fn generators_reach_the_checked_codes() {
    use proptest::strategy::ValueTree;
    use proptest::test_runner::{Config, TestRunner};
    let mut runner = TestRunner::new(Config { rng_algorithm: proptest::test_runner::RngAlgorithm::ChaCha, ..Config::default() });
    let mut seen: HashMap<DiagnosticCode, usize> = HashMap::new();
    for feasible in [false, true] {
        for _ in 0..1500 {
            let spec = spec_strategy().new_tree(&mut runner).expect("strategy").current();
            for d in &build(&spec, feasible).problem.diagnostics().diagnostics {
                *seen.entry(d.code).or_default() += 1;
            }
        }
    }
    for code in [
        DiagnosticCode::InvalidBounds,
        DiagnosticCode::IntegerNoFeasibleValues,
        DiagnosticCode::EmptyRowInfeasible,
        DiagnosticCode::RowInfeasibleByBounds,
        DiagnosticCode::RowRedundantByBounds,
        DiagnosticCode::DuplicateRow,
        DiagnosticCode::ProportionalRow,
        DiagnosticCode::DominatedRow,
        DiagnosticCode::ContradictoryRows,
        DiagnosticCode::NoEffectiveBound,
        DiagnosticCode::SingletonRow,
    ] {
        assert!(seen.get(&code).copied().unwrap_or(0) >= 5, "generators rarely reach {code}: {seen:?}");
    }
}

/// One generated statement: a plain row or a ranged constraint over `x + y`.
fn statement_strategy() -> impl Strategy<Value = (u8, i32, i32, i32)> {
    (0u8..4, 1i32..=3, -9i32..=9, -9i32..=9)
}

fn statement_text(index: usize, (kind, scale, low, high): (u8, i32, i32, i32)) -> String {
    let expression = if scale == 1 { "x + y".to_owned() } else { format!("{scale} x + {scale} y") };
    let (low, high) = (low.min(high), low.max(high));
    match kind {
        0 => format!(" c{index}: {expression} >= {low}\n"),
        1 => format!(" c{index}: {expression} <= {high}\n"),
        2 => format!(" c{index}: {low} <= {expression} <= {high}\n"),
        _ => format!(" {low} <= {expression} <= {high}\n"),
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    /// `rows-form-range` covers user-written rows only: its subject and related
    /// rows never share a source statement with another row, and at most one
    /// is reported per group of identical terms.
    #[test]
    fn rows_form_range_never_names_a_ranged_half(statements in prop::collection::vec(statement_strategy(), 1..6)) {
        let mut text = String::from("Minimize\n obj: x + y\nSubject To\n");
        for (index, statement) in statements.iter().enumerate() {
            text.push_str(&statement_text(index, *statement));
        }
        text.push_str("End\n");
        let problem = LpProblem::parse(&text).expect("generated model parses");
        let shares_offset = |name: &str| {
            let id = problem.name_id(name).expect("known name");
            let offset = problem.constraints[&id].byte_offset();
            offset.is_some() && problem.constraints.values().filter(|c| c.byte_offset() == offset).count() > 1
        };
        let diagnostics = problem.diagnostics();
        let found: Vec<_> = diagnostics.with_code(DiagnosticCode::RowsFormRange).collect();
        for diagnostic in &found {
            let subject = diagnostic.subject.as_ref().expect("subject");
            prop_assert!(!shares_offset(&subject.name), "{text}\n{diagnostic}");
            for related in &diagnostic.related {
                prop_assert!(!shares_offset(&related.subject.name), "{text}\n{diagnostic}");
            }
        }
        // All statements use the same terms up to scale, so at most one group exists.
        prop_assert!(found.len() <= 1, "{text}\n{found:?}");
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    /// Rows whose coefficients differ by a hair are different rows: free `x`
    /// and `y` with two non-parallel rows are always feasible, so no proven
    /// row-pair finding may appear however the right-hand sides are chosen.
    #[test]
    fn near_parallel_rows_never_contradict(
        eps in prop::sample::select(vec![1e-12, 1e-11, 1e-10, 1e-9]),
        low in -20i32..=20,
        high in -20i32..=20,
        op1 in 0u8..2,
        op2 in 0u8..2,
    ) {
        let sign = |op: u8| if op == 0 { "<=" } else { ">=" };
        let text = format!(
            "Minimize\n obj: x + y\nSubject To\n c1: x + {} y {} {low}\n c2: x + y {} {high}\nBounds\n x free\n y free\nEnd\n",
            1.0 + eps,
            sign(op1),
            sign(op2)
        );
        let problem = LpProblem::parse(&text).expect("generated model parses");
        let diagnostics = problem.diagnostics();
        for code in [DiagnosticCode::ContradictoryRows, DiagnosticCode::DuplicateRow, DiagnosticCode::ProportionalRow, DiagnosticCode::DominatedRow] {
            prop_assert!(diagnostics.with_code(code).next().is_none(), "{text}\n{code:?}");
        }
    }
}
