#![cfg(feature = "lp-solvers")]

use std::path::PathBuf;

use lp_parser_rs::compat::lp_solvers::{LpSolversCompatError, ToLpSolvers};
use lp_parser_rs::parser::parse_file;
use lp_parser_rs::problem::LpProblem;
use lp_solvers::lp_format::{LpObjective, LpProblem as LpSolversProblem};

fn read_file_from_resources(file_name: &str) -> String {
    let mut file_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    file_path.push(format!("resources/{file_name}"));
    parse_file(&file_path).expect("failed to read file from resources")
}

#[test]
fn test_diet_file_converts_to_lp_solvers() {
    let content = read_file_from_resources("diet.lp");
    let problem = LpProblem::parse(&content).expect("failed to parse LP problem");

    assert_eq!(problem.objective_count(), 1);

    let compat = problem.to_lp_solvers().expect("failed to convert to lp-solvers format");

    assert!(compat.is_fully_compatible());
    assert_eq!(compat.name(), "diet");
    assert!(matches!(LpSolversProblem::sense(&compat), LpObjective::Minimize));
}

#[test]
fn test_afiro_file_rejects_multiple_objectives() {
    let content = read_file_from_resources("afiro.lp");
    let problem = LpProblem::parse(&content).expect("failed to parse LP problem");

    assert!(problem.objective_count() > 1);

    let result = problem.to_lp_solvers();
    assert!(matches!(result, Err(LpSolversCompatError::MultipleObjectives { .. })));
}

#[test]
fn test_sos_file_converts_with_warning() {
    let content = read_file_from_resources("sos.lp");
    let problem = LpProblem::parse(&content).expect("failed to parse LP problem");

    let compat = problem.to_lp_solvers().expect("failed to convert to lp-solvers format");

    assert!(!compat.is_fully_compatible());
    assert!(!compat.warnings().is_empty());
}

#[test]
fn test_lp_format_output() {
    let content = read_file_from_resources("diet.lp");
    let problem = LpProblem::parse(&content).expect("failed to parse LP problem");
    let compat = problem.to_lp_solvers().expect("failed to convert to lp-solvers format");

    let displayed = format!("{}", compat.display_lp());

    assert!(displayed.contains("Minimize"));
    assert!(displayed.contains("Subject To") || displayed.contains("st"));
}

#[test]
#[cfg(feature = "test-cbc")]
fn test_solve_with_cbc() {
    use lp_solvers::solvers::{CbcSolver, SolverTrait, Status};

    let content = read_file_from_resources("diet.lp");
    let problem = LpProblem::parse(&content).expect("failed to parse LP problem");
    let compat = problem.to_lp_solvers().expect("failed to convert to lp-solvers format");

    let solver = CbcSolver::new();
    let solution = solver.run(&compat).expect("solver failed");

    assert!(matches!(solution.status, Status::Optimal));
    assert!(!solution.results.is_empty());
}
