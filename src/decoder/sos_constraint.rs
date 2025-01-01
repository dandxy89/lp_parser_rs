use std::{
    borrow::Cow,
    collections::{hash_map::Entry, HashMap},
};

use nom::{
    branch::alt,
    bytes::complete::tag_no_case,
    character::complete::{char, multispace0, multispace1},
    combinator::{map, opt},
    multi::many1,
    sequence::{delimited, preceded, terminated, tuple},
    IResult,
};

use crate::{
    decoder::{number::parse_num_value, parser_traits::parse_variable},
    log_remaining,
    model::{Coefficient, Constraint, SOSType, Variable, VariableType},
};

#[inline]
fn parse_sos_type(input: &str) -> IResult<&str, SOSType> {
    alt((map(tag_no_case("S1"), |_| SOSType::S1), map(tag_no_case("S2"), |_| SOSType::S2)))(input)
}

#[inline]
fn parse_sos_weight(input: &str) -> IResult<&str, Coefficient> {
    map(tuple((preceded(multispace0, parse_variable), preceded(char(':'), parse_num_value))), |(var_name, coefficient)| Coefficient {
        var_name,
        coefficient,
    })(input)
}

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
        tuple((
            // Name part with optional whitespace
            terminated(preceded(multispace0, parse_variable), delimited(multispace0, char(':'), multispace0)),
            // SOS type (S1 or S2)
            terminated(parse_sos_type, delimited(multispace0, tag_no_case("::"), multispace0)),
            // Weights with flexible whitespace
            many1(preceded(multispace0, parse_sos_weight)),
        )),
        |(name, sos_type, weights)| {
            for coeff in &weights {
                if let Entry::Vacant(vacant_entry) = constraint_vars.entry(coeff.var_name) {
                    vacant_entry.insert(Variable::new(coeff.var_name).with_var_type(VariableType::SOS));
                }
            }

            Constraint::SOS { name: Cow::Borrowed(name), sos_type, weights }
        },
    );

    let (remaining, constraints) = preceded(tuple((multispace0, tag_no_case("SOS"), opt(char(':')), multispace1)), many1(parser))(input)?;
    let cons = constraints.into_iter().map(|c| (Cow::Owned(c.name().to_string()), c)).collect();

    log_remaining("Failed to parse sos constraints fully", remaining);
    Ok(("", (cons, constraint_vars)))
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
