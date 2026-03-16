use super::state::{extract_mps_name, parse_mps};
use super::writer::write_mps_string;
use crate::lexer::RawConstraint;
use crate::model::{ComparisonOp, Sense, VariableType};
use crate::problem::LpProblem;

#[test]
fn test_minimal_mps() {
    let input = "\
NAME        test
ROWS
 N  obj
 L  c1
COLUMNS
    x1        obj       1
    x1        c1        2
RHS
    RHS_V     c1        10
ENDATA
";
    let result = parse_mps(input).unwrap();
    assert_eq!(result.sense, Sense::Minimize);
    assert_eq!(result.objectives.len(), 1);
    assert_eq!(result.objectives[0].coefficients.len(), 1);
    assert_eq!(result.objectives[0].coefficients[0].name, "x1");
    assert_eq!(result.objectives[0].coefficients[0].value, 1.0);
    assert_eq!(result.constraints.len(), 1);
    if let RawConstraint::Standard { name, operator, rhs, .. } = &result.constraints[0] {
        assert_eq!(name.as_ref(), "c1");
        assert_eq!(*operator, ComparisonOp::LTE);
        assert_eq!(*rhs, 10.0);
    } else {
        panic!("Expected Standard constraint");
    }
}

#[test]
fn test_objsense_max() {
    let input = "\
NAME        test
OBJSENSE
  MAX
ROWS
 N  obj
 L  c1
COLUMNS
    x1        obj       1
    x1        c1        1
RHS
    RHS_V     c1        5
ENDATA
";
    let result = parse_mps(input).unwrap();
    assert_eq!(result.sense, Sense::Maximize);
}

#[test]
fn test_integer_markers() {
    let input = "\
NAME        test
ROWS
 N  obj
 L  c1
COLUMNS
    MARK0000  'MARKER'                 'INTORG'
    x1        obj       1
    x1        c1        2
    MARK0001  'MARKER'                 'INTEND'
    x2        obj       3
    x2        c1        4
ENDATA
";
    let result = parse_mps(input).unwrap();
    assert!(result.integers.contains(&"x1"));
    assert!(!result.integers.contains(&"x2"));
}

#[test]
fn test_bound_types() {
    let input = "\
NAME        test
ROWS
 N  obj
 L  c1
COLUMNS
    x1        obj       1
    x1        c1        1
    x2        obj       1
    x2        c1        1
    x3        obj       1
    x3        c1        1
    x4        obj       1
    x4        c1        1
RHS
    RHS_V     c1        10
BOUNDS
 FR BOUND     x1
 LO BOUND     x2        5
 UP BOUND     x2        15
 BV BOUND     x3
 FX BOUND     x4        7
ENDATA
";
    let result = parse_mps(input).unwrap();

    // x1 = Free
    assert!(result.bounds.iter().any(|(n, t)| *n == "x1" && *t == VariableType::Free));
    // x2 = DoubleBound(5, 15)
    assert!(result.bounds.iter().any(|(n, t)| *n == "x2" && *t == VariableType::DoubleBound(5.0, 15.0)));
    // x3 = Binary
    assert!(result.bounds.iter().any(|(n, t)| *n == "x3" && *t == VariableType::Binary));
    assert!(result.binaries.contains(&"x3"));
    // x4 = Fixed = DoubleBound(7, 7)
    assert!(result.bounds.iter().any(|(n, t)| *n == "x4" && *t == VariableType::DoubleBound(7.0, 7.0)));
}

#[test]
fn test_multiple_constraint_types() {
    let input = "\
NAME        test
ROWS
 N  obj
 L  c1
 G  c2
 E  c3
COLUMNS
    x1        obj       1
    x1        c1        1
    x1        c2        2
    x1        c3        3
RHS
    RHS_V     c1        10
    RHS_V     c2        5
    RHS_V     c3        7
ENDATA
";
    let result = parse_mps(input).unwrap();
    assert_eq!(result.constraints.len(), 3);

    let ops: Vec<ComparisonOp> = result
        .constraints
        .iter()
        .filter_map(|c| if let RawConstraint::Standard { operator, .. } = c { Some(*operator) } else { None })
        .collect();
    assert_eq!(ops, vec![ComparisonOp::LTE, ComparisonOp::GTE, ComparisonOp::EQ]);
}

