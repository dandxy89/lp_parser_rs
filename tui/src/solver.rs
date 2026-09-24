//! `HiGHS` solver integration — converts an `LpProblem` to a `HiGHS` problem and solves it.

use std::collections::HashMap;
use std::error::Error;
use std::fmt::Write as _;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use lp_parser_rs::interner::NameId;
use lp_parser_rs::model::{ComparisonOp, Constraint, Variable, VariableKind};
use lp_parser_rs::problem::LpProblem;

/// Result returned after a successful solve.
#[derive(Debug, Clone)]
pub struct SolveResult {
    /// `HiGHS` model status as a string (e.g. "Optimal", "Infeasible").
    pub status: String,
    /// Objective function value (if a solution exists).
    pub objective_value: Option<f64>,
    /// Variable values in deterministic order.
    pub variables: Vec<(String, f64)>,
    /// Reduced costs (dual column values) per variable.
    pub reduced_costs: Vec<(String, f64)>,
    /// Shadow prices (dual row values) per constraint.
    pub shadow_prices: Vec<(String, f64)>,
    /// Row activity values per constraint.
    pub row_values: Vec<(String, f64)>,
    /// Wall-clock time to build the `HiGHS` `RowProblem` from `LpProblem`.
    pub build_time: std::time::Duration,
    /// Wall-clock solve time.
    pub solve_time: std::time::Duration,
    /// Wall-clock time to extract the solution into `SolveResult`.
    pub extract_time: std::time::Duration,
    /// Captured solver log output (presolve info, iteration counts, etc.).
    pub solver_log: String,
    /// Number of SOS constraints that were skipped (not supported by `RowProblem`).
    pub skipped_sos: usize,
}

/// Pre-computed diff counts, avoiding per-frame iteration.
#[derive(Debug, Clone, Copy)]
pub struct DiffCounts {
    pub added: usize,
    pub removed: usize,
    pub modified: usize,
}

/// Comparison of two solve results.
#[derive(Debug, Clone)]
pub struct SolveDiffResult {
    pub file1_label: String,
    pub file2_label: String,
    pub result1: SolveResult,
    pub result2: SolveResult,
    pub variable_diff: Vec<VarDiffRow>,
    pub constraint_diff: Vec<ConstraintDiffRow>,
    /// Pre-computed variable diff counts (computed once in `diff_results`).
    pub variable_counts: DiffCounts,
    /// Pre-computed constraint diff counts (computed once in `diff_results`).
    pub constraint_counts: DiffCounts,
    /// Wall-clock time to compute the diff between the two results.
    pub diff_time: std::time::Duration,
}

/// Zero-copy name reference into a [`SolveResult`]'s variable or row-value vec,
/// avoiding per-row `String` clones during diff construction.
#[derive(Debug, Clone, Copy)]
pub struct NameRef {
    /// `false` → name lives in result1, `true` → result2.
    pub from_result2: bool,
    /// Index into the source result's `variables` (for [`VarDiffRow`]) or
    /// `row_values` (for [`ConstraintDiffRow`]) vec.
    pub index: u32,
}

/// A single variable row in a solve diff comparison.
#[derive(Debug, Clone)]
pub struct VarDiffRow {
    pub name_ref: NameRef,
    pub val1: Option<f64>,
    pub val2: Option<f64>,
    pub reduced_cost1: Option<f64>,
    pub reduced_cost2: Option<f64>,
    pub changed: bool,
}

impl VarDiffRow {
    /// Resolve the variable name from the source solve results.
    pub fn name<'a>(&self, r1: &'a SolveResult, r2: &'a SolveResult) -> &'a str {
        let (result, idx) =
            if self.name_ref.from_result2 { (r2, self.name_ref.index as usize) } else { (r1, self.name_ref.index as usize) };
        debug_assert!(idx < result.variables.len(), "VarDiffRow name_ref index {idx} out of bounds");
        &result.variables[idx].0
    }
}

/// A single constraint row in a solve diff comparison.
#[derive(Debug, Clone)]
pub struct ConstraintDiffRow {
    pub name_ref: NameRef,
    pub activity1: Option<f64>,
    pub activity2: Option<f64>,
    pub shadow_price1: Option<f64>,
    pub shadow_price2: Option<f64>,
    pub changed: bool,
}

impl ConstraintDiffRow {
    /// Resolve the constraint name from the source solve results.
    pub fn name<'a>(&self, r1: &'a SolveResult, r2: &'a SolveResult) -> &'a str {
        let (result, idx) =
            if self.name_ref.from_result2 { (r2, self.name_ref.index as usize) } else { (r1, self.name_ref.index as usize) };
        debug_assert!(idx < result.row_values.len(), "ConstraintDiffRow name_ref index {idx} out of bounds");
        &result.row_values[idx].0
    }
}

/// Build a `SolveDiffResult` by comparing two solve results.
///
/// Variables and constraints are matched by name. Rows present in only one result
/// are included with `None` on the other side and marked as changed.
pub fn diff_results(
    file1_label: String,
    file2_label: String,
    result1: SolveResult,
    result2: SolveResult,
    threshold: f64,
) -> SolveDiffResult {
    debug_assert!(threshold >= 0.0, "diff threshold must be non-negative, got {threshold}");
    let variable_diff = diff_variables(&result1, &result2, threshold);
    let constraint_diff = diff_constraints(&result1, &result2, threshold);
    let variable_counts = count_var_diffs(&variable_diff);
    let constraint_counts = count_constraint_diffs_from_rows(&constraint_diff);
    SolveDiffResult {
        file1_label,
        file2_label,
        result1,
        result2,
        variable_diff,
        constraint_diff,
        variable_counts,
        constraint_counts,
        diff_time: std::time::Duration::ZERO, // filled in by caller when timed externally
    }
}

/// Count variable-level diff statistics in a single pass.
fn count_var_diffs(rows: &[VarDiffRow]) -> DiffCounts {
    let mut counts = DiffCounts { added: 0, removed: 0, modified: 0 };
    for row in rows {
        if row.val1.is_none() {
            counts.added += 1;
        } else if row.val2.is_none() {
            counts.removed += 1;
        } else if row.changed {
            counts.modified += 1;
        }
    }
    counts
}

/// Count constraint-level diff statistics in a single pass.
fn count_constraint_diffs_from_rows(rows: &[ConstraintDiffRow]) -> DiffCounts {
    let mut counts = DiffCounts { added: 0, removed: 0, modified: 0 };
    for row in rows {
        if row.activity1.is_none() {
            counts.added += 1;
        } else if row.activity2.is_none() {
            counts.removed += 1;
        } else if row.changed {
            counts.modified += 1;
        }
    }
    counts
}

// NameRef indices fit u32: solver column counts are far below 4 billion.
#[allow(clippy::cast_possible_truncation)]
fn diff_variables(r1: &SolveResult, r2: &SolveResult, threshold: f64) -> Vec<VarDiffRow> {
    debug_assert_eq!(r1.variables.len(), r1.reduced_costs.len(), "variables and reduced_costs must have equal length for result 1");
    debug_assert_eq!(r2.variables.len(), r2.reduced_costs.len(), "variables and reduced_costs must have equal length for result 2");

    let mut i: usize = 0;
    let mut j: usize = 0;
    let mut rows = Vec::with_capacity(r1.variables.len().max(r2.variables.len()));

    while i < r1.variables.len() || j < r2.variables.len() {
        let cmp = match (r1.variables.get(i), r2.variables.get(j)) {
            (Some((n1, _)), Some((n2, _))) => n1.cmp(n2),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => break,
        };

        let row = match cmp {
            std::cmp::Ordering::Less => {
                let val = r1.variables[i].1;
                let rc1 = r1.reduced_costs.get(i).map(|(_, v)| *v);
                let name_ref = NameRef { from_result2: false, index: i as u32 };
                i += 1;
                VarDiffRow { name_ref, val1: Some(val), val2: None, reduced_cost1: rc1, reduced_cost2: None, changed: true }
            }
            std::cmp::Ordering::Greater => {
                let val = r2.variables[j].1;
                let rc2 = r2.reduced_costs.get(j).map(|(_, v)| *v);
                let name_ref = NameRef { from_result2: true, index: j as u32 };
                j += 1;
                VarDiffRow { name_ref, val1: None, val2: Some(val), reduced_cost1: None, reduced_cost2: rc2, changed: true }
            }
            std::cmp::Ordering::Equal => {
                let val1 = r1.variables[i].1;
                let val2 = r2.variables[j].1;
                let rc1 = r1.reduced_costs.get(i).map(|(_, v)| *v);
                let rc2 = r2.reduced_costs.get(j).map(|(_, v)| *v);
                let changed = (val1 - val2).abs() > threshold || opt_diff(rc1, rc2, threshold);
                let name_ref = NameRef { from_result2: false, index: i as u32 };
                i += 1;
                j += 1;
                VarDiffRow { name_ref, val1: Some(val1), val2: Some(val2), reduced_cost1: rc1, reduced_cost2: rc2, changed }
            }
        };
        rows.push(row);
    }
    rows
}

