//! Solution-preserving problem rewrites ("presolve").
//!
//! Each rule removes work from the model without changing the set of optimal
//! solutions: rows that cannot bind are dropped, and bounds are tightened to
//! the smallest box that still contains every feasible point. The rules feed
//! each other — a singleton row becomes a bound, the tighter bound makes
//! another row redundant — so they are run to a fixpoint (see [`MAX_PASSES`]).
//!
//! Rows are removed; **columns are only ever fixed, never removed**. Keeping
//! the variable set identical on both sides is what lets the original and the
//! rewritten model be compared with the ordinary solve-diff view: every
//! variable still appears in both results, so a difference in the comparison is
//! a real difference and not an artefact of the rewrite.
//!
//! Bounds are interpreted exactly as [`crate::solver::variable_bounds`]
//! interprets them, so presolve reasons about the same box `HiGHS` will see.

use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::time::{Duration, Instant};

use lp_parser_rs::interner::NameId;
use lp_parser_rs::model::{Coefficient, ComparisonOp, Constraint, Sense};
use lp_parser_rs::problem::LpProblem;

use crate::solver::{primary_objective_coefficients, variable_bounds};

/// Absolute tolerance for treating a coefficient as zero and for comparing a
/// row activity against its right-hand side.
const EPS: f64 = 1e-9;

/// Relative improvement a derived bound must beat before it replaces the
/// current one. Without it, propagation on a continuous model can inch towards
/// a limit by ever-smaller steps and never reach the fixpoint.
const MIN_TIGHTENING: f64 = 1e-7;

/// Hard cap on fixpoint iterations. Rules only ever narrow bounds and drop
/// rows, so they converge on their own; this bounds the worst case.
pub const MAX_PASSES: usize = 10;

/// A single rewrite rule. Declaration order is application order within a pass:
/// singleton rows become bounds, propagation tightens from those bounds,
/// rounding sharpens integer bounds, and the two removal rules act on the
/// result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rule {
    /// A row with one variable is a bound in disguise: `3x <= 12` ⇒ `x <= 4`.
    SingletonToBound,
    /// Derive implied bounds from each row's minimum and maximum activity.
    BoundPropagation,
    /// Round fractional bounds on integer variables inwards: `x <= 3.7` ⇒ `x <= 3`.
    IntegerRounding,
    /// Drop rows that hold everywhere in the box; fix variables in rows that
    /// can only be satisfied at a single point.
    RedundantRows,
    /// Drop rows with no terms; fix variables that appear in no row.
    EmptyRowsCols,
}

impl Rule {
    /// Every rule, in application order.
    pub const ALL: [Self; 5] =
        [Self::SingletonToBound, Self::BoundPropagation, Self::IntegerRounding, Self::RedundantRows, Self::EmptyRowsCols];

    /// Number of rules — the width of a [`RuleSet`].
    pub const COUNT: usize = Self::ALL.len();

    /// Short name shown in the picker.
    pub const fn label(self) -> &'static str {
        match self {
            Self::SingletonToBound => "Singleton rows \u{2192} bounds",
            Self::BoundPropagation => "Bound propagation",
            Self::IntegerRounding => "Integer bound rounding",
            Self::RedundantRows => "Redundant & forcing rows",
            Self::EmptyRowsCols => "Empty rows & columns",
        }
    }

    /// One-line explanation shown under the picker.
    pub const fn detail(self) -> &'static str {
        match self {
            Self::SingletonToBound => "a row with one term is a bound: 3x <= 12 becomes x <= 4",
            Self::BoundPropagation => "implied bounds from each row's min/max activity",
            Self::IntegerRounding => "round fractional bounds inwards on integer variables",
            Self::RedundantRows => "drop rows that can never bind; fix variables pinned by forcing rows",
            Self::EmptyRowsCols => "drop termless rows; fix variables appearing in no row",
        }
    }
}

/// Which rules are enabled, indexed by `rule as usize`.
pub type RuleSet = [bool; Rule::COUNT];

/// All rules enabled — the default for the picker.
pub const ALL_RULES: RuleSet = [true; Rule::COUNT];

/// Whether `rule` is enabled in `rules`.
pub const fn enabled(rules: RuleSet, rule: Rule) -> bool {
    rules[rule as usize]
}

/// What one fixpoint pass achieved.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PassStats {
    pub rows_removed: usize,
    pub cols_fixed: usize,
    pub bounds_tightened: usize,
}

