//! Core data structures for representing Linear Programming problems.
//!
//! This module contains the fundamental types used to represent various
//! components of a Linear Programming problem, including variables,
//! constraints, objectives, and their associated properties.
//!
//! All name strings (variables, constraints, objectives) are stored in a
//! [`NameInterner`](crate::interner::NameInterner) and referenced by
//! [`NameId`].

use std::fmt::{Display, Formatter, Result as FmtResult};

use crate::interner::NameId;

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Represents comparison operations that can be used to compare values.
pub enum ComparisonOp {
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

impl ComparisonOp {
    const fn as_str(self) -> &'static str {
        match self {
            Self::GT => ">",
            Self::GTE => ">=",
            Self::EQ => "=",
            Self::LT => "<",
            Self::LTE => "<=",
        }
    }
}

impl AsRef<[u8]> for ComparisonOp {
    fn as_ref(&self) -> &[u8] {
        self.as_str().as_bytes()
    }
}

impl Display for ComparisonOp {
    #[inline]
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        f.write_str(self.as_str())
    }
}

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Default, Clone, PartialEq, Eq)]
/// Represents the optimisation sense for an objective function.
pub enum Sense {
    /// Minimise the objective function.
    #[default]
    Minimize,
    /// Maximise the objective function.
    Maximize,
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

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
/// Represents the type of SOS (Special Ordered Set) with variants `S1` and `S2`.
pub enum SOSType {
    #[default]
    /// At most one variable in the set can be non-zero.
    S1,
    /// At most two adjacent variables (in terms of weights) can be non-zero.
    S2,
}

impl SOSType {
    const fn as_str(self) -> &'static str {
        match self {
            Self::S1 => "S1",
            Self::S2 => "S2",
        }
    }
}

impl AsRef<[u8]> for SOSType {
    fn as_ref(&self) -> &[u8] {
        self.as_str().as_bytes()
    }
}

impl Display for SOSType {
    #[inline]
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        f.write_str(self.as_str())
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

#[derive(Debug, Clone)]
/// Represents a constraint in an optimisation problem, which can be either a
/// standard linear constraint or a special ordered set (SOS) constraint.
pub enum Constraint {
    /// A linear constraint defined by a name, coefficients, comparison operator, and RHS value.
    Standard {
        /// Interned constraint name.
        name: NameId,
        /// Left-hand-side coefficients.
        coefficients: Vec<Coefficient>,
        /// Comparison operator between the LHS and the RHS.
        operator: ComparisonOp,
        /// Right-hand-side value.
        rhs: f64,
        /// Byte offset of this constraint in the source text (for line number mapping).
        byte_offset: Option<usize>,
    },
    /// A special ordered set constraint defined by a name, SOS type, and weights.
    SOS {
        /// Interned constraint name.
        name: NameId,
        /// SOS type (S1 or S2).
        sos_type: SOSType,
        /// Weight per participating variable.
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
    /// Constant term of the objective function.
    pub constant: f64,
    /// Byte offset of this objective in the source text (for line number mapping).
    pub byte_offset: Option<usize>,
}

/// Discrete / structural kind of a decision variable (independent of bounds).
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum VariableKind {
    /// Continuous variable (default).
    #[default]
    Continuous,
    /// General integer variable (LP Generals section; typically non-negative integer).
    General,
    /// Integer variable.
    Integer,
    /// Binary (0/1) variable.
    Binary,
    /// Semi-continuous variable.
    SemiContinuous,
    /// Variable participating in an SOS set.
    Sos,
}

impl VariableKind {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Continuous => "Continuous",
            Self::General => "General",
            Self::Integer => "Integer",
            Self::Binary => "Binary",
            Self::SemiContinuous => "SemiContinuous",
            Self::Sos => "Sos",
        }
    }

    /// Whether this kind is treated as integer-valued by solvers.
    #[must_use]
    pub const fn is_integer(self) -> bool {
        matches!(self, Self::Integer | Self::General | Self::Binary)
    }
}

impl Display for VariableKind {
    #[inline]
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        f.write_str(self.as_str())
    }
}

/// Finite bounds on a variable. `None` on a side means unbounded on that side.
///
/// Free variables have both sides `None`. A fixed variable has `lower == upper`.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct VariableBounds {
    /// Lower bound, if finite.
    pub lower: Option<f64>,
    /// Upper bound, if finite.
    pub upper: Option<f64>,
}

impl VariableBounds {
    /// Fully free (unbounded) bounds.
    #[must_use]
    pub const fn free() -> Self {
        Self { lower: None, upper: None }
    }

    /// Lower-bounded only (`x >= lb`).
    #[must_use]
    pub const fn lower(lb: f64) -> Self {
        Self { lower: Some(lb), upper: None }
    }

    /// Upper-bounded only (`x <= ub`).
    #[must_use]
    pub const fn upper(ub: f64) -> Self {
        Self { lower: None, upper: Some(ub) }
    }

    /// Double-sided bounds (`lb <= x <= ub`).
    #[must_use]
    pub const fn range(lb: f64, ub: f64) -> Self {
        Self { lower: Some(lb), upper: Some(ub) }
    }

