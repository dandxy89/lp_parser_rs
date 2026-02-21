//! Rich diff data model and algorithm for comparing two LP problems.
//!
//! This module is pure data types and logic — no ratatui dependency.

use std::collections::HashMap;
use std::fmt;

use lp_parser_rs::analysis::ProblemAnalysis;
use lp_parser_rs::model::{CoefficientOwned, ComparisonOp, ConstraintOwned, SOSType, Sense, VariableType};
use lp_parser_rs::problem::LpProblemOwned;

// Epsilon used for floating-point coefficient value comparison.
const COEFF_EPSILON: f64 = 1e-10;

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
    pub old_value: Option<f64>,
    /// Value in the second problem; `None` when the coefficient was removed.
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
                let entry = VariableDiffEntry {
                    name: name.to_string(),
                    kind: DiffKind::Removed,
                    old_type: Some(v1.var_type.clone()),
                    new_type: None,
                };
                debug_assert!(entry.old_type.is_some(), "Removed variable must have old_type");
                debug_assert!(entry.new_type.is_none(), "Removed variable must not have new_type");
                entries.push(entry);
            }
            (None, Some(v2)) => {
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
            }
            (Some(v1), Some(v2)) => {
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
            }
            (None, None) => {
                unreachable!("name in union but absent from both problems");
            }
        }
    }

    // Entries are already sorted by name because we sorted all_names above.
    SectionDiff { entries, counts }
}

/// Diff two standard constraints and return the detail if they differ, `None` if unchanged.
fn diff_standard_constraints(
    old_coefficients: &[CoefficientOwned],
    old_operator: &ComparisonOp,
    old_rhs: f64,
    new_coefficients: &[CoefficientOwned],
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
    old_weights: &[CoefficientOwned],
    new_type: &SOSType,
    new_weights: &[CoefficientOwned],
) -> Option<ConstraintDiffDetail> {
    let weight_changes = diff_coefficients(old_weights, new_weights);
    let type_change = if old_type == new_type { None } else { Some((old_type.clone(), new_type.clone())) };

    if weight_changes.is_empty() && type_change.is_none() {
        return None;
    }

    Some(ConstraintDiffDetail::Sos { old_weights: old_weights.to_vec(), new_weights: new_weights.to_vec(), weight_changes, type_change })
}

