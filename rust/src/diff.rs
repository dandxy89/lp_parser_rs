//! Structural and numeric diff engine for two parsed [`LpProblem`]s.
//!
//! This module is gated behind the `diff` feature. It is the single, shared
//! definition of "how two LP problems differ" used by both the `lp_parser diff`
//! CLI subcommand and the `lp_diff` TUI: keeping the comparison logic here
//! prevents the two front-ends from drifting apart.
//!
//! # Overview
//!
//! - [`DiffTol`] carries the absolute and relative tolerances that decide when
//!   two floats count as different.
//! - [`DiffOptions`] bundles a [`DiffTol`] with an optional caller-supplied
//!   name normaliser so callers can rewrite variable/constraint/objective
//!   names (e.g. the CLI's regex `--rename` rules) *without* forcing a
//!   `regex` dependency onto this crate.
//! - [`compare`](crate::diff::compare) (or the convenience [`LpProblem::diff`] method) walks both
//!   problems and returns an [`LpDiff`] describing every added, removed, or
//!   modified variable, constraint, and objective.
//!
//! # Example
//!
//! ```rust
//! use lp_parser_rs::LpProblem;
//! use lp_parser_rs::diff::DiffOptions;
//!
//! let a = LpProblem::parse("Minimize\n obj: 2 x\nSubject To\n c1: x >= 1\nEnd")?;
//! let b = LpProblem::parse("Minimize\n obj: 3 x\nSubject To\n c1: x >= 2\nEnd")?;
//!
//! let diff = a.diff(&b, &DiffOptions::default());
//! assert_eq!(diff.cons_modified.len(), 1); // c1's rhs changed
//! assert_eq!(diff.objs_modified.len(), 1); // obj's coefficient changed
//! # Ok::<(), lp_parser_rs::LpParseError>(())
//! ```

use std::collections::{BTreeMap, BTreeSet, HashMap};

use crate::interner::NameId;
use crate::model::{Coefficient, Constraint};
use crate::problem::LpProblem;

/// A name normaliser: rewrites a name before matching.
///
/// The CLI passes a closure wrapping its regex `--rename` rules; consumers
/// that do not rename names leave [`DiffOptions::normalise`] as `None`.
pub type Normaliser<'a> = &'a dyn Fn(&str) -> String;

/// Absolute and relative tolerances for treating two floats as different.
///
/// Two values `a` and `b` differ when their absolute difference exceeds **both**
/// the absolute tolerance and the relative tolerance scaled by
/// `max(|a|, |b|)`. Equal values (or a zero difference) never differ.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DiffTol {
    /// Absolute tolerance: differences no larger than this are ignored.
    pub abs: f64,
    /// Relative tolerance: differences no larger than `rel * max(|a|, |b|)` are ignored.
    pub rel: f64,
}

impl Default for DiffTol {
    /// Both tolerances zero: any non-zero difference counts as a change.
    fn default() -> Self {
        Self { abs: 0.0, rel: 0.0 }
    }
}

impl DiffTol {
    /// Return true if `a` and `b` differ beyond both tolerances.
    #[must_use]
    pub fn differ(self, a: f64, b: f64) -> bool {
        debug_assert!(self.abs.is_finite() && self.abs >= 0.0, "abs tolerance must be finite and non-negative");
        debug_assert!(self.rel.is_finite() && self.rel >= 0.0, "rel tolerance must be finite and non-negative");
        if a.is_nan() || b.is_nan() {
            // A value that became NaN is a change; NaN on both sides is not.
            return a.is_nan() != b.is_nan();
        }
        let diff = (a - b).abs();
        if diff == 0.0 {
            return false;
        }
        let scale = a.abs().max(b.abs());
        diff > self.abs && diff > self.rel * scale
    }
}

/// Comparison options shared by a whole diff: numeric tolerances plus a
/// caller-supplied name normaliser applied to every name in both problems.
///
/// The normaliser lets callers rewrite names (e.g. to strip volatile row/column
/// indices) before matching, without this crate depending on `regex`.
#[derive(Default)]
pub struct DiffOptions<'a> {
    /// Numeric tolerances for RHS and coefficient comparisons.
    pub tol: DiffTol,
    /// Name rewrite applied to every variable, constraint, and objective name;
    /// `None` compares names as-is.
    pub normalise: Option<Normaliser<'a>>,
}

