//! Rich diff data model and algorithm for comparing two LP problems.
//!
//! This module is pure data types and logic — no ratatui dependency.

use std::collections::HashMap;
use std::fmt;

use lp_parser_rs::model::{CoefficientOwned, ComparisonOp, ConstraintOwned, SOSType, Sense, VariableType};
use lp_parser_rs::problem::LpProblemOwned;

/// Separator used between fields in searchable text strings.
/// NUL cannot appear in valid LP identifiers, avoiding false substring matches.
const SEARCH_FIELD_SEP: char = '\0';

/// Build a searchable text string from an entry name and extra field names
/// (e.g. coefficient variable names). Fields are separated by [`SEARCH_FIELD_SEP`]
/// so they won't collide with valid LP identifiers.
fn build_searchable_text(name: &str, extra: impl Iterator<Item = impl AsRef<str>>) -> String {
    let mut text = String::from(name);
    for field in extra {
        text.push(SEARCH_FIELD_SEP);
        text.push_str(field.as_ref());
    }
    text
}

// Epsilon used for floating-point coefficient value comparison.
const COEFF_EPSILON: f64 = 1e-10;

/// Index mapping byte offsets to 1-based line numbers within source text.
///
/// Built once per input file then used to convert `Constraint::byte_offset`
/// values into human-readable line numbers before the input text is dropped.
pub struct LineIndex {
    /// Byte offsets of each line start (index 0 → byte 0, index 1 → first byte after first '\n', etc.).
    line_starts: Vec<usize>,
}

impl LineIndex {
    /// Build a line index from the full source text.
    #[must_use]
    pub fn new(source: &str) -> Self {
        debug_assert!(!source.is_empty(), "LineIndex::new called with empty source");
        let mut line_starts = vec![0];
        for (i, byte) in source.bytes().enumerate() {
            if byte == b'\n' {
                line_starts.push(i + 1);
            }
        }
        Self { line_starts }
    }

    /// Convert a byte offset to a 1-based line number.
    ///
    /// Returns `None` if `byte_offset` is past the end of the source.
    #[must_use]
    pub fn line_number(&self, byte_offset: usize) -> Option<usize> {
        if self.line_starts.is_empty() {
            return None;
        }
        // Binary search: find the last line_start <= byte_offset.
        match self.line_starts.binary_search(&byte_offset) {
            Ok(idx) => Some(idx + 1),
            Err(idx) => {
                if idx == 0 {
                    None
                } else {
                    Some(idx) // idx is insertion point; line number is idx (1-based)
                }
            }
        }
    }
}

/// A complete diff report between two LP problem files.
#[derive(Debug)]
pub struct LpDiffReport {
    /// Path or label for the first file.
    pub file1: String,
    /// Path or label for the second file.
    pub file2: String,
    /// Set when the optimisation sense differs between the two problems.
    pub sense_changed: Option<(Sense, Sense)>,
    /// Set when the problem name differs (including None → Some or vice versa).
    pub name_changed: Option<(Option<String>, Option<String>)>,
    /// Per-variable diff entries.
    pub variables: SectionDiff<VariableDiffEntry>,
    /// Per-constraint diff entries.
    pub constraints: SectionDiff<ConstraintDiffEntry>,
    /// Per-objective diff entries.
    pub objectives: SectionDiff<ObjectiveDiffEntry>,
}

impl LpDiffReport {
    /// Derive a high-level summary from the per-section counts.
    #[must_use]
    pub const fn summary(&self) -> DiffSummary {
        DiffSummary { variables: self.variables.counts, constraints: self.constraints.counts, objectives: self.objectives.counts }
    }
}

/// Diff results for one section (variables, constraints, or objectives).
///
/// Only stores CHANGED entries. Unchanged entries are counted but not stored.
#[derive(Debug)]
pub struct SectionDiff<T> {
    /// Added, removed, or modified entries sorted by name.
    pub entries: Vec<T>,
    /// Counts for all entry states in this section.
    pub counts: DiffCounts,
}

/// Counts of entries in each diff state within a section.
#[derive(Debug, Clone, Copy, Default)]
pub struct DiffCounts {
    pub added: usize,
    pub removed: usize,
    pub modified: usize,
    pub unchanged: usize,
}

impl DiffCounts {
    /// Total number of entries (all states combined).
    #[must_use]
    pub const fn total(&self) -> usize {
        self.added + self.removed + self.modified + self.unchanged
    }

    /// Number of entries that differ in any way.
    #[must_use]
    pub const fn changed(&self) -> usize {
        self.added + self.removed + self.modified
    }
}

/// High-level summary of changes across all three sections.
#[derive(Debug, Clone, Copy, Default)]
pub struct DiffSummary {
    pub variables: DiffCounts,
    pub constraints: DiffCounts,
    pub objectives: DiffCounts,
}

impl DiffSummary {
    /// Total number of changed entries across all sections.
    #[must_use]
    pub const fn total_changes(&self) -> usize {
        self.variables.changed() + self.constraints.changed() + self.objectives.changed()
    }
}

/// The kind of change represented by a diff entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffKind {
    Added,
    Removed,
    Modified,
}

impl fmt::Display for DiffKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Added => write!(f, "Added"),
            Self::Removed => write!(f, "Removed"),
            Self::Modified => write!(f, "Modified"),
        }
    }
}

/// Diff entry for a single variable.
#[derive(Debug, Clone)]
pub struct VariableDiffEntry {
    pub name: String,
    pub kind: DiffKind,
    /// Type in the first file; None when the variable was added.
    pub old_type: Option<VariableType>,
    /// Type in the second file; None when the variable was removed.
    pub new_type: Option<VariableType>,
    /// Pre-built text for search matching (name only for variables).
    pub searchable_text: String,
}

