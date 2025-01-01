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
fn is_new_objective(input: &str) -> IResult<&str, ()> {
    map(tuple((multispace0, parse_variable, multispace0, char(':'))), |_| ())(input)
}

#[inline]
fn objective_continuations(input: &str) -> IResult<&str, Vec<Coefficient<'_>>> {
    preceded(tuple((multispace1, not(peek(is_new_objective)))), many1(preceded(space0, parse_coefficient)))(input)
}

type ParsedObjectives<'a> = IResult<&'a str, (HashMap<Cow<'a, str>, Objective<'a>>, HashMap<&'a str, Variable<'a>>)>;

#[inline]
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
                name: match name {
                    Some(s) => Cow::Borrowed(s),
                    None => {
                        let next = gen.next_id();
                        Cow::Owned(format!("OBJECTIVE_{next}"))
                    }
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