#[test]
fn test_missing_rows_section() {
    let input = "\
NAME        test
COLUMNS
    x1        obj       1
ENDATA
";
    let result = parse_mps(input);
    assert!(result.is_err());
}

#[test]
fn test_missing_columns_section() {
    let input = "\
NAME        test
ROWS
 N  obj
ENDATA
";
    let result = parse_mps(input);
    assert!(result.is_err());
}

#[test]
fn test_two_entries_per_line() {
    let input = "\
NAME        test
ROWS
 N  obj
 L  c1
 L  c2
COLUMNS
    x1        c1        1          c2        2
RHS
    RHS_V     c1        10         c2        20
ENDATA
";
    let result = parse_mps(input).unwrap();
    assert_eq!(result.constraints.len(), 2);

    if let RawConstraint::Standard { rhs, .. } = &result.constraints[0] {
        assert_eq!(*rhs, 10.0);
    }
    if let RawConstraint::Standard { rhs, .. } = &result.constraints[1] {
        assert_eq!(*rhs, 20.0);
    }
}

#[test]
fn test_extract_mps_name() {
    let input = "NAME        my_problem\nROWS\n N  obj\n";
    assert_eq!(extract_mps_name(input), Some("my_problem".to_string()));
}

#[test]
fn test_comment_lines_skipped() {
    let input = "\
* This is a comment
NAME        test
* Another comment
ROWS
 N  obj
 L  c1
COLUMNS
    x1        obj       1
    x1        c1        1
ENDATA
";
    let result = parse_mps(input).unwrap();
    assert_eq!(result.objectives.len(), 1);
}

#[test]
fn test_blank_lines_skipped() {
    let input = "\
NAME        test

ROWS
 N  obj
 L  c1

COLUMNS
    x1        obj       1
    x1        c1        1

ENDATA
";
    let result = parse_mps(input).unwrap();
    assert_eq!(result.constraints.len(), 1);
}

#[test]
fn test_semi_continuous_bounds() {
    let input = "\
NAME        test
ROWS
 N  obj
 L  c1
COLUMNS
    x1        obj       1
    x1        c1        1
    x2        obj       1
    x2        c1        1
BOUNDS
 SC BOUND     x1        100
 SI BOUND     x2        200
ENDATA
";
    let result = parse_mps(input).unwrap();
    assert!(result.semi_continuous.contains(&"x1"));
    assert!(result.semi_continuous.contains(&"x2"));
    assert!(!result.integers.contains(&"x1"));
    assert!(result.integers.contains(&"x2"));
}

#[test]
fn test_default_rhs_zero() {
    let input = "\
NAME        test
ROWS
 N  obj
 L  c1
COLUMNS
    x1        obj       1
    x1        c1        1
ENDATA
";
    let result = parse_mps(input).unwrap();
    if let RawConstraint::Standard { rhs, .. } = &result.constraints[0] {
        assert_eq!(*rhs, 0.0);
    }
}

// --- New spec-compliance tests ---

#[test]
fn test_default_bounds_zero_to_inf() {
    let input = "\
NAME        test
ROWS
 N  obj
 L  c1
COLUMNS
    x1        obj       1
    x1        c1        1
    x2        obj       2
    x2        c1        3
ENDATA
";
    let result = parse_mps(input).unwrap();

    // Variables with no BOUNDS entry should default to LowerBound(0.0)
    assert!(
        result.bounds.iter().any(|(n, t)| *n == "x1" && *t == VariableType::LowerBound(0.0)),
        "x1 should have default LowerBound(0.0), got: {:?}",
        result.bounds.iter().find(|(n, _)| *n == "x1")
    );
    assert!(
        result.bounds.iter().any(|(n, t)| *n == "x2" && *t == VariableType::LowerBound(0.0)),
        "x2 should have default LowerBound(0.0), got: {:?}",
        result.bounds.iter().find(|(n, _)| *n == "x2")
    );
}

