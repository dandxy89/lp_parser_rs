//! Would a different `HiGHS` configuration solve this model faster?
//!
//! The diagnostics pane ([`crate::diagnostics`]) reads telemetry out of *one*
//! solve: iterations, presolve reductions, run time. It can say the solve was
//! slow; it cannot say what to do about it. This runs the same model under a
//! handful of configurations and puts the times side by side, which turns
//! "elevated iteration count" into "the dual simplex finishes in half the time".
//!
//! The last preset is the one that justifies the module. The crate already owns
//! a rewrite pass ([`crate::presolve`]) whose whole purpose is to hand `HiGHS` an
//! easier model, and until now there was no way to tell whether it helped.
//! Rewriting and solving the result — paying the rewrite in the reported time —
//! answers that directly.
//!
//! # Why this runs sequentially
//!
//! Presets are solved one after another, not in parallel. Six solves competing
//! for the same cores inflate each other's wall-clock time by an amount that
//! depends on how many finish early, which is precisely the quantity being
//! measured. A parallel sweep is faster to run and useless to read. The whole
//! sweep happens on the analysis worker thread, so the UI stays responsive
//! either way.
//!
//! Later presets are capped by a time limit derived from the baseline solve, so
//! one pathological configuration cannot hold the sweep open indefinitely.

use std::time::{Duration, Instant};

use lp_parser_rs::problem::LpProblem;

use crate::diagnostics::parse_telemetry;
use crate::presolve::{DEFAULT_RULES, PresolveStats, presolve};
use crate::solver::solve_problem_with;

/// Relative objective gap above which a preset counts as having returned a
/// *different* answer rather than a faster one. A preset that changes the
/// optimum is a bug worth surfacing, not a speed-up worth taking.
const OBJECTIVE_TOLERANCE: f64 = 1e-6;

/// Floor for the per-preset time budget: below this, a model that simply solves
/// quickly would have its later presets cut off by rounding.
const MIN_BUDGET: Duration = Duration::from_secs(10);

/// How many times the baseline's own time each later preset is allowed. A
/// preset an order of magnitude slower than the default has made its point.
const BUDGET_FACTOR: u32 = 10;

/// One configuration to measure.
pub struct Preset {
    pub label: &'static str,
    /// `HiGHS` options applied on top of the defaults and `highs.opt`.
    pub options: &'static [(&'static str, &'static str)],
    /// Apply [`crate::presolve`]'s rewrite before solving, and charge the
    /// rewrite to this preset's time.
    pub rewrite: bool,
}

/// The sweep, in display order. The first entry is the baseline every other row
/// is compared against, so it must stay first and must stay unconfigured.
pub const PRESETS: [Preset; 6] = [
    Preset { label: "default", options: &[], rewrite: false },
    Preset { label: "presolve off", options: &[("presolve", "off")], rewrite: false },
    Preset { label: "simplex dual", options: &[("simplex_strategy", "1")], rewrite: false },
    Preset { label: "simplex primal", options: &[("simplex_strategy", "4")], rewrite: false },
    Preset { label: "ipm", options: &[("solver", "ipm")], rewrite: false },
    Preset { label: "local presolve", options: &[], rewrite: true },
];

/// What one preset achieved.
#[derive(Debug, Clone)]
pub struct Measurement {
    pub status: String,
    pub objective: Option<f64>,
    /// `HiGHS`'s own solve time, excluding model build and extraction.
    pub solve_time: Duration,
    /// Everything this preset cost: rewrite (if any) plus build, solve, extract.
    pub total_time: Duration,
    /// Simplex iterations, scraped from the solver log.
    pub iterations: Option<u64>,
    /// The objective differs from the baseline's beyond [`OBJECTIVE_TOLERANCE`].
    pub differs: bool,
}

/// One row of the sweep: a preset and either its measurement or why it failed.
#[derive(Debug, Clone)]
pub struct Run {
    pub label: &'static str,
    pub outcome: Result<Measurement, String>,
}

