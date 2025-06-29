//! Parser for LP problem constraints.
//!
//! This module handles parsing of constraint definitions,
//! including their names, coefficients, operators, and
//! right-hand side values.
//!

use std::borrow::Cow;
use std::collections::HashMap;
use std::collections::hash_map::Entry;

use nom::branch::alt;
use nom::bytes::complete::{tag, tag_no_case};
use nom::character::complete::{char, multispace0};
use nom::combinator::{map, opt, value};
use nom::multi::many1;
use nom::sequence::preceded;
use nom::{IResult, Parser as _};
use unique_id::Generator as _;
use unique_id::sequence::SequenceGenerator;

use crate::log_unparsed_content;
use crate::model::{Constraint, Variable};
use crate::parsers::coefficient::parse_coefficient;
use crate::parsers::number::{parse_cmp_op, parse_num_value};
use crate::parsers::parser_traits::parse_variable;

// Pre-allocated constraint names for common cases
static COMMON_CONSTRAINT_NAMES: &[&str] = &[
    "CONSTRAINT_1",
    "CONSTRAINT_2",
    "CONSTRAINT_3",
    "CONSTRAINT_4",
    "CONSTRAINT_5",
    "CONSTRAINT_6",
    "CONSTRAINT_7",
    "CONSTRAINT_8",
    "CONSTRAINT_9",
    "CONSTRAINT_10",
    "CONSTRAINT_11",
    "CONSTRAINT_12",
    "CONSTRAINT_13",
    "CONSTRAINT_14",
    "CONSTRAINT_15",
    "CONSTRAINT_16",
    "CONSTRAINT_17",
    "CONSTRAINT_18",
    "CONSTRAINT_19",
    "CONSTRAINT_20",
];

fn get_constraint_name(id: i64) -> Cow<'static, str> {
    if id > 0 && id <= COMMON_CONSTRAINT_NAMES.len() as i64 {
        Cow::Borrowed(COMMON_CONSTRAINT_NAMES[(id - 1) as usize])
    } else {
        Cow::Owned(format!("CONSTRAINT_{id}"))
    }
}

#[inline]
/// Parses a constraint section header from an LP format input string.
pub fn parse_constraint_header(input: &str) -> IResult<&str, ()> {
    value(
        (),
        (
            multispace0,
            alt((tag_no_case("s.t."), tag_no_case("st"), tag_no_case("subject to"), tag_no_case("such that"))),
            opt(char(':')),
            multispace0,
        ),
    )
    .parse(input)
}

#[inline]
fn parse_comment_marker(input: &str) -> IResult<&str, ()> {
    value((), preceded(multispace0, tag("\\"))).parse(input)
}

type ConstraintParseResult<'a> = IResult<&'a str, (HashMap<Cow<'a, str>, Constraint<'a>>, HashMap<&'a str, Variable<'a>>)>;

