use nom::{
    branch::alt,
    bytes::complete::{tag, tag_no_case},
    character::complete::{char, digit1, multispace0, one_of},
    combinator::{all_consuming, complete, map, opt, recognize, value},
    error::ErrorKind,
    sequence::{pair, preceded, tuple},
    Err, IResult,
};

use crate::nom::model::ComparisonOp;

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

#[inline]
pub fn parse_num_value(input: &str) -> IResult<&str, f64> {
    preceded(multispace0, alt((infinity, map(number, |v| v.parse::<f64>().unwrap_or_default()))))(input)
}

#[inline]
pub fn parse_cmp_op(input: &str) -> IResult<&str, ComparisonOp> {
    preceded(
        multispace0,
        alt((
            value(ComparisonOp::LTE, tag("<=")),
            value(ComparisonOp::GTE, tag(">=")),
            value(ComparisonOp::EQ, tag("=")),
            value(ComparisonOp::LT, tag("<")),
            value(ComparisonOp::GT, tag(">")),
        )),
    )(input)
}

#[cfg(test)]
mod tests {
    use crate::nom::decoder::number::{infinity, number, parse_num_value};

    #[test]
    fn test_number_value() {
        assert!(parse_num_value("inf").is_ok());
        assert!(parse_num_value("123.1").is_ok());
        assert!(parse_num_value("13e12").is_ok());
        assert!(parse_num_value("13.12e14").is_ok());
    }

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
        assert!(infinity("infx").is_err());
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
        for input in valid_numbers {
            assert!(number(input).is_ok());
        }

        assert!(number("abc").is_err());
        assert!(number(".123").is_err());
        assert!(number("1.23e").is_err());
    }
}
