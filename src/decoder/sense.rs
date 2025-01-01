use nom::{branch::alt, bytes::complete::tag_no_case, character::complete::multispace0, combinator::value, sequence::delimited, IResult};

use crate::model::Sense;

#[inline]
/// Parses the input string to determine the optimization sense.
///
/// This function attempts to match the input string against known
/// optimization sense keywords, such as "minimize", "maximum", etc.,
/// and returns the corresponding `Sense` variant. It ignores leading
/// and trailing whitespace and is case-insensitive.
///
/// # Arguments
///
/// * `input` - A string slice that holds the input to be parsed.
///
/// # Returns
///
/// * `IResult<&str, Sense>` - A result containing the remaining input
///   and the parsed `Sense` variant if successful, or an error if parsing fails.
///
pub fn parse_sense(input: &str) -> IResult<&str, Sense> {
    delimited(
        multispace0,
        alt((
            value(Sense::Minimize, alt((tag_no_case("minimize"), tag_no_case("minimum"), tag_no_case("min")))),
            value(Sense::Maximize, alt((tag_no_case("maximize"), tag_no_case("maximum"), tag_no_case("max")))),
        )),
        multispace0,
    )(input)
}

#[cfg(test)]
mod test {
    use crate::decoder::sense::parse_sense;

    #[test]
    fn test_parse_sense() {
        let valid = ["Minimize", "minimize", "min", "minimum", "Maximize", "maximize", "Max", "maximum"];
        for input in valid {
            assert!(parse_sense(input).is_ok());
        }
    }
}
