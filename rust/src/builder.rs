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
    /// Create a new LP problem builder
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the problem name
    pub fn name(mut self, name: impl Into<Cow<'a, str>>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// Set the optimization sense
    pub fn sense(mut self, sense: Sense) -> Self {
        self.sense = Some(sense);
        self
    }

    /// Set the problem to minimize
    pub fn minimize(self) -> Self {
        self.sense(Sense::Minimize)
    }

    /// Set the problem to maximize
    pub fn maximize(self) -> Self {
        self.sense(Sense::Maximize)
    }

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

    /// Add multiple variables with the same type
    pub fn variables(mut self, names: &[&'a str], var_type: VariableType) -> Self {
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
    /// Create a new objective builder
    pub fn new(name: Cow<'a, str>) -> Self {
        Self { name, coefficients: Vec::new() }
    }

    /// Add a coefficient to the objective
    pub fn coefficient(mut self, name: &'a str, value: f64) -> Self {
        self.coefficients.push(Coefficient { name, value });
        self
    }

    /// Add multiple coefficients at once
    pub fn coefficients(mut self, coeffs: &[(impl AsRef<str>, f64)]) -> Self
    where
        'a: 'static, // This is a simplification for the example
    {
        for (name, value) in coeffs {
            // Note: This is a simplified implementation that assumes static strings
            // In a real implementation, you'd need to handle lifetimes more carefully
            let name_str = unsafe { std::mem::transmute::<&str, &'a str>(name.as_ref()) };
            self.coefficients.push(Coefficient { name: name_str, value: *value });
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
    /// Create a new standard constraint builder
    pub fn standard(name: Cow<'a, str>) -> Self {
        Self::Standard { name, coefficients: Vec::new(), operator: None, rhs: None }
    }

    /// Create a new SOS constraint builder
    pub fn sos(name: Cow<'a, str>, sos_type: SOSType) -> Self {
        Self::SOS { name, sos_type, weights: Vec::new() }
    }

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

    /// Set the constraint to less than or equal
    pub fn le(mut self, rhs: f64) -> Self {
        if let Self::Standard { operator, rhs: rhs_ref, .. } = &mut self {
            *operator = Some(ComparisonOp::LTE);
            *rhs_ref = Some(rhs);
        }
        self
    }

    /// Set the constraint to less than
    pub fn lt(mut self, rhs: f64) -> Self {
        if let Self::Standard { operator, rhs: rhs_ref, .. } = &mut self {
            *operator = Some(ComparisonOp::LT);
            *rhs_ref = Some(rhs);
        }
        self
    }

    /// Set the constraint to greater than or equal
    pub fn ge(mut self, rhs: f64) -> Self {
        if let Self::Standard { operator, rhs: rhs_ref, .. } = &mut self {
            *operator = Some(ComparisonOp::GTE);
            *rhs_ref = Some(rhs);
        }
        self
    }

    /// Set the constraint to greater than
    pub fn gt(mut self, rhs: f64) -> Self {
        if let Self::Standard { operator, rhs: rhs_ref, .. } = &mut self {
            *operator = Some(ComparisonOp::GT);
            *rhs_ref = Some(rhs);
        }
        self
    }

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
    /// Create a new variable builder
    pub fn new(name: &'a str) -> Self {
        Self { name, var_type: VariableType::default() }
    }

    /// Set the variable type
    pub fn var_type(mut self, var_type: VariableType) -> Self {
        self.var_type = var_type;
        self
    }

    /// Set the variable as binary
    pub fn binary(self) -> Self {
        self.var_type(VariableType::Binary)
    }

    /// Set the variable as integer
    pub fn integer(self) -> Self {
        self.var_type(VariableType::Integer)
    }

    /// Set the variable as general (non-negative)
    pub fn general(self) -> Self {
        self.var_type(VariableType::General)
    }

    /// Set the variable as free (unbounded)
    pub fn free(self) -> Self {
        self.var_type(VariableType::Free)
    }

    /// Set the variable as semi-continuous
    pub fn semi_continuous(self) -> Self {
        self.var_type(VariableType::SemiContinuous)
    }

    /// Set lower bound for the variable
    pub fn lower_bound(self, bound: f64) -> Self {
        self.var_type(VariableType::LowerBound(bound))
    }

    /// Set upper bound for the variable
    pub fn upper_bound(self, bound: f64) -> Self {
        self.var_type(VariableType::UpperBound(bound))
    }

    /// Set both lower and upper bounds
    pub fn bounds(self, lower: f64, upper: f64) -> Self {
        self.var_type(VariableType::DoubleBound(lower, upper))
    }

    /// Build the variable
    pub fn build(self) -> LpResult<Variable<'a>> {
        Ok(Variable { name: self.name, var_type: self.var_type })
    }
}

/// Convenience function to create a new LP problem builder
pub fn lp_problem() -> LpProblemBuilder<'static> {
    LpProblemBuilder::new()
}

#[cfg(test)]
mod tests {
    use super::*;

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
            .variables(&["x1", "x2", "x3"], VariableType::Free)
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
