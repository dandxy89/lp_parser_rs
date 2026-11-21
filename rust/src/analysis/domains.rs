//! Variable-domain, objective, SOS and indicator checks.

// Bounds and weights are compared exactly on purpose: a declared bound either
// equals another value or it does not.
#![allow(clippy::float_cmp)]

use rustc_hash::FxHashMap;

use super::diagnostics::{Ctx, DiagnosticCode, DiagnosticSubject, RelatedSubject, Sink, SubjectSite, fmt_num};
use super::rows::{cluster, merge_terms, normalise};
use crate::interner::NameId;
use crate::model::{ComparisonOp, Constraint, SOSType, Sense, Variable, VariableKind};

/// Slack for treating a bound as an integer: `equality_tolerance * max(1, |b|)`.
fn int_slack(ctx: &Ctx<'_>, bound: f64) -> f64 {
    ctx.config.equality_tolerance * bound.abs().max(1.0)
}

/// Whether the integer interval `[ceil(l - e), floor(u + e)]` is empty.
fn no_integer_between(ctx: &Ctx<'_>, lower: f64, upper: f64) -> bool {
    if !lower.is_finite() || !upper.is_finite() {
        return false;
    }
    (lower - int_slack(ctx, lower)).ceil() > (upper + int_slack(ctx, upper)).floor()
}

/// The values a variable can take, as used for activity bounds.
///
/// Semi-continuous and semi-integer variables may also be zero, so their domain
/// is `[min(0, l), max(0, u)]`. Integer, general and binary variables round
/// inwards. The default lower bound of zero and binary clamping come from
/// [`VariableBounds::effective_lower`](crate::model::VariableBounds::effective_lower).
pub(super) fn activity_domain(ctx: &Ctx<'_>, variable: &Variable) -> (f64, f64) {
    let lower = variable.bounds.effective_lower(variable.kind);
    let upper = variable.bounds.effective_upper(variable.kind);
    match variable.kind {
        VariableKind::SemiContinuous | VariableKind::SemiInteger => (lower.min(0.0), upper.max(0.0)),
        VariableKind::Integer | VariableKind::General | VariableKind::Binary => {
            let lower = if lower.is_finite() { (lower - int_slack(ctx, lower)).ceil() } else { lower };
            let upper = if upper.is_finite() { (upper + int_slack(ctx, upper)).floor() } else { upper };
            (lower, upper)
        }
        VariableKind::Continuous | VariableKind::Sos => (lower, upper),
    }
}

/// How each variable is used, indexed like `problem.variables`.
#[derive(Default, Clone)]
#[expect(clippy::struct_excessive_bools, reason = "independent usage flags")]
struct Usage {
    in_objective_linear: bool,
    in_objective_quadratic: bool,
    in_constraint: bool,
    /// Occurs somewhere that stops the unbounded-ray argument.
    blocked: bool,
    /// Ordinary linear rows containing the variable with a non-zero merged
    /// weight: the row operator and the variable's summed coefficient.
    rows: Vec<(ComparisonOp, f64)>,
}

fn usage(ctx: &Ctx<'_>) -> Vec<Usage> {
    let problem = ctx.problem;
    let mut usage = vec![Usage::default(); problem.variables.len()];
    let slot = |id| problem.variables.get_index_of(&id);
    for objective in problem.objectives.values() {
        for term in &objective.coefficients {
            if let Some(i) = slot(term.name) {
                usage[i].in_objective_linear = true;
            }
        }
        for term in &objective.quadratic {
            for id in [term.var1, term.var2] {
                if let Some(i) = slot(id) {
                    usage[i].in_objective_quadratic = true;
                }
            }
        }
    }
    for (id, constraint) in &problem.constraints {
        let ordinary = matches!(constraint, Constraint::Standard { .. }) && problem.constraint_class(*id).is_normal();
        constraint.for_each_variable(|var| {
            if let Some(i) = slot(var) {
                usage[i].in_constraint = true;
                if !ordinary {
                    usage[i].blocked = true;
                }
            }
        });
        // Merge each ordinary row once so the ray test reads weights directly.
        if let (true, Constraint::Standard { coefficients, operator, .. }) = (ordinary, constraint) {
            match merge_terms(coefficients) {
                Some(terms) => {
                    for (var, weight) in terms {
                        if let Some(i) = slot(var) {
                            usage[i].rows.push((*operator, weight));
                        }
                    }
                }
                None => constraint.for_each_variable(|var| {
                    if let Some(i) = slot(var) {
                        usage[i].blocked = true;
                    }
                }),
            }
        }
    }
    usage
}

