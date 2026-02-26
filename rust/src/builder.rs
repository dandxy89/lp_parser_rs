use std::collections::HashMap;

use crate::error::{LpParseError, LpResult};
use crate::model::{Coefficient, ComparisonOp, Constraint, Objective, SOSType, Sense, Variable, VariableType};
use crate::problem::LpProblem;

/// Builder for constructing LP problems with a fluent API
#[derive(Debug, Default)]
pub struct LpProblemBuilder {
    name: Option<String>,
    sense: Option<Sense>,
    objectives: HashMap<String, ObjectiveBuilder>,
    constraints: HashMap<String, ConstraintBuilder>,
    variables: HashMap<String, VariableBuilder>,
}

/// Builder for objectives
#[derive(Debug, Clone)]
pub struct ObjectiveBuilder {
    name: String,
    coefficients: Vec<(String, f64)>,
}

/// Builder for constraints
#[derive(Debug, Clone)]
pub enum ConstraintBuilder {
    /// Standard linear constraint builder
    Standard { name: String, coefficients: Vec<(String, f64)>, operator: Option<ComparisonOp>, rhs: Option<f64> },
    /// SOS constraint builder
    SOS { name: String, sos_type: SOSType, weights: Vec<(String, f64)> },
}

/// Builder for variables
#[derive(Debug, Clone)]
pub struct VariableBuilder {
    name: String,
    var_type: VariableType,
}

impl LpProblemBuilder {
    #[must_use]
    /// Create a new LP problem builder
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    /// Set the problem name
    pub fn name(mut self, name: impl Into<String>) -> Self {
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
    pub fn objective<F>(mut self, name: impl Into<String>, f: F) -> Self
    where
        F: FnOnce(ObjectiveBuilder) -> ObjectiveBuilder,
    {
        let name = name.into();
        let builder = ObjectiveBuilder::new(name.clone());
        let completed_builder = f(builder);
        self.objectives.insert(name, completed_builder);
        self
    }

    #[must_use]
    /// Add a constraint to the problem
    pub fn constraint<F>(mut self, name: impl Into<String>, f: F) -> Self
    where
        F: FnOnce(ConstraintBuilder) -> ConstraintBuilder,
    {
        let name = name.into();
        let builder = ConstraintBuilder::standard(name.clone());
        let completed_builder = f(builder);
        self.constraints.insert(name, completed_builder);
        self
    }

    #[must_use]
    /// Add an SOS constraint to the problem
    pub fn sos_constraint<F>(mut self, name: impl Into<String>, sos_type: SOSType, f: F) -> Self
    where
        F: FnOnce(ConstraintBuilder) -> ConstraintBuilder,
    {
        let name = name.into();
        let builder = ConstraintBuilder::sos(name.clone(), sos_type);
        let completed_builder = f(builder);
        self.constraints.insert(name, completed_builder);
        self
    }

    #[must_use]
    /// Add a variable to the problem
    pub fn variable<F>(mut self, name: impl Into<String>, f: F) -> Self
    where
        F: FnOnce(VariableBuilder) -> VariableBuilder,
    {
        let name = name.into();
        let builder = VariableBuilder::new(name.clone());
        let completed_builder = f(builder);
        self.variables.insert(name, completed_builder);
        self
    }

    #[must_use]
    /// Add multiple variables with the same type
    pub fn variables(mut self, names: &[&str], var_type: &VariableType) -> Self {
        for &name in names {
            let builder = VariableBuilder::new(name.to_string()).var_type(var_type.clone());
            self.variables.insert(name.to_string(), builder);
        }
        self
    }

    /// Build the LP problem
    ///
    /// # Errors
    ///
    /// Returns an error if any objective or constraint builder fails validation
    pub fn build(self) -> LpResult<LpProblem> {
        let mut problem = LpProblem::new();

        // Set name and sense
        if let Some(name) = self.name {
            problem = problem.with_problem_name(name);
        }
        problem = problem.with_sense(self.sense.unwrap_or_default());

        // Add variables first (intern names)
        for (_, var_builder) in self.variables {
            let name_id = problem.intern(&var_builder.name);
            let variable = Variable::new(name_id).with_var_type(var_builder.var_type);
            problem.add_variable(variable);
        }

        // Add objectives (intern names)
        for (_, obj_builder) in self.objectives {
            let name_id = problem.intern(&obj_builder.name);
            let coefficients: Vec<Coefficient> = obj_builder
                .coefficients
                .iter()
                .map(|(var_name, value)| {
                    let var_id = problem.intern(var_name);
                    Coefficient { name: var_id, value: *value }
                })
                .collect();

            if coefficients.is_empty() {
                return Err(LpParseError::validation_error(format!("Objective '{}' has no coefficients", obj_builder.name)));
            }

            let objective = Objective { name: name_id, coefficients, byte_offset: None };
            debug_assert!(!objective.coefficients.is_empty(), "postcondition: built objective must have coefficients");
            problem.add_objective(objective);
        }

        // Add constraints (intern names)
        for (_, constraint_builder) in self.constraints {
            match constraint_builder {
                ConstraintBuilder::Standard { name, coefficients, operator, rhs } => {
                    let operator = operator
                        .ok_or_else(|| LpParseError::constraint_syntax(0, format!("Constraint '{name}' is missing an operator")))?;
                    let rhs =
                        rhs.ok_or_else(|| LpParseError::constraint_syntax(0, format!("Constraint '{name}' is missing a right-hand side")))?;

                    let interned_coeffs: Vec<Coefficient> = coefficients
                        .iter()
                        .map(|(var_name, value)| {
                            let var_id = problem.intern(var_name);
                            Coefficient { name: var_id, value: *value }
                        })
                        .collect();

                    if interned_coeffs.is_empty() {
                        return Err(LpParseError::constraint_syntax(0, format!("Constraint '{name}' has no coefficients")));
                    }

                    debug_assert!(
                        interned_coeffs.iter().all(|c| c.value.is_finite()),
                        "postcondition: all Standard constraint coefficient values must be finite"
                    );

                    let name_id = problem.intern(&name);
                    problem.add_constraint(Constraint::Standard {
                        name: name_id,
                        coefficients: interned_coeffs,
                        operator,
                        rhs,
                        byte_offset: None,
                    });
                }
                ConstraintBuilder::SOS { name, sos_type, weights } => {
                    let interned_weights: Vec<Coefficient> = weights
                        .iter()
                        .map(|(var_name, value)| {
                            let var_id = problem.intern(var_name);
                            Coefficient { name: var_id, value: *value }
                        })
                        .collect();

                    if interned_weights.is_empty() {
                        return Err(LpParseError::invalid_sos_constraint(&name, "No weights specified"));
                    }

                    debug_assert!(
                        interned_weights.iter().all(|w| w.value.is_finite()),
                        "postcondition: all SOS constraint weight values must be finite"
                    );

                    let name_id = problem.intern(&name);
                    problem.add_constraint(Constraint::SOS { name: name_id, sos_type, weights: interned_weights, byte_offset: None });
                }
            }
        }

        Ok(problem)
    }
}

impl ObjectiveBuilder {
    #[must_use]
    /// Create a new objective builder
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into(), coefficients: Vec::new() }
    }

    #[must_use]
    /// Add a coefficient to the objective
    pub fn coefficient(mut self, name: impl Into<String>, value: f64) -> Self {
        self.coefficients.push((name.into(), value));
        self
    }

    #[must_use]
    /// Add multiple coefficients at once
    pub fn coefficients(mut self, coeffs: &[(&str, f64)]) -> Self {
        for &(name, value) in coeffs {
            self.coefficients.push((name.to_string(), value));
        }
        self
    }
}

