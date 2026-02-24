//! Plain-text rendering of detail panel content for clipboard yanking.

use std::fmt::Write;

use lp_parser_rs::interner::NameInterner;

use crate::app::App;
use crate::detail_model::{CoefficientRow, build_coeff_rows};
use crate::diff_model::{
    CoefficientChange, ConstraintDiffDetail, ConstraintDiffEntry, DiffKind, ObjectiveDiffEntry, ResolvedCoefficient, ResolvedConstraint,
    VariableDiffEntry,
};
use crate::solver::{SolveDiffResult, SolveResult};
use crate::state::Section;
use crate::widgets::detail::{fmt_bound, variable_bounds};

/// Pre-computed horizontal rules to avoid heap allocations from `repeat()`.
const RULE_30: &str = "──────────────────────────────────";
const RULE_38: &str = "──────────────────────────────────────";
const RULE_60: &str = "──────────────────────────────────────────────────────────────";
const RULE_78: &str = "──────────────────────────────────────────────────────────────────────────────────";
const RULE_98: &str = "──────────────────────────────────────────────────────────────────────────────────────────────────────────";

/// Writing to a `String` via `fmt::Write` is infallible. This macro replaces
/// `let _ = writeln!(...)` with an asserting version that satisfies Tiger Style.
macro_rules! w {
    ($dst:expr, $($arg:tt)*) => {
        writeln!($dst, $($arg)*).expect("writing to String is infallible")
    };
    ($dst:expr) => {
        writeln!($dst).expect("writing to String is infallible")
    };
}

/// Render the currently selected detail panel as plain text.
/// Returns `None` if no entry is selected or the section is Summary.
pub fn render_detail_plain(app: &App) -> Option<String> {
    let entry_index = app.selected_entry_index()?;
    let cached_rows = app.cached_coeff_rows();

    match app.active_section {
        Section::Variables => {
            let entry = app.report.variables.entries.get(entry_index)?;
            Some(render_variable_plain(entry))
        }
        Section::Constraints => {
            let entry = app.report.constraints.entries.get(entry_index)?;
            Some(render_constraint_plain(entry, cached_rows, &app.report.interner))
        }
        Section::Objectives => {
            let entry = app.report.objectives.entries.get(entry_index)?;
            Some(render_objective_plain(entry, cached_rows, &app.report.interner))
        }
        Section::Summary => None,
    }
}

/// Write type/bounds lines for a single-side variable (added or removed).
fn write_variable_type_info(out: &mut String, variable_type: &lp_parser_rs::model::VariableType) {
    let label = std::str::from_utf8(variable_type.as_ref()).unwrap_or("?");
    w!(out, "  Type:   {label}");
    let (lower_bound, upper_bound) = variable_bounds(variable_type);
    if let Some(lower) = lower_bound {
        w!(out, "  Lower:  {lower}");
    }
    if let Some(upper) = upper_bound {
        w!(out, "  Upper:  {upper}");
    }
    if let (Some(lower), Some(upper)) = (lower_bound, upper_bound) {
        w!(out, "  Range:  {}", upper - lower);
    }
}

#[allow(clippy::similar_names)] // lower_bound/upper_bound share prefixes
fn render_variable_plain(entry: &VariableDiffEntry) -> String {
    let mut out = String::new();
    w!(out, "Variable: {} [{}]", entry.name, entry.kind);
    w!(out, "{}", RULE_38);

    match entry.kind {
        DiffKind::Added => {
            let variable_type = entry.new_type.as_ref().expect("invariant: Added entry must have new_type");
            write_variable_type_info(&mut out, variable_type);
        }
        DiffKind::Removed => {
            let variable_type = entry.old_type.as_ref().expect("invariant: Removed entry must have old_type");
            write_variable_type_info(&mut out, variable_type);
        }
        DiffKind::Modified => {
            let old = entry.old_type.as_ref().expect("invariant: Modified entry must have old_type");
            let new = entry.new_type.as_ref().expect("invariant: Modified entry must have new_type");
            let old_label = std::str::from_utf8(old.as_ref()).unwrap_or("?");
            let new_label = std::str::from_utf8(new.as_ref()).unwrap_or("?");

            if old_label == new_label {
                w!(out, "  Type:   {old_label} (unchanged)");
            } else {
                w!(out, "  Type:   {old_label}  \u{2192}  {new_label}");
            }

            let (old_lower, old_upper) = variable_bounds(old);
            let (new_lower, new_upper) = variable_bounds(new);

            if old_lower.is_some() || new_lower.is_some() {
                w!(out, "  Lower:  {}  \u{2192}  {}", fmt_bound(old_lower), fmt_bound(new_lower));
            }
            if old_upper.is_some() || new_upper.is_some() {
                w!(out, "  Upper:  {}  \u{2192}  {}", fmt_bound(old_upper), fmt_bound(new_upper));
            }
            let old_range = old_lower.zip(old_upper).map(|(lower, upper)| upper - lower);
            let new_range = new_lower.zip(new_upper).map(|(lower, upper)| upper - lower);
            if old_range.is_some() || new_range.is_some() {
                w!(out, "  Range:  {}  \u{2192}  {}", fmt_bound(old_range), fmt_bound(new_range));
            }
        }
    }
    out
}

