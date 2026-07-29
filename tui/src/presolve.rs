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
//!
//! Three of the rules target what the diagnostics pane (`D`) ranks rather than
//! the row count: [`Rule::FixedToRhs`] thins the densest rows, and
//! [`Rule::RowScaling`] and [`Rule::ColumnScaling`] equilibrate the matrix.
//! Because each row is divided by its own factor and each column multiplied by
//! its own, the two together move both the worst-conditioned rows and the
//! worst-conditioned columns — a single row factor alone cannot, since a row's
//! max-to-min ratio is scale-invariant.
//!
//! Scaling is the one rewrite that does not leave the solution in the original
//! units: a column multiplied by 8 reports its variable at an eighth of its
//! real value, and a divided row reports a proportionally larger shadow price.
//! [`PresolveStats::scaling`] carries the factors, and [`Scaling::unscale`] puts
//! a [`SolveResult`] back into the original model's units before it reaches the
//! comparison view. Everything the user sees is therefore in the units they
//! wrote, whichever rules ran.

use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::time::{Duration, Instant};

use lp_parser_rs::interner::NameId;
use lp_parser_rs::model::{Coefficient, ComparisonOp, Constraint, Sense, VariableKind};
use lp_parser_rs::problem::LpProblem;

use crate::solver::{SolveResult, primary_objective_coefficients, variable_bounds};

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
/// fixed columns fold into the right-hand side, singleton rows become bounds,
/// propagation tightens from those bounds, rounding sharpens integer bounds,
/// the two removal rules act on the result, and scaling equilibrates whatever
/// rows survive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rule {
    /// Fold fixed variables into the right-hand side and drop their terms.
    FixedToRhs,
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
    /// Divide each row by a power of two so its largest coefficient is near 1.
    RowScaling,
    /// Rescale each continuous variable's units by a power of two so its
    /// largest coefficient is near 1. Undone in the reported solution.
    ColumnScaling,
}

impl Rule {
    /// Every rule, in application order.
    pub const ALL: [Self; 8] = [
        Self::FixedToRhs,
        Self::SingletonToBound,
        Self::BoundPropagation,
        Self::IntegerRounding,
        Self::RedundantRows,
        Self::EmptyRowsCols,
        Self::RowScaling,
        Self::ColumnScaling,
    ];

    /// Number of rules — the width of a [`RuleSet`].
    pub const COUNT: usize = Self::ALL.len();

    /// Short name shown in the picker.
    pub const fn label(self) -> &'static str {
        match self {
            Self::FixedToRhs => "Fixed columns \u{2192} rhs (thins rows)",
            Self::SingletonToBound => "Singleton rows \u{2192} bounds",
            Self::BoundPropagation => "Bound propagation",
            Self::IntegerRounding => "Integer bound rounding",
            Self::RedundantRows => "Redundant & forcing rows",
            Self::EmptyRowsCols => "Empty rows & columns",
            Self::RowScaling => "Row scaling (powers of two)",
            Self::ColumnScaling => "Column scaling (powers of two)",
        }
    }

    /// One-line explanation shown under the picker.
    pub const fn detail(self) -> &'static str {
        match self {
            Self::FixedToRhs => "a fixed variable's term is a constant: move it to the rhs and drop it",
            Self::SingletonToBound => "a row with one term is a bound: 3x <= 12 becomes x <= 4",
            Self::BoundPropagation => "implied bounds from each row's min/max activity",
            Self::IntegerRounding => "round fractional bounds inwards on integer variables",
            Self::RedundantRows => "drop rows that can never bind; fix variables pinned by forcing rows",
            Self::EmptyRowsCols => "drop termless rows; fix variables appearing in no row",
            Self::RowScaling => "divide each row by 2^k so its largest coefficient is near 1",
            Self::ColumnScaling => "rescale continuous variables by 2^k; the solution is unscaled back",
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
    /// Non-zeros dropped from rows by folding fixed columns into the rhs.
    pub terms_removed: usize,
    /// Rows divided by a power of two by the scaling rule.
    pub rows_scaled: usize,
    /// Columns rescaled by a power of two by the scaling rule.
    pub cols_scaled: usize,
}

