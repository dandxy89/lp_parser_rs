use crate::Rule;

#[derive(Debug, Default, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
/// A enum representing the bounds of a variable
pub enum VariableType {
    /// Unbounded variable (-Infinity, +Infinity)
    Free,
    // Lower bounded variable
    LB(f64),
    // Upper bounded variable
    UB(f64),
    // Bounded variable
    Bounded(f64, f64, bool),
    // Integer variable [0, 1]
    Integer,
    // Binary variable
    Binary,
    #[default]
    // General variable [0, +Infinity]
    General,
    // Semi-continuous
    SemiContinuous,
}

impl From<Rule> for VariableType {
    #[allow(clippy::wildcard_enum_match_arm, clippy::unreachable)]
    fn from(value: Rule) -> Self {
        match value {
            Rule::INTEGERS => Self::Integer,
            Rule::GENERALS => Self::General,
            Rule::BINARIES => Self::Binary,
            Rule::SEMI_CONTINUOUS => Self::SemiContinuous,
            _ => unreachable!(),
        }
    }
}

impl PartialEq for VariableType {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::LB(l0), Self::LB(r0)) | (Self::UB(l0), Self::UB(r0)) => l0 == r0,
            (Self::Bounded(l0, l1, l2), Self::Bounded(r0, r1, r2)) => l2 == r2 && l1 == r1 && l0 == r0,
            _ => core::mem::discriminant(self) == core::mem::discriminant(other),
        }
    }
}

impl Eq for VariableType {}

impl VariableType {
    #[allow(clippy::wildcard_enum_match_arm)]
    pub fn set_semi_continuous(&mut self) {
        if let Self::Bounded(lb, ub, _) = self {
            *self = Self::Bounded(*lb, *ub, true);
        }
    }
}