/// The finished sweep.
#[derive(Debug, Clone)]
pub struct Profile {
    pub runs: Vec<Run>,
    /// The baseline objective every row is compared against, if it solved.
    pub baseline: Option<f64>,
    /// Index into `runs` of the fastest preset that reached an optimal solution.
    pub fastest: Option<usize>,
    /// What the rewrite did, for the `local presolve` row.
    pub rewrite: Option<PresolveStats>,
    /// The time limit applied to every preset after the baseline.
    pub budget: Duration,
    pub duration: Duration,
}

impl Profile {
    /// Speed-up of the fastest preset over the baseline, when both solved and
    /// the fastest is not itself the baseline.
    #[must_use]
    pub fn speedup(&self) -> Option<(&'static str, f64)> {
        let fastest = self.fastest?;
        if fastest == 0 {
            return None;
        }
        let baseline = self.runs.first()?.outcome.as_ref().ok()?.total_time.as_secs_f64();
        let best = self.runs.get(fastest)?.outcome.as_ref().ok()?.total_time.as_secs_f64();
        (best > 0.0 && baseline > 0.0).then(|| (self.runs[fastest].label, baseline / best))
    }
}

/// Whether two objective values disagree by more than [`OBJECTIVE_TOLERANCE`],
/// relatively for large values and absolutely near zero.
fn objective_differs(baseline: Option<f64>, candidate: Option<f64>) -> bool {
    match (baseline, candidate) {
        (Some(a), Some(b)) => (a - b).abs() > OBJECTIVE_TOLERANCE * a.abs().max(b.abs()).max(1.0),
        // A preset that found no objective where the baseline did (or the
        // reverse) has certainly not agreed with it.
        (Some(_), None) | (None, Some(_)) => true,
        (None, None) => false,
    }
}

/// Run one preset and reduce its solve to a [`Measurement`].
///
/// The `SolveResult` is dropped before returning: on a large model each one
/// carries a value and a dual for every row and column, and the sweep needs
/// four numbers from it.
fn measure(
    problem: &LpProblem,
    preset: &Preset,
    budget: Option<Duration>,
    baseline: Option<f64>,
) -> Result<(Measurement, Option<PresolveStats>), String> {
    let started = Instant::now();

    let mut options: Vec<(String, String)> = preset.options.iter().map(|(key, value)| ((*key).to_owned(), (*value).to_owned())).collect();
    if let Some(budget) = budget {
        options.push(("time_limit".to_owned(), format!("{:.3}", budget.as_secs_f64())));
    }
    let borrowed: Vec<(&str, &str)> = options.iter().map(|(key, value)| (key.as_str(), value.as_str())).collect();

    // The rewrite is charged to this preset: handing `HiGHS` an easier model is
    // only a win if it beats the default including the cost of producing it.
    let (rewritten, stats) = if preset.rewrite {
        let (rewritten, stats) = presolve(problem, DEFAULT_RULES);
        (Some(rewritten), Some(stats))
    } else {
        (None, None)
    };
    let target = rewritten.as_ref().unwrap_or(problem);
    if target.variables.is_empty() {
        return Err("the rewrite removed every variable".to_owned());
    }

    let result = solve_problem_with(target, &borrowed)?;
    let measurement = Measurement {
        status: result.status.clone(),
        objective: result.objective_value,
        solve_time: result.solve_time,
        total_time: started.elapsed(),
        iterations: parse_telemetry(&result.solver_log).iterations,
        differs: objective_differs(baseline, result.objective_value),
    };
    Ok((measurement, stats))
}

