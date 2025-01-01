//! Parser for objective functions in LP files.
//!
//! This module handles the parsing of objective function definitions, including:
//! - Single and multiple objective functions
//! - Named and unnamed objectives
//! - Coefficient and variable parsing
//! - Multi-line objective definitions
//!

use std::{
    borrow::Cow,
    collections::{hash_map::Entry, HashMap},
};

use nom::{
    character::complete::{char, multispace0, multispace1, space0},
    combinator::{map, not, opt, peek},
    multi::{many0, many1},
    sequence::{delimited, preceded, terminated, tuple},
    IResult,
};
use unique_id::{sequence::SequenceGenerator, Generator as _};

use crate::{
    decoder::{coefficient::parse_coefficient, parser_traits::parse_variable},
    log_remaining,
    model::{Coefficient, Objective, Variable},
};

#[inline]
/// Checks if a string starts with a new objective function definition.
fn is_new_objective(input: &str) -> IResult<&str, ()> {
    map(tuple((multispace0, parse_variable, multispace0, char(':'))), |_| ())(input)
}

#[inline]
/// Parses continuation lines of an objective function.
fn objective_continuations(input: &str) -> IResult<&str, Vec<Coefficient<'_>>> {
    preceded(tuple((multispace1, not(peek(is_new_objective)))), many1(preceded(space0, parse_coefficient)))(input)
}

/// Type alias for the parsed result of objectives.
type ParsedObjectives<'a> = IResult<&'a str, (HashMap<Cow<'a, str>, Objective<'a>>, HashMap<&'a str, Variable<'a>>)>;

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
pub fn parse_objectives(input: &str) -> ParsedObjectives<'_> {
    let mut objective_vars = HashMap::default();
    let gen = SequenceGenerator;

    // Inline function to extra Objective functions
    let parser = map(
        tuple((
            // Name part (optional)
            opt(terminated(preceded(multispace0, parse_variable), delimited(multispace0, char(':'), multispace0))),
            // Initial coefficients
            many1(preceded(space0, parse_coefficient)),
            // Continuation lines
            many0(objective_continuations),
        )),
        |(name, coefficients, continuation_coefficients)| {
            let coefficients = coefficients
                .into_iter()
                .chain(continuation_coefficients.into_iter().flatten())
                .inspect(|coeff| {
                    if let Entry::Vacant(vacant_entry) = objective_vars.entry(coeff.var_name) {
                        vacant_entry.insert(Variable::new(coeff.var_name));
                    }
                })
                .collect();

            Objective {
                name: if let Some(s) = name {
                    Cow::Borrowed(s)
                } else {
                    let next = gen.next_id();
                    Cow::Owned(format!("OBJECTIVE_{next}"))
                },
                coefficients,
            }
        },
    );

    let (remaining, objectives) = many1(parser)(input)?;

    log_remaining("Failed to parse objectives fully", remaining);
    Ok(("", (objectives.into_iter().map(|ob| (ob.name.clone(), ob)).collect(), objective_vars)))
}

#[cfg(test)]
mod test {
    use crate::decoder::objective::parse_objectives;

    #[test]
    fn test_objective_section() {
        let input = " obj1: -0.5 x - 2y - 8z\n obj2: y + x + z\n obj3: 10z - 2.5x\n       + y";

        let (input, (objs, vars)) = parse_objectives(input).unwrap();

        assert_eq!(input, "");
        assert_eq!(objs.len(), 3);
        assert_eq!(vars.len(), 3);
    }
}
