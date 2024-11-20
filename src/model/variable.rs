use std::mem::discriminant;

use pest::iterators::Pair;

use crate::Rule;

#[derive(Debug, Default, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "diff", derive(diff::Diff), diff(attr(#[derive(Debug, PartialEq, serde::Serialize, serde::Deserialize)])))]
/// A enum representing the bounds of a variable
pub enum Variable {
    /// Binary variable
    Binary,

    /// Bounded variable
    Bounded(f64, f64, bool),

    /// Unbounded variable (-Infinity, +Infinity)
    Free,

    #[default]
    /// General variable [0, +Infinity]
    General,

    /// Integer variable [0, 1]
    Integer,

    /// Lower bounded variable
    LB(f64),

    /// Semi-continuous
    SemiContinuous,

    /// Upper bounded variable
    UB(f64),
}

impl From<Rule> for Variable {
    #[inline]
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

impl Eq for Variable {}

impl PartialEq for Variable {
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::LB(l0), Self::LB(r0)) | (Self::UB(l0), Self::UB(r0)) => l0 == r0,
            (Self::Bounded(l0, l1, l2), Self::Bounded(r0, r1, r2)) => l2 == r2 && l1 == r1 && l0 == r0,
            _ => discriminant(self) == discriminant(other),
        }
    }
}

impl Variable {
    #[inline]
    #[allow(clippy::wildcard_enum_match_arm)]
    pub fn set_semi_continuous(&mut self) {
        if let Self::Bounded(lb, ub, _) = *self {
            *self = Self::Bounded(lb, ub, true);
        }
    }
}

#[allow(clippy::wildcard_enum_match_arm, clippy::unwrap_used, clippy::single_call_fn)]
pub(crate) fn get_bound<'a>(pair: &'a Pair<'_, Rule>) -> Option<(&'a str, Variable)> {
    let mut parts = pair.clone().into_inner();
    match pair.as_rule() {
        Rule::LOWER_BOUND => {
            let name = parts.next().unwrap().as_str();
            let _ = parts.next(); // Ignore the mathematical operator
            Some((name, Variable::LB(parts.next().unwrap().as_str().parse().unwrap())))
        }
        Rule::LOWER_BOUND_REV => {
            let value = parts.next().unwrap().as_str().parse().unwrap();
            let _ = parts.next(); // Ignore the mathematical operator
            Some((parts.next().unwrap().as_str(), Variable::LB(value)))
        }
        Rule::UPPER_BOUND => {
            let name = parts.next().unwrap().as_str();
            let _ = parts.next(); // Ignore the mathematical operator
            Some((name, Variable::UB(parts.next().unwrap().as_str().parse().unwrap())))
        }
        Rule::UPPER_BOUND_REV => {
            let value = parts.next().unwrap().as_str().parse().unwrap();
            let _ = parts.next(); // Ignore the mathematical operator
            Some((parts.next().unwrap().as_str(), Variable::UB(value)))
        }
        Rule::BOUNDED => {
            let lb = parts.next().unwrap().as_str();
            let _ = parts.next(); // Ignore the mathematical operator

            let name = parts.next().unwrap().as_str();
            let _ = parts.next(); // Ignore the mathematical operator

            let ub = parts.next().unwrap().as_str();
            Some((name, Variable::Bounded(lb.parse().unwrap(), ub.parse().unwrap(), false)))
        }
        Rule::FREE => {
            let name = parts.next().unwrap().as_str();
            Some((name, Variable::Free))
        }
        _ => None,
    }
}