/// Merged linear costs indexed by [`NameId::index`], when the objective role
/// checks apply: exactly one objective with finite coefficients.
fn objective_costs(ctx: &Ctx<'_>) -> Option<Vec<f64>> {
    let problem = ctx.problem;
    if problem.objectives.len() != 1 {
        return None;
    }
    let terms = merge_terms(&problem.objectives[0].coefficients)?;
    let mut costs = vec![0.0; problem.interner.len()];
    for (id, cost) in terms {
        costs[id.index()] = cost;
    }
    Some(costs)
}

pub(super) fn run(ctx: &Ctx<'_>, sink: &mut Sink) {
    let usage = usage(ctx);
    let costs = objective_costs(ctx);
    for (index, (id, variable)) in ctx.problem.variables.iter().enumerate() {
        check_variable(ctx, sink, ctx.name(*id), variable, &usage[index]);
        if let Some(costs) = &costs {
            check_objective_role(ctx, sink, *id, variable, &usage[index], costs[id.index()]);
        }
    }
    check_objectives(ctx, sink);
    check_sos_and_indicators(ctx, sink);
}

#[expect(clippy::too_many_lines, reason = "one arm per bound check keeps the table readable")]
fn check_variable(ctx: &Ctx<'_>, sink: &mut Sink, name: &str, variable: &Variable, usage: &Usage) {
    debug_assert!(!name.is_empty(), "variable name must not be empty");
    let kind = variable.kind;
    let bounds = variable.bounds;
    let tolerance = ctx.config.feasibility_tolerance;
    let at_bound = || DiagnosticSubject::variable(name, SubjectSite::Bound);

    // 1. invalid-bounds
    let invalid = match (bounds.lower, bounds.upper) {
        (Some(lower), Some(upper)) => {
            lower > upper && (!(lower - upper).is_finite() || lower - upper > tolerance * lower.abs().max(upper.abs()).max(1.0))
        }
        _ => false,
    };
    if let (true, Some(lower), Some(upper)) = (invalid, bounds.lower, bounds.upper) {
        sink.push(
            DiagnosticCode::InvalidBounds,
            format!("Variable '{name}' has lower bound {} above its upper bound {}", fmt_num(lower), fmt_num(upper)),
            Some("No value satisfies both bounds, so the model is infeasible. Swap or correct the bounds."),
            Some(at_bound()),
            Vec::new(),
        );
    }

    let lower = bounds.effective_lower(kind);
    let upper = bounds.effective_upper(kind);

    // 2. negative-upper-default-lower
    let negative_default_lower = matches!((bounds.lower, bounds.upper), (None, Some(upper_declared)) if upper_declared < 0.0)
        && !matches!(kind, VariableKind::Binary | VariableKind::SemiContinuous | VariableKind::SemiInteger);
    if let (true, Some(upper_declared)) = (negative_default_lower, bounds.upper) {
        sink.push(
            DiagnosticCode::NegativeUpperDefaultLower,
            format!(
                "Variable '{name}' has upper bound {} but no lower bound, so the default lower bound of 0 leaves no feasible value",
                fmt_num(upper_declared)
            ),
            Some("LP files give an undeclared lower bound the value 0. Declare it explicitly, for example `-inf <= x <= -5`."),
            Some(at_bound()),
            Vec::new(),
        );
    }

    // 3. integer-no-feasible-values
    let integer_like = matches!(kind, VariableKind::Integer | VariableKind::General | VariableKind::Binary);
    // Dialects disagree on the implied lower bound, so the heuristic finding #2
    // is the only one reported when the emptiness comes from that default.
    let integer_empty = integer_like && !invalid && !negative_default_lower && no_integer_between(ctx, lower, upper);
    if integer_empty {
        sink.push(
            DiagnosticCode::IntegerNoFeasibleValues,
            format!("Integer variable '{name}' has no integer between its bounds {} and {}", fmt_num(lower), fmt_num(upper)),
            Some("The model is infeasible. Widen the bounds so they contain an integer."),
            Some(at_bound()),
            Vec::new(),
        );
    }

    // 4. integer-fractional-bound
    if matches!(kind, VariableKind::Integer | VariableKind::General) && !integer_empty {
        let fractional = |bound: f64| bound.is_finite() && (bound - bound.round()).abs() > int_slack(ctx, bound);
        let mut parts = Vec::new();
        if let Some(declared) = bounds.lower.filter(|b| fractional(*b)) {
            parts.push(format!("lower bound {} acts as {}", fmt_num(declared), fmt_num(declared.ceil())));
        }
        if let Some(declared) = bounds.upper.filter(|b| fractional(*b)) {
            parts.push(format!("upper bound {} acts as {}", fmt_num(declared), fmt_num(declared.floor())));
        }
        if !parts.is_empty() {
            sink.push(
                DiagnosticCode::IntegerFractionalBound,
                format!("Integer variable '{name}': {}", parts.join(" and ")),
                Some("An integer variable can never sit on a fractional bound. Write the rounded bound to say what you mean."),
                Some(at_bound()),
                Vec::new(),
            );
        }
    }

    // 5. semi-integer-only-zero
    if kind == VariableKind::SemiInteger && no_integer_between(ctx, lower, upper) {
        sink.push(
            DiagnosticCode::SemiIntegerOnlyZero,
            format!(
                "Semi-integer variable '{name}' has no integer between {} and {}, so only 0 is feasible",
                fmt_num(lower),
                fmt_num(upper)
            ),
            Some("Widen the bounds if the variable should be able to take a non-zero value."),
            Some(at_bound()),
            Vec::new(),
        );
    }

    if kind == VariableKind::Binary {
        // 6. binary-bound-outside-unit
        let outside = |bound: f64| !(0.0..=1.0).contains(&bound);
        if bounds.lower.is_some_and(outside) || bounds.upper.is_some_and(outside) {
            sink.push(
                DiagnosticCode::BinaryBoundOutsideUnit,
                format!(
                    "Binary variable '{name}' has a declared bound outside [0, 1]; it is clamped to [{}, {}]",
                    fmt_num(lower),
                    fmt_num(upper)
                ),
                Some("A binary variable always lies in [0, 1]. Remove the bound or declare the variable as an integer."),
                Some(at_bound()),
                Vec::new(),
            );
        }
        // 7. binary-redundant-bounds
        if bounds.lower == Some(0.0) || bounds.upper == Some(1.0) {
            sink.push(
                DiagnosticCode::BinaryRedundantBounds,
                format!("Binary variable '{name}' declares bounds that its type already implies"),
                Some("Remove the 0 and 1 bounds; `binary` already means [0, 1]."),
                Some(at_bound()),
                Vec::new(),
            );
        }
    }

    // 8. integer-is-binary
    if matches!(kind, VariableKind::Integer | VariableKind::General) && lower == 0.0 && upper == 1.0 {
        sink.push(
            DiagnosticCode::IntegerIsBinary,
            format!("Integer variable '{name}' is bounded to [0, 1]; declaring it binary says the same"),
            Some("Binary variables are handled specially by solvers and read more clearly."),
            Some(DiagnosticSubject::variable(name, SubjectSite::TypeDeclaration)),
            Vec::new(),
        );
    }

    // 9. semicontinuous-infinite-upper
    if matches!(kind, VariableKind::SemiContinuous | VariableKind::SemiInteger) && upper == f64::INFINITY {
        sink.push(
            DiagnosticCode::SemicontinuousInfiniteUpper,
            format!("Semi-continuous variable '{name}' has no finite upper bound"),
            Some("HiGHS and CPLEX reject a semi-continuous variable without an upper bound. Declare one."),
            Some(at_bound()),
            Vec::new(),
        );
    }

    // 10. fixed-variable
    if let (Some(lower), Some(upper)) = (bounds.lower, bounds.upper)
        && lower.is_finite()
        && lower == upper
    {
        sink.push(
            DiagnosticCode::FixedVariable,
            format!("Variable '{name}' is fixed at {}", fmt_num(lower)),
            Some("A fixed variable is a constant; this is often intentional."),
            Some(at_bound()),
            Vec::new(),
        );
    }

    // 11. unused-variable
    if !usage.in_objective_linear && !usage.in_objective_quadratic && !usage.in_constraint {
        sink.push(
            DiagnosticCode::UnusedVariable,
            format!("Variable '{name}' is declared but appears in no objective or constraint"),
            Some("Check the spelling of the name where it should have been used, or remove the declaration."),
            Some(DiagnosticSubject::variable(name, SubjectSite::Definition)),
            Vec::new(),
        );
    }
}