impl PassStats {
    /// Whether the pass changed nothing — the fixpoint has been reached.
    const fn is_empty(self) -> bool {
        self.rows_removed == 0 && self.cols_fixed == 0 && self.bounds_tightened == 0
    }
}

/// Outcome of a presolve run.
#[derive(Debug, Clone)]
pub struct PresolveStats {
    /// Per-pass totals; length is the number of passes that changed something.
    pub per_pass: Vec<PassStats>,
    /// Row count before any rule ran.
    pub rows_before: usize,
    /// Row count after the fixpoint.
    pub rows_after: usize,
    /// Column count (unchanged: presolve fixes columns, it never removes them).
    pub cols: usize,
    /// Wall-clock time for the whole run.
    pub duration: Duration,
    /// Set when a rule proved the model infeasible. The rewritten problem is
    /// not usable in that case and the run stops immediately.
    pub infeasible: Option<String>,
}

impl PresolveStats {
    /// Total rows removed across all passes.
    #[must_use]
    pub const fn rows_removed(&self) -> usize {
        self.rows_before - self.rows_after
    }

    /// Total columns fixed across all passes.
    #[must_use]
    pub fn cols_fixed(&self) -> usize {
        self.per_pass.iter().map(|p| p.cols_fixed).sum()
    }

    /// Total bound tightenings across all passes.
    #[must_use]
    pub fn bounds_tightened(&self) -> usize {
        self.per_pass.iter().map(|p| p.bounds_tightened).sum()
    }

    /// Whether the rewrite left the model untouched.
    #[must_use]
    pub const fn is_noop(&self) -> bool {
        self.per_pass.is_empty()
    }

    /// One-line summary, used as the comparison label for the rewritten side.
    #[must_use]
    pub fn headline(&self) -> String {
        if let Some(reason) = &self.infeasible {
            return format!("presolve: infeasible \u{2014} {reason}");
        }
        if self.is_noop() {
            return "presolve: no change".to_owned();
        }
        format!(
            "presolve: -{} rows, {} cols fixed, {} bounds, {} pass(es), {:.1}ms",
            self.rows_removed(),
            self.cols_fixed(),
            self.bounds_tightened(),
            self.per_pass.len(),
            self.duration.as_secs_f64() * 1000.0,
        )
    }
}

/// Apply the enabled rules to a copy of `problem` until nothing changes.
///
/// Returns the rewritten problem alongside what each pass achieved. When a rule
/// proves infeasibility the run stops and [`PresolveStats::infeasible`] is set;
/// the returned problem is then a partially-rewritten intermediate and should
/// not be solved.
#[must_use]
pub fn presolve(problem: &LpProblem, rules: RuleSet) -> (LpProblem, PresolveStats) {
    let started = Instant::now();
    let mut out = problem.clone();
    let rows_before = out.constraints.len();
    let mut per_pass = Vec::new();
    let mut infeasible = None;

    for _ in 0..MAX_PASSES {
        let mut pass = PassStats::default();

        if enabled(rules, Rule::SingletonToBound) {
            singleton_to_bound(&mut out, &mut pass, &mut infeasible);
        }
        if infeasible.is_none() && enabled(rules, Rule::BoundPropagation) {
            bound_propagation(&mut out, &mut pass, &mut infeasible);
        }
        if infeasible.is_none() && enabled(rules, Rule::IntegerRounding) {
            integer_rounding(&mut out, &mut pass, &mut infeasible);
        }
        if infeasible.is_none() && enabled(rules, Rule::RedundantRows) {
            redundant_rows(&mut out, &mut pass, &mut infeasible);
        }
        if infeasible.is_none() && enabled(rules, Rule::EmptyRowsCols) {
            empty_rows_cols(&mut out, &mut pass, &mut infeasible);
        }

        let converged = pass.is_empty();
        if !converged {
            per_pass.push(pass);
        }
        if converged || infeasible.is_some() {
            break;
        }
    }

    let stats = PresolveStats {
        per_pass,
        rows_before,
        rows_after: out.constraints.len(),
        cols: out.variables.len(),
        duration: started.elapsed(),
        infeasible,
    };
    debug_assert_eq!(stats.cols, problem.variables.len(), "presolve fixes columns but must never remove them");
    debug_assert!(stats.rows_after <= stats.rows_before, "presolve must not add rows");
    (out, stats)
}

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

/// The box `HiGHS` will see for `var`, as `(lower, upper)`.
fn effective_box(problem: &LpProblem, var: NameId) -> (f64, f64) {
    let (_, lower, upper) = variable_bounds(problem.variables.get(&var));
    (lower, upper)
}