fn render_constraint_plain(entry: &ConstraintDiffEntry, cached_rows: Option<&[CoefficientRow]>, interner: &NameInterner) -> String {
    let mut out = String::new();
    w!(out, "Constraint: {} [{}]", entry.name, entry.kind);
    w!(out, "{}", RULE_38);

    if entry.line_file1.is_some() || entry.line_file2.is_some() {
        match (entry.line_file1, entry.line_file2) {
            (Some(l1), Some(l2)) => {
                w!(out, "  Location: L{l1} / L{l2}");
            }
            (Some(l1), None) => {
                w!(out, "  Location: L{l1} (removed)");
            }
            (None, Some(l2)) => {
                w!(out, "  Location: L{l2} (added)");
            }
            (None, None) => unreachable!("guarded by outer if"),
        }
    }

    match &entry.detail {
        ConstraintDiffDetail::Standard {
            old_coefficients,
            new_coefficients,
            coeff_changes,
            operator_change,
            rhs_change,
            old_rhs,
            new_rhs,
        } => {
            if let Some((old_op, new_op)) = operator_change {
                w!(out, "  Operator: {old_op}  \u{2192}  {new_op}");
            }
            if rhs_change.is_some() {
                w!(out, "  RHS:      {old_rhs}  \u{2192}  {new_rhs}");
            } else {
                w!(out, "  RHS:      {old_rhs} (unchanged)");
            }
            w!(out);
            w!(out, "  Coefficients:");
            write_coeff_changes(&mut out, coeff_changes, old_coefficients, new_coefficients, cached_rows, interner);
        }
        ConstraintDiffDetail::Sos { old_weights, new_weights, weight_changes, type_change } => {
            if let Some((old_type, new_type)) = type_change {
                w!(out, "  SOS Type: {old_type}  \u{2192}  {new_type}");
            }
            w!(out);
            w!(out, "  Weights:");
            write_coeff_changes(&mut out, weight_changes, old_weights, new_weights, None, interner);
        }
        ConstraintDiffDetail::TypeChanged { old_summary, new_summary } => {
            w!(out, "  Was:  {old_summary}");
            w!(out, "  Now:  {new_summary}");
        }
        ConstraintDiffDetail::AddedOrRemoved(constraint) => match constraint {
            ResolvedConstraint::Standard { coefficients, operator, rhs } => {
                w!(out, "  Operator: {operator}");
                w!(out, "  RHS:      {rhs}");
                w!(out);
                w!(out, "  Coefficients:");
                for c in coefficients {
                    w!(out, "    {:<20}{}", interner.resolve(c.name), c.value);
                }
            }
            ResolvedConstraint::Sos { sos_type, weights } => {
                w!(out, "  SOS Type: {sos_type}");
                w!(out);
                w!(out, "  Weights:");
                for w_entry in weights {
                    w!(out, "    {:<20}{}", interner.resolve(w_entry.name), w_entry.value);
                }
            }
        },
    }
    out
}

