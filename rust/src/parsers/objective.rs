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
                name: name.map_or_else(
                    || {
                        let next = generals.next_id();
                        Cow::Owned(format!("OBJECTIVE_{next}"))
                    },
                    Cow::Borrowed,
                ),
                coefficients,
            }
        },
    );

    let (remaining, objectives) = many1(parser).parse(input)?;

    log_unparsed_content("Failed to parse objectives fully", remaining);
    Ok(("", (objectives.into_iter().map(|ob| (ob.name.clone(), ob)).collect(), objective_vars)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_single_objective_unnamed() {
        let input = "x1 + 2x2 - 3x3";
        let result = parse_objectives(input).unwrap();
        assert_eq!(result.0, "");

        let (objectives, vars) = result.1;
        assert_eq!(objectives.len(), 1);
        assert_eq!(vars.len(), 3);
        assert!(vars.contains_key("x1"));
        assert!(vars.contains_key("x2"));
        assert!(vars.contains_key("x3"));

        let objective = objectives.values().next().unwrap();
        assert!(objective.name.starts_with("OBJECTIVE_"));
        assert_eq!(objective.coefficients.len(), 3);
    }

    #[test]
    fn test_single_objective_named() {
        let input = "profit: 5x1 + 3x2";
        let result = parse_objectives(input).unwrap();
        assert_eq!(result.0, "");

        let (objectives, vars) = result.1;
        assert_eq!(objectives.len(), 1);
        assert_eq!(vars.len(), 2);

        let objective = objectives.values().next().unwrap();
        assert_eq!(objective.name, "profit");
        assert_eq!(objective.coefficients.len(), 2);
    }

    #[test]
    fn test_multiple_objectives() {
        let input = "obj1: x1 + x2\nobj2: 2x1 - x3";

        let result = parse_objectives(input).unwrap();
        let (objectives, vars) = result.1;
        assert_eq!(objectives.len(), 2);
        assert_eq!(vars.len(), 3);

        let names: Vec<_> = objectives.values().map(|obj| obj.name.as_ref()).collect();
        assert!(names.contains(&"obj1"));
        assert!(names.contains(&"obj2"));
    }

    #[test]
    fn test_objective_with_coefficients() {
        let input = "maximize: 2.5x1 - 1.5x2 + 0.75x3";
        let result = parse_objectives(input).unwrap();

        let (objectives, vars) = result.1;
        assert_eq!(objectives.len(), 1);
        assert_eq!(vars.len(), 3);

        let objective = objectives.values().next().unwrap();
        assert_eq!(objective.name, "maximize");
        assert_eq!(objective.coefficients.len(), 3);

        // Check coefficient values
        assert_eq!(objective.coefficients[0].value, 2.5);
        assert_eq!(objective.coefficients[0].name, "x1");
        assert_eq!(objective.coefficients[1].value, -1.5);
        assert_eq!(objective.coefficients[1].name, "x2");
        assert_eq!(objective.coefficients[2].value, 0.75);
        assert_eq!(objective.coefficients[2].name, "x3");
    }

    #[test]
    fn test_objective_with_scientific_notation() {
        let input = "cost: 1e3x1 + 2.5e-2x2 - 1.23e+5x3";
        let result = parse_objectives(input).unwrap();

        let (objectives, _) = result.1;
        assert_eq!(objectives.len(), 1);

        let objective = objectives.values().next().unwrap();
        assert_eq!(objective.coefficients[0].value, 1000.0);
        assert_eq!(objective.coefficients[1].value, 0.025);
        assert_eq!(objective.coefficients[2].value, -123000.0);
    }

    #[test]
    fn test_objective_with_infinity() {
        let input = "obj: inf x1 - infinity x2";
        let result = parse_objectives(input).unwrap();

        let (objectives, _) = result.1;
        assert_eq!(objectives.len(), 1);

        let objective = objectives.values().next().unwrap();
        assert_eq!(objective.coefficients[0].value, f64::INFINITY);
        assert_eq!(objective.coefficients[1].value, f64::NEG_INFINITY);
    }

    #[test]
    fn test_objective_multiline_continuation() {
        let input = "
profit:
    2x1 + 3x2
    + 4x3 - 1.5x4
    + 0.5x5";

        let result = parse_objectives(input).unwrap();
        let (objectives, vars) = result.1;
        assert_eq!(objectives.len(), 1);
        assert_eq!(vars.len(), 5);

        let objective = objectives.values().next().unwrap();
        assert_eq!(objective.name, "profit");
        assert_eq!(objective.coefficients.len(), 5);

        // Check all coefficients are parsed correctly
        assert_eq!(objective.coefficients[0].value, 2.0);
        assert_eq!(objective.coefficients[1].value, 3.0);
        assert_eq!(objective.coefficients[2].value, 4.0);
        assert_eq!(objective.coefficients[3].value, -1.5);
        assert_eq!(objective.coefficients[4].value, 0.5);
    }

    #[test]
    fn test_objective_with_whitespace() {
        let input = "
   revenue:
       5  x1   +   3.5  x2
       -   2.1  x3  +   x4
";
        let result = parse_objectives(input).unwrap();

        let (objectives, vars) = result.1;
        assert_eq!(objectives.len(), 1);
        assert_eq!(vars.len(), 4);

        let objective = objectives.values().next().unwrap();
        assert_eq!(objective.name, "revenue");
        assert_eq!(objective.coefficients.len(), 4);
    }

    #[test]
    fn test_objective_with_complex_variable_names() {
        let input = "objective: complex_var_1 + Variable_Name_123 + x.1.2";
        let result = parse_objectives(input).unwrap();

        let (objectives, vars) = result.1;
        assert_eq!(objectives.len(), 1);
        assert_eq!(vars.len(), 3);
        assert!(vars.contains_key("complex_var_1"));
        assert!(vars.contains_key("Variable_Name_123"));
        assert!(vars.contains_key("x.1.2"));
    }

    #[test]
    fn test_objective_zero_coefficients() {
        let input = "obj: 0x1 + x2 - 0x3";
        let result = parse_objectives(input).unwrap();

        let (objectives, vars) = result.1;
        assert_eq!(objectives.len(), 1);
        assert_eq!(vars.len(), 3);

        let objective = objectives.values().next().unwrap();
        assert_eq!(objective.coefficients[0].value, 0.0);
        assert_eq!(objective.coefficients[1].value, 1.0);
        assert_eq!(objective.coefficients[2].value, 0.0);
    }

    #[test]
    fn test_objective_single_variable() {
        let input = "minimize: x1";
        let result = parse_objectives(input).unwrap();

        let (objectives, vars) = result.1;
        assert_eq!(objectives.len(), 1);
        assert_eq!(vars.len(), 1);

        let objective = objectives.values().next().unwrap();
        assert_eq!(objective.coefficients.len(), 1);
        assert_eq!(objective.coefficients[0].value, 1.0);
        assert_eq!(objective.coefficients[0].name, "x1");
    }

    #[test]
    fn test_objective_fractional_coefficients() {
        let input = "obj: 0.5x1 + 1.25x2 - 0.333x3";
        let result = parse_objectives(input).unwrap();

        let (objectives, _) = result.1;
        assert_eq!(objectives.len(), 1);

        let objective = objectives.values().next().unwrap();
        assert_eq!(objective.coefficients[0].value, 0.5);
        assert_eq!(objective.coefficients[1].value, 1.25);
        assert!((objective.coefficients[2].value - (-0.333)).abs() < 1e-10);
    }

    #[test]
    fn test_objective_name_generation() {
        let input = "x1 + x2";

        let result = parse_objectives(input).unwrap();
        let (objectives, _) = result.1;
        assert_eq!(objectives.len(), 1);

        // Should have auto-generated name
        let objective = objectives.values().next().unwrap();
        assert!(objective.name.starts_with("OBJECTIVE_"));
    }

    #[test]
    fn test_objective_with_special_character_names() {
        let input = "obj_1.2.3: x1 + x2";
        let result = parse_objectives(input).unwrap();

        let (objectives, _) = result.1;
        assert_eq!(objectives.len(), 1);

        let objective = objectives.values().next().unwrap();
        assert_eq!(objective.name, "obj_1.2.3");
    }

    #[test]
    fn test_objective_edge_cases() {
        // Very long variable names
        let input = "very_long_variable_name_that_goes_on_and_on_x1";
        let result = parse_objectives(input);
        assert!(result.is_ok());

        // Many variables in one objective
        let input = "x1 + x2 + x3 + x4 + x5 + x6 + x7 + x8 + x9 + x10";
        let result = parse_objectives(input).unwrap();
        let (objectives, vars) = result.1;
        assert_eq!(objectives.len(), 1);
        assert_eq!(vars.len(), 10);
    }

    #[test]
    fn test_objective_mixed_signs() {
        let input = "obj: +x1 - x2 + 3x3 - 2.5x4";
        let result = parse_objectives(input).unwrap();

        let (objectives, _) = result.1;
        assert_eq!(objectives.len(), 1);

        let objective = objectives.values().next().unwrap();
        assert_eq!(objective.coefficients[0].value, 1.0);
        assert_eq!(objective.coefficients[1].value, -1.0);
        assert_eq!(objective.coefficients[2].value, 3.0);
        assert_eq!(objective.coefficients[3].value, -2.5);
    }

    #[test]
    fn test_objective_continuation_line_detection() {
        let input = "
obj1: x1 + x2
    + x3
obj2: y1 + y2";

        let result = parse_objectives(input).unwrap();
        let (objectives, vars) = result.1;
        assert_eq!(objectives.len(), 2);
        assert_eq!(vars.len(), 5);

        // Check that continuation lines are properly associated
        let obj1 = objectives.get("obj1").unwrap();
        assert_eq!(obj1.coefficients.len(), 3);

        let obj2 = objectives.get("obj2").unwrap();
        assert_eq!(obj2.coefficients.len(), 2);
    }

    #[test]
    fn test_objective_boundary_values() {
        let input = "obj: 999999999x1 + 0.000001x2";
        let result = parse_objectives(input).unwrap();

        let (objectives, _) = result.1;
        assert_eq!(objectives.len(), 1);

        let objective = objectives.values().next().unwrap();
        assert_eq!(objective.coefficients[0].value, 999999999.0);
        assert_eq!(objective.coefficients[1].value, 0.000001);
    }

    #[test]
    fn test_invalid_objectives() {
        // Empty input
        let result = parse_objectives("");
        assert!(result.is_err());

        // Only whitespace
        let result = parse_objectives("   ");
        assert!(result.is_err());

        // Invalid coefficient format (no variable)
        let result = parse_objectives("obj: 2.5");
        assert!(result.is_err());
    }

    #[test]
    fn test_objective_with_negative_coefficients() {
        let input = "cost: -5x1 - 3x2 - x3";
        let result = parse_objectives(input).unwrap();

        let (objectives, _) = result.1;
        assert_eq!(objectives.len(), 1);

        let objective = objectives.values().next().unwrap();
        assert_eq!(objective.coefficients[0].value, -5.0);
        assert_eq!(objective.coefficients[1].value, -3.0);
        assert_eq!(objective.coefficients[2].value, -1.0);
    }

    #[test]
    fn test_objective_with_long_continuation() {
        let input = "
maximize:
    x1 + x2 + x3
    + x4 + x5 + x6
    + x7 + x8 + x9
    + x10";

        let result = parse_objectives(input).unwrap();
        let (objectives, vars) = result.1;
        assert_eq!(objectives.len(), 1);
        assert_eq!(vars.len(), 10);

        let objective = objectives.values().next().unwrap();
        assert_eq!(objective.coefficients.len(), 10);

        // All coefficients should be 1.0
        for coeff in &objective.coefficients {
            assert_eq!(coeff.value, 1.0);
        }
    }

    #[test]
    fn test_objective_section() {
        let input = " obj1: -0.5 x - 2y - 8z\n obj2: y + x + z\n obj3: 10z - 2.5x\n       + y";

        let (input, (objs, vars)) = parse_objectives(input).unwrap();

        assert_eq!(input, "");
        assert_eq!(objs.len(), 3);
        assert_eq!(vars.len(), 3);
    }
}
