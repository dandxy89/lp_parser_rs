//! Ergonomic programmatic construction of an [`LpProblem`] using string names.
//!
//! [`ProblemBuilder`] handles interning internally, so callers work purely with
//! `&str` names and never construct a [`NameId`](crate::interner::NameId) by
//! hand. This covers the common "build a model in code" use case, distinct from
//! the parse-then-mutate path.

use crate::model::{Coefficient, ComparisonOp, Constraint, Objective, SOSType, Sense, Variable, VariableBounds, VariableKind};
use crate::problem::LpProblem;

/// Builder for constructing an [`LpProblem`] programmatically with string names.
///
/// # Example
///
/// ```rust
/// use lp_parser_rs::ProblemBuilder;
/// use lp_parser_rs::model::{ComparisonOp, Sense, VariableBounds, VariableKind};
///
/// let problem = ProblemBuilder::new()
///     .name("example")
///     .sense(Sense::Minimize)
///     .variable("x", VariableKind::Continuous, VariableBounds::range(0.0, 10.0))
///     .variable("y", VariableKind::Binary, VariableBounds::free())
///     .objective("obj", &[("x", 2.0), ("y", 3.0)])
///     .constraint("c1", &[("x", 1.0), ("y", 1.0)], ComparisonOp::GTE, 5.0)
///     .build();
///
/// assert_eq!(problem.variable_count(), 2);
/// assert_eq!(problem.constraint_count(), 1);
/// assert_eq!(problem.objective_count(), 1);
/// ```
#[derive(Debug, Default)]
pub struct ProblemBuilder {
    problem: LpProblem,
}

impl ProblemBuilder {
    /// Start a new, empty builder.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the problem name.
    #[must_use]
    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.problem.set_name(name);
        self
    }

    /// Set the optimisation sense.
    #[must_use]
    pub const fn sense(mut self, sense: Sense) -> Self {
        self.problem.set_sense(sense);
        self
    }

    /// Declare a variable with an explicit kind and bounds.
    ///
    /// Declaring a variable before referencing it in an objective/constraint
    /// preserves its kind and bounds; referencing an undeclared variable
    /// auto-creates it as a continuous, free variable.
    #[must_use]
    pub fn variable(mut self, name: &str, kind: VariableKind, bounds: VariableBounds) -> Self {
        let id = self.problem.intern(name);
        self.problem.add_variable(Variable::new(id).with_kind(kind).with_bounds(bounds));
        self
    }

    /// Add an objective from `(variable_name, coefficient)` terms.
    #[must_use]
    pub fn objective(mut self, name: &str, terms: &[(&str, f64)]) -> Self {
        let obj_id = self.problem.intern(name);
        let coefficients = self.coefficients(terms);
        self.problem.add_objective(Objective { name: obj_id, coefficients, constant: 0.0, byte_offset: None });
        self
    }

    /// Add a standard linear constraint from `(variable_name, coefficient)` terms.
    #[must_use]
    pub fn constraint(mut self, name: &str, terms: &[(&str, f64)], operator: ComparisonOp, rhs: f64) -> Self {
        let con_id = self.problem.intern(name);
        let coefficients = self.coefficients(terms);
        self.problem.add_constraint(Constraint::Standard { name: con_id, coefficients, operator, rhs, byte_offset: None });
        self
    }

    /// Add a special-ordered-set constraint from `(variable_name, weight)` terms.
    #[must_use]
    pub fn sos_constraint(mut self, name: &str, sos_type: SOSType, terms: &[(&str, f64)]) -> Self {
        let con_id = self.problem.intern(name);
        let weights = self.coefficients(terms);
        self.problem.add_constraint(Constraint::SOS { name: con_id, sos_type, weights, byte_offset: None });
        self
    }

    /// Finish building and return the assembled [`LpProblem`].
    #[must_use]
    pub fn build(self) -> LpProblem {
        self.problem
    }

    /// Intern each term's variable name into a [`Coefficient`] vector.
    fn coefficients(&mut self, terms: &[(&str, f64)]) -> Vec<Coefficient> {
        terms.iter().map(|(name, value)| Coefficient { name: self.problem.intern(name), value: *value }).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builder_assembles_problem() {
        let problem = ProblemBuilder::new()
            .name("example")
            .sense(Sense::Minimize)
            .variable("x", VariableKind::Continuous, VariableBounds::range(0.0, 10.0))
            .variable("y", VariableKind::Binary, VariableBounds::free())
            .objective("obj", &[("x", 2.0), ("y", 3.0)])
            .constraint("c1", &[("x", 1.0), ("y", 1.0)], ComparisonOp::GTE, 5.0)
            .build();

        assert_eq!(problem.name(), Some("example"));
        assert_eq!(problem.sense, Sense::Minimize);
        assert_eq!(problem.variable_count(), 2);
        assert_eq!(problem.constraint_count(), 1);
        assert_eq!(problem.objective_count(), 1);

        let x = problem.name_id("x").expect("x interned");
        assert_eq!(problem.variables[&x].kind, VariableKind::Continuous);
        assert_eq!(problem.variables[&x].bounds, VariableBounds::range(0.0, 10.0));
        let y = problem.name_id("y").expect("y interned");
        assert_eq!(problem.variables[&y].kind, VariableKind::Binary);
    }

    #[test]
    fn test_builder_auto_creates_referenced_variable() {
        // "z" is referenced but never declared: it becomes continuous + free.
        let problem = ProblemBuilder::new().objective("obj", &[("z", 1.0)]).build();
        let z = problem.name_id("z").expect("z interned via objective term");
        assert_eq!(problem.variables[&z].kind, VariableKind::Continuous);
        assert!(problem.variables[&z].bounds.is_free());
    }

    #[test]
    fn test_builder_declared_kind_survives_reference() {
        // Declaring the variable first must not be clobbered by a later reference.
        let problem = ProblemBuilder::new()
            .variable("w", VariableKind::Integer, VariableBounds::range(0.0, 4.0))
            .constraint("c", &[("w", 1.0)], ComparisonOp::LTE, 4.0)
            .build();
        let w = problem.name_id("w").expect("w interned");
        assert_eq!(problem.variables[&w].kind, VariableKind::Integer);
        assert_eq!(problem.variables[&w].bounds, VariableBounds::range(0.0, 4.0));
    }
}
