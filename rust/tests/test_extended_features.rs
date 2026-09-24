//! Parse, write and round-trip tests for the extended LP features.

use std::path::PathBuf;

use lp_parser_rs::model::{ConstraintClass, VariableBounds, VariableKind};
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

// --- Lazy constraints and user cuts ------------------------------------------

fn class_of(problem: &LpProblem, name: &str) -> ConstraintClass {
    let id = problem.name_id(name).unwrap_or_else(|| panic!("constraint '{name}' must exist"));
    assert!(problem.constraints.contains_key(&id), "constraint '{name}' must exist");
    problem.constraint_class(id)
}

#[test]
fn lazy_and_user_cut_fixture_parses() {
    let problem = parse_resource("lazy_user_cuts.lp");
    assert_eq!(problem.constraint_count(), 7, "the ranged l2 expands into two constraints");
    assert_eq!(class_of(&problem, "c1"), ConstraintClass::Normal);
    assert_eq!(class_of(&problem, "c2"), ConstraintClass::Normal);
    assert_eq!(class_of(&problem, "l1"), ConstraintClass::Lazy);
    assert_eq!(class_of(&problem, "l2"), ConstraintClass::Lazy);
    assert_eq!(class_of(&problem, "l2_rng"), ConstraintClass::Lazy);
    assert_eq!(class_of(&problem, "u1"), ConstraintClass::UserCut);
    assert_eq!(class_of(&problem, "C1"), ConstraintClass::UserCut, "an unnamed cut gets a generated name");
}

#[test]
fn lazy_and_user_cut_lp_round_trip() {
    let problem = parse_resource("lazy_user_cuts.lp");
    let written = write_lp_string(&problem).unwrap();
    let lazy_at = written.find("Lazy Constraints").expect("a Lazy Constraints section");
    let cuts_at = written.find("User Cuts").expect("a User Cuts section");
    assert!(written.find("Subject To").unwrap() < lazy_at && lazy_at < cuts_at, "{written}");
    assert!(cuts_at < written.find("Bounds").unwrap(), "{written}");

    let reparsed = lp_round_trip(&problem);
    for name in ["c1", "c2", "l1", "l2", "l2_rng", "u1", "C1"] {
        assert_eq!(class_of(&reparsed, name), class_of(&problem, name), "constraint {name}");
    }
}

#[test]
fn lazy_and_user_cut_mps_round_trip() {
    let problem = parse_resource("lazy_user_cuts.lp");
    let mps = write_mps_string(&problem).expect("lazy constraints and user cuts are representable in MPS");
    assert!(mps.contains("\nLAZYCONS\n") && mps.contains("\nUSERCUTS\n"), "{mps}");
    let reparsed = LpProblem::parse_mps(&mps).expect("written MPS must re-parse");
    assert_eq!(reparsed.constraint_count(), problem.constraint_count());
    for name in ["c1", "c2", "l1", "l2", "l2_rng", "u1", "C1"] {
        assert_eq!(class_of(&reparsed, name), class_of(&problem, name), "constraint {name}");
    }
    // The lazy range pair is written back as a single ranged LAZYCONS row.
    assert!(mps.contains("RANGES"), "{mps}");
}

#[test]
fn mps_lazycons_rejects_an_objective_row() {
    let mps = "NAME t\nROWS\n N obj\nLAZYCONS\n N other\nCOLUMNS\n x obj 1\nENDATA\n";
    assert!(LpProblem::parse_mps(mps).is_err());
}

#[test]
fn lazy_keyword_words_alone_are_names() {
    let problem = LpProblem::parse("minimize\nobj: lazy + cuts\nsubject to\nuser: lazy + cuts >= 1\nend").unwrap();
    assert_eq!(problem.variable_count(), 2);
    assert_eq!(class_of(&problem, "user"), ConstraintClass::Normal);
}

#[test]
fn constraint_class_follows_rename_and_remove() {
    let mut problem = parse_resource("lazy_user_cuts.lp");
    problem.rename_constraint("l1", "lazy_one").unwrap();
    assert_eq!(class_of(&problem, "lazy_one"), ConstraintClass::Lazy);
    problem.remove_constraint("lazy_one").unwrap();
    assert!(problem.constraint_classes.keys().all(|id| problem.constraints.contains_key(id)));
    problem.set_constraint_class("c1", ConstraintClass::UserCut).unwrap();
    assert_eq!(class_of(&problem, "c1"), ConstraintClass::UserCut);
    problem.set_constraint_class("c1", ConstraintClass::Normal).unwrap();
    assert_eq!(class_of(&problem, "c1"), ConstraintClass::Normal);
    assert!(problem.set_constraint_class("missing", ConstraintClass::Lazy).is_err());
}

#[cfg(feature = "diff")]
#[test]
fn constraint_class_change_is_detected_by_diff() {
    let lazy = LpProblem::parse("minimize\nobj: x\nsubject to\nc: x >= 1\nlazy constraints\nl: x <= 5\nend").unwrap();
    let normal = LpProblem::parse("minimize\nobj: x\nsubject to\nc: x >= 1\nl: x <= 5\nend").unwrap();
    let diff = lazy.diff(&normal, &lp_parser_rs::diff::DiffOptions::default());
    assert_eq!(diff.cons_modified, vec![("l".to_string(), vec!["class Lazy -> Normal".to_string()])]);
}

#[test]
fn lazy_and_user_cuts_are_counted_by_analysis() {
    let analysis = parse_resource("lazy_user_cuts.lp").analyze();
    assert_eq!(analysis.constraints.type_distribution.lazy, 3);
    assert_eq!(analysis.constraints.type_distribution.user_cuts, 2);
}

#[cfg(feature = "serde")]
#[test]
fn constraint_class_survives_serde() {
    let problem = parse_resource("lazy_user_cuts.lp");
    let json = serde_json::to_string(&problem).unwrap();
    let back: LpProblem = serde_json::from_str(&json).unwrap();
    for name in ["c1", "l1", "l2_rng", "u1", "C1"] {
        assert_eq!(class_of(&back, name), class_of(&problem, name), "constraint {name}");
    }
}
