//! Main problem representation and parsing logic.
//!
//! This module defines the `LpProblem` struct and it's associated
//! parsing functionality. It serves as the main entry point for
//! working with LP problems.
//!

use std::borrow::Cow;
use std::collections::HashMap;
use std::collections::hash_map::Entry;

use nom::Parser as _;
use nom::combinator::opt;

use crate::error::{LpParseError, LpResult};
use crate::model::{Constraint, Objective, Sense, Variable, VariableType};
use crate::parsers::constraint::{parse_constraint_header, parse_constraints};
use crate::parsers::objective::parse_objectives;
use crate::parsers::problem_name::parse_problem_name;
use crate::parsers::sense::parse_sense;
use crate::parsers::sos_constraint::parse_sos_section;
use crate::parsers::variable::{
    parse_binary_section, parse_bounds_section, parse_generals_section, parse_integer_section, parse_semi_section,
};
use crate::{
    ALL_BOUND_HEADERS, BINARY_HEADERS, CONSTRAINT_HEADERS, END_HEADER, GENERAL_HEADERS, INTEGER_HEADERS, SEMI_HEADERS, SOS_HEADERS,
    is_binary_section, is_bounds_section, is_generals_section, is_integers_section, is_semi_section, is_sos_section, take_until_parser,
};

// Type aliases to reduce complexity
type ObjectivesParseResult<'a> = (HashMap<Cow<'a, str>, Objective<'a>>, HashMap<&'a str, Variable<'a>>);
type ConstraintsParseResult<'a> = (&'a str, HashMap<Cow<'a, str>, Constraint<'a>>, HashMap<&'a str, Variable<'a>>);

#[cfg_attr(feature = "diff", derive(diff::Diff), diff(attr(#[derive(Debug, PartialEq)])))]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[derive(Debug, Default, PartialEq)]
/// Represents a Linear Programming (LP) problem.
///
/// The `LpProblem` struct encapsulates the components of an LP problem, including its name,
/// sense (e.g., minimization, or maximization), objectives, constraints, and variables.
///
/// # Attributes
///
/// * `#[cfg_attr(feature = "diff", derive(diff::Diff), diff(attr(#[derive(Debug, PartialEq)])))]`:
///   Enables the `diff` feature for comparing differences between instances of `LpProblem`.
/// * `#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]`:
///   Enables serialization and deserialization of `LpProblem` instances when the `serde` feature is active.
///
pub struct LpProblem<'a> {
    /// An optional reference to a string slice representing the name of the LP problem.
    pub name: Option<Cow<'a, str>>,
    /// The optimization sense of the problem, indicating whether it is a minimization or maximization problem.
    pub sense: Sense,
    /// A `HashMap` where the keys are the names of the objectives and the values are `Objective` structs.
    pub objectives: HashMap<Cow<'a, str>, Objective<'a>>,
    /// A `HashMap` where the keys are the names of the constraints and the values are `Constraint` structs.
    pub constraints: HashMap<Cow<'a, str>, Constraint<'a>>,
    /// A `HashMap` where the keys are the names of the variables and the values are `Variable` structs.
    pub variables: HashMap<&'a str, Variable<'a>>,
}

impl<'a> LpProblem<'a> {
    #[must_use]
    #[inline]
    /// Initialise a new `Self`
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    #[inline]
    /// Override the problem name
    pub fn with_problem_name(self, problem_name: Cow<'a, str>) -> Self {
        Self { name: Some(problem_name), ..self }
    }

    #[must_use]
    #[inline]
    /// Override the problem sense
    pub fn with_sense(self, sense: Sense) -> Self {
        Self { sense, ..self }
    }

    #[must_use]
    #[inline]
    /// Returns the name of the LP Problem
    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    #[must_use]
    #[inline]
    /// Returns `true` if the `Self` a Minimize LP Problem
    pub fn is_minimization(&self) -> bool {
        self.sense.is_minimization()
    }

    #[must_use]
    #[inline]
    /// Returns the number of constraints contained within the Problem
    pub fn constraint_count(&self) -> usize {
        self.constraints.len()
    }

    #[must_use]
    #[inline]
    /// Returns the number of objectives contained within the Problem
    pub fn objective_count(&self) -> usize {
        self.objectives.len()
    }

    #[must_use]
    #[inline]
    /// Returns the number of variables contained within the Problem
    pub fn variable_count(&self) -> usize {
        self.variables.len()
    }

