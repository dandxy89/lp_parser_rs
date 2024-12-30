#[cfg_attr(feature = "diff", derive(diff::Diff), diff(attr(#[derive(Debug, PartialEq)])))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ComparisonOp {
    LT,
    LTE,
    EQ,
    GT,
    GTE,
}

#[cfg_attr(feature = "diff", derive(diff::Diff), diff(attr(#[derive(Debug, PartialEq)])))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Sense {
    Minimize,
    Maximize,
}

impl Sense {
    pub fn is_minimization(&self) -> bool {
        self == &Sense::Minimize
    }
}

#[cfg_attr(feature = "diff", derive(diff::Diff), diff(attr(#[derive(Debug, PartialEq)])))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, PartialEq, Eq)]
pub enum SOSType {
    S1,
    S2,
}

#[cfg_attr(feature = "diff", derive(diff::Diff), diff(attr(#[derive(Debug, PartialEq)])))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, PartialEq)]
pub enum Bound<'a> {
    Free(&'a str),
    LowerBound(&'a str, f64),
    UpperBound(&'a str, f64),
    DoubleBound(&'a str, f64, f64),
}

#[cfg_attr(feature = "diff", derive(diff::Diff), diff(attr(#[derive(Debug, PartialEq)])))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, PartialEq)]
pub struct Coefficient<'a> {
    pub var_name: &'a str,
    pub coefficient: f64,
}

#[cfg_attr(feature = "diff", derive(diff::Diff), diff(attr(#[derive(Debug, PartialEq)])))]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[cfg_attr(feature = "serde", serde(tag = "type"))]
#[derive(Debug, PartialEq)]
pub enum Constraint<'a> {
    Standard { name: Option<String>, coefficients: Vec<Coefficient<'a>>, operator: ComparisonOp, rhs: f64 },
    SOS { name: Option<String>, sos_type: SOSType, weights: Vec<Coefficient<'a>> },
}

#[cfg_attr(feature = "diff", derive(diff::Diff), diff(attr(#[derive(Debug, PartialEq)])))]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[derive(Debug, PartialEq)]
pub struct Objective<'a> {
    pub name: Option<String>,
    pub coefficients: Vec<Coefficient<'a>>,
}

#[cfg_attr(feature = "diff", derive(diff::Diff), diff(attr(#[derive(Debug, PartialEq)])))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, PartialEq)]
pub enum VariableType {
    Free,
    Binary,
    Integer,
    General { lower_bound: Option<f64>, upper_bound: Option<f64> },
    SemiContinuous { lower_bound: Option<f64>, upper_bound: Option<f64> },
}

#[cfg_attr(feature = "diff", derive(diff::Diff), diff(attr(#[derive(Debug, PartialEq)])))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, PartialEq)]
pub struct Variable<'a> {
    pub name: &'a str,
    pub var_type: VariableType,
}

#[cfg(feature = "serde")]
impl<'de: 'a, 'a> serde::Deserialize<'de> for Constraint<'a> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        // Define the fields we expect to see
        #[derive(PartialEq, serde::Deserialize)]
        #[serde(field_identifier, rename_all = "lowercase")]
        enum Field {
            Type,
            Name,
            Coefficients,
            Weights,
            Operator,
            Rhs,
            SosType,
        }

        struct ConstraintVisitor<'a>(std::marker::PhantomData<Constraint<'a>>);

        impl<'de: 'a, 'a> serde::de::Visitor<'de> for ConstraintVisitor<'a> {
            type Value = Constraint<'a>;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("struct Constraint")
            }

            fn visit_map<V>(self, mut map: V) -> Result<Constraint<'a>, V::Error>
            where
                V: serde::de::MapAccess<'de>,
            {
                let constraint_type: String = match map.next_key::<Field>()? {
                    Some(Field::Type) => map.next_value()?,
                    _ => return Err(serde::de::Error::missing_field("type")),
                };
                match constraint_type.as_str() {
                    "Standard" => {
                        let mut name = None;
                        let mut coefficients = None;
                        let mut operator = None;
                        let mut rhs = None;

                        while let Some(key) = map.next_key()? {
                            match key {
                                Field::Name => name = Some(map.next_value()?),
                                Field::Coefficients => coefficients = Some(map.next_value()?),
                                Field::Operator => operator = Some(map.next_value()?),
                                Field::Rhs => rhs = Some(map.next_value()?),
                                _ => {
                                    let _ = map.next_value::<serde::de::IgnoredAny>()?;
                                }
                            }
                        }

                        Ok(Constraint::Standard {
                            name,
                            coefficients: coefficients.ok_or_else(|| serde::de::Error::missing_field("coefficients"))?,
                            operator: operator.ok_or_else(|| serde::de::Error::missing_field("operator"))?,
                            rhs: rhs.ok_or_else(|| serde::de::Error::missing_field("rhs"))?,
                        })
                    }
                    "SOS" => {
                        let mut name = None;
                        let mut sos_type = None;
                        let mut weights = None;

                        while let Some(key) = map.next_key()? {
                            match key {
                                Field::Name => name = Some(map.next_value()?),
                                Field::SosType => sos_type = Some(map.next_value()?),
                                Field::Weights => weights = Some(map.next_value()?),
                                _ => {
                                    let _ = map.next_value::<serde::de::IgnoredAny>()?;
                                }
                            }
                        }

                        Ok(Constraint::SOS {
                            name,
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
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
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

            fn visit_map<V>(self, mut map: V) -> Result<Objective<'a>, V::Error>
            where
                V: serde::de::MapAccess<'de>,
            {
                let mut name = None;
                let mut coefficients = None;

                while let Some(key) = map.next_key()? {
                    match key {
                        Field::Name => name = Some(map.next_value()?),
                        Field::Coefficients => coefficients = Some(map.next_value()?),
                    }
                }

                Ok(Objective { name, coefficients: coefficients.ok_or_else(|| serde::de::Error::missing_field("coefficients"))? })
            }
        }

        deserializer.deserialize_struct("Objective", &["name", "coefficients"], ObjectiveVisitor(std::marker::PhantomData))
    }
}
