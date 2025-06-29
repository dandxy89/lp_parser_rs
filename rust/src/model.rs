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

#[cfg(test)]
mod tests {
    use std::borrow::Cow;

    use super::*;

    #[test]
    fn test_comparison_op_values() {
        assert_eq!(ComparisonOp::default(), ComparisonOp::GT);

        let ops = [ComparisonOp::GT, ComparisonOp::GTE, ComparisonOp::EQ, ComparisonOp::LT, ComparisonOp::LTE];

        // Test that all variants are distinct
        for (i, op1) in ops.iter().enumerate() {
            for (j, op2) in ops.iter().enumerate() {
                if i == j {
                    assert_eq!(op1, op2);
                } else {
                    assert_ne!(op1, op2);
                }
            }
        }
    }

    #[test]
    fn test_comparison_op_as_ref() {
        assert_eq!(ComparisonOp::GT.as_ref(), b">");
        assert_eq!(ComparisonOp::GTE.as_ref(), b">=");
        assert_eq!(ComparisonOp::EQ.as_ref(), b"=");
        assert_eq!(ComparisonOp::LT.as_ref(), b"<");
        assert_eq!(ComparisonOp::LTE.as_ref(), b"<=");
    }

    #[test]
    fn test_comparison_op_display() {
        assert_eq!(format!("{}", ComparisonOp::GT), ">");
        assert_eq!(format!("{}", ComparisonOp::GTE), ">=");
        assert_eq!(format!("{}", ComparisonOp::EQ), "=");
        assert_eq!(format!("{}", ComparisonOp::LT), "<");
        assert_eq!(format!("{}", ComparisonOp::LTE), "<=");
    }

    #[test]
    fn test_comparison_op_clone_and_eq() {
        let op1 = ComparisonOp::EQ;
        let op2 = op1.clone();
        assert_eq!(op1, op2);

        let op3 = ComparisonOp::LTE;
        assert_ne!(op1, op3);
    }

    // Test Sense enum
    #[test]
    fn test_sense_values() {
        assert_eq!(Sense::default(), Sense::Minimize);

        let minimize = Sense::Minimize;
        let maximize = Sense::Maximize;

        assert_ne!(minimize, maximize);
        assert_eq!(minimize.clone(), minimize);
        assert_eq!(maximize.clone(), maximize);
    }

    #[test]
    fn test_sense_is_minimisation() {
        assert!(Sense::Minimize.is_minimisation());
        assert!(!Sense::Maximize.is_minimisation());
    }

    #[test]
    fn test_sense_display() {
        assert_eq!(format!("{}", Sense::Minimize), "Minimize");
        assert_eq!(format!("{}", Sense::Maximize), "Maximize");
    }

    #[test]
    fn test_sos_type_values() {
        let s1 = SOSType::S1;
        let s2 = SOSType::S2;

        assert_ne!(s1, s2);
        assert_eq!(s1.clone(), s1);
        assert_eq!(s2.clone(), s2);
    }

    #[test]
    fn test_sos_type_as_ref() {
        assert_eq!(SOSType::S1.as_ref(), b"S1");
        assert_eq!(SOSType::S2.as_ref(), b"S2");
    }

    #[test]
    fn test_sos_type_display() {
        assert_eq!(format!("{}", SOSType::S1), "S1");
        assert_eq!(format!("{}", SOSType::S2), "S2");
    }

    // Test Coefficient struct
    #[test]
    fn test_coefficient_creation() {
        let coeff = Coefficient { name: "x1", value: 2.5 };
        assert_eq!(coeff.name, "x1");
        assert_eq!(coeff.value, 2.5);
    }

    #[test]
    fn test_coefficient_clone_and_eq() {
        let coeff1 = Coefficient { name: "x1", value: 2.5 };
        let coeff2 = coeff1.clone();
        assert_eq!(coeff1, coeff2);

        let coeff3 = Coefficient { name: "x2", value: 2.5 };
        assert_ne!(coeff1, coeff3);

        let coeff4 = Coefficient { name: "x1", value: 3.0 };
        assert_ne!(coeff1, coeff4);
    }