/// Diff entry for a single constraint.
#[derive(Debug, Clone)]
pub struct ConstraintDiffEntry {
    pub name: String,
    pub kind: DiffKind,
    pub detail: ConstraintDiffDetail,
    /// Pre-built text for search matching (name + coefficient/weight variable names).
    pub searchable_text: String,
    /// 1-based line number in the first file, if known.
    pub line_file1: Option<usize>,
    /// 1-based line number in the second file, if known.
    pub line_file2: Option<usize>,
}

/// Detailed change information for a constraint diff entry.
#[derive(Debug, Clone)]
pub enum ConstraintDiffDetail {
    /// Both versions are standard linear constraints.
    Standard {
        old_coefficients: Vec<CoefficientOwned>,
        new_coefficients: Vec<CoefficientOwned>,
        coeff_changes: Vec<CoefficientChange>,
        operator_change: Option<(ComparisonOp, ComparisonOp)>,
        rhs_change: Option<(f64, f64)>,
        old_rhs: f64,
        new_rhs: f64,
    },
    /// Both versions are SOS constraints.
    Sos {
        old_weights: Vec<CoefficientOwned>,
        new_weights: Vec<CoefficientOwned>,
        weight_changes: Vec<CoefficientChange>,
        type_change: Option<(SOSType, SOSType)>,
    },
    /// Constraint changed from Standard to SOS or vice versa.
    TypeChanged { old_summary: String, new_summary: String },
    /// Constraint exists only in one of the two problems.
    AddedOrRemoved(ConstraintOwned),
}

/// A change to a single coefficient (or weight) within a constraint or objective.
#[derive(Debug, Clone)]
pub struct CoefficientChange {
    pub variable: String,
    pub kind: DiffKind,
    /// Value in the first problem; `None` when the coefficient was added.
    ///
    /// Used by tests to verify diff correctness; part of the public diff API
    /// for future consumers (e.g. non-TUI formatters).
    #[allow(dead_code)]
    pub old_value: Option<f64>,
    /// Value in the second problem; `None` when the coefficient was removed.
    ///
    /// Used by tests to verify diff correctness; part of the public diff API
    /// for future consumers (e.g. non-TUI formatters).
    #[allow(dead_code)]
    pub new_value: Option<f64>,
}

/// Diff entry for a single objective.
#[derive(Debug, Clone)]
pub struct ObjectiveDiffEntry {
    pub name: String,
    pub kind: DiffKind,
    pub old_coefficients: Vec<CoefficientOwned>,
    pub new_coefficients: Vec<CoefficientOwned>,
    pub coeff_changes: Vec<CoefficientChange>,
    /// Pre-built text for search matching (name + coefficient variable names).
    pub searchable_text: String,
}

/// Trait implemented by all diff entry types so the TUI can render them uniformly.
pub trait DiffEntry {
    fn name(&self) -> &str;
    fn kind(&self) -> DiffKind;
    /// Pre-built text for search matching (name + variable names from coefficients/weights).
    fn searchable_text(&self) -> &str;
    /// A single-line human-readable summary of what changed.
    ///
    /// Used by tests to verify diff correctness; part of the public diff API
    /// for future consumers (e.g. non-TUI formatters).
    #[allow(dead_code)]
    fn summary_line(&self) -> String;
}

impl DiffEntry for VariableDiffEntry {
    fn name(&self) -> &str {
        &self.name
    }

    fn kind(&self) -> DiffKind {
        self.kind
    }

    fn searchable_text(&self) -> &str {
        &self.searchable_text
    }

    fn summary_line(&self) -> String {
        match self.kind {
            DiffKind::Added => {
                let var_type = self.new_type.as_ref().expect("invariant: Added entry must have new_type");
                format!("{var_type}")
            }
            DiffKind::Removed => {
                let var_type = self.old_type.as_ref().expect("invariant: Removed entry must have old_type");
                format!("{var_type}")
            }
            DiffKind::Modified => {
                let old = self.old_type.as_ref().expect("invariant: Modified entry must have old_type");
                let new = self.new_type.as_ref().expect("invariant: Modified entry must have new_type");
                format!("{old} \u{2192} {new}")
            }
        }
    }
}

impl DiffEntry for ConstraintDiffEntry {
    fn name(&self) -> &str {
        &self.name
    }

    fn kind(&self) -> DiffKind {
        self.kind
    }

    fn searchable_text(&self) -> &str {
        &self.searchable_text
    }

    fn summary_line(&self) -> String {
        match &self.detail {
            ConstraintDiffDetail::AddedOrRemoved(constraint) => match constraint {
                ConstraintOwned::Standard { coefficients, .. } => {
                    format!("Standard, {} coefficient(s)", coefficients.len())
                }
                ConstraintOwned::SOS { sos_type, weights, .. } => {
                    format!("{sos_type}, {} weight(s)", weights.len())
                }
            },
            ConstraintDiffDetail::Standard { coeff_changes, operator_change, rhs_change, .. } => {
                let mut parts = vec![format!("{} coeff change(s)", coeff_changes.len())];
                if let Some((old_op, new_op)) = operator_change {
                    parts.push(format!("op {old_op}\u{2192}{new_op}"));
                }
                if let Some((old_rhs, new_rhs)) = rhs_change {
                    parts.push(format!("rhs {old_rhs}\u{2192}{new_rhs}"));
                }
                parts.join(", ")
            }
            ConstraintDiffDetail::Sos { weight_changes, type_change, .. } => {
                let mut parts = vec![format!("{} weight change(s)", weight_changes.len())];
                if let Some((old_type, new_type)) = type_change {
                    parts.push(format!("type {old_type}\u{2192}{new_type}"));
                }
                parts.join(", ")
            }
            ConstraintDiffDetail::TypeChanged { old_summary, new_summary } => {
                format!("{old_summary} \u{2192} {new_summary}")
            }
        }
    }
}

