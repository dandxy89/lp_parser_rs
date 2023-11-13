use super::{coefficient::Coefficient, sos::SOSClass};

#[derive(Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Constraint {
    Standard { name: String, coefficients: Vec<Coefficient>, sense: String, rhs: f64 },
    SOS { name: String, kind: SOSClass, coefficients: Vec<Coefficient> },
}

impl Constraint {
    #[must_use]
    pub fn name(&self) -> String {
        match self {
            Self::Standard { name, .. } | Self::SOS { name, .. } => name.to_string(),
        }
    }

    #[must_use]
    pub fn coefficients(&self) -> &[Coefficient] {
        match self {
            Self::Standard { coefficients, .. } | Self::SOS { coefficients, .. } => coefficients,
        }
    }
}
