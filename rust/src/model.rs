//! Core data structures for representing Linear Programming problems.
//!
//! This module contains the fundamental types used to represent various
//! components of a Linear Programming problem, including variables,
//! constraints, objectives, and their associated properties.
//!
//! All name strings (variables, constraints, objectives) are stored in a
//! [`NameInterner`](crate::interner::NameInterner) and referenced by
//! [`NameId`](crate::interner::NameId).

// Allow float_cmp in this module because the diff::Diff derive macro generates
// code that uses direct f64 comparisons which we can't annotate
#![allow(clippy::float_cmp)]

use std::fmt::{Display, Formatter, Result as FmtResult};

use crate::interner::NameId;

#[cfg_attr(feature = "diff", derive(diff::Diff), diff(attr(#[derive(Debug, PartialEq, Eq)])))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Default, Clone, PartialEq, Eq)]
/// Represents comparison operations that can be used to compare values.
pub enum ComparisonOp {
    #[default]
    /// Greater than
    GT,
    /// Greater than or equal
    GTE,
    /// Equals
    EQ,
    /// Less than
    LT,
    /// Less than or equal
    LTE,
}

impl AsRef<[u8]> for ComparisonOp {
    fn as_ref(&self) -> &[u8] {
        match self {
            Self::GT => b">",
            Self::GTE => b">=",
            Self::EQ => b"=",
            Self::LT => b"<",
            Self::LTE => b"<=",
        }
    }
}

impl Display for ComparisonOp {
    #[inline]
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        match self {
            Self::GT => write!(f, ">"),
            Self::GTE => write!(f, ">="),
            Self::EQ => write!(f, "="),
            Self::LT => write!(f, "<"),
            Self::LTE => write!(f, "<="),
        }
    }
}

#[cfg_attr(feature = "diff", derive(diff::Diff), diff(attr(#[derive(Debug, PartialEq, Eq)])))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Default, Clone, PartialEq, Eq)]
/// Represents the optimisation sense for an objective function.
pub enum Sense {
    #[default]
    Minimize,
    Maximize,
}

impl Sense {
    #[inline]
    #[must_use]
    /// Determines if the current optimisation sense is minimisation.
    pub const fn is_minimisation(&self) -> bool {
        matches!(self, Self::Minimize)
    }
}

impl Display for Sense {
    #[inline]
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        match self {
            Self::Minimize => write!(f, "Minimize"),
            Self::Maximize => write!(f, "Maximize"),
        }
    }
}

#[cfg_attr(feature = "diff", derive(diff::Diff), diff(attr(#[derive(Debug, PartialEq, Eq)])))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, Default, PartialEq, Eq)]
/// Represents the type of SOS (Special Ordered Set) with variants `S1` and `S2`.
pub enum SOSType {
    #[default]
    /// At most one variable in the set can be non-zero.
    S1,
    /// At most two adjacent variables (in terms of weights) can be non-zero.
    S2,
}

impl AsRef<[u8]> for SOSType {
    fn as_ref(&self) -> &[u8] {
        match self {
            Self::S1 => b"S1",
            Self::S2 => b"S2",
        }
    }
}