/// Collapse a row's coefficients into `(variable, coefficient)` terms: repeated
/// variables are summed, and terms that are numerically zero or reference an
/// unknown variable are dropped.
///
/// Insertion order is preserved so the activity sums below are reproducible
/// across runs — a `HashMap` iteration order would make the rewrite
/// non-deterministic in the last bits.
fn row_terms(problem: &LpProblem, coefficients: &[Coefficient]) -> Vec<(NameId, f64)> {
    let mut index: HashMap<NameId, usize> = HashMap::with_capacity(coefficients.len());
    let mut terms: Vec<(NameId, f64)> = Vec::with_capacity(coefficients.len());
    for coefficient in coefficients {
        if !problem.variables.contains_key(&coefficient.name) {
            continue;
        }
        match index.entry(coefficient.name) {
            Entry::Occupied(slot) => terms[*slot.get()].1 += coefficient.value,
            Entry::Vacant(slot) => {
                slot.insert(terms.len());
                terms.push((coefficient.name, coefficient.value));
            }
        }
    }
    terms.retain(|(_, value)| value.abs() > EPS);
    terms
}

/// A running activity sum that counts infinite contributions separately, so the
/// residual "everything except term j" stays well defined when some other term
/// is unbounded.
///
/// Every contribution to a minimum activity is either finite or `-inf`, and
/// every contribution to a maximum activity is either finite or `+inf`, so the
/// infinities never cancel and a plain count is enough.
#[derive(Debug, Clone, Copy, Default)]
struct Activity {
    finite: f64,
    infinities: usize,
}

impl Activity {
    fn add(&mut self, value: f64) {
        debug_assert!(!value.is_nan(), "activity contribution must not be NaN");
        if value.is_infinite() {
            self.infinities += 1;
        } else {
            self.finite += value;
        }
    }

    /// The sum, or `None` when an unbounded term makes it infinite.
    const fn total(self) -> Option<f64> {
        if self.infinities == 0 { Some(self.finite) } else { None }
    }

    /// The sum excluding one previously added contribution, or `None` when the
    /// remainder is still infinite.
    fn without(self, value: f64) -> Option<f64> {
        let (finite, infinities) =
            if value.is_infinite() { (self.finite, self.infinities - 1) } else { (self.finite - value, self.infinities) };
        if infinities == 0 { Some(finite) } else { None }
    }
}

/// This term's contribution to the row's minimum and maximum activity.
fn contributions(problem: &LpProblem, var: NameId, coefficient: f64) -> (f64, f64) {
    debug_assert!(coefficient.abs() > EPS, "zero terms are dropped by row_terms");
    let (lower, upper) = effective_box(problem, var);
    if coefficient > 0.0 { (coefficient * lower, coefficient * upper) } else { (coefficient * upper, coefficient * lower) }
}

/// Minimum and maximum activity of a row over the current variable box.
fn row_activities(problem: &LpProblem, terms: &[(NameId, f64)]) -> (Activity, Activity) {
    let mut min = Activity::default();
    let mut max = Activity::default();
    for &(var, coefficient) in terms {
        let (low, high) = contributions(problem, var, coefficient);
        debug_assert!(low != f64::INFINITY, "a minimum-activity contribution is never +inf");
        debug_assert!(high != f64::NEG_INFINITY, "a maximum-activity contribution is never -inf");
        min.add(low);
        max.add(high);
    }
    (min, max)
}

/// Whether the row is bounded above by its right-hand side (`<=` or `=`).
const fn bounded_above(operator: ComparisonOp) -> bool {
    matches!(operator, ComparisonOp::LT | ComparisonOp::LTE | ComparisonOp::EQ)
}

/// Whether the row is bounded below by its right-hand side (`>=` or `=`).
const fn bounded_below(operator: ComparisonOp) -> bool {
    matches!(operator, ComparisonOp::GT | ComparisonOp::GTE | ComparisonOp::EQ)
}

/// Result of intersecting a derived bound into a variable's current box.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tighten {
    Unchanged,
    Tightened,
    Infeasible,
}

/// Whether `new` is a strict enough improvement on `old` to be worth storing.
fn is_tighter(old: f64, new: f64) -> bool {
    if !new.is_finite() {
        return false;
    }
    if !old.is_finite() {
        return true;
    }
    (new - old).abs() > MIN_TIGHTENING * (1.0 + old.abs())
}