fn render_objective_plain(entry: &ObjectiveDiffEntry, cached_rows: Option<&[CoefficientRow]>, interner: &NameInterner) -> String {
    let mut out = String::new();
    w!(out, "Objective: {} [{}]", entry.name, entry.kind);
    w!(out, "{}", RULE_38);
    w!(out, "  Coefficients:");

    if entry.kind == DiffKind::Modified {
        write_coeff_changes(&mut out, &entry.coeff_changes, &entry.old_coefficients, &entry.new_coefficients, cached_rows, interner);
    } else {
        let coeffs = if entry.kind == DiffKind::Added { &entry.new_coefficients } else { &entry.old_coefficients };
        for c in coeffs {
            w!(out, "    {:<20}{}", interner.resolve(c.name), c.value);
        }
    }
    out
}

/// Column width for value formatting.
const VAL_WIDTH: usize = 12;

/// Format an `Option<f64>` into a reusable buffer, returning the formatted slice.
///
/// Avoids per-row `map_or_else` / `format!` heap allocations by writing into a
/// caller-owned `String` buffer that is cleared before each use.
fn fmt_opt_f64_precise(buf: &mut String, value: Option<f64>) -> &str {
    buf.clear();
    match value {
        Some(v) => {
            write!(buf, "{v:.6}").expect("writing to String is infallible");
            buf.as_str()
        }
        None => "\u{2014}",
    }
}

/// Like [`fmt_opt_f64_precise`] but with 4 decimal places, for constraint tables.
fn fmt_opt_f64_short(buf: &mut String, value: Option<f64>) -> &str {
    buf.clear();
    match value {
        Some(v) => {
            write!(buf, "{v:.4}").expect("writing to String is infallible");
            buf.as_str()
        }
        None => "\u{2014}",
    }
}

fn write_coeff_changes(
    out: &mut String,
    changes: &[CoefficientChange],
    old_coefficients: &[ResolvedCoefficient],
    new_coefficients: &[ResolvedCoefficient],
    cached_rows: Option<&[CoefficientRow]>,
    interner: &NameInterner,
) {
    let built;
    let rows = if let Some(cached) = cached_rows {
        cached
    } else {
        built = build_coeff_rows(changes, old_coefficients, new_coefficients, interner);
        &built
    };

    // Reuse buffers across rows to avoid per-row String allocations.
    let mut old_buf = String::with_capacity(16);
    let mut new_buf = String::with_capacity(16);
    for row in rows {
        old_buf.clear();
        if let Some(v) = row.old_value {
            write!(old_buf, "{v}").expect("writing to String is infallible");
        }
        new_buf.clear();
        if let Some(v) = row.new_value {
            write!(new_buf, "{v}").expect("writing to String is infallible");
        }

        match row.change_kind {
            Some(DiffKind::Added) => {
                w!(out, "    {:<20}{:>VAL_WIDTH$}  \u{2192}  {:<VAL_WIDTH$} [added]", row.variable, "", new_buf);
            }
            Some(DiffKind::Removed) => {
                w!(out, "    {:<20}{:>VAL_WIDTH$}  \u{2192}  {:VAL_WIDTH$} [removed]", row.variable, old_buf, "");
            }
            Some(DiffKind::Modified) => {
                w!(out, "    {:<20}{:>VAL_WIDTH$}  \u{2192}  {:<VAL_WIDTH$} [modified]", row.variable, old_buf, new_buf);
            }
            None => {
                w!(out, "    {:<20}{:>VAL_WIDTH$} (unchanged)", row.variable, old_buf);
            }
        }
    }
}

