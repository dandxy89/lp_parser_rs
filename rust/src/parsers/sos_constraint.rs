//! Parser for Special Ordered Set (SOS) constraints in LP files.
//!
//! This module provides functionality for parsing SOS constraints, which are special
//! constraints that define relationships between sets of variables. The module supports
//! both Type 1 (SOS1) and Type 2 (SOS2) constraints with associated weights.
//!

use std::borrow::Cow;
use std::collections::HashMap;
use std::collections::hash_map::Entry;

use nom::branch::alt;
use nom::bytes::complete::tag_no_case;
use nom::character::complete::{char, multispace0, multispace1};
use nom::combinator::{map, opt};
use nom::multi::many1;
use nom::sequence::{delimited, preceded, terminated};
use nom::{IResult, Parser as _};

use crate::log_unparsed_content;
use crate::model::{Coefficient, Constraint, SOSType, Variable, VariableType};
use crate::parsers::number::parse_num_value;
use crate::parsers::parser_traits::parse_variable;

#[inline]
/// Parses the SOS constraint type (S1 or S2).
fn parse_sos_type(input: &str) -> IResult<&str, SOSType> {
    alt((map(tag_no_case("S1"), |_| SOSType::S1), map(tag_no_case("S2"), |_| SOSType::S2))).parse(input)
}

#[inline]
/// Parses a variable-weight pair for an SOS constraint.
fn parse_sos_weight(input: &str) -> IResult<&str, Coefficient<'_>> {
    map((preceded(multispace0, parse_variable), preceded(char(':'), parse_num_value)), |(var_name, value)| Coefficient {
        name: var_name,
        value,
    })
    .parse(input)
}

/// Type alias for the parsed result of SOS constraints.
type ParsedConstraints<'a> = IResult<&'a str, (HashMap<Cow<'a, str>, Constraint<'a>>, HashMap<&'a str, Variable<'a>>)>;

#[inline]
/// Parses a section of SOS constraints from the given input string.
///
/// This function processes a string input to extract SOS constraints, which
/// include a name, SOS type, and associated weights. It constructs a map of
/// variables and returns a collection of parsed constraints along with any
/// remaining unparsed input. The function logs a message if parsing is not
/// fully successful.
///
/// # Arguments
///
/// * `input` - A string slice containing the SOS constraints to be parsed.
///
/// # Returns
///
/// A result containing a tuple with the parsed constraints and a map of
/// variables, or an error if parsing fails.
///
pub fn parse_sos_section<'a>(input: &'a str) -> ParsedConstraints<'a> {
    let mut constraint_vars: HashMap<&'a str, Variable<'a>> = HashMap::default();

    let parser = map(
        (
            // Name part with optional whitespace
            terminated(preceded(multispace0, parse_variable), delimited(multispace0, char(':'), multispace0)),
            // SOS type (S1 or S2)
            terminated(parse_sos_type, delimited(multispace0, tag_no_case("::"), multispace0)),
            // Weights with flexible whitespace
            many1(preceded(multispace0, parse_sos_weight)),
        ),
        |(name, sos_type, weights)| {
            for coeff in &weights {
                if let Entry::Vacant(vacant_entry) = constraint_vars.entry(coeff.name) {
                    vacant_entry.insert(Variable::new(coeff.name).with_var_type(VariableType::SOS));
                }
            }

            Constraint::SOS { name: Cow::Borrowed(name), sos_type, weights }
        },
    );

    let (remaining, constraints) = preceded((multispace0, tag_no_case("SOS"), opt(char(':')), multispace1), many1(parser)).parse(input)?;
    let constraints = constraints.into_iter().map(|c| (Cow::Owned(c.name().to_string()), c)).collect();

    log_unparsed_content("Failed to parse sos constraints fully", remaining);
    Ok(("", (constraints, constraint_vars)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_sos_section() {
        let input = "SOS\ncsos1: S1:: V1:1 V3:2 V5:3\ncsos2: S2:: V2:2 V4:1 V5:2.5";
        let result = parse_sos_section(input);
        assert!(result.is_ok());

        if let Ok((_, (constraints, variables))) = result {
            assert_eq!(constraints.len(), 2);
            assert_eq!(variables.len(), 5);
        }
    }
}