/// Intersect `lower`/`upper` into `var`'s box. Bounds only ever narrow, which
/// is what guarantees the fixpoint loop terminates.
fn tighten(problem: &mut LpProblem, var: NameId, lower: Option<f64>, upper: Option<f64>) -> Tighten {
    let (old_lower, old_upper) = effective_box(problem, var);
    let new_lower = lower.map_or(old_lower, |value| old_lower.max(value));
    let new_upper = upper.map_or(old_upper, |value| old_upper.min(value));

    if new_lower > new_upper + EPS {
        return Tighten::Infeasible;
    }
    if !is_tighter(old_lower, new_lower) && !is_tighter(old_upper, new_upper) {
        return Tighten::Unchanged;
    }
    let Some(variable) = problem.variables.get_mut(&var) else {
        return Tighten::Unchanged;
    };
    if new_lower.is_finite() {
        variable.bounds.lower = Some(new_lower);
    }
    if new_upper.is_finite() {
        variable.bounds.upper = Some(new_upper);
    }
    Tighten::Tightened
}

/// Apply a batch of derived bounds, counting tightenings and stopping on the
/// first infeasibility.
fn apply_bounds(
    problem: &mut LpProblem,
    updates: &[(NameId, Option<f64>, Option<f64>)],
    pass: &mut PassStats,
    infeasible: &mut Option<String>,
) {
    for &(var, lower, upper) in updates {
        match tighten(problem, var, lower, upper) {
            Tighten::Unchanged => {}
            Tighten::Tightened => pass.bounds_tightened += 1,
            Tighten::Infeasible => {
                *infeasible = Some(format!("variable {} has an empty domain after tightening", problem.resolve(var)));
                return;
            }
        }
    }
}

/// Drop the named rows, preserving the order of those that remain.
fn remove_rows(problem: &mut LpProblem, doomed: &[NameId]) {
    if doomed.is_empty() {
        return;
    }
    // ponytail: linear membership scan; a pass drops a handful of rows, and a
    // HashSet only pays off well past that.
    problem.constraints.retain(|id, _| !doomed.contains(id));
}

// ---------------------------------------------------------------------------
// Rules
// ---------------------------------------------------------------------------

/// The bound implied by `coefficient * x <operator> rhs`, as `(lower, upper)`.
fn implied_bound(operator: ComparisonOp, coefficient: f64, rhs: f64) -> (Option<f64>, Option<f64>) {
    debug_assert!(coefficient.abs() > EPS, "a singleton row's coefficient is non-zero");
    let bound = rhs / coefficient;
    match operator {
        // Dividing by a negative coefficient flips the comparison.
        ComparisonOp::EQ => (Some(bound), Some(bound)),
        ComparisonOp::LT | ComparisonOp::LTE => {
            if coefficient > 0.0 {
                (None, Some(bound))
            } else {
                (Some(bound), None)
            }
        }
        ComparisonOp::GT | ComparisonOp::GTE => {
            if coefficient > 0.0 {
                (Some(bound), None)
            } else {
                (None, Some(bound))
            }
        }
    }
}

/// Rewrite one-term rows as variable bounds and drop them.
fn singleton_to_bound(problem: &mut LpProblem, pass: &mut PassStats, infeasible: &mut Option<String>) {
    let mut doomed = Vec::new();
    let mut updates = Vec::new();

    for (row_id, constraint) in &problem.constraints {
        let Constraint::Standard { coefficients, operator, rhs, .. } = constraint else {
            continue;
        };
        let terms = row_terms(problem, coefficients);
        if terms.len() != 1 {
            continue;
        }
        let (var, coefficient) = terms[0];
        let (lower, upper) = implied_bound(*operator, coefficient, *rhs);
        updates.push((var, lower, upper));
        doomed.push(*row_id);
    }

    apply_bounds(problem, &updates, pass, infeasible);
    if infeasible.is_some() {
        return;
    }
    pass.rows_removed += doomed.len();
    remove_rows(problem, &doomed);
}

