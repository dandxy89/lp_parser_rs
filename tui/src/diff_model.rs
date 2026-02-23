//! Rich diff data model and algorithm for comparing two LP problems.
//!
//! This module is pure data types and logic — no ratatui dependency.

use std::collections::HashMap;
use std::fmt;

use lp_parser_rs::analysis::ProblemAnalysis;
use lp_parser_rs::model::{ComparisonOp, Constraint, SOSType, Sense, VariableType};
use lp_parser_rs::problem::LpProblem;

// Epsilon used for floating-point coefficient value comparison.
const COEFF_EPSILON: f64 = 1e-10;

/// A coefficient with its variable name resolved from the interner to a `String`.
///
/// Used at the diff/display boundary so downstream code (detail panels, plain-text
/// rendering) can work with owned strings without needing the interner.
#[derive(Debug, Clone)]
pub struct ResolvedCoefficient {
    pub name: String,
    pub value: f64,
}

/// A constraint with all names resolved from the interner to owned `String`s.
///
/// Used in the `AddedOrRemoved` variant of [`ConstraintDiffDetail`] so the detail
/// panel can render a constraint that exists only in one of the two problems.
#[derive(Debug, Clone)]
pub enum ResolvedConstraint {
    Standard { coefficients: Vec<ResolvedCoefficient>, operator: ComparisonOp, rhs: f64 },
    Sos { sos_type: SOSType, weights: Vec<ResolvedCoefficient> },
}

/// Compare two coefficient slices across different interners without allocating.
///
/// Resolves each `NameId` inline via the respective problem's interner and compares
/// names and values pairwise. Returns `false` if lengths differ or any pair mismatches.
fn coefficients_equal(
    p1: &LpProblem,
    c1: &[lp_parser_rs::model::Coefficient],
    p2: &LpProblem,
    c2: &[lp_parser_rs::model::Coefficient],
) -> bool {
    debug_assert!(!c1.is_empty() || !c2.is_empty() || c1.len() == c2.len(), "both slices empty is trivially equal");
    if c1.len() != c2.len() {
        return false;
    }
    c1.iter().zip(c2.iter()).all(|(a, b)| p1.resolve(a.name) == p2.resolve(b.name) && (a.value - b.value).abs() <= COEFF_EPSILON)
}

/// Resolve a slice of model `Coefficient`s into `ResolvedCoefficient`s using the problem's interner.
fn resolve_coefficients(problem: &LpProblem, coefficients: &[lp_parser_rs::model::Coefficient]) -> Vec<ResolvedCoefficient> {
    coefficients.iter().map(|c| ResolvedCoefficient { name: problem.resolve(c.name).to_string(), value: c.value }).collect()
}

