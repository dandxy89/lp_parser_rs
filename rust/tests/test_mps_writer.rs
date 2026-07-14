//! Round-trip tests for the MPS writer (`lp_parser_rs::mps::writer`).
//!
//! Two directions are exercised:
//! - MPS -> MPS: every fixture under `resources/mps/` is parsed, written back
//!   out, and re-parsed, asserting the structural shape is preserved.
//! - LP -> MPS: a selection of existing LP fixtures (already exercised by
//!   `test_from_file.rs`) are parsed, converted to MPS, and re-parsed.

// RHS values must round-trip bit-exactly at high decimal precision, so this
// suite intentionally compares floats strictly.
#![allow(clippy::float_cmp)]

use std::error::Error;
use std::fs;
use std::path::PathBuf;

use lp_parser_rs::model::{Constraint, VariableType};
use lp_parser_rs::mps::writer::{MpsWriterOptions, write_mps_string_with_options};
use lp_parser_rs::parser::parse_file;
use lp_parser_rs::problem::LpProblem;

/// High-precision options so RHS/coefficient round-trip assertions aren't
/// muddied by the writer's default 6-decimal-place rounding (a formatting
/// choice, not a correctness bug -- see `LpWriterOptions::decimal_precision`
/// for the equivalent LP-writer behaviour).
fn lossless_options() -> MpsWriterOptions {
    MpsWriterOptions { decimal_precision: 15, ..MpsWriterOptions::default() }
}

fn resource_path(relative: &str) -> PathBuf {
    let mut file_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    file_path.push("resources");
    file_path.push(relative);
    file_path
}

fn read_resource(relative: &str) -> Result<String, Box<dyn Error + 'static>> {
    Ok(parse_file(&resource_path(relative))?)
}

/// Assert that `reparsed` has the same structural shape as `original`:
/// sense, objective/constraint/variable counts, and per-variable bounds
/// (`VariableType`), per-constraint operator and RHS.
///
/// Matches variables/constraints by name rather than `NameId` since the two
/// problems come from independent interners.
fn assert_round_trip_preserves_structure(original: &LpProblem, reparsed: &LpProblem, context: &str) {
    assert_eq!(reparsed.sense, original.sense, "{context}: sense mismatch");
    assert_eq!(reparsed.objective_count(), original.objective_count(), "{context}: objective count mismatch");
    assert_eq!(reparsed.constraint_count(), original.constraint_count(), "{context}: constraint count mismatch");
    assert_eq!(reparsed.variable_count(), original.variable_count(), "{context}: variable count mismatch");

    for (name_id, var) in &original.variables {
        let name = original.resolve(*name_id);
        let reparsed_id = reparsed.name_id(name).unwrap_or_else(|| panic!("{context}: variable '{name}' missing after round trip"));
        let reparsed_var = &reparsed.variables[&reparsed_id];
        // Compare the semantic shape (integrality + effective feasible region)
        // rather than the lossy `VariableType` view. MPS cannot distinguish
        // `General` from `Integer`, and normalises a binary variable's redundant
        // explicit `[0, 1]` bounds away -- both preserve integrality and the
        // feasible region, which is what a structural round trip must keep.
        assert_eq!(
            reparsed_var.kind.is_integer(),
            var.kind.is_integer(),
            "{context}: integrality mismatch for variable '{name}' ({:?} vs {:?})",
            reparsed_var.kind,
            var.kind
        );
        let orig_bounds = (var.bounds.effective_lower(var.kind), var.bounds.effective_upper(var.kind));
        let reparsed_bounds =
            (reparsed_var.bounds.effective_lower(reparsed_var.kind), reparsed_var.bounds.effective_upper(reparsed_var.kind));
        assert_eq!(reparsed_bounds, orig_bounds, "{context}: effective bounds mismatch for variable '{name}'");
    }

    for (name_id, constraint) in &original.constraints {
        let name = original.resolve(*name_id);
        let reparsed_id = reparsed.name_id(name).unwrap_or_else(|| panic!("{context}: constraint '{name}' missing after round trip"));
        let reparsed_constraint =
            reparsed.constraints.get(&reparsed_id).unwrap_or_else(|| panic!("{context}: constraint '{name}' vanished"));

        match (constraint, reparsed_constraint) {
            (Constraint::Standard { operator: op1, rhs: rhs1, .. }, Constraint::Standard { operator: op2, rhs: rhs2, .. }) => {
                assert_eq!(op2, op1, "{context}: operator mismatch for constraint '{name}'");
                assert_eq!(rhs2, rhs1, "{context}: RHS mismatch for constraint '{name}'");
            }
            (Constraint::SOS { sos_type: t1, weights: w1, .. }, Constraint::SOS { sos_type: t2, weights: w2, .. }) => {
                assert_eq!(t2, t1, "{context}: SOS type mismatch for constraint '{name}'");
                assert_eq!(w2.len(), w1.len(), "{context}: SOS weight count mismatch for constraint '{name}'");
            }
            _ => panic!("{context}: constraint kind mismatch for '{name}'"),
        }
    }
}

