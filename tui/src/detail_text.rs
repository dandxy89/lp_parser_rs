//! Plain-text rendering of detail panel content for clipboard yanking.

use std::fmt::Write;

use lp_parser_rs::analysis::ProblemAnalysis;
use lp_parser_rs::interner::NameInterner;

use crate::app::App;
use crate::detail_model::{CoefficientRow, build_coeff_rows};
use crate::diff_model::{
    CoefficientChange, ConstraintDiffDetail, ConstraintDiffEntry, DiffCounts, DiffKind, ObjectiveDiffEntry, ResolvedCoefficient,
    ResolvedConstraint, VariableDiffEntry,
};
use crate::solver::{SolveDiffResult, SolveResult};
use crate::state::{Section, Side};
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
/// Returns `None` if no entry is selected (except for Summary, which has no entry).
pub fn render_detail_plain(app: &App) -> Option<String> {
    match app.active_section {
        Section::Summary => Some(render_summary_plain(app)),
        _ => {
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
                Section::Summary => unreachable!("handled above"),
            }
        }
    }
}

/// Render a single side (old or new) of the selected entry as plain text.
///
/// Returns `None` if the active section is Summary, no entry is selected,
/// or the requested side does not exist for the entry (e.g. `Old` on an Added entry).
pub fn render_side_plain(app: &App, side: Side) -> Option<String> {
    let entry_index = app.selected_entry_index()?;
    match app.active_section {
        Section::Summary => None,
        Section::Variables => {
            let entry = app.report.variables.entries.get(entry_index)?;
            render_variable_side(entry, side)
        }
        Section::Constraints => {
            let entry = app.report.constraints.entries.get(entry_index)?;
            render_constraint_side(entry, side, &app.report.interner)
        }
        Section::Objectives => {
            let entry = app.report.objectives.entries.get(entry_index)?;
            render_objective_side(entry, side, &app.report.interner)
        }
    }
}

/// Render a single side of a variable entry.
fn render_variable_side(entry: &VariableDiffEntry, side: Side) -> Option<String> {
    let variable_type = match side {
        Side::Old => entry.old_type.as_ref()?,
        Side::New => entry.new_type.as_ref()?,
    };
    let mut out = String::new();
    w!(out, "{}", entry.name);
    write_variable_type_info(&mut out, variable_type);
    Some(out)
}

/// Render a single side of a constraint entry.
fn render_constraint_side(entry: &ConstraintDiffEntry, side: Side, interner: &NameInterner) -> Option<String> {
    let mut out = String::new();
    match &entry.detail {
        ConstraintDiffDetail::Standard { old_coefficients, new_coefficients, old_rhs, new_rhs, operator_change, old_operator, .. } => {
            let coefficients = match side {
                Side::Old => old_coefficients,
                Side::New => new_coefficients,
            };
            // For Added entries old_coefficients is empty; for Removed entries new_coefficients is empty.
            if coefficients.is_empty() {
                return None;
            }
            let rhs = match side {
                Side::Old => old_rhs,
                Side::New => new_rhs,
            };
            let operator = match operator_change {
                Some((old_op, new_op)) => match side {
                    Side::Old => old_op,
                    Side::New => new_op,
                },
                None => old_operator,
            };
            write_lp_expression(&mut out, &entry.name, coefficients, Some((*operator, *rhs)), interner);
        }
        ConstraintDiffDetail::Sos { old_weights, new_weights, type_change, old_sos_type, .. } => {
            let weights = match side {
                Side::Old => old_weights,
                Side::New => new_weights,
            };
            if weights.is_empty() {
                return None;
            }
            let sos_type = match type_change {
                Some((old_type, new_type)) => match side {
                    Side::Old => old_type,
                    Side::New => new_type,
                },
                None => old_sos_type,
            };
            w!(out, "{}: {} ::", entry.name, sos_type);
            for weight in weights {
                w!(out, "  {} : {}", interner.resolve(weight.name), weight.value);
            }
        }
        ConstraintDiffDetail::TypeChanged { old_summary, new_summary } => {
            let summary = match side {
                Side::Old => old_summary,
                Side::New => new_summary,
            };
            w!(out, "{}: {}", entry.name, summary);
        }
        ConstraintDiffDetail::AddedOrRemoved(constraint) => {
            // Only one side exists; check that the requested side matches.
            match (side, entry.kind) {
                (Side::Old, DiffKind::Removed) | (Side::New, DiffKind::Added) => {}
                _ => return None,
            }
            match constraint {
                ResolvedConstraint::Standard { coefficients, operator, rhs } => {
                    write_lp_expression(&mut out, &entry.name, coefficients, Some((*operator, *rhs)), interner);
                }
                ResolvedConstraint::Sos { sos_type, weights } => {
                    w!(out, "{}: {} ::", entry.name, sos_type);
                    for weight in weights {
                        w!(out, "  {} : {}", interner.resolve(weight.name), weight.value);
                    }
                }
            }
        }
    }
    Some(out)
}

