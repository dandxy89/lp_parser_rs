//! Parser for variable declarations and bounds in LP files.
//!

use nom::character::complete::multispace0;
use nom::error::{Error, ErrorKind};
use nom::multi::many0;
use nom::{Err, IResult, Parser as _};

use crate::model::VariableType;
use crate::parsers::parser_traits::{
    BinaryParser, BoundsParser, GeneralParser, IntegerParser, SectionParser as _, SemiParser, parse_variable,
};
use crate::{ALL_BOUND_HEADERS, log_unparsed_content};

#[inline]
/// Checks if the input string is the start of a section header.
fn is_section_header(input: &str) -> bool {
    let lower_input = input.trim().to_lowercase();
    ALL_BOUND_HEADERS.iter().any(|&header| lower_input.starts_with(header))
}

#[inline]
/// Parses a variable name that is not the start of a section header.
fn variable_not_header(input: &str) -> IResult<&str, &str> {
    let (input, _) = multispace0(input)?;
    if is_section_header(input) {
        return Err(Err::Error(Error::new(input, ErrorKind::Not)));
    }
    parse_variable(input)
}

#[inline]
/// Parses a list of variables until a section header is encountered.
pub fn parse_variable_list(input: &str) -> IResult<&str, Vec<&str>> {
    many0(variable_not_header).parse(input)
}

#[inline]
/// Parses a bounds section from the input string.
pub fn parse_bounds_section(input: &str) -> IResult<&str, Vec<(&str, VariableType)>> {
    let (remaining, section) = BoundsParser::parse_section(input)?;
    log_unparsed_content("Failed to parse bounds fully", remaining);
    Ok(("", section))
}

#[inline]
/// Parses a binary variables section.
pub fn parse_binary_section(input: &str) -> IResult<&str, Vec<&str>> {
    let (remaining, section) = BinaryParser::parse_section(input)?;
    log_unparsed_content("Failed to parse binaries fully", remaining);
    Ok(("", section))
}

#[inline]
/// Parses a generals variables section.
pub fn parse_generals_section(input: &str) -> IResult<&str, Vec<&str>> {
    let (remaining, section) = GeneralParser::parse_section(input)?;
    log_unparsed_content("Failed to parse generals fully", remaining);
    Ok(("", section))
}

#[inline]
/// Parses a general integer variables section.
pub fn parse_integer_section(input: &str) -> IResult<&str, Vec<&str>> {
    let (remaining, section) = IntegerParser::parse_section(input)?;
    log_unparsed_content("Failed to parse integers fully", remaining);
    Ok(("", section))
}

#[inline]
/// Parses a semi-continuous variables section.
pub fn parse_semi_section(input: &str) -> IResult<&str, Vec<&str>> {
    let (remaining, section) = SemiParser::parse_section(input)?;
    log_unparsed_content("Failed to parse semi-continuous fully", remaining);
    Ok(("", section))
}

#[cfg(test)]
mod test {
    use crate::parsers::variable::{parse_bounds_section, parse_generals_section, parse_integer_section, parse_semi_section};

    #[test]
    fn test_bounds() {
        let input = "
bounds
x1 free
x2 >= 1
x2 >= inf
100 <= x2dfsdf <= -1
-infinity <= qwer < +inf";
        let (remaining, bounds) = parse_bounds_section(input).unwrap();
        assert_eq!(remaining, "");
        assert_eq!(bounds.len(), 5);
    }

    #[test]
    fn test_generals() {
        let input = "
Generals
b_5829890_x2 b_5880854_x2";
        let (remaining, bounds) = parse_generals_section(input).unwrap();
        assert_eq!(remaining, "");
        assert_eq!(bounds.len(), 2);
    }

    #[test]
    fn test_integers() {
        let input = "
Integers
X31
X32";
        let (remaining, bounds) = parse_integer_section(input).unwrap();
        assert_eq!(remaining, "");
        assert_eq!(bounds.len(), 2);
    }

    #[test]
    fn test_semi() {
        let input = "
Semi-Continuous
 y";
        let (remaining, bounds) = parse_semi_section(input).unwrap();
        assert_eq!(remaining, "");
        assert_eq!(bounds.len(), 1);
    }
}
