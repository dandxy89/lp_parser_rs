use nom::{
    branch::alt,
    bytes::complete::{tag, take_until},
    character::complete::{line_ending, multispace0, not_line_ending},
    combinator::recognize,
    multi::many0,
    sequence::tuple,
    IResult,
};

#[inline]
/// Parses a single comment from the input string.
///
/// This function recognizes three types of comment starts: `\\*`, `\*`, and `\`.
/// For block comments starting with `\\*` or `\*`, it captures content until `*\` is found.
/// For line comments starting with `\`, it captures content until the end of the line.
///
/// # Arguments
///
/// * `input` - A string slice that holds the input to be parsed.
///
/// # Returns
///
/// An `IResult` containing the remaining input and the parsed comment content.
///
fn parse_single_comment(input: &str) -> IResult<&str, &str> {
    let (input, comment_start) = alt((tag("\\\\*"), tag("\\*"), tag("\\")))(input)?;
    let (input, content) = match comment_start {
        "\\\\*" | "\\*" => {
            let (i, content) = recognize(take_until("*\\"))(input)?;
            let (i, _) = tuple((tag("*\\"), multispace0))(i)?;
            (i, content)
        }
        "\\" => {
            let (i, content) = recognize(not_line_ending)(input)?;
            let (i, _) = line_ending(i)?;
            (i, content)
        }
        _ => unreachable!(),
    };
    Ok((input, content))
}

#[inline]
/// Parses the input string to extract the last comment, if any, from a sequence of comments.
///
/// # Arguments
///
/// * `input` - A string slice that holds the input data to be parsed.
///
/// # Returns
///
/// * `IResult<&str, Option<&str>>` - A result containing the remaining unparsed input and an
///   optional string slice of the last comment found. If no comments are found, returns `None`.
///
pub fn parse_problem_name(input: &str) -> IResult<&str, Option<&str>> {
    let (remaining, comments) = many0(parse_single_comment)(input)?;
    let last_comment = comments.last().copied();
    Ok((remaining, last_comment))
}

#[cfg(test)]
mod test {
    use crate::decoder::problem_name::parse_problem_name;

    #[test]
    fn test_parse_lp_file_comments() {
        let valid = [
            "\\* First comment *\\\n\\ENCODING=ISO-8859-1\n\\* Middle comment *\\\\Problem name: ilog.cplex\n\\* Last comment *\\",
            "\\Problem name: kb2.mps\n",
            "\\ File: lo1.lp\n",
            "\\* WBM_Problem *\\\n",
        ];
        for input in valid {
            let (remainder, x) = parse_problem_name(input).unwrap();
            assert_eq!("", remainder);
            assert!(x.is_some());
        }
    }
}