    #[test]
    fn test_coefficient_display() {
        // Test special cases
        let coeff1 = Coefficient { name: "x1", value: 1.0 };
        assert_eq!(format!("{coeff1}"), "x1");

        let coeff2 = Coefficient { name: "x2", value: -1.0 };
        assert_eq!(format!("{coeff2}"), "-x2");

        // Test general cases
        let coeff3 = Coefficient { name: "x3", value: 2.5 };
        assert_eq!(format!("{coeff3}"), "2.5 x3");

        let coeff4 = Coefficient { name: "x4", value: -3.7 };
        assert_eq!(format!("{coeff4}"), "-3.7 x4");

        let coeff5 = Coefficient { name: "x5", value: 0.0 };
        assert_eq!(format!("{coeff5}"), "0 x5");
    }

    // Test Constraint enum
    #[test]
    fn test_constraint_standard() {
        let coefficients = vec![Coefficient { name: "x1", value: 2.0 }, Coefficient { name: "x2", value: -1.0 }];

        let constraint =
            Constraint::Standard { name: Cow::Borrowed("test_constraint"), coefficients, operator: ComparisonOp::LTE, rhs: 10.0 };

        assert_eq!(constraint.name(), Cow::Borrowed("test_constraint"));

        if let Constraint::Standard { coefficients, operator, rhs, .. } = &constraint {
            assert_eq!(coefficients.len(), 2);
            assert_eq!(*operator, ComparisonOp::LTE);
            assert_eq!(*rhs, 10.0);
        } else {
            panic!("Expected Standard constraint");
        }
    }

    #[test]
    fn test_constraint_sos() {
        let weights = vec![Coefficient { name: "x1", value: 1.0 }, Coefficient { name: "x2", value: 2.0 }];

        let constraint = Constraint::SOS { name: Cow::Borrowed("sos_constraint"), sos_type: SOSType::S1, weights };

        assert_eq!(constraint.name(), Cow::Borrowed("sos_constraint"));

        if let Constraint::SOS { sos_type, weights, .. } = &constraint {
            assert_eq!(*sos_type, SOSType::S1);
            assert_eq!(weights.len(), 2);
        } else {
            panic!("Expected SOS constraint");
        }
    }

    #[test]
    fn test_constraint_display_standard() {
        let coefficients =
            vec![Coefficient { name: "x1", value: 2.0 }, Coefficient { name: "x2", value: 3.0 }, Coefficient { name: "x3", value: -1.0 }];

        let constraint = Constraint::Standard { name: Cow::Borrowed("test"), coefficients, operator: ComparisonOp::LTE, rhs: 100.0 };

        let display = format!("{constraint}");
        assert!(display.contains("test:"));
        assert!(display.contains("2 x1"));
        assert!(display.contains("+ 3 x2"));
        assert!(display.contains("-x3"));
        assert!(display.contains("<= 100"));
    }

    #[test]
    fn test_constraint_display_sos() {
        let weights = vec![Coefficient { name: "x1", value: 1.0 }, Coefficient { name: "x2", value: 2.0 }];

        let constraint = Constraint::SOS { name: Cow::Borrowed("sos_test"), sos_type: SOSType::S2, weights };

        let display = format!("{constraint}");
        assert!(display.contains("sos_test:"));
        assert!(display.contains("S2::"));
        assert!(display.contains("x1:1"));
        assert!(display.contains("x2:2"));
    }

    // Test Objective struct
    #[test]
    fn test_objective_creation() {
        let coefficients = vec![Coefficient { name: "x1", value: 5.0 }, Coefficient { name: "x2", value: 3.0 }];

        let objective = Objective { name: Cow::Borrowed("profit"), coefficients };

        assert_eq!(objective.name, "profit");
        assert_eq!(objective.coefficients.len(), 2);
        assert_eq!(objective.coefficients[0].name, "x1");
        assert_eq!(objective.coefficients[0].value, 5.0);
    }

    #[test]
    fn test_objective_with_owned_name() {
        let coefficients = vec![Coefficient { name: "x1", value: 1.0 }];

        let objective = Objective { name: Cow::Owned("dynamic_objective".to_string()), coefficients };

        assert_eq!(objective.name, "dynamic_objective");
    }

