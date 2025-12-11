//! Core data structures for representing Linear Programming problems.
//!
//! This module contains the fundamental types used to represent various
//! components of a Linear Programming problem, including variables,
//! constraints, objectives, and their associated properties.
//!
//! - `ComparisonOp`: Enum for comparison operations like greater than, less than, etc.
//! - `Sense`: Enum for optimisation sense, either minimisation or maximisation.
//! - `SOSType`: Enum for types of System of Systems (SOS), with variants `S1` and `S2`.
//! - `Coefficient`: Struct representing a coefficient associated with a variable name.
//! - `Constraint`: Enum representing a constraint in an optimisation problem, either standard or SOS.
//! - `Objective`: Struct representing an optimisation objective with a name and coefficients.
//! - `VariableType`: Enum for different types of variables in optimisation models.
//! - `Variable`: Struct representing a variable with a name and type.
//!

// Allow float_cmp in this module because the diff::Diff derive macro generates
// code that uses direct f64 comparisons which we can't annotate
#![allow(clippy::float_cmp)]

use std::borrow::Cow;

#[cfg_attr(feature = "diff", derive(diff::Diff), diff(attr(#[derive(Debug, PartialEq)])))]
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

impl std::fmt::Display for ComparisonOp {
    #[inline]
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::GT => write!(f, ">"),
            Self::GTE => write!(f, ">="),
            Self::EQ => write!(f, "="),
            Self::LT => write!(f, "<"),
            Self::LTE => write!(f, "<="),
        }
    }
}

#[cfg_attr(feature = "diff", derive(diff::Diff), diff(attr(#[derive(Debug, PartialEq)])))]
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

impl std::fmt::Display for Sense {
    #[inline]
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Minimize => write!(f, "Minimize"),
            Self::Maximize => write!(f, "Maximize"),
        }
    }
}

#[cfg_attr(feature = "diff", derive(diff::Diff), diff(attr(#[derive(Debug, PartialEq)])))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, Default, PartialEq, Eq)]
/// Represents the type of SOS (System of Systems) with variants `S1` and `S2`.
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

impl std::fmt::Display for SOSType {
    #[inline]
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::S1 => write!(f, "S1"),
            Self::S2 => write!(f, "S2"),
        }
    }
}

#[cfg_attr(feature = "diff", derive(diff::Diff), diff(attr(#[derive(Debug, PartialEq)])))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, PartialEq)]
/// Represents a coefficient associated with a variable name.
pub struct Coefficient<'a> {
    /// A string slice representing the name of the variable.
    pub name: &'a str,
    /// A floating-point number representing the coefficient value.
    pub value: f64,
}

impl std::fmt::Display for Coefficient<'_> {
    #[inline]
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if (self.value - 1.0).abs() < f64::EPSILON {
            write!(f, "{}", self.name)
        } else if (self.value - (-1.0)).abs() < f64::EPSILON {
            write!(f, "-{}", self.name)
        } else {
            write!(f, "{} {}", self.value, self.name)
        }
    }
}

#[cfg_attr(feature = "diff", derive(diff::Diff), diff(attr(#[derive(Debug, PartialEq)])))]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[cfg_attr(feature = "serde", serde(tag = "type"))]
#[derive(Debug, Clone, PartialEq)]
/// Represents a constraint in an optimisation problem, which can be either a
/// standard linear constraint or a special ordered set (SOS) constraint.
///
/// # Attributes
///
/// * `name` - The name of the constraint.
/// * `coefficients` - A vector of coefficients for the standard constraint.
/// * `operator` - The comparison operator for the standard constraint.
/// * `rhs` - The right-hand side value for the standard constraint.
/// * `sos_type` - The type of SOS for the SOS constraint.
/// * `weights` - A vector of weights for the SOS constraint.
///
pub enum Constraint<'a> {
    /// A linear constraint defined by a name, a vector of coefficients, a comparison operator, and a right-hand side value.
    Standard { name: Cow<'a, str>, coefficients: Vec<Coefficient<'a>>, operator: ComparisonOp, rhs: f64 },
    /// A special ordered set constraint defined by a name, a type of SOS and a vector of weights.
    SOS { name: Cow<'a, str>, sos_type: SOSType, weights: Vec<Coefficient<'a>> },
}