/// Resolve a model `Constraint` into a `ResolvedConstraint` using the problem's interner.
fn resolve_constraint(problem: &LpProblem, constraint: &Constraint) -> ResolvedConstraint {
    match constraint {
        Constraint::Standard { coefficients, operator, rhs, .. } => ResolvedConstraint::Standard {
            coefficients: resolve_coefficients(problem, coefficients),
            operator: operator.clone(),
            rhs: *rhs,
        },
        Constraint::SOS { sos_type, weights, .. } => {
            ResolvedConstraint::Sos { sos_type: sos_type.clone(), weights: resolve_coefficients(problem, weights) }
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
    /// Structural analysis of the first file.
    pub analysis1: ProblemAnalysis,
    /// Structural analysis of the second file.
    pub analysis2: ProblemAnalysis,
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

    /// Aggregate counts across all three sections into a single `DiffCounts`.
    #[must_use]
    pub const fn aggregate_counts(&self) -> DiffCounts {
        DiffCounts {
            added: self.variables.added + self.constraints.added + self.objectives.added,
            removed: self.variables.removed + self.constraints.removed + self.objectives.removed,
            modified: self.variables.modified + self.constraints.modified + self.objectives.modified,
            unchanged: self.variables.unchanged + self.constraints.unchanged + self.objectives.unchanged,
        }
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
}

/// Diff entry for a single constraint.
#[derive(Debug, Clone)]
pub struct ConstraintDiffEntry {
    pub name: String,
    pub kind: DiffKind,
    pub detail: ConstraintDiffDetail,
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
        old_coefficients: Vec<ResolvedCoefficient>,
        new_coefficients: Vec<ResolvedCoefficient>,
        coeff_changes: Vec<CoefficientChange>,
        operator_change: Option<(ComparisonOp, ComparisonOp)>,
        rhs_change: Option<(f64, f64)>,
        old_rhs: f64,
        new_rhs: f64,
    },
    /// Both versions are SOS constraints.
    Sos {
        old_weights: Vec<ResolvedCoefficient>,
        new_weights: Vec<ResolvedCoefficient>,
        weight_changes: Vec<CoefficientChange>,
        type_change: Option<(SOSType, SOSType)>,
    },
    /// Constraint changed from Standard to SOS or vice versa.
    TypeChanged { old_summary: String, new_summary: String },
    /// Constraint exists only in one of the two problems.
    AddedOrRemoved(ResolvedConstraint),
}

/// A change to a single coefficient (or weight) within a constraint or objective.
#[derive(Debug, Clone)]
pub struct CoefficientChange {
    pub variable: String,
    pub kind: DiffKind,
    /// Value in the first problem; `None` when the coefficient was added.
    pub old_value: Option<f64>,
    /// Value in the second problem; `None` when the coefficient was removed.
    pub new_value: Option<f64>,
}

/// Diff entry for a single objective.
#[derive(Debug, Clone)]
pub struct ObjectiveDiffEntry {
    pub name: String,
    pub kind: DiffKind,
    pub old_coefficients: Vec<ResolvedCoefficient>,
    pub new_coefficients: Vec<ResolvedCoefficient>,
    pub coeff_changes: Vec<CoefficientChange>,
}

/// Trait implemented by all diff entry types so the TUI can render them uniformly.
pub trait DiffEntry {
    fn name(&self) -> &str;
    fn kind(&self) -> DiffKind;
}

impl DiffEntry for VariableDiffEntry {
    fn name(&self) -> &str {
        &self.name
    }

    fn kind(&self) -> DiffKind {
        self.kind
    }
}

impl DiffEntry for ConstraintDiffEntry {
    fn name(&self) -> &str {
        &self.name
    }

    fn kind(&self) -> DiffKind {
        self.kind
    }
}

impl DiffEntry for ObjectiveDiffEntry {
    fn name(&self) -> &str {
        &self.name
    }

    fn kind(&self) -> DiffKind {
        self.kind
    }
}

/// Diff two coefficient lists using sorted merge-join and return only the changed entries.
///
/// Both slices are sorted by name before merging. Uses an epsilon of [`COEFF_EPSILON`]
/// for floating-point comparison.
fn diff_coefficients(old: &[ResolvedCoefficient], new: &[ResolvedCoefficient]) -> Vec<CoefficientChange> {
    let mut old_sorted: Vec<(&str, f64)> = old.iter().map(|c| (c.name.as_str(), c.value)).collect();
    let mut new_sorted: Vec<(&str, f64)> = new.iter().map(|c| (c.name.as_str(), c.value)).collect();
    old_sorted.sort_unstable_by(|a, b| a.0.cmp(b.0));
    new_sorted.sort_unstable_by(|a, b| a.0.cmp(b.0));

    let mut changes = Vec::new();
    let mut i = 0;
    let mut j = 0;

    while i < old_sorted.len() || j < new_sorted.len() {
        debug_assert!(i <= old_sorted.len(), "old index out of bounds");
        debug_assert!(j <= new_sorted.len(), "new index out of bounds");

        let cmp = match (old_sorted.get(i), new_sorted.get(j)) {
            (Some((n1, _)), Some((n2, _))) => n1.cmp(n2),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => break,
        };

        match cmp {
            std::cmp::Ordering::Less => {
                let (name, old_val) = old_sorted[i];
                changes.push(CoefficientChange {
                    variable: name.to_string(),
                    kind: DiffKind::Removed,
                    old_value: Some(old_val),
                    new_value: None,
                });
                i += 1;
            }
            std::cmp::Ordering::Greater => {
                let (name, new_val) = new_sorted[j];
                changes.push(CoefficientChange {
                    variable: name.to_string(),
                    kind: DiffKind::Added,
                    old_value: None,
                    new_value: Some(new_val),
                });
                j += 1;
            }
            std::cmp::Ordering::Equal => {
                let (name, old_val) = old_sorted[i];
                let new_val = new_sorted[j].1;
                if (old_val - new_val).abs() > COEFF_EPSILON {
                    changes.push(CoefficientChange {
                        variable: name.to_string(),
                        kind: DiffKind::Modified,
                        old_value: Some(old_val),
                        new_value: Some(new_val),
                    });
                }
                // Unchanged coefficients are skipped.
                i += 1;
                j += 1;
            }
        }
    }

    changes
}

/// Build a short human-readable summary string for a constraint (used in `TypeChanged`).
fn constraint_summary(problem: &LpProblem, constraint: &Constraint) -> String {
    match constraint {
        Constraint::Standard { coefficients, operator, rhs, .. } => {
            format!("Standard({} coeffs, {operator}, {rhs})", coefficients.len())
        }
        Constraint::SOS { sos_type, weights, .. } => {
            let _ = problem; // used for consistency; SOS summary doesn't need name resolution
            format!("SOS({sos_type}, {} weights)", weights.len())
        }
    }
}

/// Build a sorted vec of (name, &Variable) pairs from the interner-keyed variables.
fn build_sorted_vars(problem: &LpProblem) -> Vec<(&str, &lp_parser_rs::model::Variable)> {
    let mut pairs: Vec<_> = problem.variables.iter().map(|(id, var)| (problem.resolve(*id), var)).collect();
    pairs.sort_unstable_by(|a, b| a.0.cmp(b.0));
    pairs
}

/// Build a sorted vec of (name, &Constraint) pairs from the interner-keyed constraints.
fn build_sorted_constraints(problem: &LpProblem) -> Vec<(&str, &Constraint)> {
    let mut pairs: Vec<_> = problem.constraints.iter().map(|(id, c)| (problem.resolve(*id), c)).collect();
    pairs.sort_unstable_by(|a, b| a.0.cmp(b.0));
    pairs
}

/// Build a sorted vec of (name, &Objective) pairs from the interner-keyed objectives.
fn build_sorted_objectives(problem: &LpProblem) -> Vec<(&str, &lp_parser_rs::model::Objective)> {
    let mut pairs: Vec<_> = problem.objectives.iter().map(|(id, o)| (problem.resolve(*id), o)).collect();
    pairs.sort_unstable_by(|a, b| a.0.cmp(b.0));
    pairs
}

/// Diff the variables section between two problems using sorted merge-join.
fn diff_variables(p1: &LpProblem, p2: &LpProblem) -> SectionDiff<VariableDiffEntry> {
    let vars1 = build_sorted_vars(p1);
    let vars2 = build_sorted_vars(p2);

    let mut entries = Vec::new();
    let mut counts = DiffCounts::default();
    let mut i = 0;
    let mut j = 0;

    while i < vars1.len() || j < vars2.len() {
        debug_assert!(i <= vars1.len(), "vars1 index out of bounds");
        debug_assert!(j <= vars2.len(), "vars2 index out of bounds");

        let cmp = match (vars1.get(i), vars2.get(j)) {
            (Some((n1, _)), Some((n2, _))) => n1.cmp(n2),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => break,
        };

        match cmp {
            std::cmp::Ordering::Less => {
                let (name, v1) = &vars1[i];
                counts.removed += 1;
                let entry = VariableDiffEntry {
                    name: name.to_string(),
                    kind: DiffKind::Removed,
                    old_type: Some(v1.var_type.clone()),
                    new_type: None,
                };
                debug_assert!(entry.old_type.is_some(), "Removed variable must have old_type");
                debug_assert!(entry.new_type.is_none(), "Removed variable must not have new_type");
                entries.push(entry);
                i += 1;
            }
            std::cmp::Ordering::Greater => {
                let (name, v2) = &vars2[j];
                counts.added += 1;
                let entry = VariableDiffEntry {
                    name: name.to_string(),
                    kind: DiffKind::Added,
                    old_type: None,
                    new_type: Some(v2.var_type.clone()),
                };
                debug_assert!(entry.new_type.is_some(), "Added variable must have new_type");
                debug_assert!(entry.old_type.is_none(), "Added variable must not have old_type");
                entries.push(entry);
                j += 1;
            }
            std::cmp::Ordering::Equal => {
                let (name, v1) = &vars1[i];
                let v2 = vars2[j].1;
                if v1.var_type == v2.var_type {
                    counts.unchanged += 1;
                } else {
                    counts.modified += 1;
                    let entry = VariableDiffEntry {
                        name: name.to_string(),
                        kind: DiffKind::Modified,
                        old_type: Some(v1.var_type.clone()),
                        new_type: Some(v2.var_type.clone()),
                    };
                    debug_assert!(entry.old_type.is_some(), "Modified variable must have old_type");
                    debug_assert!(entry.new_type.is_some(), "Modified variable must have new_type");
                    entries.push(entry);
                }
                i += 1;
                j += 1;
            }
        }
    }

    // Entries are already sorted by name from the merge-join.
    SectionDiff { entries, counts }
}

/// Diff two standard constraints and return the detail if they differ, `None` if unchanged.
fn diff_standard_constraints(
    old_coefficients: &[ResolvedCoefficient],
    old_operator: &ComparisonOp,
    old_rhs: f64,
    new_coefficients: &[ResolvedCoefficient],
    new_operator: &ComparisonOp,
    new_rhs: f64,
) -> Option<ConstraintDiffDetail> {
    let coeff_changes = diff_coefficients(old_coefficients, new_coefficients);
    let operator_change = if old_operator == new_operator { None } else { Some((old_operator.clone(), new_operator.clone())) };
    let rhs_change = if (old_rhs - new_rhs).abs() > COEFF_EPSILON { Some((old_rhs, new_rhs)) } else { None };

    if coeff_changes.is_empty() && operator_change.is_none() && rhs_change.is_none() {
        return None;
    }

    Some(ConstraintDiffDetail::Standard {
        old_coefficients: old_coefficients.to_vec(),
        new_coefficients: new_coefficients.to_vec(),
        coeff_changes,
        operator_change,
        rhs_change,
        old_rhs,
        new_rhs,
    })
}

/// Diff two SOS constraints and return the detail if they differ, `None` if unchanged.
fn diff_sos_constraints(
    old_type: &SOSType,
    old_weights: &[ResolvedCoefficient],
    new_type: &SOSType,
    new_weights: &[ResolvedCoefficient],
) -> Option<ConstraintDiffDetail> {
    let weight_changes = diff_coefficients(old_weights, new_weights);
    let type_change = if old_type == new_type { None } else { Some((old_type.clone(), new_type.clone())) };

    if weight_changes.is_empty() && type_change.is_none() {
        return None;
    }

    Some(ConstraintDiffDetail::Sos { old_weights: old_weights.to_vec(), new_weights: new_weights.to_vec(), weight_changes, type_change })
}

/// Compute the detail for two constraints that both exist, returning `None` if unchanged.
fn diff_constraint_pair(p1: &LpProblem, c1: &Constraint, p2: &LpProblem, c2: &Constraint) -> Option<ConstraintDiffDetail> {
    match (c1, c2) {
        // Standard vs SOS: structurally incompatible.
        (Constraint::Standard { .. }, Constraint::SOS { .. }) | (Constraint::SOS { .. }, Constraint::Standard { .. }) => {
            Some(ConstraintDiffDetail::TypeChanged { old_summary: constraint_summary(p1, c1), new_summary: constraint_summary(p2, c2) })
        }

        // Both standard: diff coefficients, operator, rhs.
        (
            Constraint::Standard { coefficients: old_coefficients, operator: old_operator, rhs: old_rhs, .. },
            Constraint::Standard { coefficients: new_coefficients, operator: new_operator, rhs: new_rhs, .. },
        ) => {
            // Fast path: skip resolution if the raw data is identical.
            if old_operator == new_operator
                && (old_rhs - new_rhs).abs() <= COEFF_EPSILON
                && coefficients_equal(p1, old_coefficients, p2, new_coefficients)
            {
                return None;
            }
            let old_resolved = resolve_coefficients(p1, old_coefficients);
            let new_resolved = resolve_coefficients(p2, new_coefficients);
            diff_standard_constraints(&old_resolved, old_operator, *old_rhs, &new_resolved, new_operator, *new_rhs)
        }

        // Both SOS: diff weights and sos_type.
        (
            Constraint::SOS { sos_type: old_type, weights: old_weights, .. },
            Constraint::SOS { sos_type: new_type, weights: new_weights, .. },
        ) => {
            // Fast path: skip resolution if the raw data is identical.
            if old_type == new_type && coefficients_equal(p1, old_weights, p2, new_weights) {
                return None;
            }
            let old_resolved = resolve_coefficients(p1, old_weights);
            let new_resolved = resolve_coefficients(p2, new_weights);
            diff_sos_constraints(old_type, &old_resolved, new_type, &new_resolved)
        }
    }
}

/// Diff the constraints section between two problems using sorted merge-join.
fn diff_constraints(
    p1: &LpProblem,
    p2: &LpProblem,
    line_map1: &HashMap<String, usize>,
    line_map2: &HashMap<String, usize>,
) -> SectionDiff<ConstraintDiffEntry> {
    let cons1 = build_sorted_constraints(p1);
    let cons2 = build_sorted_constraints(p2);

    let mut entries = Vec::new();
    let mut counts = DiffCounts::default();
    let mut i = 0;
    let mut j = 0;

    while i < cons1.len() || j < cons2.len() {
        debug_assert!(i <= cons1.len(), "cons1 index out of bounds");
        debug_assert!(j <= cons2.len(), "cons2 index out of bounds");

        let cmp = match (cons1.get(i), cons2.get(j)) {
            (Some((n1, _)), Some((n2, _))) => n1.cmp(n2),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => break,
        };

        match cmp {
            std::cmp::Ordering::Less => {
                let (name, constraint) = &cons1[i];
                let line1 = line_map1.get(*name).copied();
                let line2 = line_map2.get(*name).copied();
                counts.removed += 1;
                entries.push(ConstraintDiffEntry {
                    name: name.to_string(),
                    kind: DiffKind::Removed,
                    detail: ConstraintDiffDetail::AddedOrRemoved(resolve_constraint(p1, constraint)),
                    line_file1: line1,
                    line_file2: line2,
                });
                i += 1;
            }
            std::cmp::Ordering::Greater => {
                let (name, constraint) = &cons2[j];
                let line1 = line_map1.get(*name).copied();
                let line2 = line_map2.get(*name).copied();
                counts.added += 1;
                entries.push(ConstraintDiffEntry {
                    name: name.to_string(),
                    kind: DiffKind::Added,
                    detail: ConstraintDiffDetail::AddedOrRemoved(resolve_constraint(p2, constraint)),
                    line_file1: line1,
                    line_file2: line2,
                });
                j += 1;
            }
            std::cmp::Ordering::Equal => {
                let (name, c1) = &cons1[i];
                let c2 = cons2[j].1;
                let line1 = line_map1.get(*name).copied();
                let line2 = line_map2.get(*name).copied();
                if let Some(detail) = diff_constraint_pair(p1, c1, p2, c2) {
                    counts.modified += 1;
                    entries.push(ConstraintDiffEntry {
                        name: name.to_string(),
                        kind: DiffKind::Modified,
                        detail,
                        line_file1: line1,
                        line_file2: line2,
                    });
                } else {
                    counts.unchanged += 1;
                }
                i += 1;
                j += 1;
            }
        }
    }

    SectionDiff { entries, counts }
}

/// Diff the objectives section between two problems using sorted merge-join.
fn diff_objectives(p1: &LpProblem, p2: &LpProblem) -> SectionDiff<ObjectiveDiffEntry> {
    let objs1 = build_sorted_objectives(p1);
    let objs2 = build_sorted_objectives(p2);

    let mut entries = Vec::new();
    let mut counts = DiffCounts::default();
    let mut i = 0;
    let mut j = 0;

    while i < objs1.len() || j < objs2.len() {
        debug_assert!(i <= objs1.len(), "objs1 index out of bounds");
        debug_assert!(j <= objs2.len(), "objs2 index out of bounds");

        let cmp = match (objs1.get(i), objs2.get(j)) {
            (Some((n1, _)), Some((n2, _))) => n1.cmp(n2),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => break,
        };

        match cmp {
            std::cmp::Ordering::Less => {
                let (name, o) = &objs1[i];
                counts.removed += 1;
                entries.push(ObjectiveDiffEntry {
                    name: name.to_string(),
                    kind: DiffKind::Removed,
                    old_coefficients: resolve_coefficients(p1, &o.coefficients),
                    new_coefficients: Vec::new(),
                    coeff_changes: Vec::new(),
                });
                i += 1;
            }
            std::cmp::Ordering::Greater => {
                let (name, o) = &objs2[j];
                counts.added += 1;
                entries.push(ObjectiveDiffEntry {
                    name: name.to_string(),
                    kind: DiffKind::Added,
                    old_coefficients: Vec::new(),
                    new_coefficients: resolve_coefficients(p2, &o.coefficients),
                    coeff_changes: Vec::new(),
                });
                j += 1;
            }
            std::cmp::Ordering::Equal => {
                let (name, o1_val) = &objs1[i];
                let o2_val = objs2[j].1;
                // Fast path: skip resolution if the raw coefficients are identical.
                if coefficients_equal(p1, &o1_val.coefficients, p2, &o2_val.coefficients) {
                    counts.unchanged += 1;
                    i += 1;
                    j += 1;
                    continue;
                }
                let old_resolved = resolve_coefficients(p1, &o1_val.coefficients);
                let new_resolved = resolve_coefficients(p2, &o2_val.coefficients);
                let coeff_changes = diff_coefficients(&old_resolved, &new_resolved);
                if coeff_changes.is_empty() {
                    counts.unchanged += 1;
                } else {
                    counts.modified += 1;
                    entries.push(ObjectiveDiffEntry {
                        name: name.to_string(),
                        kind: DiffKind::Modified,
                        old_coefficients: old_resolved,
                        new_coefficients: new_resolved,
                        coeff_changes,
                    });
                }
                i += 1;
                j += 1;
            }
        }
    }

    SectionDiff { entries, counts }
}

/// All inputs needed to build a diff report between two LP problem files.
pub struct DiffInput<'a> {
    /// Label or path for the first file.
    pub file1: &'a str,
    /// Label or path for the second file.
    pub file2: &'a str,
    /// The first LP problem.
    pub p1: &'a LpProblem,
    /// The second LP problem.
    pub p2: &'a LpProblem,
    /// Constraint name → 1-based line number for file 1.
    pub line_map1: &'a HashMap<String, usize>,
    /// Constraint name → 1-based line number for file 2.
    pub line_map2: &'a HashMap<String, usize>,
    /// Structural analysis of the first file.
    pub analysis1: ProblemAnalysis,
    /// Structural analysis of the second file.
    pub analysis2: ProblemAnalysis,
}

/// Build a complete diff report comparing two LP problems.
pub fn build_diff_report(input: &DiffInput<'_>) -> LpDiffReport {
    debug_assert!(!input.file1.is_empty(), "file1 label must not be empty");
    debug_assert!(!input.file2.is_empty(), "file2 label must not be empty");

    let variables = diff_variables(input.p1, input.p2);
    let constraints = diff_constraints(input.p1, input.p2, input.line_map1, input.line_map2);
    let objectives = diff_objectives(input.p1, input.p2);

    let sense_changed = if input.p1.sense == input.p2.sense { None } else { Some((input.p1.sense.clone(), input.p2.sense.clone())) };

    let name_changed = if input.p1.name == input.p2.name { None } else { Some((input.p1.name.clone(), input.p2.name.clone())) };

    LpDiffReport {
        file1: input.file1.to_string(),
        file2: input.file2.to_string(),
        sense_changed,
        name_changed,
        variables,
        constraints,
        objectives,
        analysis1: input.analysis1.clone(),
        analysis2: input.analysis2.clone(),
    }
}

#[cfg(test)]
mod tests {
    use lp_parser_rs::analysis::ProblemAnalysis;
    use lp_parser_rs::model::{ComparisonOp, Sense, VariableType};
    use lp_parser_rs::problem::LpProblem;

    use super::*;

    /// Create a dummy `ProblemAnalysis` for tests that don't care about analysis content.
    fn dummy_analysis() -> ProblemAnalysis {
        let content = "Minimize\n obj: x\nSubject To\n c1: x >= 0\nEnd\n";
        let problem = LpProblem::parse(content).expect("dummy LP should parse");
        problem.analyze()
    }

    fn empty_problem() -> LpProblem {
        LpProblem::parse("minimize\n_dummy\nsubject to\nend").expect("minimal LP should parse")
    }

    fn problem_with_variable(name: &str, var_type: &VariableType) -> LpProblem {
        let bounds_section = match var_type {
            VariableType::Binary => format!("binary\n {name}\n"),
            VariableType::Integer => format!("general\n {name}\n"),
            VariableType::Free => format!("bounds\n {name} free\n"),
            VariableType::LowerBound(lb) => format!("bounds\n {lb} <= {name}\n"),
            VariableType::UpperBound(ub) => format!("bounds\n {name} <= {ub}\n"),
            VariableType::DoubleBound(lb, ub) => format!("bounds\n {lb} <= {name} <= {ub}\n"),
            VariableType::SemiContinuous | VariableType::SOS | VariableType::General => {
                format!("bounds\n {name} free\n")
            }
        };
        let content = format!("minimize\nobj: {name}\nsubject to\n{bounds_section}end");
        LpProblem::parse(&content).expect("variable problem should parse")
    }

    fn problem_with_standard_constraint(constraint_name: &str, coeffs: &[(&str, f64)], operator: &ComparisonOp, rhs: f64) -> LpProblem {
        let op_str = match *operator {
            ComparisonOp::LTE => "<=",
            ComparisonOp::GTE => ">=",
            ComparisonOp::EQ => "=",
            ComparisonOp::LT => "<",
            ComparisonOp::GT => ">",
        };
        let terms: Vec<String> = coeffs.iter().map(|(n, v)| format!("{v} {n}")).collect();
        let lhs = terms.join(" + ");
        let content = format!("minimize\nobj: _dummy\nsubject to\n {constraint_name}: {lhs} {op_str} {rhs}\nend");
        LpProblem::parse(&content).expect("constraint problem should parse")
    }

    fn problem_with_objective(name: &str, coeffs: &[(&str, f64)]) -> LpProblem {
        let terms: Vec<String> = coeffs.iter().map(|(n, v)| format!("{v} {n}")).collect();
        let lhs = terms.join(" + ");
        let content = format!("minimize\n{name}: {lhs}\nsubject to\nend");
        LpProblem::parse(&content).expect("objective problem should parse")
    }

    /// Shorthand for building a `DiffInput` and calling `build_diff_report` in tests.
    #[allow(clippy::too_many_arguments)]
    fn test_diff_report(
        file1: &str,
        file2: &str,
        p1: &LpProblem,
        p2: &LpProblem,
        line_map1: &HashMap<String, usize>,
        line_map2: &HashMap<String, usize>,
        analysis1: ProblemAnalysis,
        analysis2: ProblemAnalysis,
    ) -> LpDiffReport {
        build_diff_report(&DiffInput { file1, file2, p1, p2, line_map1, line_map2, analysis1, analysis2 })
    }

    /// Build a diff report from two problems with no line maps and dummy analyses.
    fn quick_report(p1: &LpProblem, p2: &LpProblem) -> LpDiffReport {
        test_diff_report("a.lp", "b.lp", p1, p2, &HashMap::new(), &HashMap::new(), dummy_analysis(), dummy_analysis())
    }

    fn assert_diff_entry(entry: &impl DiffEntry, name: &str, kind: DiffKind) {
        assert_eq!(entry.name(), name);
        assert_eq!(entry.kind(), kind);
    }

    #[test]
    fn test_empty_problems() {
        let p1 = empty_problem();
        let p2 = empty_problem();
        let report = quick_report(&p1, &p2);

        // Both empty problems have the same "obj" objective, so no objective diffs.
        assert!(report.variables.entries.is_empty());
        assert!(report.constraints.entries.is_empty());
        assert!(report.objectives.entries.is_empty());
        assert!(report.sense_changed.is_none());
        assert!(report.name_changed.is_none());
    }

    #[test]
    fn test_variable_added() {
        let p1 = empty_problem();
        let p2 = problem_with_variable("x", &VariableType::Binary);
        let report = quick_report(&p1, &p2);
        // Find the "x" variable entry (there may be an "obj" objective-related variable too).
        let entry = report.variables.entries.iter().find(|e| e.name == "x");
        assert!(entry.is_some(), "should have an entry for variable 'x'");
        let entry = entry.unwrap();
        assert_eq!(entry.kind, DiffKind::Added);
        assert!(entry.old_type.is_none());
        assert_eq!(entry.new_type, Some(VariableType::Binary));
    }

    #[test]
    fn test_variable_removed() {
        let p1 = problem_with_variable("y", &VariableType::Integer);
        let p2 = empty_problem();
        let report = quick_report(&p1, &p2);
        let entry = report.variables.entries.iter().find(|e| e.name == "y");
        assert!(entry.is_some(), "should have an entry for variable 'y'");
        let entry = entry.unwrap();
        assert_eq!(entry.kind, DiffKind::Removed);
        // The General section in LP format sets VariableType::General
        assert_eq!(entry.old_type, Some(VariableType::General));
        assert!(entry.new_type.is_none());
    }

    #[test]
    fn test_variable_modified() {
        let p1 = problem_with_variable("z", &VariableType::Free);
        let p2 = problem_with_variable("z", &VariableType::Binary);
        let report = quick_report(&p1, &p2);
        let entry = report.variables.entries.iter().find(|e| e.name == "z");
        assert!(entry.is_some(), "should have an entry for variable 'z'");
        let entry = entry.unwrap();
        assert_eq!(entry.kind, DiffKind::Modified);
        assert_eq!(entry.old_type, Some(VariableType::Free));
        assert_eq!(entry.new_type, Some(VariableType::Binary));
    }

    #[test]
    fn test_constraint_coeff_diff() {
        // p1: c1: 2x + 3y <= 10
        // p2: c1: 2x + 5z <= 10   (y removed, z added, x unchanged)
        let p1 = problem_with_standard_constraint("c1", &[("x", 2.0), ("y", 3.0)], &ComparisonOp::LTE, 10.0);
        let p2 = problem_with_standard_constraint("c1", &[("x", 2.0), ("z", 5.0)], &ComparisonOp::LTE, 10.0);
        let report = quick_report(&p1, &p2);

        let entry = report.constraints.entries.iter().find(|e| e.name == "c1");
        assert!(entry.is_some(), "should have a constraint entry for 'c1'");
        let entry = entry.unwrap();
        assert_eq!(entry.kind, DiffKind::Modified);

        if let ConstraintDiffDetail::Standard { coeff_changes, operator_change, rhs_change, .. } = &entry.detail {
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
        // p1: standard constraint "con1: x = 0"
        let p1_content = "minimize\nobj: x\nsubject to\n con1: x = 0\nend";
        let p1 = LpProblem::parse(p1_content).expect("p1 should parse");

        // p2: SOS constraint "con1: S1:: x:1"
        let p2_content = "minimize\nobj: x\nsubject to\nsos\n con1: S1:: x:1\nend";
        let p2 = LpProblem::parse(p2_content).expect("p2 should parse");

        let report = quick_report(&p1, &p2);

        let entry = report.constraints.entries.iter().find(|e| e.name == "con1");
        assert!(entry.is_some(), "should have a constraint entry for 'con1'");
        let entry = entry.unwrap();
        assert_eq!(entry.kind, DiffKind::Modified);
        assert!(matches!(entry.detail, ConstraintDiffDetail::TypeChanged { .. }));
    }

    #[test]
    fn test_objective_diff() {
        let p1 = problem_with_objective("obj1", &[("a", 1.0), ("b", 2.0)]);
        let p2 = problem_with_objective("obj1", &[("a", 1.0), ("b", 5.0), ("c", 3.0)]);

        let report = quick_report(&p1, &p2);

        let entry = report.objectives.entries.iter().find(|e| e.name == "obj1");
        assert!(entry.is_some(), "should have an objective entry for 'obj1'");
        let entry = entry.unwrap();
        assert_eq!(entry.kind, DiffKind::Modified);
        assert_eq!(entry.coeff_changes.len(), 2);

        let b_change = entry.coeff_changes.iter().find(|c| c.variable == "b").expect("b should be modified");
        assert_eq!(b_change.kind, DiffKind::Modified);
        assert_eq!(b_change.old_value, Some(2.0));
        assert_eq!(b_change.new_value, Some(5.0));
    }

    #[test]
    fn test_sense_changed() {
        // Minimise vs Maximise
        let p1 = LpProblem::parse("minimize\nx\nsubject to\nend").expect("p1 should parse");
        let p2 = LpProblem::parse("maximize\nx\nsubject to\nend").expect("p2 should parse");
        let report = quick_report(&p1, &p2);

        assert!(report.sense_changed.is_some());
        let (old, new) = report.sense_changed.unwrap();
        assert_eq!(old, Sense::Minimize);
        assert_eq!(new, Sense::Maximize);
    }

    #[test]
    fn test_unchanged_not_stored() {
        // Build two problems with 10 identical Free variables and 1 modified variable.
        use std::fmt::Write;
        let mut var_lines_1 = String::new();
        let mut var_lines_2 = String::new();
        for i in 0..10 {
            writeln!(var_lines_1, " x{i} free").unwrap();
            writeln!(var_lines_2, " x{i} free").unwrap();
        }
        var_lines_1.push_str(" changed free\n");
        let content1 = format!("minimize\nobj: x0\nsubject to\nbounds\n{var_lines_1}end");
        let content2 = format!("minimize\nobj: x0\nsubject to\nbounds\n{var_lines_2}binary\n changed\nend");

        let p1 = LpProblem::parse(&content1).expect("p1 should parse");
        let p2 = LpProblem::parse(&content2).expect("p2 should parse");

        let report = quick_report(&p1, &p2);

        // Only the "changed" variable should appear in entries.
        let changed_entries: Vec<_> = report.variables.entries.iter().filter(|e| e.name == "changed").collect();
        assert_eq!(changed_entries.len(), 1);
        assert_eq!(changed_entries[0].name, "changed");
        assert_eq!(report.variables.counts.modified, 1);
    }

    #[test]
    fn test_diff_entry_trait() {
        let added_var =
            VariableDiffEntry { name: "x".to_string(), kind: DiffKind::Added, old_type: None, new_type: Some(VariableType::Binary) };
        assert_diff_entry(&added_var, "x", DiffKind::Added);

        let removed_var =
            VariableDiffEntry { name: "y".to_string(), kind: DiffKind::Removed, old_type: Some(VariableType::Integer), new_type: None };
        assert_diff_entry(&removed_var, "y", DiffKind::Removed);

        let added_constraint = ConstraintDiffEntry {
            name: "c1".to_string(),
            kind: DiffKind::Added,
            detail: ConstraintDiffDetail::AddedOrRemoved(ResolvedConstraint::Standard {
                coefficients: vec![
                    ResolvedCoefficient { name: "x".to_string(), value: 1.0 },
                    ResolvedCoefficient { name: "y".to_string(), value: 2.0 },
                ],
                operator: ComparisonOp::LTE,
                rhs: 5.0,
            }),
            line_file1: None,
            line_file2: None,
        };
        assert_diff_entry(&added_constraint, "c1", DiffKind::Added);

        let added_obj = ObjectiveDiffEntry {
            name: "obj".to_string(),
            kind: DiffKind::Added,
            old_coefficients: vec![],
            new_coefficients: vec![
                ResolvedCoefficient { name: "a".to_string(), value: 1.0 },
                ResolvedCoefficient { name: "b".to_string(), value: 2.0 },
            ],
            coeff_changes: vec![],
        };
        assert_diff_entry(&added_obj, "obj", DiffKind::Added);
    }

    #[test]
    fn test_line_numbers_in_constraint_diff() {
        let p1 = problem_with_standard_constraint("c1", &[("x", 1.0)], &ComparisonOp::LTE, 10.0);
        let p2 = problem_with_standard_constraint("c1", &[("x", 2.0)], &ComparisonOp::LTE, 10.0);
        let mut lm1 = HashMap::new();
        lm1.insert("c1".to_string(), 5);
        let mut lm2 = HashMap::new();
        lm2.insert("c1".to_string(), 8);

        let report = test_diff_report("a.lp", "b.lp", &p1, &p2, &lm1, &lm2, dummy_analysis(), dummy_analysis());
        let entry = report.constraints.entries.iter().find(|e| e.name == "c1").expect("should have c1 entry");
        assert_eq!(entry.line_file1, Some(5));
        assert_eq!(entry.line_file2, Some(8));
    }
}
