//! Plain-text rendering of detail panel content for clipboard yanking.

use std::collections::BTreeMap;
use std::fmt::Write;

use crate::app::App;
use crate::diff_model::{CoefficientChange, ConstraintDiffDetail, ConstraintDiffEntry, DiffKind, ObjectiveDiffEntry, VariableDiffEntry};
use crate::state::Section;
use crate::widgets::detail::variable_bounds;

/// Render the currently selected detail panel as plain text.
/// Returns `None` if no entry is selected or the section is Summary.
pub fn render_detail_plain(app: &App) -> Option<String> {
    let idx = app.active_section.list_index()?;
    let sel = app.section_states[idx].list_state.selected()?;
    let entry_idx = *app.section_states[idx].cached_indices().get(sel)?;

    match app.active_section {
        Section::Variables => {
            let entry = app.report.variables.entries.get(entry_idx)?;
            Some(render_variable_plain(entry))
        }
        Section::Constraints => {
            let entry = app.report.constraints.entries.get(entry_idx)?;
            Some(render_constraint_plain(entry))
        }
        Section::Objectives => {
            let entry = app.report.objectives.entries.get(entry_idx)?;
            Some(render_objective_plain(entry))
        }
        Section::Summary => None,
    }
}

fn fmt_bound(val: Option<f64>) -> String {
    val.map_or_else(|| "\u{2014}".to_owned(), |v| format!("{v}"))
}

fn render_variable_plain(entry: &VariableDiffEntry) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "Variable: {} [{}]", entry.name, entry.kind);
    let _ = writeln!(out, "{}", "\u{2500}".repeat(38));

    match entry.kind {
        DiffKind::Added => {
            let vt = entry.new_type.as_ref().expect("invariant: Added entry must have new_type");
            let label = std::str::from_utf8(vt.as_ref()).unwrap_or("?");
            let _ = writeln!(out, "  Type:   {label}");
            let (lb, ub) = variable_bounds(vt);
            if let Some(l) = lb {
                let _ = writeln!(out, "  Lower:  {l}");
            }
            if let Some(u) = ub {
                let _ = writeln!(out, "  Upper:  {u}");
            }
            if let (Some(l), Some(u)) = (lb, ub) {
                let _ = writeln!(out, "  Range:  {}", u - l);
            }
        }
        DiffKind::Removed => {
            let vt = entry.old_type.as_ref().expect("invariant: Removed entry must have old_type");
            let label = std::str::from_utf8(vt.as_ref()).unwrap_or("?");
            let _ = writeln!(out, "  Type:   {label}");
            let (lb, ub) = variable_bounds(vt);
            if let Some(l) = lb {
                let _ = writeln!(out, "  Lower:  {l}");
            }
            if let Some(u) = ub {
                let _ = writeln!(out, "  Upper:  {u}");
            }
            if let (Some(l), Some(u)) = (lb, ub) {
                let _ = writeln!(out, "  Range:  {}", u - l);
            }
        }
        DiffKind::Modified => {
            let old = entry.old_type.as_ref().expect("invariant: Modified entry must have old_type");
            let new = entry.new_type.as_ref().expect("invariant: Modified entry must have new_type");
            let old_label = std::str::from_utf8(old.as_ref()).unwrap_or("?");
            let new_label = std::str::from_utf8(new.as_ref()).unwrap_or("?");

            if old_label == new_label {
                let _ = writeln!(out, "  Type:   {old_label} (unchanged)");
            } else {
                let _ = writeln!(out, "  Type:   {old_label}  \u{2192}  {new_label}");
            }

            #[allow(clippy::similar_names)]
            let (old_lb, old_ub) = variable_bounds(old);
            #[allow(clippy::similar_names)]
            let (new_lb, new_ub) = variable_bounds(new);

            if old_lb.is_some() || new_lb.is_some() {
                let _ = writeln!(out, "  Lower:  {}  \u{2192}  {}", fmt_bound(old_lb), fmt_bound(new_lb));
            }
            if old_ub.is_some() || new_ub.is_some() {
                let _ = writeln!(out, "  Upper:  {}  \u{2192}  {}", fmt_bound(old_ub), fmt_bound(new_ub));
            }
            let old_range = old_lb.zip(old_ub).map(|(l, u)| u - l);
            let new_range = new_lb.zip(new_ub).map(|(l, u)| u - l);
            if old_range.is_some() || new_range.is_some() {
                let _ = writeln!(out, "  Range:  {}  \u{2192}  {}", fmt_bound(old_range), fmt_bound(new_range));
            }
        }
    }
    out
}

