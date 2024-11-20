use pest::iterators::Pair;

use crate::{model::lp_error::LPParserError, Rule};

pub trait AsFloat {
    /// # Errors
    /// Returns an error if the rule cannot be converted to a float
    fn as_float(&self) -> Result<f64, LPParserError>;
}

pub trait RuleExt {
    fn is_cmp(&self) -> bool;
    fn is_numeric(&self) -> bool;
}

impl RuleExt for Rule {
    #[inline]
    fn is_cmp(&self) -> bool {
        matches!(*self, Self::GT | Self::LT | Self::EQ | Self::GTE | Self::LTE | Self::CMP)
    }

    #[inline]
    fn is_numeric(&self) -> bool {
        matches!(*self, Self::FLOAT | Self::PLUS | Self::MINUS | Self::POS_INFINITY | Self::NEG_INFINITY)
    }
}

impl AsFloat for Pair<'_, Rule> {
    #[inline]
    #[allow(clippy::unreachable, clippy::wildcard_enum_match_arm)]
    fn as_float(&self) -> Result<f64, LPParserError> {
        match self.as_rule() {
            Rule::POS_INFINITY => Ok(f64::INFINITY),
            Rule::NEG_INFINITY => Ok(f64::NEG_INFINITY),
            Rule::FLOAT => {
                let value = self.as_str().parse().map_err(|_e| LPParserError::FloatParseError(self.as_str().to_owned()))?;
                Ok(value)
            }
            Rule::PLUS => Ok(1.0),
            Rule::MINUS => Ok(-1.0),
            _ => unreachable!("Unexpected rule observed: {:?}", self.as_rule()),
        }
    }
}
