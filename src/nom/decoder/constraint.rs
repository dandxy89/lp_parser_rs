use std::{
    borrow::Cow,
    collections::{hash_map::Entry, HashMap},
};

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
    log_remaining,
    model::{Constraint, Variable},
};

#[inline]
pub fn parse_constraint_header(input: &str) -> IResult<&str, ()> {
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

type ParsedConstraints<'a> = IResult<&'a str, (HashMap<Cow<'a, str>, Constraint<'a>>, HashMap<&'a str, Variable<'a>>)>;

#[inline]
pub fn parse_constraints<'a>(input: &'a str) -> ParsedConstraints<'a> {
    let mut constraint_vars: HashMap<&'a str, Variable<'a>> = HashMap::default();

    let parser = map(
        tuple((
            // Name part with optional whitespace and newlines
            opt(terminated(preceded(multispace0, parse_variable), delimited(multispace0, opt(char(':')), multispace0))),
            // Coefficients with flexible whitespace and newlines
            many1(preceded(multispace0, parse_coefficient)),
            // Operator and RHS with flexible whitespace
            preceded(multispace0, parse_cmp_op),
            preceded(multispace0, parse_num_value),
        )),
        |(name, coefficients, operator, rhs)| {
            for coeff in &coefficients {
                if let Entry::Vacant(vacant_entry) = constraint_vars.entry(coeff.var_name) {
                    vacant_entry.insert(Variable::new(coeff.var_name));
                }
            }

            // Standard (SOS constraints are handled separately)
            Constraint::Standard { name: Cow::Borrowed(name.unwrap_or_default()), coefficients, operator, rhs }
        },
    );

    let (remaining, constraints) = many1(parser)(input)?;
    let cons = constraints.into_iter().map(|c| (Cow::Owned(c.name().to_string()), c)).collect();

    log_remaining("Failed to parse constraints fully", remaining);
    Ok(("", (cons, constraint_vars)))
}