impl ConstraintBuilder {
    #[must_use]
    /// Create a new standard constraint builder
    pub fn standard(name: impl Into<String>) -> Self {
        Self::Standard { name: name.into(), coefficients: Vec::new(), operator: None, rhs: None }
    }

    #[must_use]
    /// Create a new SOS constraint builder
    pub fn sos(name: impl Into<String>, sos_type: SOSType) -> Self {
        Self::SOS { name: name.into(), sos_type, weights: Vec::new() }
    }

    #[must_use]
    /// Add a coefficient to the constraint
    pub fn coefficient(mut self, name: impl Into<String>, value: f64) -> Self {
        match &mut self {
            Self::Standard { coefficients, .. } => {
                coefficients.push((name.into(), value));
            }
            Self::SOS { weights, .. } => {
                weights.push((name.into(), value));
            }
        }
        self
    }

    #[must_use]
    /// Set the constraint to less than or equal
    pub const fn le(mut self, rhs: f64) -> Self {
        if let Self::Standard { operator, rhs: rhs_ref, .. } = &mut self {
            *operator = Some(ComparisonOp::LTE);
            *rhs_ref = Some(rhs);
        }
        self
    }

    #[must_use]
    /// Set the constraint to less than
    pub const fn lt(mut self, rhs: f64) -> Self {
        if let Self::Standard { operator, rhs: rhs_ref, .. } = &mut self {
            *operator = Some(ComparisonOp::LT);
            *rhs_ref = Some(rhs);
        }
        self
    }