/// Checks 12 and 13: what a variable that only the objective pulls on will do.
fn check_objective_role(ctx: &Ctx<'_>, sink: &mut Sink, id: NameId, variable: &Variable, usage: &Usage, cost: f64) {
    let problem = ctx.problem;
    let name = ctx.name(id);
    if usage.in_objective_quadratic {
        return;
    }
    if cost == 0.0 {
        return;
    }
    // Direction of improvement.
    let improving_up = match problem.sense {
        Sense::Minimize => cost < 0.0,
        Sense::Maximize => cost > 0.0,
    };
    let (lower, upper) = activity_domain(ctx, variable);
    if lower > upper {
        return;
    }
    let bound = if improving_up { upper } else { lower };
    let side = if improving_up { "upper" } else { "lower" };

    if bound.is_finite() {
        // 12. objective-only-variable
        if !usage.in_constraint {
            sink.push(
                DiagnosticCode::ObjectiveOnlyVariable,
                format!("Variable '{name}' appears only in the objective, so it will sit at its {side} bound {}", fmt_num(bound)),
                Some("No constraint limits this variable; check that it should not be constrained."),
                Some(DiagnosticSubject::variable(name, SubjectSite::Definition)),
                Vec::new(),
            );
        }
        return;
    }

    // 13. no-effective-bound
    if usage.blocked {
        return;
    }
    let direction = if improving_up { 1.0 } else { -1.0 };
    for &(operator, weight) in &usage.rows {
        let relaxes = match operator {
            ComparisonOp::EQ => false,
            ComparisonOp::LTE | ComparisonOp::LT => weight * direction < 0.0,
            ComparisonOp::GTE | ComparisonOp::GT => weight * direction > 0.0,
        };
        if !relaxes {
            return;
        }
    }
    let verb = if improving_up { "increased" } else { "decreased" };
    sink.push(
        DiagnosticCode::NoEffectiveBound,
        format!("Variable '{name}' can be {verb} without limit and improves the objective each time, so the objective is unbounded if the model is feasible"),
        Some(&format!("Nothing bounds '{name}' on its {side} side and every constraint it appears in is relaxed by moving that way. Add a bound or a constraint.")),
        Some(DiagnosticSubject::variable(name, SubjectSite::Definition)),
        Vec::new(),
    );
}

