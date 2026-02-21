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
    /// Wall-clock solve time.
    pub solve_time: std::time::Duration,
    /// Captured solver log output (presolve info, iteration counts, etc.).
    pub solver_log: String,
    /// Number of SOS constraints that were skipped (not supported by `RowProblem`).
    pub skipped_sos: usize,
}

/// Parse and solve an LP file, returning a `SolveResult` or an error message.
pub fn solve_file(path: &Path) -> Result<SolveResult, String> {
    let content = parse_file(path).map_err(|e| format!("failed to read '{}': {e}", path.display()))?;
    let problem = LpProblem::parse(&content).map_err(|e| format!("failed to parse '{}': {e}", path.display()))?;
    let owned = problem.to_owned();

    solve_problem(&owned)
}

/// Convert an `LpProblemOwned` to a `HiGHS` `RowProblem` and solve it.
fn solve_problem(problem: &lp_parser_rs::problem::LpProblemOwned) -> Result<SolveResult, String> {
    // Collect all variable names in sorted order for deterministic column indices.
    let var_names: Vec<&String> = {
        let mut names: Vec<&String> = problem.variables.keys().collect();
        names.sort();
        names
    };

    // Build a name→index map.
    let var_index: BTreeMap<&str, usize> = var_names.iter().enumerate().map(|(i, name)| (name.as_str(), i)).collect();

    // Pick the first objective (by sorted name) for coefficients.
    let obj_coeffs: BTreeMap<&str, f64> = {
        let mut map = BTreeMap::new();
        let mut obj_names: Vec<&String> = problem.objectives.keys().collect();
        obj_names.sort();
        if let Some(obj_name) = obj_names.first()
            && let Some(obj) = problem.objectives.get(*obj_name)
        {
            for c in &obj.coefficients {
                map.insert(c.name.as_str(), c.value);
            }
        }
        map
    };

    let mut pb = highs::RowProblem::new();
    let mut cols = Vec::with_capacity(var_names.len());

    for name in &var_names {
        let obj_coeff = obj_coeffs.get(name.as_str()).copied().unwrap_or(0.0);
        let var = problem.variables.get(name.as_str());

        let (is_integer, lb, ub) = match var.map(|v| &v.var_type) {
            Some(VariableType::Binary) => (true, 0.0, 1.0),
            Some(VariableType::Integer) => (true, 0.0, f64::INFINITY),
            Some(VariableType::Free) => (false, f64::NEG_INFINITY, f64::INFINITY),
            Some(VariableType::LowerBound(l)) => (false, *l, f64::INFINITY),
            Some(VariableType::UpperBound(u)) => (false, 0.0, *u),
            Some(VariableType::DoubleBound(l, u)) => (false, *l, *u),
            Some(VariableType::General | VariableType::SemiContinuous | VariableType::SOS) | None => (false, 0.0, f64::INFINITY),
        };

        let col = pb.add_column_with_integrality(obj_coeff, lb..=ub, is_integer);
        cols.push(col);
    }

    // Add constraints.
    let mut constraint_names: Vec<&String> = problem.constraints.keys().collect();
    constraint_names.sort();

    let mut skipped_sos: usize = 0;

    for con_name in &constraint_names {
        let Some(constraint) = problem.constraints.get(con_name.as_str()) else {
            continue;
        };

        match constraint {
            ConstraintOwned::Standard { coefficients, operator, rhs, .. } => {
                let row_factors: Vec<(highs::Col, f64)> =
                    coefficients.iter().filter_map(|c| var_index.get(c.name.as_str()).map(|&idx| (cols[idx], c.value))).collect();

                match operator {
                    ComparisonOp::LTE | ComparisonOp::LT => pb.add_row(..=*rhs, &row_factors),
                    ComparisonOp::GTE | ComparisonOp::GT => pb.add_row(*rhs.., &row_factors),
                    ComparisonOp::EQ => pb.add_row(*rhs..=*rhs, &row_factors),
                }
            }
            ConstraintOwned::SOS { .. } => {
                skipped_sos += 1;
            }
        }
    }

    let sense = match problem.sense {
        lp_parser_rs::model::Sense::Minimize => highs::Sense::Minimise,
        lp_parser_rs::model::Sense::Maximize => highs::Sense::Maximise,
    };

    // Create a temp file to capture HiGHS solver log output.
    // `make_quiet()` is called internally by the crate, so we re-enable file logging.
    let log_file = tempfile::NamedTempFile::new().map_err(|e| format!("failed to create solver log temp file: {e}"))?;
    let log_path = log_file.path().to_owned();

    let mut model = pb.optimise(sense);
    model.set_option("output_flag", true);
    model.set_option("log_file", log_path.to_str().ok_or_else(|| "temp file path is not valid UTF-8".to_owned())?);

    let start = Instant::now();
    let solved = model.solve();
    let solve_time = start.elapsed();

    let solver_log = std::fs::read_to_string(&log_path).unwrap_or_else(|e| format!("(failed to read solver log: {e})"));

    let status = format!("{:?}", solved.status());

    let (objective_value, variables) = match solved.status() {
        highs::HighsModelStatus::Optimal | highs::HighsModelStatus::ObjectiveBound => {
            let solution = solved.get_solution();
            let obj_val = Some(
                solution
                    .columns()
                    .iter()
                    .enumerate()
                    .map(|(i, &val)| {
                        let obj_c = obj_coeffs.get(var_names[i].as_str()).copied().unwrap_or(0.0);
                        val * obj_c
                    })
                    .sum::<f64>(),
            );
            let vars: Vec<(String, f64)> =
                var_names.iter().zip(solution.columns().iter()).map(|(name, &val)| ((*name).clone(), val)).collect();
            (obj_val, vars)
        }
        _ => (None, Vec::new()),
    };

    Ok(SolveResult { status, objective_value, variables, solve_time, solver_log, skipped_sos })
}
