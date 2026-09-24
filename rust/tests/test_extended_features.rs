//! Parse, write and round-trip tests for the extended LP features.

use std::path::PathBuf;

use lp_parser_rs::model::{ComparisonOp, Constraint, ConstraintClass, ObjectiveAttributes, QuadraticTerm, VariableBounds, VariableKind};
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

// --- Indicator constraints ----------------------------------------------------

/// `(indicator variable, active value, coefficient count, operator, rhs)`.
fn indicator(problem: &LpProblem, name: &str) -> (String, bool, usize, ComparisonOp, f64) {
    let id = problem.name_id(name).unwrap_or_else(|| panic!("constraint '{name}' must exist"));
    match &problem.constraints[&id] {
        Constraint::Indicator { variable, active_value, coefficients, operator, rhs, .. } => {
            (problem.resolve(*variable).to_string(), *active_value, coefficients.len(), *operator, *rhs)
        }
        other => panic!("'{name}' must be an indicator constraint, got {other:?}"),
    }
}

#[test]
fn indicator_fixture_parses() {
    let problem = parse_resource("indicator.lp");
    assert_eq!(indicator(&problem, "cap_x"), ("b1".to_string(), true, 1, ComparisonOp::LTE, 3.0));
    assert_eq!(indicator(&problem, "cap_y"), ("b2".to_string(), false, 1, ComparisonOp::LTE, 0.0));
    // A flipped linear part is normalised like any other constraint.
    assert_eq!(indicator(&problem, "C1"), ("b1".to_string(), true, 1, ComparisonOp::GTE, 2.0));
    assert_eq!(indicator(&problem, "lazy_ind"), ("b2".to_string(), true, 2, ComparisonOp::LTE, 10.0));
    assert_eq!(class_of(&problem, "lazy_ind"), ConstraintClass::Lazy);
    assert_eq!(variable(&problem, "b1").0, VariableKind::Binary);
}

#[test]
fn indicator_lp_round_trip() {
    let problem = parse_resource("indicator.lp");
    let written = write_lp_string(&problem).unwrap();
    assert!(written.contains(" cap_x: b1 = 1 -> x <= 3"), "{written}");
    assert!(written.contains(" cap_y: b2 = 0 -> y <= 0"), "{written}");
    let reparsed = lp_round_trip(&problem);
    for name in ["cap_x", "cap_y", "C1", "lazy_ind"] {
        assert_eq!(indicator(&reparsed, name), indicator(&problem, name), "constraint {name}");
    }
    assert_eq!(class_of(&reparsed, "lazy_ind"), ConstraintClass::Lazy);
}

#[test]
fn indicator_mps_round_trip() {
    let problem = parse_resource("indicator.lp");
    let mps = write_mps_string(&problem).expect("indicator constraints are representable in MPS");
    assert!(mps.contains("INDICATORS\n IF cap_x"), "{mps}");
    let reparsed = LpProblem::parse_mps(&mps).expect("written MPS must re-parse");
    for name in ["cap_x", "cap_y", "C1", "lazy_ind"] {
        assert_eq!(indicator(&reparsed, name), indicator(&problem, name), "constraint {name}");
    }
    assert_eq!(class_of(&reparsed, "lazy_ind"), ConstraintClass::Lazy);
}

#[test]
fn indicator_errors() {
    // The value must be 0 or 1.
    assert!(LpProblem::parse("minimize\nobj: x\nsubject to\nc: b = 2 -> x <= 1\nend").is_err());
    // The linear part cannot be ranged.
    assert!(LpProblem::parse("minimize\nobj: x\nsubject to\nc: b = 1 -> 1 <= x <= 2\nend").is_err());
    // Nothing after the arrow.
    assert!(LpProblem::parse("minimize\nobj: x\nsubject to\nc: b = 1 ->\nend").is_err());
    // An arrow in the objective is not an indicator.
    assert!(LpProblem::parse("minimize\nobj: b = 1 -> x\nsubject to\nc: x <= 1\nend").is_err());

    // MPS: an indicator on a ranged row, an unknown row, a bad value.
    let base = "NAME t\nROWS\n N obj\n L c1\nCOLUMNS\n x obj 1 c1 1\n b obj 1\nRHS\n RHS c1 4\n";
    assert!(LpProblem::parse_mps(&format!("{base}INDICATORS\n IF c1 b 1\nENDATA\n")).is_ok());
    assert!(LpProblem::parse_mps(&format!("{base}RANGES\n RNG c1 2\nINDICATORS\n IF c1 b 1\nENDATA\n")).is_err());
    assert!(LpProblem::parse_mps(&format!("{base}INDICATORS\n IF nope b 1\nENDATA\n")).is_err());
    assert!(LpProblem::parse_mps(&format!("{base}INDICATORS\n IF c1 b 2\nENDATA\n")).is_err());
}