    // Test VariableType enum
    #[test]
    fn test_variable_type_variants() {
        let types = vec![
            VariableType::Free,
            VariableType::General,
            VariableType::LowerBound(0.0),
            VariableType::UpperBound(100.0),
            VariableType::DoubleBound(0.0, 100.0),
            VariableType::Binary,
            VariableType::Integer,
            VariableType::SemiContinuous,
            VariableType::SOS,
        ];

        assert_eq!(VariableType::default(), VariableType::Free);

        // Test that all variants are distinct except for parameterized ones
        for var_type in &types {
            assert_eq!(var_type.clone(), *var_type);
        }
    }

    #[test]
    fn test_variable_type_as_ref() {
        assert_eq!(VariableType::Free.as_ref(), b"Free");
        assert_eq!(VariableType::General.as_ref(), b"General");
        assert_eq!(VariableType::LowerBound(5.0).as_ref(), b"LowerBound");
        assert_eq!(VariableType::UpperBound(10.0).as_ref(), b"UpperBound");
        assert_eq!(VariableType::DoubleBound(0.0, 10.0).as_ref(), b"DoubleBound");
        assert_eq!(VariableType::Binary.as_ref(), b"Binary");
        assert_eq!(VariableType::Integer.as_ref(), b"Integer");
        assert_eq!(VariableType::SemiContinuous.as_ref(), b"Semi-Continuous");
        assert_eq!(VariableType::SOS.as_ref(), b"SOS");
    }

    #[test]
    fn test_variable_type_display() {
        assert_eq!(format!("{}", VariableType::Free), "Free");
        assert_eq!(format!("{}", VariableType::General), "General");
        assert_eq!(format!("{}", VariableType::LowerBound(5.0)), "LowerBound");
        assert_eq!(format!("{}", VariableType::UpperBound(10.0)), "UpperBound");
        assert_eq!(format!("{}", VariableType::DoubleBound(0.0, 10.0)), "DualBounds");
        assert_eq!(format!("{}", VariableType::Binary), "Binary");
        assert_eq!(format!("{}", VariableType::Integer), "Integer");
        assert_eq!(format!("{}", VariableType::SemiContinuous), "Semi-Continuous");
        assert_eq!(format!("{}", VariableType::SOS), "SOS");
    }

    #[test]
    fn test_variable_type_bounds() {
        let lower = VariableType::LowerBound(5.0);
        let upper = VariableType::UpperBound(10.0);
        let double = VariableType::DoubleBound(0.0, 100.0);

        if let VariableType::LowerBound(bound) = lower {
            assert_eq!(bound, 5.0);
        }

        if let VariableType::UpperBound(bound) = upper {
            assert_eq!(bound, 10.0);
        }

        if let VariableType::DoubleBound(lower_bound, upper_bound) = double {
            assert_eq!(lower_bound, 0.0);
            assert_eq!(upper_bound, 100.0);
        }
    }

    // Test Variable struct
    #[test]
    fn test_variable_new() {
        let var = Variable::new("x1");
        assert_eq!(var.name, "x1");
        assert_eq!(var.var_type, VariableType::Free);
    }

    #[test]
    fn test_variable_with_var_type() {
        let var = Variable::new("x1").with_var_type(VariableType::Binary);
        assert_eq!(var.name, "x1");
        assert_eq!(var.var_type, VariableType::Binary);
    }

    #[test]
    fn test_variable_set_var_type() {
        let mut var = Variable::new("x1");
        var.set_var_type(VariableType::Integer);
        assert_eq!(var.var_type, VariableType::Integer);
    }

    #[test]
    fn test_variable_builder_pattern() {
        let var = Variable::new("production_rate").with_var_type(VariableType::DoubleBound(0.0, 1000.0));

        assert_eq!(var.name, "production_rate");
        if let VariableType::DoubleBound(lower, upper) = var.var_type {
            assert_eq!(lower, 0.0);
            assert_eq!(upper, 1000.0);
        } else {
            panic!("Expected DoubleBound variable type");
        }
    }

