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

    // Only worth guessing when the model really is unbounded and HiGHS gave no
    // certificate: on a bounded model these would be pure noise.
    let suspects = if crate::solver::status_is_unbounded(&status) && has_ray == 0 { suspects(&relaxed) } else { Vec::new() };

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
    fn a_model_with_no_variables_is_refused_rather_than_passed_to_highs() {
        assert!(unbounded_ray(&LpProblem::default()).is_err(), "an empty model has no ray to find");
    }
}
