// Parsed coefficients must round-trip bit-exactly from the source text,
// so the tests intentionally compare floats strictly.
#![allow(clippy::float_cmp)]

use super::state::{extract_mps_name, parse_mps};
use crate::lexer::RawConstraint;
use crate::model::{ComparisonOp, Sense, VariableType};

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
fn test_objsense_inline() {
    // Gurobi/CPLEX write the sense on the OBJSENSE header line itself.
    let input = "\
NAME        test
OBJSENSE    MAXIMIZE
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

    let input = input.replace("OBJSENSE    MAXIMIZE", "OBJSENSE MAX");
    let result = parse_mps(&input).unwrap();
    assert_eq!(result.sense, Sense::Maximize);

    let input = input.replace("OBJSENSE MAX", "OBJSENSE BOGUS");
    assert!(parse_mps(&input).is_err());
}

#[test]
fn test_unclosed_intorg_block() {
    // A missing INTEND must not panic; trailing columns stay integer.
    let input = "\
NAME        test
ROWS
 N  obj
 L  c1
COLUMNS
    MARK0000  'MARKER'                 'INTORG'
    x1        obj       1
    x1        c1        2
ENDATA
";
    let result = parse_mps(input).unwrap();
    assert!(result.integers.contains(&"x1"));
}

#[test]
fn test_sos_section_parsing() {
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
RHS
    RHS_V     c1        5
SOS
 S1 set1
    x1        1
    x2        2
ENDATA
";
    let result = parse_mps(input).unwrap();
    assert_eq!(result.sos.len(), 1);
    let RawConstraint::SOS { name, weights, .. } = &result.sos[0] else {
        panic!("expected SOS constraint");
    };
    assert_eq!(name.as_ref(), "set1");
    assert_eq!(weights.len(), 2);
    assert_eq!((weights[0].name, weights[0].value), ("x1", 1.0));
    assert_eq!((weights[1].name, weights[1].value), ("x2", 2.0));
}

#[test]
fn test_sos_weight_variable_named_like_header() {
    // A weight entry whose variable name starts with "S1" must not be
    // misread as a new SOS set header.
    let input = "\
NAME        test
ROWS
 N  obj
 L  c1
COLUMNS
    x1        obj       1
    x1        c1        1
    S1X       obj       1
    S1X       c1        1
RHS
    RHS_V     c1        5
SOS
 S1 set1
    x1        1
    S1X       2
ENDATA
";
    let result = parse_mps(input).unwrap();
    assert_eq!(result.sos.len(), 1);
    let RawConstraint::SOS { weights, .. } = &result.sos[0] else {
        panic!("expected SOS constraint");
    };
    assert_eq!(weights.len(), 2);
    assert_eq!((weights[1].name, weights[1].value), ("S1X", 2.0));
}

