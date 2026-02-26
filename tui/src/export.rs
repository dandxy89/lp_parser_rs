//! CSV export of the full diff report.
//!
//! Writes a single `lp_diff_report.csv` summarising all variable, constraint,
//! and objective changes.

use std::error::Error;
use std::fmt::Write as _;
use std::path::Path;

use crate::diff_model::{ConstraintDiffDetail, DiffKind, LpDiffReport};

/// Write the full diff report as a CSV file in `dir`.
///
/// Returns the filename on success.
///
/// # Errors
///
/// Returns an error if the CSV file cannot be created or written to.
pub fn write_diff_csv(report: &LpDiffReport, dir: &Path) -> Result<String, Box<dyn Error>> {
    debug_assert!(dir.is_dir(), "write_diff_csv: dir must be an existing directory");

    let ts = chrono::Local::now().format("%Y%m%d_%H%M%S");
    let filename = format!("lp_diff_report_{ts}.csv");

    let mut wtr = csv::Writer::from_path(dir.join(&filename))?;
    wtr.write_record(["section", "name", "change_type", "detail"])?;

    // Reusable buffer for the detail column, avoiding per-row allocations.
    let mut detail_buf = String::with_capacity(128);

    // Variables.
    for entry in &report.variables.entries {
        detail_buf.clear();
        match entry.kind {
            DiffKind::Added => {
                if let Some(ref new_type) = entry.new_type {
                    write!(detail_buf, "{new_type}").expect("writing to String cannot fail");
                }
            }
            DiffKind::Removed => {
                if let Some(ref old_type) = entry.old_type {
                    write!(detail_buf, "{old_type}").expect("writing to String cannot fail");
                }
            }
            DiffKind::Modified => {
                let old_label = entry.old_type.as_ref().map_or("?", |t| type_label(t));
                let new_label = entry.new_type.as_ref().map_or("?", |t| type_label(t));
                write!(detail_buf, "{old_label} -> {new_label}").expect("writing to String cannot fail");
            }
        }
        wtr.write_record(["Variables", &entry.name, &entry.kind.to_string(), &detail_buf])?;
    }

    // Constraints.
    for entry in &report.constraints.entries {
        detail_buf.clear();
        match &entry.detail {
            ConstraintDiffDetail::Standard { operator_change, rhs_change, coeff_changes, old_rhs, new_rhs, .. } => {
                // For modified entries, summarise what changed.
                if entry.kind == DiffKind::Modified {
                    let mut parts: Vec<String> = Vec::new();
                    if let Some((old_op, new_op)) = operator_change {
                        parts.push(format!("operator: {old_op} -> {new_op}"));
                    }
                    if rhs_change.is_some() {
                        parts.push(format!("rhs: {old_rhs} -> {new_rhs}"));
                    }
                    if !coeff_changes.is_empty() {
                        parts.push(format!("{} coefficient(s) changed", coeff_changes.len()));
                    }
                    write!(detail_buf, "{}", parts.join("; ")).expect("writing to String cannot fail");
                }
            }
            ConstraintDiffDetail::Sos { weight_changes, type_change, .. } => {
                if entry.kind == DiffKind::Modified {
                    let mut parts: Vec<String> = Vec::new();
                    if let Some((old_t, new_t)) = type_change {
                        parts.push(format!("SOS type: {old_t:?} -> {new_t:?}"));
                    }
                    if !weight_changes.is_empty() {
                        parts.push(format!("{} weight(s) changed", weight_changes.len()));
                    }
                    write!(detail_buf, "{}", parts.join("; ")).expect("writing to String cannot fail");
                }
            }
            ConstraintDiffDetail::TypeChanged { old_summary, new_summary } => {
                write!(detail_buf, "{old_summary} -> {new_summary}").expect("writing to String cannot fail");
            }
            ConstraintDiffDetail::AddedOrRemoved(_) => {
                // No extra detail needed for purely added/removed constraints.
            }
        }
        wtr.write_record(["Constraints", &entry.name, &entry.kind.to_string(), &detail_buf])?;
    }

    // Objectives.
    for entry in &report.objectives.entries {
        detail_buf.clear();
        if !entry.coeff_changes.is_empty() {
            write!(detail_buf, "{} coefficient(s) changed", entry.coeff_changes.len()).expect("writing to String cannot fail");
        }
        wtr.write_record(["Objectives", &entry.name, &entry.kind.to_string(), &detail_buf])?;
    }

    wtr.flush()?;
    Ok(filename)
}

/// Return a short label for a `VariableType`, borrowing the Display output
/// without allocating for the common cases.
const fn type_label(t: &lp_parser_rs::model::VariableType) -> &'static str {
    use lp_parser_rs::model::VariableType;
    match t {
        VariableType::Free => "Free",
        VariableType::General => "General",
        VariableType::Binary => "Binary",
        VariableType::Integer => "Integer",
        VariableType::SemiContinuous => "Semi-Continuous",
        VariableType::SOS => "SOS",
        VariableType::LowerBound(_) => "LowerBound",
        VariableType::UpperBound(_) => "UpperBound",
        VariableType::DoubleBound(_, _) => "DoubleBound",
    }
}