#[test]
fn indicator_variable_rename_and_remove() {
    let mut problem = parse_resource("indicator.lp");
    problem.rename_variable("b1", "switch").unwrap();
    assert_eq!(indicator(&problem, "cap_x").0, "switch");
    assert!(problem.remove_variable("switch").is_err(), "removing an indicator variable must be refused");
    problem.remove_variable("x").unwrap();
    assert_eq!(indicator(&problem, "cap_x").2, 0, "x is gone from the linear part");
}

#[cfg(feature = "diff")]
#[test]
fn indicator_changes_are_detected_by_diff() {
    let a = LpProblem::parse("minimize\nobj: x\nsubject to\nc: b = 1 -> x <= 3\nbinary\nb\nend").unwrap();
    let b = LpProblem::parse("minimize\nobj: x\nsubject to\nc: b = 0 -> x <= 4\nbinary\nb\nend").unwrap();
    let diff = a.diff(&b, &lp_parser_rs::diff::DiffOptions::default());
    assert_eq!(diff.cons_modified, vec![("c".to_string(), vec!["indicator b = 1 -> b = 0".to_string(), "rhs 3 -> 4".to_string()])]);

    let plain = LpProblem::parse("minimize\nobj: x\nsubject to\nc: x <= 3\nbinary\nb\nend").unwrap();
    let diff = a.diff(&plain, &lp_parser_rs::diff::DiffOptions::default());
    assert_eq!(diff.cons_modified, vec![("c".to_string(), vec!["constraint kind changed (Indicator <-> Standard)".to_string()])]);
}

#[test]
fn indicators_are_counted_by_analysis() {
    let analysis = parse_resource("indicator.lp").analyze();
    assert_eq!(analysis.constraints.type_distribution.indicator, 4);
    assert!(analysis.variables.unused_variables.is_empty(), "indicator variables count as used");
}

#[cfg(feature = "lp-solvers")]
#[test]
fn indicator_is_refused_by_lp_solvers_compat() {
    use lp_parser_rs::compat::lp_solvers::{LpSolversCompat, LpSolversCompatError};
    let problem = parse_resource("indicator.lp");
    let error = LpSolversCompat::try_new(&problem).expect_err("an indicator constraint cannot be dropped");
    assert!(matches!(error, LpSolversCompatError::UnsupportedConstraint { kind: "indicator", .. }), "{error:?}");
}

// --- Quadratic objectives and constraints ------------------------------------

/// Quadratic terms as `(var1, var2, coefficient)` with resolved names.
fn quadratic_terms(problem: &LpProblem, terms: &[QuadraticTerm]) -> Vec<(String, String, f64)> {
    terms.iter().map(|t| (problem.resolve(t.var1).to_string(), problem.resolve(t.var2).to_string(), t.coefficient)).collect()
}

fn objective_quadratic(problem: &LpProblem, name: &str) -> Vec<(String, String, f64)> {
    let id = problem.name_id(name).unwrap_or_else(|| panic!("objective '{name}' must exist"));
    quadratic_terms(problem, &problem.objectives[&id].quadratic)
}

/// `(linear coefficient count, quadratic terms, operator, rhs)` of a quadratic constraint.
fn quadratic_constraint(problem: &LpProblem, name: &str) -> (usize, Vec<(String, String, f64)>, ComparisonOp, f64) {
    let id = problem.name_id(name).unwrap_or_else(|| panic!("constraint '{name}' must exist"));
    match &problem.constraints[&id] {
        Constraint::Quadratic { coefficients, quadratic, operator, rhs, .. } => {
            (coefficients.len(), quadratic_terms(problem, quadratic), *operator, *rhs)
        }
        other => panic!("'{name}' must be a quadratic constraint, got {other:?}"),
    }
}

