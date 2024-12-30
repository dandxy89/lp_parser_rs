use nom::{
    branch::alt,
    bytes::complete::tag_no_case,
    character::complete::{char, multispace0},
    combinator::{map, opt, value},
    multi::many1,
    sequence::{delimited, preceded, terminated, tuple},
    IResult,
};

use crate::nom::{
    decoder::{
        coefficient::parse_coefficient,
        number::{parse_cmp_op, parse_num_value},
        variable::parse_variable,
    },
    model::Constraint,
};

#[inline]
pub fn parse_cons_header(input: &str) -> IResult<&str, ()> {
    value(
        (),
        tuple((
            multispace0,
            alt((tag_no_case("subject to"), tag_no_case("such that"), tag_no_case("s.t."), tag_no_case("st"))),
            opt(char(':')),
            multispace0,
        )),
    )(input)
}

#[inline]
fn parse_constraint(input: &str) -> IResult<&str, Constraint> {
    map(
        tuple((
            // Name part with optional whitespace and newlines
            opt(terminated(preceded(multispace0, parse_variable), delimited(multispace0, opt(char(':')), multispace0))),
            // Coefficients with flexible whitespace and newlines
            many1(preceded(
                multispace0, // This will handle spaces, tabs, and newlines
                parse_coefficient,
            )),
            // Operator and RHS with flexible whitespace
            preceded(multispace0, parse_cmp_op),
            preceded(multispace0, parse_num_value),
        )),
        |(name, coefficients, operator, rhs)| Constraint::Standard { name: name.map(|s| s.to_string()), coefficients, operator, rhs },
    )(input)
}

#[inline]
pub fn parse_cons(input: &str) -> IResult<&str, Vec<Constraint>> {
    many1(parse_constraint)(input)
}

#[cfg(test)]
mod test {
    use crate::nom::decoder::constraint::{parse_cons_header, parse_constraint};

    #[test]
    fn test_constraint_section_header() {
        let input = "subject to:";
        assert!(parse_cons_header(input).is_ok());
    }

    #[test]
    fn test_constraint() {
        let input = "c1:  3 x1 + x2 + 2 x3 = 30";
        let result = parse_constraint(input);
        assert!(result.is_ok());
    }
}
