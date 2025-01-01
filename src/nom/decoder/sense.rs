use nom::{branch::alt, bytes::complete::tag_no_case, character::complete::multispace0, combinator::value, sequence::delimited, IResult};

use crate::nom::model::Sense;

#[inline]
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
    use crate::nom::decoder::sense::parse_sense;

    #[test]
    fn test_parse_sense() {
        let valid = ["Minimize", "minimize", "min", "minimum", "Maximize", "maximize", "Max", "maximum"];
        for input in valid {
            assert!(parse_sense(input).is_ok());
        }
    }
}
