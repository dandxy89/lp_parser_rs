//! Structural and numeric diff engine for two parsed [`LpProblem`]s.
//!
//! This module is gated behind the `diff` feature. It backs the `lp_parser diff`
//! CLI subcommand and is available to library users. The `lp_diff` TUI builds
//! its own, more detailed per-coefficient diff, but uses [`DiffTol`] from here
//! so that both front-ends agree on when two numbers differ.
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

use std::borrow::Cow;
use std::hash::Hash;

use rustc_hash::FxHashMap;

use crate::error::{LpParseError, LpResult};
use crate::interner::NameId;
use crate::model::{Coefficient, Constraint, GeneralFunction, QuadraticTerm, Variable};
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
/// Infinities match only an equal infinity, and `NaN` differs from any
/// non-`NaN` value but not from another `NaN`.
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
    /// Build a tolerance pair, validating both values.
    ///
    /// # Errors
    ///
    /// Returns a validation error if either tolerance is negative, `NaN` or
    /// infinite. [`DiffTol::differ`] relies on this invariant.
    pub fn new(abs: f64, rel: f64) -> LpResult<Self> {
        for (label, value) in [("absolute", abs), ("relative", rel)] {
            if !(value.is_finite() && value >= 0.0) {
                return Err(LpParseError::validation_error(format!("{label} tolerance must be finite and non-negative, got {value}")));
            }
        }
        Ok(Self { abs, rel })
    }

    /// Return true if `a` and `b` differ beyond both tolerances.
    #[must_use]
    pub fn differ(self, a: f64, b: f64) -> bool {
        debug_assert!(self.abs.is_finite() && self.abs >= 0.0, "abs tolerance must be finite and non-negative");
        debug_assert!(self.rel.is_finite() && self.rel >= 0.0, "rel tolerance must be finite and non-negative");
        if a.is_nan() || b.is_nan() {
            // A value that became NaN is a change; NaN on both sides is not.
            return a.is_nan() != b.is_nan();
        }
        if a.is_infinite() || b.is_infinite() {
            // `rel * inf` is NaN and `inf - inf` is NaN, so the tolerance
            // arithmetic below cannot judge infinities: only equal ones match.
            #[allow(clippy::float_cmp)]
            return a != b;
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
    /// `None` compares names as-is. If two names in one problem rewrite to the
    /// same name, the later entry is the one compared.
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
    /// The optimisation sense, `(old, new)`, when it differs between the problems.
    pub sense_changed: Option<(String, String)>,
    /// Variables present only in the second problem.
    pub vars_added: Vec<String>,
    /// Variables present only in the first problem.
    pub vars_removed: Vec<String>,
    /// Variables whose kind or effective bounds changed:
    /// `(name, old, new)`, each side formatted as `Kind/bounds`.
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
    pub const fn is_empty(&self) -> bool {
        self.sense_changed.is_none()
            && self.vars_added.is_empty()
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

/// Maps a name to its canonical form: through the caller's normaliser when
/// one is set, otherwise the name itself, borrowed without allocating.
#[derive(Clone, Copy)]
struct Canon<'a>(Option<Normaliser<'a>>);

impl Canon<'_> {
    fn name(self, name: &str) -> Cow<'_, str> {
        match self.0 {
            Some(normalise) => Cow::Owned(normalise(name)),
            None => Cow::Borrowed(name),
        }
    }
}

/// Canonical name -> position in the owning `IndexMap`. A later entry with
/// the same canonical name replaces an earlier one.
type CanonIndex<'p> = FxHashMap<Cow<'p, str>, usize>;

/// Index the names yielded by `names` (in `IndexMap` order) by canonical name.
fn canon_index<'p>(problem: &'p LpProblem, names: impl Iterator<Item = NameId>, canon: Canon) -> CanonIndex<'p> {
    names.enumerate().map(|(position, id)| (canon.name(problem.resolve(id)), position)).collect()
}

