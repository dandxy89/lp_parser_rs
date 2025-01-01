use nom::{
    branch::alt,
    character::complete::{char, space0},
    combinator::{map, opt},
    sequence::{preceded, tuple},
    IResult,
};

use crate::{
    decoder::{number::parse_num_value, parser_traits::parse_variable},
    model::Coefficient,
};

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
        tuple((
            opt(preceded(space0, alt((char('+'), char('-'))))),
            opt(preceded(space0, parse_num_value)),
            preceded(space0, parse_variable),
        )),
        |(sign, coef, var_name)| {
            let base_coef = coef.unwrap_or(1.0);
            let coefficient = if sign == Some('-') { -base_coef } else { base_coef };
            Coefficient { var_name, coefficient }
        },
    )(input)
}