// NameRef indices fit u32: solver row counts are far below 4 billion.
#[allow(clippy::cast_possible_truncation)]
fn diff_constraints(r1: &SolveResult, r2: &SolveResult, threshold: f64) -> Vec<ConstraintDiffRow> {
    debug_assert_eq!(r1.row_values.len(), r1.shadow_prices.len(), "row_values and shadow_prices must have equal length for result 1");
    debug_assert_eq!(r2.row_values.len(), r2.shadow_prices.len(), "row_values and shadow_prices must have equal length for result 2");

    let mut i: usize = 0;
    let mut j: usize = 0;
    let mut rows = Vec::with_capacity(r1.row_values.len().max(r2.row_values.len()));

    while i < r1.row_values.len() || j < r2.row_values.len() {
        let cmp = match (r1.row_values.get(i), r2.row_values.get(j)) {
            (Some((n1, _)), Some((n2, _))) => n1.cmp(n2),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => break,
        };

        let row = match cmp {
            std::cmp::Ordering::Less => {
                let activity = r1.row_values[i].1;
                let sp = r1.shadow_prices[i].1;
                let name_ref = NameRef { from_result2: false, index: i as u32 };
                i += 1;
                ConstraintDiffRow {
                    name_ref,
                    activity1: Some(activity),
                    activity2: None,
                    shadow_price1: Some(sp),
                    shadow_price2: None,
                    changed: true,
                }
            }
            std::cmp::Ordering::Greater => {
                let activity = r2.row_values[j].1;
                let sp = r2.shadow_prices[j].1;
                let name_ref = NameRef { from_result2: true, index: j as u32 };
                j += 1;
                ConstraintDiffRow {
                    name_ref,
                    activity1: None,
                    activity2: Some(activity),
                    shadow_price1: None,
                    shadow_price2: Some(sp),
                    changed: true,
                }
            }
            std::cmp::Ordering::Equal => {
                let a1 = r1.row_values[i].1;
                let a2 = r2.row_values[j].1;
                let sp1 = r1.shadow_prices[i].1;
                let sp2 = r2.shadow_prices[j].1;
                let changed = (a1 - a2).abs() > threshold || (sp1 - sp2).abs() > threshold;
                let name_ref = NameRef { from_result2: false, index: i as u32 };
                i += 1;
                j += 1;
                ConstraintDiffRow {
                    name_ref,
                    activity1: Some(a1),
                    activity2: Some(a2),
                    shadow_price1: Some(sp1),
                    shadow_price2: Some(sp2),
                    changed,
                }
            }
        };
        rows.push(row);
    }
    rows
}

/// Return `true` if two optional f64 values differ beyond the given threshold.
fn opt_diff(a: Option<f64>, b: Option<f64>, threshold: f64) -> bool {
    match (a, b) {
        (Some(x), Some(y)) => (x - y).abs() > threshold,
        (None, None) => false,
        _ => true,
    }
}

/// Magnitude key used to rank a dual-value pair: `|Δ|` when both sides are
/// present, `|present value|` when exactly one side is present, and `None`
/// (excluded from ranking) when neither side has a value.
fn dual_pair_magnitude(v1: Option<f64>, v2: Option<f64>) -> Option<f64> {
    match (v1, v2) {
        (Some(a), Some(b)) => Some((b - a).abs()),
        (Some(a), None) => Some(a.abs()),
        (None, Some(b)) => Some(b.abs()),
        (None, None) => None,
    }
}

/// Rank `len` items by a descending magnitude key, dropping items whose key is
/// `None`. Ties break on the original index so the order is deterministic.
fn rank_by_key(len: usize, key: impl Fn(usize) -> Option<f64>) -> Vec<usize> {
    let mut keyed: Vec<(usize, f64)> = (0..len).filter_map(|i| key(i).map(|k| (i, k))).collect();
    keyed.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal).then_with(|| a.0.cmp(&b.0)));
    keyed.into_iter().map(|(i, _)| i).collect()
}

/// Rank constraint diff rows by descending `|Δ shadow price|`.
///
/// Rows where both sides have a shadow price rank by `|sp2 - sp1|`; rows where
/// exactly one side has a value rank by the present value's magnitude; rows
/// where neither side has a value are excluded. Returns indices into `rows`.
pub fn rank_constraints_by_shadow_delta(rows: &[ConstraintDiffRow]) -> Vec<usize> {
    rank_by_key(rows.len(), |i| dual_pair_magnitude(rows[i].shadow_price1, rows[i].shadow_price2))
}

/// Rank variable diff rows by descending `|Δ reduced cost|`.
///
/// Same missing-side semantics as [`rank_constraints_by_shadow_delta`].
/// Returns indices into `rows`.
pub fn rank_variables_by_reduced_cost_delta(rows: &[VarDiffRow]) -> Vec<usize> {
    rank_by_key(rows.len(), |i| dual_pair_magnitude(rows[i].reduced_cost1, rows[i].reduced_cost2))
}

/// Rank `(name, value)` pairs by descending `|value|`. Returns indices into `values`.
///
/// Used by the single-solve Duals tab to rank constraints by `|shadow price|`
/// and variables by `|reduced cost|`.
pub fn rank_by_magnitude(values: &[(String, f64)]) -> Vec<usize> {
    rank_by_key(values.len(), |i| Some(values[i].1.abs()))
}

/// Intermediate model built from an `LpProblem` before solving.
///
/// `pub(crate)` because [`crate::highs_presolve`] builds the same model to ask
/// `HiGHS` what its own presolve removes: the report is only about the model
/// the solver actually sees if it is built the same way, down to the column
/// order.
pub(crate) struct BuiltModel {
    pub(crate) row_problem: highs::RowProblem,
    /// Quadratic part of the primary objective as `(column, column, coefficient)`
    /// of `coefficient * x_i * x_j`; empty for a linear objective.
    objective_quadratic: Vec<(usize, usize, f64)>,
    pub(crate) variable_names: Vec<String>,
    sorted_var_ids: Vec<NameId>,
    objective_coefficients: HashMap<NameId, f64>,
    pub(crate) row_constraint_names: Vec<String>,
    pub(crate) skipped_sos: usize,
    pub(crate) sense: highs::Sense,
}

/// Metadata from the built model needed for solution extraction (after
/// `row_problem` has been consumed by `optimise`).
struct SolveMetadata {
    variable_names: Vec<String>,
    sorted_var_ids: Vec<NameId>,
    objective_coefficients: HashMap<NameId, f64>,
    objective_quadratic: Vec<(usize, usize, f64)>,
    row_constraint_names: Vec<String>,
    skipped_sos: usize,
}

/// Map a variable's kind + bounds to `(is_integer, lower, upper)` bounds for `HiGHS`.
///
/// Keyed off `kind`/`bounds` directly rather than the lossy legacy `VariableType`
/// so an integer variable carrying explicit bounds (e.g. `0 <= x <= 10`) is still
/// reported as integer. An unbounded side falls back to the LP default of
/// `[0, +inf)` (binary defaults to `[0, 1]`).
pub(crate) fn variable_bounds(variable: Option<&Variable>) -> (bool, f64, f64) {
    let Some(v) = variable else {
        return (false, 0.0, f64::INFINITY);
    };
    // Delegated rather than reimplemented: `effective_lower` is the one place
    // that knows a declared-`free` variable has no lower bound while an
    // undeclared one defaults to zero, and that a binary is canonically [0, 1].
    (v.kind.is_integer(), v.bounds.effective_lower(v.kind), v.bounds.effective_upper(v.kind))
}

/// Objective coefficients of the problem's primary objective, keyed by variable.
///
/// "Primary" is the alphabetically first objective by resolved name — the same
/// choice the solve makes, so presolve reasons about the objective that will
/// actually be optimised.
pub(crate) fn primary_objective_coefficients(problem: &LpProblem) -> HashMap<NameId, f64> {
    let Some((_, objective)) = problem.objectives.iter().min_by_key(|(id, _)| problem.resolve(**id)) else {
        return HashMap::new();
    };
    let mut map = HashMap::with_capacity(objective.coefficients.len());
    for coefficient in &objective.coefficients {
        map.insert(coefficient.name, coefficient.value);
    }
    map
}

/// Sort variable `NameId`s by resolved name for deterministic column ordering.
fn sorted_variable_ids(problem: &LpProblem) -> Vec<NameId> {
    let mut sorted_var_ids: Vec<NameId> = problem.variables.keys().copied().collect();
    sorted_var_ids.sort_by(|a, b| problem.resolve(*a).cmp(problem.resolve(*b)));
    sorted_var_ids
}

/// Reject a model containing constraints `HiGHS` cannot express, rather than
/// silently solving the model without them.
///
/// # Errors
///
/// Returns an error naming the first unsupported constraint.
pub(crate) fn check_supported(problem: &LpProblem) -> Result<(), String> {
    for (name_id, constraint) in &problem.constraints {
        let kind = match constraint {
            Constraint::Indicator { .. } => "indicator",
            Constraint::Quadratic { .. } => "quadratic",
            Constraint::General { .. } => "general",
            Constraint::Standard { .. } | Constraint::SOS { .. } => continue,
        };
        return Err(format!("{kind} constraint '{}' is not supported by the HiGHS solver", problem.resolve(*name_id)));
    }
    Ok(())
}

