//! CSV export of the full diff report.
//!
//! Writes a single `lp_diff_report.csv` summarising all variable, constraint,
//! and objective changes.

use std::error::Error;
use std::fmt::Write as _;
use std::path::Path;

use crate::diff_model::{ConstraintDiffDetail, DiffKind, LpDiffReport};

/// Writing to a `String` via `fmt::Write` is infallible. This macro replaces
/// `let _ = write!(...)` with an asserting version that satisfies Tiger Style.
/// (No trailing newline — CSV fields must stay single-line, so this is
/// deliberately `write!` rather than `detail_text`'s `writeln!`-based `w!`.)
macro_rules! w {
    ($dst:expr, $($arg:tt)*) => {
        write!($dst, $($arg)*).expect("writing to String is infallible")
    };
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
                    w!(detail_buf, "{new_type}");
                }
            }
            DiffKind::Removed => {
                if let Some(ref old_type) = entry.old_type {
                    w!(detail_buf, "{old_type}");
                }
            }
            DiffKind::Modified => {
                let old_label = entry.old_type.as_ref().map_or_else(|| "?".to_owned(), ToString::to_string);
                let new_label = entry.new_type.as_ref().map_or_else(|| "?".to_owned(), ToString::to_string);
                w!(detail_buf, "{old_label} -> {new_label}");
            }
            DiffKind::Renamed => {
                // Rename detection applies to constraints only; variables never carry Renamed.
                debug_assert!(false, "variable entry cannot be Renamed");
            }
        }
        wtr.write_record(["Variables", &entry.name, &entry.kind.to_string(), &detail_buf])?;
    }

    // Constraints.
    for entry in &report.constraints.entries {
        detail_buf.clear();
        if let Some(old_name) = &entry.renamed_from {
            w!(detail_buf, "renamed from {old_name}");
        } else if entry.order_only {
            w!(detail_buf, "order change only");
        } else {
            match &entry.detail {
                ConstraintDiffDetail::Standard { operator_change, rhs_change, coeff_changes, old_rhs, new_rhs, order_changed, .. } => {
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
                        if *order_changed {
                            parts.push("order changed".to_owned());
                        }
                        w!(detail_buf, "{}", parts.join("; "));
                    }
                }
                ConstraintDiffDetail::Sos { weight_changes, type_change, order_changed, .. } => {
                    if entry.kind == DiffKind::Modified {
                        let mut parts: Vec<String> = Vec::new();
                        if let Some((old_t, new_t)) = type_change {
                            parts.push(format!("SOS type: {old_t:?} -> {new_t:?}"));
                        }
                        if !weight_changes.is_empty() {
                            parts.push(format!("{} weight(s) changed", weight_changes.len()));
                        }
                        if *order_changed {
                            parts.push("order changed".to_owned());
                        }
                        w!(detail_buf, "{}", parts.join("; "));
                    }
                }
                ConstraintDiffDetail::TypeChanged { old_summary, new_summary } => {
                    w!(detail_buf, "{old_summary} -> {new_summary}");
                }
                ConstraintDiffDetail::AddedOrRemoved(_) => {
                    // No extra detail needed for purely added/removed constraints.
                }
            }
        } // end else (not order_only)
        wtr.write_record(["Constraints", &entry.name, &entry.kind.to_string(), &detail_buf])?;
    }

    // Objectives.
    for entry in &report.objectives.entries {
        detail_buf.clear();
        if entry.order_only {
            w!(detail_buf, "order change only");
        } else if !entry.coeff_changes.is_empty() {
            let mut msg = format!("{} coefficient(s) changed", entry.coeff_changes.len());
            if entry.order_changed {
                msg.push_str("; order changed");
            }
            w!(detail_buf, "{msg}");
        } else if entry.order_changed {
            w!(detail_buf, "order changed");
        }
        wtr.write_record(["Objectives", &entry.name, &entry.kind.to_string(), &detail_buf])?;
    }

    wtr.flush()?;
    Ok(filename)
}