impl Display for SOSType {
    #[inline]
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        match self {
            Self::S1 => write!(f, "S1"),
            Self::S2 => write!(f, "S2"),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
/// Represents a coefficient associated with a variable name.
pub struct Coefficient {
    /// Interned name of the variable.
    pub name: NameId,
    /// The coefficient value.
    pub value: f64,
}

/// Format a coefficient value with a resolved variable name for display.
/// Uses `NUMERIC_EPSILON` for consistent tolerance across the crate.
///
/// # Errors
///
/// Returns `fmt::Error` if the underlying write fails.
#[inline]
pub fn fmt_coefficient(name: &str, value: f64, f: &mut Formatter<'_>) -> FmtResult {
    if (value - 1.0).abs() < crate::NUMERIC_EPSILON {
        write!(f, "{name}")
    } else if (value - (-1.0)).abs() < crate::NUMERIC_EPSILON {
        write!(f, "-{name}")
    } else {
        write!(f, "{value} {name}")
    }
}

#[derive(Debug, Clone)]
/// Represents a constraint in an optimisation problem, which can be either a
/// standard linear constraint or a special ordered set (SOS) constraint.
pub enum Constraint {
    /// A linear constraint defined by a name, coefficients, comparison operator, and RHS value.
    Standard {
        /// Interned constraint name.
        name: NameId,
        coefficients: Vec<Coefficient>,
        operator: ComparisonOp,
        rhs: f64,
        /// Byte offset of this constraint in the source text (for line number mapping).
        byte_offset: Option<usize>,
    },
    /// A special ordered set constraint defined by a name, SOS type, and weights.
    SOS {
        /// Interned constraint name.
        name: NameId,
        sos_type: SOSType,
        weights: Vec<Coefficient>,
        /// Byte offset of this constraint in the source text (for line number mapping).
        byte_offset: Option<usize>,
    },
}

impl PartialEq for Constraint {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (
                Self::Standard { name: n1, coefficients: c1, operator: o1, rhs: r1, .. },
                Self::Standard { name: n2, coefficients: c2, operator: o2, rhs: r2, .. },
            ) => n1 == n2 && c1 == c2 && o1 == o2 && r1 == r2,
            (Self::SOS { name: n1, sos_type: t1, weights: w1, .. }, Self::SOS { name: n2, sos_type: t2, weights: w2, .. }) => {
                n1 == n2 && t1 == t2 && w1 == w2
            }
            _ => false,
        }
    }
}

impl Constraint {
    #[must_use]
    #[inline]
    /// Returns the interned name of the constraint.
    pub const fn name(&self) -> NameId {
        match self {
            Self::Standard { name, .. } | Self::SOS { name, .. } => *name,
        }
    }

