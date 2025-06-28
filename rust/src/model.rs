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
    /// Determines if the current optimisation sense is minimisation.
    pub const fn is_minimisation(&self) -> bool {
        matches!(self, Sense::Minimize)
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
#[derive(Debug, Clone, PartialEq, Eq)]
/// Represents the type of SOS (System of Systems) with variants `S1` and `S2`.
pub enum SOSType {
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
        if self.value == 1.0 {
            write!(f, "{}", self.name)
        } else if self.value == -1.0 {
            write!(f, "-{}", self.name)
        } else {
            write!(f, "{} {}", self.value, self.name)
        }
    }
}

#[cfg_attr(feature = "diff", derive(diff::Diff), diff(attr(#[derive(Debug, PartialEq)])))]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[cfg_attr(feature = "serde", serde(tag = "type"))]
#[derive(Debug, PartialEq)]
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
#[derive(Debug, PartialEq)]
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
            VariableType::Free => b"Free",
            VariableType::General => b"General",
            VariableType::LowerBound(_) => b"LowerBound",
            VariableType::UpperBound(_) => b"UpperBound",
            VariableType::DoubleBound(_, _) => b"DoubleBound",
            VariableType::Binary => b"Binary",
            VariableType::Integer => b"Integer",
            VariableType::SemiContinuous => b"Semi-Continuous",
            VariableType::SOS => b"SOS",
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
            Self::DoubleBound(_, _) => write!(f, "DualBounds"),
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
