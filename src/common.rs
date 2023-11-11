use crate::Rule;
use pest::iterators::Pair;

pub trait IsNumeric {
    fn is_numeric(&self) -> bool;
}

impl IsNumeric for Rule {
    fn is_numeric(&self) -> bool {
        matches!(self, Self::FLOAT | Self::PLUS | Self::MINUS | Self::POS_INFINITY | Self::NEG_INFINITY)
    }
}

pub trait AsFloat {
    /// # Errors
    /// Returns an error if the rule cannot be converted to a float
    fn as_float(&self) -> anyhow::Result<f64>;
}

impl AsFloat for Pair<'_, Rule> {
    #[allow(clippy::unreachable, clippy::wildcard_enum_match_arm)]
    fn as_float(&self) -> anyhow::Result<f64> {
        match self.as_rule() {
            Rule::POS_INFINITY => Ok(f64::INFINITY),
            Rule::NEG_INFINITY => Ok(f64::NEG_INFINITY),
            Rule::FLOAT => Ok(self.as_str().trim().parse()?),
            Rule::PLUS => Ok(1.0),
            Rule::MINUS => Ok(-1.0),
            _ => unreachable!("Unexpected rule observed: {:?}", self.as_rule()),
        }
    }
}
