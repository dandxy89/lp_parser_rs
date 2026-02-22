//! Compatibility layer for the `lp-solvers` crate.
//!
//! This module provides adapters that allow `LpProblem` from `lp_parser_rs`
//! to be used with the `lp-solvers` crate for solving LP problems using
//! external solvers like Cbc, Gurobi, CPLEX, and GLPK.
//!
//! # Feature Flag
//!
//! This module is only available when the `lp-solvers` feature is enabled:
//!
//! ```toml
//! [dependencies]
//! lp_parser_rs = { version = "3.0", features = ["lp-solvers"] }
//! ```
//!
//! # Usage Example
//!
//! ```rust,ignore
//! use lp_parser_rs::problem::LpProblem;
//! use lp_parser_rs::lp_solvers_compat::ToLpSolvers;
//! use lp_solvers::solvers::{CbcSolver, SolverTrait};
//!
//! let lp_content = r"
//! Minimize
//!  obj: 2 x + 3 y
//! Subject To
//!  c1: x + y <= 10
//! End
//! ";
//!
//! let problem = LpProblem::parse(lp_content).unwrap();
//! let compat = problem.to_lp_solvers().unwrap();
//!
//! // Check for any warnings about unsupported features
//! for warning in compat.warnings() {
//!     eprintln!("Warning: {}", warning);
//! }
//!
//! // Solve using CBC solver
//! let solver = CbcSolver::new();
//! let solution = solver.run(&compat).unwrap();
//! println!("Solution: {:?}", solution);
//! ```
//!
//! # Limitations
//!
//! The following features of `lp_parser_rs` are not fully supported by `lp-solvers`:
//!
//! - **Multiple objectives**: Only single-objective problems are supported.
//!   Multi-objective problems will result in an error.
//!
//! - **Strict inequalities**: Constraints using `<` or `>` are not supported.
//!   Use `<=` or `>=` instead.
//!
//! - **SOS constraints**: Special Ordered Set constraints are ignored with a warning.
//!
//! - **Semi-continuous variables**: These are approximated as continuous variables
//!   with a warning.

use std::cmp::Ordering;
use std::fmt;

use indexmap::map::Values;
use lp_solvers::lp_format::{AsVariable, LpObjective, WriteToLpFileFormat};

use crate::NUMERIC_EPSILON;
use crate::interner::{NameId, NameInterner};
use crate::model::{Coefficient, ComparisonOp, Constraint, Objective, Sense, Variable, VariableType};
use crate::problem::LpProblem;

/// Errors that can occur when converting an `LpProblem` to lp-solvers format.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LpSolversCompatError {
    /// The problem has multiple objectives, but lp-solvers only supports a single objective.
    #[error("multiple objectives found ({count}); lp-solvers only supports single objective")]
    MultipleObjectives {
        /// The number of objectives in the problem.
        count: usize,
    },

    /// The problem has no objectives.
    #[error("no objectives found; at least one objective is required")]
    NoObjectives,

    /// A constraint uses a strict inequality operator which is not supported.
    #[error("strict inequality '{operator}' in constraint '{constraint}' is not supported by lp-solvers")]
    StrictInequalityNotSupported {
        /// The name of the constraint with the unsupported operator.
        constraint: String,
        /// The unsupported operator (< or >).
        operator: String,
    },
}

/// Warnings about features that are not fully supported but can be approximated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LpSolversCompatWarning {
    /// An SOS constraint was ignored because lp-solvers does not support them.
    SosConstraintIgnored {
        /// The name of the ignored SOS constraint.
        name: String,
    },

    /// A semi-continuous variable is being treated as continuous.
    SemiContinuousApproximated {
        /// The name of the semi-continuous variable.
        name: String,
    },
}

impl fmt::Display for LpSolversCompatWarning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SosConstraintIgnored { name } => {
                write!(f, "SOS constraint '{name}' will be ignored; lp-solvers does not support SOS constraints")
            }
            Self::SemiContinuousApproximated { name } => {
                write!(f, "semi-continuous variable '{name}' is not directly supported; treating as continuous")
            }
        }
    }
}