/// Build a `HiGHS` `RowProblem` from an `LpProblem` with a linear objective.
///
/// Used by the queries (IIS, ranging, rays, presolve), which have no quadratic
/// counterpart; only `solve_problem` passes a quadratic objective on.
///
/// # Errors
///
/// Returns an error when the model has a constraint `HiGHS` cannot express
/// (see [`check_supported`]) or a quadratic objective.
pub(crate) fn build_highs_model(problem: &LpProblem) -> Result<BuiltModel, String> {
    let built = build_highs_qp_model(problem)?;
    if !built.objective_quadratic.is_empty() {
        return Err("the objective has quadratic terms, which only a full solve supports".to_owned());
    }
    Ok(built)
}

/// Build a `HiGHS` `RowProblem` from an `LpProblem`, keeping the primary
/// objective's quadratic terms in [`BuiltModel::objective_quadratic`] for the
/// caller to pass as a Hessian.
///
/// # Errors
///
/// As [`build_highs_model`], except that a quadratic objective is allowed.
fn build_highs_qp_model(problem: &LpProblem) -> Result<BuiltModel, String> {
    debug_assert!(!problem.variables.is_empty(), "cannot build a HiGHS model with no variables");
    check_supported(problem)?;

    // Sort variable NameIds by resolved name for deterministic ordering.
    let sorted_var_ids = sorted_variable_ids(problem);

    let variable_names: Vec<String> = sorted_var_ids.iter().map(|id| problem.resolve(*id).to_string()).collect();

    let variable_index: HashMap<NameId, usize> = {
        let mut map = HashMap::with_capacity(sorted_var_ids.len());
        map.extend(sorted_var_ids.iter().enumerate().map(|(i, &id)| (id, i)));
        map
    };

    let objective_coefficients = primary_objective_coefficients(problem);

    let mut row_problem = highs::RowProblem::new();
    let mut columns = Vec::with_capacity(sorted_var_ids.len());

    for &var_id in &sorted_var_ids {
        let objective_coefficient = objective_coefficients.get(&var_id).copied().unwrap_or(0.0);
        let variable = problem.variables.get(&var_id);

        let (is_integer, lower, upper) = variable_bounds(variable);

        // A semi-integer column is passed as such (HiGHS rejects one with an
        // infinite upper bound, which surfaces as a model error rather than a
        // silently relaxed solve).
        let integrality =
            if variable.is_some_and(|v| v.kind == VariableKind::SemiInteger) { highs::Integrality::SemiInteger } else { is_integer.into() };
        let col = row_problem.add_column_with_integrality_kind(objective_coefficient, lower..=upper, integrality);
        columns.push(col);
    }

    // Sort constraints by resolved name for deterministic ordering.
    let mut sorted_constraints: Vec<_> = problem.constraints.iter().collect();
    sorted_constraints.sort_by_key(|(id, _)| problem.resolve(**id));

    let mut skipped_sos: usize = 0;
    let mut row_constraint_names = Vec::new();
    let mut row_factors: Vec<(highs::Col, f64)> = Vec::new();

    for (name_id, constraint) in &sorted_constraints {
        let constraint_name = problem.resolve(**name_id);

        match constraint {
            Constraint::Standard { coefficients, operator, rhs, .. } => {
                row_factors.clear();
                row_factors.extend(coefficients.iter().filter_map(|c| variable_index.get(&c.name).map(|&idx| (columns[idx], c.value))));

                match operator {
                    ComparisonOp::LTE | ComparisonOp::LT => {
                        row_problem.add_row(..=*rhs, &row_factors);
                    }
                    ComparisonOp::GTE | ComparisonOp::GT => {
                        row_problem.add_row(*rhs.., &row_factors);
                    }
                    ComparisonOp::EQ => {
                        row_problem.add_row(*rhs..=*rhs, &row_factors);
                    }
                }
                row_constraint_names.push(constraint_name.to_string());
            }
            Constraint::SOS { .. } => {
                skipped_sos += 1;
            }
            Constraint::Indicator { .. } | Constraint::Quadratic { .. } | Constraint::General { .. } => {
                unreachable!("rejected by check_supported")
            }
        }
    }

    let sense = match problem.sense {
        lp_parser_rs::model::Sense::Minimize => highs::Sense::Minimise,
        lp_parser_rs::model::Sense::Maximize => highs::Sense::Maximise,
    };

    debug_assert_eq!(columns.len(), variable_names.len(), "column count must match variable count");

    // The primary objective, as in `primary_objective_coefficients`.
    let objective_quadratic: Vec<(usize, usize, f64)> = problem
        .objectives
        .iter()
        .min_by_key(|(id, _)| problem.resolve(**id))
        .map(|(_, objective)| {
            objective
                .quadratic
                .iter()
                .filter_map(|t| Some((*variable_index.get(&t.var1)?, *variable_index.get(&t.var2)?, t.coefficient)))
                .collect()
        })
        .unwrap_or_default();

    Ok(BuiltModel {
        row_problem,
        objective_quadratic,
        variable_names,
        sorted_var_ids,
        objective_coefficients,
        row_constraint_names,
        skipped_sos,
        sense,
    })
}

/// Hand a built problem to `HiGHS`, reporting a rejected model as an error.
///
/// The crate's `optimise` panics when `HiGHS` refuses the model, which it does
/// for an infinite right-hand side, bound or coefficient — all of which the
/// parser accepts — so every caller goes through the fallible variant.
///
/// # Errors
///
/// Returns an error when `HiGHS` rejects the model.
pub(crate) fn pass_model(row_problem: highs::RowProblem, sense: highs::Sense) -> Result<highs::Model, String> {
    row_problem
        .try_optimise(sense)
        .map_err(|status| format!("HiGHS rejected the model ({status:?}): check for an infinite right-hand side, bound or coefficient"))
}

/// Run `HiGHS` on `model`, reporting a solver error instead of panicking.
///
/// # Errors
///
/// Returns an error when `HiGHS` reports an error while solving.
pub(crate) fn run_model(model: highs::Model) -> Result<highs::SolvedModel, String> {
    model.try_solve().map_err(|status| format!("HiGHS failed to solve the model ({status:?})"))
}

/// Extract the solution from a solved `HiGHS` model into a `SolveResult`.
fn extract_solution(
    metadata: &SolveMetadata,
    solved: &highs::SolvedModel,
    build_time: std::time::Duration,
    solve_time: std::time::Duration,
    solver_log: String,
) -> SolveResult {
    let status = format!("{:?}", solved.status());

    let (objective_value, variables, reduced_costs, shadow_prices, row_values) = match solved.status() {
        highs::HighsModelStatus::Optimal | highs::HighsModelStatus::ObjectiveBound => {
            let solution = solved.get_solution();

            debug_assert_eq!(solution.columns().len(), metadata.variable_names.len(), "solution column count must match variable count");

            let objective_value = Some(
                solution
                    .columns()
                    .iter()
                    .enumerate()
                    .map(|(i, &value)| {
                        let coefficient = metadata.objective_coefficients.get(&metadata.sorted_var_ids[i]).copied().unwrap_or(0.0);
                        value * coefficient
                    })
                    .sum::<f64>()
                    + metadata
                        .objective_quadratic
                        .iter()
                        .map(|&(i, j, coefficient)| coefficient * solution.columns()[i] * solution.columns()[j])
                        .sum::<f64>(),
            );

            let variables: Vec<(String, f64)> =
                metadata.variable_names.iter().zip(solution.columns().iter()).map(|(name, &value)| (name.clone(), value)).collect();

            let reduced_costs: Vec<(String, f64)> =
                metadata.variable_names.iter().zip(solution.dual_columns().iter()).map(|(name, &value)| (name.clone(), value)).collect();

            let shadow_prices: Vec<(String, f64)> =
                metadata.row_constraint_names.iter().zip(solution.dual_rows().iter()).map(|(name, &value)| (name.clone(), value)).collect();

            let row_values: Vec<(String, f64)> =
                metadata.row_constraint_names.iter().zip(solution.rows().iter()).map(|(name, &value)| (name.clone(), value)).collect();

            (objective_value, variables, reduced_costs, shadow_prices, row_values)
        }
        _ => (None, Vec::new(), Vec::new(), Vec::new(), Vec::new()),
    };

    SolveResult {
        status,
        objective_value,
        variables,
        reduced_costs,
        shadow_prices,
        row_values,
        build_time,
        solve_time,
        extract_time: std::time::Duration::ZERO, // filled in by caller
        solver_log,
        skipped_sos: metadata.skipped_sos,
    }
}

/// Monotonic counter distinguishing concurrent solver-log temp files within one process.
static SOLVE_LOG_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// `HiGHS` options file picked up from the current directory, if present.
const OPTIONS_FILE: &str = "highs.opt";

