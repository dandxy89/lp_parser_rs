//! Parser for coefficients in LP problems.
//!
//! This module provides functionality for parsing coefficients
//! that appear in objective functions and constraints.
//!

use nom::branch::alt;
use nom::character::complete::{char, space0};
use nom::combinator::{map, opt};
use nom::sequence::preceded;
use nom::{IResult, Parser as _};

use crate::model::Coefficient;
use crate::parsers::number::parse_num_value;
use crate::parsers::parser_traits::parse_variable;

#[inline]
/// Parses a coefficient from the given input string.
///
/// This function attempts to parse a coefficient, which may include an optional
/// sign ('+' or '-') and an optional numeric value, followed by a variable name.
/// If the numeric value is not provided, it defaults to 1.0. The sign, if present,
/// will determine the sign of the coefficient.
///
pub fn parse_coefficient(input: &str) -> IResult<&str, Coefficient<'_>> {
    map(
        preceded(space0, (opt(alt((char('+'), char('-')))), opt(parse_num_value), preceded(space0, parse_variable))),
        |(sign, coef, var_name)| {
            let base_coef = coef.unwrap_or(1.0);
            let value = match sign {
                Some('-') => -base_coef,
                _ => base_coef,
            };
            Coefficient { name: var_name, value }
        },
    )
    .parse(input)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_positive_coefficient() {
        let result = parse_coefficient("x1").unwrap();
        assert_eq!(result.0, "");
        assert_eq!(result.1.name, "x1");
        assert_eq!(result.1.value, 1.0);

        let result = parse_coefficient("variable_name").unwrap();
        assert_eq!(result.0, "");
        assert_eq!(result.1.name, "variable_name");
        assert_eq!(result.1.value, 1.0);

        let result = parse_coefficient("x_123").unwrap();
        assert_eq!(result.0, "");
        assert_eq!(result.1.name, "x_123");
        assert_eq!(result.1.value, 1.0);
    }

    #[test]
    fn test_explicit_positive_coefficient() {
        let result = parse_coefficient("+x1").unwrap();
        assert_eq!(result.0, "");
        assert_eq!(result.1.name, "x1");
        assert_eq!(result.1.value, 1.0);

        let result = parse_coefficient("+ y2").unwrap();
        assert_eq!(result.0, "");
        assert_eq!(result.1.name, "y2");
        assert_eq!(result.1.value, 1.0);

        let result = parse_coefficient("+  z3").unwrap();
        assert_eq!(result.0, "");
        assert_eq!(result.1.name, "z3");
        assert_eq!(result.1.value, 1.0);
    }

    #[test]
    fn test_negative_coefficient() {
        let result = parse_coefficient("-x1").unwrap();
        assert_eq!(result.0, "");
        assert_eq!(result.1.name, "x1");
        assert_eq!(result.1.value, -1.0);

        let result = parse_coefficient("- y2").unwrap();
        assert_eq!(result.0, "");
        assert_eq!(result.1.name, "y2");
        assert_eq!(result.1.value, -1.0);

        let result = parse_coefficient("-  z3").unwrap();
        assert_eq!(result.0, "");
        assert_eq!(result.1.name, "z3");
        assert_eq!(result.1.value, -1.0);
    }

    #[test]
    fn test_positive_numeric_coefficient() {
        let result = parse_coefficient("2x1").unwrap();
        assert_eq!(result.0, "");
        assert_eq!(result.1.name, "x1");
        assert_eq!(result.1.value, 2.0);

        let result = parse_coefficient("3.5y2").unwrap();
        assert_eq!(result.0, "");
        assert_eq!(result.1.name, "y2");
        assert_eq!(result.1.value, 3.5);

        let result = parse_coefficient("0.25 z3").unwrap();
        assert_eq!(result.0, "");
        assert_eq!(result.1.name, "z3");
        assert_eq!(result.1.value, 0.25);

        let result = parse_coefficient("1e3  w4").unwrap();
        assert_eq!(result.0, "");
        assert_eq!(result.1.name, "w4");
        assert_eq!(result.1.value, 1000.0);
    }

    #[test]
    fn test_explicit_positive_numeric_coefficient() {
        let result = parse_coefficient("+2x1").unwrap();
        assert_eq!(result.0, "");
        assert_eq!(result.1.name, "x1");
        assert_eq!(result.1.value, 2.0);

        let result = parse_coefficient("+ 3.5y2").unwrap();
        assert_eq!(result.0, "");
        assert_eq!(result.1.name, "y2");
        assert_eq!(result.1.value, 3.5);

        let result = parse_coefficient("+0.25 z3").unwrap();
        assert_eq!(result.0, "");
        assert_eq!(result.1.name, "z3");
        assert_eq!(result.1.value, 0.25);
    }

    #[test]
    fn test_negative_numeric_coefficient() {
        let result = parse_coefficient("-2x1").unwrap();
        assert_eq!(result.0, "");
        assert_eq!(result.1.name, "x1");
        assert_eq!(result.1.value, -2.0);

        let result = parse_coefficient("- 3.5y2").unwrap();
        assert_eq!(result.0, "");
        assert_eq!(result.1.name, "y2");
        assert_eq!(result.1.value, -3.5);

        let result = parse_coefficient("-0.25 z3").unwrap();
        assert_eq!(result.0, "");
        assert_eq!(result.1.name, "z3");
        assert_eq!(result.1.value, -0.25);

        let result = parse_coefficient("-1e-3 w4").unwrap();
        assert_eq!(result.0, "");
        assert_eq!(result.1.name, "w4");
        assert_eq!(result.1.value, -0.001);
    }

    #[test]
    fn test_scientific_notation_coefficients() {
        let result = parse_coefficient("1e5x").unwrap();
        assert_eq!(result.0, "");
        assert_eq!(result.1.name, "x");
        assert_eq!(result.1.value, 100000.0);

        let result = parse_coefficient("-2.5e-3 y").unwrap();
        assert_eq!(result.0, "");
        assert_eq!(result.1.name, "y");
        assert_eq!(result.1.value, -0.0025);

        let result = parse_coefficient("+1.23E+10 z").unwrap();
        assert_eq!(result.0, "");
        assert_eq!(result.1.name, "z");
        assert!(result.1.value > 1e10);
    }

    #[test]
    fn test_zero_coefficient() {
        let result = parse_coefficient("0x1").unwrap();
        assert_eq!(result.0, "");
        assert_eq!(result.1.name, "x1");
        assert_eq!(result.1.value, 0.0);

        let result = parse_coefficient("-0 y2").unwrap();
        assert_eq!(result.0, "");
        assert_eq!(result.1.name, "y2");
        assert_eq!(result.1.value, -0.0);

        let result = parse_coefficient("+0.0 z3").unwrap();
        assert_eq!(result.0, "");
        assert_eq!(result.1.name, "z3");
        assert_eq!(result.1.value, 0.0);
    }

    #[test]
    fn test_infinity_coefficient() {
        let result = parse_coefficient("inf x").unwrap();
        assert_eq!(result.0, "");
        assert_eq!(result.1.name, "x");
        assert_eq!(result.1.value, f64::INFINITY);

        let result = parse_coefficient("-infinity y").unwrap();
        assert_eq!(result.0, "");
        assert_eq!(result.1.name, "y");
        assert_eq!(result.1.value, f64::NEG_INFINITY);

        let result = parse_coefficient("+inf z").unwrap();
        assert_eq!(result.0, "");
        assert_eq!(result.1.name, "z");
        assert_eq!(result.1.value, f64::INFINITY);
    }

    #[test]
    #[allow(clippy::approx_constant)]
    fn test_whitespace_handling() {
        let result = parse_coefficient("  x1").unwrap();
        assert_eq!(result.0, "");
        assert_eq!(result.1.name, "x1");
        assert_eq!(result.1.value, 1.0);

        let result = parse_coefficient("\t\n  2.5 \t y2").unwrap();
        assert_eq!(result.0, "");
        assert_eq!(result.1.name, "y2");
        assert_eq!(result.1.value, 2.5);

        let result = parse_coefficient("   -   3.14   z3").unwrap();
        assert_eq!(result.0, "");
        assert_eq!(result.1.name, "z3");
        assert_eq!(result.1.value, -3.14);
    }

    #[test]
    fn test_trailing_content() {
        let result = parse_coefficient("2x + 3y").unwrap();
        assert_eq!(result.0, " + 3y");
        assert_eq!(result.1.name, "x");
        assert_eq!(result.1.value, 2.0);

        let result = parse_coefficient("-3.5y <= 10").unwrap();
        assert_eq!(result.0, " <= 10");
        assert_eq!(result.1.name, "y");
        assert_eq!(result.1.value, -3.5);

        let result = parse_coefficient("variable_name rest_of_line").unwrap();
        assert_eq!(result.0, " rest_of_line");
        assert_eq!(result.1.name, "variable_name");
        assert_eq!(result.1.value, 1.0);
    }

    #[test]
    fn test_variable_name_patterns() {
        // Single letter
        let result = parse_coefficient("x").unwrap();
        assert_eq!(result.1.name, "x");

        // Letter with numbers
        let result = parse_coefficient("x123").unwrap();
        assert_eq!(result.1.name, "x123");

        // With underscores
        let result = parse_coefficient("var_name_123").unwrap();
        assert_eq!(result.1.name, "var_name_123");

        // Mixed case
        let result = parse_coefficient("XyZ").unwrap();
        assert_eq!(result.1.name, "XyZ");

        // Starting with underscore (if allowed by parse_variable)
        let result = parse_coefficient("_variable");
        if result.is_ok() {
            assert_eq!(result.unwrap().1.name, "_variable");
        }
    }

    #[test]
    #[allow(clippy::approx_constant)]
    fn test_edge_cases() {
        // Very small coefficient
        let result = parse_coefficient("1e-100 x").unwrap();
        assert_eq!(result.1.name, "x");
        assert!(result.1.value > 0.0);
        assert!(result.1.value < 1e-99);

        // Very large coefficient
        let result = parse_coefficient("1e100 y").unwrap();
        assert_eq!(result.1.name, "y");
        assert!(result.1.value > 1e99);

        // Decimal with many digits
        let result = parse_coefficient("3.141592653589793 pi").unwrap();
        assert_eq!(result.1.name, "pi");
        assert!((result.1.value - 3.141592653589793).abs() < 1e-10);
    }

    #[test]
    fn test_invalid_coefficients() {
        // Missing variable name
        assert!(parse_coefficient("2.5").is_err());
        assert!(parse_coefficient("+").is_err());
        assert!(parse_coefficient("-").is_err());

        // Empty input
        assert!(parse_coefficient("").is_err());

        // Only whitespace
        assert!(parse_coefficient("   ").is_err());
    }

    #[test]
    fn test_unusual_but_valid_coefficients() {
        // Variable name that looks like number format but is valid
        let result = parse_coefficient("abc x").unwrap();
        assert_eq!(result.0, " x"); // "abc" is parsed as variable, leaving " x"
        assert_eq!(result.1.name, "abc");
        assert_eq!(result.1.value, 1.0);

        // Partial number parsing
        let result = parse_coefficient("2.3.4 x").unwrap();
        // Parses "2.3" as coefficient, ".4" as variable name, leaves " x"
        assert_eq!(result.0, " x");
        assert_eq!(result.1.name, ".4");
        assert_eq!(result.1.value, 2.3);
    }

    #[test]
    fn test_fractional_coefficients() {
        let result = parse_coefficient("0.5 x").unwrap();
        assert_eq!(result.1.value, 0.5);

        let result = parse_coefficient("1.25 y").unwrap();
        assert_eq!(result.1.value, 1.25);

        let result = parse_coefficient("-0.75 z").unwrap();
        assert_eq!(result.1.value, -0.75);

        let result = parse_coefficient("0.333333 w").unwrap();
        assert!((result.1.value - 0.333333).abs() < 1e-6);
    }

    #[test]
    fn test_boundary_numeric_values() {
        // Test parsing boundary numeric values
        let result = parse_coefficient("1 x").unwrap();
        assert_eq!(result.1.value, 1.0);

        let result = parse_coefficient("-1 y").unwrap();
        assert_eq!(result.1.value, -1.0);

        // Test very large numbers
        let result = parse_coefficient("999999999 z").unwrap();
        assert_eq!(result.1.value, 999999999.0);

        // Test very small positive numbers
        let result = parse_coefficient("0.000001 w").unwrap();
        assert_eq!(result.1.value, 0.000001);
    }
}