/// Solve `problem` under every [`PRESETS`] entry and compare.
///
/// # Errors
///
/// Returns an error only when the model cannot be solved at all (no variables).
/// A preset that fails on its own is recorded as a failed row, so one bad
/// configuration does not lose the rest of the sweep.
pub fn run_profile(problem: &LpProblem) -> Result<Profile, String> {
    if problem.variables.is_empty() {
        return Err("the model has no variables".to_owned());
    }
    debug_assert!(!PRESETS[0].rewrite && PRESETS[0].options.is_empty(), "the first preset is the baseline and must be unconfigured");

    let started = Instant::now();
    let mut runs: Vec<Run> = Vec::with_capacity(PRESETS.len());
    let mut rewrite: Option<PresolveStats> = None;

    // The baseline runs uncapped: it is the measurement every budget derives from.
    let mut baseline_run = measure(problem, &PRESETS[0], None, None);
    let baseline = baseline_run.as_ref().ok().and_then(|(m, _)| m.objective);
    // It is measured before its own objective is known, and it is the thing
    // every other row is compared against, so it cannot disagree with itself.
    if let Ok((measurement, _)) = &mut baseline_run {
        measurement.differs = false;
    }
    let budget = baseline_run.as_ref().ok().map_or(MIN_BUDGET, |(m, _)| (m.total_time * BUDGET_FACTOR).max(MIN_BUDGET));
    runs.push(Run { label: PRESETS[0].label, outcome: baseline_run.map(|(m, _)| m) });

    for preset in &PRESETS[1..] {
        let outcome = match measure(problem, preset, Some(budget), baseline) {
            Ok((measurement, stats)) => {
                if stats.is_some() {
                    rewrite = stats;
                }
                Ok(measurement)
            }
            Err(e) => Err(e),
        };
        runs.push(Run { label: preset.label, outcome });
    }

    debug_assert_eq!(runs.len(), PRESETS.len(), "every preset must produce a row");

    // Only an optimal solve is a fair candidate: a preset that hit the time
    // limit is fast precisely because it stopped early.
    let fastest = runs
        .iter()
        .enumerate()
        .filter(|(_, run)| run.outcome.as_ref().is_ok_and(|m| m.status == "Optimal" && !m.differs))
        .min_by_key(|(_, run)| run.outcome.as_ref().map_or(Duration::MAX, |m| m.total_time))
        .map(|(index, _)| index);

    Ok(Profile { runs, baseline, fastest, rewrite, budget, duration: started.elapsed() })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(source: &str) -> LpProblem {
        LpProblem::parse(source).expect("test fixture must parse")
    }

    /// A small but non-trivial LP: enough rows for the presets to differ, small
    /// enough that six solves stay quick.
    const TRANSPORT: &str = "Minimize\n obj: 3 x1 + 5 x2 + 4 x3 + 2 x4\n\
        Subject To\n supply1: x1 + x2 <= 40\n supply2: x3 + x4 <= 60\n\
        demand1: x1 + x3 >= 30\n demand2: x2 + x4 >= 50\nEnd";

    #[test]
    fn the_baseline_is_first_and_unconfigured() {
        // Every other row is compared against it, so a configured baseline
        // would silently rebase the whole sweep.
        assert_eq!(PRESETS[0].label, "default");
        assert!(PRESETS[0].options.is_empty(), "the baseline must apply no options");
        assert!(!PRESETS[0].rewrite, "the baseline must solve the model as written");
    }

    #[test]
    fn every_preset_produces_a_row_and_they_agree_on_the_optimum() {
        let problem = parse(TRANSPORT);
        let profile = run_profile(&problem).expect("a well-formed LP must profile");

        assert_eq!(profile.runs.len(), PRESETS.len(), "one row per preset");
        for (run, preset) in profile.runs.iter().zip(&PRESETS) {
            assert_eq!(run.label, preset.label, "rows must stay in preset order");
        }

        let baseline = profile.baseline.expect("the baseline must solve");
        for run in &profile.runs {
            let measurement = run.outcome.as_ref().unwrap_or_else(|e| panic!("preset {} failed: {e}", run.label));
            assert!(!measurement.differs, "preset {} returned objective {:?}, baseline was {baseline}", run.label, measurement.objective);
        }
    }

    #[test]
    fn the_fastest_preset_is_an_optimal_one() {
        let problem = parse(TRANSPORT);
        let profile = run_profile(&problem).expect("a well-formed LP must profile");

        let fastest = profile.fastest.expect("at least the baseline solves optimally");
        let measurement = profile.runs[fastest].outcome.as_ref().expect("the fastest row solved");
        assert_eq!(measurement.status, "Optimal", "a row that stopped early is not a win");
        assert!(!measurement.differs, "a row with a different optimum is not a win");
    }

    #[test]
    fn the_local_presolve_row_reports_what_the_rewrite_did() {
        let problem = parse(TRANSPORT);
        let profile = run_profile(&problem).expect("a well-formed LP must profile");

        assert!(profile.rewrite.is_some(), "the rewrite preset must record its stats");
        let row = profile.runs.last().expect("six presets");
        assert_eq!(row.label, "local presolve");
    }

    /// The sweep must survive a real model end to end, not just the inline
    /// fixtures: every preset applies, every row solves, all agree.
    #[test]
    fn a_real_model_profiles_end_to_end() {
        let source = std::fs::read_to_string("../rust/resources/afiro_ext.lp").expect("fixture must exist");
        let problem = LpProblem::parse(&source).expect("afiro must parse");
        let profile = run_profile(&problem).expect("afiro must profile");

        for run in &profile.runs {
            let measurement = run.outcome.as_ref().unwrap_or_else(|e| panic!("preset {} failed: {e}", run.label));
            assert!(!measurement.differs, "preset {} disagreed on the optimum", run.label);
        }
        // afiro_ext is a MIP, so this also pins that a MIP's iteration count
        // is read out of its differently-shaped solving report.
        let baseline = profile.runs[0].outcome.as_ref().expect("the baseline solved");
        assert!(baseline.iterations.is_some(), "a MIP's iteration count must be reported");
    }

    #[test]
    fn a_model_with_no_variables_is_refused_rather_than_solved() {
        let problem = LpProblem::default();
        assert!(run_profile(&problem).is_err(), "an empty model has nothing to profile");
    }

    #[test]
    fn a_differing_objective_is_flagged_relative_to_magnitude() {
        // Large values compare relatively: 1e9 and 1e9+1 are the same answer.
        assert!(!objective_differs(Some(1e9), Some(1e9 + 1.0)));
        assert!(objective_differs(Some(1e9), Some(1.1e9)));
        // Near zero the tolerance is absolute, so it does not collapse.
        assert!(!objective_differs(Some(0.0), Some(1e-9)));
        assert!(objective_differs(Some(0.0), Some(0.5)));
    }

    #[test]
    fn a_missing_objective_on_either_side_counts_as_a_disagreement() {
        assert!(objective_differs(Some(1.0), None));
        assert!(objective_differs(None, Some(1.0)));
        assert!(!objective_differs(None, None), "two presets that both failed have not disagreed");
    }

    #[test]
    fn the_budget_never_falls_below_the_floor() {
        let problem = parse(TRANSPORT);
        let profile = run_profile(&problem).expect("a well-formed LP must profile");
        // A tiny model solves in microseconds; ten times that must not become
        // a time limit later presets cannot meet.
        assert!(profile.budget >= MIN_BUDGET, "budget {:?} fell below the floor", profile.budget);
    }

    #[test]
    fn speedup_is_none_when_the_baseline_itself_wins() {
        let profile = Profile {
            runs: vec![Run {
                label: "default",
                outcome: Ok(Measurement {
                    status: "Optimal".to_owned(),
                    objective: Some(1.0),
                    solve_time: Duration::from_millis(1),
                    total_time: Duration::from_millis(1),
                    iterations: None,
                    differs: false,
                }),
            }],
            baseline: Some(1.0),
            fastest: Some(0),
            rewrite: None,
            budget: MIN_BUDGET,
            duration: Duration::from_millis(1),
        };
        assert!(profile.speedup().is_none(), "the baseline cannot be a speed-up over itself");
    }
}