/// Options we set ourselves; a file must not redirect the log the pane reads back.
const RESERVED_OPTIONS: [&str; 2] = ["log_file", "output_flag"];

/// Parse a `HiGHS` options file: `key = value` per line, `#` comments, blanks skipped.
///
/// Same format as the `highs` CLI's `--options_file`, so one file serves both.
/// Reserved keys are dropped here rather than at the call site so the test covers it.
fn parse_options(text: &str) -> Vec<(&str, &str)> {
    text.lines()
        .map(|line| line.split_once('#').map_or(line, |(before, _)| before))
        .filter_map(|line| line.split_once('='))
        .map(|(key, value)| (key.trim(), value.trim()))
        .filter(|(key, value)| !key.is_empty() && !value.is_empty() && !RESERVED_OPTIONS.contains(key))
        .collect()
}

/// Set one `HiGHS` option from its string form, discovering the option's type.
///
/// `HiGHS` types each option and rejects a mismatched setter, and there is no
/// way to ask which type it wanted, so the setters are tried in turn from the
/// most specific reading of `value` to the least. `simplex_strategy = 4` takes
/// the integer setter, `time_limit = 300` the double, `presolve = off` the
/// string, and each falls through to the next on rejection.
///
/// Every attempt goes through `try_set_option`: the crate's `set_option`
/// *panics* on rejection, so probing with it would take the TUI down on the
/// first mistyped key.
///
/// # Errors
///
/// Returns an error when `HiGHS` rejects every setter — an unknown option name,
/// or a value outside the option's range.
fn set_option_value(model: &mut highs::Model, key: &str, value: &str) -> Result<(), String> {
    debug_assert!(!key.is_empty(), "option key must not be empty");

    // Ordered most-specific first; `||` stops at the first setter HiGHS accepts.
    let mut accepted = match value {
        "true" => model.try_set_option(key, true).is_ok(),
        "false" => model.try_set_option(key, false).is_ok(),
        _ => false,
    };
    if !accepted && let Ok(int) = value.parse::<i32>() {
        accepted = model.try_set_option(key, int).is_ok();
    }
    if !accepted && let Ok(float) = value.parse::<f64>() {
        accepted = model.try_set_option(key, float).is_ok();
    }
    if !accepted {
        accepted = model.try_set_option(key, value).is_ok();
    }

    if accepted { Ok(()) } else { Err(format!("HiGHS rejected `{key} = {value}` (unknown option or value out of range)")) }
}

/// Apply `highs.opt` from the current directory. Returns the options applied.
///
/// Absent file is the normal case, not an error; anything else is surfaced.
fn apply_options_file(model: &mut highs::Model) -> Result<Vec<String>, String> {
    let text = match std::fs::read_to_string(OPTIONS_FILE) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(format!("failed to read {OPTIONS_FILE}: {e}")),
    };

    // A bad line in a user's options file must not lose the solve: record what
    // HiGHS refused and carry on, so the note lands in the log the pane shows.
    let options = parse_options(&text);
    let mut applied = Vec::with_capacity(options.len());
    for (key, value) in &options {
        match set_option_value(model, key, value) {
            Ok(()) => applied.push(format!("{key} = {value}")),
            Err(e) => applied.push(format!("{key} = {value} (ignored: {e})")),
        }
    }

    Ok(applied)
}

/// Convert an `LpProblem` to a `HiGHS` `RowProblem` and solve it.
///
/// # Errors
///
/// Returns an error if the temp log path is not UTF-8, `highs.opt` cannot be
/// read, or the solver log cannot be read back.
#[cfg(test)]
pub fn solve_problem(problem: &LpProblem) -> Result<SolveResult, String> {
    solve_problem_with(problem, &[])
}

/// `solve_problem`, stopping early once `cancel` is set.
///
/// `HiGHS` polls its interrupt callbacks between simplex, interior-point and
/// branch-and-bound iterations; the callback registered here answers them
/// with the flag, so setting it ends the solve within an iteration or so. The
/// result then carries the `ReachedInterrupt` status.
///
/// # Errors
///
/// As `solve_problem`, and when `HiGHS` will not register the callback.
pub fn solve_problem_cancellable(problem: &LpProblem, cancel: &AtomicBool) -> Result<SolveResult, String> {
    solve(problem, &[], Some(cancel))
}

/// The interrupt callback [`solve_problem_cancellable`] registers: it raises
/// `HiGHS`'s interrupt flag once the caller's cancel flag is set.
///
/// `user_data` is the `AtomicBool` the solve was started with.
unsafe extern "C" fn interrupt_on_cancel(
    _callback_type: std::os::raw::c_int,
    _message: *const std::os::raw::c_char,
    _data_out: *const highs_sys::HighsCallbackDataOut,
    data_in: *mut highs_sys::HighsCallbackDataIn,
    user_data: *mut std::os::raw::c_void,
) {
    if user_data.is_null() || data_in.is_null() {
        return;
    }
    // SAFETY: `user_data` is the `&AtomicBool` passed to `Highs_setCallback`
    // in `solve`, which borrows it for the whole of the solve that calls this.
    let cancel = unsafe { &*user_data.cast::<AtomicBool>() };
    if cancel.load(Ordering::Relaxed) {
        // SAFETY: `HiGHS` passes a valid, exclusively borrowed input struct to
        // every interrupt callback; it was checked non-null above.
        unsafe { (*data_in).user_interrupt = 1 };
    }
}

/// Register [`interrupt_on_cancel`] on `model` for every interrupt point.
fn register_cancel(model: &mut highs::Model, cancel: &AtomicBool) -> Result<(), String> {
    let highs = model.as_mut_ptr();
    let user_data = std::ptr::from_ref(cancel).cast_mut().cast::<std::os::raw::c_void>();
    // SAFETY: `highs` is the live model `model` owns. `user_data` points at
    // `cancel`, which the caller keeps borrowed until the solve has returned,
    // and the callback only reads it through a shared reference.
    let status = unsafe { highs_sys::Highs_setCallback(highs, Some(interrupt_on_cancel), user_data) };
    if status == highs_sys::kHighsStatusError {
        return Err("HiGHS would not register the cancel callback".to_owned());
    }
    for callback in
        [highs_sys::kHighsCallbackSimplexInterrupt, highs_sys::kHighsCallbackIpmInterrupt, highs_sys::kHighsCallbackMipInterrupt]
    {
        // SAFETY: the same live model; starting a callback only sets a flag.
        let status = unsafe { highs_sys::Highs_startCallback(highs, callback) };
        if status == highs_sys::kHighsStatusError {
            return Err(format!("HiGHS would not start interrupt callback {callback}"));
        }
    }
    Ok(())
}

/// `solve_problem`, with `extra` `HiGHS` options applied on top.
///
/// `extra` is applied *after* `highs.opt`, so a caller-supplied preset wins over
/// a stale options file in the working directory. Keys reserved by the solve
/// itself (see [`RESERVED_OPTIONS`]) are ignored, as they are for the file.
///
/// # Errors
///
/// As `solve_problem`.
pub fn solve_problem_with(problem: &LpProblem, extra: &[(&str, &str)]) -> Result<SolveResult, String> {
    solve(problem, extra, None)
}