#[test]
fn test_integer_default_bounds_zero_to_one() {
    let input = "\
NAME        test
ROWS
 N  obj
 L  c1
COLUMNS
    MARK0000  'MARKER'                 'INTORG'
    y1        obj       1
    y1        c1        2
    MARK0001  'MARKER'                 'INTEND'
ENDATA
";
    let result = parse_mps(input).unwrap();

    // INTORG/INTEND variable with no BOUNDS should default to [0, 1]
    assert!(
        result.bounds.iter().any(|(n, t)| *n == "y1" && *t == VariableType::DoubleBound(0.0, 1.0)),
        "y1 should have default DoubleBound(0.0, 1.0), got: {:?}",
        result.bounds.iter().find(|(n, _)| *n == "y1")
    );
}

#[test]
fn test_negative_upper_implies_mi() {
    let input = "\
NAME        test
ROWS
 N  obj
 L  c1
COLUMNS
    x1        obj       1
    x1        c1        1
BOUNDS
 UP BOUND     x1        -5
ENDATA
";
    let result = parse_mps(input).unwrap();

    // UP < 0 with no LO should produce DoubleBound(-inf, -5)
    assert!(
        result.bounds.iter().any(|(n, t)| *n == "x1" && *t == VariableType::DoubleBound(f64::NEG_INFINITY, -5.0)),
        "x1 should have DoubleBound(-inf, -5.0), got: {:?}",
        result.bounds.iter().find(|(n, _)| *n == "x1")
    );
}

#[test]
fn test_dollar_inline_comment() {
    let input = "\
NAME        test
ROWS
 N  obj
 L  c1
COLUMNS
    x1        obj       1          $ this is a comment
    x1        c1        2
RHS
    RHS_V     c1        10         $ comment here too
ENDATA
";
    let result = parse_mps(input).unwrap();
    assert_eq!(result.objectives[0].coefficients.len(), 1);
    assert_eq!(result.objectives[0].coefficients[0].value, 1.0);
    assert_eq!(result.constraints.len(), 1);
    if let RawConstraint::Standard { rhs, .. } = &result.constraints[0] {
        assert_eq!(*rhs, 10.0);
    }
}

#[test]
fn test_multiple_n_rows() {
    let input = "\
NAME        test
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
    let result = parse_mps(input).unwrap();

    // Two N-rows should produce two objectives
    assert_eq!(result.objectives.len(), 2);
    assert_eq!(result.objectives[0].name.as_ref(), "obj1");
    assert_eq!(result.objectives[0].coefficients[0].value, 1.0);
    assert_eq!(result.objectives[1].name.as_ref(), "obj2");
    assert_eq!(result.objectives[1].coefficients[0].value, 2.0);
}

#[test]
fn test_ranges_section_g_row() {
    // G row with range r: lower = rhs, upper = rhs + |r|
    let input = "\
NAME        test
ROWS
 N  obj
 G  c1
COLUMNS
    x1        obj       1
    x1        c1        1
RHS
    RHS_V     c1        5
RANGES
    RNG_V     c1        10
ENDATA
";
    let result = parse_mps(input).unwrap();

    // Should expand into two constraints
    assert_eq!(result.constraints.len(), 2);

    // First: c1 >= 5 (the lower bound)
    if let RawConstraint::Standard { name, operator, rhs, .. } = &result.constraints[0] {
        assert_eq!(name.as_ref(), "c1");
        assert_eq!(*operator, ComparisonOp::GTE);
        assert_eq!(*rhs, 5.0);
    } else {
        panic!("Expected Standard constraint");
    }

    // Second: c1_rng <= 15 (the upper bound = rhs + |range|)
    if let RawConstraint::Standard { name, operator, rhs, .. } = &result.constraints[1] {
        assert_eq!(name.as_ref(), "c1_rng");
        assert_eq!(*operator, ComparisonOp::LTE);
        assert_eq!(*rhs, 15.0);
    } else {
        panic!("Expected Standard constraint");
    }
}

