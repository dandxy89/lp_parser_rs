//! One `lp-highs` protocol request, end to end.

use std::collections::HashMap;
use std::ffi::CStr;
use std::fmt::Write as _;
use std::sync::atomic::AtomicBool;
use std::time::Instant;

use lp_parser_rs::model::{ComparisonOp, Constraint, VariableKind};
use lp_parser_rs::problem::LpProblem;

use crate::protocol::{
    ConstraintSolution, Diagnose, DiagnosisMethod, ErrorKind, Event, Format, Infeasibility, InfeasibleBound, InfeasibleConstraint,
    MAX_LOG_BYTES, PROTOCOL_VERSION, Phase, Side, Skipped, SolveOptions, SolveOutcome, SolveRequest, SolveResponse, SolverId, Timings,
    VariableSolution,
};
use crate::query;
use crate::solve::{self, SolvePhase, SolveResult};

/// A row is binding when `|slack| <= BINDING_TOLERANCE * max(1, |rhs|)`.
const BINDING_TOLERANCE: f64 = 1e-7;

/// The version of the linked `HiGHS` library.
#[must_use]
pub fn highs_version() -> String {
    // SAFETY: `Highs_version` takes no arguments and returns a pointer to a
    // static NUL-terminated string owned by the library.
    let version = unsafe { CStr::from_ptr(highs_sys::Highs_version()) };
    version.to_string_lossy().into_owned()
}

fn solver_id() -> SolverId {
    SolverId { name: "HiGHS".to_owned(), version: highs_version(), helper: format!("lp-highs {}", env!("CARGO_PKG_VERSION")) }
}

/// Run `request` and describe the outcome.
///
/// `on_event` is called as each [`Phase`] begins. Setting `cancel` interrupts a
/// running solve or diagnosis (the response then carries the `ReachedInterrupt`
/// status). Never panics on bad input: parse failures, unsupported models and
/// solver failures are all reported as an unsuccessful [`SolveResponse`].
pub fn run_request(request: &SolveRequest, cancel: &AtomicBool, mut on_event: impl FnMut(Event)) -> SolveResponse {
    if let Err(violation) = request.validate() {
        return SolveResponse::failure(ErrorKind::Protocol, violation.to_string());
    }

    let started = Instant::now();
    let mut emit = |phase: Phase| {
        let elapsed_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
        on_event(Event::Phase { phase, elapsed_ms });
    };

    emit(Phase::Parsing);
    let parsed = match request.format {
        Format::Lp => LpProblem::parse(&request.text),
        Format::Mps => LpProblem::parse_mps(&request.text),
    };
    let problem = match parsed {
        Ok(problem) => problem,
        Err(error) => return SolveResponse::failure(ErrorKind::ParseError, error.to_string()),
    };
    if problem.variables.is_empty() {
        return SolveResponse::failure(ErrorKind::UnsupportedModel, "the model has no variables");
    }
    if let Err(message) = solve::check_supported(&problem) {
        return SolveResponse::failure(ErrorKind::UnsupportedModel, message);
    }

    let extra = option_pairs(&request.options);
    let extra_refs: Vec<(&str, &str)> = extra.iter().map(|(key, value)| (*key, value.as_str())).collect();
    let solved = solve::solve_problem_observed(&problem, &extra_refs, Some(cancel), &mut |phase| {
        emit(match phase {
            SolvePhase::Building => Phase::Building,
            SolvePhase::Solving => Phase::Solving,
            SolvePhase::Extracting => Phase::Extracting,
        });
    });
    let result = match solved {
        Ok(result) => result,
        Err(message) => {
            let kind = if message.contains("MIQP") { ErrorKind::UnsupportedModel } else { ErrorKind::SolverFailed };
            return SolveResponse::failure(kind, message);
        }
    };

    let mut notes = String::new();
    let infeasibility = if request.diagnose == Diagnose::Auto && solve::status_is_infeasible(&result.status) {
        emit(Phase::Diagnosing);
        diagnose(&problem, cancel, &mut notes)
    } else {
        None
    };

    let mut log = result.solver_log.clone();
    log.push_str(&notes);
    SolveResponse::success(outcome(&problem, &result, infeasibility, request.include_log.then(|| log_tail(&log))))
}

