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
    ///
    /// # Errors
    ///
    /// Returns an error if any objective or constraint builder fails validation
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
    ///
    /// # Errors
    ///
    /// Returns an error if the objective has no coefficients
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
    ///
    /// # Errors
    ///
    /// Returns an error if the constraint is missing an operator, RHS, or has no coefficients
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
    ///
    /// # Errors
    ///
    /// This method currently never fails, but returns `Result` for API consistency
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
    fn test_problem_builder_defaults_and_naming() {
        // Default construction
        let builder = LpProblemBuilder::new();
        assert!(builder.name.is_none() && builder.sense.is_none() && builder.objectives.is_empty());

        // Empty build
        let problem = LpProblemBuilder::new().build().unwrap();
        assert_eq!(problem.name(), None);
        assert_eq!(problem.sense, Sense::default());

        // With name (borrowed and owned)
        assert_eq!(LpProblemBuilder::new().name("test").build().unwrap().name(), Some("test"));
        assert_eq!(LpProblemBuilder::new().name(String::from("owned")).build().unwrap().name(), Some("owned"));
    }

    #[test]
    fn test_sense_methods() {
        assert_eq!(LpProblemBuilder::new().minimize().build().unwrap().sense, Sense::Minimize);
        assert_eq!(LpProblemBuilder::new().maximize().build().unwrap().sense, Sense::Maximize);
        assert_eq!(LpProblemBuilder::new().sense(Sense::Minimize).build().unwrap().sense, Sense::Minimize);
        // Override behavior
        assert_eq!(LpProblemBuilder::new().minimize().maximize().build().unwrap().sense, Sense::Maximize);
    }

    #[test]
    fn test_chaining_multiple_elements() {
        let problem = LpProblemBuilder::new()
            .objective("obj1", |o| o.coefficient("x1", 1.0))
            .objective("obj2", |o| o.coefficient("x2", 2.0))
            .constraint("c1", |c| c.coefficient("x1", 1.0).le(10.0))
            .constraint("c2", |c| c.coefficient("x2", 2.0).ge(5.0))
            .variable("x1", super::VariableBuilder::binary)
            .variable("x2", super::VariableBuilder::integer)
            .build()
            .unwrap();

        assert_eq!(problem.objectives.len(), 2);
        assert_eq!(problem.constraints.len(), 2);
        assert_eq!(problem.variables.len(), 2);
    }

    #[test]
    fn test_override_behavior() {
        // Name, objective, constraint, variable overrides
        let problem = LpProblemBuilder::new()
            .name("first")
            .name("second")
            .objective("obj", |o| o.coefficient("x", 1.0))
            .objective("obj", |o| o.coefficient("y", 2.0))
            .constraint("c", |c| c.coefficient("a", 1.0).le(10.0))
            .constraint("c", |c| c.coefficient("b", 2.0).ge(5.0))
            .variable("v", super::VariableBuilder::binary)
            .variable("v", super::VariableBuilder::integer)
            .build()
            .unwrap();

        assert_eq!(problem.name(), Some("second"));
        assert_eq!(problem.objectives.len(), 1);
        assert_eq!(problem.objectives.get("obj").unwrap().coefficients[0].name, "y");
        assert_eq!(problem.constraints.len(), 1);
        assert_eq!(problem.variables.get("v").unwrap().var_type, VariableType::Integer);
    }

    #[test]
    fn test_objective_builder() {
        // Single and multiple coefficients
        let obj = ObjectiveBuilder::new("test".into()).coefficient("x", 5.0).build().unwrap();
        assert_eq!(obj.name, "test");
        assert_eq!(obj.coefficients.len(), 1);

        let obj2 = ObjectiveBuilder::new("m".into()).coefficient("x", 1.0).coefficient("y", 2.0).coefficient("z", -3.0).build().unwrap();
        assert_eq!(obj2.coefficients.len(), 3);

        // Empty fails
        assert!(ObjectiveBuilder::new("e".into()).build().unwrap_err().to_string().contains("no coefficients"));
    }

    #[test]
    fn test_constraint_builder_operators() {
        // Test all operators
        let c = ConstraintBuilder::standard("t".into()).coefficient("x", 1.0).le(10.0).build().unwrap();
        if let Constraint::Standard { operator, rhs, .. } = c {
            assert_eq!((operator, rhs), (ComparisonOp::LTE, 10.0));
        }

        let c = ConstraintBuilder::standard("t".into()).coefficient("x", 1.0).ge(5.0).build().unwrap();
        if let Constraint::Standard { operator, rhs, .. } = c {
            assert_eq!((operator, rhs), (ComparisonOp::GTE, 5.0));
        }

        let c = ConstraintBuilder::standard("t".into()).coefficient("x", 1.0).eq(7.0).build().unwrap();
        if let Constraint::Standard { operator, rhs, .. } = c {
            assert_eq!((operator, rhs), (ComparisonOp::EQ, 7.0));
        }

        let c = ConstraintBuilder::standard("t".into()).coefficient("x", 1.0).lt(15.0).build().unwrap();
        if let Constraint::Standard { operator, rhs, .. } = c {
            assert_eq!((operator, rhs), (ComparisonOp::LT, 15.0));
        }

        let c = ConstraintBuilder::standard("t".into()).coefficient("x", 1.0).gt(2.0).build().unwrap();
        if let Constraint::Standard { operator, rhs, .. } = c {
            assert_eq!((operator, rhs), (ComparisonOp::GT, 2.0));
        }

        // Missing operator and no coefficients errors
        assert!(ConstraintBuilder::standard("i".into()).coefficient("x", 1.0).build().is_err());
        assert!(ConstraintBuilder::standard("e".into()).le(10.0).build().is_err());
    }

    #[test]
    fn test_constraint_builder_sos() {
        let constraint = ConstraintBuilder::sos("sos".into(), SOSType::S1).coefficient("x1", 1.0).coefficient("x2", 2.0).build().unwrap();

        if let Constraint::SOS { name, sos_type, weights } = constraint {
            assert_eq!(name, "sos");
            assert_eq!(sos_type, SOSType::S1);
            assert_eq!(weights.len(), 2);
        } else {
            panic!("Expected SOS");
        }

        // Empty SOS fails
        assert!(ConstraintBuilder::sos("e".into(), SOSType::S2).build().is_err());

        // Operators on SOS have no effect
        let sos = ConstraintBuilder::sos("s".into(), SOSType::S1).coefficient("x", 1.0).le(10.0).ge(5.0).build().unwrap();
        assert!(matches!(sos, Constraint::SOS { .. }));
    }

    #[test]
    fn test_variable_builder_types() {
        // Test all variable types
        assert_eq!(VariableBuilder::new("x").build().unwrap().var_type, VariableType::Free);
        assert_eq!(VariableBuilder::new("x").binary().build().unwrap().var_type, VariableType::Binary);
        assert_eq!(VariableBuilder::new("x").integer().build().unwrap().var_type, VariableType::Integer);
        assert_eq!(VariableBuilder::new("x").general().build().unwrap().var_type, VariableType::General);
        assert_eq!(VariableBuilder::new("x").free().build().unwrap().var_type, VariableType::Free);
        assert_eq!(VariableBuilder::new("x").semi_continuous().build().unwrap().var_type, VariableType::SemiContinuous);
        assert_eq!(VariableBuilder::new("x").lower_bound(5.0).build().unwrap().var_type, VariableType::LowerBound(5.0));
        assert_eq!(VariableBuilder::new("x").upper_bound(10.0).build().unwrap().var_type, VariableType::UpperBound(10.0));
        assert_eq!(VariableBuilder::new("x").bounds(0.0, 100.0).build().unwrap().var_type, VariableType::DoubleBound(0.0, 100.0));

        // Override behavior
        assert_eq!(VariableBuilder::new("x").binary().integer().build().unwrap().var_type, VariableType::Integer);
    }

    #[test]
    fn test_bulk_variables() {
        let problem = LpProblemBuilder::new().variables(&["x1", "x2", "x3"], &VariableType::Binary).build().unwrap();
        assert_eq!(problem.variables.len(), 3);
        assert!(problem.variables.values().all(|v| v.var_type == VariableType::Binary));

        // Empty array
        assert!(LpProblemBuilder::new().variables(&[], &VariableType::General).build().unwrap().variables.is_empty());
    }

    #[test]
    fn test_sos_constraint_on_problem() {
        for sos_type in [SOSType::S1, SOSType::S2] {
            let problem = LpProblemBuilder::new().sos_constraint("sos", sos_type.clone(), |c| c.coefficient("x", 1.0)).build().unwrap();

            if let Constraint::SOS { sos_type: st, .. } = problem.constraints.get("sos").unwrap() {
                assert_eq!(*st, sos_type);
            }
        }
    }

    #[test]
    fn test_complex_problem_and_convenience() {
        let problem = lp_problem()
            .name("complex")
            .maximize()
            .objective("profit", |o| o.coefficient("x1", 10.0).coefficient("x2", 15.0))
            .constraint("cap", |c| c.coefficient("x1", 2.0).coefficient("x2", 3.0).le(100.0))
            .variable("x1", |v| v.bounds(0.0, f64::INFINITY))
            .variable("x2", super::VariableBuilder::binary)
            .build()
            .unwrap();

        assert_eq!(problem.name(), Some("complex"));
        assert_eq!(problem.sense, Sense::Maximize);
        assert_eq!(problem.objectives.len(), 1);
        assert_eq!(problem.constraints.len(), 1);
        assert_eq!(problem.variables.len(), 2);
    }

    #[test]
    fn test_edge_cases() {
        // Zero and extreme coefficients
        let problem = LpProblemBuilder::new()
            .objective("ext", |o| o.coefficient("a", 0.0).coefficient("b", f64::MAX).coefficient("c", f64::NEG_INFINITY))
            .build()
            .unwrap();
        assert_eq!(problem.objectives.get("ext").unwrap().coefficients.len(), 3);

        // Special characters and long names
        let long = "x".repeat(1000);
        let problem = LpProblemBuilder::new()
            .name(&long)
            .variable("var_1.2$special", super::VariableBuilder::general)
            .variable(&long, super::VariableBuilder::binary)
            .build()
            .unwrap();
        assert!(problem.variables.contains_key("var_1.2$special"));
        assert!(problem.variables.contains_key(long.as_str()));
    }

    #[test]
    fn test_validation_errors() {
        assert!(LpProblemBuilder::new().objective("e", |o| o).build().is_err());
        assert!(LpProblemBuilder::new().constraint("i", |c| c.coefficient("x", 1.0)).build().is_err());
    }
}