    /// Whether both sides are unbounded.
    #[must_use]
    pub const fn is_free(self) -> bool {
        self.lower.is_none() && self.upper.is_none()
    }

    /// Merge a new bound declaration into existing bounds.
    ///
    /// Complementary single-sided bounds combine; when both sides are already
    /// set (or the new declaration is free), the new declaration wins.
    #[must_use]
    pub const fn merge(self, new: Self) -> Self {
        match (self.lower, self.upper, new.lower, new.upper) {
            // Existing lower-only + new upper-only (or vice versa) → range
            (Some(lb), None, None, Some(ub)) | (None, Some(ub), Some(lb), None) => Self::range(lb, ub),
            // Free existing: take new entirely
            (None, None, _, _) => new,
            // New free: take free
            (_, _, None, None) => Self::free(),
            // Otherwise last-declaration-wins
            _ => new,
        }
    }

    /// Default lower bound assumed by solvers when none is set for integer-like kinds.
    ///
    /// A binary variable is canonically `[0, 1]`, so any explicit (redundant or
    /// contradictory) bound stored alongside the `Binary` kind is ignored here.
    #[must_use]
    pub const fn effective_lower(self, kind: VariableKind) -> f64 {
        if matches!(kind, VariableKind::Binary) {
            return 0.0;
        }
        if let Some(lb) = self.lower {
            return lb;
        }
        match kind {
            VariableKind::Continuous | VariableKind::Sos if self.is_free() => f64::NEG_INFINITY,
            _ => 0.0,
        }
    }

    /// Default upper bound assumed by solvers when none is set.
    ///
    /// A binary variable is canonically `[0, 1]` (see [`Self::effective_lower`]).
    #[must_use]
    pub fn effective_upper(self, kind: VariableKind) -> f64 {
        if matches!(kind, VariableKind::Binary) {
            return 1.0;
        }
        self.upper.unwrap_or(f64::INFINITY)
    }
}

impl Display for VariableBounds {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        match (self.lower, self.upper) {
            (None, None) => f.write_str("free"),
            (Some(lb), None) => write!(f, ">= {lb}"),
            (None, Some(ub)) => write!(f, "<= {ub}"),
            (Some(lb), Some(ub)) => write!(f, "{lb} .. {ub}"),
        }
    }
}

/// Intermediate / legacy bound-or-type declaration used by the LP grammar and
/// MPS builders when assembling a [`ParseResult`](crate::lexer::ParseResult).
///
/// Prefer [`VariableKind`] + [`VariableBounds`] on the public [`Variable`] model.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, Default, PartialEq)]
pub enum VariableType {
    #[default]
    /// Unbounded variable (-Infinity, +Infinity)
    Free,
    /// General integer variable [0, +Infinity]
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

impl VariableType {
    const fn as_str(&self) -> &'static str {
        match self {
            Self::Free => "Free",
            Self::General => "General",
            Self::LowerBound(_) => "LowerBound",
            Self::UpperBound(_) => "UpperBound",
            Self::DoubleBound(_, _) => "DoubleBound",
            Self::Binary => "Binary",
            Self::Integer => "Integer",
            Self::SemiContinuous => "Semi-Continuous",
            Self::SOS => "SOS",
        }
    }

    /// Split into kind and bounds components.
    #[must_use]
    pub const fn into_kind_and_bounds(self) -> (VariableKind, VariableBounds) {
        match self {
            Self::Free => (VariableKind::Continuous, VariableBounds::free()),
            Self::General => (VariableKind::General, VariableBounds::free()),
            Self::LowerBound(lb) => (VariableKind::Continuous, VariableBounds::lower(lb)),
            Self::UpperBound(ub) => (VariableKind::Continuous, VariableBounds::upper(ub)),
            Self::DoubleBound(lb, ub) => (VariableKind::Continuous, VariableBounds::range(lb, ub)),
            Self::Binary => (VariableKind::Binary, VariableBounds::free()),
            Self::Integer => (VariableKind::Integer, VariableBounds::free()),
            Self::SemiContinuous => (VariableKind::SemiContinuous, VariableBounds::free()),
            Self::SOS => (VariableKind::Sos, VariableBounds::free()),
        }
    }

    /// Reconstruct a legacy `VariableType` from kind + bounds (lossy for mixed cases).
    #[must_use]
    pub const fn from_kind_and_bounds(kind: VariableKind, bounds: VariableBounds) -> Self {
        // Prefer kind for discrete types when bounds are free; otherwise prefer bounds shape.
        match kind {
            VariableKind::Binary if bounds.is_free() => Self::Binary,
            VariableKind::Integer if bounds.is_free() => Self::Integer,
            VariableKind::General if bounds.is_free() => Self::General,
            VariableKind::SemiContinuous if bounds.is_free() => Self::SemiContinuous,
            VariableKind::Sos if bounds.is_free() => Self::SOS,
            _ => match (bounds.lower, bounds.upper) {
                (None, None) => match kind {
                    VariableKind::Binary => Self::Binary,
                    VariableKind::Integer => Self::Integer,
                    VariableKind::General => Self::General,
                    VariableKind::SemiContinuous => Self::SemiContinuous,
                    VariableKind::Sos => Self::SOS,
                    VariableKind::Continuous => Self::Free,
                },
                (Some(lb), None) => Self::LowerBound(lb),
                (None, Some(ub)) => Self::UpperBound(ub),
                (Some(lb), Some(ub)) => Self::DoubleBound(lb, ub),
            },
        }
    }
}

