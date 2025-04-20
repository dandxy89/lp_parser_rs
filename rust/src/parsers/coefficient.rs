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
pub fn parse_coefficient(input: &str) -> IResult<&str, Coefficient> {
    map(
        (opt(preceded(space0, alt((char('+'), char('-'))))), opt(preceded(space0, parse_num_value)), preceded(space0, parse_variable)),
        |(sign, coef, var_name)| {
            let base_coef = coef.unwrap_or(1.0);
            let value = if sign == Some('-') { -base_coef } else { base_coef };
            Coefficient { name: var_name, value }
        },
    )
    .parse(input)
}
