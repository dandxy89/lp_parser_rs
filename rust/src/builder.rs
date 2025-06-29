//! Builder pattern API for constructing LP problems.
//!
//! This module provides a fluent, type-safe API for constructing LP problems
//! programmatically with method chaining and validation.

use std::borrow::Cow;
use std::collections::HashMap;

use crate::error::{LpParseError, LpResult};
use crate::model::{Coefficient, ComparisonOp, Constraint, Objective, SOSType, Sense, Variable, VariableType};
use crate::problem::LpProblem;

/// Builder for constructing LP problems with a fluent API
#[derive(Debug, Default)]
pub struct LpProblemBuilder<'a> {
    name: Option<Cow<'a, str>>,
    sense: Option<Sense>,
    objectives: HashMap<Cow<'a, str>, ObjectiveBuilder<'a>>,
    constraints: HashMap<Cow<'a, str>, ConstraintBuilder<'a>>,
    variables: HashMap<&'a str, VariableBuilder<'a>>,
}

/// Builder for objectives
#[derive(Debug, Clone)]
pub struct ObjectiveBuilder<'a> {
    name: Cow<'a, str>,
    coefficients: Vec<Coefficient<'a>>,
}

/// Builder for constraints
#[derive(Debug, Clone)]
pub enum ConstraintBuilder<'a> {
    /// Standard linear constraint builder
    Standard { name: Cow<'a, str>, coefficients: Vec<Coefficient<'a>>, operator: Option<ComparisonOp>, rhs: Option<f64> },
    /// SOS constraint builder
    SOS { name: Cow<'a, str>, sos_type: SOSType, weights: Vec<Coefficient<'a>> },
}

/// Builder for variables
#[derive(Debug, Clone)]
pub struct VariableBuilder<'a> {
    name: &'a str,
    var_type: VariableType,
}

impl<'a> LpProblemBuilder<'a> {
    #[must_use]
    /// Create a new LP problem builder
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    /// Set the problem name
    pub fn name(mut self, name: impl Into<Cow<'a, str>>) -> Self {
        self.name = Some(name.into());
        self
    }

    #[must_use]
    /// Set the optimization sense
    pub const fn sense(mut self, sense: Sense) -> Self {
        self.sense = Some(sense);
        self
    }

    #[must_use]
    /// Set the problem to minimize
    pub const fn minimize(self) -> Self {
        self.sense(Sense::Minimize)
    }

    #[must_use]
    /// Set the problem to maximize
    pub const fn maximize(self) -> Self {
        self.sense(Sense::Maximize)
    }