/// The `HiGHS` options a request's [`SolveOptions`] set.
fn option_pairs(options: &SolveOptions) -> Vec<(&'static str, String)> {
    let mut pairs = Vec::new();
    if let Some(limit) = options.time_limit {
        pairs.push(("time_limit", limit.to_string()));
    }
    if let Some(gap) = options.mip_rel_gap {
        pairs.push(("mip_rel_gap", gap.to_string()));
    }
    if let Some(presolve) = options.presolve {
        pairs.push(("presolve", presolve.as_str().to_owned()));
    }
    if let Some(threads) = options.threads {
        pairs.push(("threads", threads.to_string()));
    }
    pairs
}

/// The last [`MAX_LOG_BYTES`] of `log`, cut on a character boundary.
fn log_tail(log: &str) -> String {
    if log.len() <= MAX_LOG_BYTES {
        return log.to_owned();
    }
    let mut start = log.len() - MAX_LOG_BYTES;
    while !log.is_char_boundary(start) {
        start += 1;
    }
    log[start..].to_owned()
}

fn is_mip(problem: &LpProblem, skipped_sos: usize) -> bool {
    skipped_sos > 0 || problem.variables.values().any(|variable| variable.kind != VariableKind::Continuous)
}

/// An IIS first; the elastic relaxation when the IIS fails, is empty, or the
/// model is a MIP. A failed diagnosis is recorded in `notes` (it lands in the
/// log) rather than failing the solve that found the infeasibility.
fn diagnose(problem: &LpProblem, cancel: &AtomicBool, notes: &mut String) -> Option<Infeasibility> {
    if !is_mip(problem, 0) {
        match query::iis(problem) {
            Ok(iis) if !iis.is_empty() => return Some(from_iis(&iis)),
            Ok(_) => notes.push_str("\n[lp-highs] the IIS was empty; falling back to elastic relaxation\n"),
            Err(error) => {
                write!(notes, "\n[lp-highs] IIS failed ({error}); falling back to elastic relaxation\n")
                    .expect("fmt::Write to String is infallible");
            }
        }
    }
    match solve::diagnose_infeasibility_cancellable(problem, cancel) {
        Ok(diagnosis) => Some(from_elastic(problem, &diagnosis)),
        Err(error) => {
            write!(notes, "\n[lp-highs] infeasibility diagnosis failed: {error}\n").expect("fmt::Write to String is infallible");
            None
        }
    }
}

fn side_of(status: &str) -> Option<Side> {
    match status {
        "lower" => Some(Side::Lower),
        "upper" => Some(Side::Upper),
        "free" => Some(Side::Free),
        "dropped" => None,
        // `both`, or in the subsystem without a bound of its own.
        _ => Some(Side::Boxed),
    }
}

fn from_iis(iis: &query::Iis) -> Infeasibility {
    Infeasibility {
        method: DiagnosisMethod::Iis,
        constraints: iis
            .rows
            .iter()
            .filter_map(|(name, status)| Some(InfeasibleConstraint { name: name.clone(), side: side_of(status)?, violation: None }))
            .collect(),
        bounds: iis
            .cols
            .iter()
            .filter_map(|(name, status)| Some(InfeasibleBound { variable: name.clone(), side: side_of(status)?, gap: None }))
            .collect(),
        total_violation: None,
    }
}

fn from_elastic(problem: &LpProblem, diagnosis: &solve::InfeasibilityDiagnosis) -> Infeasibility {
    let rows = row_data(problem);
    Infeasibility {
        method: DiagnosisMethod::Elastic,
        constraints: diagnosis
            .violations
            .iter()
            .map(|(name, violation)| {
                let side = match rows.get(name.as_str()).map(|row| row.0) {
                    Some(ComparisonOp::LTE | ComparisonOp::LT) => Side::Upper,
                    Some(ComparisonOp::GTE | ComparisonOp::GT) => Side::Lower,
                    Some(ComparisonOp::EQ) | None => Side::Boxed,
                };
                InfeasibleConstraint { name: name.clone(), side, violation: Some(*violation) }
            })
            .collect(),
        bounds: diagnosis
            .bound_conflicts
            .iter()
            .map(|(name, gap)| InfeasibleBound { variable: name.clone(), side: Side::Boxed, gap: Some(*gap) })
            .collect(),
        total_violation: Some(diagnosis.total_violation),
    }
}