    #[inline]
    /// Parse a `Self` from a string slice
    pub fn parse(input: &'a str) -> LpResult<Self> {
        log::debug!("Starting to parse LP problem");
        Self::try_from(input)
    }

    #[inline]
    /// Add a new variable to the problem.
    ///
    /// If a variable with the same name already exists, it will be replaced.
    pub fn add_variable(&mut self, variable: Variable<'a>) {
        self.variables.insert(variable.name, variable);
    }

    #[inline]
    /// Add a new constraint to the problem.
    ///
    /// If a constraint with the same name already exists, it will be replaced.
    pub fn add_constraint(&mut self, constraint: Constraint<'a>) {
        let name = constraint.name().as_ref().to_owned();

        if let Constraint::Standard { coefficients, .. } = &constraint {
            for coeff in coefficients {
                if !self.variables.contains_key(coeff.name) {
                    self.variables.insert(coeff.name, Variable::new(coeff.name));
                }
            }
        }

        if let Constraint::SOS { weights, .. } = &constraint {
            for coeff in weights {
                if !self.variables.contains_key(coeff.name) {
                    self.variables.insert(coeff.name, Variable::new(coeff.name).with_var_type(VariableType::SOS));
                }
            }
        }

        self.constraints.insert(Cow::Owned(name), constraint);
    }

    #[inline]
    /// Add a new objective to the problem.
    ///
    /// If an objective with the same name already exists, it will be replaced.
    pub fn add_objective(&mut self, objective: Objective<'a>) {
        for coeff in &objective.coefficients {
            if !self.variables.contains_key(coeff.name) {
                self.variables.insert(coeff.name, Variable::new(coeff.name));
            }
        }

        let name = objective.name.clone();
        self.objectives.insert(name, objective);
    }
}

impl std::fmt::Display for LpProblem<'_> {
    #[inline]
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(problem_name) = &self.name {
            writeln!(f, "Problem name: {problem_name}")?;
        }
        writeln!(f, "Sense: {}", self.sense)?;
        writeln!(f, "Objectives: {}", self.objectives.len())?;
        writeln!(f, "Constraints: {}", self.constraints.len())?;
        writeln!(f, "Variables: {}", self.variables.len())?;

        Ok(())
    }
}

impl<'a> LpProblem<'a> {
    /// Parse the header section (name and sense)
    fn parse_header_section(input: &'a str) -> LpResult<(&'a str, &'a str, Option<Cow<'a, str>>, Sense)> {
        let (remaining_input, (name, sense, obj_section, ())) =
            (parse_problem_name, parse_sense, take_until_parser(&CONSTRAINT_HEADERS), parse_constraint_header)
                .parse(input)
                .map_err(|err| LpParseError::parse_error(0, format!("Failed to parse header section: {err:?}")))?;

        Ok((remaining_input, obj_section, name, sense))
    }

