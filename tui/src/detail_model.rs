//! Shared data extraction for coefficient/weight rendering in both styled (TUI)
//! and plain-text (clipboard yank) detail views.

use std::collections::BTreeMap;

use lp_parser_rs::model::CoefficientOwned;

use crate::diff_model::{CoefficientChange, DiffKind};

/// A single row in the unified coefficient diff table.
#[derive(Debug)]
pub struct CoefficientRow {
    pub variable: String,
    pub old_value: Option<f64>,
    pub new_value: Option<f64>,
    pub change_kind: Option<DiffKind>,
}

/// Build a sorted list of coefficient rows from old/new coefficient lists and
/// a list of detected changes.
///
/// Every variable that appears in *either* old or new is included. Variables
/// with no entry in `changes` are treated as unchanged.
pub fn build_coeff_rows(
    changes: &[CoefficientChange],
    old_coefficients: &[CoefficientOwned],
    new_coefficients: &[CoefficientOwned],
) -> Vec<CoefficientRow> {
    debug_assert!(
        changes.iter().all(|c| matches!(c.kind, DiffKind::Added | DiffKind::Removed | DiffKind::Modified)),
        "all coefficient changes must have a valid DiffKind"
    );

    // (old_value, new_value, change_kind)
    #[allow(clippy::items_after_statements)]
    type Entry = (Option<f64>, Option<f64>, Option<DiffKind>);

    let mut all_vars: BTreeMap<&str, Entry> = BTreeMap::new();

    for c in old_coefficients {
        all_vars.entry(&c.name).or_default().0 = Some(c.value);
    }
    for c in new_coefficients {
        all_vars.entry(&c.name).or_default().1 = Some(c.value);
    }
    for change in changes {
        if let Some(entry) = all_vars.get_mut(change.variable.as_str()) {
            // Prefer the values already computed during diffing when present.
            if change.old_value.is_some() {
                entry.0 = change.old_value;
            }
            if change.new_value.is_some() {
                entry.1 = change.new_value;
            }
            entry.2 = Some(change.kind);
        }
    }

    all_vars
        .into_iter()
        .map(|(var_name, (old_value, new_value, change_kind))| CoefficientRow {
            variable: var_name.to_owned(),
            old_value,
            new_value,
            change_kind,
        })
        .collect()
}