/// Canonical names split by which side has them, each list sorted (as a
/// `BTreeSet` would order them), borrowed from the indexes' keys.
struct NameSplit<'n> {
    /// Names only in the first problem.
    removed: Vec<&'n str>,
    /// Names in both problems.
    common: Vec<&'n str>,
    /// Names only in the second problem.
    added: Vec<&'n str>,
}

impl<'n> NameSplit<'n> {
    /// Split the keys of `old` and `new`. Each name is sorted once, in the
    /// list it belongs to, rather than sorting both sides whole.
    fn new(old: &'n CanonIndex<'_>, new: &'n CanonIndex<'_>) -> Self {
        let mut removed = Vec::new();
        let mut common = Vec::with_capacity(old.len().min(new.len()));
        for name in old.keys() {
            if new.contains_key(name) { common.push(name.as_ref()) } else { removed.push(name.as_ref()) }
        }
        let mut added: Vec<&str> = new.keys().filter(|name| !old.contains_key(*name)).map(AsRef::as_ref).collect();
        // Keys are unique, so an unstable sort gives the one sorted order.
        removed.sort_unstable();
        common.sort_unstable();
        added.sort_unstable();
        debug_assert!(removed.len() + common.len() == old.len() && added.len() + common.len() == new.len(), "every name lands in one list");
        Self { removed, common, added }
    }
}

/// Owned copies of `names`, for the result.
fn owned(names: &[&str]) -> Vec<String> {
    names.iter().map(|name| (*name).to_string()).collect()
}

/// Counts linear coefficient differences between rows of `p1` and `p2`,
/// reusing its buffers across rows.
struct LinearComparer<'p1, 'p2, 'c> {
    p1: &'p1 LpProblem,
    p2: &'p2 LpProblem,
    canon: Canon<'c>,
    tol: DiffTol,
    old: Vec<(Cow<'p1, str>, f64)>,
    new: Vec<(Cow<'p2, str>, f64)>,
}

impl<'p1, 'p2, 'c> LinearComparer<'p1, 'p2, 'c> {
    const fn new(p1: &'p1 LpProblem, p2: &'p2 LpProblem, canon: Canon<'c>, tol: DiffTol) -> Self {
        Self { p1, p2, canon, tol, old: Vec::new(), new: Vec::new() }
    }

    /// Count coefficients that changed value, were removed, or were added,
    /// matching them by canonical variable name. A later coefficient on the
    /// same canonical name replaces an earlier one.
    ///
    /// Equivalent to building a name -> value map of each side and comparing
    /// the maps, but sorts two reused buffers and merges them instead.
    fn count(&mut self, old: &[Coefficient], new: &[Coefficient]) -> usize {
        if let Some(diffs) = self.count_aligned(old, new) {
            return diffs;
        }
        sorted_by_name(&mut self.old, self.p1, old, self.canon);
        sorted_by_name(&mut self.new, self.p2, new, self.canon);
        let (a, b) = (&self.old, &self.new);
        let (mut i, mut j, mut diffs) = (0, 0, 0);
        while i < a.len() && j < b.len() {
            match a[i].0.cmp(&b[j].0) {
                std::cmp::Ordering::Less => {
                    diffs += 1;
                    i += 1;
                }
                std::cmp::Ordering::Greater => {
                    diffs += 1;
                    j += 1;
                }
                std::cmp::Ordering::Equal => {
                    if self.tol.differ(a[i].1, b[j].1) {
                        diffs += 1;
                    }
                    i += 1;
                    j += 1;
                }
            }
        }
        diffs + (a.len() - i) + (b.len() - j)
    }

    /// Fast path for the common case of a row whose variables are listed in
    /// the same order on both sides: count the changed values position by
    /// position. Returns `None` when that would not match [`Self::count`]:
    /// with a normaliser (distinct names may merge), when the names differ
    /// in order or number, or when a name repeats (only its last value
    /// counts). Without a normaliser, equal names within one problem are
    /// equal ids, so repeats are found by comparing ids.
    fn count_aligned(&self, old: &[Coefficient], new: &[Coefficient]) -> Option<usize> {
        const MAX_ALIGNED_LEN: usize = 16;
        if self.canon.0.is_some() || old.len() != new.len() || old.len() > MAX_ALIGNED_LEN {
            return None;
        }
        let aligned = old.iter().zip(new).all(|(a, b)| self.p1.resolve(a.name) == self.p2.resolve(b.name));
        let repeats = old.iter().enumerate().any(|(i, a)| old[..i].iter().any(|earlier| earlier.name == a.name));
        (aligned && !repeats).then(|| old.iter().zip(new).filter(|(a, b)| self.tol.differ(a.value, b.value)).count())
    }
}

/// Fill `buffer` with `coeffs` keyed by canonical name, sorted by name with
/// one entry per name holding the value of its last occurrence.
fn sorted_by_name<'p>(buffer: &mut Vec<(Cow<'p, str>, f64)>, problem: &'p LpProblem, coeffs: &[Coefficient], canon: Canon) {
    buffer.clear();
    buffer.extend(coeffs.iter().map(|c| (canon.name(problem.resolve(c.name)), c.value)));
    // Stable, so repeats of a name stay in their original order...
    buffer.sort_by(|x, y| x.0.cmp(&y.0));
    // ...and the first of each run, which is kept, takes the last value.
    buffer.dedup_by(|later, kept| {
        if later.0 == kept.0 {
            kept.1 = later.1;
            true
        } else {
            false
        }
    });
    debug_assert!(buffer.windows(2).all(|w| w[0].0 < w[1].0), "names must be strictly increasing after dedup");
}