impl PassStats {
    /// Whether the pass changed nothing — the fixpoint has been reached.
    const fn is_empty(self) -> bool {
        self.rows_removed == 0
            && self.cols_fixed == 0
            && self.bounds_tightened == 0
            && self.terms_removed == 0
            && self.rows_scaled == 0
            && self.cols_scaled == 0
    }
}

/// The scale factors a run applied, keyed by resolved row and column name.
///
/// A row's factor is what its coefficients and rhs were multiplied by; a
/// column's factor `s` is the change of units `x = s * x'`, so the rewritten
/// model's coefficients for that column were multiplied by `s` and its bounds
/// divided by it. [`Scaling::unscale`] is the inverse, applied to a solve
/// result before anything looks at it.
#[derive(Debug, Clone, Default)]
pub struct Scaling {
    pub rows: HashMap<String, f64>,
    pub cols: HashMap<String, f64>,
}

impl Scaling {
    /// Put a solve result of the rewritten model back into the original
    /// model's units, in place.
    ///
    /// Each of the four quantities needs exactly one of the two factors: the
    /// other cancels. A row's activity is `sum_j a_ij x_j`, and column scaling
    /// multiplies `a_ij` by `s_j` exactly as it divides `x_j`, so activities
    /// only see the row factor. Reduced costs are `c_j - sum_i a_ij y_i`, and
    /// row scaling divides `y_i` exactly as it multiplies `a_ij`, so they only
    /// see the column factor. The objective value is invariant under both.
    pub fn unscale(&self, result: &mut SolveResult) {
        for (name, value) in &mut result.variables {
            if let Some(scale) = self.cols.get(name) {
                *value *= scale;
            }
        }
        for (name, value) in &mut result.reduced_costs {
            if let Some(scale) = self.cols.get(name) {
                *value /= scale;
            }
        }
        for (name, value) in &mut result.row_values {
            if let Some(scale) = self.rows.get(name) {
                *value /= scale;
            }
        }
        for (name, value) in &mut result.shadow_prices {
            if let Some(scale) = self.rows.get(name) {
                *value *= scale;
            }
        }
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
    /// Scale factors applied, for putting the rewritten model's solution back
    /// into the original's units. Empty unless a scaling rule fired.
    pub scaling: Scaling,
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

    /// Total non-zeros dropped from rows across all passes.
    #[must_use]
    pub fn terms_removed(&self) -> usize {
        self.per_pass.iter().map(|p| p.terms_removed).sum()
    }

    /// Total rows rescaled across all passes.
    #[must_use]
    pub fn rows_scaled(&self) -> usize {
        self.per_pass.iter().map(|p| p.rows_scaled).sum()
    }

    /// Total columns rescaled across all passes.
    #[must_use]
    pub fn cols_scaled(&self) -> usize {
        self.per_pass.iter().map(|p| p.cols_scaled).sum()
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
        // The two structural counts only appear when their rule fired, so the
        // headline stays readable as a comparison label.
        let nnz = if self.terms_removed() > 0 { format!(", -{} nnz", self.terms_removed()) } else { String::new() };
        let scaled = if self.rows_scaled() + self.cols_scaled() > 0 {
            format!(", {}r/{}c scaled", self.rows_scaled(), self.cols_scaled())
        } else {
            String::new()
        };
        format!(
            "presolve: -{} rows, {} cols fixed, {} bounds{nnz}{scaled}, {} pass(es), {:.1}ms",
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
    // Accumulated as products: a row or column can be rescaled on more than one
    // pass as the other dimension's scaling shifts what its largest entry is.
    let mut row_factors: HashMap<NameId, f64> = HashMap::new();
    let mut col_factors: HashMap<NameId, f64> = HashMap::new();

    for _ in 0..MAX_PASSES {
        let mut pass = PassStats::default();

        if enabled(rules, Rule::FixedToRhs) {
            fixed_to_rhs(&mut out, &mut pass);
        }
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
        if infeasible.is_none() && enabled(rules, Rule::RowScaling) {
            row_scaling(&mut out, &mut pass, &mut row_factors);
        }
        if infeasible.is_none() && enabled(rules, Rule::ColumnScaling) {
            column_scaling(&mut out, &mut pass, &mut col_factors);
        }

        let converged = pass.is_empty();
        if !converged {
            per_pass.push(pass);
        }
        if converged || infeasible.is_some() {
            break;
        }
    }

    // Resolved to names here: a solve result identifies rows and columns by
    // name, and the interner belongs to the problem, not to the result.
    let resolve_factors = |factors: HashMap<NameId, f64>| -> HashMap<String, f64> {
        factors.into_iter().map(|(id, scale)| (out.resolve(id).to_owned(), scale)).collect()
    };
    let scaling = Scaling { rows: resolve_factors(row_factors), cols: resolve_factors(col_factors) };

    let stats = PresolveStats {
        per_pass,
        rows_before,
        rows_after: out.constraints.len(),
        cols: out.variables.len(),
        duration: started.elapsed(),
        infeasible,
        scaling,
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

/// Fold fixed variables into the right-hand side and drop their terms.
///
/// A variable whose box has collapsed to a point contributes a constant to
/// every row it appears in, so `a x + rest <op> rhs` becomes
/// `rest <op> rhs - a v`. The variable itself stays declared and fixed, so the
/// column set is untouched and the comparison view still lines both sides up.
///
/// This is the rule that thins the densest rows: the other rules fix columns
/// (forcing rows, singleton rows, unused columns) but leave their now-constant
/// terms sitting in the matrix, and the fixpoint loop feeds each new fix back
/// through here.
fn fixed_to_rhs(problem: &mut LpProblem, pass: &mut PassStats) {
    let fixed: HashMap<NameId, f64> = problem
        .variables
        .iter()
        .filter_map(|(id, variable)| {
            let (_, lower, upper) = variable_bounds(Some(variable));
            (lower.is_finite() && (upper - lower).abs() <= EPS).then_some((*id, lower))
        })
        .collect();
    if fixed.is_empty() {
        return;
    }

    for constraint in problem.constraints.values_mut() {
        // SOS sets carry weights, not an activity, so there is no rhs to fold into.
        let Constraint::Standard { coefficients, rhs, .. } = constraint else {
            continue;
        };
        let before = coefficients.len();
        let mut shift = 0.0;
        coefficients.retain(|coefficient| match fixed.get(&coefficient.name) {
            Some(value) => {
                shift += coefficient.value * value;
                false
            }
            None => true,
        });
        let removed = before - coefficients.len();
        if removed > 0 {
            *rhs -= shift;
            pass.terms_removed += removed;
        }
    }
}

/// Half-width of the band a row's largest coefficient is left in: scaling only
/// fires when `max` is outside `[1/BAND, BAND]`, and always lands inside it, so
/// a rescaled row is never rescaled again and the fixpoint loop terminates.
const SCALE_BAND: f64 = 2.0;

/// A scaled-down row must keep its smallest coefficient this far above [`EPS`].
/// Below it the term would read as zero to the activity rules, which would let
/// them derive bounds that cut off feasible points.
const MIN_SCALED_COEFF: f64 = 1e-7;

/// Divide each row by a power of two so its largest coefficient sits near 1.
///
/// A model whose rows span many orders of magnitude gives the simplex ratio
/// test nothing to compare: this pulls the rows onto a common magnitude. On its
/// own it moves the global range and the worst-conditioned *columns*; a row's
/// own max-to-min ratio is scale-invariant, so improving that needs
/// [`column_scaling`] alongside it, which divides the row's entries by
/// different factors.
///
/// The factor is a power of two so every coefficient's mantissa survives
/// untouched: the rewrite introduces no rounding error of its own.
fn row_scaling(problem: &mut LpProblem, pass: &mut PassStats, factors: &mut HashMap<NameId, f64>) {
    for (row_id, constraint) in &mut problem.constraints {
        let Constraint::Standard { coefficients, rhs, .. } = constraint else {
            continue;
        };
        let (mut min, mut max) = (f64::INFINITY, 0.0_f64);
        for coefficient in coefficients.iter() {
            let magnitude = coefficient.value.abs();
            if magnitude > EPS {
                min = min.min(magnitude);
                max = max.max(magnitude);
            }
        }
        if max <= EPS || (1.0 / SCALE_BAND..=SCALE_BAND).contains(&max) {
            continue;
        }
        let scale = (-max.log2().round()).exp2();
        debug_assert!(scale.is_finite() && scale > 0.0, "scale must be a finite positive power of two");
        if min * scale <= MIN_SCALED_COEFF {
            continue;
        }
        for coefficient in coefficients.iter_mut() {
            coefficient.value *= scale;
        }
        // The rhs scales with the row, so the feasible set is unchanged; a
        // positive factor also leaves the comparison operator alone.
        *rhs *= scale;
        *factors.entry(*row_id).or_insert(1.0) *= scale;
        pass.rows_scaled += 1;
    }
}

/// Rescale each continuous variable's units by a power of two, so its largest
/// coefficient sits near 1.
///
/// This is the substitution `x = s x'`: every coefficient in the column is
/// multiplied by `s` and the variable's bounds divided by it, which leaves the
/// feasible set and the objective value untouched but reports `x'` in place of
/// `x`. The factors are kept in [`PresolveStats::scaling`] and undone by
/// [`Scaling::unscale`] before the solution is shown, so the change of units
/// never escapes the rewrite.
///
/// Only continuous columns qualify. Integrality, binariness and the
/// semi-continuous "zero or in range" rule are all statements about a
/// variable's own units, and rescaling would silently redefine them; an SOS
/// set's weights order the original variable in the same way.
///
/// Missing bounds need no attention: the solver's implicit `0` and `+inf` are
/// both fixed points of division by a positive factor.
fn column_scaling(problem: &mut LpProblem, pass: &mut PassStats, factors: &mut HashMap<NameId, f64>) {
    let mut spread: HashMap<NameId, (f64, f64)> = HashMap::new();
    // A variable first seen in an ordinary row keeps its Continuous kind even
    // when a later SOS set names it, so membership has to be checked here too.
    // ponytail: linear scan, sorted lookup if a model ever carries many SOS sets.
    let mut in_sos: Vec<NameId> = Vec::new();
    for constraint in problem.constraints.values() {
        match constraint {
            Constraint::Standard { coefficients, .. } => {
                for coefficient in coefficients {
                    let magnitude = coefficient.value.abs();
                    if magnitude <= EPS {
                        continue;
                    }
                    let entry = spread.entry(coefficient.name).or_insert((f64::INFINITY, 0.0));
                    entry.0 = entry.0.min(magnitude);
                    entry.1 = entry.1.max(magnitude);
                }
            }
            Constraint::SOS { weights, .. } => in_sos.extend(weights.iter().map(|weight| weight.name)),
        }
    }

    let mut scales: HashMap<NameId, f64> = HashMap::new();
    for (var, (min, max)) in &spread {
        let Some(variable) = problem.variables.get(var) else {
            continue;
        };
        if variable.kind != VariableKind::Continuous || in_sos.contains(var) {
            continue;
        }
        if (1.0 / SCALE_BAND..=SCALE_BAND).contains(max) {
            continue;
        }
        let scale = (-max.log2().round()).exp2();
        debug_assert!(scale.is_finite() && scale > 0.0, "scale must be a finite positive power of two");
        if min * scale <= MIN_SCALED_COEFF {
            continue;
        }
        scales.insert(*var, scale);
    }
    if scales.is_empty() {
        return;
    }

    for constraint in problem.constraints.values_mut() {
        let Constraint::Standard { coefficients, .. } = constraint else {
            continue;
        };
        for coefficient in coefficients.iter_mut() {
            if let Some(scale) = scales.get(&coefficient.name) {
                coefficient.value *= scale;
            }
        }
    }
    // The objective is in the same units as the columns: `c_j x_j` is
    // `(c_j s_j) x'_j`, so its value comes out unchanged.
    for objective in problem.objectives.values_mut() {
        for coefficient in &mut objective.coefficients {
            if let Some(scale) = scales.get(&coefficient.name) {
                coefficient.value *= scale;
            }
        }
    }
    for (var, scale) in &scales {
        let Some(variable) = problem.variables.get_mut(var) else {
            continue;
        };
        if let Some(lower) = variable.bounds.lower {
            variable.bounds.lower = Some(lower / scale);
        }
        if let Some(upper) = variable.bounds.upper {
            variable.bounds.upper = Some(upper / scale);
        }
        *factors.entry(*var).or_insert(1.0) *= scale;
        pass.cols_scaled += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(source: &str) -> LpProblem {
        LpProblem::parse(source).expect("test fixture must parse")
    }

    /// A rule set with only the named rules on, so a test exercises one rule
    /// without a positional literal that every new rule would have to widen.
    fn only(rules: &[Rule]) -> RuleSet {
        let mut set = [false; Rule::COUNT];
        for rule in rules {
            set[*rule as usize] = true;
        }
        set
    }

    /// The row's coefficients and rhs, for the scaling tests.
    fn row_of(problem: &LpProblem, name: &str) -> (Vec<f64>, f64) {
        let id = problem.name_id(name).expect("row must exist");
        match problem.constraints.get(&id).expect("row must exist") {
            Constraint::Standard { coefficients, rhs, .. } => (coefficients.iter().map(|c| c.value).collect(), *rhs),
            Constraint::SOS { .. } => panic!("row {name} is an SOS set"),
        }
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
        let (out, stats) = presolve(&problem, only(&[Rule::SingletonToBound]));

        assert_eq!(out.constraint_count(), 1, "the singleton row is removed");
        assert_bound(bounds_of(&out, "x").1, 4.0, "3x <= 12 tightens x's upper bound to 4");
        assert_eq!(stats.rows_removed(), 1);
    }

    #[test]
    fn singleton_with_a_negative_coefficient_flips_the_comparison() {
        let problem = parse("Minimize\n obj: x\nSubject To\n c1: -2 x <= -6\n c2: x + y >= 0\nEnd");
        let (out, _) = presolve(&problem, only(&[Rule::SingletonToBound]));

        assert_bound(bounds_of(&out, "x").0, 3.0, "-2x <= -6 is x >= 3, a lower bound");
    }

    #[test]
    fn propagation_derives_an_implied_upper_bound() {
        // y >= 0 by default, so x + y <= 10 forces x <= 10.
        let problem = parse("Minimize\n obj: x + y\nSubject To\n c1: x + y <= 10\nEnd");
        let (out, stats) = presolve(&problem, only(&[Rule::BoundPropagation]));

        assert_bound(bounds_of(&out, "x").1, 10.0, "the other term's minimum activity caps x at 10");
        assert!(stats.bounds_tightened() >= 1);
    }

    #[test]
    fn propagation_leaves_unbounded_rows_alone() {
        // y is free above, so nothing caps x.
        let problem = parse("Minimize\n obj: x\nSubject To\n c1: x - y <= 10\nEnd");
        let (out, stats) = presolve(&problem, only(&[Rule::BoundPropagation]));

        assert_bound(bounds_of(&out, "x").1, f64::INFINITY, "an unbounded partner term yields no implied bound");
        assert_eq!(stats.bounds_tightened(), 0);
    }

    #[test]
    fn integer_bounds_round_inwards() {
        let problem = parse("Maximize\n obj: x\nSubject To\n c1: x + y <= 9\nBounds\n 0.4 <= x <= 3.7\nGenerals\n x\nEnd");
        let (out, _) = presolve(&problem, only(&[Rule::IntegerRounding]));

        assert_eq!(bounds_of(&out, "x"), (1.0, 3.0), "an integer variable's fractional bounds round inwards");
    }

    #[test]
    fn a_row_that_can_never_bind_is_removed() {
        let problem = parse("Minimize\n obj: x + y\nSubject To\n c1: x + y <= 1000\nBounds\n x <= 5\n y <= 5\nEnd");
        let (out, stats) = presolve(&problem, only(&[Rule::RedundantRows]));

        assert_eq!(out.constraint_count(), 0, "maximum activity 10 never reaches a rhs of 1000");
        assert_eq!(stats.rows_removed(), 1);
    }

    #[test]
    fn a_forcing_row_pins_its_variables() {
        // x, y >= 0 and x + y <= 0 leaves only the origin.
        let problem = parse("Minimize\n obj: x + y\nSubject To\n c1: x + y <= 0\nEnd");
        let (out, stats) = presolve(&problem, only(&[Rule::RedundantRows]));

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
        let (out, stats) = presolve(&problem, only(&[Rule::EmptyRowsCols]));

        assert_eq!(bounds_of(&out, "z"), (0.0, 0.0), "minimising a positive cost drives z to its lower bound");
        assert_eq!(stats.cols_fixed(), 1);
        assert_eq!(out.variable_count(), problem.variable_count(), "columns are fixed, never removed");
    }

    #[test]
    fn a_maximised_variable_in_no_row_goes_to_its_upper_bound() {
        let problem = parse("Maximize\n obj: x + 2 z\nSubject To\n c1: x + y >= 1\nBounds\n 0 <= z <= 8\nEnd");
        let (out, _) = presolve(&problem, only(&[Rule::EmptyRowsCols]));

        assert_eq!(bounds_of(&out, "z"), (8.0, 8.0), "maximising a positive cost drives z to its upper bound");
    }

    #[test]
    fn a_fixed_column_folds_into_the_rhs() {
        let problem = parse("Minimize\n obj: x + y\nSubject To\n c1: 2 x + y <= 10\nBounds\n 2 <= x <= 2\nEnd");
        let (out, stats) = presolve(&problem, only(&[Rule::FixedToRhs]));

        assert_eq!(row_of(&out, "c1"), (vec![1.0], 6.0), "x is fixed at 2, so 2x moves to the rhs");
        assert_eq!(stats.terms_removed(), 1);
        assert_eq!(out.variable_count(), problem.variable_count(), "the fixed column stays declared");
    }

    #[test]
    fn folding_fixed_columns_thins_a_dense_row_across_passes() {
        // c2 forces y to 0, and the next pass folds y out of the dense row.
        let problem = parse("Minimize\n obj: x + y + z\nSubject To\n c1: x + y + z >= 1\n c2: y <= 0\nEnd");
        let (out, stats) = presolve(&problem, only(&[Rule::FixedToRhs, Rule::SingletonToBound]));

        assert_eq!(row_of(&out, "c1").0.len(), 2, "the fixed column is gone from the dense row");
        assert!(stats.terms_removed() >= 1);
    }

    #[test]
    fn row_scaling_divides_by_a_power_of_two() {
        let problem = parse("Minimize\n obj: x + y\nSubject To\n c1: 1024 x + 512 y <= 2048\nEnd");
        let (out, stats) = presolve(&problem, only(&[Rule::RowScaling]));

        assert_eq!(row_of(&out, "c1"), (vec![1.0, 0.5], 2.0), "the row is divided by 2^10, exactly");
        assert_eq!(stats.rows_scaled(), 1);
    }

    #[test]
    fn a_scaled_row_is_not_scaled_again() {
        let problem = parse("Minimize\n obj: x + y\nSubject To\n c1: 1024 x + 512 y <= 2048\nEnd");
        let (once, _) = presolve(&problem, only(&[Rule::RowScaling]));
        let (_, again) = presolve(&once, only(&[Rule::RowScaling]));

        assert!(again.is_noop(), "scaling lands inside the band it tests against, so it reaches a fixpoint");
    }

    #[test]
    fn scaling_leaves_a_row_alone_when_it_would_sink_a_term_into_the_noise() {
        // Scaling by 2^-30 would drag 1e-3 down to ~1e-12, where the activity
        // rules would read it as zero and derive bounds that cut off feasible points.
        let source = "Minimize\n obj: x + y\nSubject To\n c1: 1e9 x + 0.001 y <= 1e9\nEnd";
        let problem = parse(source);
        let (out, stats) = presolve(&problem, only(&[Rule::RowScaling]));

        assert_eq!(stats.rows_scaled(), 0);
        assert_eq!(row_of(&out, "c1"), row_of(&problem, "c1"), "the row is left exactly as it was");
    }

    #[test]
    fn rules_cascade() {
        // c1 becomes x <= 2, which drops c2's maximum activity to 6 — far below
        // its rhs of 50 — so c2 becomes redundant and goes too.
        let source = "Minimize\n obj: x + y\nSubject To\n c1: 5 x <= 10\n c2: x + 2 y <= 50\nBounds\n y <= 2\nEnd";
        let problem = parse(source);

        let (alone, _) = presolve(&problem, only(&[Rule::RedundantRows]));
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
    fn column_scaling_rescales_the_column_its_bounds_and_its_objective() {
        let source = "Minimize\n obj: 3 x + y\nSubject To\n c1: 1024 x + y >= 2048\n c2: 512 x + y >= 0\nBounds\n x <= 8192\nEnd";
        let problem = parse(source);
        let (out, stats) = presolve(&problem, only(&[Rule::ColumnScaling]));

        // x's largest coefficient is 1024, so the column is multiplied by
        // 2^-10 and x' counts in units of 1/1024 of an x.
        let scale = 1.0 / 1024.0;
        assert_eq!(row_of(&out, "c1"), (vec![1.0, 1.0], 2048.0), "the column is scaled, the rhs is not");
        assert_eq!(row_of(&out, "c2").0, vec![0.5, 1.0]);
        assert_bound(bounds_of(&out, "x").1, 8192.0 / scale, "bounds move with the units");
        assert_eq!(stats.cols_scaled(), 1);
        assert_bound(*stats.scaling.cols.get("x").expect("x was scaled"), scale, "x = s x'");
        assert!(!stats.scaling.cols.contains_key("y"), "y's coefficients are already near 1");

        let objective = out.objectives.values().next().expect("one objective");
        let x_cost = objective.coefficients.iter().find(|c| out.resolve(c.name) == "x").expect("x in the objective");
        assert_bound(x_cost.value, 3.0 * scale, "the objective moves with the units so its value is unchanged");
    }

    #[test]
    fn integer_columns_keep_their_units() {
        let source = "Minimize\n obj: x + y\nSubject To\n c1: 1024 x + 1024 y >= 2048\nGenerals\n x\nEnd";
        let problem = parse(source);
        let (out, stats) = presolve(&problem, only(&[Rule::ColumnScaling]));

        assert!(!stats.scaling.cols.contains_key("x"), "rescaling an integer variable would redefine integrality");
        assert_eq!(row_of(&out, "c1").0, vec![1024.0, 1.0], "the continuous column is scaled, the integer one is not");
    }

    /// The end-to-end promise of the scaling rules: the rewritten model is
    /// solved in different units, and what the comparison view sees is back in
    /// the original's — values, duals, activities and all.
    #[test]
    fn unscaling_puts_the_solution_back_in_the_original_units() {
        // A unique optimum, so the two solves have nothing to disagree about
        // beyond the scaling itself.
        // Row scaling pulls both rows down by 2^-12, which leaves y's column
        // far below 1 and gives column scaling something to do.
        let source = "Maximize\n obj: 3 x + 2 y\nSubject To\n c1: 4096 x + y <= 8192\n c2: 4096 x + 2 y <= 9000\nEnd";
        let problem = parse(source);
        let (rewritten, stats) = presolve(&problem, only(&[Rule::RowScaling, Rule::ColumnScaling]));
        assert!(stats.rows_scaled() > 0 && stats.cols_scaled() > 0, "both scalings must fire for this to prove anything");

        let original = crate::solver::solve_problem(&problem).expect("original solve");
        let mut scaled = crate::solver::solve_problem(&rewritten).expect("rewritten solve");

        // Before unscaling the two disagree; that is the whole reason for it.
        assert!(scaled.variables != original.variables, "the rewritten model reports its own units");

        stats.scaling.unscale(&mut scaled);

        let close = |a: f64, b: f64| (a - b).abs() <= 1e-6 * (1.0 + a.abs());
        for ((name, value), (original_name, original_value)) in scaled.variables.iter().zip(&original.variables) {
            assert_eq!(name, original_name);
            assert!(close(*value, *original_value), "{name}: {value} != {original_value}");
        }
        for ((name, value), (original_name, original_value)) in scaled.reduced_costs.iter().zip(&original.reduced_costs) {
            assert_eq!(name, original_name);
            assert!(close(*value, *original_value), "reduced cost {name}: {value} != {original_value}");
        }
        for ((name, value), (original_name, original_value)) in scaled.row_values.iter().zip(&original.row_values) {
            assert_eq!(name, original_name);
            assert!(close(*value, *original_value), "activity {name}: {value} != {original_value}");
        }
        for ((name, value), (original_name, original_value)) in scaled.shadow_prices.iter().zip(&original.shadow_prices) {
            assert_eq!(name, original_name);
            assert!(close(*value, *original_value), "shadow price {name}: {value} != {original_value}");
        }
    }

    /// `w` in the picker writes the rewritten model out as LP, so the rewrite
    /// has to survive a round trip through the writer and the parser.
    #[test]
    fn the_rewritten_model_round_trips_through_the_lp_writer() {
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let source = std::fs::read_to_string(root.join("../rust/resources/afiro.lp")).expect("fixture must read");
        let problem = LpProblem::parse(&source).expect("fixture must parse");

        let (rewritten, stats) = presolve(&problem, ALL_RULES);
        assert!(!stats.is_noop(), "a no-op rewrite would make the round trip prove nothing");

        let written = lp_parser_rs::writer::write_lp_string(&rewritten);
        let reparsed = LpProblem::parse(&written).expect("the written rewrite must parse back");

        assert_eq!(reparsed.constraint_count(), rewritten.constraint_count());
        assert_eq!(reparsed.variable_count(), rewritten.variable_count());

        let before = crate::solver::solve_problem(&rewritten).expect("rewritten solve");
        let after = crate::solver::solve_problem(&reparsed).expect("round-tripped solve");
        match (before.objective_value, after.objective_value) {
            (Some(original), Some(round_tripped)) => {
                let tolerance = 1e-6 * (1.0 + original.abs());
                assert!((original - round_tripped).abs() <= tolerance, "objective moved from {original} to {round_tripped}");
            }
            (original, round_tripped) => panic!("one side has an objective and the other does not: {original:?} vs {round_tripped:?}"),
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
