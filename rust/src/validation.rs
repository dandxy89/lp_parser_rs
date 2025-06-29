//! Validation module for LP problem consistency and correctness.
//!
//! This module provides comprehensive validation for parsed LP problems,
//! checking for logical consistency, reference validity, and mathematical
//! correctness.

use std::collections::{HashMap, HashSet};

use crate::error::{LpParseError, LpResult};
use crate::model::{ComparisonOp, Constraint, Objective, Variable, VariableType};
use crate::problem::LpProblem;

/// Validation context containing problem state and validation results.
#[derive(Debug, Default)]
pub struct ValidationContext<'a> {
    /// Variables referenced in objectives but not declared
    pub undeclared_objective_vars: HashSet<&'a str>,
    /// Variables referenced in constraints but not declared
    pub undeclared_constraint_vars: HashSet<&'a str>,
    /// Variables declared but never used
    pub unused_variables: HashSet<&'a str>,
    /// Constraints with potential feasibility issues
    pub infeasible_constraints: Vec<String>,
    /// SOS constraints with invalid weights
    pub invalid_sos_constraints: Vec<String>,
    /// Variables with conflicting type declarations
    pub conflicting_variable_types: Vec<String>,
    /// Duplicate constraint names
    pub duplicate_constraints: Vec<String>,
    /// Duplicate objective names
    pub duplicate_objectives: Vec<String>,
    /// Warnings about the problem structure
    pub warnings: Vec<String>,
}

impl ValidationContext<'_> {
    #[must_use]
    /// Create a new validation context
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    /// Check if validation found any errors
    pub fn has_errors(&self) -> bool {
        !self.undeclared_objective_vars.is_empty()
            || !self.undeclared_constraint_vars.is_empty()
            || !self.infeasible_constraints.is_empty()
            || !self.invalid_sos_constraints.is_empty()
            || !self.conflicting_variable_types.is_empty()
            || !self.duplicate_constraints.is_empty()
            || !self.duplicate_objectives.is_empty()
    }

    #[must_use]
    /// Get a summary of all validation issues
    pub fn summary(&self) -> String {
        let mut summary = Vec::new();

        if !self.undeclared_objective_vars.is_empty() {
            summary.push(format!("Undeclared variables in objectives: {:?}", self.undeclared_objective_vars));
        }

        if !self.undeclared_constraint_vars.is_empty() {
            summary.push(format!("Undeclared variables in constraints: {:?}", self.undeclared_constraint_vars));
        }

        if !self.unused_variables.is_empty() {
            summary.push(format!("Unused variables: {:?}", self.unused_variables));
        }

        if !self.infeasible_constraints.is_empty() {
            summary.push(format!("Potentially infeasible constraints: {:?}", self.infeasible_constraints));
        }

        if !self.invalid_sos_constraints.is_empty() {
            summary.push(format!("Invalid SOS constraints: {:?}", self.invalid_sos_constraints));
        }

        if !self.conflicting_variable_types.is_empty() {
            summary.push(format!("Conflicting variable types: {:?}", self.conflicting_variable_types));
        }

        if !self.duplicate_constraints.is_empty() {
            summary.push(format!("Duplicate constraints: {:?}", self.duplicate_constraints));
        }

        if !self.duplicate_objectives.is_empty() {
            summary.push(format!("Duplicate objectives: {:?}", self.duplicate_objectives));
        }

        if !self.warnings.is_empty() {
            summary.push(format!("Warnings: {:?}", self.warnings));
        }

        if summary.is_empty() { "No validation issues found".to_string() } else { summary.join("; ") }
    }
}

/// Trait for validation of LP components
pub trait Validate<'a> {
    /// Validate the component and update the validation context
    fn validate(&self, context: &mut ValidationContext<'a>) -> LpResult<()>;
}

/// Comprehensive validator for LP problems
pub struct LpValidator;

impl LpValidator {
    /// Validate an entire LP problem
    pub fn validate<'a>(problem: &'a LpProblem<'a>) -> LpResult<ValidationContext<'a>> {
        let mut context = ValidationContext::new();

        // Collect all variable references
        let mut objective_vars = HashSet::new();
        let mut constraint_vars = HashSet::new();

        // Validate objectives
        for (name, objective) in &problem.objectives {
            Self::validate_objective(objective, &mut context, &mut objective_vars)?;

            // Check for duplicate objectives (shouldn't happen with HashMap, but good to validate)
            if problem.objectives.keys().filter(|&k| k == name).count() > 1 {
                context.duplicate_objectives.push(name.to_string());
            }
        }

        // Validate constraints
        for (name, constraint) in &problem.constraints {
            Self::validate_constraint(constraint, &mut context, &mut constraint_vars)?;

            // Check for duplicate constraints
            if problem.constraints.keys().filter(|&k| k == name).count() > 1 {
                context.duplicate_constraints.push(name.to_string());
            }
        }

        // Check variable references
        Self::validate_variable_references(&problem.variables, &objective_vars, &constraint_vars, &mut context);

        // Validate variable bounds
        Self::validate_variable_bounds(&problem.variables, &mut context)?;

        // Add structural warnings
        Self::add_structural_warnings(problem, &mut context);

        Ok(context)
    }