fn check_objectives(ctx: &Ctx<'_>, sink: &mut Sink) {
    let problem = ctx.problem;
    let tolerance = ctx.config.equality_tolerance;
    let mut vectors = Vec::new();

    for (id, objective) in &problem.objectives {
        let name = ctx.name(*id);

        // 20. explicit-zero-coefficient (objective part)
        let zeros: Vec<&str> = objective.coefficients.iter().filter(|t| t.value == 0.0).map(|t| ctx.name(t.name)).collect();
        if !zeros.is_empty() {
            super::rows::explicit_zero(sink, DiagnosticSubject::objective(name), name, &zeros);
        }

        // 14. nonconvex-objective: a negative diagonal entry proves Q is not PSD.
        let mut squares: Vec<(NameId, f64)> = Vec::new();
        let mut square_slot: FxHashMap<NameId, usize> = FxHashMap::default();
        for term in objective.quadratic.iter().filter(|t| t.is_square()) {
            let slot = *square_slot.entry(term.var1).or_insert_with(|| {
                squares.push((term.var1, 0.0));
                squares.len() - 1
            });
            squares[slot].1 += term.coefficient;
        }
        let offending = squares.iter().find(|(_, sum)| match problem.sense {
            Sense::Minimize => *sum < 0.0,
            Sense::Maximize => *sum > 0.0,
        });
        if let Some(&(var, sum)) = offending {
            let var_name = ctx.name(var);
            let (sense, wanted) = match problem.sense {
                Sense::Minimize => ("minimised", "positive"),
                Sense::Maximize => ("maximised", "negative"),
            };
            sink.push(
                DiagnosticCode::NonconvexObjective,
                format!("Objective '{name}' is {sense} but its square term for '{var_name}' has coefficient {}, so it is not convex", fmt_num(sum)),
                Some(&format!("A {sense} quadratic objective needs a {wanted} semi-definite form. Solvers that need convexity will reject this or return a local optimum.")),
                Some(DiagnosticSubject::objective(name)),
                Vec::new(),
            );
        }

        // 15. empty-objective
        let terms = merge_terms(&objective.coefficients);
        let has_linear = terms.as_ref().is_none_or(|t| !t.is_empty());
        if !has_linear && objective.quadratic.is_empty() {
            sink.push(
                DiagnosticCode::EmptyObjective,
                format!("Objective '{name}' has no terms"),
                Some("The model is a pure feasibility problem. Add terms if an optimum is wanted."),
                Some(DiagnosticSubject::objective(name)),
                Vec::new(),
            );
        }

        if objective.quadratic.is_empty()
            && let Some(vector) = terms.and_then(|t| normalise(&t))
        {
            vectors.push((*id, vector, objective.attributes.priority));
        }
    }

    // 16. duplicate-objective / 17. opposed-objectives
    let items: Vec<((), &_)> = vectors.iter().map(|(_, v, _)| ((), v)).collect();
    for group in cluster(&items, tolerance) {
        let (first_id, first, first_priority) = &vectors[group[0]];
        let first_name = ctx.name(*first_id);
        for &member in &group[1..] {
            let (id, vector, priority) = &vectors[member];
            let name = ctx.name(*id);
            let factor = vector.pivot / first.pivot;
            let related = vec![RelatedSubject::new(DiagnosticSubject::objective(first_name), "the objective it matches")];
            if factor > 0.0 {
                sink.push(
                    DiagnosticCode::DuplicateObjective,
                    format!("Objective '{name}' is objective '{first_name}' scaled by {}", fmt_num(factor)),
                    Some("Both objectives pull the same way; one of them adds nothing."),
                    Some(DiagnosticSubject::objective(name)),
                    related,
                );
            } else if priority == first_priority {
                sink.push(
                    DiagnosticCode::OpposedObjectives,
                    format!("Objective '{name}' is objective '{first_name}' scaled by {}, so the two pull in opposite directions", fmt_num(factor)),
                    Some("Objectives of equal priority that oppose each other cannot both be improved. Give them different priorities or drop one."),
                    Some(DiagnosticSubject::objective(name)),
                    related,
                );
            }
        }
    }
}