    #[test]
    fn test_variable_equality() {
        let var1 = Variable::new("x1").with_var_type(VariableType::Binary);
        let var2 = Variable::new("x1").with_var_type(VariableType::Binary);
        let var3 = Variable::new("x2").with_var_type(VariableType::Binary);
        let var4 = Variable::new("x1").with_var_type(VariableType::Integer);

        assert_eq!(var1, var2);
        assert_ne!(var1, var3);
        assert_ne!(var1, var4);
    }

    // Test edge cases and error conditions
    #[test]
    fn test_coefficient_with_extreme_values() {
        let coeff_inf = Coefficient { name: "x1", value: f64::INFINITY };
        let coeff_neg_inf = Coefficient { name: "x2", value: f64::NEG_INFINITY };
        let coeff_nan = Coefficient { name: "x3", value: f64::NAN };

        // Test that these can be created
        assert_eq!(coeff_inf.name, "x1");
        assert_eq!(coeff_neg_inf.name, "x2");
        assert_eq!(coeff_nan.name, "x3");

        // Test display with special values
        assert!(format!("{coeff_inf}").contains("inf"));
        assert!(format!("{coeff_neg_inf}").contains("-inf"));
        assert!(format!("{coeff_nan}").contains("NaN"));
    }

    #[test]
    fn test_variable_type_extreme_bounds() {
        let extreme_lower = VariableType::LowerBound(f64::NEG_INFINITY);
        let extreme_upper = VariableType::UpperBound(f64::INFINITY);
        let extreme_double = VariableType::DoubleBound(f64::NEG_INFINITY, f64::INFINITY);

        // These should be valid
        if let VariableType::LowerBound(bound) = extreme_lower {
            assert_eq!(bound, f64::NEG_INFINITY);
        }

        if let VariableType::UpperBound(bound) = extreme_upper {
            assert_eq!(bound, f64::INFINITY);
        }

        if let VariableType::DoubleBound(lower, upper) = extreme_double {
            assert_eq!(lower, f64::NEG_INFINITY);
            assert_eq!(upper, f64::INFINITY);
        }
    }

    #[test]
    fn test_constraint_with_empty_coefficients() {
        let constraint = Constraint::Standard { name: Cow::Borrowed("empty"), coefficients: vec![], operator: ComparisonOp::EQ, rhs: 0.0 };

        if let Constraint::Standard { coefficients, .. } = constraint {
            assert_eq!(coefficients.len(), 0);
        }
    }

    #[test]
    fn test_objective_with_empty_coefficients() {
        let objective = Objective { name: Cow::Borrowed("empty_obj"), coefficients: vec![] };

        assert_eq!(objective.coefficients.len(), 0);
    }

    // Test serialization round-trips if serde feature is enabled
    #[cfg(feature = "serde")]
    #[test]
    fn test_serde_roundtrip_comparison_op() {
        let original = ComparisonOp::GTE;
        let serialized = serde_json::to_string(&original).unwrap();
        let deserialized: ComparisonOp = serde_json::from_str(&serialized).unwrap();
        assert_eq!(original, deserialized);
    }

    #[cfg(feature = "serde")]
    #[test]
    fn test_serde_roundtrip_sense() {
        let original = Sense::Maximize;
        let serialized = serde_json::to_string(&original).unwrap();
        let deserialized: Sense = serde_json::from_str(&serialized).unwrap();
        assert_eq!(original, deserialized);
    }

    #[cfg(feature = "serde")]
    #[test]
    fn test_serde_roundtrip_variable_type() {
        let original = VariableType::DoubleBound(0.0, 100.0);
        let serialized = serde_json::to_string(&original).unwrap();
        let deserialized: VariableType = serde_json::from_str(&serialized).unwrap();
        assert_eq!(original, deserialized);
    }

    #[cfg(feature = "serde")]
    #[test]
    fn test_serde_roundtrip_variable() {
        let original = Variable::new("test_var").with_var_type(VariableType::Binary);
        let serialized = serde_json::to_string(&original).unwrap();
        let deserialized: Variable = serde_json::from_str(&serialized).unwrap();
        assert_eq!(original, deserialized);
    }
}