/// The computed differences between two LP problems, keyed by canonical
/// (normalised) name.
///
/// Names in every field are already normalised through
/// [`DiffOptions::normalise`]. The `*_modified` fields pair a name with a list
/// of human-readable change descriptions.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LpDiff {
    /// Variables present only in the second problem.
    pub vars_added: Vec<String>,
    /// Variables present only in the first problem.
    pub vars_removed: Vec<String>,
    /// Variables whose type changed: `(name, old_type, new_type)`.
    pub vars_type_changed: Vec<(String, String, String)>,
    /// Constraints present only in the second problem.
    pub cons_added: Vec<String>,
    /// Constraints present only in the first problem.
    pub cons_removed: Vec<String>,
    /// Constraints present in both but changed: `(name, changes)`.
    pub cons_modified: Vec<(String, Vec<String>)>,
    /// Objectives present only in the second problem.
    pub objs_added: Vec<String>,
    /// Objectives present only in the first problem.
    pub objs_removed: Vec<String>,
    /// Objectives present in both but changed: `(name, changes)`.
    pub objs_modified: Vec<(String, Vec<String>)>,
}

impl LpDiff {
    /// Returns `true` when the two problems are identical under the diff
    /// options used, i.e. no additions, removals, or modifications were found.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.vars_added.is_empty()
            && self.vars_removed.is_empty()
            && self.vars_type_changed.is_empty()
            && self.cons_added.is_empty()
            && self.cons_removed.is_empty()
            && self.cons_modified.is_empty()
            && self.objs_added.is_empty()
            && self.objs_removed.is_empty()
            && self.objs_modified.is_empty()
    }
}

impl LpProblem {
    /// Compare this problem against `other`, returning the structural and numeric diff.
    ///
    /// Convenience method equivalent to [`compare(self, other, options)`](compare).
    #[must_use]
    pub fn diff(&self, other: &LpProblem, options: &DiffOptions) -> LpDiff {
        compare(self, other, options)
    }
}

/// Build a coefficient map keyed by canonical (normalised) variable name.
fn coeff_map(problem: &LpProblem, coeffs: &[Coefficient], normalise: Normaliser) -> BTreeMap<String, f64> {
    coeffs.iter().map(|c| (normalise(problem.resolve(c.name)), c.value)).collect()
}

/// Count coefficients that changed value, were removed, or were added.
fn count_coeff_diffs(m1: &BTreeMap<String, f64>, m2: &BTreeMap<String, f64>, tol: DiffTol) -> usize {
    let mut diffs = 0usize;
    for (k, v1) in m1 {
        match m2.get(k) {
            Some(v2) if tol.differ(*v1, *v2) => diffs += 1,
            None => diffs += 1,
            _ => {}
        }
    }
    diffs += m2.keys().filter(|k| !m1.contains_key(*k)).count();
    diffs
}

/// Describe how each common constraint changed (operator, rhs, coefficients).
fn diff_modified_constraints(
    p1: &LpProblem,
    p2: &LpProblem,
    ccons1: &HashMap<String, NameId>,
    ccons2: &HashMap<String, NameId>,
    common: &[String],
    normalise: Normaliser,
    tol: DiffTol,
) -> Vec<(String, Vec<String>)> {
    let mut modified = Vec::new();
    for name in common {
        let c1 = &p1.constraints[&ccons1[name]];
        let c2 = &p2.constraints[&ccons2[name]];
        let mut changes = Vec::new();
        match (c1, c2) {
            (
                Constraint::Standard { coefficients: cf1, operator: op1, rhs: r1, .. },
                Constraint::Standard { coefficients: cf2, operator: op2, rhs: r2, .. },
            ) => {
                if op1 != op2 {
                    changes.push(format!("operator {op1} -> {op2}"));
                }
                if tol.differ(*r1, *r2) {
                    changes.push(format!("rhs {r1} -> {r2}"));
                }
                let coef_diffs = count_coeff_diffs(&coeff_map(p1, cf1, normalise), &coeff_map(p2, cf2, normalise), tol);
                if coef_diffs > 0 {
                    changes.push(format!("{coef_diffs} coefficient change(s)"));
                }
            }
            (Constraint::SOS { .. }, Constraint::SOS { .. }) => {
                if c1 != c2 {
                    changes.push("SOS definition changed".to_string());
                }
            }
            _ => changes.push("constraint kind changed (Standard <-> SOS)".to_string()),
        }
        if !changes.is_empty() {
            modified.push((name.clone(), changes));
        }
    }
    modified
}