/// Build a quadratic-term map keyed by the canonical (normalised, sorted)
/// variable pair, so `x * y` and `y * x` match. Repeated pairs are summed.
fn quad_map(problem: &LpProblem, terms: &[QuadraticTerm], canon: Canon) -> FxHashMap<String, f64> {
    let mut map = FxHashMap::default();
    for term in terms {
        let (a, b) = (canon.name(problem.resolve(term.var1)), canon.name(problem.resolve(term.var2)));
        let key = if a <= b { format!("{a}*{b}") } else { format!("{b}*{a}") };
        *map.entry(key).or_insert(0.0) += term.coefficient;
    }
    map
}

/// Count coefficients that changed value, were removed, or were added.
fn count_coeff_diffs<K: Hash + Eq>(m1: &FxHashMap<K, f64>, m2: &FxHashMap<K, f64>, tol: DiffTol) -> usize {
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
    ccons1: &CanonIndex<'_>,
    ccons2: &CanonIndex<'_>,
    common: &[&str],
    canon: Canon,
    tol: DiffTol,
) -> Vec<(String, Vec<String>)> {
    let mut modified = Vec::new();
    let mut linear = LinearComparer::new(p1, p2, canon, tol);
    for &name in common {
        let (id1, c1) = p1.constraints.get_index(ccons1[name]).expect("canonical index holds positions of existing constraints");
        let (id2, c2) = p2.constraints.get_index(ccons2[name]).expect("canonical index holds positions of existing constraints");
        debug_assert!(*id1 == c1.name() && *id2 == c2.name(), "constraints are keyed by their own name");
        let mut changes = Vec::new();
        let (class1, class2) = (p1.constraint_class(c1.name()), p2.constraint_class(c2.name()));
        if class1 != class2 {
            changes.push(format!("class {class1} -> {class2}"));
        }
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
                let coef_diffs = linear.count(cf1, cf2);
                if coef_diffs > 0 {
                    changes.push(format!("{coef_diffs} coefficient change(s)"));
                }
            }
            (Constraint::SOS { sos_type: t1, weights: w1, .. }, Constraint::SOS { sos_type: t2, weights: w2, .. }) => {
                // Compare by resolved (normalised) member name: the two problems'
                // NameIds come from different interners, and an SOS set's order is
                // given by its weights, not by the order the members are listed in.
                if t1 != t2 || linear.count(w1, w2) > 0 {
                    changes.push("SOS definition changed".to_string());
                }
            }
            (
                Constraint::Indicator { variable: v1, active_value: a1, coefficients: cf1, operator: op1, rhs: r1, .. },
                Constraint::Indicator { variable: v2, active_value: a2, coefficients: cf2, operator: op2, rhs: r2, .. },
            ) => {
                let (var1, var2) = (canon.name(p1.resolve(*v1)), canon.name(p2.resolve(*v2)));
                if var1 != var2 || a1 != a2 {
                    changes.push(format!("indicator {var1} = {} -> {var2} = {}", u8::from(*a1), u8::from(*a2)));
                }
                if op1 != op2 {
                    changes.push(format!("operator {op1} -> {op2}"));
                }
                if tol.differ(*r1, *r2) {
                    changes.push(format!("rhs {r1} -> {r2}"));
                }
                let coef_diffs = linear.count(cf1, cf2);
                if coef_diffs > 0 {
                    changes.push(format!("{coef_diffs} coefficient change(s)"));
                }
            }
            (
                Constraint::Quadratic { coefficients: cf1, quadratic: q1, operator: op1, rhs: r1, .. },
                Constraint::Quadratic { coefficients: cf2, quadratic: q2, operator: op2, rhs: r2, .. },
            ) => {
                if op1 != op2 {
                    changes.push(format!("operator {op1} -> {op2}"));
                }
                if tol.differ(*r1, *r2) {
                    changes.push(format!("rhs {r1} -> {r2}"));
                }
                let coef_diffs = linear.count(cf1, cf2);
                if coef_diffs > 0 {
                    changes.push(format!("{coef_diffs} coefficient change(s)"));
                }
                let quad_diffs = count_coeff_diffs(&quad_map(p1, q1, canon), &quad_map(p2, q2, canon), tol);
                if quad_diffs > 0 {
                    changes.push(format!("{quad_diffs} quadratic term change(s)"));
                }
            }
            (Constraint::General { resultant: r1, function: f1, .. }, Constraint::General { resultant: r2, function: f2, .. }) => {
                changes.extend(general_constraint_change((p1, *r1, f1), (p2, *r2, f2), canon, tol));
            }
            _ => changes.push(format!("constraint kind changed ({} <-> {})", constraint_kind(c1), constraint_kind(c2))),
        }
        if !changes.is_empty() {
            modified.push((name.to_string(), changes));
        }
    }
    modified
}