impl<'a> Constraint<'a> {
    #[must_use]
    #[inline]
    /// Returns the name of the constraint as a `Cow<str>`.
    pub fn name(&'a self) -> Cow<'a, str> {
        match self {
            Constraint::Standard { name, .. } | Constraint::SOS { name, .. } => name.clone(),
        }
    }
}

impl std::fmt::Display for Constraint<'_> {
    #[inline]
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Constraint::Standard { name, coefficients, operator, rhs } => {
                write!(f, "{name}: ")?;
                for (i, coef) in coefficients.iter().enumerate() {
                    if i > 0 && coef.value > 0.0 {
                        write!(f, "+ ")?;
                    }
                    write!(f, "{coef} ")?;
                }
                write!(f, "{operator} {rhs}")
            }
            Constraint::SOS { name, sos_type, weights } => {
                write!(f, "{name}: {sos_type}:: ")?;
                for (i, weight) in weights.iter().enumerate() {
                    if i > 0 {
                        write!(f, " ")?;
                    }
                    write!(f, "{}:{}", weight.name, weight.value)?;
                }
                Ok(())
            }
        }
    }
}

#[cfg_attr(feature = "diff", derive(diff::Diff), diff(attr(#[derive(Debug, PartialEq)])))]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[derive(Debug, Clone, PartialEq)]
/// Represents an optimisation objective with a name and a list of coefficients.
///
/// This struct can optionally derive `Diff` for change tracking and `Serialize`
/// for serialisation, depending on the enabled features.
pub struct Objective<'a> {
    /// A borrowed string representing the name of the objective.
    pub name: Cow<'a, str>,
    /// A vector of `Coefficient` instances associated with the objective.
    pub coefficients: Vec<Coefficient<'a>>,
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
    /// Variable with an upper bound (`x ≤ ub`).
    UpperBound(f64),
    /// Variable with both lower and upper bounds (`lb ≤ x ≤ ub`).
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

impl std::fmt::Display for VariableType {
    #[inline]
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
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

#[cfg_attr(feature = "diff", derive(diff::Diff), diff(attr(#[derive(Debug, PartialEq)])))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, PartialEq)]
/// Represents a variable in a Linear Programming problem.
///
/// Variables are the fundamental building blocks of LP problems,
/// representing the quantities to be optimized.
///
/// # Examples
///
/// ```rust
/// use lp_parser::model::{Variable, VariableType};
///
/// // Create a free variable
/// let x = Variable::new("x");
///
/// // Create a binary variable
/// let y = Variable::new("y")
///     .with_var_type(VariableType::Binary);
/// ```
///
pub struct Variable<'a> {
    /// A string slice that holds the name of the variable.
    pub name: &'a str,
    /// The type of the variable, represented by `VariableType`.
    pub var_type: VariableType,
}

impl<'a> Variable<'a> {
    #[must_use]
    #[inline]
    /// Initialise a new `Variable`.
    pub fn new(name: &'a str) -> Self {
        Self { name, var_type: VariableType::default() }
    }

    #[inline]
    /// Setter to override `VariableType`.
    pub fn set_var_type(&mut self, var_type: VariableType) {
        self.var_type = var_type;
    }

    #[must_use]
    #[inline]
    /// Builder method for constructing a `Variable` with a non-default `VariableType`.
    pub const fn with_var_type(self, var_type: VariableType) -> Self {
        Self { var_type, ..self }
    }
}

