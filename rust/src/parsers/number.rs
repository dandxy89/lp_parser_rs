//! Parsers for numeric values and comparison operators in LP files.
//!
//! This module provides parsers for handling various numeric formats including:
//! - Regular numbers (integer and floating-point)
//! - Scientific notation
//! - Infinity values (positive and negative)
//! - Comparison operators
//!

use nom::branch::alt;
use nom::bytes::complete::{tag, tag_no_case, take};
use nom::character::complete::{char, digit1, multispace0, one_of};
use nom::combinator::{complete, eof, map, opt, peek, recognize, value, verify};
use nom::error::{Error, ErrorKind};
use nom::sequence::{pair, preceded};
use nom::{Err, IResult, Parser as _};

use crate::model::ComparisonOp;

#[inline]
/// Parses infinity values from the input string.
fn parse_infinity(input: &str) -> IResult<&str, f64> {
    map(
        (
            opt(one_of("+-")),
            alt((tag_no_case("infinity"), tag_no_case("inf"))),
            peek(alt((eof, verify(take(1_usize), |c: &str| !c.chars().next().unwrap().is_alphanumeric())))),
        ),
        |(sign, _, _)| match sign {
            Some('-') => f64::NEG_INFINITY,
            _ => f64::INFINITY,
        },
    )
    .parse(input)
}

#[inline]
/// Fast direct f64 parsing without intermediate string allocation
fn parse_f64_direct(input: &str) -> IResult<&str, f64> {
    let (remainder, matched) = recognize((
        opt(one_of("+-")),
        digit1,
        opt(pair(char('.'), opt(digit1))),
        opt(complete((alt((char('e'), char('E'))), opt(one_of("+-")), digit1))),
    ))
    .parse(input)?;

    // Parse directly using fast_float or lexical for better performance
    matched.parse::<f64>().map_or_else(|_| Err(Err::Error(Error::new(input, ErrorKind::Verify))), |value| Ok((remainder, value)))
}

#[inline]
/// Parses a numeric value with optional whitespace, handling both regular numbers and infinity.
pub fn parse_num_value(input: &str) -> IResult<&str, f64> {
    preceded(multispace0, alt((parse_infinity, parse_f64_direct))).parse(input)
}