impl DiffEntry for ObjectiveDiffEntry {
    fn name(&self) -> &str {
        &self.name
    }

    fn kind(&self) -> DiffKind {
        self.kind
    }

    fn searchable_text(&self) -> &str {
        &self.searchable_text
    }

    fn summary_line(&self) -> String {
        match self.kind {
            DiffKind::Added => format!("{} coefficient(s)", self.new_coefficients.len()),
            DiffKind::Removed => format!("{} coefficient(s)", self.old_coefficients.len()),
            DiffKind::Modified => format!("{} coeff change(s)", self.coeff_changes.len()),
        }
    }
}

/// Collect deduplicated variable names from a constraint (union of old + new coefficients/weights).
fn constraint_variable_names(c: &ConstraintOwned) -> Vec<&str> {
    match c {
        ConstraintOwned::Standard { coefficients, .. } => coefficients.iter().map(|c| c.name.as_str()).collect(),
        ConstraintOwned::SOS { weights, .. } => weights.iter().map(|w| w.name.as_str()).collect(),
    }
}

/// Build searchable text for a constraint entry from its detail.
fn constraint_searchable_text(name: &str, detail: &ConstraintDiffDetail) -> String {
    match detail {
        ConstraintDiffDetail::Standard { old_coefficients, new_coefficients, .. } => {
            let vars = sorted_key_union(old_coefficients.iter().map(|c| c.name.as_str()), new_coefficients.iter().map(|c| c.name.as_str()));
            build_searchable_text(name, vars.into_iter())
        }
        ConstraintDiffDetail::Sos { old_weights, new_weights, .. } => {
            let vars = sorted_key_union(old_weights.iter().map(|w| w.name.as_str()), new_weights.iter().map(|w| w.name.as_str()));
            build_searchable_text(name, vars.into_iter())
        }
        ConstraintDiffDetail::TypeChanged { .. } => build_searchable_text(name, std::iter::empty::<&str>()),
        ConstraintDiffDetail::AddedOrRemoved(c) => {
            let vars = constraint_variable_names(c);
            build_searchable_text(name, vars.into_iter())
        }
    }
}

/// Collect the sorted, deduplicated union of keys from two iterators.
fn sorted_key_union<'a>(left: impl Iterator<Item = &'a str>, right: impl Iterator<Item = &'a str>) -> Vec<&'a str> {
    let mut keys: Vec<&str> = left.chain(right).collect();
    keys.sort_unstable();
    keys.dedup();
    keys
}

/// Diff two coefficient lists and return only the changed entries.
///
/// Uses an epsilon of [`COEFF_EPSILON`] for float comparison.
fn diff_coefficients(old: &[CoefficientOwned], new: &[CoefficientOwned]) -> Vec<CoefficientChange> {
    let old_map: HashMap<&str, f64> = old.iter().map(|c| (c.name.as_str(), c.value)).collect();
    let new_map: HashMap<&str, f64> = new.iter().map(|c| (c.name.as_str(), c.value)).collect();

    let all_names = sorted_key_union(old_map.keys().copied(), new_map.keys().copied());

    let mut changes = Vec::new();

    for name in all_names {
        match (old_map.get(name), new_map.get(name)) {
            (Some(&old_val), None) => {
                changes.push(CoefficientChange {
                    variable: name.to_string(),
                    kind: DiffKind::Removed,
                    old_value: Some(old_val),
                    new_value: None,
                });
            }
            (None, Some(&new_val)) => {
                changes.push(CoefficientChange {
                    variable: name.to_string(),
                    kind: DiffKind::Added,
                    old_value: None,
                    new_value: Some(new_val),
                });
            }
            (Some(&old_val), Some(&new_val)) => {
                if (old_val - new_val).abs() > COEFF_EPSILON {
                    changes.push(CoefficientChange {
                        variable: name.to_string(),
                        kind: DiffKind::Modified,
                        old_value: Some(old_val),
                        new_value: Some(new_val),
                    });
                }
                // Unchanged coefficients are skipped.
            }
            (None, None) => {
                unreachable!("name in union but absent from both maps");
            }
        }
    }

    changes
}

/// Build a short human-readable summary string for a constraint (used in `TypeChanged`).
fn constraint_summary(constraint: &ConstraintOwned) -> String {
    match constraint {
        ConstraintOwned::Standard { coefficients, operator, rhs, .. } => {
            format!("Standard({} coeffs, {operator}, {rhs})", coefficients.len())
        }
        ConstraintOwned::SOS { sos_type, weights, .. } => {
            format!("SOS({sos_type}, {} weights)", weights.len())
        }
    }
}