    /// Validate a single objective
    fn validate_objective<'a>(
        objective: &'a Objective<'a>,
        context: &mut ValidationContext<'a>,
        referenced_vars: &mut HashSet<&'a str>,
    ) -> LpResult<()> {
        if objective.coefficients.is_empty() {
            context.warnings.push(format!("Objective '{}' has no coefficients", objective.name));
        }

        for coeff in &objective.coefficients {
            referenced_vars.insert(coeff.name);

            // Check for infinite or NaN coefficients
            if !coeff.value.is_finite() {
                return Err(LpParseError::validation_error(format!(
                    "Invalid coefficient {} for variable '{}' in objective '{}'",
                    coeff.value, coeff.name, objective.name
                )));
            }
        }

        Ok(())
    }

    /// Validate a single constraint
    fn validate_constraint<'a>(
        constraint: &'a Constraint<'a>,
        context: &mut ValidationContext<'a>,
        referenced_vars: &mut HashSet<&'a str>,
    ) -> LpResult<()> {
        match constraint {
            Constraint::Standard { name, coefficients, operator, rhs } => {
                if coefficients.is_empty() {
                    context.warnings.push(format!("Constraint '{name}' has no coefficients"));
                }

                // Check for invalid RHS
                if !rhs.is_finite() {
                    return Err(LpParseError::validation_error(format!("Invalid RHS {rhs} in constraint '{name}'")));
                }

                // Check for infeasible constraints (e.g., 0 >= 1)
                if coefficients.is_empty() && *rhs != 0.0 {
                    match operator {
                        ComparisonOp::EQ if *rhs != 0.0 => {
                            context.infeasible_constraints.push(format!("Constraint '{name}': 0 = {rhs} is infeasible"));
                        }
                        ComparisonOp::LTE | ComparisonOp::LT if *rhs < 0.0 => {
                            context.infeasible_constraints.push(format!("Constraint '{name}': 0 <= {rhs} is infeasible"));
                        }
                        ComparisonOp::GTE | ComparisonOp::GT if *rhs > 0.0 => {
                            context.infeasible_constraints.push(format!("Constraint '{name}': 0 >= {rhs} is infeasible"));
                        }
                        _ => {}
                    }
                }

                for coeff in coefficients {
                    referenced_vars.insert(coeff.name);

                    // Check for infinite or NaN coefficients
                    if !coeff.value.is_finite() {
                        return Err(LpParseError::validation_error(format!(
                            "Invalid coefficient {} for variable '{}' in constraint '{}'",
                            coeff.value, coeff.name, name
                        )));
                    }
                }
            }
            Constraint::SOS { name, sos_type: _, weights } => {
                if weights.is_empty() {
                    context.invalid_sos_constraints.push(format!("SOS constraint '{name}' has no weights"));
                }

                // Check for duplicate variables in SOS constraint
                let mut seen_vars = HashSet::new();
                for weight in weights {
                    if !seen_vars.insert(weight.name) {
                        context.invalid_sos_constraints.push(format!("SOS constraint '{name}' has duplicate variable '{}'", weight.name));
                    }

                    referenced_vars.insert(weight.name);

                    // Check for invalid weights
                    if !weight.value.is_finite() || weight.value < 0.0 {
                        context
                            .invalid_sos_constraints
                            .push(format!("SOS constraint '{name}' has invalid weight {} for variable '{}'", weight.value, weight.name));
                    }
                }
            }
        }

        Ok(())
    }

    /// Validate variable references and usage
    fn validate_variable_references<'a>(
        declared_vars: &HashMap<&'a str, Variable<'a>>,
        objective_vars: &HashSet<&'a str>,
        constraint_vars: &HashSet<&'a str>,
        context: &mut ValidationContext<'a>,
    ) {
        let all_referenced: HashSet<&str> = objective_vars.union(constraint_vars).copied().collect();

        // Find undeclared variables
        for &var in objective_vars {
            if !declared_vars.contains_key(var) {
                context.undeclared_objective_vars.insert(var);
            }
        }

        for &var in constraint_vars {
            if !declared_vars.contains_key(var) {
                context.undeclared_constraint_vars.insert(var);
            }
        }

        // Find unused variables
        for &var_name in declared_vars.keys() {
            if !all_referenced.contains(var_name) {
                context.unused_variables.insert(var_name);
            }
        }
    }

    /// Validate variable bounds and types
    fn validate_variable_bounds<'a>(variables: &HashMap<&'a str, Variable<'a>>, context: &mut ValidationContext<'a>) -> LpResult<()> {
        for (name, variable) in variables {
            match &variable.var_type {
                VariableType::DoubleBound(lower, upper) => {
                    if !lower.is_finite() || !upper.is_finite() {
                        return Err(LpParseError::invalid_bounds(*name, format!("Invalid bounds: {lower} <= {name} <= {upper}")));
                    }

                    if lower > upper {
                        context
                            .infeasible_constraints
                            .push(format!("Variable '{name}' has infeasible bounds: {lower} <= {name} <= {upper}"));
                    }
                }
                VariableType::LowerBound(bound) | VariableType::UpperBound(bound) => {
                    if !bound.is_finite() {
                        return Err(LpParseError::invalid_bounds(*name, format!("Invalid bound: {bound}")));
                    }
                }
                _ => {}
            }
        }

        Ok(())
    }

    /// Add structural warnings about the problem
    fn add_structural_warnings<'a>(problem: &LpProblem<'a>, context: &mut ValidationContext<'a>) {
        if problem.objectives.is_empty() {
            context.warnings.push("Problem has no objectives".to_string());
        }

        if problem.constraints.is_empty() {
            context.warnings.push("Problem has no constraints".to_string());
        }

        if problem.variables.is_empty() {
            context.warnings.push("Problem has no variables".to_string());
        }

        if problem.objectives.len() > 1 {
            context.warnings.push(format!("Problem has {} objectives (multi-objective)", problem.objectives.len()));
        }
    }
}

