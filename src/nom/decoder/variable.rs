use nom::{
    branch::alt,
    bytes::complete::{tag, tag_no_case, take_while1},
    character::complete::{char, multispace0, space0},
    combinator::{map, opt},
    error::ErrorKind,
    multi::many0,
    sequence::{preceded, tuple},
    IResult,
};

use crate::nom::{decoder::number::parse_num_value, log_remaining, model::VariableType, ALL_BOUND_HEADERS, VALID_LP_CHARS};

#[inline]
fn is_valid_lp_char(c: char) -> bool {
    c.is_alphanumeric() || VALID_LP_CHARS.contains(&c)
}

#[inline]
pub fn parse_variable(input: &str) -> IResult<&str, &str> {
    take_while1(is_valid_lp_char)(input)
}

#[inline]
pub fn single_bound(input: &str) -> IResult<&str, (&str, VariableType)> {
    preceded(
        multispace0,
        alt((
            // Free variable: `x1 free`
            map(tuple((parse_variable, preceded(space0, tag_no_case("free")))), |(var_name, _)| (var_name, VariableType::Free)),
            // Double bound: `0 <= x1 <= 5`
            map(
                tuple((
                    parse_num_value,
                    preceded(space0, alt((tag("<="), tag("<")))),
                    preceded(space0, parse_variable),
                    preceded(space0, alt((tag("<="), tag("<")))),
                    preceded(space0, parse_num_value),
                )),
                |(lower, _, var_name, _, upper)| (var_name, VariableType::DoubleBound(lower, upper)),
            ),
            // Lower bound: `x1 >= 5` or `5 <= x1`
            alt((
                map(tuple((parse_variable, preceded(space0, tag(">=")), preceded(space0, parse_num_value))), |(var_name, _, bound)| {
                    (var_name, VariableType::LowerBound(bound))
                }),
                map(tuple((parse_num_value, preceded(space0, tag("<=")), preceded(space0, parse_variable))), |(bound, _, var_name)| {
                    (var_name, VariableType::LowerBound(bound))
                }),
            )),
            // Upper bound: `x1 <= 5` or `5 >= x1`
            alt((
                map(tuple((parse_variable, preceded(space0, tag("<=")), preceded(space0, parse_num_value))), |(var_name, _, bound)| {
                    (var_name, VariableType::UpperBound(bound))
                }),
                map(tuple((parse_num_value, preceded(space0, tag(">=")), preceded(space0, parse_variable))), |(bound, _, var_name)| {
                    (var_name, VariableType::UpperBound(bound))
                }),
            )),
        )),
    )(input)
}

#[inline]
pub fn parse_bounds_section(input: &str) -> IResult<&str, Vec<(&str, VariableType)>> {
    let (remaining, section) =
        preceded(tuple((multispace0, tag_no_case("bounds"), opt(preceded(space0, char(':'))), multispace0)), many0(single_bound))(input)?;

    log_remaining("Failed to parse bounds fully", remaining);
    Ok(("", section))
}

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
pub fn parse_generals_section(input: &str) -> IResult<&str, Vec<&str>> {
    if input.is_empty() || input == "\n" {
        return Ok(("", Vec::with_capacity(0)));
    }

    let (remaining, section) = preceded(
        tuple((multispace0, alt((tag_no_case("generals"), tag_no_case("general"))), opt(preceded(space0, char(':'))), multispace0)),
        parse_variable_list,
    )(input)?;

    log_remaining("Failed to parse generals fully", remaining);
    Ok(("", section))
}

#[inline]
pub fn parse_integer_section(input: &str) -> IResult<&str, Vec<&str>> {
    if input.is_empty() || input == "\n" {
        return Ok(("", Vec::with_capacity(0)));
    }

    let (remaining, section) = preceded(
        tuple((multispace0, alt((tag_no_case("integers"), tag_no_case("integer"))), opt(preceded(space0, char(':'))), multispace0)),
        parse_variable_list,
    )(input)?;

    log_remaining("Failed to parse integers fully", remaining);
    Ok(("", section))
}

#[inline]
pub fn parse_binary_section(input: &str) -> IResult<&str, Vec<&str>> {
    if input.is_empty() || input == "\n" {
        return Ok(("", Vec::with_capacity(0)));
    }

    let (remaining, section) = preceded(
        tuple((
            multispace0,
            alt((tag_no_case("binaries"), tag_no_case("binary"), tag_no_case("bin"))),
            opt(preceded(space0, char(':'))),
            multispace0,
        )),
        parse_variable_list,
    )(input)?;

    log_remaining("Failed to parse binaries fully", remaining);
    Ok(("", section))
}

#[inline]
pub fn parse_semi_section(input: &str) -> IResult<&str, Vec<&str>> {
    if input.is_empty() || input == "\n" {
        return Ok(("", Vec::with_capacity(0)));
    }

    let (remaining, section) = preceded(
        tuple((
            multispace0,
            alt((tag_no_case("semi-continuous"), tag_no_case("semis"), tag_no_case("semi"))),
            opt(preceded(space0, char(':'))),
            multispace0,
        )),
        parse_variable_list,
    )(input)?;

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
