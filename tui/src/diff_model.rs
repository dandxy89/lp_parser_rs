//! Rich diff data model and algorithm for comparing two LP problems.
//!
//! This module is pure data types and logic — no ratatui dependency.

use std::collections::HashMap;
use std::fmt;

use lp_parser_rs::analysis::ProblemAnalysis;
use lp_parser_rs::interner::{NameId, NameInterner};
use lp_parser_rs::model::{ComparisonOp, Constraint, SOSType, Sense, VariableType};
use lp_parser_rs::problem::LpProblem;

// Epsilon used for floating-point coefficient value comparison.
const COEFF_EPSILON: f64 = 1e-10;

/// Preset tolerance values cycled by the live `t` / `T` keys in the TUI.
pub const TOLERANCE_PRESETS: [f64; 5] = [0.0, 1e-9, 1e-6, 1e-4, 1e-2];

/// Return the next tolerance preset after `current`, wrapping around.
///
/// When `current` is not one of [`TOLERANCE_PRESETS`] (e.g. a custom CLI value),
/// the first press jumps to `TOLERANCE_PRESETS[0]` (tolerance off) — the simplest
/// well-defined starting point for the cycle.
#[must_use]
pub fn next_tolerance_preset(current: f64) -> f64 {
    debug_assert!(current.is_finite() && current >= 0.0, "tolerance must be finite and non-negative");
    match TOLERANCE_PRESETS.iter().position(|&p| p == current) {
        Some(index) => TOLERANCE_PRESETS[(index + 1) % TOLERANCE_PRESETS.len()],
        None => TOLERANCE_PRESETS[0],
    }
}

// Deliberate cap on pairwise rename comparisons within one (operator, term count)
// bucket. Buckets larger than this are skipped silently — the entries simply stay
// reported as added/removed rather than risking quadratic blow-up on degenerate models.
const MAX_RENAME_BUCKET_PAIRS: usize = 250_000;

/// Comparison options shared by the whole diff: name-rewrite rules and numeric tolerances.
///
/// Semantics mirror the `lp_parser diff` CLI: rename rules rewrite names in both files
/// before matching; `abs_tol`/`rel_tol` suppress near-equal RHS and coefficient changes.
#[derive(Clone, Default)]
pub struct DiffOptions {
    /// Absolute tolerance for RHS and coefficient comparisons. `0.0` disables.
    pub abs_tol: f64,
    /// Relative tolerance: treat `|a-b| <= rel_tol * max(|a|,|b|)` as equal. `0.0` disables.
    pub rel_tol: f64,
    /// Regex rewrite rules applied to every name in both files before matching.
    /// Rules apply in order; each is `(pattern, replacement)`.
    pub rename_rules: Vec<(regex::Regex, String)>,
}

impl fmt::Debug for DiffOptions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DiffOptions")
            .field("abs_tol", &self.abs_tol)
            .field("rel_tol", &self.rel_tol)
            .field("rename_rules", &self.rename_rules.len())
            .finish()
    }
}

impl DiffOptions {
    /// Rewrite a name through the rename rules in order. Returns the original string
    /// unchanged when no rules are configured (avoiding an allocation in the hot path).
    #[must_use]
    pub fn rewrite(&self, name: &str) -> String {
        if self.rename_rules.is_empty() {
            return name.to_string();
        }
        let mut s = name.to_string();
        for (re, rep) in &self.rename_rules {
            s = re.replace_all(&s, rep.as_str()).into_owned();
        }
        s
    }

    /// Decide whether two floats should be treated as different under the configured tolerances.
    ///
    /// Always floors the absolute tolerance at `COEFF_EPSILON` so that ordinary float noise
    /// (e.g. `1.0` vs `1.0 + 1e-15`) is still suppressed when no tolerance is supplied.
    #[must_use]
    pub fn numeric_differs(&self, a: f64, b: f64) -> bool {
        debug_assert!(self.abs_tol.is_finite() && self.abs_tol >= 0.0, "abs_tol must be finite and non-negative");
        debug_assert!(self.rel_tol.is_finite() && self.rel_tol >= 0.0, "rel_tol must be finite and non-negative");
        let diff = (a - b).abs();
        let abs_gate = self.abs_tol.max(COEFF_EPSILON);
        if diff <= abs_gate {
            return false;
        }
        diff > self.rel_tol * a.abs().max(b.abs())
    }
}

/// A coefficient with its variable name interned as a [`NameId`].
///
/// Names are resolved lazily via the [`LpDiffReport::resolve`] method, avoiding
/// per-coefficient `String` allocations during diff construction.
#[derive(Debug, Clone)]
pub struct ResolvedCoefficient {
    pub name: NameId,
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
    opts: &DiffOptions,
) -> bool {
    debug_assert!(!c1.is_empty() || !c2.is_empty() || c1.len() == c2.len(), "both slices empty is trivially equal");
    if c1.len() != c2.len() {
        return false;
    }
    c1.iter()
        .zip(c2.iter())
        .all(|(a, b)| opts.rewrite(p1.resolve(a.name)) == opts.rewrite(p2.resolve(b.name)) && !opts.numeric_differs(a.value, b.value))
}

/// Check whether two coefficient slices have the same set of (name, value) pairs
/// but in different positional order. Only called on the slow path when the fast
/// positional comparison has already failed.
fn coefficients_reordered(
    p1: &LpProblem,
    c1: &[lp_parser_rs::model::Coefficient],
    p2: &LpProblem,
    c2: &[lp_parser_rs::model::Coefficient],
    opts: &DiffOptions,
) -> bool {
    if c1.len() != c2.len() {
        return false;
    }
    // Compare using canonical (rewritten) names so that renamed-but-otherwise-equal
    // coefficient lists are not reported as reordered.
    let positional_match = c1.iter().zip(c2.iter()).all(|(a, b)| opts.rewrite(p1.resolve(a.name)) == opts.rewrite(p2.resolve(b.name)));
    if positional_match {
        return false;
    }
    let mut n1: Vec<String> = c1.iter().map(|c| opts.rewrite(p1.resolve(c.name))).collect();
    let mut n2: Vec<String> = c2.iter().map(|c| opts.rewrite(p2.resolve(c.name))).collect();
    n1.sort_unstable();
    n2.sort_unstable();
    n1 == n2
}

/// Intern a slice of model `Coefficient`s into `ResolvedCoefficient`s, adding
/// their names to the report's shared interner instead of allocating `String`s.
fn resolve_coefficients(
    problem: &LpProblem,
    coefficients: &[lp_parser_rs::model::Coefficient],
    interner: &mut NameInterner,
    opts: &DiffOptions,
) -> Vec<ResolvedCoefficient> {
    let mut out = Vec::with_capacity(coefficients.len());
    for c in coefficients {
        let name = interner.intern(&opts.rewrite(problem.resolve(c.name)));
        out.push(ResolvedCoefficient { name, value: c.value });
    }
    out
}