#[test]
fn test_sos_orphan_weights_and_empty_sets_dropped() {
    // Weights before any header must not bleed into the next set, and a
    // header with no weights must not emit a constraint.
    let input = "\
NAME        test
ROWS
 N  obj
 L  c1
COLUMNS
    x1        obj       1
    x1        c1        1
RHS
    RHS_V     c1        5
SOS
    x9        1
 S1 set1
    x1        1
 S2 empty_set
ENDATA
";
    let result = parse_mps(input).unwrap();
    assert_eq!(result.sos.len(), 1);
    let RawConstraint::SOS { name, weights, .. } = &result.sos[0] else {
        panic!("expected SOS constraint");
    };
    assert_eq!(name.as_ref(), "set1");
    assert_eq!(weights.len(), 1);
    assert_eq!(weights[0].name, "x1");
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
    // SC: semi-continuity is preserved; the (unrepresentable) upper bound is dropped.
    assert!(result.semi_continuous.contains(&"x1"));
    assert!(!result.integers.contains(&"x1"));
    assert!(result.bounds.iter().all(|(name, _)| *name != "x1"), "SC-only variable must not resolve to a plain bound");
    // SI: the model has no semi-integer type; the closest representation is
    // an integer variable with the given upper bound.
    assert!(!result.semi_continuous.contains(&"x2"));
    assert!(result.integers.contains(&"x2"));
    assert!(result.bounds.contains(&("x2", VariableType::UpperBound(200.0))));
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

/// Assert that every input in the slice fails to parse.
fn assert_all_err(inputs: &[&str]) {
    for input in inputs {
        assert!(parse_mps(input).is_err(), "expected parse error for input:\n{input}");
    }
}

#[test]
fn test_rows_section_errors() {
    assert_all_err(&[
        // Unknown row type
        "ROWS\n X  foo\nCOLUMNS\n    x1        foo       1\nENDATA\n",
        // ROWS data line with only one field
        "ROWS\n N\nCOLUMNS\n    x1        obj       1\nENDATA\n",
    ]);
}

#[test]
fn test_columns_section_errors() {
    assert_all_err(&[
        // Unknown MARKER type
        "ROWS\n N  obj\nCOLUMNS\n    MARK0000  'MARKER'  'INTBAD'\nENDATA\n",
        // Fewer than three fields on a data line
        "ROWS\n N  obj\nCOLUMNS\n    x1        obj\nENDATA\n",
        // Reference to an undefined row
        "ROWS\n N  obj\nCOLUMNS\n    x1        nosuchrow 1\nENDATA\n",
        // Invalid number
        "ROWS\n N  obj\nCOLUMNS\n    x1        obj       abc\nENDATA\n",
    ]);
}

#[test]
fn test_rhs_section_errors() {
    let skeleton = "ROWS\n N  obj\n L  c1\nCOLUMNS\n    x1        obj       1\n    x1        c1        1\nRHS\n";
    assert_all_err(&[
        // Reference to an undefined row
        &format!("{skeleton}    RHS_V     nosuchrow 1\nENDATA\n"),
        // Fewer than three fields
        &format!("{skeleton}    RHS_V     c1\nENDATA\n"),
        // Invalid number
        &format!("{skeleton}    RHS_V     c1        abc\nENDATA\n"),
    ]);
}

#[test]
fn test_ranges_section_errors() {
    let skeleton = "ROWS\n N  obj\n G  c1\nCOLUMNS\n    x1        obj       1\n    x1        c1        1\nRANGES\n";
    assert_all_err(&[
        // Reference to an undefined row
        &format!("{skeleton}    RNG_V     nosuchrow 1\nENDATA\n"),
        // Fewer than three fields
        &format!("{skeleton}    RNG_V     c1\nENDATA\n"),
        // Invalid number
        &format!("{skeleton}    RNG_V     c1        abc\nENDATA\n"),
    ]);
}

#[test]
fn test_bounds_section_errors() {
    let skeleton = "ROWS\n N  obj\n L  c1\nCOLUMNS\n    x1        obj       1\n    x1        c1        1\nBOUNDS\n";
    assert_all_err(&[
        // Fewer than three fields
        &format!("{skeleton} LO BOUND\nENDATA\n"),
        // Unknown bound type
        &format!("{skeleton} XX BOUND     x1        1\nENDATA\n"),
        // Bound type requiring a value, with no value
        &format!("{skeleton} LO BOUND     x1\nENDATA\n"),
        // Invalid number
        &format!("{skeleton} LO BOUND     x1        abc\nENDATA\n"),
    ]);
}

#[test]
fn test_structural_errors() {
    assert_all_err(&[
        // Invalid OBJSENSE value on a data line
        "OBJSENSE\n  BOGUS\nROWS\n N  obj\nCOLUMNS\n    x1        obj       1\nENDATA\n",
        // Unknown section header
        "GARBAGE\nROWS\n N  obj\nCOLUMNS\n    x1        obj       1\nENDATA\n",
        // Data line before any section header
        "    x1        obj       1\nROWS\n N  obj\nCOLUMNS\n    x1        obj       1\nENDATA\n",
        // Empty input
        "",
    ]);
}

#[test]
fn test_li_bound_makes_integer_with_lower_bound() {
    let input = "\
NAME        test
ROWS
 N  obj
 L  c1
COLUMNS
    x1        obj       1
    x1        c1        1
BOUNDS
 LI BOUND     x1        3
ENDATA
";
    let result = parse_mps(input).unwrap();
    assert!(result.integers.contains(&"x1"), "LI bound must mark the variable integer");
    assert!(
        result.bounds.iter().any(|(n, t)| *n == "x1" && *t == VariableType::LowerBound(3.0)),
        "x1 should have LowerBound(3.0), got: {:?}",
        result.bounds.iter().find(|(n, _)| *n == "x1")
    );
}

#[test]
fn test_ui_bound_makes_integer_with_upper_bound() {
    let input = "\
NAME        test
ROWS
 N  obj
 L  c1
COLUMNS
    x1        obj       1
    x1        c1        1
BOUNDS
 UI BOUND     x1        5
ENDATA
";
    let result = parse_mps(input).unwrap();
    assert!(result.integers.contains(&"x1"), "UI bound must mark the variable integer");
    assert!(
        result.bounds.iter().any(|(n, t)| *n == "x1" && *t == VariableType::UpperBound(5.0)),
        "x1 should have UpperBound(5.0), got: {:?}",
        result.bounds.iter().find(|(n, _)| *n == "x1")
    );
}

#[test]
fn test_duplicate_mi_after_lo_and_pl_after_up_rejected() {
    let skeleton = "ROWS\n N  obj\n L  c1\nCOLUMNS\n    x1        obj       1\n    x1        c1        1\nBOUNDS\n";

    let mi_input = format!("{skeleton} LO BOUND     x1        5\n MI BOUND     x1\nENDATA\n");
    let err = parse_mps(&mi_input).unwrap_err().to_string();
    assert!(err.contains("duplicate lower bound (MI)"), "unexpected error: {err}");

    let pl_input = format!("{skeleton} UP BOUND     x1        5\n PL BOUND     x1\nENDATA\n");
    let err = parse_mps(&pl_input).unwrap_err().to_string();
    assert!(err.contains("duplicate upper bound (PL)"), "unexpected error: {err}");
}

#[test]
fn test_duplicate_coefficients_accumulate_additively() {
    // MPS allows split entries: duplicate (variable, row) coefficients sum.
    let input = "\
NAME        test
ROWS
 N  obj
 L  c1
COLUMNS
    x1        c1        2
    x1        obj       1
    x1        c1        3
RHS
    RHS_V     c1        10
ENDATA
";
    let result = parse_mps(input).unwrap();
    let RawConstraint::Standard { coefficients, .. } = &result.constraints[0] else {
        panic!("expected Standard constraint");
    };
    assert_eq!(coefficients.len(), 1);
    assert_eq!((coefficients[0].name, coefficients[0].value), ("x1", 5.0));
}

#[test]
fn test_zero_n_rows_gets_synthetic_objective() {
    // A file with no N row still parses, with a synthetic empty objective.
    let input = "\
NAME        test
ROWS
 L  c1
COLUMNS
    x1        c1        1
RHS
    RHS_V     c1        10
ENDATA
";
    let result = parse_mps(input).unwrap();
    assert_eq!(result.objectives.len(), 1);
    assert_eq!(result.objectives[0].name.as_ref(), "__obj__");
    assert!(result.objectives[0].coefficients.is_empty());
    assert_eq!(result.constraints.len(), 1);
}

#[test]
fn test_scientific_notation_and_leading_dot_values() {
    let input = "\
NAME        test
ROWS
 N  obj
 L  c1
COLUMNS
    x1        obj       1.5E-5
    x1        c1        1
RHS
    RHS_V     c1        1e+30
BOUNDS
 LO BOUND     x1        .5
ENDATA
";
    let result = parse_mps(input).unwrap();
    assert_eq!(result.objectives[0].coefficients[0].value, 1.5e-5);
    let RawConstraint::Standard { rhs, .. } = &result.constraints[0] else {
        panic!("expected Standard constraint");
    };
    assert_eq!(*rhs, 1e30);
    assert!(result.bounds.iter().any(|(n, t)| *n == "x1" && *t == VariableType::LowerBound(0.5)));
}

#[test]
fn test_negative_rhs() {
    let input = "\
NAME        test
ROWS
 N  obj
 G  c1
COLUMNS
    x1        obj       1
    x1        c1        1
RHS
    RHS_V     c1        -10
ENDATA
";
    let result = parse_mps(input).unwrap();
    let RawConstraint::Standard { rhs, .. } = &result.constraints[0] else {
        panic!("expected Standard constraint");
    };
    assert_eq!(*rhs, -10.0);
}

#[test]
fn test_two_range_pairs_on_one_line() {
    // A RANGES data line may carry two (row, value) pairs.
    let input = "\
NAME        test
ROWS
 N  obj
 G  c1
 G  c2
COLUMNS
    x1        obj       1
    x1        c1        1
    x1        c2        1
RHS
    RHS_V     c1        5          c2        10
RANGES
    RNG_V     c1        4          c2        6
ENDATA
";
    let result = parse_mps(input).unwrap();
    assert_eq!(result.constraints.len(), 4, "two ranged rows must expand to four constraints");

    let expected = [
        ("c1", ComparisonOp::GTE, 5.0),
        ("c1_rng", ComparisonOp::LTE, 9.0),
        ("c2", ComparisonOp::GTE, 10.0),
        ("c2_rng", ComparisonOp::LTE, 16.0),
    ];
    for (constraint, (name, op, rhs_val)) in result.constraints.iter().zip(expected) {
        let RawConstraint::Standard { name: n, operator, rhs, .. } = constraint else {
            panic!("expected Standard constraint");
        };
        assert_eq!(n.as_ref(), name);
        assert_eq!(*operator, op, "operator mismatch for '{name}'");
        assert_eq!(*rhs, rhs_val, "rhs mismatch for '{name}'");
    }
}

#[test]
fn test_inline_comment_only_data_lines_tolerated() {
    // A data line consisting solely of a `$` inline comment yields zero
    // fields; every section must skip it rather than reject the file.
    let input = "\
NAME        test
ROWS
    $ comment only
 N  obj
 L  c1
COLUMNS
    $ comment only
    x1        obj       1
    x1        c1        1
RHS
    $ comment only
    RHS_V     c1        10
RANGES
    $ comment only
BOUNDS
    $ comment only
 UP BOUND     x1        5
ENDATA
";
    let result = parse_mps(input).unwrap();
    assert_eq!(result.objectives.len(), 1);
    assert_eq!(result.constraints.len(), 1);
    let RawConstraint::Standard { rhs, .. } = &result.constraints[0] else {
        panic!("expected Standard constraint");
    };
    assert_eq!(*rhs, 10.0);
    assert!(result.bounds.iter().any(|(n, t)| *n == "x1" && *t == VariableType::UpperBound(5.0)));
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

#[test]
fn test_columns_more_than_two_pairs() {
    // Free-format writers emit more than the strict two pairs per line; every
    // pair must be parsed rather than silently truncated.
    let input = "\
NAME t
ROWS
 N obj
 L r1
 L r2
 L r3
COLUMNS
    x obj 1 r1 1 r2 2 r3 3
RHS
    rhs r1 10
ENDATA
";
    let result = parse_mps(input).unwrap();
    assert_eq!(result.constraints.len(), 3);
    for (idx, expected) in [("r1", 1.0), ("r2", 2.0), ("r3", 3.0)].iter().enumerate() {
        let RawConstraint::Standard { name, coefficients, .. } = &result.constraints[idx] else {
            panic!("Expected Standard constraint");
        };
        assert_eq!(name.as_ref(), expected.0);
        assert_eq!(coefficients.len(), 1);
        assert_eq!(coefficients[0].value, expected.1);
    }
}

#[test]
fn test_dangling_field_is_error() {
    // A trailing row name without a value is malformed input, not data to drop.
    let input = "NAME t\nROWS\n N obj\n L r1\n L r2\nCOLUMNS\n    x r1 1 r2\nRHS\n    rhs r1 10\nENDATA\n";
    assert!(parse_mps(input).is_err());

    let input = "NAME t\nROWS\n N obj\n L r1\n L r2\nCOLUMNS\n    x r1 1\nRHS\n    rhs r1 10 r2\nENDATA\n";
    assert!(parse_mps(input).is_err());
}

#[test]
fn test_label_less_rhs_and_bounds() {
    // The RHS/BOUNDS vector label is a blank field in fixed format; free-format
    // files omitting it must still parse.
    let input = "\
NAME t
ROWS
 N obj
 G r1
COLUMNS
    x obj 1 r1 1
RHS
    r1 10
BOUNDS
 UP x 5
ENDATA
";
    let result = parse_mps(input).unwrap();
    let RawConstraint::Standard { rhs, .. } = &result.constraints[0] else { panic!("Expected Standard constraint") };
    assert_eq!(*rhs, 10.0);
    assert_eq!(result.bounds, vec![("x", VariableType::UpperBound(5.0))]);
}

#[test]
fn test_duplicate_row_name_is_error() {
    let input = "NAME t\nROWS\n N obj\n L r1\n G r1\nCOLUMNS\n    x obj 1 r1 1\nRHS\n    rhs r1 10\nENDATA\n";
    assert!(parse_mps(input).is_err());
}

#[test]
fn test_ranges_on_objective_row_is_error() {
    let input = "NAME t\nROWS\n N obj\n E r1\nCOLUMNS\n    x obj 1 r1 1\nRHS\n    rhs r1 10\nRANGES\n    rng obj 4\nENDATA\n";
    assert!(parse_mps(input).is_err());
}

#[test]
fn test_objective_constant_from_rhs() {
    // An RHS entry on the objective row is the negated objective constant.
    let input = "NAME t\nROWS\n N obj\n L r1\nCOLUMNS\n    x obj 1 r1 1\nRHS\n    rhs r1 10 obj -2.5\nENDATA\n";
    let result = parse_mps(input).unwrap();
    assert_eq!(result.objectives[0].constant, 2.5);
}