/// Diff the variables section between two problems.
fn diff_variables(p1: &LpProblemOwned, p2: &LpProblemOwned) -> SectionDiff<VariableDiffEntry> {
    let all_names = sorted_key_union(p1.variables.keys().map(String::as_str), p2.variables.keys().map(String::as_str));

    let mut entries = Vec::new();
    let mut counts = DiffCounts::default();

    for name in all_names {
        let in_p1 = p1.variables.get(name);
        let in_p2 = p2.variables.get(name);

        match (in_p1, in_p2) {
            (Some(v1), None) => {
                counts.removed += 1;
                let searchable_text = build_searchable_text(name, std::iter::empty::<&str>());
                let entry = VariableDiffEntry {
                    name: name.to_string(),
                    kind: DiffKind::Removed,
                    old_type: Some(v1.var_type.clone()),
                    new_type: None,
                    searchable_text,
                };
                debug_assert!(entry.old_type.is_some(), "Removed variable must have old_type");
                debug_assert!(entry.new_type.is_none(), "Removed variable must not have new_type");
                entries.push(entry);
            }
            (None, Some(v2)) => {
                counts.added += 1;
                let searchable_text = build_searchable_text(name, std::iter::empty::<&str>());
                let entry = VariableDiffEntry {
                    name: name.to_string(),
                    kind: DiffKind::Added,
                    old_type: None,
                    new_type: Some(v2.var_type.clone()),
                    searchable_text,
                };
                debug_assert!(entry.new_type.is_some(), "Added variable must have new_type");
                debug_assert!(entry.old_type.is_none(), "Added variable must not have old_type");
                entries.push(entry);
            }
            (Some(v1), Some(v2)) => {
                if v1.var_type == v2.var_type {
                    counts.unchanged += 1;
                } else {
                    counts.modified += 1;
                    let searchable_text = build_searchable_text(name, std::iter::empty::<&str>());
                    let entry = VariableDiffEntry {
                        name: name.to_string(),
                        kind: DiffKind::Modified,
                        old_type: Some(v1.var_type.clone()),
                        new_type: Some(v2.var_type.clone()),
                        searchable_text,
                    };
                    debug_assert!(entry.old_type.is_some(), "Modified variable must have old_type");
                    debug_assert!(entry.new_type.is_some(), "Modified variable must have new_type");
                    entries.push(entry);
                }
            }
            (None, None) => {
                unreachable!("name in union but absent from both problems");
            }
        }
    }

    // Entries are already sorted by name because we sorted all_names above.
    SectionDiff { entries, counts }
}

/// Diff the constraints section between two problems.
fn diff_constraints(
    p1: &LpProblemOwned,
    p2: &LpProblemOwned,
    line_map1: &HashMap<String, usize>,
    line_map2: &HashMap<String, usize>,
) -> SectionDiff<ConstraintDiffEntry> {
    let all_names = sorted_key_union(p1.constraints.keys().map(String::as_str), p2.constraints.keys().map(String::as_str));

    let mut entries = Vec::new();
    let mut counts = DiffCounts::default();

    for name in all_names {
        let c1 = p1.constraints.get(name);
        let c2 = p2.constraints.get(name);
        let l1 = line_map1.get(name).copied();
        let l2 = line_map2.get(name).copied();

        match (c1, c2) {
            (Some(c), None) => {
                counts.removed += 1;
                let detail = ConstraintDiffDetail::AddedOrRemoved(c.clone());
                let searchable_text = constraint_searchable_text(name, &detail);
                entries.push(ConstraintDiffEntry {
                    name: name.to_string(),
                    kind: DiffKind::Removed,
                    detail,
                    searchable_text,
                    line_file1: l1,
                    line_file2: l2,
                });
            }
            (None, Some(c)) => {
                counts.added += 1;
                let detail = ConstraintDiffDetail::AddedOrRemoved(c.clone());
                let searchable_text = constraint_searchable_text(name, &detail);
                entries.push(ConstraintDiffEntry {
                    name: name.to_string(),
                    kind: DiffKind::Added,
                    detail,
                    searchable_text,
                    line_file1: l1,
                    line_file2: l2,
                });
            }
            (Some(c1), Some(c2)) => {
                let detail = match (c1, c2) {
                    // Standard vs SOS: structurally incompatible.
                    (ConstraintOwned::Standard { .. }, ConstraintOwned::SOS { .. })
                    | (ConstraintOwned::SOS { .. }, ConstraintOwned::Standard { .. }) => {
                        Some(ConstraintDiffDetail::TypeChanged { old_summary: constraint_summary(c1), new_summary: constraint_summary(c2) })
                    }

                    // Both standard: diff coefficients, operator, rhs.
                    (
                        ConstraintOwned::Standard { coefficients: old_coeffs, operator: old_op, rhs: old_rhs, .. },
                        ConstraintOwned::Standard { coefficients: new_coeffs, operator: new_op, rhs: new_rhs, .. },
                    ) => {
                        let coeff_changes = diff_coefficients(old_coeffs, new_coeffs);
                        let operator_change = if old_op == new_op { None } else { Some((old_op.clone(), new_op.clone())) };
                        let rhs_change = if (old_rhs - new_rhs).abs() > COEFF_EPSILON { Some((*old_rhs, *new_rhs)) } else { None };

                        if coeff_changes.is_empty() && operator_change.is_none() && rhs_change.is_none() {
                            // No change at all.
                            None
                        } else {
                            Some(ConstraintDiffDetail::Standard {
                                old_coefficients: old_coeffs.clone(),
                                new_coefficients: new_coeffs.clone(),
                                coeff_changes,
                                operator_change,
                                rhs_change,
                                old_rhs: *old_rhs,
                                new_rhs: *new_rhs,
                            })
                        }
                    }

                    // Both SOS: diff weights and sos_type.
                    (
                        ConstraintOwned::SOS { sos_type: old_type, weights: old_weights, .. },
                        ConstraintOwned::SOS { sos_type: new_type, weights: new_weights, .. },
                    ) => {
                        let weight_changes = diff_coefficients(old_weights, new_weights);
                        let type_change = if old_type == new_type { None } else { Some((old_type.clone(), new_type.clone())) };

                        if weight_changes.is_empty() && type_change.is_none() {
                            None
                        } else {
                            Some(ConstraintDiffDetail::Sos {
                                old_weights: old_weights.clone(),
                                new_weights: new_weights.clone(),
                                weight_changes,
                                type_change,
                            })
                        }
                    }
                };

                if let Some(d) = detail {
                    counts.modified += 1;
                    let searchable_text = constraint_searchable_text(name, &d);
                    entries.push(ConstraintDiffEntry {
                        name: name.to_string(),
                        kind: DiffKind::Modified,
                        detail: d,
                        searchable_text,
                        line_file1: l1,
                        line_file2: l2,
                    });
                } else {
                    counts.unchanged += 1;
                }
            }
            (None, None) => {
                unreachable!("name in union but absent from both problems");
            }
        }
    }

    SectionDiff { entries, counts }
}