/// Format a solve result as plain text for clipboard yanking.
pub fn format_solve_result(result: &SolveResult) -> String {
    let mut text = String::new();
    w!(text, "Status: {}", result.status);
    if let Some(obj) = result.objective_value {
        w!(text, "Objective: {obj}");
    }
    let total = result.build_time + result.solve_time + result.extract_time;
    w!(text, "Build time:   {:.3}s", result.build_time.as_secs_f64());
    w!(text, "Solve time:   {:.3}s", result.solve_time.as_secs_f64());
    w!(text, "Extract time: {:.3}s", result.extract_time.as_secs_f64());
    w!(text, "Total time:   {:.3}s", total.as_secs_f64());
    if result.skipped_sos > 0 {
        w!(text, "Warning: {} SOS constraint(s) skipped (not supported by solver)", result.skipped_sos);
    }
    if !result.variables.is_empty() {
        w!(text);
        w!(text, "Variables:");
        let has_reduced_costs = !result.reduced_costs.is_empty();
        if has_reduced_costs {
            w!(text, "  {:<30} {:>12}  {:>14}", "Name", "Value", "Reduced Cost");
        }
        for (i, (name, val)) in result.variables.iter().enumerate() {
            if has_reduced_costs {
                let reduced_cost = result.reduced_costs.get(i).map_or(0.0, |(_, v)| *v);
                w!(text, "  {name:<30} {val:>12.6}  {reduced_cost:>14.6}");
            } else {
                w!(text, "  {name:<30} {val}");
            }
        }
    }
    if !result.shadow_prices.is_empty() {
        w!(text);
        w!(text, "Constraints:");
        w!(text, "  {:<30} {:>12}  {:>14}", "Name", "Activity", "Shadow Price");
        for (i, (name, shadow_price)) in result.shadow_prices.iter().enumerate() {
            let row_value = result.row_values.get(i).map_or(0.0, |(_, v)| *v);
            w!(text, "  {name:<30} {row_value:>12.6}  {shadow_price:>14.6}");
        }
    }
    if !result.solver_log.is_empty() {
        w!(text);
        w!(text, "Solver Log:");
        for line in result.solver_log.lines() {
            w!(text, "  {line}");
        }
    }
    text
}

/// Format a solve diff comparison as plain text for clipboard yanking.
pub fn format_solve_diff_result(diff: &SolveDiffResult) -> String {
    let mut text = String::new();

    w!(text, "Solve Comparison");
    w!(text, "File 1: {}", diff.file1_label);
    w!(text, "File 2: {}", diff.file2_label);
    w!(text);

    write_diff_summary(&mut text, diff);
    write_diff_variables(&mut text, diff);
    write_diff_constraints(&mut text, diff);
    write_diff_solver_logs(&mut text, diff);

    text
}

/// Write the summary comparison table (status, objective, time, counts).
fn write_diff_summary(text: &mut String, diff: &SolveDiffResult) {
    let r1 = &diff.result1;
    let r2 = &diff.result2;

    w!(text, "{:<18} {:<20} {:<20}", "", "File 1", "File 2");
    w!(text, "{RULE_60}");
    w!(text, "{:<18} {:<20} {:<20}", "Status:", r1.status, r2.status);

    let obj1 = r1.objective_value.map_or_else(|| "N/A".to_owned(), |v| format!("{v:.6}"));
    let obj2 = r2.objective_value.map_or_else(|| "N/A".to_owned(), |v| format!("{v:.6}"));
    w!(text, "{:<18} {:<20} {:<20}", "Objective:", obj1, obj2);
    w!(text, "{:<18} {:<20} {:<20}", "Variables:", r1.variables.len(), r2.variables.len());
    w!(text, "{:<18} {:<20} {:<20}", "Constraints:", r1.shadow_prices.len(), r2.shadow_prices.len());

    let total1 = r1.build_time + r1.solve_time + r1.extract_time;
    let total2 = r2.build_time + r2.solve_time + r2.extract_time;
    w!(text);
    w!(text, "Timings");
    w!(text, "{RULE_60}");
    w!(
        text,
        "{:<18} {:<20} {:<20}",
        "Build:",
        format!("{:.3}s", r1.build_time.as_secs_f64()),
        format!("{:.3}s", r2.build_time.as_secs_f64())
    );
    w!(
        text,
        "{:<18} {:<20} {:<20}",
        "Solve:",
        format!("{:.3}s", r1.solve_time.as_secs_f64()),
        format!("{:.3}s", r2.solve_time.as_secs_f64())
    );
    w!(
        text,
        "{:<18} {:<20} {:<20}",
        "Extract:",
        format!("{:.3}s", r1.extract_time.as_secs_f64()),
        format!("{:.3}s", r2.extract_time.as_secs_f64())
    );
    w!(text, "{:<18} {:<20} {:<20}", "Total:", format!("{:.3}s", total1.as_secs_f64()), format!("{:.3}s", total2.as_secs_f64()));
    w!(text, "{RULE_60}");
    w!(text, "{:<18} {:.3}s", "Diff:", diff.diff_time.as_secs_f64());
}

