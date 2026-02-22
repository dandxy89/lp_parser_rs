//! `HiGHS` solver integration — converts an `LpProblem` to a `HiGHS` problem and solves it.

use std::collections::BTreeMap;
use std::error::Error;
use std::fmt::Write as _;
use std::path::Path;
use std::time::Instant;

use lp_parser_rs::model::{ComparisonOp, Constraint, VariableType};
use lp_parser_rs::parser::parse_file;
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
    /// Wall-clock solve time.
    pub solve_time: std::time::Duration,
    /// Captured solver log output (presolve info, iteration counts, etc.).
    pub solver_log: String,
    /// Number of SOS constraints that were skipped (not supported by `RowProblem`).
    pub skipped_sos: usize,
}

/// Tolerance for floating-point comparison in diff results.
const EPSILON: f64 = 1e-10;

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
}

/// A single variable row in a solve diff comparison.
#[derive(Debug, Clone)]
pub struct VarDiffRow {
    pub name: String,
    pub val1: Option<f64>,
    pub val2: Option<f64>,
    pub reduced_cost1: Option<f64>,
    pub reduced_cost2: Option<f64>,
    pub changed: bool,
}

/// A single constraint row in a solve diff comparison.
#[derive(Debug, Clone)]
pub struct ConstraintDiffRow {
    pub name: String,
    pub activity1: Option<f64>,
    pub activity2: Option<f64>,
    pub shadow_price1: Option<f64>,
    pub shadow_price2: Option<f64>,
    pub changed: bool,
}

/// Build a `SolveDiffResult` by comparing two solve results.
///
/// Variables and constraints are matched by name. Rows present in only one result
/// are included with `None` on the other side and marked as changed.
pub fn diff_results(file1_label: String, file2_label: String, result1: SolveResult, result2: SolveResult) -> SolveDiffResult {
    let variable_diff = diff_variables(&result1, &result2);
    let constraint_diff = diff_constraints(&result1, &result2);
    let variable_counts = count_var_diffs(&variable_diff);
    let constraint_counts = count_constraint_diffs_from_rows(&constraint_diff);
    SolveDiffResult { file1_label, file2_label, result1, result2, variable_diff, constraint_diff, variable_counts, constraint_counts }
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

fn diff_variables(r1: &SolveResult, r2: &SolveResult) -> Vec<VarDiffRow> {
    debug_assert_eq!(r1.variables.len(), r1.reduced_costs.len(), "variables and reduced_costs must have equal length for result 1");
    debug_assert_eq!(r2.variables.len(), r2.reduced_costs.len(), "variables and reduced_costs must have equal length for result 2");

    let mut i = 0;
    let mut j = 0;
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
                let (name, val) = &r1.variables[i];
                let rc1 = r1.reduced_costs.get(i).map(|(_, v)| *v);
                i += 1;
                VarDiffRow { name: name.clone(), val1: Some(*val), val2: None, reduced_cost1: rc1, reduced_cost2: None, changed: true }
            }
            std::cmp::Ordering::Greater => {
                let (name, val) = &r2.variables[j];
                let rc2 = r2.reduced_costs.get(j).map(|(_, v)| *v);
                j += 1;
                VarDiffRow { name: name.clone(), val1: None, val2: Some(*val), reduced_cost1: None, reduced_cost2: rc2, changed: true }
            }
            std::cmp::Ordering::Equal => {
                let (name, val1) = &r1.variables[i];
                let val2 = r2.variables[j].1;
                let rc1 = r1.reduced_costs.get(i).map(|(_, v)| *v);
                let rc2 = r2.reduced_costs.get(j).map(|(_, v)| *v);
                let changed = (*val1 - val2).abs() > EPSILON || opt_diff(rc1, rc2);
                i += 1;
                j += 1;
                VarDiffRow { name: name.clone(), val1: Some(*val1), val2: Some(val2), reduced_cost1: rc1, reduced_cost2: rc2, changed }
            }
        };
        rows.push(row);
    }
    rows
}