fn term(var1: &str, var2: &str, coefficient: f64) -> (String, String, f64) {
    (var1.to_string(), var2.to_string(), coefficient)
}

#[test]
fn quadratic_fixture_parses() {
    let problem = parse_resource("quadratic.lp");
    // `[ x ^ 2 + 4 x * y + 2 y ^ 2 ] / 2` halves every coefficient.
    assert_eq!(objective_quadratic(&problem, "obj"), vec![term("x", "x", 0.5), term("x", "y", 2.0), term("y", "y", 1.0)]);
    let obj = &problem.objectives[&problem.name_id("obj").unwrap()];
    assert_eq!((obj.coefficients.len(), obj.constant), (2, 1.0));

    assert_eq!(quadratic_constraint(&problem, "q1"), (1, vec![term("x", "x", 1.0), term("y", "y", 1.0)], ComparisonOp::LTE, 4.0));
    assert_eq!(quadratic_constraint(&problem, "q2"), (0, vec![term("x", "y", -1.0), term("z", "z", 3.0)], ComparisonOp::GTE, -2.0));
    // A flipped quadratic constraint is normalised like a linear one.
    assert_eq!(quadratic_constraint(&problem, "C1"), (1, vec![term("z", "x", 1.0)], ComparisonOp::LTE, 10.0));
    assert!(matches!(problem.constraints[&problem.name_id("c1").unwrap()], Constraint::Standard { .. }));
}

#[test]
fn quadratic_terms_of_the_same_pair_are_merged() {
    let problem = LpProblem::parse("minimize\nobj: [ x * y + y * x + x ^ 2 + x * x ] / 2\nsubject to\nc: x + y >= 1\nend").unwrap();
    assert_eq!(objective_quadratic(&problem, "obj"), vec![term("x", "y", 1.0), term("x", "x", 1.0)]);
}

#[test]
fn quadratic_lp_round_trip() {
    let problem = parse_resource("quadratic.lp");
    let written = write_lp_string(&problem).unwrap();
    // Objective coefficients are doubled back inside `[ ... ] / 2`.
    assert!(written.contains(" obj: 2 x + 3 y + [ x ^ 2 + 4 x * y + 2 y ^ 2 ] / 2 + 1"), "{written}");
    assert!(written.contains(" q1: x + [ x ^ 2 + y ^ 2 ] <= 4"), "{written}");
    assert!(written.contains(" q2: [ - x * y + 3 z ^ 2 ] >= -2"), "{written}");

    let reparsed = lp_round_trip(&problem);
    assert_eq!(objective_quadratic(&reparsed, "obj"), objective_quadratic(&problem, "obj"));
    for name in ["q1", "q2", "C1"] {
        assert_eq!(quadratic_constraint(&reparsed, name), quadratic_constraint(&problem, name), "constraint {name}");
    }
}

#[test]
fn quadratic_mps_round_trip() {
    let problem = parse_resource("quadratic.lp");
    let mps = write_mps_string(&problem).expect("quadratic terms are representable in MPS");
    assert!(mps.contains("QUADOBJ\n") && mps.contains("QCMATRIX   q1\n"), "{mps}");
    let reparsed = LpProblem::parse_mps(&mps).unwrap_or_else(|e| panic!("written MPS must re-parse: {e}\n{mps}"));
    assert_eq!(objective_quadratic(&reparsed, "obj"), objective_quadratic(&problem, "obj"));
    for name in ["q1", "q2", "C1"] {
        assert_eq!(quadratic_constraint(&reparsed, name), quadratic_constraint(&problem, name), "constraint {name}");
    }
}

