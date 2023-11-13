use super::{coefficient::Coefficient, sos::SOSClass};

#[derive(Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Constraint {
    /// Standard LP constraint
    Standard { name: String, coefficients: Vec<Coefficient>, sense: String, rhs: f64 },
    /// Special Order Set (SOS)
    SOS { name: String, kind: SOSClass, coefficients: Vec<Coefficient> },
}

impl PartialEq for Constraint {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (
                Self::Standard { name: l_name, coefficients: l_coefficients, sense: l_sense, rhs: l_rhs },
                Self::Standard { name: r_name, coefficients: r_coefficients, sense: r_sense, rhs: r_rhs },
            ) => l_name == r_name && l_coefficients == r_coefficients && l_sense == r_sense && l_rhs == r_rhs,
            (
                Self::SOS { name: l_name, kind: l_kind, coefficients: l_coefficients },
                Self::SOS { name: r_name, kind: r_kind, coefficients: r_coefficients },
            ) => l_name == r_name && l_kind == r_kind && l_coefficients == r_coefficients,
            _ => false,
        }
    }
}

impl Eq for Constraint {}

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