    #[must_use]
    #[inline]
    /// Returns the byte offset of this constraint in the source text, if available.
    pub const fn byte_offset(&self) -> Option<usize> {
        match self {
            Self::Standard { byte_offset, .. } | Self::SOS { byte_offset, .. } => *byte_offset,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
/// Represents an optimisation objective with a name and a list of coefficients.
pub struct Objective {
    /// Interned name of the objective.
    pub name: NameId,
    /// Coefficients associated with the objective.
    pub coefficients: Vec<Coefficient>,
}

#[cfg_attr(feature = "diff", derive(diff::Diff), diff(attr(#[derive(Debug, PartialEq)])))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, Default, PartialEq)]
/// Represents different types of variables that can be used in optimisation models.
pub enum VariableType {
    #[default]
    /// Unbounded variable (-Infinity, +Infinity)
    Free,
    /// General variable [0, +Infinity]
    General,
    /// Variable with a lower bound (`x >= lb`).
    LowerBound(f64),
    /// Variable with an upper bound (`x <= ub`).
    UpperBound(f64),
    /// Variable with both lower and upper bounds (`lb <= x <= ub`).
    DoubleBound(f64, f64),
    /// Binary variable.
    Binary,
    /// Integer variable.
    Integer,
    /// Semi-continuous variable.
    SemiContinuous,
    /// Special Order Set (SOS)
    SOS,
}

impl AsRef<[u8]> for VariableType {
    fn as_ref(&self) -> &[u8] {
        match self {
            Self::Free => b"Free",
            Self::General => b"General",
            Self::LowerBound(_) => b"LowerBound",
            Self::UpperBound(_) => b"UpperBound",
            Self::DoubleBound(_, _) => b"DoubleBound",
            Self::Binary => b"Binary",
            Self::Integer => b"Integer",
            Self::SemiContinuous => b"Semi-Continuous",
            Self::SOS => b"SOS",
        }
    }
}

impl Display for VariableType {
    #[inline]
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        match self {
            Self::Free => write!(f, "Free"),
            Self::General => write!(f, "General"),
            Self::LowerBound(_) => write!(f, "LowerBound"),
            Self::UpperBound(_) => write!(f, "UpperBound"),
            Self::DoubleBound(_, _) => write!(f, "DoubleBound"),
            Self::Binary => write!(f, "Binary"),
            Self::Integer => write!(f, "Integer"),
            Self::SemiContinuous => write!(f, "Semi-Continuous"),
            Self::SOS => write!(f, "SOS"),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
/// Represents a variable in a Linear Programming problem.
pub struct Variable {
    /// Interned name of the variable.
    pub name: NameId,
    /// The type of the variable.
    pub var_type: VariableType,
}

impl Variable {
    #[must_use]
    #[inline]
    /// Initialise a new `Variable` with default (Free) type.
    pub fn new(name: NameId) -> Self {
        Self { name, var_type: VariableType::default() }
    }

    #[inline]
    /// Setter to override `VariableType`.
    pub const fn set_var_type(&mut self, var_type: VariableType) {
        self.var_type = var_type;
    }

    #[must_use]
    #[inline]
    /// Builder method for constructing a `Variable` with a non-default `VariableType`.
    pub const fn with_var_type(self, var_type: VariableType) -> Self {
        Self { var_type, ..self }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::interner::NameInterner;

    #[test]
    fn test_comparison_op() {
        assert_eq!(ComparisonOp::default(), ComparisonOp::GT);

        let test_cases = [
            (ComparisonOp::GT, b">".as_slice(), ">"),
            (ComparisonOp::GTE, b">=".as_slice(), ">="),
            (ComparisonOp::EQ, b"=".as_slice(), "="),
            (ComparisonOp::LT, b"<".as_slice(), "<"),
            (ComparisonOp::LTE, b"<=".as_slice(), "<="),
        ];

        for (op, expected_bytes, expected_str) in test_cases {
            assert_eq!(op.as_ref(), expected_bytes);
            assert_eq!(format!("{op}"), expected_str);
            assert_eq!(op.clone(), op);
        }
    }

    #[test]
    fn test_sense() {
        assert_eq!(Sense::default(), Sense::Minimize);
        assert!(Sense::Minimize.is_minimisation());
        assert!(!Sense::Maximize.is_minimisation());
        assert_eq!(format!("{}", Sense::Minimize), "Minimize");
        assert_eq!(format!("{}", Sense::Maximize), "Maximize");
        assert_ne!(Sense::Minimize, Sense::Maximize);
    }

    #[test]
    fn test_sos_type() {
        let test_cases = [(SOSType::S1, b"S1".as_slice(), "S1"), (SOSType::S2, b"S2".as_slice(), "S2")];

        for (sos, expected_bytes, expected_str) in test_cases {
            assert_eq!(sos.as_ref(), expected_bytes);
            assert_eq!(format!("{sos}"), expected_str);
            assert_eq!(sos.clone(), sos);
        }
        assert_ne!(SOSType::S1, SOSType::S2);
    }

    #[test]
    fn test_coefficient() {
        use std::fmt::Write;

        let mut interner = NameInterner::new();
        let x1 = interner.intern("x1");
        let x = interner.intern("x");

        let coeff = Coefficient { name: x1, value: 2.5 };
        assert_eq!(interner.resolve(coeff.name), "x1");
        assert_eq!(coeff.value, 2.5);
        assert_eq!(coeff.clone(), coeff);

        // fmt_coefficient display special cases
        let mut buf = String::new();
        write!(buf, "{}", FmtCoeff { name: "x", value: 1.0 }).unwrap();
        assert_eq!(buf, "x");
        buf.clear();
        write!(buf, "{}", FmtCoeff { name: "x", value: -1.0 }).unwrap();
        assert_eq!(buf, "-x");
        buf.clear();
        write!(buf, "{}", FmtCoeff { name: "x", value: 2.5 }).unwrap();
        assert_eq!(buf, "2.5 x");
        buf.clear();
        write!(buf, "{}", FmtCoeff { name: "x", value: 0.0 }).unwrap();
        assert_eq!(buf, "0 x");

        // Verify x is used (suppress unused warning)
        let _ = x;
    }

    /// Helper for testing `fmt_coefficient`.
    struct FmtCoeff {
        name: &'static str,
        value: f64,
    }

    impl Display for FmtCoeff {
        fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
            fmt_coefficient(self.name, self.value, f)
        }
    }

    #[test]
    fn test_constraint() {
        let mut interner = NameInterner::new();
        let c1 = interner.intern("c1");
        let x1 = interner.intern("x1");
        let x2 = interner.intern("x2");
        let sos1 = interner.intern("sos1");

        let std_constraint = Constraint::Standard {
            name: c1,
            coefficients: vec![Coefficient { name: x1, value: 2.0 }, Coefficient { name: x2, value: -1.0 }],
            operator: ComparisonOp::LTE,
            rhs: 10.0,
            byte_offset: None,
        };
        assert_eq!(std_constraint.name(), c1);
        assert_eq!(interner.resolve(std_constraint.name()), "c1");

        let sos_constraint =
            Constraint::SOS { name: sos1, sos_type: SOSType::S1, weights: vec![Coefficient { name: x1, value: 1.0 }], byte_offset: None };
        assert_eq!(sos_constraint.name(), sos1);
        assert_eq!(interner.resolve(sos_constraint.name()), "sos1");

        // Empty coefficients
        let e = interner.intern("e");
        let empty = Constraint::Standard { name: e, coefficients: vec![], operator: ComparisonOp::EQ, rhs: 0.0, byte_offset: None };
        if let Constraint::Standard { coefficients, .. } = empty {
            assert!(coefficients.is_empty());
        }
    }

    #[test]
    fn test_objective() {
        let mut interner = NameInterner::new();
        let profit = interner.intern("profit");
        let x1 = interner.intern("x1");

        let obj = Objective { name: profit, coefficients: vec![Coefficient { name: x1, value: 5.0 }] };
        assert_eq!(interner.resolve(obj.name), "profit");
        assert_eq!(obj.coefficients.len(), 1);

        let dynamic = interner.intern("dynamic");
        let obj_empty = Objective { name: dynamic, coefficients: vec![] };
        assert_eq!(interner.resolve(obj_empty.name), "dynamic");
        assert!(obj_empty.coefficients.is_empty());
    }

    #[test]
    fn test_variable_type() {
        assert_eq!(VariableType::default(), VariableType::Free);

        let test_cases = [
            (VariableType::Free, b"Free".as_slice(), "Free"),
            (VariableType::General, b"General".as_slice(), "General"),
            (VariableType::Binary, b"Binary".as_slice(), "Binary"),
            (VariableType::Integer, b"Integer".as_slice(), "Integer"),
            (VariableType::SemiContinuous, b"Semi-Continuous".as_slice(), "Semi-Continuous"),
            (VariableType::SOS, b"SOS".as_slice(), "SOS"),
            (VariableType::LowerBound(5.0), b"LowerBound".as_slice(), "LowerBound"),
            (VariableType::UpperBound(10.0), b"UpperBound".as_slice(), "UpperBound"),
            (VariableType::DoubleBound(0.0, 100.0), b"DoubleBound".as_slice(), "DoubleBound"),
        ];

        for (vt, expected_bytes, expected_str) in test_cases {
            assert_eq!(vt.as_ref(), expected_bytes);
            assert_eq!(format!("{vt}"), expected_str);
            assert_eq!(vt.clone(), vt);
        }

        if let VariableType::DoubleBound(l, u) = VariableType::DoubleBound(0.0, 100.0) {
            assert_eq!((l, u), (0.0, 100.0));
        }
    }

    #[test]
    fn test_variable() {
        let mut interner = NameInterner::new();
        let x1 = interner.intern("x1");
        let x = interner.intern("x");
        let y = interner.intern("y");

        let var = Variable::new(x1);
        assert_eq!(interner.resolve(var.name), "x1");
        assert_eq!(var.var_type, VariableType::Free);

        let var_binary = Variable::new(x).with_var_type(VariableType::Binary);
        assert_eq!(var_binary.var_type, VariableType::Binary);

        let mut var_mut = Variable::new(y);
        var_mut.set_var_type(VariableType::Integer);
        assert_eq!(var_mut.var_type, VariableType::Integer);

        assert_eq!(Variable::new(x).with_var_type(VariableType::Binary), Variable::new(x).with_var_type(VariableType::Binary));
        assert_ne!(Variable::new(x).with_var_type(VariableType::Binary), Variable::new(y).with_var_type(VariableType::Binary));
    }
}
