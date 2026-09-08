//! What `HiGHS` knows about a model beyond its solution.
//!
//! The safe `highs` crate exposes a solve and its primal/dual values. The C API
//! exposes rather more: a certificate of unboundedness, an irreducible
//! infeasible subsystem, and ranging information from the optimal basis. This
//! module reaches past the crate for those, the same way [`crate::highs_presolve`]
//! does for presolve, and for the same reason: the model is built by the shared
//! [`crate::solver::build_highs_model`], so every report describes the model the
//! solver actually sees rather than a lookalike.
//!
//! # Integrality
//!
//! Rays, an IIS and ranging are all LP concepts, resting on a basis a MIP does
//! not have. Each entry point therefore relaxes integrality first and reports
//! how many columns it relaxed, so a MIP still gets an answer and the reader
//! knows which model it is about.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use lp_parser_rs::model::{ComparisonOp, Constraint, Sense, VariableBounds, VariableKind};
use lp_parser_rs::problem::LpProblem;
use lp_parser_rs::interner::NameId;

use crate::solver::{build_highs_model, primary_objective_coefficients, variable_bounds};

/// Ray components at or below this magnitude are treated as zero, matching the
/// tolerance the diagnostics pane uses for matrix entries.
const ZERO: f64 = 1e-9;

/// A variable that moves along the unbounded ray.
#[derive(Debug, Clone)]
pub struct RayEntry {
    pub name: String,
    /// The variable's component of the ray: the direction it runs in.
    pub ray: f64,
    /// Its objective coefficient, so the reader can see why moving pays.
    pub cost: f64,
    pub lower: f64,
    pub upper: f64,
}

/// A variable that *could* explain unboundedness, found without a certificate:
/// improving the objective moves it towards a bound it does not have.
#[derive(Debug, Clone)]
pub struct Suspect {
    pub name: String,
    pub cost: f64,
    /// The direction the objective pushes it in — the side missing a bound.
    pub direction: &'static str,
}

/// Why a model is unbounded, and which variables run away.
#[derive(Debug, Clone)]
pub struct UnboundedRay {
    /// The status the relaxed model solved to.
    pub status: String,
    /// Variables with a non-zero ray component, largest first.
    pub directions: Vec<RayEntry>,
    /// The rate the objective improves along the ray, per unit of it.
    pub objective_rate: f64,
    /// Candidates found by inspection, populated only when `HiGHS` returned no
    /// ray. Not a proof — see [`suspects`].
    pub suspects: Vec<Suspect>,
    /// Columns whose integrality was dropped to make the question an LP one.
    pub relaxed_integrality: usize,
    /// SOS constraints the model build could not represent.
    pub skipped_sos: usize,
    pub duration: Duration,
}

impl UnboundedRay {
    /// Whether the model turned out to be unbounded at all.
    #[must_use]
    pub fn is_unbounded(&self) -> bool {
        crate::solver::status_is_unbounded(&self.status)
    }
}

/// Drop integrality, returning the relaxed model and how many columns changed.
///
/// The integer variable keeps the box it effectively had, so the relaxation is
/// the usual one: the LP whose feasible region contains the MIP's.
fn relax_integrality(problem: &LpProblem) -> (LpProblem, usize) {
    let mut relaxed = problem.clone();
    let mut count = 0;
    for variable in relaxed.variables.values_mut() {
        if !variable.kind.is_integer() {
            continue;
        }
        let (_, lower, upper) = variable_bounds(Some(variable));
        variable.kind = VariableKind::Continuous;
        variable.bounds = VariableBounds::range(lower, upper);
        count += 1;
    }
    debug_assert!(count <= problem.variables.len(), "cannot relax more columns than the model has");
    (relaxed, count)
}

/// Which directions each variable is held in by the constraints.
///
/// A row `sum a_j x_j <= rhs` caps every `x_j` with a positive coefficient from
/// above and every negative one from below; a `>=` row does the reverse; an
/// equality holds both ways. One pass over the matrix, so this costs the same
/// as reading the model once.
#[derive(Debug, Clone, Copy, Default)]
struct Held {
    up: bool,
    down: bool,
}

fn held_by_rows(problem: &LpProblem) -> HashMap<NameId, Held> {
    let mut held: HashMap<NameId, Held> = HashMap::with_capacity(problem.variables.len());

    for constraint in problem.constraints.values() {
        // An SOS set is not an activity bound, so it holds nothing here.
        let Constraint::Standard { coefficients, operator, .. } = constraint else {
            continue;
        };
        let (caps_above, caps_below) = match operator {
            ComparisonOp::LTE | ComparisonOp::LT => (true, false),
            ComparisonOp::GTE | ComparisonOp::GT => (false, true),
            ComparisonOp::EQ => (true, true),
        };

        for coefficient in coefficients {
            if coefficient.value.abs() <= ZERO {
                continue;
            }
            let entry = held.entry(coefficient.name).or_default();
            // A positive coefficient moves the row activity the same way as the
            // variable; a negative one moves it the opposite way.
            let (blocks_up, blocks_down) =
                if coefficient.value > 0.0 { (caps_above, caps_below) } else { (caps_below, caps_above) };
            entry.up |= blocks_up;
            entry.down |= blocks_down;
        }
    }

    held
}