    #[must_use]
    /// Add an objective to the problem
    pub fn objective<F>(mut self, name: impl Into<Cow<'a, str>>, f: F) -> Self
    where
        F: FnOnce(ObjectiveBuilder<'a>) -> ObjectiveBuilder<'a>,
    {
        let name = name.into();
        let builder = ObjectiveBuilder::new(name.clone());
        let completed_builder = f(builder);
        self.objectives.insert(name, completed_builder);
        self
    }

    #[must_use]
    /// Add a constraint to the problem
    pub fn constraint<F>(mut self, name: impl Into<Cow<'a, str>>, f: F) -> Self
    where
        F: FnOnce(ConstraintBuilder<'a>) -> ConstraintBuilder<'a>,
    {
        let name = name.into();
        let builder = ConstraintBuilder::standard(name.clone());
        let completed_builder = f(builder);
        self.constraints.insert(name, completed_builder);
        self
    }

    #[must_use]
    /// Add an SOS constraint to the problem
    pub fn sos_constraint<F>(mut self, name: impl Into<Cow<'a, str>>, sos_type: SOSType, f: F) -> Self
    where
        F: FnOnce(ConstraintBuilder<'a>) -> ConstraintBuilder<'a>,
    {
        let name = name.into();
        let builder = ConstraintBuilder::sos(name.clone(), sos_type);
        let completed_builder = f(builder);
        self.constraints.insert(name, completed_builder);
        self
    }

    #[must_use]
    /// Add a variable to the problem
    pub fn variable<F>(mut self, name: &'a str, f: F) -> Self
    where
        F: FnOnce(VariableBuilder<'a>) -> VariableBuilder<'a>,
    {
        let builder = VariableBuilder::new(name);
        let completed_builder = f(builder);
        self.variables.insert(name, completed_builder);
        self
    }

    #[must_use]
    /// Add multiple variables with the same type
    pub fn variables(mut self, names: &[&'a str], var_type: &VariableType) -> Self {
        for &name in names {
            let builder = VariableBuilder::new(name).var_type(var_type.clone());
            self.variables.insert(name, builder);
        }
        self
    }

    /// Build the LP problem
    pub fn build(self) -> LpResult<LpProblem<'a>> {
        let mut problem = LpProblem::new();

        // Set name and sense
        if let Some(name) = self.name {
            problem = problem.with_problem_name(name);
        }
        problem = problem.with_sense(self.sense.unwrap_or_default());

        // Add variables first
        for (_, var_builder) in self.variables {
            let variable = var_builder.build()?;
            problem.add_variable(variable);
        }

        // Add objectives
        for (_, obj_builder) in self.objectives {
            let objective = obj_builder.build()?;
            problem.add_objective(objective);
        }

        // Add constraints
        for (_, constraint_builder) in self.constraints {
            let constraint = constraint_builder.build()?;
            problem.add_constraint(constraint);
        }

        Ok(problem)
    }
}

impl<'a> ObjectiveBuilder<'a> {
    #[must_use]
    /// Create a new objective builder
    pub const fn new(name: Cow<'a, str>) -> Self {
        Self { name, coefficients: Vec::new() }
    }

    #[must_use]
    /// Add a coefficient to the objective
    pub fn coefficient(mut self, name: &'a str, value: f64) -> Self {
        self.coefficients.push(Coefficient { name, value });
        self
    }

    #[must_use]
    /// Add multiple coefficients at once
    ///
    /// Note: This method requires coefficient names to have the same lifetime as the builder.
    /// For dynamic strings, use individual `coefficient()` calls instead.
    pub fn coefficients(mut self, coeffs: &[(&'a str, f64)]) -> Self {
        for &(name, value) in coeffs {
            self.coefficients.push(Coefficient { name, value });
        }
        self
    }

    /// Build the objective
    pub fn build(self) -> LpResult<Objective<'a>> {
        if self.coefficients.is_empty() {
            return Err(LpParseError::validation_error(format!("Objective '{}' has no coefficients", self.name)));
        }

        Ok(Objective { name: self.name, coefficients: self.coefficients })
    }
}

impl<'a> ConstraintBuilder<'a> {
    #[must_use]
    /// Create a new standard constraint builder
    pub const fn standard(name: Cow<'a, str>) -> Self {
        Self::Standard { name, coefficients: Vec::new(), operator: None, rhs: None }
    }

    #[must_use]
    /// Create a new SOS constraint builder
    pub const fn sos(name: Cow<'a, str>, sos_type: SOSType) -> Self {
        Self::SOS { name, sos_type, weights: Vec::new() }
    }

    #[must_use]
    /// Add a coefficient to the constraint
    pub fn coefficient(mut self, name: &'a str, value: f64) -> Self {
        match &mut self {
            Self::Standard { coefficients, .. } => {
                coefficients.push(Coefficient { name, value });
            }
            Self::SOS { weights, .. } => {
                weights.push(Coefficient { name, value });
            }
        }
        self
    }

    #[must_use]
    /// Set the constraint to less than or equal
    pub fn le(mut self, rhs: f64) -> Self {
        if let Self::Standard { operator, rhs: rhs_ref, .. } = &mut self {
            *operator = Some(ComparisonOp::LTE);
            *rhs_ref = Some(rhs);
        }
        self
    }

    #[must_use]
    /// Set the constraint to less than
    pub fn lt(mut self, rhs: f64) -> Self {
        if let Self::Standard { operator, rhs: rhs_ref, .. } = &mut self {
            *operator = Some(ComparisonOp::LT);
            *rhs_ref = Some(rhs);
        }
        self
    }

    #[must_use]
    /// Set the constraint to greater than or equal
    pub fn ge(mut self, rhs: f64) -> Self {
        if let Self::Standard { operator, rhs: rhs_ref, .. } = &mut self {
            *operator = Some(ComparisonOp::GTE);
            *rhs_ref = Some(rhs);
        }
        self
    }

    #[must_use]
    /// Set the constraint to greater than
    pub fn gt(mut self, rhs: f64) -> Self {
        if let Self::Standard { operator, rhs: rhs_ref, .. } = &mut self {
            *operator = Some(ComparisonOp::GT);
            *rhs_ref = Some(rhs);
        }
        self
    }

    #[must_use]
    /// Set the constraint to equal
    pub fn eq(mut self, rhs: f64) -> Self {
        if let Self::Standard { operator, rhs: rhs_ref, .. } = &mut self {
            *operator = Some(ComparisonOp::EQ);
            *rhs_ref = Some(rhs);
        }
        self
    }

    /// Build the constraint
    pub fn build(self) -> LpResult<Constraint<'a>> {
        match self {
            Self::Standard { name, coefficients, operator, rhs } => {
                let operator =
                    operator.ok_or_else(|| LpParseError::constraint_syntax(0, format!("Constraint '{name}' is missing an operator")))?;
                let rhs =
                    rhs.ok_or_else(|| LpParseError::constraint_syntax(0, format!("Constraint '{name}' is missing a right-hand side")))?;

                if coefficients.is_empty() {
                    return Err(LpParseError::constraint_syntax(0, format!("Constraint '{name}' has no coefficients")));
                }

                Ok(Constraint::Standard { name, coefficients, operator, rhs })
            }
            Self::SOS { name, sos_type, weights } => {
                if weights.is_empty() {
                    return Err(LpParseError::invalid_sos_constraint(name.as_ref(), "No weights specified"));
                }

                Ok(Constraint::SOS { name, sos_type, weights })
            }
        }
    }
}

impl<'a> VariableBuilder<'a> {
    #[must_use]
    /// Create a new variable builder
    pub fn new(name: &'a str) -> Self {
        Self { name, var_type: VariableType::default() }
    }

    #[must_use]
    /// Set the variable type
    pub const fn var_type(mut self, var_type: VariableType) -> Self {
        self.var_type = var_type;
        self
    }

    #[must_use]
    /// Set the variable as binary
    pub const fn binary(self) -> Self {
        self.var_type(VariableType::Binary)
    }

    #[must_use]
    /// Set the variable as integer
    pub const fn integer(self) -> Self {
        self.var_type(VariableType::Integer)
    }

    #[must_use]
    /// Set the variable as general (non-negative)
    pub const fn general(self) -> Self {
        self.var_type(VariableType::General)
    }

    #[must_use]
    /// Set the variable as free (unbounded)
    pub const fn free(self) -> Self {
        self.var_type(VariableType::Free)
    }

    #[must_use]
    /// Set the variable as semi-continuous
    pub const fn semi_continuous(self) -> Self {
        self.var_type(VariableType::SemiContinuous)
    }

    #[must_use]
    /// Set lower bound for the variable
    pub const fn lower_bound(self, bound: f64) -> Self {
        self.var_type(VariableType::LowerBound(bound))
    }

    #[must_use]
    /// Set upper bound for the variable
    pub const fn upper_bound(self, bound: f64) -> Self {
        self.var_type(VariableType::UpperBound(bound))
    }

    #[must_use]
    /// Set both lower and upper bounds
    pub const fn bounds(self, lower: f64, upper: f64) -> Self {
        self.var_type(VariableType::DoubleBound(lower, upper))
    }

    /// Build the variable
    pub const fn build(self) -> LpResult<Variable<'a>> {
        Ok(Variable { name: self.name, var_type: self.var_type })
    }
}

#[must_use]
/// Convenience function to create a new LP problem builder
pub fn lp_problem() -> LpProblemBuilder<'static> {
    LpProblemBuilder::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lp_problem_builder_default_construction() {
        let builder = LpProblemBuilder::new();
        assert!(builder.name.is_none());
        assert!(builder.sense.is_none());
        assert!(builder.objectives.is_empty());
        assert!(builder.constraints.is_empty());
        assert!(builder.variables.is_empty());
    }

    #[test]
    fn test_problem_builder_basic_building() {
        let problem = LpProblemBuilder::new().build().expect("Should build empty problem");

        assert_eq!(problem.name(), None);
        assert_eq!(problem.sense, Sense::default());
        assert!(problem.objectives.is_empty());
        assert!(problem.constraints.is_empty());
        assert!(problem.variables.is_empty());
    }

    #[test]
    fn test_problem_builder_with_name() {
        let problem = LpProblemBuilder::new().name("my_problem").build().expect("Should build successfully");

        assert_eq!(problem.name(), Some("my_problem"));
    }

    #[test]
    fn test_problem_builder_with_name_cow_owned() {
        let name = String::from("dynamic_problem");
        let problem = LpProblemBuilder::new().name(name.clone()).build().expect("Should build successfully");

        assert_eq!(problem.name(), Some(name.as_str()));
    }

    #[test]
    fn test_method_chaining_fluency() {
        let builder = LpProblemBuilder::new()
            .name("test_problem")
            .minimize()
            .objective("cost", |obj| obj.coefficient("x", 1.0))
            .constraint("capacity", |c| c.coefficient("x", 1.0).le(10.0))
            .variable("x", |v| v.general());

        let problem = builder.build().expect("Should build successfully");

        assert_eq!(problem.name(), Some("test_problem"));
        assert_eq!(problem.sense, Sense::Minimize);
        assert_eq!(problem.objectives.len(), 1);
        assert_eq!(problem.constraints.len(), 1);
        assert_eq!(problem.variables.len(), 1);
    }

    #[test]
    fn test_chaining_multiple_objectives() {
        let problem = LpProblemBuilder::new()
            .objective("obj1", |obj| obj.coefficient("x1", 1.0).coefficient("x2", 2.0))
            .objective("obj2", |obj| obj.coefficient("x3", 3.0))
            .objective("obj3", |obj| obj.coefficient("x1", 0.5).coefficient("x3", 1.5))
            .build()
            .expect("Should build successfully");

        assert_eq!(problem.objectives.len(), 3);
        assert!(problem.objectives.contains_key("obj1"));
        assert!(problem.objectives.contains_key("obj2"));
        assert!(problem.objectives.contains_key("obj3"));
    }

    #[test]
    fn test_chaining_multiple_constraints() {
        let problem = LpProblemBuilder::new()
            .constraint("c1", |c| c.coefficient("x", 1.0).le(10.0))
            .constraint("c2", |c| c.coefficient("y", 2.0).ge(5.0))
            .constraint("c3", |c| c.coefficient("x", 1.0).coefficient("y", 1.0).eq(7.0))
            .build()
            .expect("Should build successfully");

        assert_eq!(problem.constraints.len(), 3);
        assert!(problem.constraints.contains_key("c1"));
        assert!(problem.constraints.contains_key("c2"));
        assert!(problem.constraints.contains_key("c3"));
    }

    #[test]
    fn test_chaining_multiple_variables() {
        let problem = LpProblemBuilder::new()
            .variable("x", |v| v.binary())
            .variable("y", |v| v.integer())
            .variable("z", |v| v.free())
            .build()
            .expect("Should build successfully");

        assert_eq!(problem.variables.len(), 3);
        assert!(problem.variables.contains_key("x"));
        assert!(problem.variables.contains_key("y"));
        assert!(problem.variables.contains_key("z"));
    }

    #[test]
    fn test_sense_override_behavior() {
        let problem = LpProblemBuilder::new()
            .minimize()
            .maximize() // Should override the minimize
            .build()
            .expect("Should build successfully");

        assert_eq!(problem.sense, Sense::Maximize);
    }

    #[test]
    fn test_name_override_behavior() {
        let problem = LpProblemBuilder::new()
            .name("first_name")
            .name("second_name") // Should override the first name
            .build()
            .expect("Should build successfully");

        assert_eq!(problem.name(), Some("second_name"));
    }

    #[test]
    fn test_objective_override_behavior() {
        let problem = LpProblemBuilder::new()
            .objective("cost", |obj| obj.coefficient("x", 1.0))
            .objective("cost", |obj| obj.coefficient("y", 2.0)) // Same name should override
            .build()
            .expect("Should build successfully");

        assert_eq!(problem.objectives.len(), 1);
        let objective = problem.objectives.get("cost").unwrap();
        // Should have the second version (coefficient for y)
        assert_eq!(objective.coefficients.len(), 1);
        assert_eq!(objective.coefficients[0].name, "y");
        assert_eq!(objective.coefficients[0].value, 2.0);
    }

    #[test]
    fn test_constraint_override_behavior() {
        let problem = LpProblemBuilder::new()
            .constraint("capacity", |c| c.coefficient("x", 1.0).le(10.0))
            .constraint("capacity", |c| c.coefficient("y", 2.0).ge(5.0)) // Same name should override
            .build()
            .expect("Should build successfully");

        assert_eq!(problem.constraints.len(), 1);
        let constraint = problem.constraints.get("capacity").unwrap();
        if let Constraint::Standard { coefficients, operator, rhs, .. } = constraint {
            assert_eq!(coefficients.len(), 1);
            assert_eq!(coefficients[0].name, "y");
            assert_eq!(coefficients[0].value, 2.0);
            assert_eq!(*operator, ComparisonOp::GTE);
            assert_eq!(*rhs, 5.0);
        } else {
            panic!("Expected standard constraint");
        }
    }

    #[test]
    fn test_variable_override_behavior() {
        let problem = LpProblemBuilder::new()
            .variable("x", |v| v.binary())
            .variable("x", |v| v.integer()) // Same name should override
            .build()
            .expect("Should build successfully");

        assert_eq!(problem.variables.len(), 1);
        let variable = problem.variables.get("x").unwrap();
        assert_eq!(variable.var_type, VariableType::Integer);
    }

    #[test]
    fn test_minimize_sense() {
        let problem = LpProblemBuilder::new().minimize().build().expect("Should build successfully");

        assert_eq!(problem.sense, Sense::Minimize);
    }

    #[test]
    fn test_maximize_sense() {
        let problem = LpProblemBuilder::new().maximize().build().expect("Should build successfully");

        assert_eq!(problem.sense, Sense::Maximize);
    }

    #[test]
    fn test_explicit_sense() {
        let problem = LpProblemBuilder::new().sense(Sense::Minimize).build().expect("Should build successfully");

        assert_eq!(problem.sense, Sense::Minimize);
    }

    #[test]
    fn test_objective_builder_single_coefficient() {
        let objective = ObjectiveBuilder::new("test".into()).coefficient("x", 5.0).build().expect("Should build successfully");

        assert_eq!(objective.name, "test");
        assert_eq!(objective.coefficients.len(), 1);
        assert_eq!(objective.coefficients[0].name, "x");
        assert_eq!(objective.coefficients[0].value, 5.0);
    }

    #[test]
    fn test_objective_builder_multiple_coefficients() {
        let objective = ObjectiveBuilder::new("multi".into())
            .coefficient("x", 1.0)
            .coefficient("y", 2.0)
            .coefficient("z", -3.0)
            .build()
            .expect("Should build successfully");

        assert_eq!(objective.coefficients.len(), 3);
        assert_eq!(objective.coefficients[0].value, 1.0);
        assert_eq!(objective.coefficients[1].value, 2.0);
        assert_eq!(objective.coefficients[2].value, -3.0);
    }

    #[test]
    fn test_objective_builder_empty_fails() {
        let result = ObjectiveBuilder::new("empty".into()).build();

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("no coefficients"));
    }

    #[test]
    fn test_constraint_builder_standard_le() {
        let constraint = ConstraintBuilder::standard("test".into())
            .coefficient("x", 2.0)
            .coefficient("y", 3.0)
            .le(10.0)
            .build()
            .expect("Should build successfully");

        if let Constraint::Standard { name, coefficients, operator, rhs } = constraint {
            assert_eq!(name, "test");
            assert_eq!(coefficients.len(), 2);
            assert_eq!(operator, ComparisonOp::LTE);
            assert_eq!(rhs, 10.0);
        } else {
            panic!("Expected standard constraint");
        }
    }

    #[test]
    fn test_constraint_builder_standard_ge() {
        let constraint =
            ConstraintBuilder::standard("test".into()).coefficient("x", 1.0).ge(5.0).build().expect("Should build successfully");

        if let Constraint::Standard { operator, rhs, .. } = constraint {
            assert_eq!(operator, ComparisonOp::GTE);
            assert_eq!(rhs, 5.0);
        }
    }

    #[test]
    fn test_constraint_builder_standard_eq() {
        let constraint =
            ConstraintBuilder::standard("test".into()).coefficient("x", 1.0).eq(7.0).build().expect("Should build successfully");

        if let Constraint::Standard { operator, rhs, .. } = constraint {
            assert_eq!(operator, ComparisonOp::EQ);
            assert_eq!(rhs, 7.0);
        }
    }

    #[test]
    fn test_constraint_builder_standard_lt() {
        let constraint =
            ConstraintBuilder::standard("test".into()).coefficient("x", 1.0).lt(15.0).build().expect("Should build successfully");

        if let Constraint::Standard { operator, rhs, .. } = constraint {
            assert_eq!(operator, ComparisonOp::LT);
            assert_eq!(rhs, 15.0);
        }
    }

    #[test]
    fn test_constraint_builder_standard_gt() {
        let constraint =
            ConstraintBuilder::standard("test".into()).coefficient("x", 1.0).gt(2.0).build().expect("Should build successfully");

        if let Constraint::Standard { operator, rhs, .. } = constraint {
            assert_eq!(operator, ComparisonOp::GT);
            assert_eq!(rhs, 2.0);
        }
    }

    #[test]
    fn test_constraint_builder_sos() {
        let constraint = ConstraintBuilder::sos("sos_test".into(), SOSType::S1)
            .coefficient("x1", 1.0)
            .coefficient("x2", 2.0)
            .coefficient("x3", 3.0)
            .build()
            .expect("Should build successfully");

        if let Constraint::SOS { name, sos_type, weights } = constraint {
            assert_eq!(name, "sos_test");
            assert_eq!(sos_type, SOSType::S1);
            assert_eq!(weights.len(), 3);
        } else {
            panic!("Expected SOS constraint");
        }
    }

    #[test]
    fn test_constraint_builder_standard_missing_operator() {
        let result = ConstraintBuilder::standard("incomplete".into()).coefficient("x", 1.0).build();

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("missing an operator"));
    }

    #[test]
    fn test_constraint_builder_standard_no_coefficients() {
        let result = ConstraintBuilder::standard("empty".into()).le(10.0).build();

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("no coefficients"));
    }

    #[test]
    fn test_constraint_builder_sos_no_weights() {
        let result = ConstraintBuilder::sos("empty_sos".into(), SOSType::S2).build();

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("No weights specified"));
    }

    #[test]
    fn test_variable_builder_default() {
        let variable = VariableBuilder::new("x").build().expect("Should build successfully");

        assert_eq!(variable.name, "x");
        assert_eq!(variable.var_type, VariableType::default());
    }

    #[test]
    fn test_variable_builder_binary() {
        let variable = VariableBuilder::new("x").binary().build().expect("Should build successfully");

        assert_eq!(variable.var_type, VariableType::Binary);
    }

    #[test]
    fn test_variable_builder_integer() {
        let variable = VariableBuilder::new("x").integer().build().expect("Should build successfully");

        assert_eq!(variable.var_type, VariableType::Integer);
    }

    #[test]
    fn test_variable_builder_general() {
        let variable = VariableBuilder::new("x").general().build().expect("Should build successfully");

        assert_eq!(variable.var_type, VariableType::General);
    }

    #[test]
    fn test_variable_builder_free() {
        let variable = VariableBuilder::new("x").free().build().expect("Should build successfully");

        assert_eq!(variable.var_type, VariableType::Free);
    }

    #[test]
    fn test_variable_builder_semi_continuous() {
        let variable = VariableBuilder::new("x").semi_continuous().build().expect("Should build successfully");

        assert_eq!(variable.var_type, VariableType::SemiContinuous);
    }

    #[test]
    fn test_variable_builder_lower_bound() {
        let variable = VariableBuilder::new("x").lower_bound(5.0).build().expect("Should build successfully");

        assert_eq!(variable.var_type, VariableType::LowerBound(5.0));
    }

    #[test]
    fn test_variable_builder_upper_bound() {
        let variable = VariableBuilder::new("x").upper_bound(10.0).build().expect("Should build successfully");

        assert_eq!(variable.var_type, VariableType::UpperBound(10.0));
    }

    #[test]
    fn test_variable_builder_double_bound() {
        let variable = VariableBuilder::new("x").bounds(0.0, 100.0).build().expect("Should build successfully");

        assert_eq!(variable.var_type, VariableType::DoubleBound(0.0, 100.0));
    }

    #[test]
    fn test_variable_builder_type_override() {
        let variable = VariableBuilder::new("x")
            .binary()
            .integer() // Should override binary
            .build()
            .expect("Should build successfully");

        assert_eq!(variable.var_type, VariableType::Integer);
    }

    #[test]
    fn test_variables_bulk_creation() {
        let problem =
            LpProblemBuilder::new().variables(&["x1", "x2", "x3", "x4"], &VariableType::Binary).build().expect("Should build successfully");

        assert_eq!(problem.variables.len(), 4);
        for var_name in &["x1", "x2", "x3", "x4"] {
            let variable = problem.variables.get(var_name).unwrap();
            assert_eq!(variable.var_type, VariableType::Binary);
        }
    }

    #[test]
    fn test_variables_empty_array() {
        let problem = LpProblemBuilder::new().variables(&[], &VariableType::General).build().expect("Should build successfully");

        assert!(problem.variables.is_empty());
    }

    #[test]
    fn test_sos_constraint_s1() {
        let problem = LpProblemBuilder::new()
            .sos_constraint("sos1", SOSType::S1, |c| c.coefficient("x1", 1.0).coefficient("x2", 2.0).coefficient("x3", 3.0))
            .build()
            .expect("Should build successfully");

        assert_eq!(problem.constraints.len(), 1);
        let constraint = problem.constraints.get("sos1").unwrap();
        if let Constraint::SOS { sos_type, weights, .. } = constraint {
            assert_eq!(*sos_type, SOSType::S1);
            assert_eq!(weights.len(), 3);
        }
    }

    #[test]
    fn test_sos_constraint_s2() {
        let problem = LpProblemBuilder::new()
            .sos_constraint("sos2", SOSType::S2, |c| c.coefficient("x1", 1.0).coefficient("x2", 2.0))
            .build()
            .expect("Should build successfully");

        let constraint = problem.constraints.get("sos2").unwrap();
        if let Constraint::SOS { sos_type, .. } = constraint {
            assert_eq!(*sos_type, SOSType::S2);
        }
    }

    #[test]
    fn test_complex_problem_construction() {
        let problem = LpProblemBuilder::new()
            .name("complex_problem")
            .maximize()
            .objective("profit", |obj| obj.coefficient("x1", 10.0).coefficient("x2", 15.0).coefficient("x3", 8.0))
            .constraint("material", |c| c.coefficient("x1", 2.0).coefficient("x2", 3.0).coefficient("x3", 1.0).le(100.0))
            .constraint("labor", |c| c.coefficient("x1", 1.0).coefficient("x2", 2.0).coefficient("x3", 2.0).le(80.0))
            .constraint("demand", |c| c.coefficient("x1", 1.0).ge(10.0))
            .variable("x1", |v| v.bounds(0.0, f64::INFINITY))
            .variable("x2", |v| v.bounds(0.0, 50.0))
            .variable("x3", |v| v.binary())
            .build()
            .expect("Should build successfully");

        assert_eq!(problem.name(), Some("complex_problem"));
        assert_eq!(problem.sense, Sense::Maximize);
        assert_eq!(problem.objectives.len(), 1);
        assert_eq!(problem.constraints.len(), 3);
        assert_eq!(problem.variables.len(), 3);

        // Check objective
        let profit_obj = problem.objectives.get("profit").unwrap();
        assert_eq!(profit_obj.coefficients.len(), 3);

        // Check constraints
        assert!(problem.constraints.contains_key("material"));
        assert!(problem.constraints.contains_key("labor"));
        assert!(problem.constraints.contains_key("demand"));

        // Check variables
        let x1_var = problem.variables.get("x1").unwrap();
        let x2_var = problem.variables.get("x2").unwrap();
        let x3_var = problem.variables.get("x3").unwrap();

        if let VariableType::DoubleBound(lower, upper) = x1_var.var_type {
            assert_eq!(lower, 0.0);
            assert_eq!(upper, f64::INFINITY);
        }

        if let VariableType::DoubleBound(lower, upper) = x2_var.var_type {
            assert_eq!(lower, 0.0);
            assert_eq!(upper, 50.0);
        }

        assert_eq!(x3_var.var_type, VariableType::Binary);
    }

    #[test]
    fn test_lp_problem_convenience_function() {
        let problem = lp_problem()
            .name("convenience_test")
            .minimize()
            .objective("cost", |obj| obj.coefficient("x", 1.0))
            .build()
            .expect("Should build successfully");

        assert_eq!(problem.name(), Some("convenience_test"));
        assert_eq!(problem.sense, Sense::Minimize);
    }

    #[test]
    fn test_zero_coefficient_values() {
        let problem = LpProblemBuilder::new()
            .objective("test", |obj| obj.coefficient("x", 0.0).coefficient("y", -0.0))
            .constraint("test", |c| c.coefficient("x", 0.0).coefficient("y", 1.0).le(10.0))
            .build()
            .expect("Should build successfully");

        let objective = problem.objectives.get("test").unwrap();
        assert_eq!(objective.coefficients[0].value, 0.0);
        assert_eq!(objective.coefficients[1].value, -0.0);
    }

    #[test]
    fn test_extreme_coefficient_values() {
        let problem = LpProblemBuilder::new()
            .objective("extreme", |obj| {
                obj.coefficient("x1", f64::MAX)
                    .coefficient("x2", f64::MIN)
                    .coefficient("x3", f64::INFINITY)
                    .coefficient("x4", f64::NEG_INFINITY)
                    .coefficient("x5", f64::EPSILON)
            })
            .build()
            .expect("Should build successfully");

        let objective = problem.objectives.get("extreme").unwrap();
        assert_eq!(objective.coefficients.len(), 5);
        assert_eq!(objective.coefficients[0].value, f64::MAX);
        assert_eq!(objective.coefficients[1].value, f64::MIN);
        assert_eq!(objective.coefficients[2].value, f64::INFINITY);
        assert_eq!(objective.coefficients[3].value, f64::NEG_INFINITY);
        assert_eq!(objective.coefficients[4].value, f64::EPSILON);
    }

    #[test]
    fn test_constraint_operators_on_sos_no_effect() {
        // Test that calling constraint operators on SOS constraints has no effect
        let constraint = ConstraintBuilder::sos("sos".into(), SOSType::S1)
            .coefficient("x", 1.0)
            .le(10.0) // Should have no effect on SOS constraint
            .ge(5.0) // Should have no effect on SOS constraint
            .build()
            .expect("Should build successfully");

        if let Constraint::SOS { weights, .. } = constraint {
            assert_eq!(weights.len(), 1);
        } else {
            panic!("Expected SOS constraint");
        }
    }

    #[test]
    fn test_variable_name_with_special_characters() {
        let problem = LpProblemBuilder::new()
            .variable("var_1.2.3", |v| v.general())
            .variable("x$special", |v| v.binary())
            .build()
            .expect("Should build successfully");

        assert!(problem.variables.contains_key("var_1.2.3"));
        assert!(problem.variables.contains_key("x$special"));
    }

    #[test]
    fn test_very_long_names() {
        let long_name = "x".repeat(1000);
        let long_obj_name = "obj".repeat(500);
        let long_cons_name = "cons".repeat(500);

        let problem = LpProblemBuilder::new()
            .name(long_name.clone())
            .objective(&long_obj_name, |obj| obj.coefficient(&long_name, 1.0))
            .constraint(&long_cons_name, |c| c.coefficient(&long_name, 1.0).le(10.0))
            .variable(&long_name, |v| v.general())
            .build()
            .expect("Should build successfully");

        assert_eq!(problem.name(), Some(long_name.as_str()));
        assert!(problem.objectives.contains_key(long_obj_name.as_str()));
        assert!(problem.constraints.contains_key(long_cons_name.as_str()));
        assert!(problem.variables.contains_key(long_name.as_str()));
    }

    #[test]
    fn test_simple_problem_builder() {
        let problem = LpProblemBuilder::new()
            .name("test_problem")
            .minimize()
            .objective("cost", |obj| obj.coefficient("x", 1.0).coefficient("y", 2.0))
            .constraint("capacity", |c| c.coefficient("x", 1.0).coefficient("y", 1.0).le(10.0))
            .variable("x", |v| v.binary())
            .variable("y", |v| v.general())
            .build()
            .expect("Should build successfully");

        assert_eq!(problem.name(), Some("test_problem"));
        assert_eq!(problem.sense, Sense::Minimize);
        assert_eq!(problem.objectives.len(), 1);
        assert_eq!(problem.constraints.len(), 1);
        assert_eq!(problem.variables.len(), 2);
    }

    #[test]
    fn test_sos_constraint_builder() {
        let problem = LpProblemBuilder::new()
            .minimize()
            .sos_constraint("sos1", SOSType::S1, |c| c.coefficient("x1", 1.0).coefficient("x2", 2.0).coefficient("x3", 3.0))
            .variables(&["x1", "x2", "x3"], &VariableType::Free)
            .build()
            .expect("Should build successfully");

        assert_eq!(problem.constraints.len(), 1);
        assert_eq!(problem.variables.len(), 3);
    }

    #[test]
    fn test_convenience_function() {
        let problem = lp_problem()
            .name("simple")
            .maximize()
            .objective("profit", |obj| obj.coefficient("x", 3.0).coefficient("y", 2.0))
            .constraint("resource", |c| c.coefficient("x", 2.0).coefficient("y", 1.0).le(100.0))
            .variable("x", |v| v.bounds(0.0, 50.0))
            .variable("y", |v| v.lower_bound(0.0))
            .build()
            .expect("Should build successfully");

        assert_eq!(problem.sense, Sense::Maximize);
        assert_eq!(problem.objectives.len(), 1);
        assert_eq!(problem.constraints.len(), 1);
    }

    #[test]
    fn test_validation_errors() {
        // Test objective with no coefficients
        let result = LpProblemBuilder::new()
            .objective("empty", |obj| obj) // No coefficients
            .build();

        assert!(result.is_err());

        // Test constraint with no operator
        let result = LpProblemBuilder::new()
            .constraint("incomplete", |c| c.coefficient("x", 1.0)) // No operator or RHS
            .build();

        assert!(result.is_err());
    }
}
