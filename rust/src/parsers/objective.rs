//! Parser for objective functions in LP files.
//!
//! This module handles the parsing of objective function definitions, including:
//! - Single and multiple objective functions
//! - Named and unnamed objectives
//! - Coefficient and variable parsing
//! - Multi-line objective definitions
//!

use std::borrow::Cow;
use std::collections::HashMap;
use std::collections::hash_map::Entry;

use nom::character::complete::{char, multispace0, multispace1, space0};
use nom::combinator::{map, not, opt, peek};
use nom::multi::{many0, many1};
use nom::sequence::{delimited, preceded, terminated};
use nom::{IResult, Parser as _};
use unique_id::Generator as _;
use unique_id::sequence::SequenceGenerator;

use crate::log_unparsed_content;
use crate::model::{Coefficient, Objective, Variable};
use crate::parsers::coefficient::parse_coefficient;
use crate::parsers::parser_traits::parse_variable;

#[inline]
/// Checks if a string starts with a new objective function definition.
fn is_new_objective(input: &str) -> IResult<&str, ()> {
    map((multispace0, parse_variable, multispace0, char(':')), |_| ()).parse(input)
}

#[inline]
/// Parses continuation lines of an objective function.
fn objective_continuations(input: &str) -> IResult<&str, Vec<Coefficient<'_>>> {
    preceded((multispace1, not(peek(is_new_objective))), many1(preceded(space0, parse_coefficient))).parse(input)
}

/// Type alias for the parsed result of objectives.
type ObjectiveParseResult<'a> = IResult<&'a str, (HashMap<Cow<'a, str>, Objective<'a>>, HashMap<&'a str, Variable<'a>>)>;

#[inline]
/// Parses a string input to extract and construct a collection of `Objective`
/// instances, each associated with a set of coefficients and optional names.
///
/// The function processes the input string to identify and parse variable names,
/// coefficients, and continuation lines, organizing them into `Objective`
/// structures. If a name is not provided for an objective, a unique identifier
/// is generated. The parsed objectives and their associated variables are
/// returned as a tuple.
///
/// # Arguments
///
/// * `input` - A string slice containing the objectives to be parsed.
///
/// # Returns
///
/// A result containing a tuple with a map of objective names to `Objective`
/// instances and a map of variable names to `Variable` instances, or an error
/// if parsing fails.
///
pub fn parse_objectives(input: &str) -> ObjectiveParseResult<'_> {
    let mut objective_vars = HashMap::with_capacity(2);
    let generals = SequenceGenerator;

    // Inline function to extra Objective functions
    let parser = map(
        (
            // Name part (optional)
            opt(terminated(preceded(multispace0, parse_variable), delimited(multispace0, char(':'), multispace0))),
            // Initial coefficients
            many1(preceded(space0, parse_coefficient)),
            // Continuation lines
            many0(objective_continuations),
        ),
        |(name, coefficients, continuation_coefficients)| {
            let coefficients = coefficients
                .into_iter()
                .chain(continuation_coefficients.into_iter().flatten())
                .inspect(|coeff| {
                    if let Entry::Vacant(vacant_entry) = objective_vars.entry(coeff.name) {
                        vacant_entry.insert(Variable::new(coeff.name));
                    }
                })
                .collect();

            Objective {
                name: if let Some(s) = name {
                    Cow::Borrowed(s)
                } else {
                    let next = generals.next_id();
                    Cow::Owned(format!("OBJECTIVE_{next}"))
                },
                coefficients,
            }
        },
    );

    let (remaining, objectives) = many1(parser).parse(input)?;

    log_unparsed_content("Failed to parse objectives fully", remaining);
    Ok(("", (objectives.into_iter().map(|ob| (ob.name.clone(), ob)).collect(), objective_vars)))
}

#[cfg(test)]
mod test {
    use crate::parsers::objective::parse_objectives;

    #[test]
    fn test_objective_section() {
        let input = " obj1: -0.5 x - 2y - 8z\n obj2: y + x + z\n obj3: 10z - 2.5x\n       + y";

        let (input, (objs, vars)) = parse_objectives(input).unwrap();

        assert_eq!(input, "");
        assert_eq!(objs.len(), 3);
        assert_eq!(vars.len(), 3);
    }
}