/// Variables the objective pushes towards a bound they do not have, and which
/// no constraint holds in that direction either.
///
/// This is a heuristic, offered only when `HiGHS` declines to produce a ray. It
/// is not a proof: a variable can be held by a *combination* of rows that no
/// single row expresses, so a name here may still be pinned down in practice.
/// It exists because the classic cause of an unbounded model — a missing bound
/// on a profitable variable — is visible without solving anything, and naming
/// the candidate is far more use than reporting that no certificate was
/// available.
///
/// Consulting the matrix is what makes it usable. Most variables in a real
/// model carry no explicit bound and are capped by their rows; without this
/// filter the list would name nearly every column in the model.
fn suspects(problem: &LpProblem) -> Vec<Suspect> {
    let costs = primary_objective_coefficients(problem);
    let held = held_by_rows(problem);
    let mut found = Vec::new();

    for (id, variable) in &problem.variables {
        let Some(&cost) = costs.get(id) else { continue };
        if cost.abs() <= ZERO {
            continue;
        }
        let (_, lower, upper) = variable_bounds(Some(variable));

        // Which way the objective wants this variable to move, and therefore
        // which side has to stop it.
        let improves_upward = match problem.sense {
            Sense::Minimize => cost < 0.0,
            Sense::Maximize => cost > 0.0,
        };
        let rows = held.get(id).copied().unwrap_or_default();
        let escapes = if improves_upward { upper.is_infinite() && !rows.up } else { lower.is_infinite() && !rows.down };

        if escapes {
            found.push(Suspect {
                name: problem.resolve(*id).to_owned(),
                cost,
                direction: if improves_upward { "up" } else { "down" },
            });
        }
    }

    found.sort_by(|a, b| b.cost.abs().partial_cmp(&a.cost.abs()).unwrap_or(std::cmp::Ordering::Equal).then_with(|| a.name.cmp(&b.name)));
    found
}

/// `HiGHS`'s `iis_strategy`: the "from LP" route, which actually searches for an
/// irreducible subsystem.
///
/// The default is `kIisStrategyLight` (0), which performs only trivial
/// inconsistent-bound and empty-row checks and returns. Without raising this the
/// feature silently degrades to a triviality check on most models. The other
/// documented route, `kIisStrategyFromRay`, is described upstream as "not robust,
/// so currently switched off".
const IIS_STRATEGY_FROM_LP: i32 = 2;

/// `HiGHS`'s `IisBoundStatus`, from `lp_data/HighsIis.h`.
///
/// Declared here rather than used from the bindings because `bindgen` only
/// processes `interfaces/highs_c_api.h` (see `highs-sys/wrapper.h`), and this
/// enum lives in a C++ header it never sees.
fn bound_status(code: i32) -> &'static str {
    match code {
        -1 => "dropped",
        1 => "free",
        2 => "lower",
        3 => "upper",
        4 => "both",
        // 0 is `Null`: in the subsystem, but not because of a bound of its own.
        _ => "\u{2014}",
    }
}