#[cfg(feature = "serde")]
impl<'de: 'a, 'a> serde::Deserialize<'de> for Constraint<'a> {
    #[inline]
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(PartialEq, serde::Deserialize)]
        #[serde(field_identifier, rename_all = "lowercase")]
        enum Field {
            Coefficients,
            Name,
            Operator,
            Rhs,
            #[serde(alias = "sos_type")]
            SosType,
            Type,
            Weights,
        }

        struct ConstraintVisitor<'a>(std::marker::PhantomData<Constraint<'a>>);

        impl<'de: 'a, 'a> serde::de::Visitor<'de> for ConstraintVisitor<'a> {
            type Value = Constraint<'a>;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("struct Constraint")
            }

            fn visit_map<V: serde::de::MapAccess<'de>>(self, mut map: V) -> Result<Constraint<'a>, V::Error> {
                let constraint_type: String = match map.next_key::<Field>()? {
                    Some(Field::Type) => map.next_value()?,
                    _ => return Err(serde::de::Error::missing_field("type")),
                };
                match constraint_type.as_str() {
                    "Standard" => {
                        let mut name = "";
                        let mut coefficients = None;
                        let mut operator = None;
                        let mut rhs = None;

                        while let Some(key) = map.next_key()? {
                            match key {
                                Field::Name => name = map.next_value()?,
                                Field::Coefficients => coefficients = Some(map.next_value()?),
                                Field::Operator => operator = Some(map.next_value()?),
                                Field::Rhs => rhs = Some(map.next_value()?),
                                Field::Type | Field::Weights | Field::SosType => {
                                    let _ = map.next_value::<serde::de::IgnoredAny>()?;
                                }
                            }
                        }

                        Ok(Constraint::Standard {
                            name: Cow::Borrowed(name),
                            coefficients: coefficients.ok_or_else(|| serde::de::Error::missing_field("coefficients"))?,
                            operator: operator.ok_or_else(|| serde::de::Error::missing_field("operator"))?,
                            rhs: rhs.ok_or_else(|| serde::de::Error::missing_field("rhs"))?,
                        })
                    }
                    "SOS" => {
                        let mut name = "";
                        let mut sos_type = None;
                        let mut weights = None;

                        while let Some(key) = map.next_key()? {
                            match key {
                                Field::Name => name = map.next_value()?,
                                Field::SosType => sos_type = Some(map.next_value()?),
                                Field::Weights => weights = Some(map.next_value()?),
                                Field::Type | Field::Coefficients | Field::Operator | Field::Rhs => {
                                    let _ = map.next_value::<serde::de::IgnoredAny>()?;
                                }
                            }
                        }

                        Ok(Constraint::SOS {
                            name: Cow::Borrowed(name),
                            sos_type: sos_type.ok_or_else(|| serde::de::Error::missing_field("sos_type"))?,
                            weights: weights.ok_or_else(|| serde::de::Error::missing_field("weights"))?,
                        })
                    }
                    _ => Err(serde::de::Error::unknown_variant(&constraint_type, &["Standard", "SOS"])),
                }
            }
        }

        const FIELDS: &[&str] = &["type", "name", "coefficients", "weights", "operator", "rhs", "sos_type"];
        deserializer.deserialize_struct("Constraint", FIELDS, ConstraintVisitor(std::marker::PhantomData))
    }
}

#[cfg(feature = "serde")]
impl<'de: 'a, 'a> serde::Deserialize<'de> for Objective<'a> {
    #[inline]
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(serde::Deserialize)]
        #[serde(field_identifier, rename_all = "lowercase")]
        enum Field {
            Coefficients,
            Name,
        }

        struct ObjectiveVisitor<'a>(std::marker::PhantomData<Objective<'a>>);

        impl<'de: 'a, 'a> serde::de::Visitor<'de> for ObjectiveVisitor<'a> {
            type Value = Objective<'a>;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("struct Objective")
            }

            fn visit_map<V: serde::de::MapAccess<'de>>(self, mut map: V) -> Result<Objective<'a>, V::Error> {
                let mut name = "";
                let mut coefficients = None;

                while let Some(key) = map.next_key()? {
                    match key {
                        Field::Name => name = map.next_value()?,
                        Field::Coefficients => coefficients = Some(map.next_value()?),
                    }
                }

                Ok(Objective {
                    name: Cow::Borrowed(name),
                    coefficients: coefficients.ok_or_else(|| serde::de::Error::missing_field("coefficients"))?,
                })
            }
        }

        deserializer.deserialize_struct("Objective", &["name", "coefficients"], ObjectiveVisitor(std::marker::PhantomData))
    }
}

/// Owned coefficient with no lifetime constraints.
///
/// Unlike [`Coefficient`], this type owns its variable name string,
/// making it suitable for long-lived data structures and mutation.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, PartialEq)]
pub struct CoefficientOwned {
    /// The name of the variable (owned).
    pub name: String,
    /// The coefficient value.
    pub value: f64,
}

impl<'a> From<&Coefficient<'a>> for CoefficientOwned {
    fn from(coeff: &Coefficient<'a>) -> Self {
        Self { name: coeff.name.to_string(), value: coeff.value }
    }
}