/// Diff the objectives section between two problems.
fn diff_objectives(p1: &LpProblemOwned, p2: &LpProblemOwned) -> SectionDiff<ObjectiveDiffEntry> {
    let all_names = sorted_key_union(p1.objectives.keys().map(String::as_str), p2.objectives.keys().map(String::as_str));

    let mut entries = Vec::new();
    let mut counts = DiffCounts::default();

    for name in all_names {
        let o1 = p1.objectives.get(name);
        let o2 = p2.objectives.get(name);

        match (o1, o2) {
            (Some(o), None) => {
                counts.removed += 1;
                let searchable_text = build_searchable_text(name, o.coefficients.iter().map(|c| &c.name));
                entries.push(ObjectiveDiffEntry {
                    name: name.to_string(),
                    kind: DiffKind::Removed,
                    old_coefficients: o.coefficients.clone(),
                    new_coefficients: Vec::new(),
                    coeff_changes: Vec::new(),
                    searchable_text,
                });
            }
            (None, Some(o)) => {
                counts.added += 1;
                let searchable_text = build_searchable_text(name, o.coefficients.iter().map(|c| &c.name));
                entries.push(ObjectiveDiffEntry {
                    name: name.to_string(),
                    kind: DiffKind::Added,
                    old_coefficients: Vec::new(),
                    new_coefficients: o.coefficients.clone(),
                    coeff_changes: Vec::new(),
                    searchable_text,
                });
            }
            (Some(o1), Some(o2)) => {
                let coeff_changes = diff_coefficients(&o1.coefficients, &o2.coefficients);
                if coeff_changes.is_empty() {
                    counts.unchanged += 1;
                } else {
                    counts.modified += 1;
                    let vars =
                        sorted_key_union(o1.coefficients.iter().map(|c| c.name.as_str()), o2.coefficients.iter().map(|c| c.name.as_str()));
                    let searchable_text = build_searchable_text(name, vars.into_iter());
                    entries.push(ObjectiveDiffEntry {
                        name: name.to_string(),
                        kind: DiffKind::Modified,
                        old_coefficients: o1.coefficients.clone(),
                        new_coefficients: o2.coefficients.clone(),
                        coeff_changes,
                        searchable_text,
                    });
                }
            }
            (None, None) => {
                unreachable!("name in union but absent from both problems");
            }
        }
    }

    SectionDiff { entries, counts }
}

/// Build a complete diff report comparing two LP problems.
///
/// # Arguments
///
/// * `file1` - Label or path for the first problem (used in the report).
/// * `file2` - Label or path for the second problem (used in the report).
/// * `p1` - The first LP problem.
/// * `p2` - The second LP problem.
/// * `line_map1` - Constraint name → 1-based line number for file 1.
/// * `line_map2` - Constraint name → 1-based line number for file 2.
pub fn build_diff_report(
    file1: &str,
    file2: &str,
    p1: &LpProblemOwned,
    p2: &LpProblemOwned,
    line_map1: &HashMap<String, usize>,
    line_map2: &HashMap<String, usize>,
) -> LpDiffReport {
    debug_assert!(!file1.is_empty(), "file1 label must not be empty");
    debug_assert!(!file2.is_empty(), "file2 label must not be empty");

    let variables = diff_variables(p1, p2);
    let constraints = diff_constraints(p1, p2, line_map1, line_map2);
    let objectives = diff_objectives(p1, p2);

    let sense_changed = if p1.sense == p2.sense { None } else { Some((p1.sense.clone(), p2.sense.clone())) };

    let name_changed = if p1.name == p2.name { None } else { Some((p1.name.clone(), p2.name.clone())) };

    LpDiffReport { file1: file1.to_string(), file2: file2.to_string(), sense_changed, name_changed, variables, constraints, objectives }
}

#[cfg(test)]
mod tests {
    use lp_parser_rs::model::{CoefficientOwned, ConstraintOwned, ObjectiveOwned, SOSType, Sense, VariableOwned, VariableType};
    use lp_parser_rs::problem::LpProblemOwned;

    use super::*;

    fn empty_problem() -> LpProblemOwned {
        LpProblemOwned::new()
    }

    fn problem_with_variable(name: &str, var_type: VariableType) -> LpProblemOwned {
        let mut p = LpProblemOwned::new();
        p.add_variable(VariableOwned::new(name).with_var_type(var_type));
        p
    }

    fn problem_with_standard_constraint(
        constraint_name: &str,
        coeffs: Vec<(&str, f64)>,
        operator: ComparisonOp,
        rhs: f64,
    ) -> LpProblemOwned {
        let mut p = LpProblemOwned::new();
        p.add_constraint(ConstraintOwned::Standard {
            name: constraint_name.to_string(),
            coefficients: coeffs.into_iter().map(|(n, v)| CoefficientOwned { name: n.to_string(), value: v }).collect(),
            operator,
            rhs,
            byte_offset: None,
        });
        p
    }

    fn make_objective(name: &str, coeffs: Vec<(&str, f64)>) -> ObjectiveOwned {
        ObjectiveOwned {
            name: name.to_string(),
            coefficients: coeffs.into_iter().map(|(n, v)| CoefficientOwned { name: n.to_string(), value: v }).collect(),
        }
    }