/// Wrapper around `Variable` that implements `AsVariable` for lp-solvers.
#[derive(Debug, Clone)]
pub struct VariableAdapter<'a> {
    name: &'a str,
    var_type: &'a VariableType,
}

impl AsVariable for VariableAdapter<'_> {
    fn name(&self) -> &str {
        self.name
    }

    fn is_integer(&self) -> bool {
        matches!(self.var_type, VariableType::Binary | VariableType::Integer)
    }

    fn lower_bound(&self) -> f64 {
        match self.var_type {
            VariableType::Free => f64::NEG_INFINITY,
            VariableType::General
            | VariableType::Binary
            | VariableType::Integer
            | VariableType::UpperBound(_)
            | VariableType::SemiContinuous
            | VariableType::SOS => 0.0,
            VariableType::LowerBound(lb) | VariableType::DoubleBound(lb, _) => *lb,
        }
    }

    fn upper_bound(&self) -> f64 {
        match self.var_type {
            VariableType::Free
            | VariableType::General
            | VariableType::Integer
            | VariableType::LowerBound(_)
            | VariableType::SemiContinuous
            | VariableType::SOS => f64::INFINITY,
            VariableType::Binary => 1.0,
            VariableType::UpperBound(ub) | VariableType::DoubleBound(_, ub) => *ub,
        }
    }
}

/// Wrapper for objective/constraint coefficients that implements `WriteToLpFileFormat`.
#[derive(Debug, Clone)]
pub struct ExpressionAdapter<'a> {
    coefficients: &'a [Coefficient],
    interner: &'a NameInterner,
}

impl WriteToLpFileFormat for ExpressionAdapter<'_> {
    fn to_lp_file_format(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Filter out zero and non-finite coefficients
        let non_zero: Vec<_> = self.coefficients.iter().filter(|c| c.value.is_finite() && c.value.abs() > NUMERIC_EPSILON).collect();

        // Handle empty expression (all zeros or no terms)
        if non_zero.is_empty() {
            return write!(f, "0");
        }

        for (i, coeff) in non_zero.iter().enumerate() {
            let value = coeff.value;
            let name = self.interner.resolve(coeff.name);

            if i == 0 {
                // First term
                if value < 0.0 {
                    if (value.abs() - 1.0).abs() < NUMERIC_EPSILON {
                        write!(f, "- {name}")?;
                    } else {
                        write!(f, "- {} {name}", value.abs())?;
                    }
                } else if (value - 1.0).abs() < NUMERIC_EPSILON {
                    write!(f, "{name}")?;
                } else {
                    write!(f, "{value} {name}")?;
                }
            } else {
                // Subsequent terms
                let sign = if value < 0.0 { "-" } else { "+" };
                let abs_val = value.abs();

                if (abs_val - 1.0).abs() < NUMERIC_EPSILON {
                    write!(f, " {sign} {name}")?;
                } else {
                    write!(f, " {sign} {abs_val} {name}")?;
                }
            }
        }
        Ok(())
    }
}

/// Iterator over variables adapted for lp-solvers.
pub struct VariableIterator<'a> {
    inner: Values<'a, NameId, Variable>,
    interner: &'a NameInterner,
}

impl<'a> Iterator for VariableIterator<'a> {
    type Item = VariableAdapter<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next().map(|v| VariableAdapter { name: self.interner.resolve(v.name), var_type: &v.var_type })
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}

impl ExactSizeIterator for VariableIterator<'_> {}

/// Iterator over constraints adapted for lp-solvers.
///
/// This iterator filters out SOS constraints since lp-solvers does not support them.
pub struct ConstraintIterator<'a> {
    inner: Values<'a, NameId, Constraint>,
    interner: &'a NameInterner,
}