impl std::fmt::Display for CoefficientOwned {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if (self.value - 1.0).abs() < f64::EPSILON {
            write!(f, "{}", self.name)
        } else if (self.value - (-1.0)).abs() < f64::EPSILON {
            write!(f, "-{}", self.name)
        } else {
            write!(f, "{} {}", self.value, self.name)
        }
    }
}

/// Owned constraint with no lifetime constraints.
///
/// Unlike [`Constraint`], this type owns all its strings,
/// making it suitable for long-lived data structures and mutation.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(tag = "type"))]
#[derive(Debug, Clone, PartialEq)]
pub enum ConstraintOwned {
    /// A standard linear constraint with owned strings.
    Standard { name: String, coefficients: Vec<CoefficientOwned>, operator: ComparisonOp, rhs: f64 },
    /// An SOS constraint with owned strings.
    SOS { name: String, sos_type: SOSType, weights: Vec<CoefficientOwned> },
}

impl ConstraintOwned {
    /// Returns the name of the constraint.
    #[must_use]
    pub fn name(&self) -> &str {
        match self {
            Self::Standard { name, .. } | Self::SOS { name, .. } => name,
        }
    }
}

impl<'a> From<&Constraint<'a>> for ConstraintOwned {
    fn from(constraint: &Constraint<'a>) -> Self {
        match constraint {
            Constraint::Standard { name, coefficients, operator, rhs } => Self::Standard {
                name: name.to_string(),
                coefficients: coefficients.iter().map(CoefficientOwned::from).collect(),
                operator: operator.clone(),
                rhs: *rhs,
            },
            Constraint::SOS { name, sos_type, weights } => Self::SOS {
                name: name.to_string(),
                sos_type: sos_type.clone(),
                weights: weights.iter().map(CoefficientOwned::from).collect(),
            },
        }
    }
}

/// Owned objective with no lifetime constraints.
///
/// Unlike [`Objective`], this type owns all its strings,
/// making it suitable for long-lived data structures and mutation.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, PartialEq)]
pub struct ObjectiveOwned {
    /// The name of the objective (owned).
    pub name: String,
    /// The coefficients of the objective.
    pub coefficients: Vec<CoefficientOwned>,
}

impl ObjectiveOwned {
    /// Create a new owned objective.
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into(), coefficients: Vec::new() }
    }

    /// Add a coefficient to the objective.
    pub fn add_coefficient(&mut self, name: impl Into<String>, value: f64) {
        self.coefficients.push(CoefficientOwned { name: name.into(), value });
    }
}

impl<'a> From<&Objective<'a>> for ObjectiveOwned {
    fn from(objective: &Objective<'a>) -> Self {
        Self { name: objective.name.to_string(), coefficients: objective.coefficients.iter().map(CoefficientOwned::from).collect() }
    }
}

/// Owned variable with no lifetime constraints.
///
/// Unlike [`Variable`], this type owns its name string,
/// making it suitable for long-lived data structures and mutation.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, PartialEq)]
pub struct VariableOwned {
    /// The name of the variable (owned).
    pub name: String,
    /// The type of the variable.
    pub var_type: VariableType,
}

impl VariableOwned {
    /// Create a new owned variable with default (Free) type.
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into(), var_type: VariableType::default() }
    }

    /// Builder method to set the variable type.
    #[must_use]
    pub fn with_var_type(self, var_type: VariableType) -> Self {
        Self { var_type, ..self }
    }
}

impl<'a> From<&Variable<'a>> for VariableOwned {
    fn from(variable: &Variable<'a>) -> Self {
        Self { name: variable.name.to_string(), var_type: variable.var_type.clone() }
    }
}

#[cfg(test)]
mod tests {
    use std::borrow::Cow;

    use super::*;

    #[test]
    fn test_comparison_op() {
        // Default and variants
        assert_eq!(ComparisonOp::default(), ComparisonOp::GT);

        // All variants with as_ref and display
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
        // Creation and equality
        let coeff = Coefficient { name: "x1", value: 2.5 };
        assert_eq!(coeff.name, "x1");
        assert_eq!(coeff.value, 2.5);
        assert_eq!(coeff.clone(), coeff);

        // Display special cases
        assert_eq!(format!("{}", Coefficient { name: "x", value: 1.0 }), "x");
        assert_eq!(format!("{}", Coefficient { name: "x", value: -1.0 }), "-x");
        assert_eq!(format!("{}", Coefficient { name: "x", value: 2.5 }), "2.5 x");
        assert_eq!(format!("{}", Coefficient { name: "x", value: 0.0 }), "0 x");

        // Extreme values
        assert!(format!("{}", Coefficient { name: "x", value: f64::INFINITY }).contains("inf"));
        assert!(format!("{}", Coefficient { name: "x", value: f64::NAN }).contains("NaN"));
    }