#[test]
fn test_ranges_section_l_row() {
    // L row with range r: lower = rhs - |r|, upper = rhs
    let input = "\
NAME        test
ROWS
 N  obj
 L  c1
COLUMNS
    x1        obj       1
    x1        c1        1
RHS
    RHS_V     c1        20
RANGES
    RNG_V     c1        8
ENDATA
";
    let result = parse_mps(input).unwrap();

    assert_eq!(result.constraints.len(), 2);

    // First: c1 >= 12 (lower = rhs - |range|)
    if let RawConstraint::Standard { name, operator, rhs, .. } = &result.constraints[0] {
        assert_eq!(name.as_ref(), "c1");
        assert_eq!(*operator, ComparisonOp::GTE);
        assert_eq!(*rhs, 12.0);
    } else {
        panic!("Expected Standard constraint");
    }

    // Second: c1_rng <= 20 (upper = rhs)
    if let RawConstraint::Standard { name, operator, rhs, .. } = &result.constraints[1] {
        assert_eq!(name.as_ref(), "c1_rng");
        assert_eq!(*operator, ComparisonOp::LTE);
        assert_eq!(*rhs, 20.0);
    } else {
        panic!("Expected Standard constraint");
    }
}

#[test]
fn test_ranges_section_e_row() {
    // E row, positive range: lower = rhs, upper = rhs + range
    let input = "\
NAME        test
ROWS
 N  obj
 E  c1
 E  c2
COLUMNS
    x1        obj       1
    x1        c1        1
    x1        c2        1
RHS
    RHS_V     c1        10
    RHS_V     c2        10
RANGES
    RNG_V     c1        5
    RNG_V     c2        -3
ENDATA
";
    let result = parse_mps(input).unwrap();

    // 2 rows * 2 constraints each = 4 constraints
    assert_eq!(result.constraints.len(), 4);

    // c1 (positive range 5): lower=10, upper=15
    if let RawConstraint::Standard { name, operator, rhs, .. } = &result.constraints[0] {
        assert_eq!(name.as_ref(), "c1");
        assert_eq!(*operator, ComparisonOp::GTE);
        assert_eq!(*rhs, 10.0);
    }
    if let RawConstraint::Standard { name, operator, rhs, .. } = &result.constraints[1] {
        assert_eq!(name.as_ref(), "c1_rng");
        assert_eq!(*operator, ComparisonOp::LTE);
        assert_eq!(*rhs, 15.0);
    }

    // c2 (negative range -3): lower=7 (rhs+range), upper=10 (rhs)
    if let RawConstraint::Standard { name, operator, rhs, .. } = &result.constraints[2] {
        assert_eq!(name.as_ref(), "c2");
        assert_eq!(*operator, ComparisonOp::GTE);
        assert_eq!(*rhs, 7.0);
    }
    if let RawConstraint::Standard { name, operator, rhs, .. } = &result.constraints[3] {
        assert_eq!(name.as_ref(), "c2_rng");
        assert_eq!(*operator, ComparisonOp::LTE);
        assert_eq!(*rhs, 10.0);
    }
}

#[test]
fn test_objective_rhs_no_crash() {
    // RHS on N-row should not crash -- it logs a warning but is otherwise ignored
    let input = "\
NAME        test
ROWS
 N  obj
 L  c1
COLUMNS
    x1        obj       1
    x1        c1        1
RHS
    RHS_V     obj       42
    RHS_V     c1        10
ENDATA
";
    let result = parse_mps(input).unwrap();
    assert_eq!(result.objectives.len(), 1);
    assert_eq!(result.constraints.len(), 1);
}

#[test]
fn test_explicit_bounds_override_default() {
    // Variables with explicit bounds should not get default [0, +inf]
    let input = "\
NAME        test
ROWS
 N  obj
 L  c1
COLUMNS
    x1        obj       1
    x1        c1        1
    x2        obj       2
    x2        c1        3
BOUNDS
 FR BOUND     x1
ENDATA
";
    let result = parse_mps(input).unwrap();

    // x1 has explicit FR bound
    assert!(result.bounds.iter().any(|(n, t)| *n == "x1" && *t == VariableType::Free));
    // x2 has no explicit bounds -- should get default LowerBound(0.0)
    assert!(result.bounds.iter().any(|(n, t)| *n == "x2" && *t == VariableType::LowerBound(0.0)));
}

