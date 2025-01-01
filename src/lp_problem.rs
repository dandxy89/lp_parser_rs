use std::{
    borrow::Cow,
    collections::{hash_map::Entry, HashMap},
};

use nom::{combinator::opt, sequence::tuple};

use crate::{
    decoder::{
        constraint::{parse_constraint_header, parse_constraints},
        objective::parse_objectives,
        problem_name::parse_problem_name,
        sense::parse_sense,
        sos_constraint::parse_sos_section,
        variable::{parse_binary_section, parse_bounds_section, parse_generals_section, parse_integer_section, parse_semi_section},
    },
    is_binary_section, is_bounds_section, is_generals_section, is_integers_section, is_semi_section, is_sos_section,
    model::{Constraint, Objective, Sense, Variable, VariableType},
    take_until_parser, ALL_BOUND_HEADERS, BINARY_HEADERS, CONSTRAINT_HEADERS, END_HEADER, GENERAL_HEADERS, INTEGER_HEADERS, SEMI_HEADERS,
    SOS_HEADERS,
};

#[cfg_attr(feature = "diff", derive(diff::Diff), diff(attr(#[derive(Debug, PartialEq)])))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Default, PartialEq)]
pub struct LpProblem<'a> {
    pub name: Option<&'a str>,
    pub sense: Sense,
    pub objectives: HashMap<Cow<'a, str>, Objective<'a>>,
    pub constraints: HashMap<Cow<'a, str>, Constraint<'a>>,
    pub variables: HashMap<&'a str, Variable<'a>>,
}

impl<'a> LpProblem<'a> {
    // TODO:
    // add_constraint
    // add_constraints
    // add_objective
    // add_variable
    // set_variable_bound
    // with_problem_name

    /// Initialise a new `Self`
    pub fn new() -> Self {
        Self::default()
    }

    /// Override the problem sense
    pub fn with_sense(self, sense: Sense) -> Self {
        Self { sense, ..self }
    }

    #[inline]
    /// Returns the name of the LP Problem
    pub fn name(&self) -> Option<&str> {
        self.name
    }

    #[inline]
    /// Returns `true` if the `Self` a Minimize LP Problem
    pub fn is_minimization(&self) -> bool {
        self.sense.is_minimization()
    }

    #[inline]
    /// Returns the number of constraints contained within the Problem
    pub fn constraint_count(&self) -> usize {
        self.constraints.len()
    }

    #[inline]
    /// Returns the number of objectives contained within the Problem
    pub fn objective_count(&self) -> usize {
        self.objectives.len()
    }

    #[inline]
    /// Returns the number of variables contained within the Problem
    pub fn variable_count(&self) -> usize {
        self.variables.len()
    }

    /// Parse a `Self` from a string slice
    pub fn parse(input: &'a str) -> Result<Self, nom::Err<nom::error::Error<&'a str>>> {
        // Ideally, we'd have like to have utilised `FromStr` but the trait does not allow the
        // specification of lifetimes.
        TryFrom::try_from(input)
    }
}

#[inline]
fn set_var_types<'a>(variables: &mut HashMap<&'a str, Variable<'a>>, vars: Vec<&'a str>, var_type: VariableType) {
    for name in vars {
        match variables.entry(name) {
            Entry::Occupied(mut occupied_entry) => {
                occupied_entry.get_mut().set_var_type(var_type.clone());
            }
            Entry::Vacant(vacant_entry) => {
                vacant_entry.insert(Variable { name, var_type: var_type.clone() });
            }
        }
    }
}

