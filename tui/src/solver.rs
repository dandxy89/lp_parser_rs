//! `HiGHS` solver integration — converts an `LpProblem` to a `HiGHS` problem and solves it.

use std::collections::HashMap;
use std::error::Error;
use std::fmt::Write as _;
use std::path::Path;
use std::time::Instant;

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
    /// Wall-clock time to build the HiGHS `RowProblem` from `LpProblem`.
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
    pub total: usize,
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
    let mut counts = DiffCounts { total: rows.len(), added: 0, removed: 0, modified: 0 };
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
    let mut counts = DiffCounts { total: rows.len(), added: 0, removed: 0, modified: 0 };
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

fn diff_variables(r1: &SolveResult, r2: &SolveResult, threshold: f64) -> Vec<VarDiffRow> {
    debug_assert_eq!(r1.variables.len(), r1.reduced_costs.len(), "variables and reduced_costs must have equal length for result 1");
    debug_assert_eq!(r2.variables.len(), r2.reduced_costs.len(), "variables and reduced_costs must have equal length for result 2");

    let mut i: usize = 0;
    let mut j: usize = 0;
    let mut rows = Vec::new();

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

fn diff_constraints(r1: &SolveResult, r2: &SolveResult, threshold: f64) -> Vec<ConstraintDiffRow> {
    debug_assert_eq!(r1.row_values.len(), r1.shadow_prices.len(), "row_values and shadow_prices must have equal length for result 1");
    debug_assert_eq!(r2.row_values.len(), r2.shadow_prices.len(), "row_values and shadow_prices must have equal length for result 2");

    let mut i: usize = 0;
    let mut j: usize = 0;
    let mut rows = Vec::new();

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

/// Build a `HiGHS` `RowProblem` from an `LpProblem`.
fn build_highs_model(problem: &LpProblem) -> BuiltModel {
    debug_assert!(!problem.variables.is_empty(), "cannot build a HiGHS model with no variables");

    // Sort variable NameIds by resolved name for deterministic ordering.
    let mut sorted_var_ids: Vec<NameId> = problem.variables.keys().copied().collect();
    sorted_var_ids.sort_by(|a, b| problem.resolve(*a).cmp(problem.resolve(*b)));

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

        let (is_integer, lower, upper) = match variable.map(|v| &v.var_type) {
            Some(VariableType::Binary) => (true, 0.0, 1.0),
            Some(VariableType::Integer) => (true, 0.0, f64::INFINITY),
            Some(VariableType::Free | VariableType::General | VariableType::SemiContinuous | VariableType::SOS) | None => {
                (false, 0.0, f64::INFINITY)
            }
            Some(VariableType::LowerBound(lb)) => (false, *lb, f64::INFINITY),
            Some(VariableType::UpperBound(ub)) => (false, 0.0, *ub),
            Some(VariableType::DoubleBound(lb, ub)) => (false, *lb, *ub),
        };

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

/// Convert an `LpProblem` to a `HiGHS` `RowProblem` and solve it.
pub fn solve_problem(problem: &LpProblem) -> Result<SolveResult, String> {
    debug_assert!(!problem.variables.is_empty(), "cannot solve a problem with no variables");

    let build_start = Instant::now();
    let model = build_highs_model(problem);
    let build_time = build_start.elapsed();

    let log_file = tempfile::NamedTempFile::new().map_err(|e| format!("failed to create solver log temp file: {e}"))?;
    let log_path = log_file.path().to_owned();

    let BuiltModel { row_problem, sense, variable_names, sorted_var_ids, objective_coefficients, row_constraint_names, skipped_sos } =
        model;

    let metadata = SolveMetadata { variable_names, sorted_var_ids, objective_coefficients, row_constraint_names, skipped_sos };

    let mut highs_model = row_problem.optimise(sense);
    highs_model.set_option("output_flag", true);
    highs_model.set_option("log_file", log_path.to_str().ok_or_else(|| "temp file path is not valid UTF-8".to_owned())?);

    let solve_start = Instant::now();
    let solved = highs_model.solve();
    let solve_time = solve_start.elapsed();

    let solver_log = std::fs::read_to_string(&log_path).map_err(|e| format!("failed to read solver log: {e}"))?;

    let extract_start = Instant::now();
    let mut result = extract_solution(&metadata, &solved, build_time, solve_time, solver_log);
    result.extract_time = extract_start.elapsed();

    Ok(result)
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