/// Derive implied bounds from each row's activity range.
///
/// For `sum a_j x_j <= rhs`, the tightest any single term can be is
/// `a_j x_j <= rhs - min(rest)`, which turns into a bound on `x_j`. The mirror
/// argument on maximum activity handles `>=`; an equality gives both.
fn bound_propagation(problem: &mut LpProblem, pass: &mut PassStats, infeasible: &mut Option<String>) {
    let mut updates = Vec::new();

    for constraint in problem.constraints.values() {
        let Constraint::Standard { coefficients, operator, rhs, .. } = constraint else {
            continue;
        };
        let terms = row_terms(problem, coefficients);
        // One-term rows are exactly the singleton rule's job.
        if terms.len() < 2 {
            continue;
        }
        let (min, max) = row_activities(problem, &terms);

        for &(var, coefficient) in &terms {
            let (min_contribution, max_contribution) = contributions(problem, var, coefficient);

            if bounded_above(*operator)
                && let Some(rest) = min.without(min_contribution)
            {
                let bound = (rhs - rest) / coefficient;
                if coefficient > 0.0 {
                    updates.push((var, None, Some(bound)));
                } else {
                    updates.push((var, Some(bound), None));
                }
            }
            if bounded_below(*operator)
                && let Some(rest) = max.without(max_contribution)
            {
                let bound = (rhs - rest) / coefficient;
                if coefficient > 0.0 {
                    updates.push((var, Some(bound), None));
                } else {
                    updates.push((var, None, Some(bound)));
                }
            }
        }
    }

    apply_bounds(problem, &updates, pass, infeasible);
}

/// Round fractional bounds on integer variables inwards.
fn integer_rounding(problem: &mut LpProblem, pass: &mut PassStats, infeasible: &mut Option<String>) {
    let mut updates = Vec::new();

    for (var_id, variable) in &problem.variables {
        if !variable.kind.is_integer() {
            continue;
        }
        // An absent bound is the solver's implicit 0 / +inf, both already integral.
        let lower = variable.bounds.lower.filter(|value| value.is_finite() && value.fract() != 0.0).map(f64::ceil);
        let upper = variable.bounds.upper.filter(|value| value.is_finite() && value.fract() != 0.0).map(f64::floor);
        if lower.is_some() || upper.is_some() {
            updates.push((*var_id, lower, upper));
        }
    }

    // Rounding must land even when it is smaller than the propagation
    // threshold, so it is applied directly rather than through `tighten`.
    for (var, lower, upper) in updates {
        let (old_lower, old_upper) = effective_box(problem, var);
        let new_lower = lower.unwrap_or(old_lower);
        let new_upper = upper.unwrap_or(old_upper);
        if new_lower > new_upper + EPS {
            *infeasible = Some(format!("integer variable {} has no integral point in its bounds", problem.resolve(var)));
            return;
        }
        let Some(variable) = problem.variables.get_mut(&var) else {
            continue;
        };
        if let Some(value) = lower {
            variable.bounds.lower = Some(value);
        }
        if let Some(value) = upper {
            variable.bounds.upper = Some(value);
        }
        pass.bounds_tightened += 1;
    }
}

/// Drop rows that hold everywhere in the box, and fix the variables of rows
/// that can only be satisfied at a single point.
fn redundant_rows(problem: &mut LpProblem, pass: &mut PassStats, infeasible: &mut Option<String>) {
    let mut doomed = Vec::new();
    let mut fixes: Vec<(NameId, Option<f64>, Option<f64>)> = Vec::new();

    for (row_id, constraint) in &problem.constraints {
        let Constraint::Standard { name, coefficients, operator, rhs, .. } = constraint else {
            continue;
        };
        let terms = row_terms(problem, coefficients);
        // Termless rows are the empty-row rule's job.
        if terms.is_empty() {
            continue;
        }
        let (min, max) = row_activities(problem, &terms);
        let (above, below) = (bounded_above(*operator), bounded_below(*operator));

        // No point in the box satisfies the row.
        if above && min.total().is_some_and(|value| value > rhs + EPS) {
            *infeasible = Some(format!("row {} cannot be satisfied: minimum activity exceeds its rhs", problem.resolve(*name)));
            return;
        }
        if below && max.total().is_some_and(|value| value < rhs - EPS) {
            *infeasible = Some(format!("row {} cannot be satisfied: maximum activity is below its rhs", problem.resolve(*name)));
            return;
        }

        // Forcing: the row is only satisfiable at its extreme activity, which
        // pins every variable in it to one end of its box.
        let forcing_at_min = above && min.total().is_some_and(|value| value >= rhs - EPS);
        let forcing_at_max = below && max.total().is_some_and(|value| value <= rhs + EPS);
        if forcing_at_min || forcing_at_max {
            for &(var, coefficient) in &terms {
                let (lower, upper) = effective_box(problem, var);
                // At minimum activity a positive coefficient sits at its lower
                // bound; at maximum activity it sits at its upper bound.
                let value = if forcing_at_min == (coefficient > 0.0) { lower } else { upper };
                debug_assert!(value.is_finite(), "a finite extreme activity pins each variable at a finite bound");
                fixes.push((var, Some(value), Some(value)));
            }
            doomed.push(*row_id);
            continue;
        }

        // Redundant: the row holds for every point in the box.
        let holds_above = !above || max.total().is_some_and(|value| value <= rhs + EPS);
        let holds_below = !below || min.total().is_some_and(|value| value >= rhs - EPS);
        if holds_above && holds_below {
            doomed.push(*row_id);
        }
    }

    let fixed_before = pass.bounds_tightened;
    apply_bounds(problem, &fixes, pass, infeasible);
    // A forcing row's fixes are reported as fixed columns, not as bound tightenings.
    let newly_fixed = pass.bounds_tightened - fixed_before;
    pass.bounds_tightened = fixed_before;
    pass.cols_fixed += newly_fixed;
    if infeasible.is_some() {
        return;
    }
    pass.rows_removed += doomed.len();
    remove_rows(problem, &doomed);
}