fn check_sos_and_indicators(ctx: &Ctx<'_>, sink: &mut Sink) {
    let problem = ctx.problem;
    for (id, constraint) in &problem.constraints {
        let name = ctx.name(*id);
        match constraint {
            Constraint::SOS { sos_type, weights, .. } => {
                // 40. sos-duplicate-weights
                let mut order: Vec<usize> = (0..weights.len()).collect();
                order.sort_by(|&a, &b| weights[a].value.total_cmp(&weights[b].value));
                if let Some(pair) = order.windows(2).find(|w| weights[w[0]].value == weights[w[1]].value) {
                    let (a, b) = (ctx.name(weights[pair[0]].name), ctx.name(weights[pair[1]].name));
                    sink.push(
                        DiagnosticCode::SosDuplicateWeights,
                        format!("SOS '{name}' gives '{a}' and '{b}' the same weight {}", fmt_num(weights[pair[0]].value)),
                        Some("The order of members is defined by their weights, so equal weights leave it undefined. CPLEX rejects this."),
                        Some(DiagnosticSubject::constraint(name)),
                        Vec::new(),
                    );
                }
                // 41. sos-trivial
                let limit = match sos_type {
                    SOSType::S1 => 1,
                    SOSType::S2 => 2,
                };
                if weights.len() <= limit {
                    sink.push(
                        DiagnosticCode::SosTrivial,
                        format!("{sos_type} set '{name}' has {} member(s) and is always satisfied", weights.len()),
                        Some("An SOS1 needs at least two members and an SOS2 at least three to restrict anything."),
                        Some(DiagnosticSubject::constraint(name)),
                        Vec::new(),
                    );
                }
            }
            Constraint::Indicator { variable, .. } => {
                // 39. indicator-not-binary
                let Some(indicator) = problem.variables.get(variable) else {
                    continue;
                };
                if indicator.kind != VariableKind::Binary {
                    let site =
                        if indicator.kind == VariableKind::Continuous { SubjectSite::Definition } else { SubjectSite::TypeDeclaration };
                    let var_name = ctx.name(*variable);
                    sink.push(
                        DiagnosticCode::IndicatorNotBinary,
                        format!("Indicator variable '{var_name}' of constraint '{name}' is not binary"),
                        Some("CPLEX, Gurobi and HiGHS require an indicator variable to be binary. Declare it in the Binary section."),
                        Some(DiagnosticSubject::variable(var_name, site)),
                        vec![RelatedSubject::new(DiagnosticSubject::constraint(name), "uses it as an indicator")],
                    );
                }
            }
            Constraint::Standard { .. } | Constraint::Quadratic { .. } | Constraint::General { .. } => {}
        }
    }
}