/// Resolve a model `Constraint` into a `ResolvedConstraint`, interning names
/// into the report's shared interner.
fn resolve_constraint(problem: &LpProblem, constraint: &Constraint, interner: &mut NameInterner, opts: &DiffOptions) -> ResolvedConstraint {
    match constraint {
        Constraint::Standard { coefficients, operator, rhs, .. } => ResolvedConstraint::Standard {
            coefficients: resolve_coefficients(problem, coefficients, interner, opts),
            operator: *operator,
            rhs: *rhs,
        },
        Constraint::SOS { sos_type, weights, .. } => {
            ResolvedConstraint::Sos { sos_type: *sos_type, weights: resolve_coefficients(problem, weights, interner, opts) }
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
    /// Shared interner for coefficient/variable names used in diff entries.
    /// Avoids per-coefficient `String` allocations by deduplicating names.
    pub interner: NameInterner,
    /// Summary of the `DiffOptions` that produced this report.
    pub options_summary: DiffOptionsSummary,
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
    /// Subset of `modified` where the only difference is coefficient order.
    pub order_only: usize,
    /// Structurally identical constraints matched across an added/removed pair.
    pub renamed: usize,
}

impl DiffCounts {
    /// Total number of entries (all states combined). A renamed pair counts once.
    #[must_use]
    pub const fn total(&self) -> usize {
        self.added + self.removed + self.modified + self.unchanged + self.renamed
    }

    /// Number of entries that differ in any way.
    #[must_use]
    pub const fn changed(&self) -> usize {
        self.added + self.removed + self.modified + self.renamed
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
            order_only: self.variables.order_only + self.constraints.order_only + self.objectives.order_only,
            renamed: self.variables.renamed + self.constraints.renamed + self.objectives.renamed,
        }
    }
}

/// The kind of change represented by a diff entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffKind {
    Added,
    Removed,
    Modified,
    /// Structurally identical constraint matched across an added/removed pair.
    /// Only produced for constraint entries; the old name lives in
    /// [`ConstraintDiffEntry::renamed_from`].
    Renamed,
}

impl fmt::Display for DiffKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Added => write!(f, "Added"),
            Self::Removed => write!(f, "Removed"),
            Self::Modified => write!(f, "Modified"),
            Self::Renamed => write!(f, "Renamed"),
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
    /// Whether the only difference is coefficient/weight ordering.
    pub order_only: bool,
    /// Name this constraint carried in the first file. Only set for
    /// [`DiffKind::Renamed`] entries; `name` holds the second file's name.
    pub renamed_from: Option<String>,
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
        /// Operator from the old (file 1) side; equals the new side when `operator_change` is `None`.
        old_operator: ComparisonOp,
        /// Whether the coefficient ordering differs between the two files.
        order_changed: bool,
    },
    /// Both versions are SOS constraints.
    Sos {
        old_weights: Vec<ResolvedCoefficient>,
        new_weights: Vec<ResolvedCoefficient>,
        weight_changes: Vec<CoefficientChange>,
        type_change: Option<(SOSType, SOSType)>,
        /// SOS type from the old (file 1) side; equals the new side when `type_change` is `None`.
        old_sos_type: SOSType,
        /// Whether the weight ordering differs between the two files.
        order_changed: bool,
    },
    /// Constraint changed from Standard to SOS or vice versa.
    TypeChanged { old_summary: String, new_summary: String },
    /// Constraint exists only in one of the two problems.
    AddedOrRemoved(ResolvedConstraint),
}

/// A change to a single coefficient (or weight) within a constraint or objective.
#[derive(Debug, Clone)]
pub struct CoefficientChange {
    pub variable: NameId,
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
    /// Whether the coefficient ordering differs between the two files.
    pub order_changed: bool,
    /// Whether the only difference is coefficient ordering.
    pub order_only: bool,
}

/// Trait implemented by all diff entry types so the TUI can render them uniformly.
pub trait DiffEntry {
    fn name(&self) -> &str;
    fn kind(&self) -> DiffKind;
    /// Whether this entry's only change is coefficient/weight ordering.
    fn is_order_only(&self) -> bool {
        false
    }
    /// Magnitude of the numeric change for delta-ranked sorting.
    ///
    /// Returns `Some(delta)` for [`DiffKind::Modified`] entries (zero when the
    /// modification has no numeric component, e.g. order-only or type changes)
    /// and `None` otherwise — added/removed/renamed entries have no delta.
    fn sort_delta(&self, relative: bool) -> Option<f64>;
}

impl DiffEntry for VariableDiffEntry {
    fn name(&self) -> &str {
        &self.name
    }

    fn kind(&self) -> DiffKind {
        self.kind
    }

    fn sort_delta(&self, relative: bool) -> Option<f64> {
        variable_sort_delta(self, relative)
    }
}

impl DiffEntry for ConstraintDiffEntry {
    fn name(&self) -> &str {
        &self.name
    }

    fn kind(&self) -> DiffKind {
        self.kind
    }

    fn is_order_only(&self) -> bool {
        self.order_only
    }

    fn sort_delta(&self, relative: bool) -> Option<f64> {
        constraint_sort_delta(self, relative)
    }
}

impl DiffEntry for ObjectiveDiffEntry {
    fn name(&self) -> &str {
        &self.name
    }

    fn kind(&self) -> DiffKind {
        self.kind
    }

    fn is_order_only(&self) -> bool {
        self.order_only
    }

    fn sort_delta(&self, relative: bool) -> Option<f64> {
        objective_sort_delta(self, relative)
    }
}

/// Extract (lower, upper) bounds from a `VariableType`, returning `None` for
/// bounds that don't apply to that type.
#[must_use]
pub const fn variable_bounds(variable_type: &VariableType) -> (Option<f64>, Option<f64>) {
    match *variable_type {
        VariableType::LowerBound(lower) => (Some(lower), None),
        VariableType::UpperBound(upper) => (None, Some(upper)),
        VariableType::DoubleBound(lower, upper) => (Some(lower), Some(upper)),
        _ => (None, None),
    }
}

/// Absolute difference `|new - old|` for a pair of optional values.
/// A missing side is treated as `0.0` (e.g. a newly introduced bound or coefficient).
#[must_use]
pub fn change_abs_delta(old: Option<f64>, new: Option<f64>) -> f64 {
    (new.unwrap_or(0.0) - old.unwrap_or(0.0)).abs()
}

/// Relative difference `|new - old| / max(|new|, |old|)` for a pair of optional
/// values, with a missing side treated as `0.0`. Returns `0.0` when both
/// magnitudes are zero, guarding against division by zero.
#[must_use]
pub fn change_rel_delta(old: Option<f64>, new: Option<f64>) -> f64 {
    let a = old.unwrap_or(0.0);
    let b = new.unwrap_or(0.0);
    let denominator = a.abs().max(b.abs());
    if denominator == 0.0 { 0.0 } else { (b - a).abs() / denominator }
}

/// Dispatch to the absolute or relative pair delta.
fn change_delta(old: Option<f64>, new: Option<f64>, relative: bool) -> f64 {
    if relative { change_rel_delta(old, new) } else { change_abs_delta(old, new) }
}

/// Maximum delta across a list of coefficient changes.
fn max_coefficient_delta(changes: &[CoefficientChange], relative: bool) -> f64 {
    changes.iter().map(|c| change_delta(c.old_value, c.new_value, relative)).fold(0.0, f64::max)
}

/// Sort key for a modified constraint: the maximum delta over the RHS change and
/// all coefficient/weight changes. `None` for non-modified entries.
#[must_use]
pub fn constraint_sort_delta(entry: &ConstraintDiffEntry, relative: bool) -> Option<f64> {
    if entry.kind != DiffKind::Modified {
        return None;
    }
    let max = match &entry.detail {
        ConstraintDiffDetail::Standard { coeff_changes, rhs_change, .. } => {
            let rhs_delta = rhs_change.map_or(0.0, |(old, new)| change_delta(Some(old), Some(new), relative));
            rhs_delta.max(max_coefficient_delta(coeff_changes, relative))
        }
        ConstraintDiffDetail::Sos { weight_changes, .. } => max_coefficient_delta(weight_changes, relative),
        // Type changes have no numeric delta but are still modifications —
        // group them with the zero-delta modified entries.
        ConstraintDiffDetail::TypeChanged { .. } | ConstraintDiffDetail::AddedOrRemoved(_) => 0.0,
    };
    debug_assert!(max.is_finite() && max >= 0.0, "constraint sort delta must be finite and non-negative");
    Some(max)
}