    #[test]
    fn test_empty_problems() {
        let p1 = empty_problem();
        let p2 = empty_problem();
        let report = build_diff_report("a.lp", "b.lp", &p1, &p2, &HashMap::new(), &HashMap::new());

        assert!(report.variables.entries.is_empty());
        assert!(report.constraints.entries.is_empty());
        assert!(report.objectives.entries.is_empty());
        assert_eq!(report.summary().total_changes(), 0);
        assert!(report.sense_changed.is_none());
        assert!(report.name_changed.is_none());
    }

    #[test]
    fn test_variable_added() {
        let p1 = empty_problem();
        let p2 = problem_with_variable("x", VariableType::Binary);
        let report = build_diff_report("a.lp", "b.lp", &p1, &p2, &HashMap::new(), &HashMap::new());

        assert_eq!(report.variables.entries.len(), 1);
        let entry = &report.variables.entries[0];
        assert_eq!(entry.name, "x");
        assert_eq!(entry.kind, DiffKind::Added);
        assert!(entry.old_type.is_none());
        assert_eq!(entry.new_type, Some(VariableType::Binary));
        assert_eq!(report.variables.counts.added, 1);
        assert_eq!(report.variables.counts.removed, 0);
    }

    #[test]
    fn test_variable_removed() {
        let p1 = problem_with_variable("y", VariableType::Integer);
        let p2 = empty_problem();
        let report = build_diff_report("a.lp", "b.lp", &p1, &p2, &HashMap::new(), &HashMap::new());

        assert_eq!(report.variables.entries.len(), 1);
        let entry = &report.variables.entries[0];
        assert_eq!(entry.kind, DiffKind::Removed);
        assert_eq!(entry.old_type, Some(VariableType::Integer));
        assert!(entry.new_type.is_none());
        assert_eq!(report.variables.counts.removed, 1);
    }

    #[test]
    fn test_variable_modified() {
        let p1 = problem_with_variable("z", VariableType::Free);
        let p2 = problem_with_variable("z", VariableType::Binary);
        let report = build_diff_report("a.lp", "b.lp", &p1, &p2, &HashMap::new(), &HashMap::new());

        assert_eq!(report.variables.entries.len(), 1);
        let entry = &report.variables.entries[0];
        assert_eq!(entry.kind, DiffKind::Modified);
        assert_eq!(entry.old_type, Some(VariableType::Free));
        assert_eq!(entry.new_type, Some(VariableType::Binary));
        assert_eq!(report.variables.counts.modified, 1);
        assert_eq!(report.variables.counts.unchanged, 0);
    }

    #[test]
    fn test_constraint_coeff_diff() {
        // p1: c1: 2x + 3y <= 10
        // p2: c1: 2x + 5z <= 10   (y removed, z added, x unchanged)
        let p1 = problem_with_standard_constraint("c1", vec![("x", 2.0), ("y", 3.0)], ComparisonOp::LTE, 10.0);
        let p2 = problem_with_standard_constraint("c1", vec![("x", 2.0), ("z", 5.0)], ComparisonOp::LTE, 10.0);
        let report = build_diff_report("a.lp", "b.lp", &p1, &p2, &HashMap::new(), &HashMap::new());

        assert_eq!(report.constraints.entries.len(), 1);
        let entry = &report.constraints.entries[0];
        assert_eq!(entry.kind, DiffKind::Modified);

        if let ConstraintDiffDetail::Standard { coeff_changes, operator_change, rhs_change, .. } = &entry.detail {
            // y removed, z added; x unchanged (not in coeff_changes).
            assert_eq!(coeff_changes.len(), 2);
            assert!(operator_change.is_none());
            assert!(rhs_change.is_none());

            let removed = coeff_changes.iter().find(|c| c.variable == "y").expect("y should be removed");
            assert_eq!(removed.kind, DiffKind::Removed);
            assert_eq!(removed.old_value, Some(3.0));
            assert!(removed.new_value.is_none());

            let added = coeff_changes.iter().find(|c| c.variable == "z").expect("z should be added");
            assert_eq!(added.kind, DiffKind::Added);
            assert!(added.old_value.is_none());
            assert_eq!(added.new_value, Some(5.0));
        } else {
            panic!("Expected Standard detail");
        }
    }

    #[test]
    fn test_constraint_type_changed() {
        let mut p1 = LpProblemOwned::new();
        p1.add_constraint(ConstraintOwned::Standard {
            name: "con1".to_string(),
            coefficients: vec![CoefficientOwned { name: "x".to_string(), value: 1.0 }],
            operator: ComparisonOp::EQ,
            rhs: 0.0,
            byte_offset: None,
        });

        let mut p2 = LpProblemOwned::new();
        p2.add_constraint(ConstraintOwned::SOS {
            name: "con1".to_string(),
            sos_type: SOSType::S1,
            weights: vec![CoefficientOwned { name: "x".to_string(), value: 1.0 }],
            byte_offset: None,
        });

        let report = build_diff_report("a.lp", "b.lp", &p1, &p2, &HashMap::new(), &HashMap::new());

        assert_eq!(report.constraints.entries.len(), 1);
        let entry = &report.constraints.entries[0];
        assert_eq!(entry.kind, DiffKind::Modified);
        assert!(matches!(entry.detail, ConstraintDiffDetail::TypeChanged { .. }));
    }