/// Write the variable diff table.
fn write_diff_variables(text: &mut String, diff: &SolveDiffResult) {
    if diff.variable_diff.is_empty() {
        return;
    }
    w!(text);
    w!(text, "Variables:");
    w!(text, "  {:<24} {:>14} {:>14} {:>14} {:>14} {:>14}", "Name", "File 1", "File 2", "\u{0394}", "RC 1", "RC 2");
    w!(text, "  {RULE_98}");
    let mut buf1 = String::with_capacity(24);
    let mut buf2 = String::with_capacity(24);
    let mut buf3 = String::with_capacity(24);
    let mut buf4 = String::with_capacity(24);
    let mut delta_buf = String::with_capacity(24);
    for row in &diff.variable_diff {
        let name = row.name(&diff.result1, &diff.result2);
        let v1 = fmt_opt_f64_precise(&mut buf1, row.val1);
        let v2 = fmt_opt_f64_precise(&mut buf2, row.val2);
        let rc1 = fmt_opt_f64_precise(&mut buf3, row.reduced_cost1);
        let rc2 = fmt_opt_f64_precise(&mut buf4, row.reduced_cost2);
        delta_buf.clear();
        let delta: &str = match (row.val1, row.val2) {
            (None, Some(_)) => "(added)",
            (Some(_), None) => "(removed)",
            (Some(a), Some(b)) if row.changed => {
                let d = b - a;
                let sign = if d >= 0.0 { "+" } else { "" };
                write!(delta_buf, "{sign}{d:.6}").expect("writing to String is infallible");
                &delta_buf
            }
            _ => "",
        };
        let marker = if row.changed { " *" } else { "" };
        w!(text, "  {:<24} {:>14} {:>14} {:>14} {:>14} {:>14}{marker}", name, v1, v2, delta, rc1, rc2);
    }
}

/// Write the constraint diff table.
fn write_diff_constraints(text: &mut String, diff: &SolveDiffResult) {
    if diff.constraint_diff.is_empty() {
        return;
    }
    w!(text);
    w!(text, "Constraints:");
    w!(text, "  {:<22} {:>13} {:>13} {:>13} {:>13}", "Name", "Activity 1", "Activity 2", "Shadow 1", "Shadow 2");
    w!(text, "  {RULE_78}");
    let mut buf1 = String::with_capacity(16);
    let mut buf2 = String::with_capacity(16);
    let mut buf3 = String::with_capacity(16);
    let mut buf4 = String::with_capacity(16);
    for row in &diff.constraint_diff {
        let name = row.name(&diff.result1, &diff.result2);
        let a1 = fmt_opt_f64_short(&mut buf1, row.activity1);
        let a2 = fmt_opt_f64_short(&mut buf2, row.activity2);
        let s1 = fmt_opt_f64_short(&mut buf3, row.shadow_price1);
        let s2 = fmt_opt_f64_short(&mut buf4, row.shadow_price2);
        let marker = if row.changed { " *" } else { "" };
        w!(text, "  {:<22} {:>13} {:>13} {:>13} {:>13}{marker}", name, a1, a2, s1, s2);
    }
}

/// Write the solver log sections for both files.
fn write_diff_solver_logs(text: &mut String, diff: &SolveDiffResult) {
    let r1 = &diff.result1;
    let r2 = &diff.result2;
    if r1.solver_log.is_empty() && r2.solver_log.is_empty() {
        return;
    }
    w!(text);
    w!(text, "Solver Logs:");
    if !r1.solver_log.is_empty() {
        w!(text, "\u{2500}\u{2500} File 1: {} \u{2500}{RULE_30}", diff.file1_label);
        for line in r1.solver_log.lines() {
            w!(text, "  {line}");
        }
    }
    if !r2.solver_log.is_empty() {
        w!(text, "\u{2500}\u{2500} File 2: {} \u{2500}{RULE_30}", diff.file2_label);
        for line in r2.solver_log.lines() {
            w!(text, "  {line}");
        }
    }
}