    #[must_use]
    /// Set the constraint to greater than or equal
    pub const fn ge(mut self, rhs: f64) -> Self {
        if let Self::Standard { operator, rhs: rhs_ref, .. } = &mut self {
            *operator = Some(ComparisonOp::GTE);
            *rhs_ref = Some(rhs);
        }
        self
    }

    #[must_use]
    /// Set the constraint to greater than
    pub const fn gt(mut self, rhs: f64) -> Self {
        if let Self::Standard { operator, rhs: rhs_ref, .. } = &mut self {
            *operator = Some(ComparisonOp::GT);
            *rhs_ref = Some(rhs);
        }
        self
    }

    #[must_use]
    /// Set the constraint to equal
    pub const fn eq(mut self, rhs: f64) -> Self {
        if let Self::Standard { operator, rhs: rhs_ref, .. } = &mut self {
            *operator = Some(ComparisonOp::EQ);
            *rhs_ref = Some(rhs);
        }
        self
    }
}

impl VariableBuilder {
    #[must_use]
    /// Create a new variable builder
    pub fn new(name: impl Into<String>) -> Self {
        let name = name.into();
        debug_assert!(!name.is_empty(), "variable name must not be empty");
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
}

#[must_use]
/// Convenience function to create a new LP problem builder
pub fn lp_problem() -> LpProblemBuilder {
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
        // Resolve name to check the coefficient variable
        let obj_id = problem.get_name_id("obj").unwrap();
        let obj = problem.objectives.get(&obj_id).unwrap();
        let coeff_name = problem.resolve(obj.coefficients[0].name);
        assert_eq!(coeff_name, "y");
        assert_eq!(problem.constraints.len(), 1);
        let v_id = problem.get_name_id("v").unwrap();
        assert_eq!(problem.variables.get(&v_id).unwrap().var_type, VariableType::Integer);
    }

    #[test]
    fn test_objective_builder() {
        // Single and multiple coefficients — build via LpProblemBuilder to intern
        let problem = LpProblemBuilder::new().objective("test", |o| o.coefficient("x", 5.0)).build().unwrap();
        let test_id = problem.get_name_id("test").unwrap();
        let obj = problem.objectives.get(&test_id).unwrap();
        assert_eq!(problem.resolve(obj.name), "test");
        assert_eq!(obj.coefficients.len(), 1);

        let problem2 = LpProblemBuilder::new()
            .objective("m", |o| o.coefficient("x", 1.0).coefficient("y", 2.0).coefficient("z", -3.0))
            .build()
            .unwrap();
        let m_id = problem2.get_name_id("m").unwrap();
        assert_eq!(problem2.objectives.get(&m_id).unwrap().coefficients.len(), 3);

        // Empty fails
        assert!(LpProblemBuilder::new().objective("e", |o| o).build().unwrap_err().to_string().contains("no coefficients"));
    }

    #[test]
    fn test_constraint_builder_operators() {
        // Test all operators via LpProblemBuilder
        let problem = LpProblemBuilder::new().constraint("t", |c| c.coefficient("x", 1.0).le(10.0)).build().unwrap();
        let t_id = problem.get_name_id("t").unwrap();
        if let Constraint::Standard { operator, rhs, .. } = problem.constraints.get(&t_id).unwrap() {
            assert_eq!((*operator, *rhs), (ComparisonOp::LTE, 10.0));
        }

        let problem = LpProblemBuilder::new().constraint("t", |c| c.coefficient("x", 1.0).ge(5.0)).build().unwrap();
        let t_id = problem.get_name_id("t").unwrap();
        if let Constraint::Standard { operator, rhs, .. } = problem.constraints.get(&t_id).unwrap() {
            assert_eq!((*operator, *rhs), (ComparisonOp::GTE, 5.0));
        }

        let problem = LpProblemBuilder::new().constraint("t", |c| c.coefficient("x", 1.0).eq(7.0)).build().unwrap();
        let t_id = problem.get_name_id("t").unwrap();
        if let Constraint::Standard { operator, rhs, .. } = problem.constraints.get(&t_id).unwrap() {
            assert_eq!((*operator, *rhs), (ComparisonOp::EQ, 7.0));
        }

        let problem = LpProblemBuilder::new().constraint("t", |c| c.coefficient("x", 1.0).lt(15.0)).build().unwrap();
        let t_id = problem.get_name_id("t").unwrap();
        if let Constraint::Standard { operator, rhs, .. } = problem.constraints.get(&t_id).unwrap() {
            assert_eq!((*operator, *rhs), (ComparisonOp::LT, 15.0));
        }

        let problem = LpProblemBuilder::new().constraint("t", |c| c.coefficient("x", 1.0).gt(2.0)).build().unwrap();
        let t_id = problem.get_name_id("t").unwrap();
        if let Constraint::Standard { operator, rhs, .. } = problem.constraints.get(&t_id).unwrap() {
            assert_eq!((*operator, *rhs), (ComparisonOp::GT, 2.0));
        }

        // Missing operator and no coefficients errors
        assert!(LpProblemBuilder::new().constraint("i", |c| c.coefficient("x", 1.0)).build().is_err());
        assert!(LpProblemBuilder::new().constraint("e", |c| c.le(10.0)).build().is_err());
    }