#[test]
fn mps_quadratic_sections_follow_their_conventions() {
    // QUADOBJ is the upper triangle of Q in 1/2 x'Qx, QMATRIX the full Q, and
    // QCMATRIX the full Q in x'Qx: all three describe x^2 + 3 x y here.
    let base = "NAME t\nROWS\n N obj\n L c1\nCOLUMNS\n x obj 1 c1 1\n y obj 1 c1 1\nRHS\n RHS c1 4\n";
    let quadobj = LpProblem::parse_mps(&format!("{base}QUADOBJ\n x x 2\n x y 3\nENDATA\n")).unwrap();
    let qmatrix = LpProblem::parse_mps(&format!("{base}QMATRIX\n x x 2\n x y 3\n y x 3\nENDATA\n")).unwrap();
    for problem in [&quadobj, &qmatrix] {
        assert_eq!(objective_quadratic(problem, "obj"), vec![term("x", "x", 1.0), term("x", "y", 3.0)]);
    }
    let qc = LpProblem::parse_mps(&format!("{base}QCMATRIX c1\n x x 1\n x y 1.5\n y x 1.5\nENDATA\n")).unwrap();
    assert_eq!(quadratic_constraint(&qc, "c1"), (2, vec![term("x", "x", 1.0), term("x", "y", 3.0)], ComparisonOp::LTE, 4.0));

    // Errors: an objective row, an unknown row, a ranged row, a bad entry.
    assert!(LpProblem::parse_mps(&format!("{base}QCMATRIX obj\n x x 1\nENDATA\n")).is_err());
    assert!(LpProblem::parse_mps(&format!("{base}QCMATRIX nope\n x x 1\nENDATA\n")).is_err());
    assert!(LpProblem::parse_mps(&format!("{base}RANGES\n RNG c1 2\nQCMATRIX c1\n x x 1\nENDATA\n")).is_err());
    assert!(LpProblem::parse_mps(&format!("{base}QUADOBJ\n x x\nENDATA\n")).is_err());
}

#[test]
fn quadratic_syntax_errors() {
    let parse = |body: &str| LpProblem::parse(&format!("minimize\n{body}\nend"));
    // An objective block must be divided by exactly 2.
    assert!(parse("obj: x + [ x ^ 2 ]\nsubject to\nc: x >= 1").is_err());
    assert!(parse("obj: x + [ x ^ 2 ] / 3\nsubject to\nc: x >= 1").is_err());
    // A constraint block must not be divided.
    assert!(parse("obj: x\nsubject to\nc: [ x ^ 2 ] / 2 <= 1").is_err());
    // Only squares, products, and non-empty, closed blocks.
    assert!(parse("obj: [ x ^ 3 ] / 2\nsubject to\nc: x >= 1").is_err());
    assert!(parse("obj: [ x y ] / 2\nsubject to\nc: x >= 1").is_err());
    assert!(parse("obj: [ ] / 2\nsubject to\nc: x >= 1").is_err());
    assert!(parse("obj: [ x ^ 2 / 2\nsubject to\nc: x >= 1").is_err());
    // No quadratic terms in a range or an indicator.
    assert!(parse("obj: x\nsubject to\nc: 1 <= [ x ^ 2 ] <= 4").is_err());
    assert!(parse("obj: x\nsubject to\nc: b = 1 -> [ x ^ 2 ] <= 4").is_err());
}

#[test]
fn quadratic_variables_rename_and_remove() {
    let mut problem = parse_resource("quadratic.lp");
    problem.rename_variable("y", "why").unwrap();
    assert_eq!(objective_quadratic(&problem, "obj")[1], term("x", "why", 2.0));
    assert_eq!(quadratic_constraint(&problem, "q1").1[1], term("why", "why", 1.0));
    problem.remove_variable("z").unwrap();
    // C1 loses its only quadratic term and becomes a linear constraint.
    assert!(matches!(problem.constraints[&problem.name_id("C1").unwrap()], Constraint::Standard { .. }));
    assert_eq!(quadratic_constraint(&problem, "q2").1, vec![term("x", "why", -1.0)]);
}

#[cfg(feature = "diff")]
#[test]
fn quadratic_changes_are_detected_by_diff() {
    let a = LpProblem::parse("minimize\nobj: x + [ x ^ 2 ] / 2\nsubject to\nq: [ x * y ] <= 1\nend").unwrap();
    let b = LpProblem::parse("minimize\nobj: x + [ 3 x ^ 2 ] / 2\nsubject to\nq: [ y * x + y ^ 2 ] <= 1\nend").unwrap();
    let diff = a.diff(&b, &lp_parser_rs::diff::DiffOptions::default());
    assert_eq!(diff.objs_modified, vec![("obj".to_string(), vec!["1 quadratic term change(s)".to_string()])]);
    // `x * y` and `y * x` are the same term; only `y ^ 2` is new.
    assert_eq!(diff.cons_modified, vec![("q".to_string(), vec!["1 quadratic term change(s)".to_string()])]);
}