fn render_constraint_plain(entry: &ConstraintDiffEntry) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "Constraint: {} [{}]", entry.name, entry.kind);
    let _ = writeln!(out, "{}", "\u{2500}".repeat(38));

    if entry.line_file1.is_some() || entry.line_file2.is_some() {
        match (entry.line_file1, entry.line_file2) {
            (Some(l1), Some(l2)) => {
                let _ = writeln!(out, "  Location: L{l1} / L{l2}");
            }
            (Some(l1), None) => {
                let _ = writeln!(out, "  Location: L{l1} (removed)");
            }
            (None, Some(l2)) => {
                let _ = writeln!(out, "  Location: L{l2} (added)");
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
                let _ = writeln!(out, "  Operator: {old_op}  \u{2192}  {new_op}");
            }
            if rhs_change.is_some() {
                let _ = writeln!(out, "  RHS:      {old_rhs}  \u{2192}  {new_rhs}");
            } else {
                let _ = writeln!(out, "  RHS:      {old_rhs} (unchanged)");
            }
            let _ = writeln!(out);
            let _ = writeln!(out, "  Coefficients:");
            write_coeff_changes(&mut out, coeff_changes, old_coefficients, new_coefficients);
        }
        ConstraintDiffDetail::Sos { old_weights, new_weights, weight_changes, type_change } => {
            if let Some((old_type, new_type)) = type_change {
                let _ = writeln!(out, "  SOS Type: {old_type}  \u{2192}  {new_type}");
            }
            let _ = writeln!(out);
            let _ = writeln!(out, "  Weights:");
            write_coeff_changes(&mut out, weight_changes, old_weights, new_weights);
        }
        ConstraintDiffDetail::TypeChanged { old_summary, new_summary } => {
            let _ = writeln!(out, "  Was:  {old_summary}");
            let _ = writeln!(out, "  Now:  {new_summary}");
        }
        ConstraintDiffDetail::AddedOrRemoved(constraint) => {
            use lp_parser_rs::model::ConstraintOwned;
            match constraint {
                ConstraintOwned::Standard { coefficients, operator, rhs, .. } => {
                    let _ = writeln!(out, "  Operator: {operator}");
                    let _ = writeln!(out, "  RHS:      {rhs}");
                    let _ = writeln!(out);
                    let _ = writeln!(out, "  Coefficients:");
                    for c in coefficients {
                        let _ = writeln!(out, "    {:<20}{}", c.name, c.value);
                    }
                }
                ConstraintOwned::SOS { sos_type, weights, .. } => {
                    let _ = writeln!(out, "  SOS Type: {sos_type}");
                    let _ = writeln!(out);
                    let _ = writeln!(out, "  Weights:");
                    for w in weights {
                        let _ = writeln!(out, "    {:<20}{}", w.name, w.value);
                    }
                }
            }
        }
    }
    out
}

fn render_objective_plain(entry: &ObjectiveDiffEntry) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "Objective: {} [{}]", entry.name, entry.kind);
    let _ = writeln!(out, "{}", "\u{2500}".repeat(38));
    let _ = writeln!(out, "  Coefficients:");

    if entry.kind == DiffKind::Modified {
        write_coeff_changes(&mut out, &entry.coeff_changes, &entry.old_coefficients, &entry.new_coefficients);
    } else {
        let coeffs = if entry.kind == DiffKind::Added { &entry.new_coefficients } else { &entry.old_coefficients };
        for c in coeffs {
            let _ = writeln!(out, "    {:<20}{}", c.name, c.value);
        }
    }
    out
}

/// Column width for value formatting.
const VAL_WIDTH: usize = 12;

fn write_coeff_changes(
    out: &mut String,
    changes: &[CoefficientChange],
    old_coefficients: &[lp_parser_rs::model::CoefficientOwned],
    new_coefficients: &[lp_parser_rs::model::CoefficientOwned],
) {
    type CoeffEntry = (Option<f64>, Option<f64>, Option<DiffKind>);

    let mut all_vars: BTreeMap<String, CoeffEntry> = BTreeMap::new();
    for c in old_coefficients {
        all_vars.entry(c.name.clone()).or_default().0 = Some(c.value);
    }
    for c in new_coefficients {
        all_vars.entry(c.name.clone()).or_default().1 = Some(c.value);
    }
    for change in changes {
        if let Some(entry) = all_vars.get_mut(&change.variable) {
            entry.2 = Some(change.kind);
        }
    }

    for (var_name, (old_val, new_val, change_kind)) in &all_vars {
        let old_str = old_val.map_or_else(String::new, |v| format!("{v}"));
        let new_str = new_val.map_or_else(String::new, |v| format!("{v}"));

        match change_kind {
            Some(DiffKind::Added) => {
                let _ = writeln!(out, "    {var_name:<20}{:>VAL_WIDTH$}  \u{2192}  {new_str:<VAL_WIDTH$} [added]", "");
            }
            Some(DiffKind::Removed) => {
                let _ = writeln!(out, "    {var_name:<20}{old_str:>VAL_WIDTH$}  \u{2192}  {:VAL_WIDTH$} [removed]", "");
            }
            Some(DiffKind::Modified) => {
                let _ = writeln!(out, "    {var_name:<20}{old_str:>VAL_WIDTH$}  \u{2192}  {new_str:<VAL_WIDTH$} [modified]");
            }
            None => {
                let _ = writeln!(out, "    {var_name:<20}{old_str:>VAL_WIDTH$} (unchanged)");
            }
        }
    }
}