/// The solve behind [`solve_problem_with`] and [`solve_problem_cancellable`].
fn solve(problem: &LpProblem, extra: &[(&str, &str)], cancel: Option<&AtomicBool>) -> Result<SolveResult, String> {
    debug_assert!(!problem.variables.is_empty(), "cannot solve a problem with no variables");
    debug_assert!(
        extra.iter().all(|(key, _)| !RESERVED_OPTIONS.contains(key)),
        "extra options must not redirect the solver log: {extra:?}"
    );

    let build_start = Instant::now();
    let model = build_highs_qp_model(problem)?;
    let build_time = build_start.elapsed();
    if !model.objective_quadratic.is_empty() && problem.variables.values().any(|v| v.kind != VariableKind::Continuous) {
        return Err("HiGHS cannot solve a quadratic objective with integer, semi-continuous or SOS variables (MIQP)".to_owned());
    }

    // pid+sequence-named temp file + explicit cleanup instead of the
    // tempfile crate. The sequence number keeps concurrent solves in one process
    // ("Solve both" runs two solver threads) from clobbering each other's log.
    let log_seq = SOLVE_LOG_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let log_path = std::env::temp_dir().join(format!("lp_diff_solver_{}_{log_seq}.log", std::process::id()));

    let BuiltModel {
        row_problem,
        objective_quadratic,
        sense,
        variable_names,
        sorted_var_ids,
        objective_coefficients,
        row_constraint_names,
        skipped_sos,
    } = model;

    let hessian = hessian_columns(&objective_quadratic, variable_names.len());
    let metadata =
        SolveMetadata { variable_names, sorted_var_ids, objective_coefficients, objective_quadratic, row_constraint_names, skipped_sos };

    let mut highs_model = pass_model(row_problem, sense)?;
    if let Some(columns) = hessian {
        highs_model
            .try_pass_hessian(highs::HessianFormat::Triangular, columns)
            .map_err(|e| format!("HiGHS rejected the quadratic objective: {e}"))?;
    }
    highs_model.set_option("output_flag", true);
    highs_model.set_option("log_file", log_path.to_str().ok_or_else(|| "temp file path is not valid UTF-8".to_owned())?);
    // After `log_file`, so anything `HiGHS` rejects is written to the log the pane
    // shows rather than to the terminal the TUI owns.
    let mut applied_options = apply_options_file(&mut highs_model)?;
    // After the file, so a preset chosen in the TUI wins over a stale `highs.opt`.
    // Unlike the options file, these are load-bearing: a caller asked for this
    // exact configuration, and silently solving the default under its label
    // would make a profile row a lie.
    for (key, value) in extra.iter().filter(|(key, _)| !RESERVED_OPTIONS.contains(key)) {
        set_option_value(&mut highs_model, key, value)?;
        applied_options.push(format!("{key} = {value}"));
    }

    if let Some(cancel) = cancel {
        register_cancel(&mut highs_model, cancel)?;
    }

    let solve_start = Instant::now();
    let solved = run_model(highs_model).map_err(|error| match std::fs::remove_file(&log_path) {
        Ok(()) => error,
        Err(e) => format!("{error} (and failed to remove solver log {}: {e})", log_path.display()),
    })?;
    let solve_time = solve_start.elapsed();

    let mut solver_log = std::fs::read_to_string(&log_path).map_err(|e| format!("failed to read solver log: {e}"))?;
    // Options applied silently would be indistinguishable from a default solve, so
    // record them alongside the log the pane shows.
    if !applied_options.is_empty() {
        solver_log.insert_str(0, &format!("[lp_diff] options: {}\n\n", applied_options.join(", ")));
    }
    // Cleanup failure is non-fatal (overwritten next solve, reaped by the OS); surface it in the log.
    if let Err(e) = std::fs::remove_file(&log_path) {
        write!(solver_log, "\n[lp_diff] warning: failed to remove solver log {}: {e}\n", log_path.display())
            .expect("fmt::Write to String is infallible");
    }

    let extract_start = Instant::now();
    let mut result = extract_solution(&metadata, &solved, build_time, solve_time, solver_log);
    result.extract_time = extract_start.elapsed();

    Ok(result)
}

/// Lower-triangular Hessian columns for `HiGHS`, whose objective is
/// `c'x + 1/2 x'Qx`: a square term `c x_i^2` is `Q_ii = 2c` and a product
/// `c x_i x_j` is `Q_ij = Q_ji = c`, stored once at row `max(i, j)` of column
/// `min(i, j)`. `None` for a linear objective.
fn hessian_columns(terms: &[(usize, usize, f64)], column_count: usize) -> Option<Vec<Vec<(usize, f64)>>> {
    if terms.is_empty() {
        return None;
    }
    let mut columns: Vec<std::collections::BTreeMap<usize, f64>> = vec![std::collections::BTreeMap::new(); column_count];
    for &(i, j, coefficient) in terms {
        debug_assert!(i < column_count && j < column_count, "quadratic term indices must name columns");
        let (row, col) = (i.max(j), i.min(j));
        let value = if i == j { 2.0 * coefficient } else { coefficient };
        *columns[col].entry(row).or_insert(0.0) += value;
    }
    Some(columns.into_iter().map(|column| column.into_iter().collect()).collect())
}

/// Return `true` if a solve status string (as produced by `extract_solution`,
/// i.e. the `Debug` form of `highs::HighsModelStatus`) indicates infeasibility.
pub fn status_is_infeasible(status: &str) -> bool {
    status == "Infeasible" || status == "UnboundedOrInfeasible"
}

/// Return `true` if a solve status string indicates unboundedness.
///
/// `UnboundedOrInfeasible` appears here *and* in [`status_is_infeasible`]: it is
/// exactly the case where presolve declined to say which, so both diagnoses are
/// worth offering.
pub fn status_is_unbounded(status: &str) -> bool {
    status == "Unbounded" || status == "UnboundedOrInfeasible"
}

/// Slack values above this threshold count as constraint violations in the
/// elastic relaxation diagnosis.
pub const VIOLATION_TOLERANCE: f64 = 1e-7;

/// Outcome of an elastic-relaxation infeasibility diagnosis.
#[derive(Debug, Clone)]
pub struct InfeasibilityDiagnosis {
    /// Sum of all slack values in the optimal elastic solution plus all bound
    /// conflict gaps (the minimum total violation needed to make the problem
    /// feasible).
    pub total_violation: f64,
    /// `(constraint name, violation amount)` for every constraint whose slack
    /// exceeds [`VIOLATION_TOLERANCE`], sorted descending by amount.
    pub violations: Vec<(String, f64)>,
    /// `(variable name, gap)` for every variable whose declared bounds
    /// conflict (`lower > upper`, gap = `lower - upper`), sorted descending
    /// by gap. Such bounds are relaxed before the elastic solve so the
    /// diagnosis can still run.
    pub bound_conflicts: Vec<(String, f64)>,
    /// Wall-clock time of the elastic solve (build + solve + extract).
    pub solve_time: Duration,
}

/// Aggregate per-constraint slack values into a sorted violation list.
///
/// `slack_names` holds the owning constraint name for each slack column (an
/// equality constraint contributes two consecutive entries with the same name,
/// which are summed). Constraints whose total slack exceeds `tolerance` are
/// returned sorted descending by violation amount, ties broken by name.
pub fn collect_violations(slack_names: &[String], slack_values: &[f64], tolerance: f64) -> Vec<(String, f64)> {
    debug_assert_eq!(slack_names.len(), slack_values.len(), "slack names and values must have equal length");
    debug_assert!(tolerance >= 0.0, "violation tolerance must be non-negative, got {tolerance}");

    let mut violations: Vec<(String, f64)> = Vec::new();
    for (name, &value) in slack_names.iter().zip(slack_values) {
        match violations.last_mut() {
            Some((last_name, total)) if last_name == name => *total += value,
            _ => violations.push((name.clone(), value)),
        }
    }
    violations.retain(|(_, amount)| *amount > tolerance);
    violations.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal).then_with(|| a.0.cmp(&b.0)));
    violations
}