    #[test]
    fn test_objective_diff() {
        let mut p1 = LpProblemOwned::new();
        p1.add_objective(make_objective("obj1", vec![("a", 1.0), ("b", 2.0)]));

        let mut p2 = LpProblemOwned::new();
        // b changed from 2.0 to 5.0; a unchanged; c added.
        p2.add_objective(make_objective("obj1", vec![("a", 1.0), ("b", 5.0), ("c", 3.0)]));

        let report = build_diff_report("a.lp", "b.lp", &p1, &p2, &HashMap::new(), &HashMap::new());

        assert_eq!(report.objectives.entries.len(), 1);
        let entry = &report.objectives.entries[0];
        assert_eq!(entry.kind, DiffKind::Modified);
        // b modified, c added.
        assert_eq!(entry.coeff_changes.len(), 2);

        let b_change = entry.coeff_changes.iter().find(|c| c.variable == "b").expect("b should be modified");
        assert_eq!(b_change.kind, DiffKind::Modified);
        assert_eq!(b_change.old_value, Some(2.0));
        assert_eq!(b_change.new_value, Some(5.0));
    }

    #[test]
    fn test_sense_changed() {
        let p1 = LpProblemOwned::new().with_sense(Sense::Minimize);
        let p2 = LpProblemOwned::new().with_sense(Sense::Maximize);
        let report = build_diff_report("a.lp", "b.lp", &p1, &p2, &HashMap::new(), &HashMap::new());

        assert!(report.sense_changed.is_some());
        let (old, new) = report.sense_changed.unwrap();
        assert_eq!(old, Sense::Minimize);
        assert_eq!(new, Sense::Maximize);
    }

    #[test]
    fn test_unchanged_not_stored() {
        // Ten identical variables plus one that differs.
        let mut p1 = LpProblemOwned::new();
        let mut p2 = LpProblemOwned::new();

        for i in 0..10 {
            let name = format!("x{i}");
            p1.add_variable(VariableOwned::new(&name).with_var_type(VariableType::Free));
            p2.add_variable(VariableOwned::new(&name).with_var_type(VariableType::Free));
        }
        // One changed variable.
        p1.add_variable(VariableOwned::new("changed").with_var_type(VariableType::Free));
        p2.add_variable(VariableOwned::new("changed").with_var_type(VariableType::Binary));

        let report = build_diff_report("a.lp", "b.lp", &p1, &p2, &HashMap::new(), &HashMap::new());

        // Only the changed variable should be stored.
        assert_eq!(report.variables.entries.len(), 1);
        assert_eq!(report.variables.entries[0].name, "changed");
        assert_eq!(report.variables.counts.unchanged, 10);
        assert_eq!(report.variables.counts.modified, 1);
        assert_eq!(report.variables.counts.total(), 11);
    }

    #[test]
    fn test_diff_entry_trait() {
        // VariableDiffEntry — Added
        let added_var = VariableDiffEntry {
            name: "x".to_string(),
            kind: DiffKind::Added,
            old_type: None,
            new_type: Some(VariableType::Binary),
            searchable_text: "x".to_string(),
        };
        assert_eq!(added_var.summary_line(), "Binary");

        // VariableDiffEntry — Removed
        let removed_var = VariableDiffEntry {
            name: "y".to_string(),
            kind: DiffKind::Removed,
            old_type: Some(VariableType::Integer),
            new_type: None,
            searchable_text: "y".to_string(),
        };
        assert_eq!(removed_var.summary_line(), "Integer");

        // VariableDiffEntry — Modified
        let modified_var = VariableDiffEntry {
            name: "z".to_string(),
            kind: DiffKind::Modified,
            old_type: Some(VariableType::Free),
            new_type: Some(VariableType::General),
            searchable_text: "z".to_string(),
        };
        assert!(modified_var.summary_line().contains('\u{2192}'));

        // ConstraintDiffEntry — AddedOrRemoved (Standard)
        let added_constraint = ConstraintDiffEntry {
            name: "c1".to_string(),
            kind: DiffKind::Added,
            detail: ConstraintDiffDetail::AddedOrRemoved(ConstraintOwned::Standard {
                name: "c1".to_string(),
                coefficients: vec![
                    CoefficientOwned { name: "x".to_string(), value: 1.0 },
                    CoefficientOwned { name: "y".to_string(), value: 2.0 },
                ],
                operator: ComparisonOp::LTE,
                rhs: 5.0,
                byte_offset: None,
            }),
            searchable_text: "c1\0x\0y".to_string(),
            line_file1: None,
            line_file2: None,
        };
        assert_eq!(added_constraint.summary_line(), "Standard, 2 coefficient(s)");

        // ConstraintDiffEntry — Standard Modified
        let modified_constraint = ConstraintDiffEntry {
            name: "c2".to_string(),
            kind: DiffKind::Modified,
            detail: ConstraintDiffDetail::Standard {
                old_coefficients: vec![],
                new_coefficients: vec![],
                coeff_changes: vec![CoefficientChange {
                    variable: "a".to_string(),
                    kind: DiffKind::Modified,
                    old_value: Some(1.0),
                    new_value: Some(2.0),
                }],
                operator_change: Some((ComparisonOp::LTE, ComparisonOp::GTE)),
                rhs_change: Some((10.0, 20.0)),
                old_rhs: 10.0,
                new_rhs: 20.0,
            },
            searchable_text: "c2".to_string(),
            line_file1: None,
            line_file2: None,
        };
        let summary = modified_constraint.summary_line();
        assert!(summary.contains("1 coeff change(s)"), "summary was: {summary}");
        assert!(summary.contains("op"), "summary was: {summary}");
        assert!(summary.contains("rhs"), "summary was: {summary}");

        // ObjectiveDiffEntry — Added
        let added_obj = ObjectiveDiffEntry {
            name: "obj".to_string(),
            kind: DiffKind::Added,
            old_coefficients: vec![],
            new_coefficients: vec![
                CoefficientOwned { name: "a".to_string(), value: 1.0 },
                CoefficientOwned { name: "b".to_string(), value: 2.0 },
            ],
            coeff_changes: vec![],
            searchable_text: "obj\0a\0b".to_string(),
        };
        assert_eq!(added_obj.summary_line(), "2 coefficient(s)");

        // ObjectiveDiffEntry — Modified
        let modified_obj = ObjectiveDiffEntry {
            name: "obj2".to_string(),
            kind: DiffKind::Modified,
            old_coefficients: vec![],
            new_coefficients: vec![],
            coeff_changes: vec![
                CoefficientChange { variable: "a".to_string(), kind: DiffKind::Added, old_value: None, new_value: Some(3.0) },
                CoefficientChange { variable: "b".to_string(), kind: DiffKind::Removed, old_value: Some(1.0), new_value: None },
            ],
            searchable_text: "obj2\0a\0b".to_string(),
        };
        assert_eq!(modified_obj.summary_line(), "2 coeff change(s)");
    }

