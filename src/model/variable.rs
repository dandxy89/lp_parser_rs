use crate::Rule;

#[derive(Debug, Default)]
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

impl VariableType {
    #[allow(clippy::wildcard_enum_match_arm)]
    pub fn set_semi_continuous(&mut self) {
        if let Self::Bounded(lb, ub, _) = self {
            *self = Self::Bounded(*lb, *ub, true);
        }
    }
}