/// Render a single side of an objective entry.
fn render_objective_side(entry: &ObjectiveDiffEntry, side: Side, interner: &NameInterner) -> Option<String> {
    let coefficients = match side {
        Side::Old => &entry.old_coefficients,
        Side::New => &entry.new_coefficients,
    };
    if coefficients.is_empty() {
        return None;
    }
    let mut out = String::new();
    write_lp_expression(&mut out, &entry.name, coefficients, None, interner);
    Some(out)
}

/// Write an LP-style expression: `name: coeff1 x1 + coeff2 x2 [operator rhs]`.
fn write_lp_expression(
    out: &mut String,
    name: &str,
    coefficients: &[ResolvedCoefficient],
    operator_rhs: Option<(lp_parser_rs::model::ComparisonOp, f64)>,
    interner: &NameInterner,
) {
    debug_assert!(!coefficients.is_empty(), "write_lp_expression called with empty coefficients");
    write!(out, "{name}:").expect("writing to String is infallible");
    for (i, coeff) in coefficients.iter().enumerate() {
        let var_name = interner.resolve(coeff.name);
        if i == 0 {
            write!(out, " {} {var_name}", coeff.value).expect("writing to String is infallible");
        } else if coeff.value < 0.0 {
            write!(out, " - {} {var_name}", -coeff.value).expect("writing to String is infallible");
        } else {
            write!(out, " + {} {var_name}", coeff.value).expect("writing to String is infallible");
        }
    }
    if let Some((operator, rhs)) = operator_rhs {
        w!(out, " {operator} {rhs}");
    } else {
        w!(out);
    }
}

/// Render the summary panel as plain text for clipboard yanking.
///
/// Mirrors the visual summary widget: file paths, name/sense changes,
/// per-section change-count table, totals, and comparative analysis.
pub fn render_summary_plain(app: &App) -> String {
    let report = &app.report;
    let summary = &app.cached_summary;
    let a1 = &report.analysis1;
    let a2 = &report.analysis2;

    let mut out = String::with_capacity(1024);

    // File header
    w!(out, "  {}  \u{2192}  {}", report.file1, report.file2);

    if let Some((ref old, ref new)) = report.name_changed {
        let old_name = old.as_deref().unwrap_or("(unnamed)");
        let new_name = new.as_deref().unwrap_or("(unnamed)");
        w!(out, "  Name:   \"{old_name}\"  \u{2192}  \"{new_name}\"");
    }
    if let Some((ref old_sense, ref new_sense)) = report.sense_changed {
        w!(out, "  Sense:  {old_sense}  \u{2192}  {new_sense}");
    }
    w!(out);

    // Change counts table
    w!(out, "  {:<14}{:>7}{:>9}{:>12}{:>9}", "Section", "Added", "Removed", "Modified", "Total");
    for (label, counts) in [("Variables", &summary.variables), ("Constraints", &summary.constraints), ("Objectives", &summary.objectives)] {
        write_count_row(&mut out, label, counts);
    }
    w!(out, "  {}", RULE_60);
    let totals = summary.aggregate_counts();
    write_count_row(&mut out, "TOTAL", &totals);

    // Problem Dimensions
    w!(out);
    write_analysis_sections(&mut out, a1, a2);

    // Issues
    w!(out);
    write_issues_plain(&mut out, report, a1, a2);

    out
}