    #[test]
    fn test_searchable_text_variable() {
        let p1 = empty_problem();
        let p2 = problem_with_variable("flow_x", VariableType::Binary);
        let report = build_diff_report("a.lp", "b.lp", &p1, &p2, &HashMap::new(), &HashMap::new());

        let entry = &report.variables.entries[0];
        // Variables have no extra fields, so searchable_text is just the name.
        assert_eq!(entry.searchable_text, "flow_x");
    }

    #[test]
    fn test_searchable_text_constraint_with_coefficients() {
        let p1 = empty_problem();
        let p2 = problem_with_standard_constraint("c1", vec![("alpha", 1.0), ("beta", 2.0)], ComparisonOp::LTE, 10.0);
        let report = build_diff_report("a.lp", "b.lp", &p1, &p2, &HashMap::new(), &HashMap::new());

        let entry = &report.constraints.entries[0];
        // Should contain name + variable names separated by \0.
        assert!(entry.searchable_text.contains("c1"), "searchable_text: {}", entry.searchable_text);
        assert!(entry.searchable_text.contains("alpha"), "searchable_text: {}", entry.searchable_text);
        assert!(entry.searchable_text.contains("beta"), "searchable_text: {}", entry.searchable_text);
        assert!(entry.searchable_text.contains('\0'), "searchable_text should use \\0 separator");
    }

    #[test]
    fn test_searchable_text_objective_with_coefficients() {
        let mut p1 = LpProblemOwned::new();
        p1.add_objective(make_objective("obj1", vec![("x", 1.0), ("y", 2.0)]));
        let p2 = empty_problem();
        let report = build_diff_report("a.lp", "b.lp", &p1, &p2, &HashMap::new(), &HashMap::new());

        let entry = &report.objectives.entries[0];
        assert!(entry.searchable_text.contains("obj1"), "searchable_text: {}", entry.searchable_text);
        assert!(entry.searchable_text.contains("x"), "searchable_text: {}", entry.searchable_text);
        assert!(entry.searchable_text.contains("y"), "searchable_text: {}", entry.searchable_text);
    }

    #[test]
    fn test_searchable_text_modified_constraint_union_of_variables() {
        // p1: c1: 2x + 3y <= 10
        // p2: c1: 2x + 5z <= 10   (y removed, z added)
        let p1 = problem_with_standard_constraint("c1", vec![("x", 2.0), ("y", 3.0)], ComparisonOp::LTE, 10.0);
        let p2 = problem_with_standard_constraint("c1", vec![("x", 2.0), ("z", 5.0)], ComparisonOp::LTE, 10.0);
        let report = build_diff_report("a.lp", "b.lp", &p1, &p2, &HashMap::new(), &HashMap::new());

        let entry = &report.constraints.entries[0];
        // Should contain all variables from both versions (union).
        assert!(entry.searchable_text.contains("x"), "searchable_text: {}", entry.searchable_text);
        assert!(entry.searchable_text.contains("y"), "searchable_text: {}", entry.searchable_text);
        assert!(entry.searchable_text.contains("z"), "searchable_text: {}", entry.searchable_text);
    }

    #[test]
    fn test_line_index_basic() {
        let source = "line1\nline2\nline3\n";
        let idx = LineIndex::new(source);
        // "line1" starts at byte 0 → line 1
        assert_eq!(idx.line_number(0), Some(1));
        // "line2" starts at byte 6 → line 2
        assert_eq!(idx.line_number(6), Some(2));
        // "line3" starts at byte 12 → line 3
        assert_eq!(idx.line_number(12), Some(3));
        // Middle of "line1" → still line 1
        assert_eq!(idx.line_number(3), Some(1));
    }

    #[test]
    fn test_line_index_single_line() {
        let source = "no newlines";
        let idx = LineIndex::new(source);
        assert_eq!(idx.line_number(0), Some(1));
        assert_eq!(idx.line_number(5), Some(1));
    }

    #[test]
    fn test_line_numbers_in_constraint_diff() {
        // Build two problems with line maps that simulate real offsets.
        let p1 = problem_with_standard_constraint("c1", vec![("x", 1.0)], ComparisonOp::LTE, 10.0);
        let p2 = problem_with_standard_constraint("c1", vec![("x", 2.0)], ComparisonOp::LTE, 10.0);
        let mut lm1 = HashMap::new();
        lm1.insert("c1".to_string(), 5);
        let mut lm2 = HashMap::new();
        lm2.insert("c1".to_string(), 8);

        let report = build_diff_report("a.lp", "b.lp", &p1, &p2, &lm1, &lm2);
        let entry = &report.constraints.entries[0];
        assert_eq!(entry.line_file1, Some(5));
        assert_eq!(entry.line_file2, Some(8));
    }
}