#[test]
fn quadratic_terms_are_counted_by_analysis() {
    let analysis = parse_resource("quadratic.lp").analyze();
    assert_eq!(analysis.summary.quadratic_objective_terms, 3);
    assert_eq!(analysis.summary.quadratic_constraint_terms, 5);
    assert_eq!(analysis.constraints.type_distribution.quadratic, 3);
}

#[cfg(feature = "lp-solvers")]
#[test]
fn quadratic_is_refused_by_lp_solvers_compat() {
    use lp_parser_rs::compat::lp_solvers::{LpSolversCompat, LpSolversCompatError};
    let objective_only = LpProblem::parse("minimize\nobj: x + [ x ^ 2 ] / 2\nsubject to\nc: x >= 1\nend").unwrap();
    assert!(matches!(LpSolversCompat::try_new(&objective_only), Err(LpSolversCompatError::QuadraticObjective { .. })));
    let constraint = LpProblem::parse("minimize\nobj: x\nsubject to\nq: [ x ^ 2 ] <= 1\nend").unwrap();
    assert!(matches!(LpSolversCompat::try_new(&constraint), Err(LpSolversCompatError::UnsupportedConstraint { kind: "quadratic", .. })));
}

// --- General constraints ------------------------------------------------------

/// `(resultant, function keyword, arguments, constant)` of a general constraint.
fn general(problem: &LpProblem, name: &str) -> (String, &'static str, Vec<String>, Option<f64>) {
    let id = problem.name_id(name).unwrap_or_else(|| panic!("constraint '{name}' must exist"));
    match &problem.constraints[&id] {
        Constraint::General { resultant, function, .. } => (
            problem.resolve(*resultant).to_string(),
            function.keyword(),
            function.variables().iter().map(|v| problem.resolve(*v).to_string()).collect(),
            function.constant(),
        ),
        other => panic!("'{name}' must be a general constraint, got {other:?}"),
    }
}

fn names(names: &[&str]) -> Vec<String> {
    names.iter().map(ToString::to_string).collect()
}

#[test]
fn general_constraint_fixture_parses() {
    let problem = parse_resource("general_constraints.lp");
    assert_eq!(general(&problem, "gc_max"), ("r1".to_string(), "MAX", names(&["x1", "x2"]), Some(3.0)));
    assert_eq!(general(&problem, "gc_min"), ("r2".to_string(), "MIN", names(&["x1", "x2"]), Some(-1.5)));
    assert_eq!(general(&problem, "gc_abs"), ("r3".to_string(), "ABS", names(&["x1"]), None));
    assert_eq!(general(&problem, "gc_and"), ("b3".to_string(), "AND", names(&["b1", "b2"]), None));
    assert_eq!(general(&problem, "C1"), ("b4".to_string(), "OR", names(&["b1", "b2"]), None), "unnamed entries get a generated name");
    assert_eq!(problem.constraint_count(), 7);
}

#[test]
fn general_constraint_lp_round_trip() {
    let problem = parse_resource("general_constraints.lp");
    let written = write_lp_string(&problem).unwrap();
    assert!(written.contains("General Constraints\n gc_max: r1 = MAX ( x1 , x2 , 3 )"), "{written}");
    assert!(written.contains(" gc_min: r2 = MIN ( x1 , x2 , -1.5 )"), "{written}");
    let reparsed = lp_round_trip(&problem);
    for name in ["gc_max", "gc_min", "gc_abs", "gc_and", "C1"] {
        assert_eq!(general(&reparsed, name), general(&problem, name), "constraint {name}");
    }
}