/// Describe how each common objective's coefficients changed.
fn diff_modified_objectives(
    p1: &LpProblem,
    p2: &LpProblem,
    cobjs1: &HashMap<String, NameId>,
    cobjs2: &HashMap<String, NameId>,
    common: &[String],
    normalise: Normaliser,
    tol: DiffTol,
) -> Vec<(String, Vec<String>)> {
    let mut modified = Vec::new();
    for name in common {
        let o1 = &p1.objectives[&cobjs1[name]];
        let o2 = &p2.objectives[&cobjs2[name]];
        let coef_diffs = count_coeff_diffs(&coeff_map(p1, &o1.coefficients, normalise), &coeff_map(p2, &o2.coefficients, normalise), tol);
        let mut changes = Vec::new();
        if coef_diffs > 0 {
            changes.push(format!("{coef_diffs} coefficient change(s)"));
        }
        if tol.differ(o1.constant, o2.constant) {
            changes.push(format!("constant: {} -> {}", o1.constant, o2.constant));
        }
        if !changes.is_empty() {
            modified.push((name.clone(), changes));
        }
    }
    modified
}

/// Compare two parsed problems, returning the structural and numeric diff.
///
/// Every name is normalised through `options.normalise` before matching, so
/// renamed-but-equal entries collapse together. Numeric comparisons respect
/// `options.tol`. The result's list fields are ordered deterministically
/// (sorted by canonical name) so callers can render stable output.
#[must_use]
// The paired 1/2-suffixed bindings are the domain language of a two-file diff.
#[allow(clippy::similar_names)]
pub fn compare(p1: &LpProblem, p2: &LpProblem, options: &DiffOptions) -> LpDiff {
    let identity = |name: &str| name.to_string();
    let normalise: Normaliser = options.normalise.unwrap_or(&identity);
    let tol = options.tol;

    let canon = |problem: &LpProblem, ids: Vec<NameId>| -> HashMap<String, NameId> {
        ids.iter().map(|id| (normalise(problem.resolve(*id)), *id)).collect()
    };

    let cvars1 = canon(p1, p1.variables.keys().copied().collect());
    let cvars2 = canon(p2, p2.variables.keys().copied().collect());
    let ccons1: HashMap<String, NameId> = p1.constraints.values().map(|c| (normalise(p1.resolve(c.name())), c.name())).collect();
    let ccons2: HashMap<String, NameId> = p2.constraints.values().map(|c| (normalise(p2.resolve(c.name())), c.name())).collect();
    let cobjs1 = canon(p1, p1.objectives.keys().copied().collect());
    let cobjs2 = canon(p2, p2.objectives.keys().copied().collect());

    let set_of = |m: &HashMap<String, NameId>| -> BTreeSet<String> { m.keys().cloned().collect() };
    let vars1 = set_of(&cvars1);
    let vars2 = set_of(&cvars2);
    let cons1 = set_of(&ccons1);
    let cons2 = set_of(&ccons2);
    let objs1 = set_of(&cobjs1);
    let objs2 = set_of(&cobjs2);

    // Sorted intersections keep modified-section output deterministic.
    let cons_common: Vec<String> = cons1.intersection(&cons2).cloned().collect();
    let objs_common: Vec<String> = objs1.intersection(&objs2).cloned().collect();

    let mut vars_type_changed = Vec::new();
    for name in vars1.intersection(&vars2) {
        let v1 = &p1.variables[&cvars1[name]];
        let v2 = &p2.variables[&cvars2[name]];
        if v1.kind != v2.kind || v1.bounds != v2.bounds {
            vars_type_changed.push((name.clone(), format!("{:?}/{}", v1.kind, v1.bounds), format!("{:?}/{}", v2.kind, v2.bounds)));
        }
    }

    LpDiff {
        vars_added: vars2.difference(&vars1).cloned().collect(),
        vars_removed: vars1.difference(&vars2).cloned().collect(),
        vars_type_changed,
        cons_added: cons2.difference(&cons1).cloned().collect(),
        cons_removed: cons1.difference(&cons2).cloned().collect(),
        cons_modified: diff_modified_constraints(p1, p2, &ccons1, &ccons2, &cons_common, normalise, tol),
        objs_added: objs2.difference(&objs1).cloned().collect(),
        objs_removed: objs1.difference(&objs2).cloned().collect(),
        objs_modified: diff_modified_objectives(p1, p2, &cobjs1, &cobjs2, &objs_common, normalise, tol),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Strip a trailing `_<digits>` suffix from a name (a volatile index).
    fn strip_index_suffix(name: &str) -> String {
        match name.rfind('_') {
            Some(idx) if idx + 1 < name.len() && name[idx + 1..].chars().all(|ch| ch.is_ascii_digit()) => name[..idx].to_string(),
            _ => name.to_string(),
        }
    }

    fn opts(tol: DiffTol) -> DiffOptions<'static> {
        DiffOptions { tol, normalise: None }
    }

    #[test]
    fn tol_zero_reports_any_nonzero_difference() {
        let tol = DiffTol::default();
        assert!(!tol.differ(1.0, 1.0));
        assert!(tol.differ(1.0, 1.0 + 1e-12));
    }

    #[test]
    fn tol_equal_within_absolute() {
        let tol = DiffTol { abs: 0.5, rel: 0.0 };
        // Difference of 0.4 is within abs=0.5.
        assert!(!tol.differ(1.0, 1.4));
        // Difference of 0.6 exceeds abs=0.5.
        assert!(tol.differ(1.0, 1.6));
    }

    #[test]
    fn tol_relative_scales_with_magnitude() {
        // 1% relative tolerance.
        let tol = DiffTol { abs: 0.0, rel: 0.01 };
        // 0.5% change is within tolerance.
        assert!(!tol.differ(1000.0, 1005.0));
        // 2% change exceeds tolerance.
        assert!(tol.differ(1000.0, 1020.0));
    }

    #[test]
    fn tol_requires_both_tolerances_exceeded() {
        // Differs only if BEYOND both abs AND rel.
        let tol = DiffTol { abs: 10.0, rel: 0.5 };
        // diff 8: below abs(10) -> not different even though 8 > 0.5*10.
        assert!(!tol.differ(10.0, 18.0));
        // diff 12: above abs(10) but 12 < 0.5*24 -> not different.
        assert!(!tol.differ(12.0, 24.0));
    }

    #[test]
    fn tol_nan_operands() {
        let tol = DiffTol::default();
        // A value that became NaN must be reported as changed.
        assert!(tol.differ(f64::NAN, 1.0));
        assert!(tol.differ(1.0, f64::NAN));
        // NaN on both sides means nothing changed.
        assert!(!tol.differ(f64::NAN, f64::NAN));
    }

    #[test]
    fn tol_zero_baseline() {
        let tol = DiffTol { abs: 0.0, rel: 0.5 };
        // Relative scale is max(|0|, |1|) = 1; diff 1 > 0.5 -> different.
        assert!(tol.differ(0.0, 1.0));
        // Both zero -> no difference.
        assert!(!tol.differ(0.0, 0.0));
    }

    fn parse(src: &str) -> LpProblem {
        LpProblem::parse(src).expect("test LP must parse")
    }

    #[test]
    fn detects_added_and_removed_variables() {
        let p1 = parse("Minimize\n obj: 2 x + 3 y\nSubject To\n c1: x + y >= 1\nEnd");
        let p2 = parse("Minimize\n obj: 2 x + 3 z\nSubject To\n c1: x + z >= 1\nEnd");
        let diff = p1.diff(&p2, &opts(DiffTol::default()));
        assert_eq!(diff.vars_added, vec!["z".to_string()]);
        assert_eq!(diff.vars_removed, vec!["y".to_string()]);
    }

    #[test]
    fn detects_variable_type_change() {
        // `x` is continuous in p1, declared integer in p2.
        let p1 = parse("Minimize\n obj: x\nSubject To\n c1: x >= 1\nEnd");
        let p2 = parse("Minimize\n obj: x\nSubject To\n c1: x >= 1\nintegers\n x\nEnd");
        let diff = p1.diff(&p2, &opts(DiffTol::default()));
        assert_eq!(diff.vars_type_changed.len(), 1);
        assert_eq!(diff.vars_type_changed[0].0, "x");
    }

    #[test]
    fn detects_added_and_removed_constraints() {
        let p1 = parse("Minimize\n obj: x + y\nSubject To\n c1: x + y >= 1\nEnd");
        let p2 = parse("Minimize\n obj: x + y\nSubject To\n c2: x + y >= 1\nEnd");
        let diff = p1.diff(&p2, &opts(DiffTol::default()));
        assert_eq!(diff.cons_added, vec!["c2".to_string()]);
        assert_eq!(diff.cons_removed, vec!["c1".to_string()]);
    }

    #[test]
    fn detects_modified_constraint_operator_rhs_and_coefficients() {
        let p1 = parse("Minimize\n obj: x + y\nSubject To\n c1: x + y >= 1\nEnd");
        let p2 = parse("Minimize\n obj: x + y\nSubject To\n c1: 2 x + y <= 5\nEnd");
        let diff = p1.diff(&p2, &opts(DiffTol::default()));
        assert_eq!(diff.cons_modified.len(), 1);
        let (name, changes) = &diff.cons_modified[0];
        assert_eq!(name, "c1");
        assert!(changes.iter().any(|c| c.contains("operator")));
        assert!(changes.iter().any(|c| c.contains("rhs")));
        assert!(changes.iter().any(|c| c.contains("coefficient change")));
    }

    #[test]
    fn detects_added_removed_and_modified_objectives() {
        let p1 = parse("Minimize\n obj: 2 x + 3 y\nSubject To\n c1: x + y >= 1\nEnd");
        let p2 = parse("Minimize\n obj: 5 x + 3 y\nSubject To\n c1: x + y >= 1\nEnd");
        let diff = p1.diff(&p2, &opts(DiffTol::default()));
        assert_eq!(diff.objs_modified.len(), 1);
        assert_eq!(diff.objs_modified[0].0, "obj");

        // Second objective `obj2` present in one problem, absent from the other.
        let single = parse("Minimize\n obj: 2 x + 3 y\nSubject To\n c1: x + y >= 1\nEnd");
        let double = parse("Minimize\n obj: 2 x + 3 y\n obj2: x\nSubject To\n c1: x + y >= 1\nEnd");

        let diff = single.diff(&double, &opts(DiffTol::default()));
        assert_eq!(diff.objs_added, vec!["obj2".to_string()]);
        assert!(diff.objs_removed.is_empty());

        let diff = double.diff(&single, &opts(DiffTol::default()));
        assert_eq!(diff.objs_removed, vec!["obj2".to_string()]);
        assert!(diff.objs_added.is_empty());
    }

    #[test]
    fn rhs_change_within_tolerance_is_ignored() {
        let p1 = parse("Minimize\n obj: x\nSubject To\n c1: x >= 100\nEnd");
        let p2 = parse("Minimize\n obj: x\nSubject To\n c1: x >= 100.4\nEnd");
        // abs tolerance 0.5 suppresses the 0.4 rhs change.
        let diff = p1.diff(&p2, &opts(DiffTol { abs: 0.5, rel: 0.0 }));
        assert!(diff.cons_modified.is_empty());
        // Without tolerance the change is reported.
        let diff = p1.diff(&p2, &opts(DiffTol::default()));
        assert_eq!(diff.cons_modified.len(), 1);
    }

    #[test]
    fn normaliser_applied_on_both_sides() {
        // Names carry a volatile numeric suffix on each side.
        let p1 = parse("Minimize\n obj: x_1\nSubject To\n c_1: x_1 >= 1\nEnd");
        let p2 = parse("Minimize\n obj: x_2\nSubject To\n c_2: x_2 >= 1\nEnd");

        // Without normalisation, everything looks added/removed.
        let diff = p1.diff(&p2, &opts(DiffTol::default()));
        assert_eq!(diff.vars_added, vec!["x_2".to_string()]);
        assert_eq!(diff.vars_removed, vec!["x_1".to_string()]);

        // Strip the trailing `_<digits>` on both sides: names now match.
        let options = DiffOptions { tol: DiffTol::default(), normalise: Some(&strip_index_suffix) };
        let diff = p1.diff(&p2, &options);
        assert!(diff.vars_added.is_empty());
        assert!(diff.vars_removed.is_empty());
        assert!(diff.cons_added.is_empty());
        assert!(diff.cons_removed.is_empty());
        // Constraint c is unchanged after normalisation.
        assert!(diff.cons_modified.is_empty());
    }

    #[test]
    fn detects_sos_weight_change() {
        // Same SOS constraint on both sides, but one weight differs.
        let p1 = parse("Minimize\n obj: x + y\nSubject To\n c1: x + y >= 1\nSOS\n sos_a: S1:: x:1 y:2\nEnd");
        let p2 = parse("Minimize\n obj: x + y\nSubject To\n c1: x + y >= 1\nSOS\n sos_a: S1:: x:1 y:3\nEnd");
        let diff = p1.diff(&p2, &opts(DiffTol::default()));
        assert_eq!(diff.cons_modified.len(), 1);
        let (name, changes) = &diff.cons_modified[0];
        assert_eq!(name, "sos_a");
        assert_eq!(changes, &vec!["SOS definition changed".to_string()]);
    }

    #[test]
    fn detects_constraint_kind_change() {
        // `mix` is a standard constraint in p1 and an SOS constraint in p2.
        let p1 = parse("Minimize\n obj: x + y\nSubject To\n c1: x + y >= 1\n mix: x + y <= 5\nEnd");
        let p2 = parse("Minimize\n obj: x + y\nSubject To\n c1: x + y >= 1\nSOS\n mix: S1:: x:1 y:2\nEnd");
        let diff = p1.diff(&p2, &opts(DiffTol::default()));
        assert_eq!(diff.cons_modified.len(), 1);
        let (name, changes) = &diff.cons_modified[0];
        assert_eq!(name, "mix");
        assert_eq!(changes, &vec!["constraint kind changed (Standard <-> SOS)".to_string()]);
    }

    #[test]
    fn identical_problems_produce_empty_diff() {
        let src = "Minimize\n obj: 2 x + 3 y\nSubject To\n c1: x + y >= 1\nBounds\n x <= 4\nSOS\n sos_a: S1:: x:1 y:2\nEnd";
        let p1 = parse(src);
        let p2 = parse(src);
        let diff = p1.diff(&p2, &opts(DiffTol::default()));
        assert!(diff.is_empty(), "identical problems must diff empty: {diff:?}");
    }

    #[test]
    fn objective_coefficient_additions_and_removals_are_counted() {
        // `y` is removed from the objective and `z` is added: two coefficient
        // changes even though `x`'s value is untouched.
        let p1 = parse("Minimize\n obj: 2 x + 3 y\nSubject To\n c1: x >= 1\nEnd");
        let p2 = parse("Minimize\n obj: 2 x + 4 z\nSubject To\n c1: x >= 1\nEnd");
        let diff = p1.diff(&p2, &opts(DiffTol::default()));
        assert_eq!(diff.objs_modified.len(), 1);
        let (name, changes) = &diff.objs_modified[0];
        assert_eq!(name, "obj");
        assert_eq!(changes, &vec!["2 coefficient change(s)".to_string()]);
    }
}