impl<'a> Iterator for ConstraintIterator<'a> {
    type Item = lp_solvers::lp_format::Constraint<ExpressionAdapter<'a>>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            match self.inner.next() {
                Some(Constraint::Standard { coefficients, operator, rhs, .. }) => {
                    let ordering = match operator {
                        ComparisonOp::LTE => Ordering::Less,
                        ComparisonOp::EQ => Ordering::Equal,
                        ComparisonOp::GTE => Ordering::Greater,
                        // LT/GT should have been caught during validation
                        ComparisonOp::LT | ComparisonOp::GT => {
                            unreachable!("strict inequalities should be caught during validation")
                        }
                    };
                    return Some(lp_solvers::lp_format::Constraint {
                        lhs: ExpressionAdapter { coefficients, interner: self.interner },
                        operator: ordering,
                        rhs: *rhs,
                    });
                }
                Some(Constraint::SOS { .. }) => {} // Skip SOS constraints
                None => return None,
            }
        }
    }
}

/// A validated wrapper that guarantees compatibility with lp-solvers.
#[derive(Debug)]
pub struct LpSolversCompat<'a> {
    problem: &'a LpProblem,
    objective: &'a Objective,
    warnings: Vec<LpSolversCompatWarning>,
}

impl<'a> LpSolversCompat<'a> {
    /// Try to create a compatible wrapper from an `LpProblem`.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The Problem has multiple objectives
    /// - The Problem has no objectives
    /// - Any constraint uses strict inequalities (`<` or `>`)
    ///
    /// # Panics
    ///
    /// Panics if the problem has exactly one objective but it cannot be accessed (internal error)
    pub fn try_new(problem: &'a LpProblem) -> Result<Self, LpSolversCompatError> {
        // Validate single objective
        if problem.objectives.is_empty() {
            return Err(LpSolversCompatError::NoObjectives);
        }
        if problem.objectives.len() > 1 {
            return Err(LpSolversCompatError::MultipleObjectives { count: problem.objectives.len() });
        }

        let objective = problem.objectives.values().next().expect("objective must exist: length check passed");
        let mut warnings = Vec::new();

        // Validate constraints (no strict inequalities)
        for constraint in problem.constraints.values() {
            match constraint {
                Constraint::Standard { name, operator, .. } => {
                    if matches!(operator, ComparisonOp::LT | ComparisonOp::GT) {
                        return Err(LpSolversCompatError::StrictInequalityNotSupported {
                            constraint: problem.interner.resolve(*name).to_string(),
                            operator: operator.to_string(),
                        });
                    }
                }
                Constraint::SOS { name, .. } => {
                    warnings.push(LpSolversCompatWarning::SosConstraintIgnored { name: problem.interner.resolve(*name).to_string() });
                }
            }
        }

        // Check for semi-continuous variables
        for variable in problem.variables.values() {
            if matches!(variable.var_type, VariableType::SemiContinuous) {
                warnings
                    .push(LpSolversCompatWarning::SemiContinuousApproximated { name: problem.interner.resolve(variable.name).to_string() });
            }
        }

        Ok(Self { problem, objective, warnings })
    }

    /// Returns any warnings generated during validation.
    #[must_use]
    pub fn warnings(&self) -> &[LpSolversCompatWarning] {
        &self.warnings
    }

    /// Returns `true` if there are no warnings.
    #[must_use]
    pub fn is_fully_compatible(&self) -> bool {
        self.warnings.is_empty()
    }
}

