//! Parsers for numeric values and comparison operators in LP files.
//!
//! This module provides parsers for handling various numeric formats including:
//! - Regular numbers (integer and floating-point)
//! - Scientific notation
//! - Infinity values (positive and negative)
//! - Comparison operators
//!

use nom::{
    branch::alt,
    bytes::complete::{tag, tag_no_case, take},
    character::complete::{char, digit1, multispace0, one_of},
    combinator::{complete, eof, map, opt, peek, recognize, value, verify},
    error::{Error, ErrorKind},
    sequence::{pair, preceded, tuple},
    Err, IResult,
};

use crate::model::ComparisonOp;

#[inline]
/// Parses infinity values from the input string.
fn parse_infinity(input: &str) -> IResult<&str, f64> {
    map(
        tuple((
            opt(one_of("+-")),
            alt((tag_no_case("infinity"), tag_no_case("inf"))),
            peek(alt((eof, verify(take(1_usize), |c: &str| !c.chars().next().unwrap().is_alphanumeric())))),
        )),
        |(sign, _, _)| match sign {
            Some('-') => f64::NEG_INFINITY,
            _ => f64::INFINITY,
        },
    )(input)
}

#[inline]
/// Parses regular numeric values from the input string.
fn parse_number(input: &str) -> IResult<&str, &str> {
    let (remainder, matched) = recognize(tuple((
        // Optional sign at the start
        opt(one_of("+-")),
        // Integer part (required)
        digit1,
        // Optional decimal part
        opt(pair(char('.'), opt(digit1))),
        // Optional scientific notation part
        opt(complete(tuple((alt((char('e'), char('E'))), opt(one_of("+-")), digit1)))),
    )))(input)?;

    if remainder.starts_with('e') || remainder.starts_with('E') {
        Err(Err::Error(Error::new(input, ErrorKind::Verify)))
    } else {
        Ok((remainder, matched))
    }
}

#[inline]
/// Parses a numeric value with optional whitespace, handling both regular numbers and infinity.
pub fn parse_num_value(input: &str) -> IResult<&str, f64> {
    preceded(multispace0, alt((parse_infinity, map(parse_number, |v| v.parse::<f64>().unwrap_or_default()))))(input)
}

#[inline]
/// Parses comparison operators used in constraints.
pub fn parse_cmp_op(input: &str) -> IResult<&str, ComparisonOp> {
    preceded(
        multispace0,
        alt((
            value(ComparisonOp::LTE, tag("<=")),
            value(ComparisonOp::GTE, tag(">=")),
            value(ComparisonOp::EQ, tag("=")),
            value(ComparisonOp::LT, tag("<")),
            value(ComparisonOp::GT, tag(">")),
        )),
    )(input)
}

#[cfg(test)]
mod tests {
    use crate::parsers::number::{parse_infinity, parse_num_value, parse_number};

    #[test]
    fn test_number_value() {
        assert!(parse_num_value("inf").is_ok());
        assert!(parse_num_value("123.1").is_ok());
        assert!(parse_num_value("13e12").is_ok());
        assert!(parse_num_value("13.12e14").is_ok());
    }

    #[test]
    fn test_infinity() {
        assert_eq!(parse_infinity("infinity").unwrap().1, f64::INFINITY);
        assert_eq!(parse_infinity("INFINITY").unwrap().1, f64::INFINITY);
        assert_eq!(parse_infinity("Infinity").unwrap().1, f64::INFINITY);
        assert_eq!(parse_infinity("inf").unwrap().1, f64::INFINITY);
        assert_eq!(parse_infinity("INF").unwrap().1, f64::INFINITY);
        assert_eq!(parse_infinity("Inf").unwrap().1, f64::INFINITY);
        assert_eq!(parse_infinity("+infinity").unwrap().1, f64::INFINITY);
        assert_eq!(parse_infinity("+inf").unwrap().1, f64::INFINITY);

        assert_eq!(parse_infinity("-infinity").unwrap().1, f64::NEG_INFINITY);
        assert_eq!(parse_infinity("-INFINITY").unwrap().1, f64::NEG_INFINITY);
        assert_eq!(parse_infinity("-Infinity").unwrap().1, f64::NEG_INFINITY);
        assert_eq!(parse_infinity("-inf").unwrap().1, f64::NEG_INFINITY);
        assert_eq!(parse_infinity("-INF").unwrap().1, f64::NEG_INFINITY);
        assert_eq!(parse_infinity("-Inf").unwrap().1, f64::NEG_INFINITY);

        assert!(parse_infinity("notinfinity").is_err());
        assert!(parse_infinity("infx").is_err());
        assert!(parse_infinity("infinit").is_err());
        assert!(parse_infinity("in").is_err());
        assert!(parse_infinity("++inf").is_err());
        assert!(parse_infinity("--inf").is_err());
    }

    #[test]
    fn test_number_parser() {
        let valid_numbers = [
            "123", "+123", "-123", "123.456", "-123.456", "+123.456", "123.", "1.23e4", "1.23E4", "1.23e+4", "1.23e-4", "-1.23e-4",
            "+1.23e+4",
        ];
        for input in valid_numbers {
            assert!(parse_number(input).is_ok());
        }

        assert!(parse_number("abc").is_err());
        assert!(parse_number(".123").is_err());
        assert!(parse_number("1.23e").is_err());
    }
}
