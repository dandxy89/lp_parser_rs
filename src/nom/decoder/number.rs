use nom::{
    branch::alt,
    bytes::complete::tag_no_case,
    character::complete::{char, digit1, one_of},
    combinator::{all_consuming, complete, map, opt, recognize},
    error::ErrorKind,
    sequence::{pair, tuple},
    Err, IResult,
};

#[inline]
fn infinity(input: &str) -> IResult<&str, f64> {
    all_consuming(map(tuple((opt(one_of("+-")), alt((tag_no_case("infinity"), tag_no_case("inf"))))), |(sign, _)| match sign {
        Some('-') => f64::NEG_INFINITY,
        _ => f64::INFINITY,
    }))(input)
}

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
    fn test_infinity() {
        assert_eq!(infinity("infinity").unwrap().1, f64::INFINITY);
        assert_eq!(infinity("INFINITY").unwrap().1, f64::INFINITY);
        assert_eq!(infinity("Infinity").unwrap().1, f64::INFINITY);
        assert_eq!(infinity("inf").unwrap().1, f64::INFINITY);
        assert_eq!(infinity("INF").unwrap().1, f64::INFINITY);
        assert_eq!(infinity("Inf").unwrap().1, f64::INFINITY);
        assert_eq!(infinity("+infinity").unwrap().1, f64::INFINITY);
        assert_eq!(infinity("+inf").unwrap().1, f64::INFINITY);

        assert_eq!(infinity("-infinity").unwrap().1, f64::NEG_INFINITY);
        assert_eq!(infinity("-INFINITY").unwrap().1, f64::NEG_INFINITY);
        assert_eq!(infinity("-Infinity").unwrap().1, f64::NEG_INFINITY);
        assert_eq!(infinity("-inf").unwrap().1, f64::NEG_INFINITY);
        assert_eq!(infinity("-INF").unwrap().1, f64::NEG_INFINITY);
        assert_eq!(infinity("-Inf").unwrap().1, f64::NEG_INFINITY);

        assert!(infinity("notinfinity").is_err());
        assert!(dbg!(infinity("infx")).is_err());
        assert!(infinity("infinit").is_err());
        assert!(infinity("in").is_err());
        assert!(infinity("++inf").is_err());
        assert!(infinity("--inf").is_err());
    }

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
