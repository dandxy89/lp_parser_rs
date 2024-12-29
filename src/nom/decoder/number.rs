use nom::{
    branch::alt,
    character::complete::{char, digit1, one_of},
    combinator::{complete, opt, recognize},
    error::ErrorKind,
    sequence::{pair, tuple},
    Err, IResult,
};

#[inline]
fn number(input: &str) -> IResult<&str, &str> {
    let (remainder, matched) = recognize(tuple((
        // Optional sign at the start
        opt(one_of("+-")),
        // Integer part (required)
        digit1,
        // Optional decimal part
        opt(pair(char('.'), opt(digit1))),
        // Optional scientific notation part
        opt(complete(tuple((alt((char('e'), char('E'))), opt(one_of("+-")), digit1)))),
    )))(input)?;

    if remainder.starts_with('e') || remainder.starts_with('E') {
        Err(Err::Error(nom::error::Error::new(input, ErrorKind::Verify)))
    } else {
        Ok((remainder, matched))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_number_parser() {
        let valid_numbers = [
            "123", "+123", "-123", "123.456", "-123.456", "+123.456", "123.", "1.23e4", "1.23E4", "1.23e+4", "1.23e-4", "-1.23e-4",
            "+1.23e+4",
        ];
        for valid_number in valid_numbers {
            assert!(number(valid_number).is_ok());
        }

        assert!(number("abc").is_err());
        assert!(number(".123").is_err());
        assert!(dbg!(number("1.23e")).is_err());
    }
}