impl<'a> lp_solvers::lp_format::LpProblem<'a> for LpSolversCompat<'a> {
    type Variable = VariableAdapter<'a>;
    type Expression = ExpressionAdapter<'a>;
    type ConstraintIterator = ConstraintIterator<'a>;
    type VariableIterator = VariableIterator<'a>;

    fn variables(&'a self) -> Self::VariableIterator {
        VariableIterator { inner: self.problem.variables.values(), interner: &self.problem.interner }
    }

    fn objective(&'a self) -> Self::Expression {
        ExpressionAdapter { coefficients: &self.objective.coefficients, interner: &self.problem.interner }
    }

    fn sense(&'a self) -> LpObjective {
        match self.problem.sense {
            Sense::Minimize => LpObjective::Minimize,
            Sense::Maximize => LpObjective::Maximize,
        }
    }

    fn constraints(&'a self) -> Self::ConstraintIterator {
        ConstraintIterator { inner: self.problem.constraints.values(), interner: &self.problem.interner }
    }

    fn name(&self) -> &str {
        self.problem.name.as_deref().unwrap_or("lp_parser_problem")
    }
}

/// Extension trait for converting `LpProblem` to lp-solvers compatible format.
pub trait ToLpSolvers {
    /// Try to convert to an lp-solvers compatible wrapper.
    ///
    /// # Errors
    ///
    /// Returns an error if the problem is not compatible with lp-solvers
    /// (e.g., multiple objectives or strict inequalities).
    fn to_lp_solvers(&self) -> Result<LpSolversCompat<'_>, LpSolversCompatError>;
}

impl ToLpSolvers for LpProblem {
    fn to_lp_solvers(&self) -> Result<LpSolversCompat<'_>, LpSolversCompatError> {
        LpSolversCompat::try_new(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{SOSType, Variable};

    fn simple_problem() -> LpProblem {
        let mut p = LpProblem::new().with_sense(Sense::Minimize);
        let obj_id = p.intern("obj");
        let x_id = p.intern("x");
        let c1_id = p.intern("c1");
        p.objectives.insert(obj_id, Objective { name: obj_id, coefficients: vec![Coefficient { name: x_id, value: 2.0 }] });
        p.constraints.insert(
            c1_id,
            Constraint::Standard {
                name: c1_id,
                coefficients: vec![Coefficient { name: x_id, value: 1.0 }],
                operator: ComparisonOp::LTE,
                rhs: 10.0,
                byte_offset: None,
            },
        );
        p.variables.insert(x_id, Variable::new(x_id).with_var_type(VariableType::General));
        p
    }

    fn adapter(var_type: VariableType) -> VariableAdapter<'static> {
        VariableAdapter { name: "x", var_type: Box::leak(Box::new(var_type)) }
    }