    #[test]
    fn test_constraint_builder_sos() {
        let problem = LpProblemBuilder::new()
            .sos_constraint("sos", SOSType::S1, |c| c.coefficient("x1", 1.0).coefficient("x2", 2.0))
            .build()
            .unwrap();
        let sos_id = problem.get_name_id("sos").unwrap();

        if let Constraint::SOS { name, sos_type, weights, .. } = problem.constraints.get(&sos_id).unwrap() {
            assert_eq!(problem.resolve(*name), "sos");
            assert_eq!(*sos_type, SOSType::S1);
            assert_eq!(weights.len(), 2);
        } else {
            panic!("Expected SOS");
        }

        // Empty SOS fails
        assert!(LpProblemBuilder::new().sos_constraint("e", SOSType::S2, |c| c).build().is_err());

        // Operators on SOS have no effect
        let problem =
            LpProblemBuilder::new().sos_constraint("s", SOSType::S1, |c| c.coefficient("x", 1.0).le(10.0).ge(5.0)).build().unwrap();
        let s_id = problem.get_name_id("s").unwrap();
        assert!(matches!(problem.constraints.get(&s_id).unwrap(), Constraint::SOS { .. }));
    }

    #[test]
    fn test_variable_builder_types() {
        // Test all variable types via LpProblemBuilder
        let build_var = |f: fn(VariableBuilder) -> VariableBuilder| -> VariableType {
            let problem = LpProblemBuilder::new().variable("x", f).build().unwrap();
            let x_id = problem.get_name_id("x").unwrap();
            problem.variables.get(&x_id).unwrap().var_type.clone()
        };

        assert_eq!(build_var(|v| v), VariableType::Free);
        assert_eq!(build_var(VariableBuilder::binary), VariableType::Binary);
        assert_eq!(build_var(VariableBuilder::integer), VariableType::Integer);
        assert_eq!(build_var(VariableBuilder::general), VariableType::General);
        assert_eq!(build_var(VariableBuilder::free), VariableType::Free);
        assert_eq!(build_var(VariableBuilder::semi_continuous), VariableType::SemiContinuous);
        assert_eq!(build_var(|v| v.lower_bound(5.0)), VariableType::LowerBound(5.0));
        assert_eq!(build_var(|v| v.upper_bound(10.0)), VariableType::UpperBound(10.0));
        assert_eq!(build_var(|v| v.bounds(0.0, 100.0)), VariableType::DoubleBound(0.0, 100.0));

        // Override behavior
        assert_eq!(build_var(|v| v.binary().integer()), VariableType::Integer);
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
            let problem = LpProblemBuilder::new().sos_constraint("sos", sos_type, |c| c.coefficient("x", 1.0)).build().unwrap();

            let sos_id = problem.get_name_id("sos").unwrap();
            if let Constraint::SOS { sos_type: st, .. } = problem.constraints.get(&sos_id).unwrap() {
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
        let ext_id = problem.get_name_id("ext").unwrap();
        assert_eq!(problem.objectives.get(&ext_id).unwrap().coefficients.len(), 3);

        // Special characters and long names
        let long = "x".repeat(1000);
        let problem = LpProblemBuilder::new()
            .name(&long)
            .variable("var_1.2$special", super::VariableBuilder::general)
            .variable(&long, super::VariableBuilder::binary)
            .build()
            .unwrap();
        assert!(problem.get_name_id("var_1.2$special").and_then(|id| problem.variables.get(&id)).is_some());
        assert!(problem.get_name_id(&long).and_then(|id| problem.variables.get(&id)).is_some());
    }

    #[test]
    fn test_validation_errors() {
        assert!(LpProblemBuilder::new().objective("e", |o| o).build().is_err());
        assert!(LpProblemBuilder::new().constraint("i", |c| c.coefficient("x", 1.0)).build().is_err());
    }
}
