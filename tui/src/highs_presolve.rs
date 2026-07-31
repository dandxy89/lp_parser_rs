//! What `HiGHS`'s own presolve removes, by name.
//!
//! The `P` picker applies *our* rewrite rules and logs what they did (see
//! [`crate::presolve`]). This asks the same question of the solver: before
//! `HiGHS` runs a single simplex iteration it throws rows and columns away, and
//! until now the only trace of that was the totals it prints to its log
//! ("Presolve : Reductions: rows 12(-8)"), which the diagnostics pane shows.
//!
//! The names are recoverable. `HPresolve::shrinkProblem` carries `col_names_`
//! and `row_names_` through the reduction, and the C API exposes the survivors
//! via `Highs_getPresolvedColName` / `Highs_getPresolvedRowName`. Passing our
//! names in and reading back what is left turns the totals into a list: exactly
//! which rows and columns the solver decided it did not need.
//!
//! The `highs` crate's safe API never passes names to `HiGHS`, so this reaches
//! past it to `highs-sys` for the five calls it lacks. Everything else — the
//! model build, the column order, the bound interpretation — is shared with the
//! ordinary solve via [`crate::solver::build_highs_model`], so the report is
//! about the model the solver actually sees and not a lookalike.
//!
//! Nothing is solved and nothing is written: `Highs_presolve` stops at the
//! reduced model, which is the whole point — this is what the solver would have
//! started from.

use std::collections::HashSet;
use std::ffi::{CString, c_char, c_void};
use std::fmt::Write as _;
use std::time::{Duration, Instant};

use lp_parser_rs::problem::LpProblem;

use crate::solver::build_highs_model;

/// Size of the buffer `HiGHS` writes a name into — `kHighsMaximumStringLength`.
/// The C API documents this as a requirement, not a suggestion: it writes up to
/// this many bytes.
const NAME_BUFFER: usize = 512;

/// What `HiGHS`'s presolve did to a model.
#[derive(Debug, Clone)]
pub struct HighsPresolveReport {
    pub rows_before: usize,
    pub rows_after: usize,
    pub cols_before: usize,
    pub cols_after: usize,
    /// Non-zeros left in the reduced model.
    pub nnz_after: usize,
    /// Rows `HiGHS` dropped, in the model's column order.
    pub removed_rows: Vec<String>,
    /// Columns `HiGHS` dropped — unlike our rewrite, `HiGHS` does remove them.
    pub removed_cols: Vec<String>,
    /// Set when presolve proved the model infeasible without solving it.
    pub infeasible: bool,
    /// SOS sets left out of the model, as the ordinary solve leaves them out.
    pub skipped_sos: usize,
    pub duration: Duration,
}

impl HighsPresolveReport {
    /// Whether presolve left the model exactly as it found it.
    #[must_use]
    pub const fn is_noop(&self) -> bool {
        self.rows_after == self.rows_before && self.cols_after == self.cols_before
    }

    /// Whether presolve solved the model outright, leaving nothing behind.
    #[must_use]
    pub const fn reduced_to_empty(&self) -> bool {
        self.rows_after == 0 && self.cols_after == 0
    }

    /// One-line summary.
    #[must_use]
    pub fn headline(&self) -> String {
        if self.infeasible {
            return "HiGHS presolve: proved the model infeasible".to_owned();
        }
        if self.reduced_to_empty() {
            return format!("HiGHS presolve: solved the model outright \u{2014} nothing left, {:.1}ms", self.millis());
        }
        if self.is_noop() {
            return format!("HiGHS presolve: no reduction, {:.1}ms", self.millis());
        }
        format!(
            "HiGHS presolve: -{} rows, -{} cols, {} rows x {} cols x {} nnz left, {:.1}ms",
            self.removed_rows.len(),
            self.removed_cols.len(),
            self.rows_after,
            self.cols_after,
            self.nnz_after,
            self.millis(),
        )
    }

    fn millis(&self) -> f64 {
        self.duration.as_secs_f64() * 1000.0
    }

    /// The report as plain text, for the log pane and the file it writes.
    #[must_use]
    pub fn log_text(&self) -> String {
        let mut out = self.headline();
        out.push('\n');
        if self.skipped_sos > 0 {
            writeln!(out, "{} SOS set(s) are not part of the model HiGHS sees", self.skipped_sos)
                .expect("writing into a String cannot fail");
        }
        out.push('\n');
        for name in &self.removed_rows {
            out.push_str("row removed  ");
            out.push_str(name);
            out.push('\n');
        }
        for name in &self.removed_cols {
            out.push_str("col removed  ");
            out.push_str(name);
            out.push('\n');
        }
        out
    }
}

