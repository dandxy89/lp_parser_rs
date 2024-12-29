use nom::{bytes::complete::take_while1, IResult};

use crate::nom::decoder::valid_lp_char;

#[inline]
pub fn parse_variable(input: &str) -> IResult<&str, &str> {
    take_while1(valid_lp_char)(input)
}