/// Sort key for a modified variable: the maximum delta across its lower and upper
/// bound changes (a missing bound counts as `0.0`). `None` for non-modified entries.
#[must_use]
pub fn variable_sort_delta(entry: &VariableDiffEntry, relative: bool) -> Option<f64> {
    if entry.kind != DiffKind::Modified {
        return None;
    }
    let (old_lower, old_upper) = entry.old_type.as_ref().map_or((None, None), variable_bounds);
    let (new_lower, new_upper) = entry.new_type.as_ref().map_or((None, None), variable_bounds);
    let max = change_delta(old_lower, new_lower, relative).max(change_delta(old_upper, new_upper, relative));
    debug_assert!(max.is_finite() && max >= 0.0, "variable sort delta must be finite and non-negative");
    Some(max)
}

/// Sort key for a modified objective: the maximum delta across its coefficient
/// changes. `None` for non-modified entries.
#[must_use]
pub fn objective_sort_delta(entry: &ObjectiveDiffEntry, relative: bool) -> Option<f64> {
    if entry.kind != DiffKind::Modified {
        return None;
    }
    Some(max_coefficient_delta(&entry.coeff_changes, relative))
}

/// Sort `indices` (positions into `entries`) by descending delta.
///
/// Modified entries (which carry a delta) come first, largest delta first;
/// entries without a delta (added/removed/renamed) follow, alphabetically.
/// Ties within the delta group fall back to name order.
pub fn sort_indices_by_delta<T: DiffEntry>(entries: &[T], indices: &mut [usize], relative: bool) {
    debug_assert!(indices.iter().all(|&i| i < entries.len()), "all indices must be in bounds");
    indices.sort_by(|&a, &b| {
        let delta_a = entries[a].sort_delta(relative);
        let delta_b = entries[b].sort_delta(relative);
        match (delta_a, delta_b) {
            (Some(x), Some(y)) => y.total_cmp(&x).then_with(|| entries[a].name().cmp(entries[b].name())),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => entries[a].name().cmp(entries[b].name()),
        }
    });
}