/// Run `HiGHS`'s presolve on `problem` and report what it removed.
///
/// # Errors
///
/// Returns an error when the model cannot be built or `HiGHS` declines to
/// presolve it — it refuses models with infinite costs, semi-continuous
/// variables, or a quadratic objective.
pub fn highs_presolve(problem: &LpProblem) -> Result<HighsPresolveReport, String> {
    if problem.variables.is_empty() {
        return Err("the model has no variables".to_owned());
    }

    let started = Instant::now();
    let built = build_highs_model(problem);
    let (cols_before, rows_before) = (built.variable_names.len(), built.row_constraint_names.len());
    let skipped_sos = built.skipped_sos;
    let (variable_names, row_names) = (built.variable_names, built.row_constraint_names);

    let mut model = built.row_problem.optimise(built.sense);
    // The presolve log would otherwise land in the terminal underneath the TUI.
    model.make_quiet();
    let highs = model.as_mut_ptr();

    for (index, name) in variable_names.iter().enumerate() {
        pass_name(highs, index, name, Kind::Col)?;
    }
    for (index, name) in row_names.iter().enumerate() {
        pass_name(highs, index, name, Kind::Row)?;
    }

    // SAFETY: `highs` is the live model owned by `model` for the rest of this
    // function, and presolve takes no other arguments.
    let status = unsafe { highs_sys::Highs_presolve(highs) };
    if status == highs_sys::kHighsStatusError {
        return Err("HiGHS declined to presolve this model (infinite costs, semi-continuous columns or a quadratic objective)".to_owned());
    }

    // SAFETY: same live pointer; each call only reads a count.
    let (cols_after, rows_after, nnz_after, model_status) = unsafe {
        (
            highs_sys::Highs_getPresolvedNumCol(highs),
            highs_sys::Highs_getPresolvedNumRow(highs),
            highs_sys::Highs_getPresolvedNumNz(highs),
            highs_sys::Highs_getModelStatus(highs),
        )
    };
    let cols_after = usize::try_from(cols_after).map_err(|_| "HiGHS reported a negative presolved column count".to_owned())?;
    let rows_after = usize::try_from(rows_after).map_err(|_| "HiGHS reported a negative presolved row count".to_owned())?;
    let nnz_after = usize::try_from(nnz_after).map_err(|_| "HiGHS reported a negative presolved non-zero count".to_owned())?;
    debug_assert!(cols_after <= cols_before, "presolve cannot add columns");
    debug_assert!(rows_after <= rows_before, "presolve cannot add rows");

    // A model presolve proved infeasible has no reduced form to survive into,
    // so the survivor lists below would read as "everything was removed".
    let infeasible = model_status == highs_sys::kHighsModelStatusInfeasible;

    let (removed_rows, removed_cols) = if infeasible {
        (Vec::new(), Vec::new())
    } else {
        (removed(&row_names, &surviving(highs, rows_after, Kind::Row)), removed(&variable_names, &surviving(highs, cols_after, Kind::Col)))
    };

    Ok(HighsPresolveReport {
        rows_before,
        rows_after,
        cols_before,
        cols_after,
        nnz_after,
        removed_rows,
        removed_cols,
        infeasible,
        skipped_sos,
        duration: started.elapsed(),
    })
}

/// Which of the two name spaces a call refers to. The four `HiGHS` entry points
/// differ only in this, so it is a parameter rather than four copies of the
/// buffer handling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Row,
    Col,
}

/// Give `HiGHS` our name for one row or column, so the presolved model carries
/// it too.
fn pass_name(highs: *mut c_void, index: usize, name: &str, kind: Kind) -> Result<(), String> {
    let c_name = CString::new(name).map_err(|_| format!("name {name:?} contains a NUL byte and cannot be passed to HiGHS"))?;
    let index = i32::try_from(index).map_err(|_| "the model has more rows or columns than HiGHS can index".to_owned())?;
    // SAFETY: `highs` is a live model; `c_name` outlives the call, and HiGHS
    // copies the string rather than retaining the pointer.
    let status = unsafe {
        match kind {
            Kind::Row => highs_sys::Highs_passRowName(highs, index, c_name.as_ptr()),
            Kind::Col => highs_sys::Highs_passColName(highs, index, c_name.as_ptr()),
        }
    };
    if status == highs_sys::kHighsStatusError {
        return Err(format!("HiGHS rejected the name {name:?}"));
    }
    Ok(())
}