#[test]
fn test_negative_upper_with_explicit_lower() {
    // UP < 0 WITH explicit LO should NOT override to -inf
    let input = "\
NAME        test
ROWS
 N  obj
 L  c1
COLUMNS
    x1        obj       1
    x1        c1        1
BOUNDS
 LO BOUND     x1        -10
 UP BOUND     x1        -5
ENDATA
";
    let result = parse_mps(input).unwrap();

    assert!(
        result.bounds.iter().any(|(n, t)| *n == "x1" && *t == VariableType::DoubleBound(-10.0, -5.0)),
        "x1 should have DoubleBound(-10.0, -5.0), got: {:?}",
        result.bounds.iter().find(|(n, _)| *n == "x1")
    );
}

#[test]
fn test_dollar_comment_in_bounds() {
    let input = "\
NAME        test
ROWS
 N  obj
 L  c1
COLUMNS
    x1        obj       1
    x1        c1        1
BOUNDS
 LO BOUND     x1        5    $ lower bound comment
 UP BOUND     x1        15   $ upper bound comment
ENDATA
";
    let result = parse_mps(input).unwrap();
    assert!(result.bounds.iter().any(|(n, t)| *n == "x1" && *t == VariableType::DoubleBound(5.0, 15.0)));
}

#[test]
fn test_first_rhs_vector_only() {
    // Two RHS vectors -- only the first (RHS1) should be used
    let input = "\
NAME        test
ROWS
 N  obj
 L  c1
 L  c2
COLUMNS
    x1        obj       1
    x1        c1        1
    x1        c2        1
RHS
    RHS1      c1        10
    RHS1      c2        20
    RHS2      c1        99
    RHS2      c2        99
ENDATA
";
    let result = parse_mps(input).unwrap();

    // c1 should have RHS=10 (from RHS1), not 99 (from RHS2)
    if let RawConstraint::Standard { name, rhs, .. } = &result.constraints[0] {
        assert_eq!(name.as_ref(), "c1");
        assert_eq!(*rhs, 10.0);
    } else {
        panic!("Expected Standard constraint");
    }

    // c2 should have RHS=20 (from RHS1), not 99 (from RHS2)
    if let RawConstraint::Standard { name, rhs, .. } = &result.constraints[1] {
        assert_eq!(name.as_ref(), "c2");
        assert_eq!(*rhs, 20.0);
    } else {
        panic!("Expected Standard constraint");
    }
}

#[test]
fn test_first_bounds_vector_only() {
    // Two BOUNDS vectors -- only the first (BND1) should be used
    let input = "\
NAME        test
ROWS
 N  obj
 L  c1
COLUMNS
    x1        obj       1
    x1        c1        1
BOUNDS
 UP BND1      x1        10
 UP BND2      x1        99
ENDATA
";
    let result = parse_mps(input).unwrap();

    // x1 should have UP=10 from BND1, not 99 from BND2
    assert!(
        result.bounds.iter().any(|(n, t)| *n == "x1" && *t == VariableType::UpperBound(10.0)),
        "x1 should have UpperBound(10.0), got: {:?}",
        result.bounds.iter().find(|(n, _)| *n == "x1")
    );
}

#[test]
fn test_first_ranges_vector_only() {
    // Two RANGES vectors -- only the first (RNG1) should be used
    let input = "\
NAME        test
ROWS
 N  obj
 G  c1
COLUMNS
    x1        obj       1
    x1        c1        1
RHS
    RHS_V     c1        5
RANGES
    RNG1      c1        10
    RNG2      c1        99
ENDATA
";
    let result = parse_mps(input).unwrap();

    // Range should expand using RNG1 value (10), not RNG2 (99)
    assert_eq!(result.constraints.len(), 2);
    if let RawConstraint::Standard { rhs, .. } = &result.constraints[1] {
        // upper = rhs + |range| = 5 + 10 = 15
        assert_eq!(*rhs, 15.0);
    } else {
        panic!("Expected Standard constraint");
    }
}