/// Diagnose an infeasible problem via elastic relaxation.
///
/// Variables whose declared bounds conflict (`lower > upper`, e.g.
/// `DoubleBound(5, 3)` or a negative `UpperBound` with the implicit lower
/// bound of 0) are reported directly as bound conflicts and their bounds
/// relaxed to the interval between the conflicting values, so the elastic
/// solve can still run. The model is then rebuilt with all integrality
/// relaxed and a non-negative slack variable added to every standard
/// constraint (one for `>=`, one for `<=`, two for `=`), minimising the sum
/// of all slacks. Constraints with a positive slack in the optimal solution
/// are exactly those that must be violated to make the rest of the problem
/// feasible.
///
/// # Errors
///
/// Returns an error if the elastic problem does not solve to optimality. By
/// construction it is feasible in both its constraints and its (sanitised)
/// bounds, so this only occurs on a solver failure.
pub fn diagnose_infeasibility(problem: &LpProblem) -> Result<InfeasibilityDiagnosis, String> {
    debug_assert!(!problem.variables.is_empty(), "cannot diagnose a problem with no variables");

    check_supported(problem)?;
    let start = Instant::now();
    let sorted_var_ids = sorted_variable_ids(problem);
    let variable_index: HashMap<NameId, usize> = {
        let mut map = HashMap::with_capacity(sorted_var_ids.len());
        map.extend(sorted_var_ids.iter().enumerate().map(|(i, &id)| (id, i)));
        map
    };

    let mut row_problem = highs::RowProblem::new();
    let mut columns = Vec::with_capacity(sorted_var_ids.len());
    let mut bound_conflicts: Vec<(String, f64)> = Vec::new();
    for &var_id in &sorted_var_ids {
        // Zero objective coefficient and relaxed integrality: the elastic
        // objective is the slack sum alone, and an LP relaxation is faster and
        // more reliable than the original MIP.
        let (_, mut lower, mut upper) = variable_bounds(problem.variables.get(&var_id));
        if lower > upper {
            // Conflicting bounds would make the elastic model itself
            // infeasible: report the gap and relax the variable to the
            // interval between the two values (the minimal region either
            // bound can move into).
            bound_conflicts.push((problem.resolve(var_id).to_string(), lower - upper));
            (lower, upper) = (upper, lower);
        }
        columns.push(row_problem.add_column_with_integrality(0.0, lower..=upper, false));
    }

    // Sort constraints by resolved name for deterministic ordering, matching `build_highs_model`.
    let mut sorted_constraints: Vec<_> = problem.constraints.iter().collect();
    sorted_constraints.sort_by_key(|(id, _)| problem.resolve(**id));

    // One slack column per inequality, two per equality; `slack_names[i]` is
    // the owning constraint of slack column `slack_cols[i]`.
    let mut slack_names: Vec<String> = Vec::new();
    let mut slack_cols: Vec<highs::Col> = Vec::new();
    let mut row_factors: Vec<(highs::Col, f64)> = Vec::new();

    for (name_id, constraint) in &sorted_constraints {
        let Constraint::Standard { coefficients, operator, rhs, .. } = constraint else {
            continue; // SOS constraints are skipped, as in `build_highs_model`.
        };
        let constraint_name = problem.resolve(**name_id);

        row_factors.clear();
        row_factors.extend(coefficients.iter().filter_map(|c| variable_index.get(&c.name).map(|&idx| (columns[idx], c.value))));

        match operator {
            ComparisonOp::LTE | ComparisonOp::LT => {
                // Surplus slack: lhs - s <= rhs.
                let slack = row_problem.add_column(1.0, 0.0..);
                slack_names.push(constraint_name.to_string());
                slack_cols.push(slack);
                row_factors.push((slack, -1.0));
                row_problem.add_row(..=*rhs, &row_factors);
            }
            ComparisonOp::GTE | ComparisonOp::GT => {
                // Deficit slack: lhs + s >= rhs.
                let slack = row_problem.add_column(1.0, 0.0..);
                slack_names.push(constraint_name.to_string());
                slack_cols.push(slack);
                row_factors.push((slack, 1.0));
                row_problem.add_row(*rhs.., &row_factors);
            }
            ComparisonOp::EQ => {
                // Both directions: lhs + s_deficit - s_surplus = rhs.
                let deficit = row_problem.add_column(1.0, 0.0..);
                let surplus = row_problem.add_column(1.0, 0.0..);
                slack_names.push(constraint_name.to_string());
                slack_names.push(constraint_name.to_string());
                slack_cols.push(deficit);
                slack_cols.push(surplus);
                row_factors.push((deficit, 1.0));
                row_factors.push((surplus, -1.0));
                row_problem.add_row(*rhs..=*rhs, &row_factors);
            }
        }
    }

    debug_assert_eq!(slack_names.len(), slack_cols.len(), "slack names and columns must be in sync");

    let mut highs_model = pass_model(row_problem, highs::Sense::Minimise)?;
    // Suppress solver output: the diagnosis runs while the TUI owns the terminal.
    highs_model.set_option("output_flag", false);

    let solved = run_model(highs_model)?;
    let status = solved.status();
    debug_assert!(
        matches!(status, highs::HighsModelStatus::Optimal),
        "elastic relaxation must solve to optimality (feasible by construction), got {status:?}"
    );
    if !matches!(status, highs::HighsModelStatus::Optimal) {
        return Err(format!("elastic relaxation returned {status:?} — solver failure"));
    }

    let solution = solved.get_solution();
    let column_values = solution.columns();
    let slack_values: Vec<f64> = slack_cols.iter().map(|col| column_values[col.index()]).collect();
    let total_violation: f64 = slack_values.iter().sum::<f64>() + bound_conflicts.iter().map(|(_, gap)| gap).sum::<f64>();
    let violations = collect_violations(&slack_names, &slack_values, VIOLATION_TOLERANCE);
    bound_conflicts.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal).then_with(|| a.0.cmp(&b.0)));

    Ok(InfeasibilityDiagnosis { total_violation, violations, bound_conflicts, solve_time: start.elapsed() })
}

/// Format an `Option<f64>` as a CSV field, empty for `None`.
fn field(value: Option<f64>) -> String {
    debug_assert!(value.is_none_or(f64::is_finite), "CSV field called with a non-finite value: {value:?}");
    value.map_or_else(String::new, |v| v.to_string())
}

/// Write the diff results to two timestamped CSV files in `dir`.
///
/// Returns the filenames of the two written files on success.
///
/// # Errors
///
/// Returns an error if the CSV files cannot be created or written to.
pub fn write_diff_csv(diff: &SolveDiffResult, dir: &Path) -> Result<(String, String), Box<dyn Error>> {
    debug_assert!(dir.is_dir(), "write_diff_csv: dir must be an existing directory");

    let ts = crate::export::file_stamp();
    let var_filename = format!("variable_diff_{ts}.csv");
    let con_filename = format!("constraint_diff_{ts}.csv");

    {
        let mut wtr = csv::Writer::from_path(dir.join(&var_filename))?;
        wtr.write_record(["name", "value_1", "value_2", "delta", "reduced_cost_1", "reduced_cost_2"])?;

        for row in &diff.variable_diff {
            if row.val1.is_none() && row.val2.is_none() {
                continue;
            }
            let delta = row.val1.zip(row.val2).map(|(v1, v2)| v2 - v1);
            wtr.write_record([
                row.name(&diff.result1, &diff.result2),
                &field(row.val1),
                &field(row.val2),
                &field(delta),
                &field(row.reduced_cost1),
                &field(row.reduced_cost2),
            ])?;
        }
        wtr.flush()?;
    }

    {
        let mut wtr = csv::Writer::from_path(dir.join(&con_filename))?;
        wtr.write_record(["name", "activity_1", "activity_2", "shadow_price_1", "shadow_price_2"])?;

        for row in &diff.constraint_diff {
            if row.activity1.is_none() && row.activity2.is_none() {
                continue;
            }
            wtr.write_record([
                row.name(&diff.result1, &diff.result2),
                &field(row.activity1),
                &field(row.activity2),
                &field(row.shadow_price1),
                &field(row.shadow_price2),
            ])?;
        }
        wtr.flush()?;
    }

    Ok((var_filename, con_filename))
}

