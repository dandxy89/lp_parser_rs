//! Solver front-end for the TUI.
//!
//! The `HiGHS` integration itself (model building, solving, cancellation,
//! `highs.opt` handling and the elastic-relaxation infeasibility diagnosis)
//! lives in the `lp-highs` crate and is re-exported here so the rest of the
//! TUI is unchanged. What stays is presentation: comparing two solve results
//! row by row and writing the CSV exports.

use std::error::Error;
use std::path::Path;

pub use lp_highs::solve::*;

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
#[expect(clippy::cast_possible_truncation)]
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
#[expect(clippy::cast_possible_truncation)]
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
    use lp_parser_rs::problem::LpProblem;

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
