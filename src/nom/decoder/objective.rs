use nom::{
    character::complete::{char, multispace0, multispace1, space0},
    combinator::{map, not, peek},
    multi::{many0, many1},
    sequence::{delimited, preceded, terminated, tuple},
    IResult,
};

use crate::nom::{
    decoder::{coefficient::parse_coefficient, variable::parse_variable},
    model::{Coefficient, Objective},
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
fn objective(input: &str) -> IResult<&str, Objective> {
    map(
        tuple((
            // Name part (required)
            terminated(preceded(multispace0, parse_variable), delimited(multispace0, char(':'), multispace0)),
            // Initial coefficients
            many1(preceded(space0, parse_coefficient)),
            // Continuation lines
            many0(line_continuation),
        )),
        |(name, first_coefficients, continuation_coefficients)| {
            let mut coefficients = first_coefficients;

            for mut cont_coeffs in continuation_coefficients {
                coefficients.append(&mut cont_coeffs);
            }

            Objective { name: Some(name.to_string()), coefficients }
        },
    )(input)
}

#[inline]
pub fn parse_objectives(input: &str) -> IResult<&str, Vec<Objective>> {
    many1(objective)(input)
}

#[cfg(test)]
mod test {
    use crate::nom::decoder::objective::parse_objectives;

    #[test]
    fn test_objective_section() {
        let input = " obj1: -0.5 x - 2y - 8z\n obj2: y + x + z\n obj3: 10z - 2.5x\n       + y";
        let (_, objectives) = parse_objectives(input).unwrap();
        insta::assert_debug_snapshot!(objectives);
    }
}