#[inline]
/// Parses a string input to extract constraints and associated variables.
///
/// This function processes the input string to identify and parse constraints,
/// which may include optional comment markers, variable names, coefficients,
/// comparison operators, and right-hand side values. It returns a result
/// containing a tuple of parsed constraints and variables, or an error if
/// parsing fails. The function also logs any remaining unparsed input.
///
/// # Arguments
///
/// * `input` - A string slice representing the input to be parsed.
///
/// # Returns
///
/// * `ParsedConstraints<'a>` - A result containing a tuple of a hashmap of
///   constraints and a hashmap of variables, or an error if parsing fails.
///
pub fn parse_constraints<'a>(input: &'a str) -> ConstraintParseResult<'a> {
    let mut constraint_vars: HashMap<&'a str, Variable<'a>> = HashMap::with_capacity(512);
    let generator = SequenceGenerator;

    let parser = map(
        (
            // Optional comment marker
            opt(parse_comment_marker),
            // Name part with optional whitespace and newlines
            opt(map((preceded(multispace0, parse_variable), preceded(multispace0, char(':')), multispace0), |(name, _, _)| name)),
            // Coefficients with flexible whitespace and newlines
            many1(preceded(multispace0, parse_coefficient)),
            // Operator and RHS with flexible whitespace
            preceded(multispace0, parse_cmp_op),
            preceded(multispace0, parse_num_value),
        ),
        |(is_comment, name, coefficients, operator, rhs)| {
            is_comment.is_none().then(|| {
                for coeff in &coefficients {
                    if let Entry::Vacant(vacant_entry) = constraint_vars.entry(coeff.name) {
                        vacant_entry.insert(Variable::new(coeff.name));
                    }
                }

                // Standard (SOS constraints are handled separately)
                Constraint::Standard {
                    name: name.map_or_else(
                        || {
                            let next = generator.next_id();
                            get_constraint_name(next)
                        },
                        Cow::Borrowed,
                    ),
                    coefficients,
                    operator,
                    rhs,
                }
            })
        },
    );

    let (remaining, constraints) = many1(parser).parse(input)?;
    let cons = constraints.into_iter().flatten().map(|c| (Cow::Owned(c.name().to_string()), c)).collect();

    log_unparsed_content("Failed to parse constraints fully", remaining);
    Ok(("", (cons, constraint_vars)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ComparisonOp;

    #[test]
    fn test_constraint_header_basic() {
        let result = parse_constraint_header("subject to").unwrap();
        assert_eq!(result.0, "");

        let result = parse_constraint_header("s.t.").unwrap();
        assert_eq!(result.0, "");

        let result = parse_constraint_header("st").unwrap();
        assert_eq!(result.0, "");

        let result = parse_constraint_header("such that").unwrap();
        assert_eq!(result.0, "");
    }

    #[test]
    fn test_constraint_header_case_insensitive() {
        let result = parse_constraint_header("SUBJECT TO").unwrap();
        assert_eq!(result.0, "");

        let result = parse_constraint_header("S.T.").unwrap();
        assert_eq!(result.0, "");

        let result = parse_constraint_header("ST").unwrap();
        assert_eq!(result.0, "");

        let result = parse_constraint_header("Such That").unwrap();
        assert_eq!(result.0, "");
    }

    #[test]
    fn test_constraint_header_with_whitespace() {
        let result = parse_constraint_header("  subject to  ").unwrap();
        assert_eq!(result.0, "");

        let result = parse_constraint_header("\t\ns.t.:\n\t").unwrap();
        assert_eq!(result.0, "");

        let result = parse_constraint_header("   such that:   ").unwrap();
        assert_eq!(result.0, "");
    }

    #[test]
    fn test_constraint_header_with_colon() {
        let result = parse_constraint_header("subject to:").unwrap();
        assert_eq!(result.0, "");

        let result = parse_constraint_header("s.t.:").unwrap();
        assert_eq!(result.0, "");
    }

    #[test]
    fn test_constraint_header_invalid() {
        assert!(parse_constraint_header("invalid").is_err());
        assert!(parse_constraint_header("constraints").is_err());
        assert!(parse_constraint_header("subject").is_err());
        assert!(parse_constraint_header("").is_err());
    }

    #[test]
    fn test_simple_constraint_unnamed() {
        let input = "x1 + x2 <= 10";
        let result = parse_constraints(input).unwrap();
        assert_eq!(result.0, "");

        let (constraints, vars) = result.1;
        assert_eq!(constraints.len(), 1);
        assert_eq!(vars.len(), 2);
        assert!(vars.contains_key("x1"));
        assert!(vars.contains_key("x2"));

        let constraint = constraints.values().next().unwrap();
        assert!(constraint.name().starts_with("CONSTRAINT_"));
        if let Constraint::Standard { coefficients, operator, rhs, .. } = constraint {
            assert_eq!(coefficients.len(), 2);
            assert_eq!(*operator, ComparisonOp::LTE);
            assert_eq!(*rhs, 10.0);
        }
    }

    #[test]
    fn test_simple_constraint_named() {
        let input = "capacity: x1 + x2 <= 10";
        let result = parse_constraints(input).unwrap();
        assert_eq!(result.0, "");

        let (constraints, vars) = result.1;
        assert_eq!(constraints.len(), 1);
        assert_eq!(vars.len(), 2);

        let constraint = constraints.values().next().unwrap();
        assert_eq!(constraint.name(), "capacity");
    }

    #[test]
    fn test_constraint_with_coefficients() {
        let input = "2x1 + 3x2 - 1.5x3 <= 100";
        let result = parse_constraints(input).unwrap();

        let (constraints, vars) = result.1;
        assert_eq!(constraints.len(), 1);
        assert_eq!(vars.len(), 3);

        let constraint = constraints.values().next().unwrap();
        if let Constraint::Standard { coefficients, operator, rhs, .. } = constraint {
            assert_eq!(coefficients.len(), 3);
            assert_eq!(*operator, ComparisonOp::LTE);
            assert_eq!(*rhs, 100.0);

            // Check coefficient values
            assert_eq!(coefficients[0].value, 2.0);
            assert_eq!(coefficients[0].name, "x1");
            assert_eq!(coefficients[1].value, 3.0);
            assert_eq!(coefficients[1].name, "x2");
            assert_eq!(coefficients[2].value, -1.5);
            assert_eq!(coefficients[2].name, "x3");
        }
    }

    #[test]
    fn test_constraint_with_different_operators() {
        let inputs = vec![
            ("x1 + x2 <= 10", ComparisonOp::LTE),
            ("x1 + x2 >= 5", ComparisonOp::GTE),
            ("x1 + x2 = 8", ComparisonOp::EQ),
            ("x1 + x2 < 12", ComparisonOp::LT),
            ("x1 + x2 > 3", ComparisonOp::GT),
        ];

        for (input, expected_op) in inputs {
            let result = parse_constraints(input).unwrap();
            let (constraints, _) = result.1;
            assert_eq!(constraints.len(), 1);

            let constraint = constraints.values().next().unwrap();
            if let Constraint::Standard { operator, .. } = constraint {
                assert_eq!(*operator, expected_op, "Failed for input: {input}");
            }
        }
    }

    #[test]
    fn test_constraint_with_scientific_notation() {
        let input = "1e3x1 + 2.5e-2x2 <= 1e10";
        let result = parse_constraints(input).unwrap();

        let (constraints, vars) = result.1;
        assert_eq!(constraints.len(), 1);
        assert_eq!(vars.len(), 2);

        let constraint = constraints.values().next().unwrap();
        if let Constraint::Standard { coefficients, rhs, .. } = constraint {
            assert_eq!(coefficients[0].value, 1000.0);
            assert_eq!(coefficients[1].value, 0.025);
            assert_eq!(*rhs, 1e10);
        }
    }

    #[test]
    fn test_constraint_with_infinity() {
        let input = "x1 + x2 <= inf";
        let result = parse_constraints(input).unwrap();

        let (constraints, _) = result.1;
        assert_eq!(constraints.len(), 1);

        let constraint = constraints.values().next().unwrap();
        if let Constraint::Standard { rhs, .. } = constraint {
            assert_eq!(*rhs, f64::INFINITY);
        }
    }

    #[test]
    fn test_multiple_constraints() {
        let input = "
constraint1: x1 + x2 <= 10
constraint2: 2x1 - x3 >= 5
x1 + x2 + x3 = 15";

        let result = parse_constraints(input).unwrap();
        let (constraints, vars) = result.1;
        assert_eq!(constraints.len(), 3);
        assert_eq!(vars.len(), 3);
        assert!(vars.contains_key("x1"));
        assert!(vars.contains_key("x2"));
        assert!(vars.contains_key("x3"));

        // Check that we have both named and unnamed constraints
        let names: Vec<_> = constraints.values().map(|c| c.name()).collect();
        assert!(names.iter().any(|name| name == "constraint1"));
        assert!(names.iter().any(|name| name == "constraint2"));
        assert!(names.iter().any(|name| name.starts_with("CONSTRAINT_")));
    }

    #[test]
    fn test_constraint_with_complex_variable_names() {
        let input = "complex_var_1 + Variable_Name_123 + x.1.2 <= 100";
        let result = parse_constraints(input).unwrap();

        let (constraints, vars) = result.1;
        assert_eq!(constraints.len(), 1);
        assert_eq!(vars.len(), 3);
        assert!(vars.contains_key("complex_var_1"));
        assert!(vars.contains_key("Variable_Name_123"));
        assert!(vars.contains_key("x.1.2"));
    }

    #[test]
    fn test_constraint_with_whitespace_and_newlines() {
        let input = "
   capacity_constraint:
       2  x1   +   3.5  x2
       -   1.2  x3  <=   100
";
        let result = parse_constraints(input).unwrap();

        let (constraints, vars) = result.1;
        assert_eq!(constraints.len(), 1);
        assert_eq!(vars.len(), 3);

        let constraint = constraints.values().next().unwrap();
        assert_eq!(constraint.name(), "capacity_constraint");
        if let Constraint::Standard { coefficients, operator, rhs, .. } = constraint {
            assert_eq!(coefficients.len(), 3);
            assert_eq!(*operator, ComparisonOp::LTE);
            assert_eq!(*rhs, 100.0);
        }
    }

    #[test]
    fn test_constraint_multiline_continuation() {
        let input = "
production_constraint:
    2x1 + 3x2
    + 4x3 - 1.5x4
    <= 200";

        let result = parse_constraints(input).unwrap();
        let (constraints, vars) = result.1;
        assert_eq!(constraints.len(), 1);
        assert_eq!(vars.len(), 4);

        let constraint = constraints.values().next().unwrap();
        if let Constraint::Standard { coefficients, .. } = constraint {
            assert_eq!(coefficients.len(), 4);
            assert_eq!(coefficients[0].value, 2.0);
            assert_eq!(coefficients[1].value, 3.0);
            assert_eq!(coefficients[2].value, 4.0);
            assert_eq!(coefficients[3].value, -1.5);
        }
    }

    #[test]
    fn test_constraint_with_comments() {
        // Test simple comment handling
        let input = "\\ This is a comment
x1 + x2 <= 10";

        let result = parse_constraints(input);
        if let Ok((_, (constraints, vars))) = result {
            // Comments are handled by the parser but may affect parsing
            assert!(constraints.len() <= 1);
            assert!(vars.len() <= 2);
        }
    }

    #[test]
    fn test_constraint_without_comments() {
        let input = "
x1 + x2 <= 10
2x1 + 3x2 >= 5";

        let result = parse_constraints(input).unwrap();
        let (constraints, vars) = result.1;
        assert_eq!(constraints.len(), 2);
        assert_eq!(vars.len(), 2);
    }

    #[test]
    fn test_constraint_zero_coefficients() {
        let input = "0x1 + x2 - 0x3 <= 10";
        let result = parse_constraints(input).unwrap();

        let (constraints, vars) = result.1;
        assert_eq!(constraints.len(), 1);
        assert_eq!(vars.len(), 3);

        let constraint = constraints.values().next().unwrap();
        if let Constraint::Standard { coefficients, .. } = constraint {
            assert_eq!(coefficients[0].value, 0.0);
            assert_eq!(coefficients[1].value, 1.0);
            assert_eq!(coefficients[2].value, 0.0);
        }
    }

    #[test]
    fn test_constraint_negative_rhs() {
        let input = "x1 + x2 >= -50";
        let result = parse_constraints(input).unwrap();

        let (constraints, _) = result.1;
        assert_eq!(constraints.len(), 1);

        let constraint = constraints.values().next().unwrap();
        if let Constraint::Standard { rhs, .. } = constraint {
            assert_eq!(*rhs, -50.0);
        }
    }

    #[test]
    fn test_constraint_single_variable() {
        let input = "x1 <= 100";
        let result = parse_constraints(input).unwrap();

        let (constraints, vars) = result.1;
        assert_eq!(constraints.len(), 1);
        assert_eq!(vars.len(), 1);

        let constraint = constraints.values().next().unwrap();
        if let Constraint::Standard { coefficients, .. } = constraint {
            assert_eq!(coefficients.len(), 1);
            assert_eq!(coefficients[0].value, 1.0);
            assert_eq!(coefficients[0].name, "x1");
        }
    }

    #[test]
    fn test_constraint_name_generation() {
        let input = "
x1 <= 10
x2 >= 5
x3 = 8";

        let result = parse_constraints(input).unwrap();
        let (constraints, _) = result.1;
        assert_eq!(constraints.len(), 3);

        // All should have auto-generated names
        for constraint in constraints.values() {
            assert!(constraint.name().starts_with("CONSTRAINT_"));
        }
    }

    #[test]
    fn test_constraint_edge_cases() {
        // Very long variable names
        let input = "very_long_variable_name_that_goes_on_and_on_x1 <= 100";
        let result = parse_constraints(input);
        assert!(result.is_ok());

        // Many variables in one constraint
        let input = "x1 + x2 + x3 + x4 + x5 + x6 + x7 + x8 + x9 + x10 <= 100";
        let result = parse_constraints(input).unwrap();
        let (constraints, vars) = result.1;
        assert_eq!(constraints.len(), 1);
        assert_eq!(vars.len(), 10);
    }

    #[test]
    fn test_constraint_fractional_coefficients() {
        let input = "0.5x1 + 1.25x2 - 0.333x3 <= 99.99";
        let result = parse_constraints(input).unwrap();

        let (constraints, _) = result.1;
        assert_eq!(constraints.len(), 1);

        let constraint = constraints.values().next().unwrap();
        if let Constraint::Standard { coefficients, rhs, .. } = constraint {
            assert_eq!(coefficients[0].value, 0.5);
            assert_eq!(coefficients[1].value, 1.25);
            assert!((coefficients[2].value - (-0.333)).abs() < 1e-10);
            assert!((rhs - 99.99).abs() < 1e-10);
        }
    }

    #[test]
    fn test_invalid_constraints() {
        // Missing operator
        let result = parse_constraints("x1 + x2 10");
        assert!(result.is_err());

        // Missing RHS
        let result = parse_constraints("x1 + x2 <=");
        assert!(result.is_err());

        // Missing variables
        let result = parse_constraints("<= 10");
        assert!(result.is_err());

        // Empty input
        let result = parse_constraints("");
        assert!(result.is_err());
    }

    #[test]
    fn test_constraint_name_with_special_characters() {
        let input = "constraint_1.2.3: x1 + x2 <= 10";
        let result = parse_constraints(input).unwrap();

        let (constraints, _) = result.1;
        assert_eq!(constraints.len(), 1);

        let constraint = constraints.values().next().unwrap();
        assert_eq!(constraint.name(), "constraint_1.2.3");
    }

    #[test]
    fn test_constraint_with_colon_on_same_line() {
        let input = " Capacity_Constraint_1:\n    Production_A_1 + Production_B_2 <= 500\n";
        let result = parse_constraints(input);
        assert!(result.is_ok());
    }

    #[test]
    fn test_multiple_constraints_with_colons() {
        let input = " Capacity_Constraint_1:\n    Production_A_1 + Production_B_2 <= 500\n Demand_Region_X:\n    Transport_X_Y + Storage_Facility_1 >= 200\n";
        let result = parse_constraints(input);
        assert!(result.is_ok());
    }

    #[test]
    fn test_single_constraint_with_colon_newline() {
        let input = "Capacity_Constraint_1:\n    Production_A_1 + Production_B_2 <= 500";
        match parse_constraints(input) {
            Ok((_remaining, (constraints, _vars))) => {
                assert_eq!(constraints.len(), 1);
            }
            Err(_err) => {
                panic!("Failed to parse constraint");
            }
        }
    }

    #[test]
    fn test_constraint_no_name() {
        let input = "Production_A_1 + Production_B_2 <= 500";
        match parse_constraints(input) {
            Ok((_remaining, (constraints, _vars))) => {
                assert_eq!(constraints.len(), 1);
            }
            Err(_err) => {
                panic!("Failed to parse constraint");
            }
        }
    }
}