/// Diff two coefficient lists using sorted merge-join and return only the changed entries.
///
/// Both slices are sorted by name (resolved via `interner`) before merging. Numeric
/// comparison respects `opts.abs_tol` / `opts.rel_tol` (with a `COEFF_EPSILON` floor).
fn diff_coefficients(
    old: &[ResolvedCoefficient],
    new: &[ResolvedCoefficient],
    interner: &NameInterner,
    opts: &DiffOptions,
) -> Vec<CoefficientChange> {
    let mut old_sorted: Vec<(NameId, f64)> = old.iter().map(|c| (c.name, c.value)).collect();
    let mut new_sorted: Vec<(NameId, f64)> = new.iter().map(|c| (c.name, c.value)).collect();
    old_sorted.sort_unstable_by(|a, b| interner.resolve(a.0).cmp(interner.resolve(b.0)));
    new_sorted.sort_unstable_by(|a, b| interner.resolve(a.0).cmp(interner.resolve(b.0)));

    let mut changes = Vec::new();
    let mut i = 0;
    let mut j = 0;

    while i < old_sorted.len() || j < new_sorted.len() {
        debug_assert!(i <= old_sorted.len(), "old index out of bounds");
        debug_assert!(j <= new_sorted.len(), "new index out of bounds");

        let cmp = match (old_sorted.get(i), new_sorted.get(j)) {
            (Some((n1, _)), Some((n2, _))) => interner.resolve(*n1).cmp(interner.resolve(*n2)),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => break,
        };

        match cmp {
            std::cmp::Ordering::Less => {
                let (name, old_val) = old_sorted[i];
                changes.push(CoefficientChange { variable: name, kind: DiffKind::Removed, old_value: Some(old_val), new_value: None });
                i += 1;
            }
            std::cmp::Ordering::Greater => {
                let (name, new_val) = new_sorted[j];
                changes.push(CoefficientChange { variable: name, kind: DiffKind::Added, old_value: None, new_value: Some(new_val) });
                j += 1;
            }
            std::cmp::Ordering::Equal => {
                let (name, old_val) = old_sorted[i];
                let new_val = new_sorted[j].1;
                if opts.numeric_differs(old_val, new_val) {
                    changes.push(CoefficientChange {
                        variable: name,
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

/// Build a sorted vec of (canonical_name, &Variable) pairs.
/// `canonical_name` is the original name with `opts.rename_rules` applied.
fn build_sorted_vars<'a>(problem: &'a LpProblem, opts: &DiffOptions) -> Vec<(String, &'a lp_parser_rs::model::Variable)> {
    let mut pairs: Vec<_> = problem.variables.iter().map(|(id, var)| (opts.rewrite(problem.resolve(*id)), var)).collect();
    pairs.sort_unstable_by(|a, b| a.0.cmp(&b.0));
    pairs
}

/// Build a sorted vec of (original_name_id, canonical_name, &Constraint) triples.
///
/// Preserves the original per-file `NameId` for line-map lookups while using the
/// rewritten (canonical) name for sort and merge-join.
fn build_sorted_constraints<'a>(problem: &'a LpProblem, opts: &DiffOptions) -> Vec<(NameId, String, &'a Constraint)> {
    let mut triples: Vec<_> = problem.constraints.iter().map(|(id, c)| (*id, opts.rewrite(problem.resolve(*id)), c)).collect();
    triples.sort_unstable_by(|a, b| a.1.cmp(&b.1));
    triples
}

/// Build a sorted vec of (canonical_name, &Objective) pairs.
fn build_sorted_objectives<'a>(problem: &'a LpProblem, opts: &DiffOptions) -> Vec<(String, &'a lp_parser_rs::model::Objective)> {
    let mut pairs: Vec<_> = problem.objectives.iter().map(|(id, o)| (opts.rewrite(problem.resolve(*id)), o)).collect();
    pairs.sort_unstable_by(|a, b| a.0.cmp(&b.0));
    pairs
}

/// Diff the variables section between two problems using sorted merge-join.
fn diff_variables(p1: &LpProblem, p2: &LpProblem, opts: &DiffOptions) -> SectionDiff<VariableDiffEntry> {
    let vars1 = build_sorted_vars(p1, opts);
    let vars2 = build_sorted_vars(p2, opts);

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
                let entry =
                    VariableDiffEntry { name: name.clone(), kind: DiffKind::Removed, old_type: Some(v1.var_type.clone()), new_type: None };
                debug_assert!(entry.old_type.is_some(), "Removed variable must have old_type");
                debug_assert!(entry.new_type.is_none(), "Removed variable must not have new_type");
                entries.push(entry);
                i += 1;
            }
            std::cmp::Ordering::Greater => {
                let (name, v2) = &vars2[j];
                counts.added += 1;
                let entry =
                    VariableDiffEntry { name: name.clone(), kind: DiffKind::Added, old_type: None, new_type: Some(v2.var_type.clone()) };
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
                        name: name.clone(),
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
#[allow(clippy::too_many_arguments)]
fn diff_standard_constraints(
    old_coefficients: &[ResolvedCoefficient],
    old_operator: &ComparisonOp,
    old_rhs: f64,
    new_coefficients: &[ResolvedCoefficient],
    new_operator: &ComparisonOp,
    new_rhs: f64,
    interner: &NameInterner,
    order_changed: bool,
    opts: &DiffOptions,
) -> Option<ConstraintDiffDetail> {
    let coeff_changes = diff_coefficients(old_coefficients, new_coefficients, interner, opts);
    let operator_change = if old_operator == new_operator { None } else { Some((*old_operator, *new_operator)) };
    let rhs_change = if opts.numeric_differs(old_rhs, new_rhs) { Some((old_rhs, new_rhs)) } else { None };

    if coeff_changes.is_empty() && operator_change.is_none() && rhs_change.is_none() && !order_changed {
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
        old_operator: *old_operator,
        order_changed,
    })
}

/// Diff two SOS constraints and return the detail if they differ, `None` if unchanged.
fn diff_sos_constraints(
    old_type: &SOSType,
    old_weights: &[ResolvedCoefficient],
    new_type: &SOSType,
    new_weights: &[ResolvedCoefficient],
    interner: &NameInterner,
    order_changed: bool,
    opts: &DiffOptions,
) -> Option<ConstraintDiffDetail> {
    let weight_changes = diff_coefficients(old_weights, new_weights, interner, opts);
    let type_change = if old_type == new_type { None } else { Some((*old_type, *new_type)) };

    if weight_changes.is_empty() && type_change.is_none() && !order_changed {
        return None;
    }

    Some(ConstraintDiffDetail::Sos {
        old_weights: old_weights.to_vec(),
        new_weights: new_weights.to_vec(),
        weight_changes,
        type_change,
        old_sos_type: *old_type,
        order_changed,
    })
}

/// Compute the detail for two constraints that both exist, returning `None` if unchanged.
fn diff_constraint_pair(
    p1: &LpProblem,
    c1: &Constraint,
    p2: &LpProblem,
    c2: &Constraint,
    interner: &mut NameInterner,
    opts: &DiffOptions,
) -> Option<ConstraintDiffDetail> {
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
            // Fast path: skip resolution if the raw data is identical under the configured tolerances.
            if old_operator == new_operator
                && !opts.numeric_differs(*old_rhs, *new_rhs)
                && coefficients_equal(p1, old_coefficients, p2, new_coefficients, opts)
            {
                return None;
            }
            let reordered = coefficients_reordered(p1, old_coefficients, p2, new_coefficients, opts);
            let old_resolved = resolve_coefficients(p1, old_coefficients, interner, opts);
            let new_resolved = resolve_coefficients(p2, new_coefficients, interner, opts);
            diff_standard_constraints(
                &old_resolved,
                old_operator,
                *old_rhs,
                &new_resolved,
                new_operator,
                *new_rhs,
                interner,
                reordered,
                opts,
            )
        }

        // Both SOS: diff weights and sos_type.
        (
            Constraint::SOS { sos_type: old_type, weights: old_weights, .. },
            Constraint::SOS { sos_type: new_type, weights: new_weights, .. },
        ) => {
            // Fast path: skip resolution if the raw data is identical.
            if old_type == new_type && coefficients_equal(p1, old_weights, p2, new_weights, opts) {
                return None;
            }
            let reordered = coefficients_reordered(p1, old_weights, p2, new_weights, opts);
            let old_resolved = resolve_coefficients(p1, old_weights, interner, opts);
            let new_resolved = resolve_coefficients(p2, new_weights, interner, opts);
            diff_sos_constraints(old_type, &old_resolved, new_type, &new_resolved, interner, reordered, opts)
        }
    }
}

/// Diff the constraints section between two problems using sorted merge-join.
fn diff_constraints(
    p1: &LpProblem,
    p2: &LpProblem,
    line_map1: &HashMap<NameId, usize>,
    line_map2: &HashMap<NameId, usize>,
    interner: &mut NameInterner,
    opts: &DiffOptions,
) -> SectionDiff<ConstraintDiffEntry> {
    let cons1 = build_sorted_constraints(p1, opts);
    let cons2 = build_sorted_constraints(p2, opts);

    let mut entries = Vec::new();
    let mut counts = DiffCounts::default();
    let mut i = 0;
    let mut j = 0;

    while i < cons1.len() || j < cons2.len() {
        debug_assert!(i <= cons1.len(), "cons1 index out of bounds");
        debug_assert!(j <= cons2.len(), "cons2 index out of bounds");

        let cmp = match (cons1.get(i), cons2.get(j)) {
            (Some((_, n1, _)), Some((_, n2, _))) => n1.cmp(n2),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => break,
        };

        match cmp {
            std::cmp::Ordering::Less => {
                let (name_id, name, constraint) = &cons1[i];
                // Line-map lookup uses the ORIGINAL per-file NameId (pre-rename).
                let line1 = line_map1.get(name_id).copied();
                let line2 = line_map2.get(name_id).copied();
                counts.removed += 1;
                entries.push(ConstraintDiffEntry {
                    name: name.clone(),
                    kind: DiffKind::Removed,
                    detail: ConstraintDiffDetail::AddedOrRemoved(resolve_constraint(p1, constraint, interner, opts)),
                    line_file1: line1,
                    line_file2: line2,
                    order_only: false,
                    renamed_from: None,
                });
                i += 1;
            }
            std::cmp::Ordering::Greater => {
                let (name_id, name, constraint) = &cons2[j];
                let line1 = line_map1.get(name_id).copied();
                let line2 = line_map2.get(name_id).copied();
                counts.added += 1;
                entries.push(ConstraintDiffEntry {
                    name: name.clone(),
                    kind: DiffKind::Added,
                    detail: ConstraintDiffDetail::AddedOrRemoved(resolve_constraint(p2, constraint, interner, opts)),
                    line_file1: line1,
                    line_file2: line2,
                    order_only: false,
                    renamed_from: None,
                });
                j += 1;
            }
            std::cmp::Ordering::Equal => {
                let (name_id1, name, c1) = &cons1[i];
                let (name_id2, _, c2) = &cons2[j];
                let line1 = line_map1.get(name_id1).copied();
                let line2 = line_map2.get(name_id2).copied();
                if let Some(detail) = diff_constraint_pair(p1, c1, p2, c2, interner, opts) {
                    // Determine if this is an order-only change.
                    let order_only = match &detail {
                        ConstraintDiffDetail::Standard { coeff_changes, operator_change, rhs_change, order_changed, .. } => {
                            coeff_changes.is_empty() && operator_change.is_none() && rhs_change.is_none() && *order_changed
                        }
                        ConstraintDiffDetail::Sos { weight_changes, type_change, order_changed, .. } => {
                            weight_changes.is_empty() && type_change.is_none() && *order_changed
                        }
                        _ => false,
                    };
                    counts.modified += 1;
                    if order_only {
                        counts.order_only += 1;
                    }
                    entries.push(ConstraintDiffEntry {
                        name: name.clone(),
                        kind: DiffKind::Modified,
                        detail,
                        line_file1: line1,
                        line_file2: line2,
                        order_only,
                        renamed_from: None,
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

/// A candidate constraint for rename matching: its entry index, RHS, and a
/// structural signature of (variable name, coefficient) pairs sorted by name.
struct RenameCandidate {
    entry_index: usize,
    rhs: f64,
    /// Sorted by `NameId`. IDs come from the report's shared interner, so equal
    /// canonical names (post `--rename` rewriting) have equal IDs.
    signature: Vec<(NameId, f64)>,
}

/// True when two candidates are structurally identical under the active tolerances:
/// same RHS and the same sorted (name, coefficient) list, with numeric fields
/// compared via [`DiffOptions::numeric_differs`].
fn rename_signatures_match(removed: &RenameCandidate, added: &RenameCandidate, opts: &DiffOptions) -> bool {
    debug_assert_eq!(removed.signature.len(), added.signature.len(), "bucketing guarantees equal term counts");
    if opts.numeric_differs(removed.rhs, added.rhs) {
        return false;
    }
    removed
        .signature
        .iter()
        .zip(added.signature.iter())
        .all(|((name1, value1), (name2, value2))| name1 == name2 && !opts.numeric_differs(*value1, *value2))
}

/// Map a `ComparisonOp` to a stable small integer for use in a bucket key
/// (`ComparisonOp` does not implement `Hash`).
const fn operator_bucket_key(operator: ComparisonOp) -> u8 {
    match operator {
        ComparisonOp::LTE => 0,
        ComparisonOp::GTE => 1,
        ComparisonOp::EQ => 2,
        ComparisonOp::LT => 3,
        ComparisonOp::GT => 4,
    }
}

/// Detect renamed constraints among the added/removed standard constraints.
///
/// Generated LP models often shift row indices between runs, producing large
/// added+removed pairs that are really the same constraint under a new name.
/// This buckets candidates by the cheap exact key (operator, term count) and then
/// does tolerance-aware pairwise comparison of sorted coefficient signatures,
/// greedily taking the first match. Each matched pair collapses into a single
/// [`DiffKind::Renamed`] entry carrying the old name in `renamed_from`.
///
/// SOS constraints are skipped for v1 — rename detection covers standard
/// constraints only.
fn detect_constraint_renames(section: &mut SectionDiff<ConstraintDiffEntry>, opts: &DiffOptions) {
    // (removed candidates, added candidates) bucketed by (operator key, term count).
    let mut buckets: HashMap<(u8, usize), (Vec<RenameCandidate>, Vec<RenameCandidate>)> = HashMap::new();

    for (entry_index, entry) in section.entries.iter().enumerate() {
        let is_added = match entry.kind {
            DiffKind::Added => true,
            DiffKind::Removed => false,
            DiffKind::Modified | DiffKind::Renamed => continue,
        };
        // Standard constraints only — SOS rename detection is deferred (see doc comment).
        let ConstraintDiffDetail::AddedOrRemoved(ResolvedConstraint::Standard { coefficients, operator, rhs }) = &entry.detail else {
            continue;
        };
        let mut signature: Vec<(NameId, f64)> = coefficients.iter().map(|c| (c.name, c.value)).collect();
        signature.sort_unstable_by_key(|(name, _)| *name);
        let bucket = buckets.entry((operator_bucket_key(*operator), signature.len())).or_default();
        let candidate = RenameCandidate { entry_index, rhs: *rhs, signature };
        if is_added {
            bucket.1.push(candidate);
        } else {
            bucket.0.push(candidate);
        }
    }

    // Greedy first-match pairing of (removed entry index, added entry index).
    let mut pairs: Vec<(usize, usize)> = Vec::new();
    for (removed, added) in buckets.into_values() {
        if removed.is_empty() || added.is_empty() {
            continue;
        }
        // Deliberate cap: skip pathological buckets rather than doing an unbounded
        // quadratic scan. Nothing is surfaced — these entries stay added/removed.
        if removed.len().saturating_mul(added.len()) > MAX_RENAME_BUCKET_PAIRS {
            continue;
        }
        let mut added_used = vec![false; added.len()];
        for removed_candidate in &removed {
            for (j, added_candidate) in added.iter().enumerate() {
                if added_used[j] {
                    continue;
                }
                if rename_signatures_match(removed_candidate, added_candidate, opts) {
                    added_used[j] = true;
                    pairs.push((removed_candidate.entry_index, added_candidate.entry_index));
                    break;
                }
            }
        }
    }

    if pairs.is_empty() {
        return;
    }

    // Build one Renamed entry per matched pair.
    let mut renamed_entries = Vec::with_capacity(pairs.len());
    for &(removed_index, added_index) in &pairs {
        let removed_entry = &section.entries[removed_index];
        let added_entry = &section.entries[added_index];
        let ConstraintDiffDetail::AddedOrRemoved(ResolvedConstraint::Standard {
            coefficients: old_coefficients,
            operator: old_operator,
            rhs: old_rhs,
        }) = &removed_entry.detail
        else {
            unreachable!("rename candidates are always standard AddedOrRemoved entries");
        };
        let ConstraintDiffDetail::AddedOrRemoved(ResolvedConstraint::Standard { coefficients: new_coefficients, rhs: new_rhs, .. }) =
            &added_entry.detail
        else {
            unreachable!("rename candidates are always standard AddedOrRemoved entries");
        };
        // Positional comparison is valid: term counts are equal within a bucket.
        let order_changed = old_coefficients.iter().zip(new_coefficients.iter()).any(|(a, b)| a.name != b.name);
        renamed_entries.push(ConstraintDiffEntry {
            name: added_entry.name.clone(),
            kind: DiffKind::Renamed,
            detail: ConstraintDiffDetail::Standard {
                old_coefficients: old_coefficients.clone(),
                new_coefficients: new_coefficients.clone(),
                // Empty by construction: the pair matched under the active tolerances.
                coeff_changes: Vec::new(),
                operator_change: None,
                rhs_change: None,
                old_rhs: *old_rhs,
                new_rhs: *new_rhs,
                old_operator: *old_operator,
                order_changed,
            },
            line_file1: removed_entry.line_file1,
            line_file2: added_entry.line_file2,
            order_only: false,
            renamed_from: Some(removed_entry.name.clone()),
        });
    }

    // Drop the matched added/removed entries and splice in the renamed ones,
    // restoring the sorted-by-name invariant.
    let matched: std::collections::HashSet<usize> =
        pairs.iter().flat_map(|&(removed_index, added_index)| [removed_index, added_index]).collect();
    debug_assert_eq!(matched.len(), pairs.len() * 2, "an entry must not appear in more than one rename pair");
    let mut kept: Vec<ConstraintDiffEntry> = Vec::with_capacity(section.entries.len() - pairs.len());
    for (index, entry) in section.entries.drain(..).enumerate() {
        if !matched.contains(&index) {
            kept.push(entry);
        }
    }
    kept.append(&mut renamed_entries);
    kept.sort_by(|a, b| a.name.cmp(&b.name));
    section.entries = kept;

    debug_assert!(section.counts.added >= pairs.len(), "added count must cover matched pairs");
    debug_assert!(section.counts.removed >= pairs.len(), "removed count must cover matched pairs");
    section.counts.added -= pairs.len();
    section.counts.removed -= pairs.len();
    section.counts.renamed += pairs.len();
}

/// Diff the objectives section between two problems using sorted merge-join.
fn diff_objectives(p1: &LpProblem, p2: &LpProblem, interner: &mut NameInterner, opts: &DiffOptions) -> SectionDiff<ObjectiveDiffEntry> {
    let objs1 = build_sorted_objectives(p1, opts);
    let objs2 = build_sorted_objectives(p2, opts);

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
                    name: name.clone(),
                    kind: DiffKind::Removed,
                    old_coefficients: resolve_coefficients(p1, &o.coefficients, interner, opts),
                    new_coefficients: Vec::new(),
                    coeff_changes: Vec::new(),
                    order_changed: false,
                    order_only: false,
                });
                i += 1;
            }
            std::cmp::Ordering::Greater => {
                let (name, o) = &objs2[j];
                counts.added += 1;
                entries.push(ObjectiveDiffEntry {
                    name: name.clone(),
                    kind: DiffKind::Added,
                    old_coefficients: Vec::new(),
                    new_coefficients: resolve_coefficients(p2, &o.coefficients, interner, opts),
                    coeff_changes: Vec::new(),
                    order_changed: false,
                    order_only: false,
                });
                j += 1;
            }
            std::cmp::Ordering::Equal => {
                let (name, o1_val) = &objs1[i];
                let o2_val = objs2[j].1;
                // Fast path: skip resolution if the raw coefficients are identical under tolerance/rename.
                if coefficients_equal(p1, &o1_val.coefficients, p2, &o2_val.coefficients, opts) {
                    counts.unchanged += 1;
                    i += 1;
                    j += 1;
                    continue;
                }
                let reordered = coefficients_reordered(p1, &o1_val.coefficients, p2, &o2_val.coefficients, opts);
                let old_resolved = resolve_coefficients(p1, &o1_val.coefficients, interner, opts);
                let new_resolved = resolve_coefficients(p2, &o2_val.coefficients, interner, opts);
                let coeff_changes = diff_coefficients(&old_resolved, &new_resolved, interner, opts);
                if coeff_changes.is_empty() && !reordered {
                    counts.unchanged += 1;
                } else {
                    let order_only = coeff_changes.is_empty() && reordered;
                    counts.modified += 1;
                    if order_only {
                        counts.order_only += 1;
                    }
                    entries.push(ObjectiveDiffEntry {
                        name: name.clone(),
                        kind: DiffKind::Modified,
                        old_coefficients: old_resolved,
                        new_coefficients: new_resolved,
                        coeff_changes,
                        order_changed: reordered,
                        order_only,
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
    /// Constraint NameId → 1-based line number for file 1.
    pub line_map1: &'a HashMap<NameId, usize>,
    /// Constraint NameId → 1-based line number for file 2.
    pub line_map2: &'a HashMap<NameId, usize>,
    /// Structural analysis of the first file.
    pub analysis1: ProblemAnalysis,
    /// Structural analysis of the second file.
    pub analysis2: ProblemAnalysis,
    /// Comparison options (rename rules + numeric tolerances).
    pub options: DiffOptions,
}

/// Build a complete diff report comparing two LP problems.
pub fn build_diff_report(input: &DiffInput<'_>) -> LpDiffReport {
    debug_assert!(!input.file1.is_empty(), "file1 label must not be empty");
    debug_assert!(!input.file2.is_empty(), "file2 label must not be empty");

    let mut interner = NameInterner::new();
    let opts = &input.options;

    let variables = diff_variables(input.p1, input.p2, opts);
    let mut constraints = diff_constraints(input.p1, input.p2, input.line_map1, input.line_map2, &mut interner, opts);
    detect_constraint_renames(&mut constraints, opts);
    let objectives = diff_objectives(input.p1, input.p2, &mut interner, opts);

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
        interner,
        options_summary: DiffOptionsSummary { abs_tol: opts.abs_tol, rel_tol: opts.rel_tol, rename_rule_count: opts.rename_rules.len() },
    }
}

/// Compact, copyable summary of the comparison options that produced a report.
///
/// Stored on [`LpDiffReport`] so TUI widgets and `--summary` output can surface the
/// configuration without holding a reference to the original `DiffOptions` (which
/// owns compiled `Regex` values).
#[derive(Debug, Clone, Copy, Default)]
pub struct DiffOptionsSummary {
    pub abs_tol: f64,
    pub rel_tol: f64,
    pub rename_rule_count: usize,
}

impl DiffOptionsSummary {
    /// True when every field has its default value — nothing worth displaying.
    #[must_use]
    pub fn is_default(&self) -> bool {
        self.abs_tol == 0.0 && self.rel_tol == 0.0 && self.rename_rule_count == 0
    }
}

impl fmt::Display for DiffOptionsSummary {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "abs_tol={}  rel_tol={}  rename_rules={}", self.abs_tol, self.rel_tol, self.rename_rule_count)
    }
}

#[cfg(test)]
mod tests {
    use std::fmt::Write;

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
        line_map1: &HashMap<NameId, usize>,
        line_map2: &HashMap<NameId, usize>,
        analysis1: ProblemAnalysis,
        analysis2: ProblemAnalysis,
    ) -> LpDiffReport {
        build_diff_report(&DiffInput { file1, file2, p1, p2, line_map1, line_map2, analysis1, analysis2, options: DiffOptions::default() })
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

            let removed = coeff_changes.iter().find(|c| report.interner.resolve(c.variable) == "y").expect("y should be removed");
            assert_eq!(removed.kind, DiffKind::Removed);
            assert_eq!(removed.old_value, Some(3.0));
            assert!(removed.new_value.is_none());

            let added = coeff_changes.iter().find(|c| report.interner.resolve(c.variable) == "z").expect("z should be added");
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

        let b_change = entry.coeff_changes.iter().find(|c| report.interner.resolve(c.variable) == "b").expect("b should be modified");
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
        let mut interner = lp_parser_rs::interner::NameInterner::new();

        let added_var =
            VariableDiffEntry { name: "x".to_string(), kind: DiffKind::Added, old_type: None, new_type: Some(VariableType::Binary) };
        assert_diff_entry(&added_var, "x", DiffKind::Added);

        let removed_var =
            VariableDiffEntry { name: "y".to_string(), kind: DiffKind::Removed, old_type: Some(VariableType::Integer), new_type: None };
        assert_diff_entry(&removed_var, "y", DiffKind::Removed);

        let x_id = interner.intern("x");
        let y_id = interner.intern("y");
        let added_constraint = ConstraintDiffEntry {
            name: "c1".to_string(),
            kind: DiffKind::Added,
            detail: ConstraintDiffDetail::AddedOrRemoved(ResolvedConstraint::Standard {
                coefficients: vec![ResolvedCoefficient { name: x_id, value: 1.0 }, ResolvedCoefficient { name: y_id, value: 2.0 }],
                operator: ComparisonOp::LTE,
                rhs: 5.0,
            }),
            line_file1: None,
            line_file2: None,
            order_only: false,
            renamed_from: None,
        };
        assert_diff_entry(&added_constraint, "c1", DiffKind::Added);

        let a_id = interner.intern("a");
        let b_id = interner.intern("b");
        let added_obj = ObjectiveDiffEntry {
            name: "obj".to_string(),
            kind: DiffKind::Added,
            old_coefficients: vec![],
            new_coefficients: vec![ResolvedCoefficient { name: a_id, value: 1.0 }, ResolvedCoefficient { name: b_id, value: 2.0 }],
            coeff_changes: vec![],
            order_changed: false,
            order_only: false,
        };
        assert_diff_entry(&added_obj, "obj", DiffKind::Added);
    }

    /// Look up the `NameId` for a constraint name in a problem.
    fn constraint_name_id(problem: &LpProblem, name: &str) -> NameId {
        *problem.constraints.keys().find(|id| problem.resolve(**id) == name).expect("constraint name should exist")
    }

    #[test]
    fn test_line_numbers_in_constraint_diff() {
        let p1 = problem_with_standard_constraint("c1", &[("x", 1.0)], &ComparisonOp::LTE, 10.0);
        let p2 = problem_with_standard_constraint("c1", &[("x", 2.0)], &ComparisonOp::LTE, 10.0);
        let mut lm1 = HashMap::new();
        lm1.insert(constraint_name_id(&p1, "c1"), 5);
        let mut lm2 = HashMap::new();
        lm2.insert(constraint_name_id(&p2, "c1"), 8);

        let report = test_diff_report("a.lp", "b.lp", &p1, &p2, &lm1, &lm2, dummy_analysis(), dummy_analysis());
        let entry = report.constraints.entries.iter().find(|e| e.name == "c1").expect("should have c1 entry");
        assert_eq!(entry.line_file1, Some(5));
        assert_eq!(entry.line_file2, Some(8));
    }

    #[test]
    fn test_constraint_order_only_change() {
        // Same coefficients, different order: 2x + 3y vs 3y + 2x
        let p1 = problem_with_standard_constraint("c1", &[("x", 2.0), ("y", 3.0)], &ComparisonOp::LTE, 10.0);
        let p2 = problem_with_standard_constraint("c1", &[("y", 3.0), ("x", 2.0)], &ComparisonOp::LTE, 10.0);
        let report = quick_report(&p1, &p2);

        let entry = report.constraints.entries.iter().find(|e| e.name == "c1");
        assert!(entry.is_some(), "order-only constraint change should produce an entry");
        let entry = entry.unwrap();
        assert_eq!(entry.kind, DiffKind::Modified);
        assert!(entry.order_only, "entry should be order_only");
        assert!(entry.is_order_only(), "DiffEntry::is_order_only() should return true");

        if let ConstraintDiffDetail::Standard { order_changed, coeff_changes, .. } = &entry.detail {
            assert!(order_changed, "order_changed flag should be set");
            assert!(coeff_changes.is_empty(), "no value changes expected");
        } else {
            panic!("Expected Standard detail");
        }

        assert_eq!(report.constraints.counts.modified, 1);
        assert_eq!(report.constraints.counts.order_only, 1);
    }

    #[test]
    fn test_constraint_order_with_value_change() {
        // Different order AND different coefficient value.
        let p1 = problem_with_standard_constraint("c1", &[("x", 2.0), ("y", 3.0)], &ComparisonOp::LTE, 10.0);
        let p2 = problem_with_standard_constraint("c1", &[("y", 5.0), ("x", 2.0)], &ComparisonOp::LTE, 10.0);
        let report = quick_report(&p1, &p2);

        let entry = report.constraints.entries.iter().find(|e| e.name == "c1").unwrap();
        assert_eq!(entry.kind, DiffKind::Modified);
        assert!(!entry.order_only, "entry should NOT be order_only when values differ");

        if let ConstraintDiffDetail::Standard { order_changed, coeff_changes, .. } = &entry.detail {
            assert!(order_changed, "order_changed flag should still be set");
            assert!(!coeff_changes.is_empty(), "value changes expected");
        } else {
            panic!("Expected Standard detail");
        }

        assert_eq!(report.constraints.counts.order_only, 0);
    }

    #[test]
    fn test_objective_order_only_change() {
        // Same coefficients, different order.
        let p1 = problem_with_objective("obj1", &[("a", 1.0), ("b", 2.0)]);
        let p2 = problem_with_objective("obj1", &[("b", 2.0), ("a", 1.0)]);
        let report = quick_report(&p1, &p2);

        let entry = report.objectives.entries.iter().find(|e| e.name == "obj1");
        assert!(entry.is_some(), "order-only objective change should produce an entry");
        let entry = entry.unwrap();
        assert_eq!(entry.kind, DiffKind::Modified);
        assert!(entry.order_only);
        assert!(entry.order_changed);
        assert!(entry.coeff_changes.is_empty());
        assert_eq!(report.objectives.counts.order_only, 1);
    }

    /// Build a report with custom `DiffOptions`. Uses empty line maps + dummy analyses.
    fn quick_report_with_opts(p1: &LpProblem, p2: &LpProblem, options: DiffOptions) -> LpDiffReport {
        build_diff_report(&DiffInput {
            file1: "a.lp",
            file2: "b.lp",
            p1,
            p2,
            line_map1: &HashMap::new(),
            line_map2: &HashMap::new(),
            analysis1: dummy_analysis(),
            analysis2: dummy_analysis(),
            options,
        })
    }

    /// Shorthand for a rename rule. Panics on invalid regex, which is what we want in a test.
    fn rule(pat: &str, rep: &str) -> (regex::Regex, String) {
        (regex::Regex::new(pat).unwrap(), rep.to_string())
    }

    #[test]
    fn test_rename_collapses_names_across_all_sections() {
        // Same structural problem in both files, only the `[N]` index differs. A single
        // rename rule should collapse variables, constraints, and objectives simultaneously.
        let p1 = LpProblem::parse("minimize\nobj[1]: 2 x[1]\nsubject to\n c[1]: x[1] >= 0\nend").unwrap();
        let p2 = LpProblem::parse("minimize\nobj[9]: 2 x[9]\nsubject to\n c[9]: x[9] >= 0\nend").unwrap();

        let opts = DiffOptions { rename_rules: vec![rule(r"\[\d+\]$", "[idx]")], ..DiffOptions::default() };
        assert_eq!(quick_report_with_opts(&p1, &p2, opts).summary().total_changes(), 0);
    }

    #[test]
    fn test_rename_rules_apply_in_sequence() {
        // Rule 1 strips `_vN` suffix; rule 2 renames `x` → `y`.
        // Neither rule alone matches `x_v1` with `y`, so both must run — in order.
        let p1 = problem_with_variable("x_v1", &VariableType::Binary);
        let p2 = problem_with_variable("y", &VariableType::Binary);
        let opts = DiffOptions { rename_rules: vec![rule(r"_v\d+$", ""), rule("^x$", "y")], ..DiffOptions::default() };
        assert_eq!(quick_report_with_opts(&p1, &p2, opts).variables.counts.changed(), 0);
    }

    #[test]
    fn test_abs_tol_suppresses_small_rhs_diff() {
        let p1 = problem_with_standard_constraint("c1", &[("x", 1.0)], &ComparisonOp::LTE, 10.0);
        let p2 = problem_with_standard_constraint("c1", &[("x", 1.0)], &ComparisonOp::LTE, 10.005);
        let opts = DiffOptions { abs_tol: 0.01, ..DiffOptions::default() };
        assert!(quick_report_with_opts(&p1, &p2, opts).constraints.entries.is_empty());
    }

    #[test]
    fn test_rel_tol_suppresses_proportional_coeff_diff() {
        // |1001 - 1000| / 1001 ≈ 1e-3, suppressed by rel_tol = 1e-2.
        let p1 = problem_with_standard_constraint("c1", &[("x", 1000.0)], &ComparisonOp::LTE, 10.0);
        let p2 = problem_with_standard_constraint("c1", &[("x", 1001.0)], &ComparisonOp::LTE, 10.0);
        let opts = DiffOptions { rel_tol: 1e-2, ..DiffOptions::default() };
        assert!(quick_report_with_opts(&p1, &p2, opts).constraints.entries.is_empty());
    }

    #[test]
    fn test_tolerance_does_not_mask_large_change() {
        // 10 → 20 is well beyond any configured tolerance. The change must still surface,
        // and the stored detail must record the original values (not the tolerance gate).
        let p1 = problem_with_standard_constraint("c1", &[("x", 1.0)], &ComparisonOp::LTE, 10.0);
        let p2 = problem_with_standard_constraint("c1", &[("x", 1.0)], &ComparisonOp::LTE, 20.0);
        let opts = DiffOptions { abs_tol: 0.5, rel_tol: 1e-3, ..DiffOptions::default() };
        let report = quick_report_with_opts(&p1, &p2, opts);
        let ConstraintDiffDetail::Standard { rhs_change, .. } = &report.constraints.entries[0].detail else {
            panic!("expected Standard detail");
        };
        assert_eq!(*rhs_change, Some((10.0, 20.0)));
    }

    #[test]
    fn test_rename_detected_for_identical_constraint() {
        // Same structure, different constraint name — must collapse to one Renamed entry.
        let p1 = problem_with_standard_constraint("row_1", &[("x", 2.0), ("y", 3.0)], &ComparisonOp::LTE, 10.0);
        let p2 = problem_with_standard_constraint("row_9", &[("x", 2.0), ("y", 3.0)], &ComparisonOp::LTE, 10.0);
        let report = quick_report(&p1, &p2);

        assert_eq!(report.constraints.counts.renamed, 1);
        assert_eq!(report.constraints.counts.added, 0, "renamed pair must not count as added");
        assert_eq!(report.constraints.counts.removed, 0, "renamed pair must not count as removed");

        let entry = report.constraints.entries.iter().find(|e| e.kind == DiffKind::Renamed).expect("should have a Renamed entry");
        assert_eq!(entry.name, "row_9");
        assert_eq!(entry.renamed_from.as_deref(), Some("row_1"));
        // The renamed entry replaces both the added and the removed entry.
        assert!(!report.constraints.entries.iter().any(|e| matches!(e.kind, DiffKind::Added | DiffKind::Removed)));
    }

    #[test]
    fn test_rename_detected_with_reordered_coefficients() {
        // Signature is order-insensitive: 2x + 3y matches 3y + 2x.
        let p1 = problem_with_standard_constraint("c_old", &[("x", 2.0), ("y", 3.0)], &ComparisonOp::GTE, 5.0);
        let p2 = problem_with_standard_constraint("c_new", &[("y", 3.0), ("x", 2.0)], &ComparisonOp::GTE, 5.0);
        let report = quick_report(&p1, &p2);

        assert_eq!(report.constraints.counts.renamed, 1);
        let entry = report.constraints.entries.iter().find(|e| e.kind == DiffKind::Renamed).expect("should have a Renamed entry");
        let ConstraintDiffDetail::Standard { order_changed, .. } = &entry.detail else {
            panic!("renamed entry must carry a Standard detail");
        };
        assert!(order_changed, "positional order differs between the two files");
    }

    #[test]
    fn test_rename_detected_within_tolerance() {
        // Coefficient differs by 5e-3, within abs_tol = 0.01 — still a rename.
        let p1 = problem_with_standard_constraint("c_old", &[("x", 1.0)], &ComparisonOp::LTE, 10.0);
        let p2 = problem_with_standard_constraint("c_new", &[("x", 1.005)], &ComparisonOp::LTE, 10.0);
        let opts = DiffOptions { abs_tol: 0.01, ..DiffOptions::default() };
        let report = quick_report_with_opts(&p1, &p2, opts);

        assert_eq!(report.constraints.counts.renamed, 1);
        assert_eq!(report.constraints.counts.added, 0);
        assert_eq!(report.constraints.counts.removed, 0);
    }

    #[test]
    fn test_rename_not_detected_for_different_rhs() {
        // Same coefficients but RHS 10 vs 20 — must stay added + removed.
        let p1 = problem_with_standard_constraint("c_old", &[("x", 1.0)], &ComparisonOp::LTE, 10.0);
        let p2 = problem_with_standard_constraint("c_new", &[("x", 1.0)], &ComparisonOp::LTE, 20.0);
        let report = quick_report(&p1, &p2);

        assert_eq!(report.constraints.counts.renamed, 0);
        assert_eq!(report.constraints.counts.added, 1);
        assert_eq!(report.constraints.counts.removed, 1);
    }

    #[test]
    fn test_renamed_counts_in_total_and_changed() {
        let counts = DiffCounts { added: 1, removed: 2, modified: 3, unchanged: 4, order_only: 0, renamed: 5 };
        assert_eq!(counts.total(), 15);
        assert_eq!(counts.changed(), 11);
    }

    #[test]
    fn test_constraint_sort_delta_max_wins() {
        // Coefficient Δ = |5 - 2| = 3, RHS Δ = |11 - 10| = 1 → max is the coefficient.
        let p1 = problem_with_standard_constraint("c1", &[("x", 2.0)], &ComparisonOp::LTE, 10.0);
        let p2 = problem_with_standard_constraint("c1", &[("x", 5.0)], &ComparisonOp::LTE, 11.0);
        let report = quick_report(&p1, &p2);
        let entry = report.constraints.entries.iter().find(|e| e.name == "c1").expect("c1 must be modified");

        assert_eq!(constraint_sort_delta(entry, false), Some(3.0));
        // Relative: coefficient 3/5 = 0.6 beats RHS 1/11 ≈ 0.09.
        let relative = constraint_sort_delta(entry, true).expect("modified entry has a delta");
        assert!((relative - 0.6).abs() < 1e-12, "expected 0.6, got {relative}");
    }

    #[test]
    fn test_constraint_sort_delta_rhs_wins() {
        // RHS Δ = 10 beats coefficient Δ = 1.
        let p1 = problem_with_standard_constraint("c1", &[("x", 2.0)], &ComparisonOp::LTE, 10.0);
        let p2 = problem_with_standard_constraint("c1", &[("x", 3.0)], &ComparisonOp::LTE, 20.0);
        let report = quick_report(&p1, &p2);
        let entry = report.constraints.entries.iter().find(|e| e.name == "c1").expect("c1 must be modified");
        assert_eq!(constraint_sort_delta(entry, false), Some(10.0));
    }

    #[test]
    fn test_rel_delta_zero_guard() {
        assert_eq!(change_rel_delta(Some(0.0), Some(0.0)), 0.0);
        assert_eq!(change_rel_delta(None, None), 0.0);
        // Coefficient appearing from nothing: |2 - 0| / max(2, 0) = 1.
        assert_eq!(change_rel_delta(None, Some(2.0)), 1.0);
        assert_eq!(change_abs_delta(None, Some(2.0)), 2.0);
    }

    #[test]
    fn test_sort_puts_modified_before_added_and_removed() {
        let p1 = LpProblem::parse("minimize\nobj: _d\nsubject to\n c_mod: 1 x <= 10\n c_gone: 1 z <= 3\nend").unwrap();
        let p2 = LpProblem::parse("minimize\nobj: _d\nsubject to\n c_mod: 1 x <= 20\n c_new: 1 y <= 5\nend").unwrap();
        let report = quick_report(&p1, &p2);
        let entries = &report.constraints.entries;
        let mut indices: Vec<usize> = (0..entries.len()).collect();
        sort_indices_by_delta(entries, &mut indices, false);

        assert_eq!(entries[indices[0]].name, "c_mod", "modified entry with a delta sorts first");
        assert_eq!(entries[indices[0]].kind, DiffKind::Modified);
        // The remaining (no-delta) entries are alphabetical among themselves.
        let tail: Vec<&str> = indices[1..].iter().map(|&i| entries[i].name.as_str()).collect();
        let mut sorted_tail = tail.clone();
        sorted_tail.sort_unstable();
        assert_eq!(tail, sorted_tail, "no-delta entries must be alphabetical");
    }

    #[test]
    fn test_variable_sort_delta_uses_bound_changes() {
        let p1 = problem_with_variable("v", &VariableType::DoubleBound(0.0, 10.0));
        let p2 = problem_with_variable("v", &VariableType::DoubleBound(1.0, 50.0));
        let report = quick_report(&p1, &p2);
        let entry = report.variables.entries.iter().find(|e| e.name == "v").expect("v must be modified");
        // Lower Δ = 1, upper Δ = 40 → max is 40.
        assert_eq!(variable_sort_delta(entry, false), Some(40.0));
    }

    #[test]
    fn test_tolerance_preset_cycling() {
        // Preset values advance in order and wrap around.
        assert_eq!(next_tolerance_preset(0.0), 1e-9);
        assert_eq!(next_tolerance_preset(1e-9), 1e-6);
        assert_eq!(next_tolerance_preset(1e-6), 1e-4);
        assert_eq!(next_tolerance_preset(1e-4), 1e-2);
        assert_eq!(next_tolerance_preset(1e-2), 0.0);
        // A non-preset CLI value jumps to the first preset (tolerance off).
        assert_eq!(next_tolerance_preset(0.5), 0.0);
    }

    #[test]
    fn test_options_summary_round_trips_onto_report() {
        let opts = DiffOptions { abs_tol: 0.1, rel_tol: 0.01, rename_rules: vec![rule("x", "y")] };
        let summary = quick_report_with_opts(&empty_problem(), &empty_problem(), opts).options_summary;
        assert_eq!(summary.abs_tol, 0.1);
        assert_eq!(summary.rel_tol, 0.01);
        assert_eq!(summary.rename_rule_count, 1);
        assert!(!summary.is_default());
        assert_eq!(summary.to_string(), "abs_tol=0.1  rel_tol=0.01  rename_rules=1");
    }
}