impl<'a> TryFrom<&'a str> for LpProblem<'a> {
    type Error = nom::Err<nom::error::Error<&'a str>>;

    #[inline]
    fn try_from(input: &'a str) -> Result<Self, Self::Error> {
        // Problem name and Sense
        let (input, (name, sense, obj_section, _)) =
            tuple((parse_problem_name, parse_sense, take_until_parser(&CONSTRAINT_HEADERS), parse_constraint_header))(input)?;
        let (_, (objectives, mut variables)) = parse_objectives(obj_section)?;

        // Constraints
        let (mut input, constraint_str) = take_until_parser(&ALL_BOUND_HEADERS)(input)?;
        let (_, (mut constraints, constraint_vars)) = parse_constraints(constraint_str)?;
        variables.extend(constraint_vars);

        // Bound
        if is_bounds_section(input).is_ok() {
            let (rem_input, bound_str) = take_until_parser(&INTEGER_HEADERS)(input)?;
            let (_, bounds) = parse_bounds_section(bound_str)?;

            for (name, var_type) in bounds {
                match variables.entry(name) {
                    Entry::Occupied(mut occupied_entry) => {
                        occupied_entry.get_mut().set_var_type(var_type);
                    }
                    Entry::Vacant(vacant_entry) => {
                        vacant_entry.insert(Variable { name, var_type });
                    }
                }
            }

            input = rem_input;
        }

        // Integer
        if is_integers_section(input).is_ok() {
            if let Ok((rem_input, Some(integer_str))) = opt(take_until_parser(&GENERAL_HEADERS))(input) {
                if let Ok((_, integer_vars)) = parse_integer_section(integer_str) {
                    set_var_types(&mut variables, integer_vars, VariableType::Integer);
                }
                input = rem_input;
            }
        }

        // General
        if is_generals_section(input).is_ok() {
            if let Ok((rem_input, Some(generals_str))) = opt(take_until_parser(&BINARY_HEADERS))(input) {
                if let Ok((_, general_vars)) = parse_generals_section(generals_str) {
                    set_var_types(&mut variables, general_vars, VariableType::General);
                }
                input = rem_input;
            }
        }

        // Binary
        if is_binary_section(input).is_ok() {
            if let Ok((rem_input, Some(binary_str))) = opt(take_until_parser(&SEMI_HEADERS))(input) {
                if let Ok((_, binary_vars)) = parse_binary_section(binary_str) {
                    set_var_types(&mut variables, binary_vars, VariableType::Binary);
                }
                input = rem_input;
            }
        }

        // Semi-continuous
        if is_semi_section(input).is_ok() {
            if let Ok((rem_input, Some(semi_str))) = opt(take_until_parser(&SOS_HEADERS))(input) {
                if let Ok((_, semi_vars)) = parse_semi_section(semi_str) {
                    set_var_types(&mut variables, semi_vars, VariableType::SemiContinuous);
                }
                input = rem_input;
            }
        }

        // SOS constraint
        if is_sos_section(input).is_ok() {
            if let Ok((rem_input, Some(sos_str))) = opt(take_until_parser(&END_HEADER))(input) {
                if let Ok((_, Some((sos_constraints, constraint_vars)))) = opt(parse_sos_section)(sos_str) {
                    variables.extend(constraint_vars);
                    for (name, constraint) in sos_constraints {
                        constraints.insert(name, constraint);
                    }
                }
                input = rem_input;
            }
        }

        if input.len() > 3 {
            log::warn!("Unused input not parsed by `LpProblem`: {input}");
        }

        Ok(LpProblem { name, sense, objectives, constraints, variables })
    }
}

#[cfg(test)]
mod test {
    use crate::lp_problem::LpProblem;

    const SMALL_INPUT: &str = "\\ This file has been generated by Author
\\ ENCODING=ISO-8859-1
\\Problem name: diet
Minimize
 obj1: -0.5 x - 2y - 8z
 obj2: y + x + z
 obj3: 10z - 2.5x
       + y
subject to:
c1:  3 x1 + x2 + 2 x3 = 30
c2:  2 x1 + x2 + 3 x3 + x4 >= 15
c3:  2 x2 + 3 x4 <= 25
bounds
x1 free
x2 >= 1
100 <= x2dfsdf <= -1
End";

    const COMPLETE_INPUT: &str = "\\ This file has been generated by Author
\\ ENCODING=ISO-8859-1
\\Problem name: diet
Minimize
 obj1: -0.5 x - 2y - 8z
 obj2: y + x + z
 obj3: 10z - 2.5x
       + y
subject to:
c1:  3 x1 + x2 + 2 x3 = 30
c2:  2 x1 + x2 + 3 x3 + x4 >= 15
c3:  2 x2 + 3 x4 <= 25
bounds
x1 free
x2 >= 1
100 <= x2dfsdf <= -1
Integers
X31
X32
Generals
Route_A_1
Route_A_2
Route_A_3
Binary
V8
Semi-Continuous
 y
SOS
csos1: S1:: V1:1 V3:2 V5:3
csos2: S2:: V2:2 V4:1 V5:2.5
End";

    #[cfg(feature = "serde")]
    #[test]
    fn test_small_input() {
        let problem = LpProblem::try_from(SMALL_INPUT).expect("test case not to fail");

        assert_eq!(problem.objectives.len(), 3);
        assert_eq!(problem.constraints.len(), 3);

        insta::assert_yaml_snapshot!(&problem, {
            ".objectives" => insta::sorted_redaction(),
            ".constraints" => insta::sorted_redaction(),
            ".variables" => insta::sorted_redaction()
        });
    }

    #[cfg(feature = "serde")]
    #[test]
    fn test_minified_example() {
        let problem = LpProblem::try_from(COMPLETE_INPUT).expect("test case not to fail");

        assert_eq!(problem.objectives.len(), 3);
        assert_eq!(problem.constraints.len(), 5);

        insta::assert_yaml_snapshot!(&problem, {
            ".objectives" => insta::sorted_redaction(),
            ".constraints" => insta::sorted_redaction(),
            ".variables" => insta::sorted_redaction()
        });
    }

    #[cfg(feature = "serde")]
    #[test]
    fn test_serialization_lifecycle() {
        let problem = LpProblem::try_from(COMPLETE_INPUT).expect("test case not to fail");
        // Serialized
        let serialized_problem = serde_json::to_string(&problem).expect("test case not to fail");
        // Deserialise
        let _: LpProblem<'_> = serde_json::from_str(&serialized_problem).expect("test case not to fail");
    }
}