/// `name -> (operator, rhs)` for every linear row.
fn row_data(problem: &LpProblem) -> HashMap<&str, (ComparisonOp, f64)> {
    problem
        .constraints
        .iter()
        .filter_map(|(id, constraint)| match constraint {
            Constraint::Standard { operator, rhs, .. } => Some((problem.resolve(*id), (*operator, *rhs))),
            _ => None,
        })
        .collect()
}

/// `slack` per the protocol: `<=` is `rhs - activity`, `>=` is `activity - rhs`,
/// `=` is `|activity - rhs|`.
fn slack_of(operator: ComparisonOp, rhs: f64, activity: f64) -> f64 {
    match operator {
        ComparisonOp::LTE | ComparisonOp::LT => rhs - activity,
        ComparisonOp::GTE | ComparisonOp::GT => activity - rhs,
        ComparisonOp::EQ => (activity - rhs).abs(),
    }
}

/// The relative gap from the MIP summary of a `HiGHS` log (`Gap   0% (...)`).
fn mip_gap(log: &str) -> Option<f64> {
    let line = log.lines().rev().map(str::trim).find(|line| line.starts_with("Gap"))?;
    let percent = line.split_whitespace().nth(1)?.strip_suffix('%')?.parse::<f64>().ok()?;
    (percent.is_finite() && percent >= 0.0).then_some(percent / 100.0)
}