/// The rows and column bounds that cannot hold together.
#[derive(Debug, Clone)]
pub struct Iis {
    /// The status the relaxed model solved to.
    pub status: String,
    /// `(constraint name, which of its bounds is implicated)`.
    pub rows: Vec<(String, &'static str)>,
    /// `(variable name, which of its bounds is implicated)`.
    pub cols: Vec<(String, &'static str)>,
    pub relaxed_integrality: usize,
    pub skipped_sos: usize,
    pub duration: Duration,
}

impl Iis {
    /// Whether `HiGHS` isolated a subsystem at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty() && self.cols.is_empty()
    }
}

/// Find an irreducible infeasible subsystem: the smallest set of constraints
/// and variable bounds that are mutually unsatisfiable.
///
/// This complements [`crate::solver::diagnose_infeasibility`] rather than
/// replacing it. That reports the *cheapest* set of constraints to relax, by
/// total violation; this reports a *minimal conflicting* set, no proper subset
/// of which is infeasible. They are different questions, and when hunting a
/// modelling mistake the second is usually the more actionable one.
///
/// # Errors
///
/// Returns an error when the model has no variables or `HiGHS` refuses the
/// query.
pub fn iis(problem: &LpProblem) -> Result<Iis, String> {
    if problem.variables.is_empty() {
        return Err("the model has no variables".to_owned());
    }

    let started = Instant::now();
    let (relaxed, relaxed_integrality) = relax_integrality(problem);
    let built = build_highs_model(&relaxed);
    let (variable_names, row_names) = (built.variable_names, built.row_constraint_names);
    let skipped_sos = built.skipped_sos;

    let mut model = built.row_problem.optimise(built.sense);
    model.make_quiet();
    model
        .try_set_option("iis_strategy", IIS_STRATEGY_FROM_LP)
        .map_err(|_| "HiGHS refused the iis_strategy option".to_owned())?;

    let mut solved = model.solve();
    let status = format!("{:?}", solved.status());

    let (num_col, num_row) = (variable_names.len(), row_names.len());
    let mut col_index = vec![0_i32; num_col];
    let mut row_index = vec![0_i32; num_row];
    let mut col_bound = vec![0_i32; num_col];
    let mut row_bound = vec![0_i32; num_row];
    let (mut iis_num_col, mut iis_num_row) = (0_i32, 0_i32);

    let highs = solved.as_mut_ptr();
    // Called once with the arrays at full size rather than twice to size them:
    // the counts are bounded by the model's own dimensions, and a second call
    // would recompute the entire IIS.
    //
    // SAFETY: `highs` is the live model owned by `solved` for the rest of this
    // function. Each array is at least as long as the count HiGHS can write to
    // it, and the two `col_status`/`row_status` outputs are passed as null,
    // which the C API explicitly checks for.
    let call = unsafe {
        highs_sys::Highs_getIis(
            highs,
            &raw mut iis_num_col,
            &raw mut iis_num_row,
            col_index.as_mut_ptr(),
            row_index.as_mut_ptr(),
            col_bound.as_mut_ptr(),
            row_bound.as_mut_ptr(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    if call == highs_sys::kHighsStatusError {
        return Err("HiGHS could not compute an irreducible infeasible subsystem for this model".to_owned());
    }

    let take = |count: i32, limit: usize| -> usize {
        let count = usize::try_from(count).unwrap_or(0);
        debug_assert!(count <= limit, "HiGHS reported {count} IIS entries for a model with {limit}");
        count.min(limit)
    };
    let (iis_cols, iis_rows) = (take(iis_num_col, num_col), take(iis_num_row, num_row));

    let gather = |count: usize, indices: &[i32], bounds: &[i32], names: &[String]| -> Vec<(String, &'static str)> {
        indices[..count]
            .iter()
            .zip(&bounds[..count])
            .filter_map(|(index, bound)| {
                let index = usize::try_from(*index).ok()?;
                Some((names.get(index)?.clone(), bound_status(*bound)))
            })
            .collect()
    };

    Ok(Iis {
        status,
        rows: gather(iis_rows, &row_index, &row_bound, &row_names),
        cols: gather(iis_cols, &col_index, &col_bound, &variable_names),
        relaxed_integrality,
        skipped_sos,
        duration: started.elapsed(),
    })
}

/// How far one quantity can move before the optimal basis changes.
#[derive(Debug, Clone)]
pub struct RangeEntry {
    pub name: String,
    /// The value the interval is centred on: for a column its objective
    /// coefficient, for a row its **activity** at the optimum.
    ///
    /// The row case is not the right-hand side. `HiGHS`'s row ranging brackets
    /// `row_value`, not the bound — the two coincide only when the row is
    /// binding, so pairing the interval with the rhs reports a range that does
    /// not contain its own value on every slack row.
    pub current: f64,
    /// Lowest value the basis survives, and the objective there.
    pub down: f64,
    pub down_objective: f64,
    /// Highest value the basis survives, and the objective there.
    pub up: f64,
    pub up_objective: f64,
}

impl RangeEntry {
    /// Whether the current value sits inside the reported interval. A range
    /// that does not contain its own value means the basis is degenerate there.
    #[must_use]
    pub fn contains_current(&self) -> bool {
        self.down <= self.current && self.current <= self.up
    }
}

/// Sensitivity of the optimum to the model's coefficients.
#[derive(Debug, Clone)]
pub struct Ranging {
    /// Objective-coefficient ranges, one per variable.
    pub costs: Vec<RangeEntry>,
    /// Row-activity ranges, one per constraint. See [`RangeEntry::current`]:
    /// these bracket each row's activity, not its right-hand side.
    pub rows: Vec<RangeEntry>,
    pub objective_value: Option<f64>,
    pub relaxed_integrality: usize,
    pub skipped_sos: usize,
    pub duration: Duration,
}

/// Ranging information from the optimal basis: how far each objective
/// coefficient and each row activity can move before the basis changes.
///
/// This is the neighbourhood the what-if prompt (`E`) explores one point at a
/// time by editing a value and re-solving: ranging gives the whole interval at
/// once, from the basis, with no further solve.
///
/// # Errors
///
/// Returns an error when the model has no variables, does not solve to
/// optimality (ranging needs a basis), or `HiGHS` refuses the query.
pub fn ranging(problem: &LpProblem) -> Result<Ranging, String> {
    if problem.variables.is_empty() {
        return Err("the model has no variables".to_owned());
    }

    let started = Instant::now();
    let (relaxed, relaxed_integrality) = relax_integrality(problem);
    let built = build_highs_model(&relaxed);
    let (variable_names, row_names) = (built.variable_names, built.row_constraint_names);
    let skipped_sos = built.skipped_sos;
    let costs_by_id = primary_objective_coefficients(&relaxed);

    let mut model = built.row_problem.optimise(built.sense);
    model.make_quiet();

    let mut solved = model.solve();
    let status = format!("{:?}", solved.status());
    if status != "Optimal" {
        return Err(format!("ranging needs an optimal basis; this model solved as {status}"));
    }

    // Both are read here, before the raw pointer is taken: `get_solution`
    // borrows `solved`, and the row activities are what the row ranging below
    // is centred on.
    let (objective_value, row_activities) = {
        let solution = solved.get_solution();
        let objective = variable_names
            .iter()
            .zip(solution.columns())
            .filter_map(|(name, value)| relaxed.name_id(name).and_then(|id| costs_by_id.get(&id)).map(|cost| cost * value))
            .sum();
        (Some(objective), solution.rows().to_vec())
    };
    debug_assert_eq!(row_activities.len(), row_names.len(), "one activity per row");

    let (num_col, num_row) = (variable_names.len(), row_names.len());
    let mut cost_up = vec![0.0_f64; num_col];
    let mut cost_up_objective = vec![0.0_f64; num_col];
    let mut cost_down = vec![0.0_f64; num_col];
    let mut cost_down_objective = vec![0.0_f64; num_col];
    let mut rhs_up = vec![0.0_f64; num_row];
    let mut rhs_up_objective = vec![0.0_f64; num_row];
    let mut rhs_down = vec![0.0_f64; num_row];
    let mut rhs_down_objective = vec![0.0_f64; num_row];

    let highs = solved.as_mut_ptr();
    // Sixteen of the twenty-four outputs are passed as null: the C API
    // null-checks each one, and the "in/out variable" indices and the column
    // *bound* ranges are not shown, so allocating them would be waste
    // proportional to the model size.
    //
    // SAFETY: `highs` is the live model owned by `solved` for the rest of this
    // function. Each non-null array is exactly `num_col` or `num_row` doubles,
    // the lengths the C API documents for its position.
    let call = unsafe {
        highs_sys::Highs_getRanging(
            highs,
            cost_up.as_mut_ptr(),
            cost_up_objective.as_mut_ptr(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            cost_down.as_mut_ptr(),
            cost_down_objective.as_mut_ptr(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            // Column bound ranging: not shown.
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            rhs_up.as_mut_ptr(),
            rhs_up_objective.as_mut_ptr(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            rhs_down.as_mut_ptr(),
            rhs_down_objective.as_mut_ptr(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    if call == highs_sys::kHighsStatusError {
        return Err("HiGHS could not compute ranging information for this model".to_owned());
    }

    let costs = variable_names
        .iter()
        .enumerate()
        .map(|(index, name)| RangeEntry {
            name: name.clone(),
            current: relaxed.name_id(name).and_then(|id| costs_by_id.get(&id)).copied().unwrap_or(0.0),
            down: cost_down[index],
            down_objective: cost_down_objective[index],
            up: cost_up[index],
            up_objective: cost_up_objective[index],
        })
        .collect();

    let rows = row_names
        .iter()
        .enumerate()
        .map(|(index, name)| RangeEntry {
            name: name.clone(),
            current: row_activities.get(index).copied().unwrap_or(f64::NAN),
            down: rhs_down[index],
            down_objective: rhs_down_objective[index],
            up: rhs_up[index],
            up_objective: rhs_up_objective[index],
        })
        .collect();

    Ok(Ranging { costs, rows, objective_value, relaxed_integrality, skipped_sos, duration: started.elapsed() })
}

/// Diagnose an unbounded model: which variables run to infinity, and how fast.
///
/// Presolve is turned off for this solve. Presolve can conclude
/// `UnboundedOrInfeasible` without producing a ray, which is exactly the answer
/// this is trying to avoid; `allow_unbounded_or_infeasible` is left at its
/// `false` default so `HiGHS` resolves the ambiguity rather than reporting it.
///
/// # Errors
///
/// Returns an error when the model has no variables or `HiGHS` refuses the
/// ray query outright.
pub fn unbounded_ray(problem: &LpProblem) -> Result<UnboundedRay, String> {
    if problem.variables.is_empty() {
        return Err("the model has no variables".to_owned());
    }

    let started = Instant::now();
    let (relaxed, relaxed_integrality) = relax_integrality(problem);
    let built = build_highs_model(&relaxed);
    let variable_names = built.variable_names;
    let skipped_sos = built.skipped_sos;
    let costs = primary_objective_coefficients(&relaxed);

    let mut model = built.row_problem.optimise(built.sense);
    // The solver log would otherwise land in the terminal underneath the TUI.
    model.make_quiet();
    // A presolve that stops at "unbounded or infeasible" has no ray to give.
    model.try_set_option("presolve", "off").map_err(|_| "HiGHS refused to disable presolve".to_owned())?;

    let mut solved = model.solve();
    let status = format!("{:?}", solved.status());

    let mut directions = Vec::new();
    let mut objective_rate = 0.0;
    let mut has_ray = 0_i32;

    if crate::solver::status_is_unbounded(&status) {
        let mut ray = vec![0.0_f64; variable_names.len()];
        let highs = solved.as_mut_ptr();
        // SAFETY: `highs` is the live model owned by `solved` for the rest of
        // this function; `ray` is `num_col` doubles, the length the C API
        // documents, and `has_ray` is a single initialised `HighsInt`.
        let call = unsafe { highs_sys::Highs_getPrimalRay(highs, &raw mut has_ray, ray.as_mut_ptr()) };
        if call == highs_sys::kHighsStatusError {
            return Err("HiGHS could not compute a primal ray for this model".to_owned());
        }

        if has_ray != 0 {
            debug_assert_eq!(ray.len(), variable_names.len(), "the ray has one component per column");
            for (name, component) in variable_names.iter().zip(&ray) {
                if component.abs() <= ZERO {
                    continue;
                }
                let id = relaxed.name_id(name);
                let cost = id.and_then(|id| costs.get(&id)).copied().unwrap_or(0.0);
                let (_, lower, upper) = variable_bounds(id.and_then(|id| relaxed.variables.get(&id)));
                objective_rate += cost * component;
                directions.push(RayEntry { name: name.clone(), ray: *component, cost, lower, upper });
            }
            directions.sort_by(|a, b| {
                b.ray.abs().partial_cmp(&a.ray.abs()).unwrap_or(std::cmp::Ordering::Equal).then_with(|| a.name.cmp(&b.name))
            });
        }
    }

    // Gated on `directions`, not on `has_ray`: HiGHS can report a ray it did not
    // actually write (its own guard against that is a debug-only assert), which
    // leaves an all-zero vector and an empty `directions`. Keying off the thing
    // the pane renders means the two can never disagree about whether a
    // certificate was produced.
    let suspects = if crate::solver::status_is_unbounded(&status) && directions.is_empty() { suspects(&relaxed) } else { Vec::new() };

    Ok(UnboundedRay {
        status,
        directions,
        objective_rate,
        suspects,
        relaxed_integrality,
        skipped_sos,
        duration: started.elapsed(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(source: &str) -> LpProblem {
        LpProblem::parse(source).expect("test fixture must parse")
    }

    /// `x` pays to increase and nothing stops it: the textbook unbounded model.
    const UNBOUNDED: &str = "Maximize\n obj: 3 x + 2 y\nSubject To\n c1: y <= 4\nEnd";

    #[test]
    fn the_runaway_variable_is_named() {
        let report = unbounded_ray(&parse(UNBOUNDED)).expect("an unbounded LP must diagnose");

        assert!(report.is_unbounded(), "status was {}", report.status);
        let named: Vec<&str> = report.directions.iter().map(|d| d.name.as_str()).collect();
        assert!(named.contains(&"x"), "x is the variable with no upper bound, got {named:?}");
        assert!(!named.contains(&"y"), "y is capped by c1 and cannot run, got {named:?}");
    }

    #[test]
    fn the_ray_improves_the_objective() {
        let report = unbounded_ray(&parse(UNBOUNDED)).expect("an unbounded LP must diagnose");

        // Maximising, so moving along the ray must increase the objective.
        assert!(report.objective_rate > 0.0, "objective rate was {}", report.objective_rate);
        let x = report.directions.iter().find(|d| d.name == "x").expect("x must be on the ray");
        assert!(x.ray > 0.0, "x must run upward, got {}", x.ray);
        assert!(x.upper.is_infinite(), "x's missing upper bound is the cause");
    }

    #[test]
    fn a_bounded_model_reports_no_ray_and_no_suspects() {
        let report = unbounded_ray(&parse("Maximize\n obj: 3 x\nSubject To\n c1: x <= 4\nEnd")).expect("a bounded LP must still report");

        assert!(!report.is_unbounded(), "status was {}", report.status);
        assert!(report.directions.is_empty(), "a bounded model has no ray");
        assert!(report.suspects.is_empty(), "guessing on a bounded model would be noise");
    }

    #[test]
    fn suspects_name_the_variable_the_objective_pushes_off_the_end() {
        // The heuristic on its own, independent of whether HiGHS produced a ray.
        // `y` carries no explicit upper bound either, but `c1` caps it, so only
        // `x` may be named — this is the filter that keeps the list usable.
        let found = suspects(&parse(UNBOUNDED));

        let named: Vec<&str> = found.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(named, vec!["x"], "y is capped by c1 and must not be suspected");
        assert_eq!(found[0].direction, "up", "maximising a positive cost pushes upward");
    }

    #[test]
    fn suspects_follow_the_sense_of_the_objective() {
        // Minimising a positive cost pushes downward, so what has to stop it is
        // a lower bound or a `>=` row. `x` has neither.
        let minimise = parse("Minimize\n obj: 3 x + y\nSubject To\n c1: x + y <= 4\nBounds\n x >= -infinity\nEnd");
        let found = suspects(&minimise);

        let named: Vec<&str> = found.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(named, vec!["x"], "y still has its default lower bound of 0");
        assert_eq!(found[0].direction, "down", "minimising a positive cost pushes downward");
    }

    #[test]
    fn a_row_that_holds_the_improving_direction_clears_the_suspicion() {
        // Same model, but `c2` now caps the downward direction, so `x` can no
        // longer run: the list must be empty rather than name it.
        let held = parse("Minimize\n obj: 3 x + y\nSubject To\n c1: x + y <= 4\n c2: x + y >= 1\nBounds\n x >= -infinity\nEnd");
        assert!(suspects(&held).is_empty(), "a `>=` row holds x from below");
    }

    #[test]
    fn a_negative_coefficient_reverses_which_side_a_row_holds() {
        // `-x <= 4` caps `x` from *below*, not above, so maximising `x` still
        // escapes upward.
        let problem = parse("Maximize\n obj: x\nSubject To\n c1: -x <= 4\nEnd");
        let found = suspects(&problem);

        let named: Vec<&str> = found.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(named, vec!["x"], "a negative coefficient in a <= row holds the other side");
        assert_eq!(found[0].direction, "up");
    }

    #[test]
    fn a_mip_is_relaxed_and_says_so() {
        let mip = parse("Maximize\n obj: 3 x + 2 y\nSubject To\n c1: y <= 4\nGeneral\n y\nEnd");
        let report = unbounded_ray(&mip).expect("a MIP must be relaxed, not refused");

        assert_eq!(report.relaxed_integrality, 1, "y's integrality was dropped");
        assert!(report.is_unbounded(), "the relaxation is still unbounded, got {}", report.status);
    }

    #[test]
    fn relaxing_leaves_the_original_model_untouched() {
        let mip = parse("Maximize\n obj: x\nSubject To\n c1: x <= 4\nGeneral\n x\nEnd");
        let (relaxed, count) = relax_integrality(&mip);

        assert_eq!(count, 1);
        let original = mip.variables.values().next().expect("one variable");
        assert!(original.kind.is_integer(), "the caller's model must not be mutated");
        assert!(!relaxed.variables.values().next().expect("one variable").kind.is_integer());
    }

    /// A real model, not just the inline fixtures: `afiro_ext` is a bounded MIP,
    /// so this also exercises the relaxation path end to end.
    #[test]
    fn a_real_bounded_model_reports_no_ray() {
        let source = std::fs::read_to_string("../rust/resources/afiro_ext.lp").expect("fixture must exist");
        let problem = LpProblem::parse(&source).expect("afiro must parse");
        let report = unbounded_ray(&problem).expect("a bounded model must still report");

        assert!(!report.is_unbounded(), "afiro is bounded, got {}", report.status);
        assert!(report.directions.is_empty(), "a bounded model has no ray");
        assert!(report.relaxed_integrality > 0, "afiro_ext is a MIP, so columns must have been relaxed");
    }

    #[test]
    fn the_conflicting_rows_are_named() {
        // c1 and c2 cannot both hold; c3 is satisfiable and must stay out.
        let problem = parse("Minimize\n obj: x + y\nSubject To\n c1: x >= 5\n c2: x <= 3\n c3: y >= 1\nEnd");
        let report = iis(&problem).expect("an infeasible LP must yield an IIS");

        let named: Vec<&str> = report.rows.iter().map(|(name, _)| name.as_str()).collect();
        assert!(named.contains(&"c1"), "c1 forces x up, got {named:?}");
        assert!(named.contains(&"c2"), "c2 forces x down, got {named:?}");
        assert!(!named.contains(&"c3"), "c3 is satisfiable and is not part of the conflict, got {named:?}");
    }

    /// The regression guard for `iis_strategy`. Under the default `Light`
    /// strategy `HiGHS` performs only trivial bound and empty-row checks, so a
    /// conflict that lives in the rows comes back empty.
    #[test]
    fn a_conflict_between_rows_is_found_not_just_trivial_bound_clashes() {
        // No single row and no variable bound is inconsistent on its own: the
        // infeasibility only appears when the two rows are taken together.
        let problem = parse("Minimize\n obj: x + y\nSubject To\n c1: x + y >= 10\n c2: x + y <= 2\nEnd");
        let report = iis(&problem).expect("an infeasible LP must yield an IIS");

        assert!(!report.is_empty(), "the row conflict must be found; iis_strategy is probably back at its Light default");
        let named: Vec<&str> = report.rows.iter().map(|(name, _)| name.as_str()).collect();
        assert!(named.contains(&"c1") && named.contains(&"c2"), "both rows belong to the conflict, got {named:?}");
    }

    #[test]
    fn a_feasible_model_yields_an_empty_subsystem_rather_than_a_wrong_one() {
        let report = iis(&parse("Minimize\n obj: x\nSubject To\n c1: x >= 1\n c2: x <= 5\nEnd")).expect("a feasible LP must report");

        assert!(report.is_empty(), "a feasible model has no irreducible infeasible subsystem");
    }

    #[test]
    fn conflicting_variable_bounds_are_reported_against_the_variable() {
        let problem = parse("Minimize\n obj: x\nSubject To\n c1: x + y >= 0\nBounds\n 5 <= x <= 3\nEnd");
        let report = iis(&problem).expect("conflicting bounds must diagnose");

        assert!(!report.is_empty(), "a bound conflict is an infeasible subsystem");
    }

    #[test]
    fn an_iis_never_exceeds_the_model_it_came_from() {
        let problem = parse("Minimize\n obj: x + y\nSubject To\n c1: x >= 5\n c2: x <= 3\n c3: y >= 1\nEnd");
        let report = iis(&problem).expect("an infeasible LP must yield an IIS");

        assert!(report.rows.len() <= problem.constraint_count(), "more IIS rows than the model has");
        assert!(report.cols.len() <= problem.variables.len(), "more IIS columns than the model has");
    }

    #[test]
    fn an_iis_on_a_model_with_no_variables_is_refused() {
        assert!(iis(&LpProblem::default()).is_err(), "an empty model has no subsystem to isolate");
    }

    #[test]
    fn bound_status_codes_map_to_their_highs_names() {
        assert_eq!(bound_status(-1), "dropped");
        assert_eq!(bound_status(2), "lower");
        assert_eq!(bound_status(3), "upper");
        assert_eq!(bound_status(4), "both");
    }

    /// A two-variable LP whose optimum and ranging can be checked by hand.
    const RANGING_LP: &str = "Maximize\n obj: 3 x + 2 y\nSubject To\n c1: x + y <= 4\n c2: x <= 3\nEnd";

    #[test]
    fn ranging_covers_every_row_and_column() {
        let report = ranging(&parse(RANGING_LP)).expect("an optimal LP must range");

        assert_eq!(report.costs.len(), 2, "one cost range per variable");
        assert_eq!(report.rows.len(), 2, "one rhs range per constraint");
        let named: Vec<&str> = report.rows.iter().map(|e| e.name.as_str()).collect();
        assert!(named.contains(&"c1") && named.contains(&"c2"), "rows must be named, got {named:?}");
    }

    #[test]
    fn a_cost_ranges_current_value_is_the_models_own_coefficient() {
        let report = ranging(&parse(RANGING_LP)).expect("an optimal LP must range");

        let x = report.costs.iter().find(|e| e.name == "x").expect("x must be ranged");
        assert!((x.current - 3.0).abs() < 1e-9, "x's cost is 3, got {}", x.current);
    }

    /// The regression guard for what a row range is centred on.
    ///
    /// `HiGHS` brackets a row's *activity*, not its right-hand side. The two
    /// coincide on a binding row, so a fixture where every row binds cannot
    /// tell the difference — `slack` here deliberately does not bind.
    #[test]
    fn a_row_range_is_centred_on_the_activity_not_the_right_hand_side() {
        // Optimum is x = 3, y = 1 (obj 3x + 2y maximised under x <= 3, x + y <= 4),
        // so `slack` has activity 3 against a right-hand side of 900.
        let problem = parse("Maximize\n obj: 3 x + 2 y\nSubject To\n c1: x + y <= 4\n c2: x <= 3\n slack: x <= 900\nEnd");
        let report = ranging(&problem).expect("an optimal LP must range");

        let slack = report.rows.iter().find(|e| e.name == "slack").expect("slack must be ranged");
        assert!(slack.current < 900.0, "the entry must carry the activity, not the rhs of 900, got {}", slack.current);
        assert!(
            slack.contains_current(),
            "a row range must bracket its own value: {} outside [{}, {}]",
            slack.current,
            slack.down,
            slack.up
        );
    }

    #[test]
    fn every_range_brackets_the_value_it_describes() {
        let report = ranging(&parse(RANGING_LP)).expect("an optimal LP must range");

        for entry in report.costs.iter().chain(&report.rows) {
            assert!(entry.down <= entry.up, "{}: down {} above up {}", entry.name, entry.down, entry.up);
            assert!(entry.contains_current(), "{}: {} outside [{}, {}]", entry.name, entry.current, entry.down, entry.up);
        }
    }

    #[test]
    fn an_infeasible_model_is_refused_because_there_is_no_basis_to_range() {
        let infeasible = parse("Minimize\n obj: x\nSubject To\n c1: x >= 5\n c2: x <= 3\nEnd");
        let error = ranging(&infeasible).expect_err("ranging needs an optimal basis");

        assert!(error.contains("optimal basis"), "the error should explain why, got: {error}");
    }

    #[test]
    fn an_unbounded_model_is_refused_too() {
        assert!(ranging(&parse(UNBOUNDED)).is_err(), "an unbounded model has no optimal basis");
    }

    #[test]
    fn ranging_a_model_with_no_variables_is_refused() {
        assert!(ranging(&LpProblem::default()).is_err(), "an empty model has nothing to range");
    }

    #[test]
    fn a_real_model_ranges_end_to_end() {
        let source = std::fs::read_to_string("../rust/resources/afiro_ext.lp").expect("fixture must exist");
        let problem = LpProblem::parse(&source).expect("afiro must parse");
        let report = ranging(&problem).expect("afiro's relaxation is optimal, so it must range");

        assert_eq!(report.costs.len(), problem.variables.len(), "one cost range per column");
        assert!(report.relaxed_integrality > 0, "afiro_ext is a MIP, so columns must have been relaxed");
        for entry in report.costs.iter().chain(&report.rows) {
            assert!(entry.down <= entry.up, "{}: down {} above up {}", entry.name, entry.down, entry.up);
        }
    }

    /// The analyses must survive the repo's largest models without panicking:
    /// `fit2d` alone has 10,500 columns, so it exercises every array the FFI
    /// calls write into at a size the small fixtures never reach.
    #[test]
    fn the_largest_models_are_handled_without_panicking() {
        for name in ["fit2d", "fit1d", "boeing1", "sudoku", "wbm"] {
            let Ok(source) = std::fs::read_to_string(format!("../rust/resources/{name}.lp")) else {
                continue;
            };
            let Ok(problem) = LpProblem::parse(&source) else { continue };

            let ray = unbounded_ray(&problem).unwrap_or_else(|e| panic!("{name}: ray failed: {e}"));
            assert!(ray.directions.len() <= problem.variables.len(), "{name}: more ray entries than columns");

            let subsystem = iis(&problem).unwrap_or_else(|e| panic!("{name}: iis failed: {e}"));
            assert!(subsystem.rows.len() <= problem.constraint_count(), "{name}: more IIS rows than the model has");
            assert!(subsystem.cols.len() <= problem.variables.len(), "{name}: more IIS columns than the model has");

            // These fixtures all solve, so ranging must cover every row and
            // column rather than returning a short table.
            let range = ranging(&problem).unwrap_or_else(|e| panic!("{name}: ranging failed: {e}"));
            assert_eq!(range.costs.len(), problem.variables.len(), "{name}: one cost range per column");
            for entry in range.costs.iter().chain(&range.rows) {
                assert!(entry.down <= entry.up, "{name}/{}: down {} above up {}", entry.name, entry.down, entry.up);
                // These models carry plenty of non-binding rows, which is
                // exactly where pairing a range with the wrong value shows up.
                assert!(
                    entry.contains_current(),
                    "{name}/{}: {} outside [{}, {}]",
                    entry.name,
                    entry.current,
                    entry.down,
                    entry.up
                );
            }
        }
    }

    #[test]
    fn a_model_with_no_variables_is_refused_rather_than_passed_to_highs() {
        assert!(unbounded_ray(&LpProblem::default()).is_err(), "an empty model has no ray to find");
    }
}