/// Compute the detail for two constraints that both exist, returning `None` if unchanged.
fn diff_constraint_pair(constraint1: &ConstraintOwned, constraint2: &ConstraintOwned) -> Option<ConstraintDiffDetail> {
    match (constraint1, constraint2) {
        // Standard vs SOS: structurally incompatible.
        (ConstraintOwned::Standard { .. }, ConstraintOwned::SOS { .. })
        | (ConstraintOwned::SOS { .. }, ConstraintOwned::Standard { .. }) => Some(ConstraintDiffDetail::TypeChanged {
            old_summary: constraint_summary(constraint1),
            new_summary: constraint_summary(constraint2),
        }),

        // Both standard: diff coefficients, operator, rhs.
        (
            ConstraintOwned::Standard { coefficients: old_coefficients, operator: old_operator, rhs: old_rhs, .. },
            ConstraintOwned::Standard { coefficients: new_coefficients, operator: new_operator, rhs: new_rhs, .. },
        ) => diff_standard_constraints(old_coefficients, old_operator, *old_rhs, new_coefficients, new_operator, *new_rhs),

        // Both SOS: diff weights and sos_type.
        (
            ConstraintOwned::SOS { sos_type: old_type, weights: old_weights, .. },
            ConstraintOwned::SOS { sos_type: new_type, weights: new_weights, .. },
        ) => diff_sos_constraints(old_type, old_weights, new_type, new_weights),
    }
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
        let constraint1 = p1.constraints.get(name);
        let constraint2 = p2.constraints.get(name);
        let line1 = line_map1.get(name).copied();
        let line2 = line_map2.get(name).copied();

        match (constraint1, constraint2) {
            (Some(constraint), None) => {
                counts.removed += 1;
                entries.push(ConstraintDiffEntry {
                    name: name.to_string(),
                    kind: DiffKind::Removed,
                    detail: ConstraintDiffDetail::AddedOrRemoved(constraint.clone()),
                    line_file1: line1,
                    line_file2: line2,
                });
            }
            (None, Some(constraint)) => {
                counts.added += 1;
                entries.push(ConstraintDiffEntry {
                    name: name.to_string(),
                    kind: DiffKind::Added,
                    detail: ConstraintDiffDetail::AddedOrRemoved(constraint.clone()),
                    line_file1: line1,
                    line_file2: line2,
                });
            }
            (Some(c1), Some(c2)) => {
                if let Some(detail) = diff_constraint_pair(c1, c2) {
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
                entries.push(ObjectiveDiffEntry {
                    name: name.to_string(),
                    kind: DiffKind::Removed,
                    old_coefficients: o.coefficients.clone(),
                    new_coefficients: Vec::new(),
                    coeff_changes: Vec::new(),
                });
            }
            (None, Some(o)) => {
                counts.added += 1;
                entries.push(ObjectiveDiffEntry {
                    name: name.to_string(),
                    kind: DiffKind::Added,
                    old_coefficients: Vec::new(),
                    new_coefficients: o.coefficients.clone(),
                    coeff_changes: Vec::new(),
                });
            }
            (Some(o1), Some(o2)) => {
                let coeff_changes = diff_coefficients(&o1.coefficients, &o2.coefficients);
                if coeff_changes.is_empty() {
                    counts.unchanged += 1;
                } else {
                    counts.modified += 1;
                    entries.push(ObjectiveDiffEntry {
                        name: name.to_string(),
                        kind: DiffKind::Modified,
                        old_coefficients: o1.coefficients.clone(),
                        new_coefficients: o2.coefficients.clone(),
                        coeff_changes,
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

/// All inputs needed to build a diff report between two LP problem files.
pub struct DiffInput<'a> {
    /// Label or path for the first file.
    pub file1: &'a str,
    /// Label or path for the second file.
    pub file2: &'a str,
    /// The first LP problem.
    pub p1: &'a LpProblemOwned,
    /// The second LP problem.
    pub p2: &'a LpProblemOwned,
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
    use lp_parser_rs::model::{CoefficientOwned, ConstraintOwned, ObjectiveOwned, SOSType, Sense, VariableOwned, VariableType};
    use lp_parser_rs::problem::{LpProblem, LpProblemOwned};

    use super::*;

    /// Create a dummy `ProblemAnalysis` for tests that don't care about analysis content.
    fn dummy_analysis() -> ProblemAnalysis {
        let content = "Minimize\n obj: x\nSubject To\n c1: x >= 0\nEnd\n";
        let problem = LpProblem::parse(content).expect("dummy LP should parse");
        problem.analyze()
    }

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

    /// Shorthand for building a `DiffInput` and calling `build_diff_report` in tests.
    #[allow(clippy::too_many_arguments)]
    fn test_diff_report(
        file1: &str,
        file2: &str,
        p1: &LpProblemOwned,
        p2: &LpProblemOwned,
        line_map1: &HashMap<String, usize>,
        line_map2: &HashMap<String, usize>,
        analysis1: ProblemAnalysis,
        analysis2: ProblemAnalysis,
    ) -> LpDiffReport {
        build_diff_report(&DiffInput { file1, file2, p1, p2, line_map1, line_map2, analysis1, analysis2 })
    }

    /// Build a diff report from two problems with no line maps and dummy analyses.
    fn quick_report(p1: &LpProblemOwned, p2: &LpProblemOwned) -> LpDiffReport {
        test_diff_report("a.lp", "b.lp", p1, p2, &HashMap::new(), &HashMap::new(), dummy_analysis(), dummy_analysis())
    }

    fn assert_diff_entry(entry: &impl DiffEntry, name: &str, kind: DiffKind) {
        assert_eq!(entry.name(), name);
        assert_eq!(entry.kind(), kind);
    }

    #[test]
    fn test_empty_problems() {
        let report = quick_report(&empty_problem(), &empty_problem());

        assert!(report.variables.entries.is_empty());
        assert!(report.constraints.entries.is_empty());
        assert!(report.objectives.entries.is_empty());
        assert_eq!(report.summary().total_changes(), 0);
        assert!(report.sense_changed.is_none());
        assert!(report.name_changed.is_none());
    }

    macro_rules! variable_diff_tests {
        ($($name:ident: $p1:expr, $p2:expr => kind=$kind:expr, old=$old:expr, new=$new:expr);+ $(;)?) => {
            $(#[test] fn $name() {
                let p1 = $p1;
                let p2 = $p2;
                let report = quick_report(&p1, &p2);
                assert_eq!(report.variables.entries.len(), 1);
                let entry = &report.variables.entries[0];
                assert_eq!(entry.kind, $kind);
                assert_eq!(entry.old_type, $old);
                assert_eq!(entry.new_type, $new);
            })+
        };
    }

    variable_diff_tests! {
        var_added:    empty_problem(), problem_with_variable("x", VariableType::Binary)
            => kind=DiffKind::Added,    old=None,                        new=Some(VariableType::Binary);
        var_removed:  problem_with_variable("y", VariableType::Integer), empty_problem()
            => kind=DiffKind::Removed,  old=Some(VariableType::Integer), new=None;
        var_modified: problem_with_variable("z", VariableType::Free), problem_with_variable("z", VariableType::Binary)
            => kind=DiffKind::Modified, old=Some(VariableType::Free),    new=Some(VariableType::Binary)
    }

    #[test]
    fn test_constraint_coeff_diff() {
        // p1: c1: 2x + 3y <= 10
        // p2: c1: 2x + 5z <= 10   (y removed, z added, x unchanged)
        let p1 = problem_with_standard_constraint("c1", vec![("x", 2.0), ("y", 3.0)], ComparisonOp::LTE, 10.0);
        let p2 = problem_with_standard_constraint("c1", vec![("x", 2.0), ("z", 5.0)], ComparisonOp::LTE, 10.0);
        let report = quick_report(&p1, &p2);

        assert_eq!(report.constraints.entries.len(), 1);
        let entry = &report.constraints.entries[0];
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

        let report = quick_report(&p1, &p2);

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
        p2.add_objective(make_objective("obj1", vec![("a", 1.0), ("b", 5.0), ("c", 3.0)]));

        let report = quick_report(&p1, &p2);

        assert_eq!(report.objectives.entries.len(), 1);
        let entry = &report.objectives.entries[0];
        assert_eq!(entry.kind, DiffKind::Modified);
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
        let report = quick_report(&p1, &p2);

        assert!(report.sense_changed.is_some());
        let (old, new) = report.sense_changed.unwrap();
        assert_eq!(old, Sense::Minimize);
        assert_eq!(new, Sense::Maximize);
    }

    #[test]
    fn test_unchanged_not_stored() {
        let mut p1 = LpProblemOwned::new();
        let mut p2 = LpProblemOwned::new();

        for i in 0..10 {
            let name = format!("x{i}");
            p1.add_variable(VariableOwned::new(&name).with_var_type(VariableType::Free));
            p2.add_variable(VariableOwned::new(&name).with_var_type(VariableType::Free));
        }
        p1.add_variable(VariableOwned::new("changed").with_var_type(VariableType::Free));
        p2.add_variable(VariableOwned::new("changed").with_var_type(VariableType::Binary));

        let report = quick_report(&p1, &p2);

        assert_eq!(report.variables.entries.len(), 1);
        assert_eq!(report.variables.entries[0].name, "changed");
        assert_eq!(report.variables.counts.unchanged, 10);
        assert_eq!(report.variables.counts.modified, 1);
        assert_eq!(report.variables.counts.total(), 11);
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
            line_file1: None,
            line_file2: None,
        };
        assert_diff_entry(&added_constraint, "c1", DiffKind::Added);

        let added_obj = ObjectiveDiffEntry {
            name: "obj".to_string(),
            kind: DiffKind::Added,
            old_coefficients: vec![],
            new_coefficients: vec![
                CoefficientOwned { name: "a".to_string(), value: 1.0 },
                CoefficientOwned { name: "b".to_string(), value: 2.0 },
            ],
            coeff_changes: vec![],
        };
        assert_diff_entry(&added_obj, "obj", DiffKind::Added);
    }

    #[test]
    fn test_line_numbers_in_constraint_diff() {
        let p1 = problem_with_standard_constraint("c1", vec![("x", 1.0)], ComparisonOp::LTE, 10.0);
        let p2 = problem_with_standard_constraint("c1", vec![("x", 2.0)], ComparisonOp::LTE, 10.0);
        let mut lm1 = HashMap::new();
        lm1.insert("c1".to_string(), 5);
        let mut lm2 = HashMap::new();
        lm2.insert("c1".to_string(), 8);

        let report = test_diff_report("a.lp", "b.lp", &p1, &p2, &lm1, &lm2, dummy_analysis(), dummy_analysis());
        let entry = &report.constraints.entries[0];
        assert_eq!(entry.line_file1, Some(5));
        assert_eq!(entry.line_file2, Some(8));
    }
}