/// Write a single row of the change-count table.
fn write_count_row(out: &mut String, label: &str, counts: &DiffCounts) {
    w!(out, "  {:<14}{:>7}{:>9}{:>12}{:>9}", label, counts.added, counts.removed, counts.modified, counts.total());
}

/// Write comparative analysis sections (dimensions, variable types,
/// constraint types, coefficient scaling) as plain text.
#[allow(clippy::cast_possible_wrap)] // LP problem dimensions never approach i64::MAX
fn write_analysis_sections(out: &mut String, a: &ProblemAnalysis, b: &ProblemAnalysis) {
    const W: usize = 18;

    // Problem Dimensions
    w!(out, "  Problem Dimensions");
    w!(out, "  {:<W$}{:>12}{:>12}{:>12}", "", "File A", "File B", "Delta");
    write_comparison_row_usize(out, "Variables", W, a.summary.variable_count, b.summary.variable_count);
    write_comparison_row_usize(out, "Constraints", W, a.summary.constraint_count, b.summary.constraint_count);
    write_comparison_row_usize(out, "Non-zeros", W, a.summary.total_nonzeros, b.summary.total_nonzeros);
    write_comparison_row_pct(out, "Density", W, a.summary.density, b.summary.density);
    let sparsity_a = format!("{}\u{2013}{}", a.sparsity.min_vars_per_constraint, a.sparsity.max_vars_per_constraint);
    let sparsity_b = format!("{}\u{2013}{}", b.sparsity.min_vars_per_constraint, b.sparsity.max_vars_per_constraint);
    w!(out, "  {:<W$}{:>12}{:>12}", "Vars/constraint", sparsity_a, sparsity_b);

    // Variable Types
    w!(out);
    w!(out, "  Variable Types");
    w!(out, "  {:<W$}{:>12}{:>12}{:>12}", "", "File A", "File B", "Delta");
    let va = &a.variables.type_distribution;
    let vb = &b.variables.type_distribution;
    write_comparison_row_usize(out, "Binary", W, va.binary, vb.binary);
    write_comparison_row_usize(out, "Integer", W, va.integer, vb.integer);
    write_comparison_row_usize(out, "General", W, va.general, vb.general);
    write_comparison_row_usize(out, "Free", W, va.free, vb.free);
    write_comparison_row_usize(out, "Lower-bounded", W, va.lower_bounded, vb.lower_bounded);
    write_comparison_row_usize(out, "Upper-bounded", W, va.upper_bounded, vb.upper_bounded);
    write_comparison_row_usize(out, "Double-bounded", W, va.double_bounded, vb.double_bounded);
    write_comparison_row_usize(out, "Semi-continuous", W, va.semi_continuous, vb.semi_continuous);

    // Constraint Types
    w!(out);
    w!(out, "  Constraint Types");
    w!(out, "  {:<W$}{:>12}{:>12}{:>12}", "", "File A", "File B", "Delta");
    let ca = &a.constraints.type_distribution;
    let cb = &b.constraints.type_distribution;
    write_comparison_row_usize(out, "Equality (=)", W, ca.equality, cb.equality);
    write_comparison_row_usize(out, "<= constraints", W, ca.less_than_equal, cb.less_than_equal);
    write_comparison_row_usize(out, ">= constraints", W, ca.greater_than_equal, cb.greater_than_equal);
    write_comparison_row_usize(out, "< constraints", W, ca.less_than, cb.less_than);
    write_comparison_row_usize(out, "> constraints", W, ca.greater_than, cb.greater_than);
    write_comparison_row_usize(out, "SOS1", W, ca.sos1, cb.sos1);
    write_comparison_row_usize(out, "SOS2", W, ca.sos2, cb.sos2);

    // Coefficient Scaling
    w!(out);
    w!(out, "  Coefficient Scaling");
    w!(out, "  {:<W$}{:>16}{:>16}", "", "File A", "File B");
    let coeff_a = format_range_stats(&a.coefficients.constraint_coeff_range);
    let coeff_b = format_range_stats(&b.coefficients.constraint_coeff_range);
    w!(out, "  {:<W$}{:>16}{:>16}", "Coeff range", coeff_a, coeff_b);
    let ratio_a = format_scientific_plain(a.coefficients.coefficient_ratio);
    let ratio_b = format_scientific_plain(b.coefficients.coefficient_ratio);
    w!(out, "  {:<W$}{:>16}{:>16}", "Coeff ratio", ratio_a, ratio_b);
    let rhs_a = format_range_stats(&a.constraints.rhs_range);
    let rhs_b = format_range_stats(&b.constraints.rhs_range);
    w!(out, "  {:<W$}{:>16}{:>16}", "RHS range", rhs_a, rhs_b);
}