/// Drop rows with no terms, and fix variables that appear in no row.
fn empty_rows_cols(problem: &mut LpProblem, pass: &mut PassStats, infeasible: &mut Option<String>) {
    let mut doomed = Vec::new();

    for (row_id, constraint) in &problem.constraints {
        let Constraint::Standard { name, coefficients, operator, rhs, .. } = constraint else {
            continue;
        };
        if !row_terms(problem, coefficients).is_empty() {
            continue;
        }
        let holds = match operator {
            ComparisonOp::LT | ComparisonOp::LTE => *rhs >= -EPS,
            ComparisonOp::GT | ComparisonOp::GTE => *rhs <= EPS,
            ComparisonOp::EQ => rhs.abs() <= EPS,
        };
        if !holds {
            *infeasible = Some(format!("row {} has no terms and a rhs it cannot meet", problem.resolve(*name)));
            return;
        }
        doomed.push(*row_id);
    }
    pass.rows_removed += doomed.len();
    remove_rows(problem, &doomed);

    // A variable in no row is decided entirely by its objective coefficient, so
    // it can be fixed at whichever bound the objective prefers. SOS sets count
    // as occurrences: the solver may skip those rows, but the variable is still
    // structurally involved.
    let mut used: Vec<NameId> = Vec::new();
    for constraint in problem.constraints.values() {
        match constraint {
            Constraint::Standard { coefficients, .. } => used.extend(coefficients.iter().map(|c| c.name)),
            Constraint::SOS { weights, .. } => used.extend(weights.iter().map(|c| c.name)),
        }
    }
    used.sort_unstable();
    used.dedup();

    let objective = primary_objective_coefficients(problem);
    let minimising = matches!(problem.sense, Sense::Minimize);
    let mut fixes = Vec::new();

    for var_id in problem.variables.keys() {
        if used.binary_search(var_id).is_ok() {
            continue;
        }
        let cost = objective.get(var_id).copied().unwrap_or(0.0);
        // Minimising, a non-negative cost is best served at the lower bound.
        let prefer_lower = if minimising { cost >= 0.0 } else { cost <= 0.0 };
        let (lower, upper) = effective_box(problem, *var_id);
        let value = if prefer_lower { lower } else { upper };
        // Unbounded in the profitable direction: the model is unbounded, which
        // is the solver's verdict to deliver, not presolve's to hide.
        if !value.is_finite() {
            continue;
        }
        fixes.push((*var_id, Some(value), Some(value)));
    }

    let tightened_before = pass.bounds_tightened;
    apply_bounds(problem, &fixes, pass, infeasible);
    let newly_fixed = pass.bounds_tightened - tightened_before;
    pass.bounds_tightened = tightened_before;
    pass.cols_fixed += newly_fixed;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(source: &str) -> LpProblem {
        LpProblem::parse(source).expect("test fixture must parse")
    }

    fn bounds_of(problem: &LpProblem, name: &str) -> (f64, f64) {
        let id = problem.name_id(name).expect("variable must exist");
        effective_box(problem, id)
    }

    /// Compare a derived bound against its expected value, tolerating the
    /// rounding of the divisions the rules perform and handling infinities.
    #[track_caller]
    fn assert_bound(actual: f64, expected: f64, what: &str) {
        if expected.is_infinite() {
            assert!(
                actual.is_infinite() && actual.is_sign_positive() == expected.is_sign_positive(),
                "{what}: expected {expected}, got {actual}"
            );
            return;
        }
        assert!((actual - expected).abs() < 1e-9, "{what}: expected {expected}, got {actual}");
    }

    #[test]
    fn singleton_row_becomes_a_bound_and_the_row_goes() {
        let problem = parse("Minimize\n obj: x + y\nSubject To\n c1: 3 x <= 12\n c2: x + y >= 2\nEnd");
        let (out, stats) = presolve(&problem, [true, false, false, false, false]);

        assert_eq!(out.constraint_count(), 1, "the singleton row is removed");
        assert_bound(bounds_of(&out, "x").1, 4.0, "3x <= 12 tightens x's upper bound to 4");
        assert_eq!(stats.rows_removed(), 1);
    }

    #[test]
    fn singleton_with_a_negative_coefficient_flips_the_comparison() {
        let problem = parse("Minimize\n obj: x\nSubject To\n c1: -2 x <= -6\n c2: x + y >= 0\nEnd");
        let (out, _) = presolve(&problem, [true, false, false, false, false]);

        assert_bound(bounds_of(&out, "x").0, 3.0, "-2x <= -6 is x >= 3, a lower bound");
    }

    #[test]
    fn propagation_derives_an_implied_upper_bound() {
        // y >= 0 by default, so x + y <= 10 forces x <= 10.
        let problem = parse("Minimize\n obj: x + y\nSubject To\n c1: x + y <= 10\nEnd");
        let (out, stats) = presolve(&problem, [false, true, false, false, false]);

        assert_bound(bounds_of(&out, "x").1, 10.0, "the other term's minimum activity caps x at 10");
        assert!(stats.bounds_tightened() >= 1);
    }

    #[test]
    fn propagation_leaves_unbounded_rows_alone() {
        // y is free above, so nothing caps x.
        let problem = parse("Minimize\n obj: x\nSubject To\n c1: x - y <= 10\nEnd");
        let (out, stats) = presolve(&problem, [false, true, false, false, false]);

        assert_bound(bounds_of(&out, "x").1, f64::INFINITY, "an unbounded partner term yields no implied bound");
        assert_eq!(stats.bounds_tightened(), 0);
    }

    #[test]
    fn integer_bounds_round_inwards() {
        let problem = parse("Maximize\n obj: x\nSubject To\n c1: x + y <= 9\nBounds\n 0.4 <= x <= 3.7\nGenerals\n x\nEnd");
        let (out, _) = presolve(&problem, [false, false, true, false, false]);

        assert_eq!(bounds_of(&out, "x"), (1.0, 3.0), "an integer variable's fractional bounds round inwards");
    }

    #[test]
    fn a_row_that_can_never_bind_is_removed() {
        let problem = parse("Minimize\n obj: x + y\nSubject To\n c1: x + y <= 1000\nBounds\n x <= 5\n y <= 5\nEnd");
        let (out, stats) = presolve(&problem, [false, false, false, true, false]);

        assert_eq!(out.constraint_count(), 0, "maximum activity 10 never reaches a rhs of 1000");
        assert_eq!(stats.rows_removed(), 1);
    }

    #[test]
    fn a_forcing_row_pins_its_variables() {
        // x, y >= 0 and x + y <= 0 leaves only the origin.
        let problem = parse("Minimize\n obj: x + y\nSubject To\n c1: x + y <= 0\nEnd");
        let (out, stats) = presolve(&problem, [false, false, false, true, false]);

        assert_eq!(bounds_of(&out, "x"), (0.0, 0.0), "a forcing row fixes every variable in it");
        assert_eq!(bounds_of(&out, "y"), (0.0, 0.0));
        assert_eq!(stats.cols_fixed(), 2);
        assert_eq!(out.constraint_count(), 0, "the forcing row is then redundant");
    }

    #[test]
    fn an_unsatisfiable_row_reports_infeasibility() {
        let problem = parse("Minimize\n obj: x + y\nSubject To\n c1: x + y <= -5\nEnd");
        let (_, stats) = presolve(&problem, ALL_RULES);

        assert!(stats.infeasible.is_some(), "x, y >= 0 cannot sum to -5");
    }

    #[test]
    fn a_variable_in_no_row_is_fixed_at_its_preferred_bound() {
        let problem = parse("Minimize\n obj: x + 2 z\nSubject To\n c1: x + y >= 1\nBounds\n 0 <= z <= 8\nEnd");
        let (out, stats) = presolve(&problem, [false, false, false, false, true]);

        assert_eq!(bounds_of(&out, "z"), (0.0, 0.0), "minimising a positive cost drives z to its lower bound");
        assert_eq!(stats.cols_fixed(), 1);
        assert_eq!(out.variable_count(), problem.variable_count(), "columns are fixed, never removed");
    }

    #[test]
    fn a_maximised_variable_in_no_row_goes_to_its_upper_bound() {
        let problem = parse("Maximize\n obj: x + 2 z\nSubject To\n c1: x + y >= 1\nBounds\n 0 <= z <= 8\nEnd");
        let (out, _) = presolve(&problem, [false, false, false, false, true]);

        assert_eq!(bounds_of(&out, "z"), (8.0, 8.0), "maximising a positive cost drives z to its upper bound");
    }

    #[test]
    fn rules_cascade() {
        // c1 becomes x <= 2, which drops c2's maximum activity to 6 — far below
        // its rhs of 50 — so c2 becomes redundant and goes too.
        let source = "Minimize\n obj: x + y\nSubject To\n c1: 5 x <= 10\n c2: x + 2 y <= 50\nBounds\n y <= 2\nEnd";
        let problem = parse(source);

        let (alone, _) = presolve(&problem, [false, false, false, true, false]);
        assert_eq!(alone.constraint_count(), 2, "without the singleton rewrite x is unbounded and neither row can be dropped");

        let (out, _) = presolve(&problem, ALL_RULES);
        assert_eq!(out.constraint_count(), 0, "the bound from c1 is what makes c2 redundant");
    }

    #[test]
    fn presolve_terminates_and_never_grows_the_model() {
        let problem = parse("Minimize\n obj: x + y + z\nSubject To\n c1: x + y <= 10\n c2: y + z >= 2\n c3: x - z = 4\nEnd");
        let (out, stats) = presolve(&problem, ALL_RULES);

        assert!(stats.per_pass.len() <= MAX_PASSES, "the fixpoint loop is bounded");
        assert!(out.constraint_count() <= problem.constraint_count(), "rules only ever remove rows");
        assert_eq!(out.variable_count(), problem.variable_count(), "columns are never removed");
    }

    /// The whole premise of the feature: on real models the rewrite must reach
    /// the same optimum as the original. A rule that quietly cuts off the true
    /// optimum would show up here as a mismatched objective value.
    #[test]
    fn real_models_keep_their_optimum_through_the_rewrite() {
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        for fixture in ["afiro.lp", "kb2.lp", "BeerDistributionProblem.lp", "ComputerPlantProblem.lp"] {
            let source = std::fs::read_to_string(root.join("../rust/resources").join(fixture))
                .unwrap_or_else(|error| panic!("failed to read {fixture}: {error}"));
            let problem = LpProblem::parse(&source).unwrap_or_else(|error| panic!("failed to parse {fixture}: {error}"));

            let (rewritten, stats) = presolve(&problem, ALL_RULES);
            assert!(stats.infeasible.is_none(), "{fixture}: presolve wrongly declared a solvable model infeasible");
            // Guards against the check going vacuous: a rewrite that did
            // nothing would trivially preserve the objective.
            assert!(!stats.is_noop(), "{fixture}: no rule fired, so this fixture proves nothing");

            let before = crate::solver::solve_problem(&problem).unwrap_or_else(|error| panic!("{fixture} original solve: {error}"));
            let after = crate::solver::solve_problem(&rewritten).unwrap_or_else(|error| panic!("{fixture} rewritten solve: {error}"));

            assert_eq!(before.status, after.status, "{fixture}: the rewrite changed the solver status");
            match (before.objective_value, after.objective_value) {
                (Some(original), Some(reduced)) => {
                    let tolerance = 1e-6 * (1.0 + original.abs());
                    assert!(
                        (original - reduced).abs() <= tolerance,
                        "{fixture}: objective moved from {original} to {reduced} \
                         ({} rows removed, {} cols fixed)",
                        stats.rows_removed(),
                        stats.cols_fixed(),
                    );
                }
                (None, None) => {}
                (original, reduced) => panic!("{fixture}: one side has an objective and the other does not: {original:?} vs {reduced:?}"),
            }
        }
    }

    #[test]
    fn disabling_every_rule_is_a_no_op() {
        let problem = parse("Minimize\n obj: x\nSubject To\n c1: 3 x <= 12\nEnd");
        let (out, stats) = presolve(&problem, [false; Rule::COUNT]);

        assert!(stats.is_noop());
        assert_eq!(out.constraint_count(), problem.constraint_count());
    }
}