fn diff_constraints(r1: &SolveResult, r2: &SolveResult) -> Vec<ConstraintDiffRow> {
    debug_assert_eq!(r1.row_values.len(), r1.shadow_prices.len(), "row_values and shadow_prices must have equal length for result 1");
    debug_assert_eq!(r2.row_values.len(), r2.shadow_prices.len(), "row_values and shadow_prices must have equal length for result 2");

    let mut i = 0;
    let mut j = 0;
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
                let (name, activity) = &r1.row_values[i];
                let sp = r1.shadow_prices[i].1;
                i += 1;
                ConstraintDiffRow {
                    name: name.clone(),
                    activity1: Some(*activity),
                    activity2: None,
                    shadow_price1: Some(sp),
                    shadow_price2: None,
                    changed: true,
                }
            }
            std::cmp::Ordering::Greater => {
                let (name, activity) = &r2.row_values[j];
                let sp = r2.shadow_prices[j].1;
                j += 1;
                ConstraintDiffRow {
                    name: name.clone(),
                    activity1: None,
                    activity2: Some(*activity),
                    shadow_price1: None,
                    shadow_price2: Some(sp),
                    changed: true,
                }
            }
            std::cmp::Ordering::Equal => {
                let (name, a1) = &r1.row_values[i];
                let a2 = r2.row_values[j].1;
                let sp1 = r1.shadow_prices[i].1;
                let sp2 = r2.shadow_prices[j].1;
                let changed = (*a1 - a2).abs() > EPSILON || (sp1 - sp2).abs() > EPSILON;
                i += 1;
                j += 1;
                ConstraintDiffRow {
                    name: name.clone(),
                    activity1: Some(*a1),
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

/// Return `true` if two optional f64 values differ beyond epsilon.
fn opt_diff(a: Option<f64>, b: Option<f64>) -> bool {
    match (a, b) {
        (Some(x), Some(y)) => (x - y).abs() > EPSILON,
        (None, None) => false,
        _ => true,
    }
}

/// Parse and solve an LP file, returning a `SolveResult` or an error message.
pub fn solve_file(path: &Path) -> Result<SolveResult, String> {
    let content = parse_file(path).map_err(|e| format!("failed to read '{}': {e}", path.display()))?;
    let problem = LpProblem::parse(&content).map_err(|e| format!("failed to parse '{}': {e}", path.display()))?;

    solve_problem(&problem)
}

/// Intermediate model built from an `LpProblem` before solving.
struct BuiltModel {
    row_problem: highs::RowProblem,
    variable_names: Vec<String>,
    objective_coefficients: BTreeMap<String, f64>,
    row_constraint_names: Vec<String>,
    skipped_sos: usize,
    sense: highs::Sense,
}

/// Metadata from the built model needed for solution extraction (after
/// `row_problem` has been consumed by `optimise`).
struct SolveMetadata {
    variable_names: Vec<String>,
    objective_coefficients: BTreeMap<String, f64>,
    row_constraint_names: Vec<String>,
    skipped_sos: usize,
}

/// Build a `HiGHS` `RowProblem` from an `LpProblem`.
fn build_highs_model(problem: &LpProblem) -> BuiltModel {
    debug_assert!(!problem.variables.is_empty(), "cannot build a HiGHS model with no variables");

    // Resolve variable names and sort them for deterministic ordering.
    let mut variable_names: Vec<String> = problem.variables.keys().map(|id| problem.resolve(*id).to_string()).collect();
    variable_names.sort();

    let variable_index: BTreeMap<&str, usize> = variable_names.iter().enumerate().map(|(i, name)| (name.as_str(), i)).collect();

    let objective_coefficients: BTreeMap<String, f64> = {
        let mut map = BTreeMap::new();
        // Sort objective names and take the first one (primary objective).
        let mut obj_names: Vec<(&lp_parser_rs::interner::NameId, &lp_parser_rs::model::Objective)> = problem.objectives.iter().collect();
        obj_names.sort_by_key(|(id, _)| problem.resolve(**id));
        if let Some((_, objective)) = obj_names.first() {
            for coefficient in &objective.coefficients {
                map.insert(problem.resolve(coefficient.name).to_string(), coefficient.value);
            }
        }
        map
    };

    let mut row_problem = highs::RowProblem::new();
    let mut columns = Vec::with_capacity(variable_names.len());

    for name in &variable_names {
        let objective_coefficient = objective_coefficients.get(name.as_str()).copied().unwrap_or(0.0);
        let var_id = problem.get_name_id(name);
        let variable = var_id.and_then(|id| problem.variables.get(&id));

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

    for (name_id, constraint) in &sorted_constraints {
        let constraint_name = problem.resolve(**name_id);

        match constraint {
            Constraint::Standard { coefficients, operator, rhs, .. } => {
                let row_factors: Vec<(highs::Col, f64)> = coefficients
                    .iter()
                    .filter_map(|c| {
                        let var_name = problem.resolve(c.name);
                        variable_index.get(var_name).map(|&idx| (columns[idx], c.value))
                    })
                    .collect();

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

    BuiltModel { row_problem, variable_names, objective_coefficients, row_constraint_names, skipped_sos, sense }
}

/// Extract the solution from a solved `HiGHS` model into a `SolveResult`.
fn extract_solution(
    metadata: &SolveMetadata,
    solved: &highs::SolvedModel,
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
                        let coefficient = metadata.objective_coefficients.get(metadata.variable_names[i].as_str()).copied().unwrap_or(0.0);
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
        solve_time,
        solver_log,
        skipped_sos: metadata.skipped_sos,
    }
}

/// Convert an `LpProblem` to a `HiGHS` `RowProblem` and solve it.
fn solve_problem(problem: &LpProblem) -> Result<SolveResult, String> {
    debug_assert!(!problem.variables.is_empty(), "cannot solve a problem with no variables");

    let model = build_highs_model(problem);

    let log_file = tempfile::NamedTempFile::new().map_err(|e| format!("failed to create solver log temp file: {e}"))?;
    let log_path = log_file.path().to_owned();

    let BuiltModel { row_problem, sense, variable_names, objective_coefficients, row_constraint_names, skipped_sos } = model;

    let metadata = SolveMetadata { variable_names, objective_coefficients, row_constraint_names, skipped_sos };

    let mut highs_model = row_problem.optimise(sense);
    highs_model.set_option("output_flag", true);
    highs_model.set_option("log_file", log_path.to_str().ok_or_else(|| "temp file path is not valid UTF-8".to_owned())?);

    let start = Instant::now();
    let solved = highs_model.solve();
    let solve_time = start.elapsed();

    let solver_log = std::fs::read_to_string(&log_path).map_err(|e| format!("failed to read solver log: {e}"))?;

    Ok(extract_solution(&metadata, &solved, solve_time, solver_log))
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

            write_opt_f64_to_buf(&mut buf_v1, row.val1);
            write_opt_f64_to_buf(&mut buf_v2, row.val2);
            buf_delta.clear();
            if let (Some(v1), Some(v2)) = (row.val1, row.val2) {
                write!(buf_delta, "{}", v2 - v1).expect("writing f64 to String cannot fail");
            }
            write_opt_f64_to_buf(&mut buf_rc1, row.reduced_cost1);
            write_opt_f64_to_buf(&mut buf_rc2, row.reduced_cost2);

            wtr.write_record([&row.name, &buf_v1, &buf_v2, &buf_delta, &buf_rc1, &buf_rc2])?;
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

            write_opt_f64_to_buf(&mut buf_a1, row.activity1);
            write_opt_f64_to_buf(&mut buf_a2, row.activity2);
            write_opt_f64_to_buf(&mut buf_sp1, row.shadow_price1);
            write_opt_f64_to_buf(&mut buf_sp2, row.shadow_price2);

            wtr.write_record([&row.name, &buf_a1, &buf_a2, &buf_sp1, &buf_sp2])?;
        }
        wtr.flush()?;
    }

    Ok((var_filename, con_filename))
}
