//! Parse, write and round-trip tests for the extended LP features.

use std::path::PathBuf;

use lp_parser_rs::model::{VariableBounds, VariableKind};
use lp_parser_rs::mps::writer::write_mps_string;
use lp_parser_rs::problem::LpProblem;
use lp_parser_rs::writer::write_lp_string;

fn read_resource(file_name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources").join(file_name);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()))
}

fn parse_resource(file_name: &str) -> LpProblem {
    LpProblem::parse(&read_resource(file_name)).unwrap_or_else(|e| panic!("{file_name} must parse: {e}"))
}

/// Write `problem` to LP, parse it back, and check the second write is
/// identical to the first (the writer is a fixed point of parse then write).
fn lp_round_trip(problem: &LpProblem) -> LpProblem {
    let written = write_lp_string(problem).expect("problem must be writable as LP");
    let reparsed = LpProblem::parse(&written).unwrap_or_else(|e| panic!("written LP must re-parse: {e}\n---\n{written}"));
    let rewritten = write_lp_string(&reparsed).expect("re-parsed problem must be writable as LP");
    assert_eq!(written, rewritten, "LP writer must be a fixed point of parse/write");
    reparsed
}

fn variable(problem: &LpProblem, name: &str) -> (VariableKind, VariableBounds) {
    let id = problem.name_id(name).unwrap_or_else(|| panic!("variable '{name}' must exist"));
    let variable = &problem.variables[&id];
    (variable.kind, variable.bounds)
}

// --- Semi-integer variables ---------------------------------------------------

#[test]
fn semi_integer_fixture_parses() {
    let problem = parse_resource("semi_integer.lp");
    assert_eq!(variable(&problem, "x"), (VariableKind::SemiInteger, VariableBounds::range(2.0, 10.0)));
    assert_eq!(variable(&problem, "w"), (VariableKind::SemiInteger, VariableBounds::range(3.0, 9.0)));
    assert_eq!(variable(&problem, "z"), (VariableKind::General, VariableBounds::upper(4.0)));
    assert_eq!(variable(&problem, "y"), (VariableKind::Continuous, VariableBounds::unspecified()));
}

#[test]
fn semi_integer_lp_round_trip() {
    let problem = parse_resource("semi_integer.lp");
    let written = write_lp_string(&problem).unwrap();
    // CPLEX declares semi-integer by listing the variable in both sections.
    let generals = written.split("Generals").nth(1).expect("a Generals section").split("Semi-Continuous").next().unwrap();
    assert!(generals.contains(" x") && generals.contains(" w"), "{written}");

    let reparsed = lp_round_trip(&problem);
    for name in ["x", "y", "z", "w"] {
        assert_eq!(variable(&reparsed, name), variable(&problem, name), "variable {name}");
    }
}

#[test]
fn semi_integer_mps_round_trip() {
    let problem = parse_resource("semi_integer.lp");
    let mps = write_mps_string(&problem).expect("semi-integer variables are representable in MPS");
    assert!(mps.contains(" SI "), "semi-integer columns are written with the SI bound type:\n{mps}");
    let reparsed = LpProblem::parse_mps(&mps).expect("written MPS must re-parse");
    for name in ["x", "w"] {
        assert_eq!(variable(&reparsed, name), variable(&problem, name), "variable {name}");
    }
}

#[cfg(feature = "diff")]
#[test]
fn semi_integer_is_detected_by_diff() {
    let semi = LpProblem::parse("minimize\nobj: x\nsubject to\nc: x >= 1\nbounds\nx <= 5\ngenerals\nx\nsemi\nx\nend").unwrap();
    let general = LpProblem::parse("minimize\nobj: x\nsubject to\nc: x >= 1\nbounds\nx <= 5\ngenerals\nx\nend").unwrap();
    let diff = semi.diff(&general, &lp_parser_rs::diff::DiffOptions::default());
    assert_eq!(diff.vars_type_changed.len(), 1, "{diff:?}");
}

#[test]
fn semi_integer_is_counted_by_analysis() {
    let problem = parse_resource("semi_integer.lp");
    let analysis = problem.analyze();
    assert_eq!(analysis.variables.type_distribution.semi_integer, 2);
    // x, w (semi-integer) and z (general) are discrete.
    assert_eq!(analysis.variables.discrete_variable_count, 3);
}
