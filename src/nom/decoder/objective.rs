use std::collections::{hash_map::Entry, HashMap};

use nom::{
    character::complete::{char, multispace0, multispace1, space0},
    combinator::{map, not, peek},
    multi::{many0, many1},
    sequence::{delimited, preceded, terminated, tuple},
    IResult,
};

use crate::nom::{
    decoder::{coefficient::parse_coefficient, variable::parse_variable},
    model::{Coefficient, Objective, Variable},
};

#[inline]
fn is_new_objective(input: &str) -> IResult<&str, ()> {
    map(tuple((multispace0, parse_variable, multispace0, char(':'))), |_| ())(input)
}

#[inline]
fn line_continuation(input: &str) -> IResult<&str, Vec<Coefficient<'_>>> {
    preceded(tuple((multispace1, not(peek(is_new_objective)))), many1(preceded(space0, parse_coefficient)))(input)
}

#[inline]
pub fn parse_objectives(input: &str) -> IResult<&str, (HashMap<&str, Objective<'_>>, HashMap<&str, Variable<'_>>)> {
    let mut objective_vars = HashMap::default();

    // Inline function to extra Objective functions
    let parser = map(
        tuple((
            // Name part (required)
            terminated(preceded(multispace0, parse_variable), delimited(multispace0, char(':'), multispace0)),
            // Initial coefficients
            many1(preceded(space0, parse_coefficient)),
            // Continuation lines
            many0(line_continuation),
        )),
        |(name, first_coefficients, continuation_coefficients)| {
            // Capture all the Variable Names
            let mut coefficients = Vec::with_capacity(48);

            // Collate variables and coefficients
            for coeff in first_coefficients.into_iter().chain(continuation_coefficients.into_iter().flatten()) {
                match objective_vars.entry(coeff.var_name) {
                    Entry::Occupied(_) => (),
                    Entry::Vacant(vacant_entry) => {
                        vacant_entry.insert(Variable::new(coeff.var_name));
                    }
                }
                coefficients.push(coeff);
            }

            Objective { name, coefficients }
        },
    );

    let (remainder, objectives) = many1(parser)(input)?;

    Ok((remainder, (objectives.into_iter().map(|ob| (ob.name, ob)).collect(), objective_vars)))
}

#[cfg(test)]
mod test {
    use crate::nom::decoder::objective::parse_objectives;

    #[test]
    fn test_objective_section() {
        let input = " obj1: -0.5 x - 2y - 8z\n obj2: y + x + z\n obj3: 10z - 2.5x\n       + y";

        let (input, (objs, vars)) = parse_objectives(input).unwrap();

        assert_eq!(input, "");
        assert_eq!(objs.len(), 3);
        assert_eq!(vars.len(), 3);
    }
}