fn millis(duration: std::time::Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

fn outcome(problem: &LpProblem, result: &SolveResult, infeasibility: Option<Infeasibility>, log: Option<String>) -> SolveOutcome {
    let mip = is_mip(problem, result.skipped_sos);
    let duals = !mip;
    let rows = row_data(problem);

    let variables = result
        .variables
        .iter()
        .enumerate()
        .map(|(index, (name, value))| VariableSolution {
            name: name.clone(),
            value: *value,
            reduced_cost: result.reduced_costs.get(index).filter(|_| duals).map(|(_, cost)| *cost),
        })
        .collect();

    let constraints = result
        .row_values
        .iter()
        .enumerate()
        .map(|(index, (name, activity))| {
            let (slack, binding) = rows.get(name.as_str()).map_or((0.0, false), |(operator, rhs)| {
                let slack = slack_of(*operator, *rhs, *activity);
                (slack, slack.abs() <= BINDING_TOLERANCE * rhs.abs().max(1.0))
            });
            ConstraintSolution {
                name: name.clone(),
                activity: *activity,
                dual: result.shadow_prices.get(index).filter(|_| duals).map(|(_, dual)| *dual),
                slack,
                binding,
            }
        })
        .collect();

    SolveOutcome {
        solver: solver_id(),
        status: result.status.clone(),
        objective_value: result.objective_value,
        mip_gap: if mip { mip_gap(&result.solver_log) } else { None },
        timings: Timings {
            build_ms: millis(result.build_time),
            solve_ms: millis(result.solve_time),
            extract_ms: millis(result.extract_time),
        },
        variables,
        constraints,
        skipped: Skipped { sos: result.skipped_sos },
        infeasibility,
        log,
    }
}

const _: () = assert!(PROTOCOL_VERSION == 1, "the response shapes above are protocol 1");

#[cfg(test)]
mod tests {
    use super::*;

    const KNAPSACK: &str = "Maximize\n obj: 3 x + 2 y\nSubject To\n cap: x + y <= 4\n lim: x + 3 y <= 6\nEnd";
    const INFEASIBLE: &str = "Minimize\n obj: x\nSubject To\n c1: x >= 2\n c2: x <= 1\nEnd";
    const INFEASIBLE_MIP: &str = "Minimize\n obj: x\nSubject To\n c1: x >= 2\n c2: x <= 1\nGenerals\n x\nEnd";

    fn run(text: &str) -> (SolveResponse, Vec<Phase>) {
        run_with(&SolveRequest::new(Format::Lp, text))
    }

    fn run_with(request: &SolveRequest) -> (SolveResponse, Vec<Phase>) {
        let mut phases = Vec::new();
        let response = run_request(request, &AtomicBool::new(false), |Event::Phase { phase, .. }| phases.push(phase));
        (response, phases)
    }

    fn outcome_of(response: SolveResponse) -> SolveOutcome {
        response.validate().expect("a consistent response");
        response.result.unwrap_or_else(|| panic!("expected a successful response, got {:?}", response.error))
    }

    #[test]
    fn an_optimal_lp_reports_values_duals_slack_and_phases() {
        let (response, phases) = run(KNAPSACK);
        let result = outcome_of(response);

        assert_eq!(result.status, "Optimal");
        assert!((result.objective_value.expect("objective") - 12.0).abs() < 1e-6, "{:?}", result.objective_value);
        assert_eq!(result.variables.iter().map(|v| v.name.as_str()).collect::<Vec<_>>(), ["x", "y"]);
        assert!(result.variables.iter().all(|v| v.reduced_cost.is_some()), "LPs carry reduced costs");

        let cap = result.constraints.iter().find(|c| c.name == "cap").expect("cap row");
        assert!(cap.binding && cap.slack.abs() < 1e-9, "cap is tight: {cap:?}");
        assert!(cap.dual.expect("dual") > 0.0);
        let lim = result.constraints.iter().find(|c| c.name == "lim").expect("lim row");
        assert!(!lim.binding && (lim.slack - 2.0).abs() < 1e-9, "lim has slack 2: {lim:?}");
        assert!(result.infeasibility.is_none());
        assert!(result.log.is_none(), "no log unless asked for");
        assert_eq!(result.solver.name, "HiGHS");
        assert_eq!(phases, [Phase::Parsing, Phase::Building, Phase::Solving, Phase::Extracting]);
    }

    #[test]
    fn a_mip_omits_duals_and_reduced_costs() {
        let (response, _) = run("Maximize\n obj: x + y\nSubject To\n c1: 2 x + 3 y <= 12\nGenerals\n x y\nEnd");
        let result = outcome_of(response);
        assert_eq!(result.status, "Optimal");
        assert!(result.variables.iter().all(|v| v.reduced_cost.is_none()));
        assert!(result.constraints.iter().all(|c| c.dual.is_none()));
    }

    #[test]
    fn the_log_is_returned_only_on_request() {
        let mut request = SolveRequest::new(Format::Lp, KNAPSACK);
        request.include_log = true;
        let log = outcome_of(run_with(&request).0).log.expect("the log was requested");
        assert!(log.contains("HiGHS"), "unexpected log: {log}");
    }

    #[test]
    fn options_reach_the_solver() {
        let mut request = SolveRequest::new(Format::Lp, KNAPSACK);
        request.options = SolveOptions {
            time_limit: Some(30.0),
            mip_rel_gap: Some(0.5),
            presolve: Some(crate::protocol::Presolve::Off),
            threads: Some(1),
        };
        assert_eq!(outcome_of(run_with(&request).0).status, "Optimal");
    }

    #[test]
    fn an_infeasible_lp_is_diagnosed_by_iis() {
        let (response, phases) = run(INFEASIBLE);
        let result = outcome_of(response);
        assert_eq!(result.status, "Infeasible");
        assert!(result.variables.is_empty() && result.constraints.is_empty(), "no solution arrays without a solution");

        let diagnosis = result.infeasibility.expect("diagnose defaults to auto");
        assert_eq!(diagnosis.method, DiagnosisMethod::Iis);
        let mut names: Vec<&str> = diagnosis.constraints.iter().map(|c| c.name.as_str()).collect();
        names.sort_unstable();
        assert_eq!(names, ["c1", "c2"], "both conflicting rows are named");
        assert_eq!(phases.last(), Some(&Phase::Diagnosing));
    }

    #[test]
    fn an_infeasible_mip_falls_back_to_elastic_relaxation() {
        let diagnosis = outcome_of(run(INFEASIBLE_MIP).0).infeasibility.expect("a MIP is diagnosed too");
        assert_eq!(diagnosis.method, DiagnosisMethod::Elastic);
        assert!(diagnosis.total_violation.expect("total violation") > 0.0);
        assert!(diagnosis.constraints.iter().any(|c| c.name == "c1" || c.name == "c2"), "{diagnosis:?}");
        assert!(diagnosis.constraints.iter().all(|c| c.violation.is_some()));
    }

    #[test]
    fn conflicting_bounds_are_reported_by_the_elastic_method() {
        let source = "Minimize\n obj: x + y\nSubject To\n c1: x + y >= 1\nBounds\n 5 <= y <= 2\nGenerals\n x\nEnd";
        let diagnosis = outcome_of(run(source).0).infeasibility.expect("diagnosed");
        assert_eq!(diagnosis.method, DiagnosisMethod::Elastic);
        let bound = diagnosis.bounds.iter().find(|b| b.variable == "y").expect("y's bounds conflict");
        assert!((bound.gap.expect("gap") - 3.0).abs() < 1e-9);
    }

    #[test]
    fn diagnose_never_skips_the_diagnosis() {
        let mut request = SolveRequest::new(Format::Lp, INFEASIBLE);
        request.diagnose = Diagnose::Never;
        let (response, phases) = run_with(&request);
        let result = outcome_of(response);
        assert_eq!(result.status, "Infeasible");
        assert!(result.infeasibility.is_none());
        assert!(!phases.contains(&Phase::Diagnosing));
    }

    #[test]
    fn a_set_cancel_flag_interrupts_the_solve() {
        let text = include_str!("../../rust/resources/boeing1.lp");
        let request = SolveRequest::new(Format::Lp, text);

        let uncancelled = run_request(&request, &AtomicBool::new(false), |_| {});
        assert_eq!(outcome_of(uncancelled).status, "Optimal");

        let cancelled = outcome_of(run_request(&request, &AtomicBool::new(true), |_| {}));
        assert_eq!(cancelled.status, "ReachedInterrupt");
        assert!(cancelled.objective_value.is_none());
        assert!(cancelled.infeasibility.is_none(), "an interrupted solve is not an infeasibility");
    }

    #[test]
    fn a_cancelled_diagnosis_is_not_reported_as_a_result() {
        let mut notes = String::new();
        let problem = LpProblem::parse(INFEASIBLE_MIP).expect("parses");
        assert!(diagnose(&problem, &AtomicBool::new(true), &mut notes).is_none());
        assert!(notes.contains("cancelled"), "the failure is recorded: {notes}");
    }

    #[test]
    fn mps_input_is_accepted() {
        let mps = "NAME          T\nROWS\n N  OBJ\n G  C1\nCOLUMNS\n    X         OBJ       1.0   C1        1.0\nRHS\n    RHS       C1        3.0\nENDATA\n";
        let (response, _) = run_with(&SolveRequest::new(Format::Mps, mps));
        let result = outcome_of(response);
        assert_eq!(result.status, "Optimal");
        assert!((result.objective_value.expect("objective") - 3.0).abs() < 1e-9);
    }

    #[test]
    fn failures_are_typed() {
        let (response, phases) = run("this is not an LP file");
        assert_eq!(response.error.as_ref().map(|e| e.kind), Some(ErrorKind::ParseError), "{response:?}");
        assert_eq!(phases, [Phase::Parsing]);

        let indicator = "Minimize\n obj: x\nSubject To\n c1: x >= 1\n ind: b = 1 -> x >= 2\nBinary\n b\nEnd";
        let (response, _) = run(indicator);
        let error = response.error.expect("indicator constraints are refused");
        assert_eq!(error.kind, ErrorKind::UnsupportedModel);
        assert!(error.message.contains("indicator constraint 'ind'"), "{}", error.message);

        let mut request = SolveRequest::new(Format::Lp, KNAPSACK);
        request.protocol = 9;
        assert_eq!(run_with(&request).0.error.map(|e| e.kind), Some(ErrorKind::Protocol));
    }

    #[test]
    fn a_log_tail_is_bounded_and_cut_on_a_character_boundary() {
        assert_eq!(log_tail("short"), "short");
        let long = "é".repeat(MAX_LOG_BYTES);
        let tail = log_tail(&long);
        assert!(tail.len() <= MAX_LOG_BYTES);
        assert!(tail.chars().all(|c| c == 'é'), "no split character");
    }

    #[test]
    fn the_mip_gap_is_read_from_the_summary() {
        let log = "Solving report\n  Status            Optimal\n  Gap               0.5% (tolerance: 0.01%)\n";
        assert_eq!(mip_gap(log), Some(0.005));
        assert_eq!(mip_gap("no summary"), None);
        assert_eq!(mip_gap("  Gap               inf\n"), None);
    }

    #[test]
    fn slack_follows_the_operator() {
        assert!((slack_of(ComparisonOp::LTE, 10.0, 7.0) - 3.0).abs() < 1e-12);
        assert!((slack_of(ComparisonOp::GTE, 10.0, 12.0) - 2.0).abs() < 1e-12);
        assert!((slack_of(ComparisonOp::EQ, 10.0, 9.0) - 1.0).abs() < 1e-12);
    }
}
