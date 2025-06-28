//! Parser for LP problem constraints.
//!
//! This module handles parsing of constraint definitions,
//! including their names, coefficients, operators, and
//! right-hand side values.
//!

use std::borrow::Cow;
use std::collections::HashMap;
use std::collections::hash_map::Entry;

use nom::branch::alt;
use nom::bytes::complete::{tag, tag_no_case};
use nom::character::complete::{char, multispace0};
use nom::combinator::{map, opt, value};
use nom::multi::many1;
use nom::sequence::preceded;
use nom::{IResult, Parser as _};
use unique_id::Generator as _;
use unique_id::sequence::SequenceGenerator;

use crate::log_unparsed_content;
use crate::model::{Constraint, Variable};
use crate::parsers::coefficient::parse_coefficient;
use crate::parsers::number::{parse_cmp_op, parse_num_value};
use crate::parsers::parser_traits::parse_variable;

#[inline]
/// Parses a constraint section header from an LP format input string.
pub fn parse_constraint_header(input: &str) -> IResult<&str, ()> {
    value(
        (),
        (
            multispace0,
            alt((tag_no_case("subject to"), tag_no_case("such that"), tag_no_case("s.t."), tag_no_case("st"))),
            opt(char(':')),
            multispace0,
        ),
    )
    .parse(input)
}

#[inline]
fn parse_comment_marker(input: &str) -> IResult<&str, ()> {
    value((), preceded(multispace0, tag("\\"))).parse(input)
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
    let generator = SequenceGenerator;

    let parser = map(
        (
            // Optional comment marker
            opt(parse_comment_marker),
            // Name part with optional whitespace and newlines
            opt(map((preceded(multispace0, parse_variable), preceded(multispace0, char(':')), multispace0), |(name, _, _)| name)),
            // Coefficients with flexible whitespace and newlines
            many1(preceded(multispace0, parse_coefficient)),
            // Operator and RHS with flexible whitespace
            preceded(multispace0, parse_cmp_op),
            preceded(multispace0, parse_num_value),
        ),
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
                        let next = generator.next_id();
                        Cow::Owned(format!("CONSTRAINT_{next}"))
                    },
                    coefficients,
                    operator,
                    rhs,
                }
            })
        },
    );

    let (remaining, constraints) = many1(parser).parse(input)?;
    let cons = constraints.into_iter().flatten().map(|c| (Cow::Owned(c.name().to_string()), c)).collect();

    log_unparsed_content("Failed to parse constraints fully", remaining);
    Ok(("", (cons, constraint_vars)))
}

#[cfg(test)]
mod test {
    use crate::parsers::constraint::parse_constraints;

    #[test]
    fn test_constraint_with_colon_on_same_line() {
        let input = " Capacity_Constraint_1:\n    Production_A_1 + Production_B_2 <= 500\n";
        let result = parse_constraints(input);
        assert!(result.is_ok());
    }

    #[test]
    fn test_multiple_constraints_with_colons() {
        let input = " Capacity_Constraint_1:\n    Production_A_1 + Production_B_2 <= 500\n Demand_Region_X:\n    Transport_X_Y + Storage_Facility_1 >= 200\n";
        let result = parse_constraints(input);
        assert!(result.is_ok());
    }

    #[test]
    fn test_single_constraint_with_colon_newline() {
        let input = "Capacity_Constraint_1:\n    Production_A_1 + Production_B_2 <= 500";
        match parse_constraints(input) {
            Ok((_remaining, (constraints, _vars))) => {
                assert_eq!(constraints.len(), 1);
            }
            Err(_err) => {
                panic!("Failed to parse constraint");
            }
        }
    }

    #[test]
    fn test_constraint_no_name() {
        let input = "Production_A_1 + Production_B_2 <= 500";
        match parse_constraints(input) {
            Ok((_remaining, (constraints, _vars))) => {
                assert_eq!(constraints.len(), 1);
            }
            Err(_err) => {
                panic!("Failed to parse constraint");
            }
        }
    }
}
