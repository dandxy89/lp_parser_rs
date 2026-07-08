//! `HiGHS` solver integration — converts an `LpProblem` to a `HiGHS` problem and solves it.

use std::collections::HashMap;
use std::error::Error;
use std::fmt::Write as _;
use std::path::Path;
use std::time::{Duration, Instant};

use lp_parser_rs::interner::NameId;
use lp_parser_rs::model::{ComparisonOp, Constraint, VariableType};
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
struct BuiltModel {
    row_problem: highs::RowProblem,
    variable_names: Vec<String>,
    sorted_var_ids: Vec<NameId>,
    objective_coefficients: HashMap<NameId, f64>,
    row_constraint_names: Vec<String>,
    skipped_sos: usize,
    sense: highs::Sense,
}

/// Metadata from the built model needed for solution extraction (after
/// `row_problem` has been consumed by `optimise`).
struct SolveMetadata {
    variable_names: Vec<String>,
    sorted_var_ids: Vec<NameId>,
    objective_coefficients: HashMap<NameId, f64>,
    row_constraint_names: Vec<String>,
    skipped_sos: usize,
}

/// Map a variable's declared type to `(is_integer, lower, upper)` bounds for `HiGHS`.
const fn variable_bounds(var_type: Option<&VariableType>) -> (bool, f64, f64) {
    match var_type {
        Some(VariableType::Binary) => (true, 0.0, 1.0),
        Some(VariableType::Integer | VariableType::General) => (true, 0.0, f64::INFINITY),
        Some(VariableType::Free | VariableType::SemiContinuous | VariableType::SOS) | None => (false, 0.0, f64::INFINITY),
        Some(VariableType::LowerBound(lb)) => (false, *lb, f64::INFINITY),
        Some(VariableType::UpperBound(ub)) => (false, 0.0, *ub),
        Some(VariableType::DoubleBound(lb, ub)) => (false, *lb, *ub),
    }
}

/// Sort variable `NameId`s by resolved name for deterministic column ordering.
fn sorted_variable_ids(problem: &LpProblem) -> Vec<NameId> {
    let mut sorted_var_ids: Vec<NameId> = problem.variables.keys().copied().collect();
    sorted_var_ids.sort_by(|a, b| problem.resolve(*a).cmp(problem.resolve(*b)));
    sorted_var_ids
}

/// Build a `HiGHS` `RowProblem` from an `LpProblem`.
fn build_highs_model(problem: &LpProblem) -> BuiltModel {
    debug_assert!(!problem.variables.is_empty(), "cannot build a HiGHS model with no variables");

    // Sort variable NameIds by resolved name for deterministic ordering.
    let sorted_var_ids = sorted_variable_ids(problem);

    let variable_names: Vec<String> = sorted_var_ids.iter().map(|id| problem.resolve(*id).to_string()).collect();

    let variable_index: HashMap<NameId, usize> = {
        let mut map = HashMap::with_capacity(sorted_var_ids.len());
        map.extend(sorted_var_ids.iter().enumerate().map(|(i, &id)| (id, i)));
        map
    };

    let objective_coefficients: HashMap<NameId, f64> = {
        // Find the primary objective (alphabetically first by resolved name).
        let primary = problem.objectives.iter().min_by_key(|(id, _)| problem.resolve(**id));
        if let Some((_, objective)) = primary {
            let mut map = HashMap::with_capacity(objective.coefficients.len());
            for coefficient in &objective.coefficients {
                map.insert(coefficient.name, coefficient.value);
            }
            map
        } else {
            HashMap::new()
        }
    };

    let mut row_problem = highs::RowProblem::new();
    let mut columns = Vec::with_capacity(sorted_var_ids.len());

    for &var_id in &sorted_var_ids {
        let objective_coefficient = objective_coefficients.get(&var_id).copied().unwrap_or(0.0);
        let variable = problem.variables.get(&var_id);

        let (is_integer, lower, upper) = variable_bounds(variable.map(|v| &v.var_type));

        let col = row_problem.add_column_with_integrality(objective_coefficient, lower..=upper, is_integer);
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
        }
    }

    let sense = match problem.sense {
        lp_parser_rs::model::Sense::Minimize => highs::Sense::Minimise,
        lp_parser_rs::model::Sense::Maximize => highs::Sense::Maximise,
    };

    debug_assert_eq!(columns.len(), variable_names.len(), "column count must match variable count");

    BuiltModel { row_problem, variable_names, sorted_var_ids, objective_coefficients, row_constraint_names, skipped_sos, sense }
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