impl AsRef<[u8]> for VariableType {
    fn as_ref(&self) -> &[u8] {
        self.as_str().as_bytes()
    }
}

impl Display for VariableType {
    #[inline]
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq)]
/// Represents a variable in a Linear Programming problem.
pub struct Variable {
    /// Interned name of the variable.
    pub name: NameId,
    /// Discrete / structural kind.
    pub kind: VariableKind,
    /// Finite bounds (independent of kind).
    pub bounds: VariableBounds,
}

impl Variable {
    #[must_use]
    #[inline]
    /// Initialise a new continuous free `Variable`.
    pub const fn new(name: NameId) -> Self {
        Self { name, kind: VariableKind::Continuous, bounds: VariableBounds::free() }
    }

    /// Legacy view as a single [`VariableType`] (lossy when kind and bounds both set).
    #[must_use]
    pub const fn var_type(&self) -> VariableType {
        VariableType::from_kind_and_bounds(self.kind, self.bounds)
    }

    #[inline]
    /// Set kind and bounds from a legacy [`VariableType`].
    pub const fn set_var_type(&mut self, var_type: VariableType) {
        let (kind, bounds) = var_type.into_kind_and_bounds();
        self.kind = kind;
        self.bounds = bounds;
    }

    #[must_use]
    #[inline]
    /// Builder: set kind and bounds from a legacy [`VariableType`].
    pub const fn with_var_type(mut self, var_type: VariableType) -> Self {
        self.set_var_type(var_type);
        self
    }

    #[must_use]
    #[inline]
    /// Builder: set the variable kind.
    pub const fn with_kind(mut self, kind: VariableKind) -> Self {
        self.kind = kind;
        self
    }

    #[must_use]
    #[inline]
    /// Builder: set the variable bounds.
    pub const fn with_bounds(mut self, bounds: VariableBounds) -> Self {
        self.bounds = bounds;
        self
    }

    #[inline]
    /// Set the variable kind without changing bounds.
    pub const fn set_kind(&mut self, kind: VariableKind) {
        self.kind = kind;
    }

    #[inline]
    /// Set the variable bounds without changing kind.
    pub const fn set_bounds(&mut self, bounds: VariableBounds) {
        self.bounds = bounds;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::interner::NameInterner;

    #[test]
    fn test_comparison_op() {
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
    // The stored value must round-trip bit-exactly, so compare floats strictly.
    #[allow(clippy::float_cmp)]
    fn test_coefficient() {
        let mut interner = NameInterner::new();
        let x1 = interner.intern("x1");

        let coeff = Coefficient { name: x1, value: 2.5 };
        assert_eq!(interner.resolve(coeff.name), "x1");
        assert_eq!(coeff.value, 2.5);
        assert_eq!(coeff.clone(), coeff);
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

        let obj = Objective { name: profit, coefficients: vec![Coefficient { name: x1, value: 5.0 }], constant: 0.0, byte_offset: None };
        assert_eq!(interner.resolve(obj.name), "profit");
        assert_eq!(obj.coefficients.len(), 1);

        let dynamic = interner.intern("dynamic");
        let obj_empty = Objective { name: dynamic, coefficients: vec![], constant: 0.0, byte_offset: None };
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
        assert_eq!(var.kind, VariableKind::Continuous);
        assert!(var.bounds.is_free());
        assert_eq!(var.var_type(), VariableType::Free);

        let var_binary = Variable::new(x).with_var_type(VariableType::Binary);
        assert_eq!(var_binary.kind, VariableKind::Binary);
        assert_eq!(var_binary.var_type(), VariableType::Binary);

        let mut var_mut = Variable::new(y);
        var_mut.set_var_type(VariableType::Integer);
        assert_eq!(var_mut.kind, VariableKind::Integer);

        let mixed = Variable::new(x).with_kind(VariableKind::Integer).with_bounds(VariableBounds::range(0.0, 10.0));
        assert_eq!(mixed.kind, VariableKind::Integer);
        assert_eq!(mixed.bounds, VariableBounds::range(0.0, 10.0));

        assert_eq!(Variable::new(x).with_var_type(VariableType::Binary), Variable::new(x).with_var_type(VariableType::Binary));
        assert_ne!(Variable::new(x).with_var_type(VariableType::Binary), Variable::new(y).with_var_type(VariableType::Binary));
    }

    #[test]
    fn test_bounds_merge() {
        let merged = VariableBounds::lower(2.0).merge(VariableBounds::upper(8.0));
        assert_eq!(merged, VariableBounds::range(2.0, 8.0));
        let free = VariableBounds::range(0.0, 1.0).merge(VariableBounds::free());
        assert!(free.is_free());
    }
}