    /// Parse the objectives section
    fn parse_objectives_section(input: &'a str) -> LpResult<ObjectivesParseResult<'a>> {
        let (_, (objectives, variables)) =
            parse_objectives(input).map_err(|err| LpParseError::objective_syntax(0, format!("Failed to parse objectives: {err:?}")))?;

        Ok((objectives, variables))
    }

    /// Parse the constraints section
    fn parse_constraints_section(input: &'a str) -> LpResult<ConstraintsParseResult<'a>> {
        let (input, constraint_str) = take_until_parser(&ALL_BOUND_HEADERS)(input)
            .map_err(|err| LpParseError::constraint_syntax(0, format!("Failed to find constraints section: {err:?}")))?;

        let (_, (constraints, constraint_vars)) = parse_constraints(constraint_str)
            .map_err(|err| LpParseError::constraint_syntax(0, format!("Failed to parse constraints: {err:?}")))?;

        Ok((input, constraints, constraint_vars))
    }

    /// Parse variable bounds section
    fn parse_bounds_section(input: &'a str, variables: &mut HashMap<&'a str, Variable<'a>>) -> LpResult<&'a str> {
        if is_bounds_section(input).is_ok() {
            let (rem_input, bound_str) = take_until_parser(&INTEGER_HEADERS)(input)
                .map_err(|err| LpParseError::parse_error(0, format!("Failed to parse bounds section: {err:?}")))?;

            let (_, bounds) = parse_bounds_section(bound_str)
                .map_err(|err| LpParseError::invalid_bounds("unknown", format!("Failed to parse bounds: {err:?}")))?;

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

            Ok(rem_input)
        } else {
            Ok(input)
        }
    }

    /// Parse variable type sections (integers, generals, binaries, semi-continuous)
    fn parse_variable_type_sections(mut input: &'a str, variables: &mut HashMap<&'a str, Variable<'a>>) -> LpResult<&'a str> {
        // Integer
        if is_integers_section(input).is_ok() {
            if let Ok((rem_input, Some(integer_str))) = opt(take_until_parser(&GENERAL_HEADERS)).parse(input) {
                if let Ok((_, integer_vars)) = parse_integer_section(integer_str) {
                    set_var_types(variables, integer_vars, VariableType::Integer);
                }
                input = rem_input;
            }
        }

        // General
        if is_generals_section(input).is_ok() {
            if let Ok((rem_input, Some(generals_str))) = opt(take_until_parser(&BINARY_HEADERS)).parse(input) {
                if let Ok((_, general_vars)) = parse_generals_section(generals_str) {
                    set_var_types(variables, general_vars, VariableType::General);
                }
                input = rem_input;
            }
        }

        // Binary
        if is_binary_section(input).is_ok() {
            if let Ok((rem_input, Some(binary_str))) = opt(take_until_parser(&SEMI_HEADERS)).parse(input) {
                if let Ok((_, binary_vars)) = parse_binary_section(binary_str) {
                    set_var_types(variables, binary_vars, VariableType::Binary);
                }
                input = rem_input;
            }
        }

        // Semi-continuous
        if is_semi_section(input).is_ok() {
            if let Ok((rem_input, Some(semi_str))) = opt(take_until_parser(&SOS_HEADERS)).parse(input) {
                if let Ok((_, semi_vars)) = parse_semi_section(semi_str) {
                    set_var_types(variables, semi_vars, VariableType::SemiContinuous);
                }
                input = rem_input;
            }
        }

        Ok(input)
    }

    /// Parse SOS constraints section
    fn parse_sos_section(
        input: &'a str,
        constraints: &mut HashMap<Cow<'a, str>, Constraint<'a>>,
        variables: &mut HashMap<&'a str, Variable<'a>>,
    ) -> LpResult<&'a str> {
        if is_sos_section(input).is_ok() {
            if let Ok((rem_input, Some(sos_str))) = opt(take_until_parser(&END_HEADER)).parse(input) {
                if let Ok((_, Some((sos_constraints, constraint_vars)))) = opt(parse_sos_section).parse(sos_str) {
                    variables.extend(constraint_vars);
                    for (name, constraint) in sos_constraints {
                        constraints.insert(name, constraint);
                    }
                }
                Ok(rem_input)
            } else {
                Ok(input)
            }
        } else {
            Ok(input)
        }
    }

    /// Validate remaining unparsed input
    fn validate_remaining_input(input: &str) -> LpResult<()> {
        if input.len() > 3 {
            log::warn!("Unused input not parsed by `LpProblem`: {input}");
        }
        Ok(())
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
    type Error = LpParseError;

    #[inline]
    fn try_from(input: &'a str) -> Result<Self, Self::Error> {
        log::debug!("Starting to parse LP problem");

        // Parse header section (name and sense)
        let (input, obj_section, name, sense) = Self::parse_header_section(input)?;

        // Parse objectives section
        let (objectives, mut variables) = Self::parse_objectives_section(obj_section)?;

        // Parse constraints section
        let (mut input, mut constraints, constraint_vars) = Self::parse_constraints_section(input)?;
        variables.extend(constraint_vars);

        // Parse bounds section
        input = Self::parse_bounds_section(input, &mut variables)?;

        // Parse variable type sections
        input = Self::parse_variable_type_sections(input, &mut variables)?;

        // Parse SOS constraints section
        input = Self::parse_sos_section(input, &mut constraints, &mut variables)?;

        // Validate remaining input
        Self::validate_remaining_input(input)?;

        Ok(LpProblem { name, sense, objectives, constraints, variables })
    }
}

#[cfg(feature = "serde")]
impl<'de: 'a, 'a> serde::Deserialize<'de> for LpProblem<'a> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(serde::Deserialize)]
        #[serde(field_identifier, rename_all = "lowercase")]
        enum Field {
            Constraints,
            Name,
            Objectives,
            Sense,
            Variables,
        }

        struct LpProblemVisitor<'a>(std::marker::PhantomData<LpProblem<'a>>);

        impl<'de: 'a, 'a> serde::de::Visitor<'de> for LpProblemVisitor<'a> {
            type Value = LpProblem<'a>;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("struct LpProblem")
            }

            fn visit_map<V: serde::de::MapAccess<'de>>(self, mut map: V) -> Result<LpProblem<'a>, V::Error> {
                let mut name: Option<Cow<'_, str>> = None;
                let mut sense = None;
                let mut objectives = None;
                let mut constraints = None;
                let mut variables = None;

                while let Some(key) = map.next_key()? {
                    match key {
                        Field::Name => {
                            if name.is_some() {
                                return Err(serde::de::Error::duplicate_field("name"));
                            }
                            name = map.next_value()?;
                        }
                        Field::Sense => {
                            if sense.is_some() {
                                return Err(serde::de::Error::duplicate_field("sense"));
                            }
                            sense = Some(map.next_value()?);
                        }
                        Field::Objectives => {
                            if objectives.is_some() {
                                return Err(serde::de::Error::duplicate_field("objectives"));
                            }
                            objectives = Some(map.next_value()?);
                        }
                        Field::Constraints => {
                            if constraints.is_some() {
                                return Err(serde::de::Error::duplicate_field("constraints"));
                            }
                            constraints = Some(map.next_value()?);
                        }
                        Field::Variables => {
                            if variables.is_some() {
                                return Err(serde::de::Error::duplicate_field("variables"));
                            }
                            variables = Some(map.next_value()?);
                        }
                    }
                }

                Ok(LpProblem {
                    name,
                    sense: sense.unwrap_or_default(),
                    objectives: objectives.unwrap_or_default(),
                    constraints: constraints.unwrap_or_default(),
                    variables: variables.unwrap_or_default(),
                })
            }
        }

