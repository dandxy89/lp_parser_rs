//! Parser for LP problem constraints.
//!
//! This module handles parsing of constraint definitions,
//! including their names, coefficients, operators, and
//! right-hand side values.
//!

use std::{
    borrow::Cow,
    collections::{hash_map::Entry, HashMap},
};

use nom::{
    branch::alt,
    bytes::complete::{tag, tag_no_case},
    character::complete::{char, multispace0},
    combinator::{map, opt, value},
    multi::many1,
    sequence::{delimited, preceded, terminated, tuple},
    IResult,
};
use unique_id::{sequence::SequenceGenerator, Generator as _};

use crate::{
    log_unparsed_content,
    model::{Constraint, Variable},
    parsers::{
        coefficient::parse_coefficient,
        number::{parse_cmp_op, parse_num_value},
        parser_traits::parse_variable,
    },
};

#[inline]
/// Parses a constraint section header from an LP format input string.
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

#[inline]
fn parse_comment_marker(input: &str) -> IResult<&str, ()> {
    value((), preceded(multispace0, tag("\\")))(input)
}

type ConstraintParseResult<'a> = IResult<&'a str, (HashMap<Cow<'a, str>, Constraint<'a>>, HashMap<&'a str, Variable<'a>>)>;

#[inline]
/// Parses a string input to extract constraints and associated variables.
///
/// This function processes the input string to identify and parse constraints,
/// which may include optional comment markers, variable names, coefficients,
/// comparison operators, and right-hand side values. It returns a result
/// containing a tuple of parsed constraints and variables, or an error if
/// parsing fails. The function also logs any remaining unparsed input.
///
/// # Arguments
///
/// * `input` - A string slice representing the input to be parsed.
///
/// # Returns
///
/// * `ParsedConstraints<'a>` - A result containing a tuple of a hashmap of
///   constraints and a hashmap of variables, or an error if parsing fails.
///
pub fn parse_constraints<'a>(input: &'a str) -> ConstraintParseResult<'a> {
    let mut constraint_vars: HashMap<&'a str, Variable<'a>> = HashMap::with_capacity(512);
    let gen = SequenceGenerator;

    let parser = map(
        tuple((
            // Optional comment marker
            opt(parse_comment_marker),
            // Name part with optional whitespace and newlines
            opt(terminated(preceded(multispace0, parse_variable), delimited(multispace0, opt(char(':')), multispace0))),
            // Coefficients with flexible whitespace and newlines
            many1(preceded(multispace0, parse_coefficient)),
            // Operator and RHS with flexible whitespace
            preceded(multispace0, parse_cmp_op),
            preceded(multispace0, parse_num_value),
        )),
        |(is_comment, name, coefficients, operator, rhs)| {
            is_comment.is_none().then(|| {
                for coeff in &coefficients {
                    if let Entry::Vacant(vacant_entry) = constraint_vars.entry(coeff.name) {
                        vacant_entry.insert(Variable::new(coeff.name));
                    }
                }

                // Standard (SOS constraints are handled separately)
                Constraint::Standard {
                    name: if let Some(s) = name {
                        Cow::Borrowed(s)
                    } else {
                        let next = gen.next_id();
                        Cow::Owned(format!("CONSTRAINT_{next}"))
                    },
                    coefficients,
                    operator,
                    rhs,
                }
            })
        },
    );

    let (remaining, constraints) = many1(parser)(input)?;
    let cons = constraints.into_iter().flatten().map(|c| (Cow::Owned(c.name().to_string()), c)).collect();

    log_unparsed_content("Failed to parse constraints fully", remaining);
    Ok(("", (cons, constraint_vars)))
}