/// A Gurobi general constraint as `(problem, resultant, function)`.
type GeneralSide<'a> = (&'a LpProblem, NameId, &'a GeneralFunction);

/// Describe a change between two general constraints, if there is one.
fn general_constraint_change(old: GeneralSide<'_>, new: GeneralSide<'_>, canon: Canon, tol: DiffTol) -> Option<String> {
    let describe = |(p, resultant, function): GeneralSide<'_>| {
        let args: Vec<String> = function.variables().iter().map(|v| canon.name(p.resolve(*v)).into_owned()).collect();
        let constant = function.constant().map_or_else(String::new, |c| format!(", {c}"));
        format!("{} = {} ({}{constant})", canon.name(p.resolve(resultant)), function.keyword(), args.join(", "))
    };
    let constants_differ = match (old.2.constant(), new.2.constant()) {
        (Some(a), Some(b)) => tol.differ(a, b),
        (a, b) => a.is_some() != b.is_some(),
    };
    let names = |(p, resultant, function): GeneralSide<'_>| -> Vec<String> {
        std::iter::once(&resultant).chain(function.variables()).map(|v| canon.name(p.resolve(*v)).into_owned()).collect()
    };
    let structure_differs = old.2.keyword() != new.2.keyword() || names(old) != names(new);
    (structure_differs || constants_differ).then(|| format!("general constraint {} -> {}", describe(old), describe(new)))
}