/// Write a comparison row with usize values and a signed delta.
#[allow(clippy::cast_possible_wrap)]
fn write_comparison_row_usize(out: &mut String, label: &str, label_width: usize, a: usize, b: usize) {
    let delta = b as i64 - a as i64;
    let delta_str = match delta.cmp(&0) {
        std::cmp::Ordering::Equal => "\u{2014}".to_string(),
        std::cmp::Ordering::Greater => format!("+{delta}"),
        std::cmp::Ordering::Less => format!("{delta}"),
    };
    w!(out, "  {:<label_width$}{:>12}{:>12}{:>12}", label, a, b, delta_str);
}

/// Write a comparison row with percentage values and a delta.
fn write_comparison_row_pct(out: &mut String, label: &str, label_width: usize, a: f64, b: f64) {
    let delta = b - a;
    let delta_str = if delta.abs() < 1e-10 { "\u{2014}".to_string() } else { format!("{:+.2}%", delta * 100.0) };
    w!(out, "  {:<label_width$}{:>11.2}%{:>11.2}%{:>12}", label, a * 100.0, b * 100.0, delta_str);
}

/// Format a `RangeStats` as a compact range string.
fn format_range_stats(r: &lp_parser_rs::analysis::RangeStats) -> String {
    if r.count == 0 { "\u{2014}".to_string() } else { format!("[{:.1e}, {:.1e}]", r.min, r.max) }
}

/// Format an f64 in scientific notation, returning em-dash for zero/non-finite.
fn format_scientific_plain(v: f64) -> String {
    if v == 0.0 || !v.is_finite() { "\u{2014}".to_string() } else { format!("{v:.2e}") }
}

/// Write the issues section as plain text.
fn write_issues_plain(out: &mut String, report: &crate::diff_model::LpDiffReport, a1: &ProblemAnalysis, a2: &ProblemAnalysis) {
    w!(out, "  Issues");

    let (err1, warn1, info1) = count_issue_severities(&a1.issues);
    let (err2, warn2, info2) = count_issue_severities(&a2.issues);

    w!(
        out,
        "  File A: {} error(s), {} warning(s), {} info  |  File B: {} error(s), {} warning(s), {} info",
        err1, warn1, info1, err2, warn2, info2
    );

    if a1.issues.is_empty() && a2.issues.is_empty() {
        w!(out, "  No issues detected");
        return;
    }
    w!(out);

    let label_a = short_filename(&report.file1);
    for issue in &a1.issues {
        w!(out, "  [{:<7}] {}: {}", issue.severity, label_a, issue.message);
    }
    let label_b = short_filename(&report.file2);
    for issue in &a2.issues {
        w!(out, "  [{:<7}] {}: {}", issue.severity, label_b, issue.message);
    }
}

/// Count issues by severity level.
fn count_issue_severities(issues: &[lp_parser_rs::analysis::AnalysisIssue]) -> (usize, usize, usize) {
    let mut errors = 0;
    let mut warnings = 0;
    let mut infos = 0;
    for issue in issues {
        match issue.severity {
            lp_parser_rs::analysis::IssueSeverity::Error => errors += 1,
            lp_parser_rs::analysis::IssueSeverity::Warning => warnings += 1,
            lp_parser_rs::analysis::IssueSeverity::Info => infos += 1,
        }
    }
    (errors, warnings, infos)
}

/// Extract the filename from a path string for compact display.
fn short_filename(path: &str) -> String {
    std::path::Path::new(path).file_name().map_or_else(|| path.to_string(), |f| f.to_string_lossy().into_owned())
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
            ..
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
        ConstraintDiffDetail::Sos { old_weights, new_weights, weight_changes, type_change, .. } => {
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
