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
    match matched.parse::<f64>() {
        Ok(value) => Ok((remainder, value)),
        Err(_) => Err(Err::Error(Error::new(input, ErrorKind::Verify))),
    }
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
    use crate::parsers::number::{parse_infinity, parse_num_value};

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
}