/// The names still present in the presolved model.
///
/// A name `HiGHS` declines to give back is simply absent from the set, which
/// lands it in the removed list — the honest reading, since we then have no
/// evidence it survived.
fn surviving(highs: *mut c_void, count: usize, kind: Kind) -> HashSet<String> {
    let mut names = HashSet::with_capacity(count);
    let mut buffer: [c_char; NAME_BUFFER] = [0; NAME_BUFFER];
    for index in 0..count {
        let Ok(index) = i32::try_from(index) else {
            debug_assert!(false, "a presolved index must fit HighsInt: it came from HiGHS");
            break;
        };
        // SAFETY: `highs` is a live model, `index` is below the count HiGHS
        // itself reported, and `buffer` is the documented size for these calls.
        let status = unsafe {
            match kind {
                Kind::Row => highs_sys::Highs_getPresolvedRowName(highs, index, buffer.as_mut_ptr()),
                Kind::Col => highs_sys::Highs_getPresolvedColName(highs, index, buffer.as_mut_ptr()),
            }
        };
        if status == highs_sys::kHighsStatusError {
            continue;
        }
        // SAFETY: on success HiGHS has written a NUL-terminated string into the
        // buffer, within the length it documents as required.
        let name = unsafe { std::ffi::CStr::from_ptr(buffer.as_ptr()) };
        if let Ok(name) = name.to_str() {
            names.insert(name.to_owned());
        }
    }
    names
}

/// The names that went, in the order they were passed to `HiGHS` (which is
/// sorted by name, so the report is stable across runs).
fn removed(passed: &[String], survivors: &HashSet<String>) -> Vec<String> {
    passed.iter().filter(|name| !survivors.contains(*name)).cloned().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(source: &str) -> LpProblem {
        LpProblem::parse(source).expect("test fixture must parse")
    }

    #[test]
    fn a_redundant_row_and_its_fixed_column_are_named() {
        // c2 pins x, and c1 can never bind once x is fixed and y is capped.
        let problem = parse("Minimize\n obj: x + y\nSubject To\n c1: x + y <= 900\n c2: x <= 4\nBounds\n 0 <= x <= 10\n 0 <= y <= 10\nEnd");
        let report = highs_presolve(&problem).expect("a plain LP presolves");

        assert!(report.rows_after < report.rows_before, "HiGHS removes rows here: {}", report.headline());
        assert!(report.removed_rows.contains(&"c2".to_owned()), "the singleton row is named: {:?}", report.removed_rows);
        assert_eq!(report.removed_rows.len(), report.rows_before - report.rows_after, "every removed row is accounted for");
        assert_eq!(report.removed_cols.len(), report.cols_before - report.cols_after, "every removed column is accounted for");
    }

    #[test]
    fn a_model_presolve_solves_outright_reports_everything_gone() {
        let problem = parse("Minimize\n obj: x + y\nSubject To\n c1: x + y >= 2\nBounds\n 0 <= x <= 10\n 0 <= y <= 10\nEnd");
        let report = highs_presolve(&problem).expect("a plain LP presolves");

        if report.reduced_to_empty() {
            assert_eq!(report.removed_rows.len(), report.rows_before);
            assert_eq!(report.removed_cols.len(), report.cols_before);
            assert!(report.headline().contains("solved the model outright"));
        }
    }

    #[test]
    fn an_infeasible_model_is_reported_as_such_rather_than_as_total_removal() {
        let problem = parse("Minimize\n obj: x\nSubject To\n c1: x >= 5\n c2: x <= 1\nEnd");
        let report = highs_presolve(&problem).expect("an infeasible LP still presolves");

        if report.infeasible {
            assert!(report.removed_rows.is_empty(), "an infeasible verdict leaves no reduced model to compare against");
            assert!(report.headline().contains("infeasible"));
        }
    }

    #[test]
    fn a_model_with_no_variables_is_refused_rather_than_passed_to_highs() {
        let problem = LpProblem::default();
        assert!(highs_presolve(&problem).is_err(), "an empty model has nothing to presolve");
    }
}