#[test]
fn test_duplicate_lower_bound_rejected() {
    let input = "\
NAME        test
ROWS
 N  obj
 L  c1
COLUMNS
    x1        obj       1
    x1        c1        1
BOUNDS
 LO BOUND     x1        5
 LO BOUND     x1        10
ENDATA
";
    let result = parse_mps(input);
    assert!(result.is_err(), "Duplicate LO bound should be rejected");
    let err = result.unwrap_err().to_string();
    assert!(err.contains("duplicate lower bound"), "Error should mention duplicate: {err}");
}

#[test]
fn test_duplicate_upper_bound_rejected() {
    let input = "\
NAME        test
ROWS
 N  obj
 L  c1
COLUMNS
    x1        obj       1
    x1        c1        1
BOUNDS
 UP BOUND     x1        10
 UP BOUND     x1        20
ENDATA
";
    let result = parse_mps(input);
    assert!(result.is_err(), "Duplicate UP bound should be rejected");
    let err = result.unwrap_err().to_string();
    assert!(err.contains("duplicate upper bound"), "Error should mention duplicate: {err}");
}

#[test]
fn test_duplicate_fixed_bound_rejected() {
    let input = "\
NAME        test
ROWS
 N  obj
 L  c1
COLUMNS
    x1        obj       1
    x1        c1        1
BOUNDS
 FX BOUND     x1        5
 FX BOUND     x1        10
ENDATA
";
    let result = parse_mps(input);
    assert!(result.is_err(), "Duplicate FX bound should be rejected");
}

#[test]
fn test_n_row_extra_fields_accepted() {
    // Gurobi-style N-row with priority/weight/tolerance fields
    let input = "\
NAME        test
ROWS
 N  OBJ0 2 1 0 0
 L  c1
COLUMNS
    x1        OBJ0      1
    x1        c1        1
ENDATA
";
    let result = parse_mps(input).unwrap();
    assert_eq!(result.objectives.len(), 1);
    // The extra fields are ignored but parsing succeeds
    assert_eq!(result.objectives[0].name.as_ref(), "OBJ0");
}

#[test]
fn test_unsupported_section_skipped() {
    let input = "\
NAME        test
ROWS
 N  obj
 L  c1
COLUMNS
    x1        obj       1
    x1        c1        1
QUADOBJ
    x1  x1  2.0
RHS
    RHS_V     c1        10
ENDATA
";
    let result = parse_mps(input).unwrap();
    assert_eq!(result.objectives.len(), 1);
    assert_eq!(result.constraints.len(), 1);
}

#[test]
fn test_enlight4_all_variables_integer() {
    use std::path::PathBuf;

    let mut file_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    file_path.push("resources/enlight4.mps");
    let input = std::fs::read_to_string(&file_path).expect("failed to read enlight4.mps");

    let result = parse_mps(&input).unwrap();

    // All variables in enlight4 are between INTORG/INTEND markers
    debug_assert!(!result.integers.is_empty(), "enlight4 should have integer variables");
    let all_column_names: Vec<&str> = result
        .objectives
        .iter()
        .flat_map(|o| o.coefficients.iter().map(|c| c.name))
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();

    for var_name in &all_column_names {
        assert!(result.integers.contains(var_name), "Variable '{var_name}' should be integer (between INTORG/INTEND markers)");
    }
}

#[test]
fn test_multiple_unsupported_sections_skipped() {
    let input = "\
NAME        test
ROWS
 N  obj
 L  c1
COLUMNS
    x1        obj       1
    x1        c1        1
INDICATORS
    IF c1 x1 1
PWLOBJ
    x1 3 0.0 0.0 1.0 1.0 2.0 3.0
GENCONS
    gc0: MIN x1 x1 x1
SCENARIOS
    scenario1
RHS
    RHS_V     c1        10
ENDATA
";
    let result = parse_mps(input).unwrap();
    assert_eq!(result.constraints.len(), 1);
}

// --- Round-trip tests: parse MPS → build LpProblem → write MPS → re-parse → compare ---

