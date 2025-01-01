//! This Rust code defines several enums and structs used in optimization models.
//!
//! - `ComparisonOp`: Enum for comparison operations like greater than, less than, etc.
//! - `Sense`: Enum for optimization sense, either minimization or maximization.
//! - `SOSType`: Enum for types of System of Systems (SOS), with variants `S1` and `S2`.
//! - `Coefficient`: Struct representing a coefficient associated with a variable name.
//! - `Constraint`: Enum representing a constraint in an optimization problem, either standard or SOS.
//! - `Objective`: Struct representing an optimization objective with a name and coefficients.
//! - `VariableType`: Enum for different types of variables in optimization models.
//! - `Variable`: Struct representing a variable with a name and type.
//!

use std::borrow::Cow;

#[cfg_attr(feature = "diff", derive(diff::Diff), diff(attr(#[derive(Debug, PartialEq)])))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, PartialEq, Eq)]
/// Represents comparison operations that can be used to compare values.
///
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

#[cfg_attr(feature = "diff", derive(diff::Diff), diff(attr(#[derive(Debug, PartialEq)])))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Default, Clone, PartialEq, Eq)]
/// Represents the optimization sense for an objective function.
///
pub enum Sense {
    #[default]
    Minimize,
    Maximize,
}

impl Sense {
    /// Determines if the current optimization sense is minimization.
    ///
    pub fn is_minimization(&self) -> bool {
        matches!(self, Sense::Minimize)
    }
}

#[cfg_attr(feature = "diff", derive(diff::Diff), diff(attr(#[derive(Debug, PartialEq)])))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, PartialEq, Eq)]
/// Represents the type of SOS (System of Systems) with variants `S1` and `S2`.
pub enum SOSType {
    S1,
    S2,
}

#[cfg_attr(feature = "diff", derive(diff::Diff), diff(attr(#[derive(Debug, PartialEq)])))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, PartialEq)]
/// Represents a coefficient associated with a variable name.
///
/// # Fields
///
/// * `var_name` - A string slice representing the name of the variable.
/// * `coefficient` - A floating-point number representing the coefficient value.
///
pub struct Coefficient<'a> {
    pub var_name: &'a str,
    pub coefficient: f64,
}

#[cfg_attr(feature = "diff", derive(diff::Diff), diff(attr(#[derive(Debug, PartialEq)])))]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[cfg_attr(feature = "serde", serde(tag = "type"))]
#[derive(Debug, PartialEq)]
/// Represents a constraint in an optimization problem, which can be either a
/// standard linear constraint or a special ordered set (SOS) constraint.
///
/// # Variants
///
/// * `Standard` - A linear constraint defined by a name, a vector of coefficients,
///   a comparison operator, and a right-hand side value.
/// * `SOS` - A special ordered set constraint defined by a name, a type of SOS,
///   and a vector of weights.
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
    Standard { name: Cow<'a, str>, coefficients: Vec<Coefficient<'a>>, operator: ComparisonOp, rhs: f64 },
    SOS { name: Cow<'a, str>, sos_type: SOSType, weights: Vec<Coefficient<'a>> },
}

impl<'a> Constraint<'a> {
    /// Returns the name of the constraint as a `Cow<str>`.
    pub fn name(&'a self) -> Cow<'a, str> {
        match self {
            Constraint::Standard { name, .. } => name.clone(),
            Constraint::SOS { name, .. } => name.clone(),
        }
    }
}

#[cfg_attr(feature = "diff", derive(diff::Diff), diff(attr(#[derive(Debug, PartialEq)])))]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[derive(Debug, PartialEq)]
/// Represents an optimization objective with a name and a list of coefficients.
///
/// This struct can optionally derive `Diff` for change tracking and `Serialize`
/// for serialization, depending on the enabled features.
///
/// # Fields
///
/// * `name` - A borrowed string representing the name of the objective.
/// * `coefficients` - A vector of `Coefficient` instances associated with the objective.
///
pub struct Objective<'a> {
    pub name: Cow<'a, str>,
    pub coefficients: Vec<Coefficient<'a>>,
}

#[cfg_attr(feature = "diff", derive(diff::Diff), diff(attr(#[derive(Debug, PartialEq)])))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, Default, PartialEq)]
/// Represents different types of variables that can be used in optimization models.
///
/// This enum supports various configurations such as:
/// - `Free`: Unbounded variable (-Infinity, +Infinity).
/// - `General`: General variable [0, +Infinity].
/// - `LowerBound(f64)`: Variable with a lower bound (`x >= lb`).
/// - `UpperBound(f64)`: Variable with an upper bound (`x ≤ ub`).
/// - `DoubleBound(f64, f64)`: Variable with both lower and upper bounds (`lb ≤ x ≤ ub`).
/// - `Binary`: Binary variable.
/// - `Integer`: Integer variable.
/// - `SemiContinuous`: Semi-continuous variable.
/// - `SOS`: Special Order Set variable.
///
pub enum VariableType {
    #[default]
    /// Unbounded variable (-Infinity, +Infinity)
    Free,

    /// General variable [0, +Infinity]
    General,

    LowerBound(f64),       // `x >= lb`
    UpperBound(f64),       // `x ≤ ub`
    DoubleBound(f64, f64), // `lb ≤ x ≤ ub`

    Binary,

    Integer,

    SemiContinuous,

    /// Special Order Set (SOS)
    SOS,
}

#[cfg_attr(feature = "diff", derive(diff::Diff), diff(attr(#[derive(Debug, PartialEq)])))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, PartialEq)]
/// Represents a variable with a name and type.
///
/// # Fields
///
/// * `name` - A string slice that holds the name of the variable.
/// * `var_type` - The type of the variable, represented by `VariableType`.
///
pub struct Variable<'a> {
    pub name: &'a str,
    pub var_type: VariableType,
}

impl<'a> Variable<'a> {
    pub fn new(name: &'a str) -> Self {
        Self { name, var_type: VariableType::default() }
    }

    pub fn with_var_type(self, var_type: VariableType) -> Self {
        Self { var_type, ..self }
    }

    pub fn set_var_type(&mut self, var_type: VariableType) {
        self.var_type = var_type;
    }
}

#[cfg(feature = "serde")]
impl<'de: 'a, 'a> serde::Deserialize<'de> for Constraint<'a> {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(PartialEq, serde::Deserialize)]
        #[serde(field_identifier, rename_all = "lowercase")]
        enum Field {
            Type,
            Name,
            Coefficients,
            Weights,
            Operator,
            Rhs,
            #[serde(alias = "sos_type")]
            SosType,
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
                                _ => {
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
                                _ => {
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
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(serde::Deserialize)]
        #[serde(field_identifier, rename_all = "lowercase")]
        enum Field {
            Name,
            Coefficients,
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
