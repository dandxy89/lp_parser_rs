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

use lp_parser_rs::model::Constraint;
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
        assert_eq!(reparsed_var.var_type, var.var_type, "{context}: bound/type mismatch for variable '{name}'");
    }

    for (name_id, constraint) in &original.constraints {
        let name = original.resolve(*name_id);
        let reparsed_id = reparsed.name_id(name).unwrap_or_else(|| panic!("{context}: constraint '{name}' missing after round trip"));
        let reparsed_constraint = reparsed.constraints.get(&reparsed_id).unwrap_or_else(|| panic!("{context}: constraint '{name}' vanished"));

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

        let output = write_mps_string_with_options(&original, &lossless_options()).unwrap_or_else(|e| panic!("failed to write {file_name} as MPS: {e}"));
        let reparsed = LpProblem::parse_mps(&output).unwrap_or_else(|e| panic!("failed to re-parse written {file_name}:\n{output}\n\nerror: {e}"));

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

        let output = write_mps_string_with_options(&original, &lossless_options()).unwrap_or_else(|e| panic!("failed to write {file_name} as MPS: {e}"));
        let reparsed = LpProblem::parse_mps(&output).unwrap_or_else(|e| panic!("failed to re-parse written {file_name}:\n{output}\n\nerror: {e}"));

        assert_round_trip_preserves_structure(&original, &reparsed, file_name);
    }
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