impl LpProblem<'_> {
    /// Validate the LP problem and return validation results
    pub fn validate(&self) -> LpResult<ValidationContext<'_>> {
        LpValidator::validate(self)
    }

    /// Validate the LP problem and return an error if validation fails
    pub fn validate_strict(&self) -> LpResult<()> {
        let context = self.validate()?;

        if context.has_errors() {
            return Err(LpParseError::validation_error(context.summary()));
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::borrow::Cow;

    use super::*;
    use crate::model::{Coefficient, ComparisonOp, SOSType, Sense};

    // Test ValidationContext functionality
    #[test]
    fn test_validation_context_new() {
        let context = ValidationContext::new();
        assert!(context.undeclared_objective_vars.is_empty());
        assert!(context.undeclared_constraint_vars.is_empty());
        assert!(context.unused_variables.is_empty());
        assert!(context.infeasible_constraints.is_empty());
        assert!(context.invalid_sos_constraints.is_empty());
        assert!(context.conflicting_variable_types.is_empty());
        assert!(context.duplicate_constraints.is_empty());
        assert!(context.duplicate_objectives.is_empty());
        assert!(context.warnings.is_empty());
        assert!(!context.has_errors());
    }

    #[test]
    fn test_validation_context_has_errors() {
        let mut context = ValidationContext::new();
        assert!(!context.has_errors());

        // Test each error type
        context.undeclared_objective_vars.insert("x1");
        assert!(context.has_errors());
        context.undeclared_objective_vars.clear();

        context.undeclared_constraint_vars.insert("x2");
        assert!(context.has_errors());
        context.undeclared_constraint_vars.clear();

        context.infeasible_constraints.push("infeasible".to_string());
        assert!(context.has_errors());
        context.infeasible_constraints.clear();

        context.invalid_sos_constraints.push("invalid_sos".to_string());
        assert!(context.has_errors());
        context.invalid_sos_constraints.clear();

        context.conflicting_variable_types.push("conflict".to_string());
        assert!(context.has_errors());
        context.conflicting_variable_types.clear();

        context.duplicate_constraints.push("duplicate".to_string());
        assert!(context.has_errors());
        context.duplicate_constraints.clear();

        context.duplicate_objectives.push("duplicate".to_string());
        assert!(context.has_errors());
        context.duplicate_objectives.clear();

        assert!(!context.has_errors());
    }

    #[test]
    fn test_validation_context_summary() {
        let mut context = ValidationContext::new();
        assert_eq!(context.summary(), "No validation issues found");

        context.undeclared_objective_vars.insert("x1");
        context.warnings.push("Warning message".to_string());
        let summary = context.summary();
        assert!(summary.contains("Undeclared variables in objectives"));
        assert!(summary.contains("Warnings"));
    }

    // Test complete valid problem
    #[test]
    fn test_valid_problem() {
        let mut problem = LpProblem::new().with_sense(Sense::Minimize);

        // Add objective
        problem.add_objective(Objective {
            name: Cow::Borrowed("obj1"),
            coefficients: vec![Coefficient { name: "x1", value: 1.0 }, Coefficient { name: "x2", value: 2.0 }],
        });

        // Add constraint
        problem.add_constraint(Constraint::Standard {
            name: Cow::Borrowed("c1"),
            coefficients: vec![Coefficient { name: "x1", value: 1.0 }, Coefficient { name: "x2", value: 1.0 }],
            operator: ComparisonOp::LTE,
            rhs: 10.0,
        });

        let context = problem.validate().expect("Validation should succeed");
        assert!(!context.has_errors());
    }

    #[test]
    fn test_valid_problem_with_bounds() {
        let mut problem = LpProblem::new().with_sense(Sense::Maximize);

        // Add variables with bounds
        problem.add_variable(Variable { name: "x1", var_type: VariableType::DoubleBound(0.0, 10.0) });
        problem.add_variable(Variable { name: "x2", var_type: VariableType::LowerBound(0.0) });
        problem.add_variable(Variable { name: "x3", var_type: VariableType::UpperBound(100.0) });
        problem.add_variable(Variable { name: "x4", var_type: VariableType::Free });

        // Add objective using all variables - this will also create default variables
        problem.add_objective(Objective {
            name: Cow::Borrowed("maximize_profit"),
            coefficients: vec![
                Coefficient { name: "x1", value: 3.0 },
                Coefficient { name: "x2", value: 2.0 },
                Coefficient { name: "x3", value: 1.0 },
                Coefficient { name: "x4", value: 0.5 },
            ],
        });

        // Add constraint using some variables - this will also create default variables
        problem.add_constraint(Constraint::Standard {
            name: Cow::Borrowed("capacity"),
            coefficients: vec![Coefficient { name: "x1", value: 2.0 }, Coefficient { name: "x2", value: 1.0 }],
            operator: ComparisonOp::LTE,
            rhs: 20.0,
        });

        let context = problem.validate().expect("Validation should succeed");
        assert!(!context.has_errors());
        // All variables are used in the objective, so none should be unused
        assert_eq!(context.unused_variables.len(), 0);
    }

    // Test variable reference validation
    #[test]
    fn test_undeclared_variable_in_objective() {
        let objectives = {
            let mut obj_map = HashMap::new();
            obj_map.insert(
                Cow::Borrowed("obj1"),
                Objective { name: Cow::Borrowed("obj1"), coefficients: vec![Coefficient { name: "undeclared_var", value: 1.0 }] },
            );
            obj_map
        };

        let problem = LpProblem { name: None, sense: Sense::Minimize, objectives, constraints: HashMap::new(), variables: HashMap::new() };

        let context = problem.validate().expect("Validation should succeed");
        assert!(context.has_errors());
        assert!(context.undeclared_objective_vars.contains("undeclared_var"));
        assert!(context.undeclared_constraint_vars.is_empty());
    }

    #[test]
    fn test_undeclared_variable_in_constraint() {
        let constraints = {
            let mut constraint_map = HashMap::new();
            constraint_map.insert(
                Cow::Borrowed("c1"),
                Constraint::Standard {
                    name: Cow::Borrowed("c1"),
                    coefficients: vec![Coefficient { name: "undeclared_var", value: 1.0 }],
                    operator: ComparisonOp::LTE,
                    rhs: 10.0,
                },
            );
            constraint_map
        };

        let problem = LpProblem { name: None, sense: Sense::Minimize, objectives: HashMap::new(), constraints, variables: HashMap::new() };

        let context = problem.validate().expect("Validation should succeed");
        assert!(context.has_errors());
        assert!(context.undeclared_constraint_vars.contains("undeclared_var"));
        assert!(context.undeclared_objective_vars.is_empty());
    }

    #[test]
    fn test_unused_variables() {
        let mut problem = LpProblem::new();

        // Add variables that are never used
        problem.add_variable(Variable { name: "unused1", var_type: VariableType::Free });
        problem.add_variable(Variable { name: "unused2", var_type: VariableType::LowerBound(0.0) });

        let context = problem.validate().expect("Validation should succeed");
        assert!(!context.has_errors()); // Unused variables are warnings, not errors
        assert_eq!(context.unused_variables.len(), 2);
        assert!(context.unused_variables.contains("unused1"));
        assert!(context.unused_variables.contains("unused2"));
    }

    #[test]
    fn test_mixed_declared_undeclared_variables() {
        // Create problem manually to avoid automatic variable creation
        let mut variables = HashMap::new();
        variables.insert("x1", Variable { name: "x1", var_type: VariableType::Free });
        variables.insert("x2", Variable { name: "x2", var_type: VariableType::Free });

        let mut objectives = HashMap::new();
        objectives.insert(
            Cow::Borrowed("obj1"),
            Objective {
                name: Cow::Borrowed("obj1"),
                coefficients: vec![
                    Coefficient { name: "x1", value: 1.0 }, // declared
                    Coefficient { name: "x3", value: 2.0 }, // undeclared
                ],
            },
        );

        let mut constraints = HashMap::new();
        constraints.insert(
            Cow::Borrowed("c1"),
            Constraint::Standard {
                name: Cow::Borrowed("c1"),
                coefficients: vec![
                    Coefficient { name: "x2", value: 1.0 }, // declared
                    Coefficient { name: "x4", value: 1.0 }, // undeclared
                ],
                operator: ComparisonOp::LTE,
                rhs: 10.0,
            },
        );

        let problem = LpProblem { name: None, sense: Sense::Minimize, objectives, constraints, variables };

        let context = problem.validate().expect("Validation should succeed");
        assert!(context.has_errors());
        assert!(context.undeclared_objective_vars.contains("x3"));
        assert!(context.undeclared_constraint_vars.contains("x4"));
        assert!(context.unused_variables.is_empty()); // All declared variables are used
    }

    // Test constraint feasibility validation
    #[test]
    fn test_infeasible_constraint_zero_equals_nonzero() {
        let mut problem = LpProblem::new();

        // Add infeasible constraint: 0 = 1
        problem.add_constraint(Constraint::Standard {
            name: Cow::Borrowed("infeasible"),
            coefficients: vec![],
            operator: ComparisonOp::EQ,
            rhs: 1.0,
        });

        let context = problem.validate().expect("Validation should succeed");
        assert!(context.has_errors());
        assert!(!context.infeasible_constraints.is_empty());
        assert!(context.infeasible_constraints[0].contains("0 = 1"));
    }

    #[test]
    fn test_infeasible_constraint_zero_lte_negative() {
        let mut problem = LpProblem::new();

        // Add infeasible constraint: 0 <= -5
        problem.add_constraint(Constraint::Standard {
            name: Cow::Borrowed("infeasible"),
            coefficients: vec![],
            operator: ComparisonOp::LTE,
            rhs: -5.0,
        });

        let context = problem.validate().expect("Validation should succeed");
        assert!(context.has_errors());
        assert!(!context.infeasible_constraints.is_empty());
        assert!(context.infeasible_constraints[0].contains("0 <= -5"));
    }

    #[test]
    fn test_infeasible_constraint_zero_gte_positive() {
        let mut problem = LpProblem::new();

        // Add infeasible constraint: 0 >= 5
        problem.add_constraint(Constraint::Standard {
            name: Cow::Borrowed("infeasible"),
            coefficients: vec![],
            operator: ComparisonOp::GTE,
            rhs: 5.0,
        });

        let context = problem.validate().expect("Validation should succeed");
        assert!(context.has_errors());
        assert!(!context.infeasible_constraints.is_empty());
        assert!(context.infeasible_constraints[0].contains("0 >= 5"));
    }

    #[test]
    fn test_feasible_empty_constraints() {
        let mut problem = LpProblem::new();

        // These should be feasible
        problem.add_constraint(Constraint::Standard {
            name: Cow::Borrowed("feasible1"),
            coefficients: vec![],
            operator: ComparisonOp::EQ,
            rhs: 0.0,
        });

        problem.add_constraint(Constraint::Standard {
            name: Cow::Borrowed("feasible2"),
            coefficients: vec![],
            operator: ComparisonOp::LTE,
            rhs: 10.0,
        });

        problem.add_constraint(Constraint::Standard {
            name: Cow::Borrowed("feasible3"),
            coefficients: vec![],
            operator: ComparisonOp::GTE,
            rhs: -10.0,
        });

        let context = problem.validate().expect("Validation should succeed");
        assert!(!context.has_errors());
        assert!(context.infeasible_constraints.is_empty());
    }

    #[test]
    fn test_invalid_bounds_lower_greater_than_upper() {
        let mut problem = LpProblem::new();

        // Add variable with invalid bounds (lower > upper)
        problem.add_variable(Variable { name: "x1", var_type: VariableType::DoubleBound(10.0, 5.0) });

        let context = problem.validate().expect("Validation should succeed");
        assert!(context.has_errors());
        assert!(!context.infeasible_constraints.is_empty());
        assert!(context.infeasible_constraints[0].contains("infeasible bounds"));
    }

    #[test]
    fn test_invalid_bounds_infinite_values() {
        let mut problem = LpProblem::new();

        // Test infinite lower bound in double bound
        problem.add_variable(Variable { name: "x1", var_type: VariableType::DoubleBound(f64::INFINITY, 10.0) });

        let result = problem.validate();
        assert!(result.is_err());

        // Test infinite upper bound in double bound
        let mut problem = LpProblem::new();
        problem.add_variable(Variable { name: "x2", var_type: VariableType::DoubleBound(0.0, f64::NEG_INFINITY) });

        let result = problem.validate();
        assert!(result.is_err());

        // Test infinite single bounds
        let mut problem = LpProblem::new();
        problem.add_variable(Variable { name: "x3", var_type: VariableType::LowerBound(f64::NAN) });

        let result = problem.validate();
        assert!(result.is_err());
    }

    #[test]
    fn test_valid_bounds() {
        let mut problem = LpProblem::new();

        problem.add_variable(Variable { name: "x1", var_type: VariableType::DoubleBound(0.0, 10.0) });
        problem.add_variable(Variable { name: "x2", var_type: VariableType::DoubleBound(-5.0, 5.0) });
        problem.add_variable(Variable { name: "x3", var_type: VariableType::LowerBound(0.0) });
        problem.add_variable(Variable { name: "x4", var_type: VariableType::UpperBound(100.0) });
        problem.add_variable(Variable { name: "x5", var_type: VariableType::Free });

        let context = problem.validate().expect("Validation should succeed");
        assert!(!context.has_errors());
        assert!(context.infeasible_constraints.is_empty());
    }

    #[test]
    fn test_boundary_case_equal_bounds() {
        let mut problem = LpProblem::new();

        // Equal bounds should be valid (fixed variable)
        problem.add_variable(Variable { name: "x1", var_type: VariableType::DoubleBound(5.0, 5.0) });

        let context = problem.validate().expect("Validation should succeed");
        assert!(!context.has_errors());
        assert!(context.infeasible_constraints.is_empty());
    }

    #[test]
    fn test_valid_sos_constraint() {
        let mut problem = LpProblem::new();

        problem.add_constraint(Constraint::SOS {
            name: Cow::Borrowed("sos1"),
            sos_type: SOSType::S1,
            weights: vec![
                Coefficient { name: "x1", value: 1.0 },
                Coefficient { name: "x2", value: 2.0 },
                Coefficient { name: "x3", value: 3.0 },
            ],
        });

        let context = problem.validate().expect("Validation should succeed");
        assert!(!context.has_errors());
        assert!(context.invalid_sos_constraints.is_empty());
    }

    #[test]
    fn test_empty_sos_constraint() {
        let mut problem = LpProblem::new();

        problem.add_constraint(Constraint::SOS { name: Cow::Borrowed("empty_sos"), sos_type: SOSType::S2, weights: vec![] });

        let context = problem.validate().expect("Validation should succeed");
        assert!(context.has_errors());
        assert!(!context.invalid_sos_constraints.is_empty());
        assert!(context.invalid_sos_constraints[0].contains("has no weights"));
    }

    #[test]
    fn test_sos_constraint_duplicate_variables() {
        let mut problem = LpProblem::new();

        problem.add_constraint(Constraint::SOS {
            name: Cow::Borrowed("dup_sos"),
            sos_type: SOSType::S1,
            weights: vec![
                Coefficient { name: "x1", value: 1.0 },
                Coefficient { name: "x2", value: 2.0 },
                Coefficient { name: "x1", value: 3.0 }, // Duplicate
            ],
        });

        let context = problem.validate().expect("Validation should succeed");
        assert!(context.has_errors());
        assert!(!context.invalid_sos_constraints.is_empty());
        assert!(context.invalid_sos_constraints[0].contains("duplicate variable"));
    }

    #[test]
    fn test_sos_constraint_invalid_weights() {
        let mut problem = LpProblem::new();

        // Test negative weight
        problem.add_constraint(Constraint::SOS {
            name: Cow::Borrowed("neg_weight_sos"),
            sos_type: SOSType::S1,
            weights: vec![
                Coefficient { name: "x1", value: 1.0 },
                Coefficient { name: "x2", value: -2.0 }, // Negative weight
            ],
        });

        let context = problem.validate().expect("Validation should succeed");
        assert!(context.has_errors());
        assert!(!context.invalid_sos_constraints.is_empty());
        assert!(context.invalid_sos_constraints[0].contains("invalid weight"));

        // Test infinite weight
        let mut problem = LpProblem::new();
        problem.add_constraint(Constraint::SOS {
            name: Cow::Borrowed("inf_weight_sos"),
            sos_type: SOSType::S2,
            weights: vec![
                Coefficient { name: "x1", value: 1.0 },
                Coefficient { name: "x2", value: f64::INFINITY }, // Infinite weight
            ],
        });

        let context = problem.validate().expect("Validation should succeed");
        assert!(context.has_errors());
        assert!(!context.invalid_sos_constraints.is_empty());
    }

    #[test]
    fn test_sos_constraint_zero_weight() {
        let mut problem = LpProblem::new();

        // Zero weight should be valid
        problem.add_constraint(Constraint::SOS {
            name: Cow::Borrowed("zero_weight_sos"),
            sos_type: SOSType::S1,
            weights: vec![Coefficient { name: "x1", value: 0.0 }, Coefficient { name: "x2", value: 1.0 }],
        });

        let context = problem.validate().expect("Validation should succeed");
        assert!(!context.has_errors());
        assert!(context.invalid_sos_constraints.is_empty());
    }

    #[test]
    fn test_empty_objective() {
        let mut problem = LpProblem::new();

        problem.add_objective(Objective { name: Cow::Borrowed("empty_obj"), coefficients: vec![] });

        let context = problem.validate().expect("Validation should succeed");
        assert!(!context.has_errors()); // Empty objective is just a warning
        assert!(!context.warnings.is_empty());
        assert!(context.warnings[0].contains("has no coefficients"));
    }

    #[test]
    fn test_objective_with_infinite_coefficient() {
        let objectives = {
            let mut obj_map = HashMap::new();
            obj_map.insert(
                Cow::Borrowed("inf_obj"),
                Objective { name: Cow::Borrowed("inf_obj"), coefficients: vec![Coefficient { name: "x1", value: f64::INFINITY }] },
            );
            obj_map
        };

        let problem = LpProblem { name: None, sense: Sense::Minimize, objectives, constraints: HashMap::new(), variables: HashMap::new() };

        let result = problem.validate();
        assert!(result.is_err());
    }

    #[test]
    fn test_objective_with_nan_coefficient() {
        let objectives = {
            let mut obj_map = HashMap::new();
            obj_map.insert(
                Cow::Borrowed("nan_obj"),
                Objective { name: Cow::Borrowed("nan_obj"), coefficients: vec![Coefficient { name: "x1", value: f64::NAN }] },
            );
            obj_map
        };

        let problem = LpProblem { name: None, sense: Sense::Minimize, objectives, constraints: HashMap::new(), variables: HashMap::new() };

        let result = problem.validate();
        assert!(result.is_err());
    }

    // Test constraint coefficient validation
    #[test]
    fn test_constraint_with_infinite_coefficient() {
        let constraints = {
            let mut constraint_map = HashMap::new();
            constraint_map.insert(
                Cow::Borrowed("inf_constraint"),
                Constraint::Standard {
                    name: Cow::Borrowed("inf_constraint"),
                    coefficients: vec![Coefficient { name: "x1", value: f64::INFINITY }],
                    operator: ComparisonOp::LTE,
                    rhs: 10.0,
                },
            );
            constraint_map
        };

        let problem = LpProblem { name: None, sense: Sense::Minimize, objectives: HashMap::new(), constraints, variables: HashMap::new() };

        let result = problem.validate();
        assert!(result.is_err());
    }

    #[test]
    fn test_constraint_with_infinite_rhs() {
        let constraints = {
            let mut constraint_map = HashMap::new();
            constraint_map.insert(
                Cow::Borrowed("inf_rhs"),
                Constraint::Standard {
                    name: Cow::Borrowed("inf_rhs"),
                    coefficients: vec![Coefficient { name: "x1", value: 1.0 }],
                    operator: ComparisonOp::LTE,
                    rhs: f64::INFINITY,
                },
            );
            constraint_map
        };

        let problem = LpProblem { name: None, sense: Sense::Minimize, objectives: HashMap::new(), constraints, variables: HashMap::new() };

        let result = problem.validate();
        assert!(result.is_err());
    }

    #[test]
    fn test_empty_constraint() {
        let mut problem = LpProblem::new();

        problem.add_constraint(Constraint::Standard {
            name: Cow::Borrowed("empty_constraint"),
            coefficients: vec![],
            operator: ComparisonOp::LTE,
            rhs: 10.0,
        });

        let context = problem.validate().expect("Validation should succeed");
        assert!(!context.has_errors()); // Empty constraint is just a warning
        assert!(!context.warnings.is_empty());
        assert!(context.warnings[0].contains("has no coefficients"));
    }

    // Test structural warnings
    #[test]
    fn test_empty_problem_warnings() {
        let problem = LpProblem::new();

        let context = problem.validate().expect("Validation should succeed");
        assert!(!context.has_errors());
        assert_eq!(context.warnings.len(), 3);
        assert!(context.warnings.iter().any(|w| w.contains("no objectives")));
        assert!(context.warnings.iter().any(|w| w.contains("no constraints")));
        assert!(context.warnings.iter().any(|w| w.contains("no variables")));
    }

    #[test]
    fn test_multi_objective_warning() {
        let mut problem = LpProblem::new();

        problem.add_objective(Objective { name: Cow::Borrowed("obj1"), coefficients: vec![Coefficient { name: "x1", value: 1.0 }] });

        problem.add_objective(Objective { name: Cow::Borrowed("obj2"), coefficients: vec![Coefficient { name: "x1", value: 2.0 }] });

        let context = problem.validate().expect("Validation should succeed");
        assert!(!context.has_errors());
        assert!(context.warnings.iter().any(|w| w.contains("multi-objective")));
    }

    #[test]
    fn test_validate_strict_success() {
        let mut problem = LpProblem::new();

        problem.add_variable(Variable { name: "x1", var_type: VariableType::Free });
        problem.add_objective(Objective { name: Cow::Borrowed("obj1"), coefficients: vec![Coefficient { name: "x1", value: 1.0 }] });

        problem.add_constraint(Constraint::Standard {
            name: Cow::Borrowed("c1"),
            coefficients: vec![Coefficient { name: "x1", value: 1.0 }],
            operator: ComparisonOp::LTE,
            rhs: 10.0,
        });

        assert!(problem.validate_strict().is_ok());
    }

    #[test]
    fn test_validate_strict_failure() {
        let objectives = {
            let mut obj_map = HashMap::new();
            obj_map.insert(
                Cow::Borrowed("obj1"),
                Objective { name: Cow::Borrowed("obj1"), coefficients: vec![Coefficient { name: "undeclared", value: 1.0 }] },
            );
            obj_map
        };

        let problem = LpProblem { name: None, sense: Sense::Minimize, objectives, constraints: HashMap::new(), variables: HashMap::new() };

        assert!(problem.validate_strict().is_err());
    }

    #[test]
    fn test_complex_problem_with_multiple_issues() {
        let mut variables = HashMap::new();
        variables.insert("x1", Variable { name: "x1", var_type: VariableType::DoubleBound(10.0, 5.0) }); // Invalid bounds
        variables.insert("unused", Variable { name: "unused", var_type: VariableType::Free }); // Unused

        let mut objectives = HashMap::new();
        objectives.insert(
            Cow::Borrowed("obj1"),
            Objective {
                name: Cow::Borrowed("obj1"),
                coefficients: vec![Coefficient { name: "x1", value: 1.0 }, Coefficient { name: "undeclared_obj", value: 2.0 }],
            },
        );

        let mut constraints = HashMap::new();
        constraints.insert(
            Cow::Borrowed("c1"),
            Constraint::Standard {
                name: Cow::Borrowed("c1"),
                coefficients: vec![Coefficient { name: "undeclared_constraint", value: 1.0 }],
                operator: ComparisonOp::LTE,
                rhs: 10.0,
            },
        );
        constraints.insert(
            Cow::Borrowed("c2"),
            Constraint::Standard {
                name: Cow::Borrowed("c2"),
                coefficients: vec![],
                operator: ComparisonOp::EQ,
                rhs: 1.0, // Infeasible: 0 = 1
            },
        );
        constraints.insert(
            Cow::Borrowed("sos1"),
            Constraint::SOS {
                name: Cow::Borrowed("sos1"),
                sos_type: SOSType::S1,
                weights: vec![
                    Coefficient { name: "x1", value: 1.0 },
                    Coefficient { name: "x1", value: 2.0 }, // Duplicate variable
                ],
            },
        );

        let problem =
            LpProblem { name: Some("complex_problem".to_string().into()), sense: Sense::Minimize, objectives, constraints, variables };

        let context = problem.validate().expect("Validation should succeed");
        assert!(context.has_errors());

        // Check all the issues are detected
        assert!(context.undeclared_objective_vars.contains("undeclared_obj"));
        assert!(context.undeclared_constraint_vars.contains("undeclared_constraint"));
        assert!(context.unused_variables.contains("unused"));
        assert!(!context.infeasible_constraints.is_empty());
        assert!(!context.invalid_sos_constraints.is_empty());

        // Test summary contains all issues
        let summary = context.summary();
        assert!(summary.contains("Undeclared variables in objectives"));
        assert!(summary.contains("Undeclared variables in constraints"));
        assert!(summary.contains("Unused variables"));
        assert!(summary.contains("infeasible constraints"));
        assert!(summary.contains("Invalid SOS constraints"));
    }

    #[test]
    fn test_large_problem_performance() {
        let mut problem = LpProblem::new();

        // Add many variables first
        for i in 0..1000 {
            problem.add_variable(Variable { name: Box::leak(format!("x{i}").into_boxed_str()), var_type: VariableType::Free });
        }

        // Add many constraints - these will overlap with some of our pre-declared variables
        for i in 0..100 {
            problem.add_constraint(Constraint::Standard {
                name: Cow::Owned(format!("c{i}")),
                coefficients: (0..10)
                    .map(|j| Coefficient { name: Box::leak(format!("x{}", i * 10 + j).into_boxed_str()), value: 1.0 })
                    .collect(),
                operator: ComparisonOp::LTE,
                rhs: 10.0,
            });
        }

        // Add objective - this will also overlap with some variables
        problem.add_objective(Objective {
            name: Cow::Borrowed("obj"),
            coefficients: (0..100).map(|i| Coefficient { name: Box::leak(format!("x{i}").into_boxed_str()), value: 1.0 }).collect(),
        });

        let context = problem.validate().expect("Validation should succeed");
        assert!(!context.has_errors());
        // Variables x0-x999 used in constraints, x0-x99 also used in objective
        // So unused variables should be x1000 and up (but we only created x0-x999)
        // All variables x0-x999 are used in either constraints or objective
        assert_eq!(context.unused_variables.len(), 0); // All variables are actually used
    }
}