#[inline]
/// Parses comparison operators used in constraints.
pub fn parse_cmp_op(input: &str) -> IResult<&str, ComparisonOp> {
    preceded(
        multispace0,
        alt((
            value(ComparisonOp::EQ, tag("=")),
            value(ComparisonOp::LTE, tag("<=")),
            value(ComparisonOp::GTE, tag(">=")),
            value(ComparisonOp::LT, tag("<")),
            value(ComparisonOp::GT, tag(">")),
        )),
    )
    .parse(input)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ComparisonOp;

    #[test]
    fn test_basic_integers() {
        let result = parse_num_value("42").unwrap();
        assert_eq!(result.0, "");
        assert_eq!(result.1, 42.0);

        let result = parse_num_value("0").unwrap();
        assert_eq!(result.0, "");
        assert_eq!(result.1, 0.0);

        let result = parse_num_value("999999").unwrap();
        assert_eq!(result.0, "");
        assert_eq!(result.1, 999999.0);
    }

    #[test]
    fn test_signed_integers() {
        let result = parse_num_value("+42").unwrap();
        assert_eq!(result.0, "");
        assert_eq!(result.1, 42.0);

        let result = parse_num_value("-42").unwrap();
        assert_eq!(result.0, "");
        assert_eq!(result.1, -42.0);

        let result = parse_num_value("+0").unwrap();
        assert_eq!(result.0, "");
        assert_eq!(result.1, 0.0);

        let result = parse_num_value("-0").unwrap();
        assert_eq!(result.0, "");
        assert_eq!(result.1, -0.0);
    }

    #[test]
    #[allow(clippy::approx_constant)]
    fn test_floating_point_numbers() {
        let result = parse_num_value("3.14159").unwrap();
        assert_eq!(result.0, "");
        assert!((result.1 - 3.14159).abs() < 1e-10);

        let result = parse_num_value("0.5").unwrap();
        assert_eq!(result.0, "");
        assert_eq!(result.1, 0.5);

        let result = parse_num_value("123.").unwrap();
        assert_eq!(result.0, "");
        assert_eq!(result.1, 123.0);

        // Should fail as we don't support leading decimal
        assert!(parse_num_value(".456").is_err());
    }

    #[test]
    #[allow(clippy::approx_constant)]
    fn test_signed_floating_point() {
        let result = parse_num_value("+3.14").unwrap();
        assert_eq!(result.0, "");
        assert!((result.1 - 3.14).abs() < 1e-10);

        let result = parse_num_value("-3.14").unwrap();
        assert_eq!(result.0, "");
        assert!((result.1 - (-3.14)).abs() < 1e-10);

        let result = parse_num_value("-0.0001").unwrap();
        assert_eq!(result.0, "");
        assert!((result.1 - (-0.0001)).abs() < 1e-10);
    }

    #[test]
    fn test_scientific_notation() {
        let result = parse_num_value("1e5").unwrap();
        assert_eq!(result.0, "");
        assert!((result.1 - 100000.0).abs() < 1e-5);

        let result = parse_num_value("1E5").unwrap();
        assert_eq!(result.0, "");
        assert!((result.1 - 100000.0).abs() < 1e-5);

        let result = parse_num_value("1.5e3").unwrap();
        assert_eq!(result.0, "");
        assert!((result.1 - 1500.0).abs() < 1e-5);

        let result = parse_num_value("1.5E-3").unwrap();
        assert_eq!(result.0, "");
        assert!((result.1 - 0.0015).abs() < 1e-10);

        let result = parse_num_value("2.5e+10").unwrap();
        assert_eq!(result.0, "");
        assert!((result.1 - 25000000000.0).abs() < 1e5);
    }

    #[test]
    fn test_scientific_notation_edge_cases() {
        let result = parse_num_value("1e0").unwrap();
        assert_eq!(result.0, "");
        assert_eq!(result.1, 1.0);

        let result = parse_num_value("1e-0").unwrap();
        assert_eq!(result.0, "");
        assert_eq!(result.1, 1.0);

        let result = parse_num_value("-1.23e-45").unwrap();
        assert_eq!(result.0, "");
        assert!((result.1 - (-1.23e-45)).abs() < 1e-50);

        let result = parse_num_value("9.999e+99").unwrap();
        assert_eq!(result.0, "");
        assert!(result.1.is_finite());
    }

    #[test]
    fn test_infinity_variants() {
        let test_cases = vec![
            ("infinity", f64::INFINITY),
            ("INFINITY", f64::INFINITY),
            ("Infinity", f64::INFINITY),
            ("inf", f64::INFINITY),
            ("INF", f64::INFINITY),
            ("Inf", f64::INFINITY),
            ("+infinity", f64::INFINITY),
            ("+inf", f64::INFINITY),
            ("-infinity", f64::NEG_INFINITY),
            ("-INFINITY", f64::NEG_INFINITY),
            ("-Infinity", f64::NEG_INFINITY),
            ("-inf", f64::NEG_INFINITY),
            ("-INF", f64::NEG_INFINITY),
            ("-Inf", f64::NEG_INFINITY),
        ];

        for (input, expected) in test_cases {
            let result = parse_infinity(input).unwrap();
            assert_eq!(result.0, "");
            assert_eq!(result.1, expected, "Failed for input: {input}");
        }
    }

    #[test]
    fn test_infinity_error_cases() {
        let error_cases = vec!["notinfinity", "infx", "infinit", "in", "++inf", "--inf", "inf1", "infinity1", "1inf", "infinityy"];

        for input in error_cases {
            assert!(parse_infinity(input).is_err(), "Should fail for input: {input}");
        }
    }

    #[test]
    fn test_parse_num_value_with_infinity() {
        let result = parse_num_value("inf").unwrap();
        assert_eq!(result.0, "");
        assert_eq!(result.1, f64::INFINITY);

        let result = parse_num_value("-infinity").unwrap();
        assert_eq!(result.0, "");
        assert_eq!(result.1, f64::NEG_INFINITY);
    }

    #[test]
    #[allow(clippy::approx_constant)]
    fn test_whitespace_handling() {
        let result = parse_num_value("  42").unwrap();
        assert_eq!(result.0, "");
        assert_eq!(result.1, 42.0);

        let result = parse_num_value("\t\n  3.14").unwrap();
        assert_eq!(result.0, "");
        assert!((result.1 - 3.14).abs() < 1e-10);

        let result = parse_num_value("   -1.5e3").unwrap();
        assert_eq!(result.0, "");
        assert!((result.1 - (-1500.0)).abs() < 1e-5);

        let result = parse_num_value("  inf").unwrap();
        assert_eq!(result.0, "");
        assert_eq!(result.1, f64::INFINITY);
    }

    #[test]
    #[allow(clippy::approx_constant)]
    fn test_trailing_content() {
        let result = parse_num_value("42 + x").unwrap();
        assert_eq!(result.0, " + x");
        assert_eq!(result.1, 42.0);

        let result = parse_num_value("3.14end").unwrap();
        assert_eq!(result.0, "end");
        assert!((result.1 - 3.14).abs() < 1e-10);

        let result = parse_num_value("inf rest").unwrap();
        assert_eq!(result.0, " rest");
        assert_eq!(result.1, f64::INFINITY);
    }

    #[test]
    fn test_invalid_number_formats() {
        let invalid_cases = vec!["", "abc", "e5", "+", "-", "+-1", "++1", "--1"];

        for input in invalid_cases {
            assert!(parse_num_value(input).is_err(), "Should fail for input: '{input}'");
        }
    }

    #[test]
    fn test_incomplete_scientific_notation() {
        // These might parse as partial numbers
        let result = parse_num_value("1e");
        let (remainder, value) = result.unwrap();
        assert_eq!(remainder, "e"); // Should parse "1" and leave "e"
        assert_eq!(value, 1.0);

        let result = parse_num_value("1e+");
        let (remainder, value) = result.unwrap();
        assert_eq!(remainder, "e+"); // Should parse "1" and leave "e+"
        assert_eq!(value, 1.0);

        let result = parse_num_value("1e-");
        let (remainder, value) = result.unwrap();
        assert_eq!(remainder, "e-"); // Should parse "1" and leave "e-"
        assert_eq!(value, 1.0);

        // These should be fully invalid
        assert!(
            parse_num_value("1ee5").is_err() || {
                let r = parse_num_value("1ee5").unwrap();
                r.0 == "ee5" && r.1 == 1.0
            }
        );

        let result = parse_num_value("1.e.5");
        let (remainder, value) = result.unwrap();
        // Could parse "1." and leave "e.5" or parse "1.e" and leave ".5"
        assert!(remainder == "e.5" || remainder == ".5");
        assert!((value - 1.0).abs() < 1e-10);
    }

    #[test]
    fn test_partial_valid_number_formats() {
        // These parse partially valid numbers, leaving remainder
        let result = parse_num_value("1.2.3").unwrap();
        assert_eq!(result.0, ".3"); // Should parse "1.2" and leave ".3"
        assert!((result.1 - 1.2).abs() < 1e-10);
    }

    #[test]
    fn test_comparison_operators() {
        let test_cases = vec![
            ("=", ComparisonOp::EQ),
            ("<=", ComparisonOp::LTE),
            (">=", ComparisonOp::GTE),
            ("<", ComparisonOp::LT),
            (">", ComparisonOp::GT),
        ];

        for (input, expected) in test_cases {
            let result = parse_cmp_op(input).unwrap();
            assert_eq!(result.0, "");
            assert_eq!(result.1, expected, "Failed for input: {input}");
        }
    }

    #[test]
    fn test_comparison_operators_with_whitespace() {
        let result = parse_cmp_op("  <=").unwrap();
        assert_eq!(result.0, "");
        assert_eq!(result.1, ComparisonOp::LTE);

        let result = parse_cmp_op("\t\n>=").unwrap();
        assert_eq!(result.0, "");
        assert_eq!(result.1, ComparisonOp::GTE);

        let result = parse_cmp_op("   = rest").unwrap();
        assert_eq!(result.0, " rest");
        assert_eq!(result.1, ComparisonOp::EQ);
    }

    #[test]
    fn test_comparison_operators_invalid() {
        let invalid_cases = vec!["", "!=", "abc"];

        for input in invalid_cases {
            assert!(parse_cmp_op(input).is_err(), "Should fail for input: '{input}'");
        }
    }

    #[test]
    fn test_comparison_operators_edge_cases() {
        // "=>" parses "=" and leaves ">"
        let result = parse_cmp_op("=>").unwrap();
        assert_eq!(result.0, ">"); // Should parse "=" and leave ">"
        assert_eq!(result.1, ComparisonOp::EQ);

        // "=<" parses "=" and leaves "<"
        let result = parse_cmp_op("=<").unwrap();
        assert_eq!(result.0, "<"); // Should parse "=" and leave "<"
        assert_eq!(result.1, ComparisonOp::EQ);
    }

    #[test]
    fn test_comparison_operators_partial_invalid() {
        // "<>" parses "<" and leaves ">"
        let result = parse_cmp_op("<>").unwrap();
        assert_eq!(result.0, ">"); // Should parse "<" and leave ">"
        assert_eq!(result.1, ComparisonOp::LT);
    }

    #[test]
    fn test_comparison_operators_partial_match() {
        // These parse valid operators but leave remainder
        let result = parse_cmp_op("==").unwrap();
        assert_eq!(result.0, "="); // Should parse "=" and leave "="
        assert_eq!(result.1, ComparisonOp::EQ);

        let result = parse_cmp_op("<<").unwrap();
        assert_eq!(result.0, "<"); // Should parse "<" and leave "<"
        assert_eq!(result.1, ComparisonOp::LT);

        let result = parse_cmp_op(">>").unwrap();
        assert_eq!(result.0, ">"); // Should parse ">" and leave ">"
        assert_eq!(result.1, ComparisonOp::GT);
    }

    #[test]
    fn test_boundary_values() {
        let result = parse_num_value("0").unwrap();
        assert_eq!(result.1, 0.0);

        let result = parse_num_value("1.7976931348623157e+308").unwrap();
        assert!(result.1.is_finite());

        let result = parse_num_value("2.2250738585072014e-308").unwrap();
        assert!(result.1.is_finite());
        assert!(result.1 > 0.0);

        // Test very small numbers close to underflow
        let result = parse_num_value("1e-323").unwrap();
        assert!(result.1.is_finite());
    }

    #[test]
    fn test_edge_case_scientific_notation() {
        // Zero with exponent
        let result = parse_num_value("0e10").unwrap();
        assert_eq!(result.1, 0.0);

        let result = parse_num_value("0.0e-5").unwrap();
        assert_eq!(result.1, 0.0);

        // Large exponents
        let result = parse_num_value("1e308").unwrap();
        assert!(result.1.is_finite());

        let result = parse_num_value("1e309").unwrap();
        assert_eq!(result.1, f64::INFINITY);

        let result = parse_num_value("1e-324").unwrap();
        assert_eq!(result.1, 0.0);
    }
}
