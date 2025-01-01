use nom::{character::complete::multispace0, error::ErrorKind, multi::many0, IResult};

use crate::nom::{
    decoder::parser_traits::{parse_variable, BinaryParser, BoundsParser, GeneralParser, IntegerParser, SectionParser as _, SemiParser},
    log_remaining,
    model::VariableType,
    ALL_BOUND_HEADERS,
};

#[inline]
fn is_section_header(input: &str) -> bool {
    let lower_input = input.trim().to_lowercase();
    ALL_BOUND_HEADERS.iter().any(|&header| lower_input.starts_with(header))
}

#[inline]
fn variable_not_header(input: &str) -> IResult<&str, &str> {
    let (input, _) = multispace0(input)?;
    if is_section_header(input) {
        return Err(nom::Err::Error(nom::error::Error::new(input, ErrorKind::Not)));
    }
    parse_variable(input)
}

#[inline]
pub fn parse_variable_list(input: &str) -> IResult<&str, Vec<&str>> {
    many0(variable_not_header)(input)
}

#[inline]
pub fn parse_bounds_section(input: &str) -> IResult<&str, Vec<(&str, VariableType)>> {
    let (remaining, section) = BoundsParser::parse_section(input)?;
    log_remaining("Failed to parse bounds fully", remaining);
    Ok(("", section))
}

#[inline]
pub fn parse_binary_section(input: &str) -> IResult<&str, Vec<&str>> {
    let (remaining, section) = BinaryParser::parse_section(input)?;
    log_remaining("Failed to parse binaries fully", remaining);
    Ok(("", section))
}

#[inline]
pub fn parse_generals_section(input: &str) -> IResult<&str, Vec<&str>> {
    let (remaining, section) = GeneralParser::parse_section(input)?;
    log_remaining("Failed to parse generals fully", remaining);
    Ok(("", section))
}

#[inline]
pub fn parse_integer_section(input: &str) -> IResult<&str, Vec<&str>> {
    let (remaining, section) = IntegerParser::parse_section(input)?;
    log_remaining("Failed to parse integers fully", remaining);
    Ok(("", section))
}

#[inline]
pub fn parse_semi_section(input: &str) -> IResult<&str, Vec<&str>> {
    let (remaining, section) = SemiParser::parse_section(input)?;
    log_remaining("Failed to parse semi-continuous fully", remaining);
    Ok(("", section))
}

#[cfg(test)]
mod test {
    use crate::nom::decoder::variable::{parse_bounds_section, parse_generals_section, parse_integer_section, parse_semi_section};

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
