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

use crate::{
    decoder::{number::parse_num_value, variable::parse_variable_list},
    model::VariableType,
    VALID_LP_FILE_CHARS,
};

/// A trait for parsing sections within a text input.
///
/// This trait defines methods for identifying and parsing sections
/// based on predefined headers. Implementors must provide the list
/// of section headers and a method to parse the content of each section.
///
/// # Type Parameters
///
/// - `'a`: The lifetime of the input string slice.
/// - `T`: The type of the parsed section content.
///
/// # Methods
///
/// - `section_headers`: Returns a static list of section headers.
/// - `parse_section_content`: Parses the content of a section and returns the result as type `T`.
/// - `is_section_header`: Checks if the input starts with any of the section headers, ignoring case.
/// - `parse_section`: Parses a section, including its header and content, and returns the parsed content as type `T`.
///
pub trait SectionParser<'a, T> {
    fn section_headers() -> &'static [&'static str];
    fn parse_section_content(input: &'a str) -> IResult<&'a str, T>;

    fn is_section_header(input: &str) -> IResult<&str, &str> {
        let headers = Self::section_headers();
        for &header in headers {
            if let Ok((rem, matched)) = tag_no_case::<&str, &str, nom::error::Error<_>>(header)(input) {
                return Ok((rem, matched));
            }
        }
        Err(nom::Err::Error(nom::error::Error::new(input, ErrorKind::Tag)))
    }

    fn parse_section(input: &'a str) -> IResult<&'a str, T> {
        map(
            tuple((multispace0, Self::is_section_header, opt(preceded(multispace0, char(':'))), multispace0, Self::parse_section_content)),
            |(_, _, _, _, content)| content,
        )(input)
    }
}

#[macro_export]
macro_rules! impl_section_parser {
    ($parser_type:ty, $return_type:ty, $headers:expr, $content_parser:expr) => {
        impl<'a> SectionParser<'a, $return_type> for $parser_type {
            fn section_headers() -> &'static [&'static str] {
                $headers
            }

            fn parse_section_content(input: &'a str) -> IResult<&'a str, $return_type> {
                $content_parser(input)
            }
        }
    };
}

#[inline]
fn is_valid_lp_char(c: char) -> bool {
    c.is_alphanumeric() || VALID_LP_FILE_CHARS.contains(&c)
}

#[inline]
pub fn parse_variable(input: &str) -> IResult<&str, &str> {
    take_while1(is_valid_lp_char)(input)
}

#[inline]
/// Parses a string input to identify and extract a variable with its bound type.
///
/// The function recognizes four types of variable bounds:
/// - Free variable: e.g., `x1 free`
/// - Double bound: e.g., `0 <= x1 <= 5`
/// - Lower bound: e.g., `x1 >= 5` or `5 <= x1`
/// - Upper bound: e.g., `x1 <= 5` or `5 >= x1`
///
/// # Arguments
///
/// * `input` - A string slice that holds the input to be parsed.
///
/// # Returns
///
/// * `IResult<&str, (&str, VariableType)>` - A result containing the remaining input and a tuple
///   with the variable name and its `VariableType`.
///
pub fn parse_single_bound(input: &str) -> IResult<&str, (&str, VariableType)> {
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

pub struct BinaryParser;
pub struct BoundsParser;
pub struct GeneralParser;
pub struct IntegerParser;
pub struct SemiParser;

impl_section_parser!(BinaryParser, Vec<&'a str>, &["binaries", "binary", "bin"], parse_variable_list);
impl_section_parser!(BoundsParser, Vec<(&'a str, VariableType)>, &["bounds", "bound"], |input| many0(parse_single_bound)(input));
impl_section_parser!(GeneralParser, Vec<&'a str>, &["generals", "general", "gen"], parse_variable_list);
impl_section_parser!(IntegerParser, Vec<&'a str>, &["integers", "integer"], parse_variable_list);
impl_section_parser!(SemiParser, Vec<&'a str>, &["semi-continuous", "semis", "semi"], parse_variable_list);