    #[test]
    fn test_constraint() {
        // Standard constraint
        let std_constraint = Constraint::Standard {
            name: Cow::Borrowed("c1"),
            coefficients: vec![Coefficient { name: "x1", value: 2.0 }, Coefficient { name: "x2", value: -1.0 }],
            operator: ComparisonOp::LTE,
            rhs: 10.0,
        };
        assert_eq!(std_constraint.name(), Cow::Borrowed("c1"));
        let display = format!("{std_constraint}");
        assert!(display.contains("c1:") && display.contains("<= 10"));

        // SOS constraint
        let sos_constraint =
            Constraint::SOS { name: Cow::Borrowed("sos1"), sos_type: SOSType::S1, weights: vec![Coefficient { name: "x1", value: 1.0 }] };
        assert_eq!(sos_constraint.name(), Cow::Borrowed("sos1"));
        assert!(format!("{sos_constraint}").contains("S1::"));

        // Empty coefficients
        let empty = Constraint::Standard { name: Cow::Borrowed("e"), coefficients: vec![], operator: ComparisonOp::EQ, rhs: 0.0 };
        if let Constraint::Standard { coefficients, .. } = empty {
            assert!(coefficients.is_empty());
        }
    }

    #[test]
    fn test_objective() {
        let obj = Objective { name: Cow::Borrowed("profit"), coefficients: vec![Coefficient { name: "x1", value: 5.0 }] };
        assert_eq!(obj.name, "profit");
        assert_eq!(obj.coefficients.len(), 1);

        // Owned name
        let obj_owned = Objective { name: Cow::Owned("dynamic".to_string()), coefficients: vec![] };
        assert_eq!(obj_owned.name, "dynamic");
        assert!(obj_owned.coefficients.is_empty());
    }

    #[test]
    fn test_variable_type() {
        assert_eq!(VariableType::default(), VariableType::Free);

        // All variants with as_ref and display
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

        // Bounds extraction
        if let VariableType::DoubleBound(l, u) = VariableType::DoubleBound(0.0, 100.0) {
            assert_eq!((l, u), (0.0, 100.0));
        }
    }

    #[test]
    fn test_variable() {
        // New and default type
        let var = Variable::new("x1");
        assert_eq!(var.name, "x1");
        assert_eq!(var.var_type, VariableType::Free);

        // Builder pattern
        let var_binary = Variable::new("x").with_var_type(VariableType::Binary);
        assert_eq!(var_binary.var_type, VariableType::Binary);

        // Setter
        let mut var_mut = Variable::new("y");
        var_mut.set_var_type(VariableType::Integer);
        assert_eq!(var_mut.var_type, VariableType::Integer);

        // Equality
        assert_eq!(Variable::new("x").with_var_type(VariableType::Binary), Variable::new("x").with_var_type(VariableType::Binary));
        assert_ne!(Variable::new("x").with_var_type(VariableType::Binary), Variable::new("y").with_var_type(VariableType::Binary));
    }

    #[cfg(feature = "serde")]
    #[test]
    fn test_serde_roundtrips() {
        // Test all serializable types in one test
        let op = ComparisonOp::GTE;
        assert_eq!(op, serde_json::from_str(&serde_json::to_string(&op).unwrap()).unwrap());

        let sense = Sense::Maximize;
        assert_eq!(sense, serde_json::from_str(&serde_json::to_string(&sense).unwrap()).unwrap());

        let vt = VariableType::DoubleBound(0.0, 100.0);
        assert_eq!(vt, serde_json::from_str(&serde_json::to_string(&vt).unwrap()).unwrap());

        let var = Variable::new("test").with_var_type(VariableType::Binary);
        assert_eq!(var, serde_json::from_str(&serde_json::to_string(&var).unwrap()).unwrap());
    }
}
