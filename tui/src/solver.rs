//! `HiGHS` solver integration — converts an `LpProblemOwned` to a `HiGHS` problem and solves it.

use std::collections::BTreeMap;
use std::path::Path;
use std::time::Instant;

use lp_parser_rs::model::{ComparisonOp, ConstraintOwned, VariableType};
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

/// Comparison of two solve results.
#[derive(Debug, Clone)]
pub struct SolveDiffResult {
    pub file1_label: String,
    pub file2_label: String,
    pub result1: SolveResult,
    pub result2: SolveResult,
    pub variable_diff: Vec<VarDiffRow>,
    pub constraint_diff: Vec<ConstraintDiffRow>,
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
    SolveDiffResult { file1_label, file2_label, result1, result2, variable_diff, constraint_diff }
}

fn diff_variables(r1: &SolveResult, r2: &SolveResult) -> Vec<VarDiffRow> {
    debug_assert_eq!(r1.variables.len(), r1.reduced_costs.len(), "variables and reduced_costs must have equal length for result 1");
    debug_assert_eq!(r2.variables.len(), r2.reduced_costs.len(), "variables and reduced_costs must have equal length for result 2");

    // Both variable lists are already sorted by name (from BTreeMap iteration in build_highs_model).
    // Use a merge-join to avoid HashMap allocation.
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

    // Both row_values lists are already sorted by name (from sorted constraint iteration).
    // Use a merge-join to avoid HashMap allocation.
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
    let owned = problem.to_owned();

    solve_problem(&owned)
}

/// Intermediate model built from an `LpProblemOwned` before solving.
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

/// Build a `HiGHS` `RowProblem` from an `LpProblemOwned`.
fn build_highs_model(problem: &lp_parser_rs::problem::LpProblemOwned) -> BuiltModel {
    debug_assert!(!problem.variables.is_empty(), "cannot build a HiGHS model with no variables");

    let variable_names: Vec<&String> = {
        let mut names: Vec<&String> = problem.variables.keys().collect();
        names.sort();
        names
    };

    let variable_index: BTreeMap<&str, usize> = variable_names.iter().enumerate().map(|(i, name)| (name.as_str(), i)).collect();

    let objective_coefficients: BTreeMap<String, f64> = {
        let mut map = BTreeMap::new();
        let mut objective_names: Vec<&String> = problem.objectives.keys().collect();
        objective_names.sort();
        if let Some(objective_name) = objective_names.first()
            && let Some(objective) = problem.objectives.get(*objective_name)
        {
            for coefficient in &objective.coefficients {
                map.insert(coefficient.name.clone(), coefficient.value);
            }
        }
        map
    };

    let mut row_problem = highs::RowProblem::new();
    let mut columns = Vec::with_capacity(variable_names.len());

    for name in &variable_names {
        let objective_coefficient = objective_coefficients.get(name.as_str()).copied().unwrap_or(0.0);
        let variable = problem.variables.get(name.as_str());

        let (is_integer, lower, upper) = match variable.map(|v| &v.var_type) {
            Some(VariableType::Binary) => (true, 0.0, 1.0),
            Some(VariableType::Integer) => (true, 0.0, f64::INFINITY),
            Some(VariableType::Free) => (false, f64::NEG_INFINITY, f64::INFINITY),
            Some(VariableType::LowerBound(lb)) => (false, *lb, f64::INFINITY),
            Some(VariableType::UpperBound(ub)) => (false, 0.0, *ub),
            Some(VariableType::DoubleBound(lb, ub)) => (false, *lb, *ub),
            Some(VariableType::General | VariableType::SemiContinuous | VariableType::SOS) | None => (false, 0.0, f64::INFINITY),
        };

        let col = row_problem.add_column_with_integrality(objective_coefficient, lower..=upper, is_integer);
        columns.push(col);
    }

    let mut constraint_names: Vec<&String> = problem.constraints.keys().collect();
    constraint_names.sort();

    let mut skipped_sos: usize = 0;

    for constraint_name in &constraint_names {
        let Some(constraint) = problem.constraints.get(constraint_name.as_str()) else {
            continue;
        };

        match constraint {
            ConstraintOwned::Standard { coefficients, operator, rhs, .. } => {
                let row_factors: Vec<(highs::Col, f64)> =
                    coefficients.iter().filter_map(|c| variable_index.get(c.name.as_str()).map(|&idx| (columns[idx], c.value))).collect();

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
            }
            ConstraintOwned::SOS { .. } => {
                skipped_sos += 1;
            }
        }
    }

    let row_constraint_names: Vec<String> = constraint_names
        .iter()
        .filter(|name| problem.constraints.get(name.as_str()).is_some_and(|c| matches!(c, ConstraintOwned::Standard { .. })))
        .map(|name| (*name).clone())
        .collect();

    let sense = match problem.sense {
        lp_parser_rs::model::Sense::Minimize => highs::Sense::Minimise,
        lp_parser_rs::model::Sense::Maximize => highs::Sense::Maximise,
    };

    debug_assert_eq!(columns.len(), variable_names.len(), "column count must match variable count");

    BuiltModel {
        row_problem,
        variable_names: variable_names.into_iter().cloned().collect(),
        objective_coefficients,
        row_constraint_names,
        skipped_sos,
        sense,
    }
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

/// Convert an `LpProblemOwned` to a `HiGHS` `RowProblem` and solve it.
fn solve_problem(problem: &lp_parser_rs::problem::LpProblemOwned) -> Result<SolveResult, String> {
    debug_assert!(!problem.variables.is_empty(), "cannot solve a problem with no variables");

    let model = build_highs_model(problem);

    let log_file = tempfile::NamedTempFile::new().map_err(|e| format!("failed to create solver log temp file: {e}"))?;
    let log_path = log_file.path().to_owned();

    // Destructure to separate the consumed `row_problem` from the
    // metadata fields that `extract_solution` needs by reference.
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
