use nom::{
    branch::alt,
    character::complete::{char, space0},
    combinator::{map, opt},
    sequence::{preceded, tuple},
    IResult,
};

use crate::nom::{
    decoder::{number::parse_num_value, variable::parse_variable},
    model::Coefficient,
};

#[inline]
pub fn parse_coefficient(input: &str) -> IResult<&str, Coefficient> {
    map(
        tuple((
            opt(preceded(space0, alt((char('+'), char('-'))))),
            opt(preceded(space0, parse_num_value)),
            preceded(space0, parse_variable),
        )),
        |(sign, coef, var_name)| Coefficient {
            var_name,
            coefficient: {
                let base_coef = coef.unwrap_or(1.0);
                if sign == Some('-') {
                    -1.0 * base_coef
                } else {
                    base_coef
                }
            },
        },
    )(input)
}