/// Helper: parse MPS input, write back, re-parse, and assert structural parity.
fn assert_mps_round_trip(input: &str) {
    let original = LpProblem::parse_mps(input).unwrap_or_else(|e| panic!("Failed to parse original MPS: {e}"));

    let written = write_mps_string(&original).unwrap_or_else(|e| panic!("Failed to write MPS: {e}"));

    let round_tripped =
        LpProblem::parse_mps(&written).unwrap_or_else(|e| panic!("Failed to re-parse written MPS: {e}\n\nWritten MPS:\n{written}"));

    assert_eq!(
        original.sense, round_tripped.sense,
        "Sense mismatch: original={:?}, round-tripped={:?}",
        original.sense, round_tripped.sense
    );
    assert_eq!(
        original.objective_count(),
        round_tripped.objective_count(),
        "Objective count mismatch: original={}, round-tripped={}",
        original.objective_count(),
        round_tripped.objective_count()
    );
    assert_eq!(
        original.constraint_count(),
        round_tripped.constraint_count(),
        "Constraint count mismatch: original={}, round-tripped={}",
        original.constraint_count(),
        round_tripped.constraint_count()
    );
    assert_eq!(
        original.variable_count(),
        round_tripped.variable_count(),
        "Variable count mismatch: original={}, round-tripped={}",
        original.variable_count(),
        round_tripped.variable_count()
    );
}

#[test]
fn test_mps_round_trip_basic() {
    let input = "\
NAME        basic
ROWS
 N  obj
 L  c1
COLUMNS
    x1        obj       1
    x1        c1        2
RHS
    RHS_V     c1        10
ENDATA
";
    assert_mps_round_trip(input);
}

#[test]
fn test_mps_round_trip_maximize() {
    let input = "\
NAME        maxtest
OBJSENSE
  MAX
ROWS
 N  obj
 L  c1
COLUMNS
    x1        obj       3
    x1        c1        1
RHS
    RHS_V     c1        5
ENDATA
";
    assert_mps_round_trip(input);
}

#[test]
fn test_mps_round_trip_integer_markers() {
    let input = "\
NAME        inttest
ROWS
 N  obj
 L  c1
COLUMNS
    MARK0000  'MARKER'                 'INTORG'
    x1        obj       1
    x1        c1        2
    x2        obj       3
    x2        c1        4
    MARK0001  'MARKER'                 'INTEND'
ENDATA
";
    assert_mps_round_trip(input);
}

#[test]
fn test_mps_round_trip_bound_types() {
    let input = "\
NAME        bounds
ROWS
 N  obj
 L  c1
COLUMNS
    x1        obj       1
    x1        c1        1
    x2        obj       1
    x2        c1        1
    x3        obj       1
    x3        c1        1
    x4        obj       1
    x4        c1        1
RHS
    RHS_V     c1        10
BOUNDS
 FR BOUND     x1
 LO BOUND     x2        5
 UP BOUND     x2        15
 BV BOUND     x3
 FX BOUND     x4        7
ENDATA
";
    assert_mps_round_trip(input);
}

#[test]
fn test_mps_round_trip_semi_continuous() {
    let input = "\
NAME        sc_test
ROWS
 N  obj
 L  c1
COLUMNS
    x1        obj       1
    x1        c1        1
    x2        obj       1
    x2        c1        1
BOUNDS
 SC BOUND     x1        100
ENDATA
";
    assert_mps_round_trip(input);
}

#[test]
fn test_mps_round_trip_multiple_constraints() {
    let input = "\
NAME        multi
ROWS
 N  obj
 L  c1
 G  c2
 E  c3
COLUMNS
    x1        obj       1
    x1        c1        1
    x1        c2        2
    x1        c3        3
    x2        obj       4
    x2        c1        5
    x2        c2        6
    x2        c3        7
RHS
    RHS_V     c1        10
    RHS_V     c2        5
    RHS_V     c3        7
ENDATA
";
    assert_mps_round_trip(input);
}

#[test]
fn test_mps_round_trip_multiple_objectives() {
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
    assert_mps_round_trip(input);
}

#[test]
fn test_mps_round_trip_enlight4() {
    use std::path::PathBuf;

    let mut file_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    file_path.push("resources/enlight4.mps");
    let input = std::fs::read_to_string(&file_path).expect("failed to read enlight4.mps");

    assert_mps_round_trip(&input);
}