/// Every fixture under `resources/mps/` should survive a full
/// parse -> write -> re-parse round trip with its structure intact.
#[test]
fn mps_fixtures_round_trip_through_writer() {
    let dir = resource_path("mps");
    let mut fixtures: Vec<PathBuf> = fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", dir.display()))
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|path| path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("mps")))
        .collect();
    fixtures.sort();
    assert!(!fixtures.is_empty(), "expected at least one .mps fixture under {}", dir.display());

    for path in fixtures {
        let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("<unknown>").to_string();
        let input = parse_file(&path).unwrap_or_else(|e| panic!("failed to read {file_name}: {e}"));
        let original = LpProblem::parse_mps(&input).unwrap_or_else(|e| panic!("failed to parse {file_name}: {e}"));

        let output = write_mps_string_with_options(&original, &lossless_options())
            .unwrap_or_else(|e| panic!("failed to write {file_name} as MPS: {e}"));
        let reparsed =
            LpProblem::parse_mps(&output).unwrap_or_else(|e| panic!("failed to re-parse written {file_name}:\n{output}\n\nerror: {e}"));

        assert_round_trip_preserves_structure(&original, &reparsed, &file_name);
    }
}

/// A selection of existing single-objective, non-strict-inequality LP
/// fixtures (already exercised by `test_from_file.rs`) round-tripped through
/// the MPS writer.
#[test]
fn lp_fixtures_round_trip_through_mps_writer() {
    let fixtures = [
        "pulp.lp",
        "pulp2.lp",
        "limbo.lp",
        "sos.lp",
        // "test.lp" is deliberately excluded: it contains a strict inequality
        // (`c3:: x2 > 1`), which MPS cannot represent -- see
        // `lp_fixture_with_strict_inequality_is_rejected` below.
        "test2.lp",
        "empty_bounds.lp",
        "blank_lines.lp",
        "infile_comments.lp",
        "infile_comments2.lp",
        "missing_signs.lp",
        "fit2d.lp",
    ];

    for file_name in fixtures {
        let input = read_resource(file_name).unwrap_or_else(|e| panic!("failed to read {file_name}: {e}"));
        let original = LpProblem::parse(&input).unwrap_or_else(|e| panic!("failed to parse {file_name}: {e}"));

        let output = write_mps_string_with_options(&original, &lossless_options())
            .unwrap_or_else(|e| panic!("failed to write {file_name} as MPS: {e}"));
        let reparsed =
            LpProblem::parse_mps(&output).unwrap_or_else(|e| panic!("failed to re-parse written {file_name}:\n{output}\n\nerror: {e}"));

        assert_round_trip_preserves_structure(&original, &reparsed, file_name);
    }
}

/// A multi-N-row MPS file parses into multiple objectives; writing it back
/// with `allow_multiple_objectives` is documented as lossy: only the first
/// objective (in insertion order) survives the round trip.
#[test]
fn multi_objective_mps_round_trip_keeps_first_objective_only() {
    let input = "\
NAME        multiobj
ROWS
 N  obj1
 N  obj2
 L  c1
COLUMNS
    x1        obj1      1
    x1        obj2      2
    x1        c1        3
RHS
    RHS_V     c1        10
ENDATA
";
    let problem = LpProblem::parse_mps(input).unwrap();
    assert_eq!(problem.objective_count(), 2, "both N rows must parse into objectives");

    let options = MpsWriterOptions { allow_multiple_objectives: true, ..lossless_options() };
    let output = write_mps_string_with_options(&problem, &options).unwrap();

    let reparsed = LpProblem::parse_mps(&output).unwrap();
    assert_eq!(reparsed.objective_count(), 1, "documented lossy behaviour: only the first objective is written");
    let obj1_id = reparsed.name_id("obj1").expect("first objective 'obj1' must survive");
    let obj1 = &reparsed.objectives[&obj1_id];
    assert_eq!(obj1.coefficients.len(), 1);
    assert_eq!(obj1.coefficients[0].value, 1.0);
    assert!(reparsed.name_id("obj2").is_none(), "second objective must be dropped entirely");
}

/// An external `SC` bound with a meaningful finite upper bound has that value
/// dropped on parse (the model's `SemiContinuous` carries no bound value);
/// the writer then emits the `1e30` sentinel, so the semi-continuity itself
/// survives the round trip but the original `50` does not.
#[test]
fn external_sc_bound_value_dropped_on_round_trip() {
    let input = "\
NAME        sctest
ROWS
 N  obj
 L  c1
COLUMNS
    x1        obj       1
    x1        c1        1
RHS
    RHS_V     c1        10
BOUNDS
 SC BOUND     x1        50
ENDATA
";
    let problem = LpProblem::parse_mps(input).unwrap();
    let x1 = &problem.variables[&problem.name_id("x1").unwrap()];
    assert_eq!(x1.var_type(), VariableType::SemiContinuous, "SC bound must resolve to SemiContinuous, dropping the 50");

    let output = write_mps_string_with_options(&problem, &lossless_options()).unwrap();
    let sc_line = output.lines().find(|line| line.trim_start().starts_with("SC ")).expect("written MPS must contain an SC bound line");
    assert!(!sc_line.contains("50"), "documented value-drop behaviour: the original upper bound must not reappear: {sc_line}");

    let reparsed = LpProblem::parse_mps(&output).unwrap();
    let x1 = &reparsed.variables[&reparsed.name_id("x1").unwrap()];
    assert_eq!(x1.var_type(), VariableType::SemiContinuous);
}

/// `test.lp` contains a strict inequality (`c3:: x2 > 1`), which MPS has no
/// row type for; writing it must return an error rather than silently
/// downgrading the operator.
#[test]
fn lp_fixture_with_strict_inequality_is_rejected() {
    let input = read_resource("test.lp").unwrap_or_else(|e| panic!("failed to read test.lp: {e}"));
    let problem = LpProblem::parse(&input).unwrap_or_else(|e| panic!("failed to parse test.lp: {e}"));

    let err = write_mps_string_with_options(&problem, &lossless_options()).unwrap_err();
    assert!(err.to_string().contains("strict inequality"), "unexpected error: {err}");
}