#[test]
fn general_constraint_section_headers() {
    for header in ["General Constraints", "General Constrs", "Gen Cons", "GenConstrs"] {
        let source = format!("maximize\nobj: r\nsubject to\nc: x <= 4\n{header}\ng: r = ABS ( x )\nend");
        let problem = LpProblem::parse(&source).unwrap_or_else(|e| panic!("{header}: {e}"));
        assert_eq!(general(&problem, "g").1, "ABS", "{header}");
    }
    // `genconstrs` where a section cannot start is an ordinary name.
    let problem = LpProblem::parse("minimize\nobj: x + genconstrs\nsubject to\nc: x >= 1\nend").unwrap();
    assert!(problem.name_id("genconstrs").is_some_and(|id| problem.variables.contains_key(&id)));
}

#[test]
fn general_constraint_errors() {
    let parse = |entry: &str| LpProblem::parse(&format!("minimize\nobj: x\nsubject to\nc: x >= 1\ngeneral constraints\n{entry}\nend"));
    assert!(parse("g: r = ABS ( x , y )").is_err(), "ABS takes one variable");
    assert!(parse("g: r = AND ( b1 , 1 )").is_err(), "AND takes variables only");
    assert!(parse("g: r = MAX ( 3 )").is_err(), "MAX needs a variable");
    assert!(parse("g: r = PWL ( x )").is_err(), "unsupported function");
    assert!(parse("g: r = MAX ( x , y").is_err(), "unclosed argument list");
    assert!(parse("g: r MAX ( x )").is_err(), "missing '='");
    assert!(parse("g: r = MAX ( x y )").is_err(), "missing ','");

    // MPS cannot carry them, so the writer refuses rather than dropping them.
    let problem = parse_resource("general_constraints.lp");
    let error = write_mps_string(&problem).expect_err("general constraints are not representable in MPS");
    assert!(error.to_string().contains("gc_max"), "{error}");
}

#[test]
fn general_constraint_variables_rename_and_remove() {
    let mut problem = parse_resource("general_constraints.lp");
    problem.rename_variable("x1", "first").unwrap();
    assert_eq!(general(&problem, "gc_max").2, names(&["first", "x2"]));
    problem.rename_variable("r3", "absolute").unwrap();
    assert_eq!(general(&problem, "gc_abs").0, "absolute");
    assert!(problem.remove_variable("x2").is_err(), "an argument of a general constraint cannot be removed on its own");
    assert!(problem.set_constraint_class("gc_max", ConstraintClass::Lazy).is_err());
}

#[cfg(feature = "diff")]
#[test]
fn general_constraint_changes_are_detected_by_diff() {
    let a = LpProblem::parse("minimize\nobj: r\nsubject to\nc: x + y >= 1\ngenconstrs\ng: r = MAX ( x , y , 1 )\nend").unwrap();
    let b = LpProblem::parse("minimize\nobj: r\nsubject to\nc: x + y >= 1\ngenconstrs\ng: r = MAX ( x , y , 2 )\nend").unwrap();
    assert!(a.diff(&a, &lp_parser_rs::diff::DiffOptions::default()).is_empty());
    let diff = a.diff(&b, &lp_parser_rs::diff::DiffOptions::default());
    assert_eq!(diff.cons_modified, vec![("g".to_string(), vec!["general constraint r = MAX (x, y, 1) -> r = MAX (x, y, 2)".to_string()])]);
}

#[test]
fn general_constraints_are_counted_by_analysis() {
    let analysis = parse_resource("general_constraints.lp").analyze();
    assert_eq!(analysis.constraints.type_distribution.general, 5);
}

#[cfg(feature = "lp-solvers")]
#[test]
fn general_constraint_is_refused_by_lp_solvers_compat() {
    use lp_parser_rs::compat::lp_solvers::{LpSolversCompat, LpSolversCompatError};
    let problem = parse_resource("general_constraints.lp");
    let error = LpSolversCompat::try_new(&problem).expect_err("a general constraint cannot be dropped");
    assert!(matches!(error, LpSolversCompatError::UnsupportedConstraint { kind: "general", .. }), "{error:?}");
}

// --- Multi-objective attributes -----------------------------------------------

fn attributes(problem: &LpProblem, name: &str) -> ObjectiveAttributes {
    let id = problem.name_id(name).unwrap_or_else(|| panic!("objective '{name}' must exist"));
    problem.objectives[&id].attributes
}