/// Short name of a constraint's kind, for "kind changed" descriptions.
const fn constraint_kind(constraint: &Constraint) -> &'static str {
    match constraint {
        Constraint::Standard { .. } => "Standard",
        Constraint::SOS { .. } => "SOS",
        Constraint::Indicator { .. } => "Indicator",
        Constraint::Quadratic { .. } => "Quadratic",
        Constraint::General { .. } => "General",
    }
}

/// Describe how each common objective's coefficients changed.
fn diff_modified_objectives(
    p1: &LpProblem,
    p2: &LpProblem,
    cobjs1: &CanonIndex<'_>,
    cobjs2: &CanonIndex<'_>,
    common: &[&str],
    canon: Canon,
    tol: DiffTol,
) -> Vec<(String, Vec<String>)> {
    let mut modified = Vec::new();
    let mut linear = LinearComparer::new(p1, p2, canon, tol);
    for &name in common {
        let (_, o1) = p1.objectives.get_index(cobjs1[name]).expect("canonical index holds positions of existing objectives");
        let (_, o2) = p2.objectives.get_index(cobjs2[name]).expect("canonical index holds positions of existing objectives");
        let coef_diffs = linear.count(&o1.coefficients, &o2.coefficients);
        let mut changes = Vec::new();
        if coef_diffs > 0 {
            changes.push(format!("{coef_diffs} coefficient change(s)"));
        }
        if tol.differ(o1.constant, o2.constant) {
            changes.push(format!("constant: {} -> {}", o1.constant, o2.constant));
        }
        let quad_diffs = count_coeff_diffs(&quad_map(p1, &o1.quadratic, canon), &quad_map(p2, &o2.quadratic, canon), tol);
        if quad_diffs > 0 {
            changes.push(format!("{quad_diffs} quadratic term change(s)"));
        }
        if o1.attributes.priority != o2.attributes.priority {
            changes.push(format!("priority: {:?} -> {:?}", o1.attributes.priority, o2.attributes.priority));
        }
        for (label, a, b) in [
            ("weight", o1.attributes.weight, o2.attributes.weight),
            ("abs_tol", o1.attributes.abs_tol, o2.attributes.abs_tol),
            ("rel_tol", o1.attributes.rel_tol, o2.attributes.rel_tol),
        ] {
            let differs = match (a, b) {
                (Some(a), Some(b)) => tol.differ(a, b),
                _ => a.is_some() != b.is_some(),
            };
            if differs {
                changes.push(format!("{label}: {a:?} -> {b:?}"));
            }
        }
        if !changes.is_empty() {
            modified.push((name.to_string(), changes));
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
// The only panics are the expects on positions taken from the same maps.
#[allow(clippy::similar_names, clippy::missing_panics_doc)]
pub fn compare(p1: &LpProblem, p2: &LpProblem, options: &DiffOptions) -> LpDiff {
    let canon = Canon(options.normalise);
    let tol = options.tol;

    let cvars1 = canon_index(p1, p1.variables.keys().copied(), canon);
    let cvars2 = canon_index(p2, p2.variables.keys().copied(), canon);
    let ccons1 = canon_index(p1, p1.constraints.values().map(Constraint::name), canon);
    let ccons2 = canon_index(p2, p2.constraints.values().map(Constraint::name), canon);
    let cobjs1 = canon_index(p1, p1.objectives.keys().copied(), canon);
    let cobjs2 = canon_index(p2, p2.objectives.keys().copied(), canon);

    // Sorted name lists keep the output deterministic.
    let vars = NameSplit::new(&cvars1, &cvars2);
    let cons = NameSplit::new(&ccons1, &ccons2);
    let objs = NameSplit::new(&cobjs1, &cobjs2);

    let mut vars_type_changed = Vec::new();
    for &name in &vars.common {
        let (_, v1) = p1.variables.get_index(cvars1[name]).expect("canonical index holds positions of existing variables");
        let (_, v2) = p2.variables.get_index(cvars2[name]).expect("canonical index holds positions of existing variables");
        if v1.kind != v2.kind || bounds_differ(tol, v1, v2) {
            vars_type_changed.push((name.to_string(), format!("{:?}/{}", v1.kind, v1.bounds), format!("{:?}/{}", v2.kind, v2.bounds)));
        }
    }

    let sense_changed = (p1.sense != p2.sense).then(|| (p1.sense.to_string(), p2.sense.to_string()));

    LpDiff {
        sense_changed,
        vars_added: owned(&vars.added),
        vars_removed: owned(&vars.removed),
        vars_type_changed,
        cons_added: owned(&cons.added),
        cons_removed: owned(&cons.removed),
        cons_modified: diff_modified_constraints(p1, p2, &ccons1, &ccons2, &cons.common, canon, tol),
        objs_added: owned(&objs.added),
        objs_removed: owned(&objs.removed),
        objs_modified: diff_modified_objectives(p1, p2, &cobjs1, &cobjs2, &objs.common, canon, tol),
    }
}

/// Whether two variables' bounds differ beyond `tol`.
///
/// Compares the *effective* bounds a solver would use, so an unspecified
/// bound and an explicit one equal to the default (`x >= 0`) match, and
/// tolerances apply as they do to coefficients and right-hand sides.
fn bounds_differ(tol: DiffTol, v1: &Variable, v2: &Variable) -> bool {
    debug_assert!(v1.kind == v2.kind, "bounds are only compared for variables of the same kind");
    tol.differ(v1.bounds.effective_lower(v1.kind), v2.bounds.effective_lower(v2.kind))
        || tol.differ(v1.bounds.effective_upper(v1.kind), v2.bounds.effective_upper(v2.kind))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The map-based definition `LinearComparer::count` must agree with.
    fn map_count(p1: &LpProblem, c1: &[Coefficient], p2: &LpProblem, c2: &[Coefficient], canon: Canon, tol: DiffTol) -> usize {
        let map = |p: &LpProblem, c: &[Coefficient]| -> FxHashMap<String, f64> {
            c.iter().map(|c| (canon.name(p.resolve(c.name)).into_owned(), c.value)).collect()
        };
        let (m1, m2) = (map(p1, c1), map(p2, c2));
        m1.iter().filter(|(k, v)| m2.get(*k).is_none_or(|w| tol.differ(**v, *w))).count()
            + m2.keys().filter(|k| !m1.contains_key(*k)).count()
    }

    #[test]
    fn linear_comparer_matches_map_semantics() {
        let mut p1 = LpProblem::new();
        let mut p2 = LpProblem::new();
        let names = ["x", "y", "z", "X", "x_1", "y_2", "w"];
        let ids1: Vec<NameId> = names.iter().map(|n| p1.intern(n)).collect();
        // Interned in reverse, so equal names have different ids on each side.
        for name in names.iter().rev() {
            p2.intern(name);
        }
        let id2 = |name: &str| p2.name_id(name).expect("known name");
        let c = |id: NameId, value: f64| Coefficient { name: id, value };
        let cases: Vec<(Vec<Coefficient>, Vec<Coefficient>)> = vec![
            (vec![], vec![]),
            (vec![c(ids1[0], 1.0)], vec![]),
            (vec![], vec![c(id2("y"), 2.0)]),
            (vec![c(ids1[0], 1.0), c(ids1[1], 2.0)], vec![c(id2("y"), 2.0), c(id2("x"), 1.5)]),
            // Repeated names: the last value counts, on either side.
            (vec![c(ids1[0], 1.0), c(ids1[0], 3.0), c(ids1[2], 1.0)], vec![c(id2("x"), 3.0), c(id2("z"), 1.0), c(id2("z"), 2.0)]),
            (vec![c(ids1[4], 1.0), c(ids1[5], 2.0), c(ids1[6], 0.0)], vec![c(id2("x"), 1.0), c(id2("y"), 2.0), c(id2("w"), f64::NAN)]),
            (vec![c(ids1[3], 1.0), c(ids1[0], 1.0)], vec![c(id2("X"), 1.0), c(id2("x"), 1.0), c(id2("x_1"), 1.0)]),
            // Same names in the same order (the aligned fast path).
            (vec![c(ids1[0], 1.0), c(ids1[1], 2.0), c(ids1[2], 3.0)], vec![c(id2("x"), 1.0), c(id2("y"), 2.5), c(id2("z"), 3.0)]),
            (vec![c(ids1[4], 1.0), c(ids1[5], 2.0)], vec![c(id2("x_1"), 1.0), c(id2("y_2"), 2.0)]),
            // Aligned but repeated: only the last value of `x` counts.
            (vec![c(ids1[0], 1.0), c(ids1[0], 2.0)], vec![c(id2("x"), 2.0), c(id2("x"), 1.0)]),
            (vec![c(ids1[0], 1.0), c(ids1[0], 2.0)], vec![c(id2("x"), 1.0), c(id2("x"), 2.0)]),
        ];
        let normalise = |name: &str| strip_index_suffix(&name.to_ascii_lowercase());
        for canon in [Canon(None), Canon(Some(&normalise))] {
            for tol in [DiffTol::default(), DiffTol { abs: 0.6, rel: 0.0 }] {
                let mut linear = LinearComparer::new(&p1, &p2, canon, tol);
                for (a, b) in &cases {
                    assert_eq!(linear.count(a, b), map_count(&p1, a, &p2, b, canon, tol), "{a:?} vs {b:?}");
                }
            }
        }
    }

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
    fn tol_reports_change_to_or_from_infinity() {
        for tol in [DiffTol::default(), DiffTol { abs: 1e-6, rel: 1e-9 }, DiffTol { abs: 0.0, rel: 0.5 }] {
            assert!(tol.differ(1.0, f64::INFINITY), "{tol:?}");
            assert!(tol.differ(f64::NEG_INFINITY, -5.0), "{tol:?}");
            assert!(tol.differ(f64::NEG_INFINITY, f64::INFINITY), "{tol:?}");
            assert!(!tol.differ(f64::INFINITY, f64::INFINITY), "{tol:?}");
            assert!(!tol.differ(f64::NEG_INFINITY, f64::NEG_INFINITY), "{tol:?}");
        }
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
    fn tol_new_rejects_invalid_tolerances() {
        assert_eq!(DiffTol::new(0.5, 0.0).unwrap(), DiffTol { abs: 0.5, rel: 0.0 });
        for (abs, rel) in [(-1.0, 0.0), (0.0, -1e-9), (f64::NAN, 0.0), (0.0, f64::INFINITY)] {
            assert!(DiffTol::new(abs, rel).is_err(), "({abs}, {rel}) must be rejected");
        }
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
    fn variable_bounds_compare_effectively_and_within_tolerance() {
        let p1 = parse("Minimize\n obj: x + y\nSubject To\n c1: x + y >= 1\nBounds\n y <= 10\nEnd");
        // `x >= 0` restates the default; `y`'s bound moves by 1e-9.
        let p2 = parse("Minimize\n obj: x + y\nSubject To\n c1: x + y >= 1\nBounds\n x >= 0\n y <= 10.00000001\nEnd");
        let diff = p1.diff(&p2, &opts(DiffTol { abs: 1e-6, rel: 0.0 }));
        assert!(diff.vars_type_changed.is_empty(), "{:?}", diff.vars_type_changed);

        // Exact comparison still sees the moved bound, but not the restated default.
        let diff = p1.diff(&p2, &opts(DiffTol::default()));
        let names: Vec<&str> = diff.vars_type_changed.iter().map(|(n, ..)| n.as_str()).collect();
        assert_eq!(names, ["y"]);

        // A bound that becomes infinite is a change.
        let p3 = parse("Minimize\n obj: x + y\nSubject To\n c1: x + y >= 1\nBounds\n y <= inf\nEnd");
        let diff = p1.diff(&p3, &opts(DiffTol { abs: 1e-6, rel: 1e-6 }));
        assert_eq!(diff.vars_type_changed.len(), 1);
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
        assert!(diff.objs_removed.is_empty(), "expected empty, got {:?}", diff.objs_removed);

        let diff = double.diff(&single, &opts(DiffTol::default()));
        assert_eq!(diff.objs_removed, vec!["obj2".to_string()]);
        assert!(diff.objs_added.is_empty(), "expected empty, got {:?}", diff.objs_added);
    }

    #[test]
    fn rhs_change_within_tolerance_is_ignored() {
        let p1 = parse("Minimize\n obj: x\nSubject To\n c1: x >= 100\nEnd");
        let p2 = parse("Minimize\n obj: x\nSubject To\n c1: x >= 100.4\nEnd");
        // abs tolerance 0.5 suppresses the 0.4 rhs change.
        let diff = p1.diff(&p2, &opts(DiffTol { abs: 0.5, rel: 0.0 }));
        assert!(diff.cons_modified.is_empty(), "expected empty, got {:?}", diff.cons_modified);
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
        assert!(diff.vars_added.is_empty(), "expected empty, got {:?}", diff.vars_added);
        assert!(diff.vars_removed.is_empty(), "expected empty, got {:?}", diff.vars_removed);
        assert!(diff.cons_added.is_empty(), "expected empty, got {:?}", diff.cons_added);
        assert!(diff.cons_removed.is_empty(), "expected empty, got {:?}", diff.cons_removed);
        // Constraint c is unchanged after normalisation.
        assert!(diff.cons_modified.is_empty(), "expected empty, got {:?}", diff.cons_modified);
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
    fn identical_sos_sets_listed_in_a_different_order_do_not_differ() {
        // `y` is interned before `x` in p2, so the NameIds differ as well.
        let p1 = parse("Minimize\n obj: x + y\nSubject To\n c1: x + y >= 1\nSOS\n s: S1:: x:1 y:2\nEnd");
        let p2 = parse("Minimize\n obj: y + x\nSubject To\n c1: y + x >= 1\nSOS\n s: S1:: y:2 x:1\nEnd");
        let diff = p1.diff(&p2, &opts(DiffTol::default()));
        assert!(diff.cons_modified.is_empty(), "expected no change, got {:?}", diff.cons_modified);

        let p3 = parse("Minimize\n obj: x + y\nSubject To\n c1: x + y >= 1\nSOS\n s: S2:: x:1 y:2\nEnd");
        let diff = p1.diff(&p3, &opts(DiffTol::default()));
        assert_eq!(diff.cons_modified, vec![("s".to_string(), vec!["SOS definition changed".to_string()])]);
    }

    #[test]
    fn detects_sense_change() {
        let p1 = parse("Minimize\n obj: x\nSubject To\n c1: x >= 1\nEnd");
        let p2 = parse("Maximize\n obj: x\nSubject To\n c1: x >= 1\nEnd");
        let diff = p1.diff(&p2, &opts(DiffTol::default()));
        assert_eq!(diff.sense_changed, Some(("Minimize".to_string(), "Maximize".to_string())));
        assert!(!diff.is_empty(), "a sense change is a difference");
        assert_eq!(p1.diff(&p1, &opts(DiffTol::default())).sense_changed, None);
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