    fn expr_fmt(problem: &LpProblem, coeffs: &[Coefficient]) -> String {
        struct D<'a>(ExpressionAdapter<'a>);
        impl fmt::Display for D<'_> {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.to_lp_file_format(f)
            }
        }
        format!("{}", D(ExpressionAdapter { coefficients: coeffs, interner: &problem.interner }))
    }

    #[test]
    fn test_validation_errors() {
        // No objectives
        assert!(matches!(LpSolversCompat::try_new(&LpProblem::new()), Err(LpSolversCompatError::NoObjectives)));

        // Multiple objectives
        let mut p = simple_problem();
        let obj2_id = p.intern("obj2");
        p.objectives.insert(obj2_id, Objective { name: obj2_id, coefficients: vec![] });
        assert!(matches!(LpSolversCompat::try_new(&p), Err(LpSolversCompatError::MultipleObjectives { count: 2 })));

        // Strict inequalities
        for op in [ComparisonOp::LT, ComparisonOp::GT] {
            let mut p = simple_problem();
            let c2_id = p.intern("c2");
            p.constraints
                .insert(c2_id, Constraint::Standard { name: c2_id, coefficients: vec![], operator: op, rhs: 0.0, byte_offset: None });
            assert!(matches!(LpSolversCompat::try_new(&p), Err(LpSolversCompatError::StrictInequalityNotSupported { .. })));
        }
    }

    #[test]
    fn test_warnings() {
        // SOS constraint
        let mut p = simple_problem();
        let sos1_id = p.intern("sos1");
        p.constraints.insert(sos1_id, Constraint::SOS { name: sos1_id, sos_type: SOSType::S1, weights: vec![], byte_offset: None });
        let c = LpSolversCompat::try_new(&p).unwrap();
        assert!(!c.is_fully_compatible());
        assert!(matches!(&c.warnings()[0], LpSolversCompatWarning::SosConstraintIgnored { .. }));

        // Semi-continuous
        let mut p = simple_problem();
        let y_id = p.intern("y");
        p.variables.insert(y_id, Variable::new(y_id).with_var_type(VariableType::SemiContinuous));
        let c = LpSolversCompat::try_new(&p).unwrap();
        assert!(matches!(&c.warnings()[0], LpSolversCompatWarning::SemiContinuousApproximated { .. }));
    }

    #[test]
    fn test_variable_bounds() {
        let cases: &[(VariableType, f64, f64, bool)] = &[
            (VariableType::Free, f64::NEG_INFINITY, f64::INFINITY, false),
            (VariableType::General, 0.0, f64::INFINITY, false),
            (VariableType::Binary, 0.0, 1.0, true),
            (VariableType::Integer, 0.0, f64::INFINITY, true),
            (VariableType::LowerBound(5.0), 5.0, f64::INFINITY, false),
            (VariableType::UpperBound(10.0), 0.0, 10.0, false),
            (VariableType::DoubleBound(-10.0, 10.0), -10.0, 10.0, false),
        ];
        for (vt, lb, ub, is_int) in cases {
            let a = adapter(vt.clone());
            assert!(
                (a.lower_bound() - *lb).abs() < f64::EPSILON
                    || (a.lower_bound().is_infinite() && lb.is_infinite() && a.lower_bound().signum() == lb.signum()),
                "lower_bound for {vt:?}"
            );
            assert!(
                (a.upper_bound() - *ub).abs() < f64::EPSILON
                    || (a.upper_bound().is_infinite() && ub.is_infinite() && a.upper_bound().signum() == ub.signum()),
                "upper_bound for {vt:?}"
            );
            assert_eq!(a.is_integer(), *is_int, "is_integer for {vt:?}");
        }
    }

    #[test]
    fn test_problem_sense_and_name() {
        let p = simple_problem();
        let c = p.to_lp_solvers().unwrap();
        assert!(matches!(lp_solvers::lp_format::LpProblem::sense(&c), LpObjective::Minimize));
        assert_eq!(lp_solvers::lp_format::LpProblem::name(&c), "lp_parser_problem");

        let mut p = simple_problem();
        p.sense = Sense::Maximize;
        p.name = Some("test".to_string());
        let c = p.to_lp_solvers().unwrap();
        assert!(matches!(lp_solvers::lp_format::LpProblem::sense(&c), LpObjective::Maximize));
        assert_eq!(lp_solvers::lp_format::LpProblem::name(&c), "test");
    }

    #[test]
    #[allow(clippy::many_single_char_names)]
    fn test_expression_formatting() {
        let mut p = LpProblem::new();
        let x = p.intern("x");
        let y = p.intern("y");
        let z = p.intern("z");

        let c = |name: NameId, v: f64| Coefficient { name, value: v };
        assert_eq!(expr_fmt(&p, &[]), "0");
        assert_eq!(expr_fmt(&p, &[c(x, 0.0), c(y, 0.0)]), "0");
        assert_eq!(expr_fmt(&p, &[c(x, f64::NAN), c(y, 2.0)]), "2 y");
        assert_eq!(expr_fmt(&p, &[c(x, f64::INFINITY)]), "0");
        assert_eq!(expr_fmt(&p, &[c(x, 1.0)]), "x");
        assert_eq!(expr_fmt(&p, &[c(x, -1.0)]), "- x");
        assert_eq!(expr_fmt(&p, &[c(x, 2.0), c(y, -3.0), c(z, 1.0)]), "2 x - 3 y + z");
        assert_eq!(expr_fmt(&p, &[c(x, 0.0), c(y, 2.0)]), "2 y");
    }
}