/// Convert an `LpProblem` to a `HiGHS` `RowProblem` and solve it.
pub fn solve_problem(problem: &LpProblem) -> Result<SolveResult, String> {
    debug_assert!(!problem.variables.is_empty(), "cannot solve a problem with no variables");

    let build_start = Instant::now();
    let model = build_highs_model(problem);
    let build_time = build_start.elapsed();

    // ponytail: pid+sequence-named temp file + explicit cleanup instead of the
    // tempfile crate. The sequence number keeps concurrent solves in one process
    // ("Solve both" runs two solver threads) from clobbering each other's log.
    let log_seq = SOLVE_LOG_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let log_path = std::env::temp_dir().join(format!("lp_diff_solver_{}_{log_seq}.log", std::process::id()));

    let BuiltModel { row_problem, sense, variable_names, sorted_var_ids, objective_coefficients, row_constraint_names, skipped_sos } =
        model;

    let metadata = SolveMetadata { variable_names, sorted_var_ids, objective_coefficients, row_constraint_names, skipped_sos };

    let mut highs_model = row_problem.optimise(sense);
    highs_model.set_option("output_flag", true);
    highs_model.set_option("log_file", log_path.to_str().ok_or_else(|| "temp file path is not valid UTF-8".to_owned())?);

    let solve_start = Instant::now();
    let solved = highs_model.solve();
    let solve_time = solve_start.elapsed();

    let mut solver_log = std::fs::read_to_string(&log_path).map_err(|e| format!("failed to read solver log: {e}"))?;
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

/// Return `true` if a solve status string (as produced by `extract_solution`,
/// i.e. the `Debug` form of `highs::HighsModelStatus`) indicates infeasibility.
pub fn status_is_infeasible(status: &str) -> bool {
    status == "Infeasible" || status == "UnboundedOrInfeasible"
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
        let (_, mut lower, mut upper) = variable_bounds(problem.variables.get(&var_id).map(|v| &v.var_type));
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

    let mut highs_model = row_problem.optimise(highs::Sense::Minimise);
    // Suppress solver output: the diagnosis runs while the TUI owns the terminal.
    highs_model.set_option("output_flag", false);

    let solved = highs_model.solve();
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

/// Write an `Option<f64>` into a reusable string buffer (cleared first), leaving it empty for `None`.
#[inline]
fn write_opt_f64_to_buf(buf: &mut String, value: Option<f64>) {
    buf.clear();
    if let Some(v) = value {
        debug_assert!(v.is_finite(), "write_opt_f64_to_buf called with non-finite value: {v}");
        write!(buf, "{v}").expect("writing f64 to String cannot fail");
    }
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

    let ts = chrono::Local::now().format("%Y%m%d_%H%M%S");
    let var_filename = format!("variable_diff_{ts}.csv");
    let con_filename = format!("constraint_diff_{ts}.csv");

    // Variable diff — one String buffer per field, reused across rows.
    {
        let mut wtr = csv::Writer::from_path(dir.join(&var_filename))?;
        wtr.write_record(["name", "value_1", "value_2", "delta", "reduced_cost_1", "reduced_cost_2"])?;

        let mut buf_v1 = String::with_capacity(24);
        let mut buf_v2 = String::with_capacity(24);
        let mut buf_delta = String::with_capacity(24);
        let mut buf_rc1 = String::with_capacity(24);
        let mut buf_rc2 = String::with_capacity(24);

        for row in &diff.variable_diff {
            if row.val1.is_none() && row.val2.is_none() {
                continue;
            }

            let name = row.name(&diff.result1, &diff.result2);
            write_opt_f64_to_buf(&mut buf_v1, row.val1);
            write_opt_f64_to_buf(&mut buf_v2, row.val2);
            buf_delta.clear();
            if let (Some(v1), Some(v2)) = (row.val1, row.val2) {
                write!(buf_delta, "{}", v2 - v1).expect("writing f64 to String cannot fail");
            }
            write_opt_f64_to_buf(&mut buf_rc1, row.reduced_cost1);
            write_opt_f64_to_buf(&mut buf_rc2, row.reduced_cost2);

            wtr.write_record([name, &buf_v1, &buf_v2, &buf_delta, &buf_rc1, &buf_rc2])?;
        }
        wtr.flush()?;
    }

    // Constraint diff — one String buffer per field, reused across rows.
    {
        let mut wtr = csv::Writer::from_path(dir.join(&con_filename))?;
        wtr.write_record(["name", "activity_1", "activity_2", "shadow_price_1", "shadow_price_2"])?;

        let mut buf_a1 = String::with_capacity(24);
        let mut buf_a2 = String::with_capacity(24);
        let mut buf_sp1 = String::with_capacity(24);
        let mut buf_sp2 = String::with_capacity(24);

        for row in &diff.constraint_diff {
            if row.activity1.is_none() && row.activity2.is_none() {
                continue;
            }

            let name = row.name(&diff.result1, &diff.result2);
            write_opt_f64_to_buf(&mut buf_a1, row.activity1);
            write_opt_f64_to_buf(&mut buf_a2, row.activity2);
            write_opt_f64_to_buf(&mut buf_sp1, row.shadow_price1);
            write_opt_f64_to_buf(&mut buf_sp2, row.shadow_price2);

            wtr.write_record([name, &buf_a1, &buf_a2, &buf_sp1, &buf_sp2])?;
        }
        wtr.flush()?;
    }

    Ok((var_filename, con_filename))
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