/// Write a single solve result to two timestamped CSV files in `dir`.
///
/// Returns the filenames of the two written files on success.
///
/// # Errors
///
/// Returns an error if the CSV files cannot be created or written to.
pub fn write_result_csv(result: &SolveResult, dir: &Path) -> Result<(String, String), Box<dyn Error>> {
    debug_assert!(dir.is_dir(), "write_result_csv: dir must be an existing directory");
    debug_assert_eq!(result.variables.len(), result.reduced_costs.len(), "variables and reduced_costs must have equal length");
    debug_assert_eq!(result.row_values.len(), result.shadow_prices.len(), "row_values and shadow_prices must have equal length");

    let ts = crate::export::file_stamp();
    let var_filename = format!("solve_variables_{ts}.csv");
    let con_filename = format!("solve_constraints_{ts}.csv");

    {
        let mut wtr = csv::Writer::from_path(dir.join(&var_filename))?;
        wtr.write_record(["name", "value", "reduced_cost"])?;

        for (i, (name, value)) in result.variables.iter().enumerate() {
            wtr.write_record([name, &field(Some(*value)), &field(result.reduced_costs.get(i).map(|(_, v)| *v))])?;
        }
        wtr.flush()?;
    }

    {
        let mut wtr = csv::Writer::from_path(dir.join(&con_filename))?;
        wtr.write_record(["name", "activity", "shadow_price"])?;

        for (i, (name, activity)) in result.row_values.iter().enumerate() {
            wtr.write_record([name, &field(Some(*activity)), &field(result.shadow_prices.get(i).map(|(_, v)| *v))])?;
        }
        wtr.flush()?;
    }

    Ok((var_filename, con_filename))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_options_handles_comments_blanks_and_reserved_keys() {
        let text = "\
# a comment\n\
solver = ipm\n\
\n\
run_crossover=off   # trailing comment\n\
time_limit = 300\n\
log_file = /tmp/hijack.log\n\
output_flag = false\n\
malformed line\n\
empty =\n";

        assert_eq!(
            parse_options(text),
            vec![("solver", "ipm"), ("run_crossover", "off"), ("time_limit", "300")],
            "comments, blanks, malformed and reserved keys must all be dropped"
        );
    }

    /// Build a constraint diff row with the given shadow prices (other fields immaterial to ranking).
    fn constraint_row(sp1: Option<f64>, sp2: Option<f64>) -> ConstraintDiffRow {
        ConstraintDiffRow {
            name_ref: NameRef { from_result2: false, index: 0 },
            activity1: sp1,
            activity2: sp2,
            shadow_price1: sp1,
            shadow_price2: sp2,
            changed: false,
        }
    }

    /// Build a variable diff row with the given reduced costs (other fields immaterial to ranking).
    fn variable_row(rc1: Option<f64>, rc2: Option<f64>) -> VarDiffRow {
        VarDiffRow {
            name_ref: NameRef { from_result2: false, index: 0 },
            val1: rc1,
            val2: rc2,
            reduced_cost1: rc1,
            reduced_cost2: rc2,
            changed: false,
        }
    }

    #[test]
    fn test_rank_constraints_by_shadow_delta_orders_by_magnitude() {
        let rows = vec![
            constraint_row(Some(1.0), Some(1.5)),  // |Δ| = 0.5
            constraint_row(Some(0.0), Some(-3.0)), // |Δ| = 3.0
            constraint_row(Some(2.0), Some(2.0)),  // |Δ| = 0.0
        ];
        assert_eq!(rank_constraints_by_shadow_delta(&rows), vec![1, 0, 2]);
    }

    #[test]
    fn test_rank_constraints_missing_sides_rank_by_present_magnitude() {
        let rows = vec![
            constraint_row(Some(0.5), Some(0.6)), // both present: |Δ| = 0.1
            constraint_row(Some(-4.0), None),     // one side: |−4| = 4.0
            constraint_row(None, Some(2.0)),      // one side: |2| = 2.0
            constraint_row(None, None),           // excluded
        ];
        assert_eq!(rank_constraints_by_shadow_delta(&rows), vec![1, 2, 0]);
    }

    #[test]
    fn test_rank_variables_by_reduced_cost_delta() {
        let rows = vec![
            variable_row(Some(1.0), Some(1.0)),  // |Δ| = 0.0
            variable_row(None, Some(-0.5)),      // one side: 0.5
            variable_row(Some(2.0), Some(-2.0)), // |Δ| = 4.0
            variable_row(None, None),            // excluded
        ];
        assert_eq!(rank_variables_by_reduced_cost_delta(&rows), vec![2, 1, 0]);
    }

    #[test]
    fn test_rank_by_magnitude_descending() {
        let values = vec![("a".to_owned(), 1.0), ("b".to_owned(), -5.0), ("c".to_owned(), 0.0)];
        assert_eq!(rank_by_magnitude(&values), vec![1, 0, 2]);
    }

    #[test]
    fn test_collect_violations_aggregates_and_sorts() {
        // c2 appears twice (equality constraint: deficit + surplus slack) and aggregates.
        let names = vec!["c1".to_owned(), "c2".to_owned(), "c2".to_owned(), "c3".to_owned()];
        let values = vec![0.5, 0.25, 0.5, 0.0];
        let violations = collect_violations(&names, &values, VIOLATION_TOLERANCE);
        assert_eq!(violations.len(), 2, "c3 has zero slack and must be filtered out");
        assert_eq!(violations[0].0, "c2");
        assert!((violations[0].1 - 0.75).abs() < 1e-12, "c2 slacks must aggregate to 0.75, got {}", violations[0].1);
        assert_eq!(violations[1].0, "c1");
        assert!((violations[1].1 - 0.5).abs() < 1e-12);
    }

    #[test]
    fn test_diagnose_infeasibility_tiny_lp() {
        // x >= 2 and x <= 1 conflict by exactly 1.
        let problem = LpProblem::parse("min\nobj: x\nst\nc1: x >= 2\nc2: x <= 1\nend").expect("failed to parse tiny LP");

        let result = solve_problem(&problem).expect("solver should not error");
        assert!(status_is_infeasible(&result.status), "tiny LP should be infeasible, got: {}", result.status);

        let diagnosis = diagnose_infeasibility(&problem).expect("elastic relaxation should solve");
        assert!((diagnosis.total_violation - 1.0).abs() < 1e-6, "total violation should be ≈ 1, got {}", diagnosis.total_violation);
        assert!(!diagnosis.violations.is_empty(), "the conflicting constraint(s) must be reported");
        for (name, _) in &diagnosis.violations {
            assert!(name == "c1" || name == "c2", "unexpected violated constraint: {name}");
        }
        let violation_sum: f64 = diagnosis.violations.iter().map(|(_, amount)| amount).sum();
        assert!((violation_sum - 1.0).abs() < 1e-6, "violations should sum to ≈ 1, got {violation_sum}");
    }

    #[test]
    fn test_diagnose_conflicting_variable_bounds() {
        // x has lower > upper (gap 1); y has a negative upper bound with the
        // implicit lower bound 0 (gap 5). Previously this errored out of the
        // elastic relaxation; now both conflicts are reported directly.
        let problem = LpProblem::parse("min\nobj: x + y\nst\nc1: x + y >= 0\nbounds\n2 <= x <= 1\ny <= -5\nend")
            .expect("failed to parse conflicting-bounds LP");

        let diagnosis = diagnose_infeasibility(&problem).expect("diagnosis must handle conflicting bounds");
        assert_eq!(diagnosis.bound_conflicts.len(), 2, "both conflicting variables must be reported");
        assert_eq!(diagnosis.bound_conflicts[0].0, "y", "conflicts must sort descending by gap");
        assert!((diagnosis.bound_conflicts[0].1 - 5.0).abs() < 1e-9, "y gap should be 5, got {}", diagnosis.bound_conflicts[0].1);
        assert_eq!(diagnosis.bound_conflicts[1].0, "x");
        assert!((diagnosis.bound_conflicts[1].1 - 1.0).abs() < 1e-9, "x gap should be 1, got {}", diagnosis.bound_conflicts[1].1);
        assert!(diagnosis.violations.is_empty(), "constraint c1 is satisfiable once bounds are relaxed, got {:?}", diagnosis.violations);
        assert!(
            (diagnosis.total_violation - 6.0).abs() < 1e-6,
            "total violation should be the sum of bound gaps, got {}",
            diagnosis.total_violation
        );
    }

    #[test]
    fn test_diagnose_feasible_lp_reports_no_violations() {
        let problem = LpProblem::parse("min\nobj: x\nst\nc1: x >= 1\nend").expect("failed to parse tiny LP");
        let diagnosis = diagnose_infeasibility(&problem).expect("elastic relaxation should solve");
        assert!(diagnosis.violations.is_empty(), "feasible problem must have no violations, got {:?}", diagnosis.violations);
        assert!(diagnosis.total_violation.abs() < 1e-9, "total violation should be ≈ 0, got {}", diagnosis.total_violation);
    }

    /// A tiny LP used by the option tests; the values do not matter, only that
    /// it solves.
    fn tiny_lp() -> LpProblem {
        LpProblem::parse("Minimize\n obj: x + y\nSubject To\n c1: x + y >= 2\nEnd").expect("fixture must parse")
    }

    #[test]
    fn test_an_integer_typed_option_applies_without_panicking() {
        // Regression: `highs::Model::set_option` panics when HiGHS rejects the
        // setter, and the old code deliberately set every integral value twice
        // (once as int, once as double) expecting the wrong one to be ignored.
        // Any `highs.opt` containing `simplex_strategy = 4` took the TUI down.
        let result = solve_problem_with(&tiny_lp(), &[("simplex_strategy", "1")]).expect("an int-typed option must apply");
        assert_eq!(result.status, "Optimal");
    }

    #[test]
    fn test_infinite_model_data_is_an_error_not_a_panic() {
        // Regression: the crate's `optimise` panics when HiGHS rejects the model,
        // and the parser accepts infinite right-hand sides and bounds (infinite
        // coefficients are now a parse error). Presolve ran on the UI thread, so it took the TUI down.
        let sources = [
            "Minimize\n obj: x + y\nSubject To\n c1: x + y >= inf\nEnd",
            "Minimize\n obj: x\nSubject To\n c1: x >= 1\nBounds\n x >= inf\nEnd",
        ];
        for source in sources {
            let problem = LpProblem::parse(source).expect("fixture must parse");
            let error = solve_problem(&problem).expect_err("solve must refuse the model");
            assert!(error.contains("HiGHS rejected"), "unexpected error for {source:?}: {error}");
            assert!(diagnose_infeasibility(&problem).is_err(), "diagnosis must refuse {source:?}");
            assert!(crate::highs_query::iis(&problem).is_err(), "IIS must refuse {source:?}");
            assert!(crate::highs_query::ranging(&problem).is_err(), "ranging must refuse {source:?}");
            assert!(crate::highs_query::unbounded_ray(&problem).is_err(), "ray must refuse {source:?}");
            assert!(crate::highs_presolve::highs_presolve(&problem).is_err(), "presolve must refuse {source:?}");
        }
    }

    /// A set cancel flag stops `HiGHS` at its first interrupt check, and an
    /// unset one leaves the solve alone.
    #[test]
    fn a_cancelled_solve_is_interrupted() {
        let source = include_str!("../../rust/resources/boeing1.lp");
        let problem = LpProblem::parse(source).expect("fixture parses");

        let result = solve_problem_cancellable(&problem, &AtomicBool::new(false)).expect("solves");
        assert_eq!(result.status, "Optimal", "an unset flag must not interrupt");

        let result = solve_problem_cancellable(&problem, &AtomicBool::new(true)).expect("an interrupted solve still returns");
        assert_eq!(result.status, "ReachedInterrupt", "a set flag must interrupt the solve");
        assert!(result.objective_value.is_none(), "an interrupted solve reports no objective");
    }

    #[test]
    fn test_options_of_each_type_are_accepted() {
        // One option per HiGHS setter type, since `set_option_value` discovers
        // the type by trying them in turn: string, double, int, bool.
        for (key, value) in [("presolve", "off"), ("time_limit", "30.0"), ("threads", "1"), ("allow_unbounded_or_infeasible", "true")] {
            let result = solve_problem_with(&tiny_lp(), &[(key, value)]);
            assert!(result.is_ok(), "`{key} = {value}` should apply, got {:?}", result.err());
        }
    }

    #[test]
    fn test_an_unknown_preset_option_is_an_error_not_a_silent_default_solve() {
        // A caller-supplied option is load-bearing: quietly solving the default
        // under a preset's label would make a profile row a lie.
        let error = solve_problem_with(&tiny_lp(), &[("made_up_option", "7")]).expect_err("HiGHS must reject an unknown option");
        assert!(error.contains("made_up_option"), "the error should name the option, got: {error}");
    }

    #[test]
    fn test_an_options_file_key_that_highs_refuses_is_noted_not_fatal() {
        // A user's `highs.opt` is not under our control, so a bad line is
        // reported into the log rather than losing them the solve.
        let mut model = build_highs_model(&tiny_lp()).expect("tiny LP is supported").row_problem.optimise(highs::Sense::Minimise);
        model.make_quiet();
        assert!(set_option_value(&mut model, "made_up_option", "7").is_err(), "an unknown option must be refused");
        assert!(set_option_value(&mut model, "presolve", "off").is_ok(), "a known option must still apply afterwards");
    }

    /// The regression guard for the solve half of the free/undeclared
    /// conflation: `x free` was handed to `HiGHS` with a lower bound of 0, so a
    /// model whose optimum is negative silently returned the wrong answer.
    #[test]
    fn test_a_declared_free_variable_may_go_negative() {
        // Minimising x subject to x >= -5, with x declared free. The optimum is
        // -5; clamping x at 0 would report 0 instead.
        let problem = LpProblem::parse("Minimize\n obj: x\nSubject To\n c1: x >= -5\nBounds\n x free\nEnd").expect("must parse");
        let result = solve_problem(&problem).expect("a bounded LP must solve");

        assert_eq!(result.status, "Optimal");
        let objective = result.objective_value.expect("an optimal solve has an objective");
        assert!((objective - -5.0).abs() < 1e-9, "a free x must reach -5, got {objective}");
    }

    #[test]
    fn test_an_undeclared_variable_keeps_the_lp_default_of_zero() {
        // The same model without the `free` declaration: LP says x >= 0, so the
        // optimum is 0, not -5.
        let problem = LpProblem::parse("Minimize\n obj: x\nSubject To\n c1: x >= -5\nEnd").expect("must parse");
        let result = solve_problem(&problem).expect("a bounded LP must solve");

        let objective = result.objective_value.expect("an optimal solve has an objective");
        assert!((objective - 0.0).abs() < 1e-9, "an undeclared x is non-negative, got {objective}");
    }

    #[test]
    fn test_a_semi_integer_variable_keeps_its_zero_branch() {
        // x is 0 or an integer in [2, 10]. Minimising x reaches 0; solving it
        // as a plain integer in [2, 10] would report 2 instead.
        let source =
            "Maximize\n obj: - x + 0.5 y\nSubject To\n c1: y - x <= 0.5\nBounds\n 2 <= x <= 10\nGenerals\n x\nSemi-Continuous\n x\nEnd";
        let problem = LpProblem::parse(source).expect("must parse");
        let result = solve_problem(&problem).expect("a bounded MIP must solve");
        let objective = result.objective_value.expect("an optimal solve has an objective");
        assert!((objective - 0.25).abs() < 1e-9, "x = 0, y = 0.5 is optimal, got {objective}");
    }

    #[test]
    fn test_unsupported_constraints_are_refused_not_dropped() {
        let problem =
            LpProblem::parse("Minimize\n obj: x\nSubject To\n c1: x >= 1\n ind: b = 1 -> x <= 0\nBinaries\n b\nEnd").expect("must parse");
        let error = solve_problem(&problem).expect_err("an indicator constraint must not be silently dropped");
        assert!(error.contains("indicator constraint 'ind'"), "unexpected error: {error}");
        assert!(diagnose_infeasibility(&problem).is_err(), "diagnosis must refuse too");

        let general = LpProblem::parse("Minimize\n obj: r\nSubject To\n c1: x >= 1\nGeneral Constraints\n g: r = ABS ( x )\nEnd")
            .expect("must parse");
        let error = solve_problem(&general).expect_err("a general constraint must not be silently dropped");
        assert!(error.contains("general constraint 'g'"), "unexpected error: {error}");
    }

    #[test]
    fn test_a_quadratic_objective_is_solved_as_a_qp() {
        // min x^2 + y^2 s.t. x + y >= 2: optimum x = y = 1, objective 2. Solving
        // only the (empty) linear part would report 0.
        let problem = LpProblem::parse("Minimize\n obj: [ 2 x ^ 2 + 2 y ^ 2 ] / 2\nSubject To\n c1: x + y >= 2\nEnd").expect("must parse");
        let result = solve_problem(&problem).expect("a convex QP must solve");
        let objective = result.objective_value.expect("an optimal solve has an objective");
        assert!((objective - 2.0).abs() < 1e-6, "x = y = 1 is optimal, got {objective}");

        // The queries have no quadratic counterpart and must say so.
        let error = crate::highs_query::ranging(&problem).expect_err("ranging must refuse a QP");
        assert!(error.contains("quadratic"), "unexpected error: {error}");

        let with_constraint = LpProblem::parse("Minimize\n obj: x\nSubject To\n q: [ x ^ 2 ] <= 4\nEnd").expect("must parse");
        let error = solve_problem(&with_constraint).expect_err("a quadratic constraint must not be dropped");
        assert!(error.contains("quadratic constraint 'q'"), "unexpected error: {error}");
    }

    #[test]
    fn test_status_is_infeasible() {
        assert!(status_is_infeasible("Infeasible"));
        assert!(status_is_infeasible("UnboundedOrInfeasible"));
        assert!(!status_is_infeasible("Optimal"));
        assert!(!status_is_infeasible("Unbounded"));
    }

    #[test]
    fn test_concurrent_solves_do_not_clobber_solver_log() {
        // "Solve both" runs two solve_problem calls concurrently in one process:
        // the fast solve must not delete the slow solve's log out from under it.
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let slow_input = std::fs::read_to_string(root.join("../rust/resources/mps/fit2d.mps")).expect("failed to read fit2d.mps");
        let slow = LpProblem::parse_mps(&slow_input).expect("failed to parse fit2d.mps");
        let fast = LpProblem::parse("min\nobj: x\nst\nc1: x >= 1\nend").expect("failed to parse tiny LP");

        // Stagger the fast solve across the slow solve's window so it finishes
        // (and cleans up its log) while the slow solve is still running.
        for stagger_ms in [0u64, 10, 25, 50, 75] {
            std::thread::scope(|scope| {
                let slow_handle = scope.spawn(|| solve_problem(&slow));
                let fast_ref = &fast;
                let fast_handle = scope.spawn(move || {
                    std::thread::sleep(Duration::from_millis(stagger_ms));
                    solve_problem(fast_ref)
                });
                fast_handle
                    .join()
                    .expect("fast solver thread panicked")
                    .unwrap_or_else(|e| panic!("fast solve failed at stagger {stagger_ms}ms: {e}"));
                slow_handle
                    .join()
                    .expect("slow solver thread panicked")
                    .unwrap_or_else(|e| panic!("slow solve failed at stagger {stagger_ms}ms: {e}"));
            });
        }
    }

    #[test]
    fn test_enlight4_infeasible() {
        let mut file_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        file_path.push("../rust/resources/enlight4.mps");
        let input = std::fs::read_to_string(&file_path).expect("failed to read enlight4.mps");

        let problem = LpProblem::parse_mps(&input).expect("failed to parse enlight4.mps");

        let result = solve_problem(&problem).expect("solver should not error");
        assert_eq!(
            result.status, "Infeasible",
            "enlight4 should be infeasible when integers are correctly applied, got: {}",
            result.status
        );
    }

    #[test]
    fn test_write_result_csv() {
        let result = SolveResult {
            status: "Optimal".to_owned(),
            objective_value: Some(12.5),
            variables: vec![("x".to_owned(), 1.5), ("y".to_owned(), 0.0)],
            reduced_costs: vec![("x".to_owned(), 0.0), ("y".to_owned(), -2.0)],
            shadow_prices: vec![("c1".to_owned(), 3.0)],
            row_values: vec![("c1".to_owned(), 4.0)],
            build_time: std::time::Duration::ZERO,
            solve_time: std::time::Duration::ZERO,
            extract_time: std::time::Duration::ZERO,
            solver_log: String::new(),
            skipped_sos: 0,
        };

        let dir = std::env::temp_dir().join(format!("lp_diff_csv_test_{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("failed to create temp dir");

        let (var_file, con_file) = write_result_csv(&result, &dir).expect("write_result_csv should succeed");
        let vars = std::fs::read_to_string(dir.join(&var_file)).expect("failed to read variables CSV");
        let cons = std::fs::read_to_string(dir.join(&con_file)).expect("failed to read constraints CSV");
        std::fs::remove_dir_all(&dir).expect("failed to remove temp dir");

        assert_eq!(vars, "name,value,reduced_cost\nx,1.5,0\ny,0,-2\n", "variable CSV mismatch");
        assert_eq!(cons, "name,activity,shadow_price\nc1,4,3\n", "constraint CSV mismatch");
    }
}