#[test]
fn multi_objective_fixture_parses() {
    let problem = parse_resource("multi_objective.lp");
    assert_eq!(problem.objective_count(), 3);
    assert_eq!(
        attributes(&problem, "Cost"),
        ObjectiveAttributes { priority: Some(2), weight: Some(1.0), abs_tol: Some(0.5), rel_tol: Some(0.01) }
    );
    assert_eq!(attributes(&problem, "Time"), ObjectiveAttributes { priority: Some(1), weight: Some(-0.5), abs_tol: None, rel_tol: None });
    assert!(attributes(&problem, "Plain").is_empty());
    let cost = &problem.objectives[&problem.name_id("Cost").unwrap()];
    assert_eq!(cost.coefficients.len(), 2, "the expression follows the attributes");
}

#[test]
fn multi_objective_lp_round_trip() {
    let problem = parse_resource("multi_objective.lp");
    let written = write_lp_string(&problem).unwrap();
    assert!(written.starts_with("Minimize multi-objectives\n"), "{written}");
    assert!(written.contains(" Cost: Priority=2 Weight=1 AbsTol=0.5 RelTol=0.01\n  3 x + 2 y"), "{written}");
    assert!(written.contains(" Time: Priority=1 Weight=-0.5\n"), "{written}");
    let reparsed = lp_round_trip(&problem);
    for name in ["Cost", "Time", "Plain"] {
        assert_eq!(attributes(&reparsed, name), attributes(&problem, name), "objective {name}");
    }

    // Without attributes the plain sense line is kept.
    let plain = LpProblem::parse("maximize\nobj: x\nsubject to\nc: x <= 1\nend").unwrap();
    assert!(write_lp_string(&plain).unwrap().contains("Maximize\n obj: x"));
}

#[test]
fn multi_objective_errors() {
    let parse = |objectives: &str| LpProblem::parse(&format!("minimize multi-objectives\n{objectives}\nsubject to\nc: x >= 1\nend"));
    assert!(parse("o: Priority=1.5\n x").is_err(), "priority must be an integer");
    assert!(parse("o: Colour=1\n x").is_err(), "unknown attribute");
    assert!(parse("o: Weight=1 Weight=2\n x").is_err(), "repeated attribute");
    assert!(parse("o: AbsTol=-1\n x").is_err(), "negative tolerance");
    assert!(parse("o: Priority=\n x").is_err(), "missing value");
    // Attributes need the `multi-objectives` marker.
    assert!(LpProblem::parse("minimize\no: Priority=1\n x\nsubject to\nc: x >= 1\nend").is_err());

    // MPS cannot carry the attributes, so the writer refuses unless told to
    // write the first objective alone.
    let single = LpProblem::parse("minimize multi-objectives\no: Priority=1\n x\nsubject to\nc: x >= 1\nend").unwrap();
    assert!(write_mps_string(&single).is_err());
    let options = lp_parser_rs::mps::writer::MpsWriterOptions { allow_multiple_objectives: true, ..Default::default() };
    assert!(lp_parser_rs::mps::writer::write_mps_string_with_options(&single, &options).is_ok());
}

#[cfg(feature = "diff")]
#[test]
fn multi_objective_attribute_changes_are_detected_by_diff() {
    let a = LpProblem::parse("minimize multi-objectives\no: Priority=2 Weight=1\n x\nsubject to\nc: x >= 1\nend").unwrap();
    let b = LpProblem::parse("minimize multi-objectives\no: Priority=1 RelTol=0.1\n x\nsubject to\nc: x >= 1\nend").unwrap();
    let diff = a.diff(&b, &lp_parser_rs::diff::DiffOptions::default());
    assert_eq!(
        diff.objs_modified,
        vec![(
            "o".to_string(),
            vec![
                "priority: Some(2) -> Some(1)".to_string(),
                "weight: Some(1.0) -> None".to_string(),
                "rel_tol: None -> Some(0.1)".to_string()
            ]
        )]
    );
}

#[cfg(feature = "serde")]
#[test]
fn multi_objective_attributes_survive_serde() {
    let problem = parse_resource("multi_objective.lp");
    let back: LpProblem = serde_json::from_str(&serde_json::to_string(&problem).unwrap()).unwrap();
    for name in ["Cost", "Time", "Plain"] {
        assert_eq!(attributes(&back, name), attributes(&problem, name), "objective {name}");
    }
}