        const FIELDS: &[&str] = &["name", "sense", "objectives", "constraints", "variables"];
        deserializer.deserialize_struct("LpProblem", FIELDS, LpProblemVisitor(std::marker::PhantomData))
    }
}

#[cfg(test)]
mod test {
    use std::borrow::Cow;

    use crate::model::{Coefficient, ComparisonOp, Constraint, Objective, Sense, Variable, VariableType};
    use crate::problem::LpProblem;

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

    #[test]
    fn test_small_input() {
        let problem = LpProblem::try_from(SMALL_INPUT).expect("test case not to fail");

        assert_eq!(problem.objectives.len(), 3);
        assert_eq!(problem.constraints.len(), 3);

        #[cfg(feature = "serde")]
        insta::assert_yaml_snapshot!(&problem, {
            ".objectives" => insta::sorted_redaction(),
            ".constraints" => insta::sorted_redaction(),
            ".variables" => insta::sorted_redaction()
        });
    }

    #[test]
    fn test_minified_example() {
        let problem = LpProblem::try_from(COMPLETE_INPUT).expect("test case not to fail");

        assert_eq!(problem.objectives.len(), 3);
        assert_eq!(problem.constraints.len(), 5);

        #[cfg(feature = "serde")]
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
        let serialized_problem = serde_json::to_string(&problem).expect("test case not to fail");
        let _: LpProblem<'_> = serde_json::from_str(&serialized_problem).expect("test case not to fail");
    }

    #[test]
    fn test_add_variable() {
        let mut problem = LpProblem::new();
        let var = Variable::new("x1").with_var_type(VariableType::Binary);

        problem.add_variable(var);
        assert_eq!(problem.variable_count(), 1);
        assert!(problem.variables.contains_key("x1"));
    }

    #[test]
    fn test_add_constraint() {
        let mut problem = LpProblem::new();
        let constraint = Constraint::Standard {
            name: Cow::Borrowed("c1"),
            coefficients: vec![Coefficient { name: "x1", value: 1.0 }, Coefficient { name: "x2", value: 2.0 }],
            operator: ComparisonOp::LTE,
            rhs: 5.0,
        };

        problem.add_constraint(constraint);
        assert_eq!(problem.constraint_count(), 1);
        assert_eq!(problem.variable_count(), 2);
    }

    #[test]
    fn test_add_objective() {
        let mut problem = LpProblem::new().with_sense(Sense::Minimize).with_problem_name(Cow::Borrowed("test"));
        let objective = Objective {
            name: Cow::Borrowed("obj1"),
            coefficients: vec![Coefficient { name: "x1", value: 1.0 }, Coefficient { name: "x2", value: -1.0 }],
        };

        problem.add_objective(objective);
        assert_eq!(problem.objective_count(), 1);
        assert_eq!(problem.variable_count(), 2);
    }
}
