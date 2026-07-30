//! Plain-text rendering of panel content for clipboard yanking.
//!
//! Everything the detail, summary, numerics, and solve panels show is derived
//! from the same `Vec<Line>` the widgets draw: [`crate::widgets::plain`] strips
//! the styles and keeps the text. One renderer, two outputs.
//!
//! The exception is [`render_side_plain`], which yanks a single side of an
//! entry as LP source syntax (`c1: 2 x + 3 y <= 10`) for pasting back into a
//! model. That is a different artefact from what the panel draws, so it is
//! written out here.

use std::fmt::Write;

use lp_parser_rs::interner::NameInterner;

use crate::app::App;
use crate::diff_model::{
    ConstraintDiffDetail, ConstraintDiffEntry, DiffKind, ObjectiveDiffEntry, ResolvedCoefficient, ResolvedConstraint, VariableDiffEntry,
};
use crate::solver::{SolveDiffResult, SolveResult};
use crate::state::{AppMode, Section, Side};
use crate::widgets::detail::{
    build_constraint_detail, build_inspect_constraint, build_inspect_objective, build_inspect_variable, build_objective_detail,
    build_variable_detail,
};
use crate::widgets::plain;

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
/// Returns `None` if no entry is selected (except for Summary and Numerics,
/// which have no entry and yank their pre-built panel lines).
pub fn render_detail_plain(app: &App) -> Option<String> {
    let interner = &app.report.interner;
    let inspect = app.mode == AppMode::Inspect;
    match app.active_section {
        Section::Summary => Some(plain(&app.summary_lines)),
        Section::Numerics => Some(plain(&app.numerics_lines)),
        Section::Variables => {
            let entry = app.report.variables.entries.get(app.selected_entry_index()?)?;
            Some(plain(&if inspect { build_inspect_variable(entry) } else { build_variable_detail(entry) }))
        }
        Section::Constraints => {
            let entry = app.report.constraints.entries.get(app.selected_entry_index()?)?;
            let lines = if inspect {
                build_inspect_constraint(entry, interner)
            } else {
                build_constraint_detail(entry, app.cached_coeff_rows(), interner)
            };
            Some(plain(&lines))
        }
        Section::Objectives => {
            let entry = app.report.objectives.entries.get(app.selected_entry_index()?)?;
            let lines = if inspect {
                build_inspect_objective(entry, interner)
            } else {
                build_objective_detail(entry, app.cached_coeff_rows(), interner, None)
            };
            Some(plain(&lines))
        }
    }
}

/// Render the old or new side of the selected entry as LP source syntax.
///
/// Returns `None` when the requested side does not exist (an added entry has no
/// old side) or the section has no per-entry sides.
pub fn render_side_plain(app: &App, side: Side) -> Option<String> {
    let entry_index = app.selected_entry_index()?;
    match app.active_section {
        Section::Summary | Section::Numerics => None,
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
/// Write type/bounds lines for a single-side variable (added or removed).
fn write_variable_type_info(out: &mut String, spec: &crate::diff_model::VarSpec) {
    w!(out, "  Type:   {}", spec.kind);
    let (lower_bound, upper_bound) = (spec.bounds.lower, spec.bounds.upper);
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

/// Panel width assumed when flattening solve results to text. The solve tabs
/// size their name columns from the terminal width; the clipboard has no width,
/// so a conventional 80 columns stands in.
const PLAIN_WIDTH: u16 = 80;

/// Format a single solve result as plain text: every tab, in the order the
/// overlay presents them.
pub fn format_solve_result(result: &SolveResult) -> String {
    let tabs = crate::widgets::solve::build_single_solve_cache(result, PLAIN_WIDTH);
    tabs.iter().map(|lines| plain(lines)).collect()
}

/// Format a solve diff comparison as plain text: every tab, in the order the
/// overlay presents them.
pub fn format_solve_diff_result(diff: &SolveDiffResult) -> String {
    let mut text = String::new();
    w!(text, "Solve Comparison");
    w!(text, "File 1: {}", diff.file1_label);
    w!(text, "File 2: {}", diff.file2_label);
    w!(text);

    let crate::app::SolveRenderCache::Diff { summary, log, duals, variable_rows, constraint_rows, .. } =
        crate::widgets::solve::build_diff_solve_cache(diff, PLAIN_WIDTH)
    else {
        debug_assert!(false, "build_diff_solve_cache always returns the Diff variant");
        return text;
    };

    text.push_str(&plain(&summary));
    for (heading, rows) in [("Variables:", &variable_rows), ("Constraints:", &constraint_rows)] {
        w!(text);
        w!(text, "{heading}");
        for row in rows {
            text.push_str(&plain(std::slice::from_ref(&row.line)));
        }
    }
    w!(text);
    text.push_str(&plain(&duals));
    text.push_str(&plain(&log));
    text
}
