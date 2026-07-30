//! CSV export of the full diff report.
//!
//! Writes a single `lp_diff_report.csv` summarising all variable, constraint,
//! and objective changes.

use std::error::Error;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::diff_model::{ConstraintDiffDetail, DiffKind, LpDiffReport};

/// Seconds since the Unix epoch, used to make exported filenames unique within
/// a session. Deliberately not a wall-clock timestamp: uniqueness is all a
/// filename needs, and formatting local time would cost a dependency.
pub fn file_stamp() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |since| since.as_secs())
}

/// Write the full diff report as a CSV file in `dir`.
///
/// Returns the filename on success.
///
/// # Errors
///
/// Returns an error if the CSV file cannot be created or written to.
pub fn write_diff_csv(report: &LpDiffReport, dir: &Path) -> Result<String, Box<dyn Error>> {
    debug_assert!(dir.is_dir(), "write_diff_csv: dir must be an existing directory");

    let ts = file_stamp();
    let filename = format!("lp_diff_report_{ts}.csv");

    let mut wtr = csv::Writer::from_path(dir.join(&filename))?;
    wtr.write_record(["section", "name", "change_type", "detail"])?;

    // Variables.
    for entry in &report.variables.entries {
        let detail = match entry.kind {
            DiffKind::Added => entry.new_type.as_ref().map_or_else(String::new, ToString::to_string),
            DiffKind::Removed => entry.old_type.as_ref().map_or_else(String::new, ToString::to_string),
            DiffKind::Modified => {
                let old_label = entry.old_type.as_ref().map_or_else(|| "?".to_owned(), ToString::to_string);
                let new_label = entry.new_type.as_ref().map_or_else(|| "?".to_owned(), ToString::to_string);
                format!("{old_label} -> {new_label}")
            }
            DiffKind::Renamed => {
                // Rename detection applies to constraints only; variables never carry Renamed.
                debug_assert!(false, "variable entry cannot be Renamed");
                String::new()
            }
        };
        wtr.write_record(["Variables", &entry.name, &entry.kind.to_string(), &detail])?;
    }

    // Constraints.
    for entry in &report.constraints.entries {
        let detail = if let Some(old_name) = &entry.renamed_from {
            format!("renamed from {old_name}")
        } else if entry.order_only {
            "order change only".to_owned()
        } else {
            constraint_detail(entry)
        };
        wtr.write_record(["Constraints", &entry.name, &entry.kind.to_string(), &detail])?;
    }

    // Objectives.
    for entry in &report.objectives.entries {
        let mut parts: Vec<String> = Vec::new();
        if entry.order_only {
            parts.push("order change only".to_owned());
        } else {
            if !entry.coeff_changes.is_empty() {
                parts.push(format!("{} coefficient(s) changed", entry.coeff_changes.len()));
            }
            if entry.order_changed {
                parts.push("order changed".to_owned());
            }
        }
        wtr.write_record(["Objectives", &entry.name, &entry.kind.to_string(), &parts.join("; ")])?;
    }

    wtr.flush()?;
    Ok(filename)
}

/// Summarise what changed in a modified constraint, for the CSV detail column.
/// Added, removed and unchanged constraints need no extra detail.
fn constraint_detail(entry: &crate::diff_model::ConstraintDiffEntry) -> String {
    // A Standard↔SOS swap is worth reporting whatever kind the entry carries.
    if let ConstraintDiffDetail::TypeChanged { old_summary, new_summary } = &entry.detail {
        return format!("{old_summary} -> {new_summary}");
    }
    if entry.kind != DiffKind::Modified {
        return String::new();
    }
    let mut parts: Vec<String> = Vec::new();
    match &entry.detail {
        ConstraintDiffDetail::Standard { operator_change, rhs_change, coeff_changes, old_rhs, new_rhs, order_changed, .. } => {
            if let Some((old_op, new_op)) = operator_change {
                parts.push(format!("operator: {old_op} -> {new_op}"));
            }
            if rhs_change.is_some() {
                parts.push(format!("rhs: {old_rhs} -> {new_rhs}"));
            }
            if !coeff_changes.is_empty() {
                parts.push(format!("{} coefficient(s) changed", coeff_changes.len()));
            }
            if *order_changed {
                parts.push("order changed".to_owned());
            }
        }
        ConstraintDiffDetail::Sos { weight_changes, type_change, order_changed, .. } => {
            if let Some((old_t, new_t)) = type_change {
                parts.push(format!("SOS type: {old_t:?} -> {new_t:?}"));
            }
            if !weight_changes.is_empty() {
                parts.push(format!("{} weight(s) changed", weight_changes.len()));
            }
            if *order_changed {
                parts.push("order changed".to_owned());
            }
        }
        // Handled above, before the Modified guard.
        ConstraintDiffDetail::TypeChanged { .. } => unreachable!("TypeChanged returns early"),
        // No extra detail needed for purely added/removed constraints.
        ConstraintDiffDetail::AddedOrRemoved(_) => {}
    }
    parts.join("; ")
}
