use pest::iterators::Pairs;

use crate::{
    common::{AsFloat as _, RuleExt as _},
    model::lp_error::LPParserError,
    Rule,
};

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "diff", derive(diff::Diff), diff(attr(#[derive(Debug, PartialEq, serde::Serialize, serde::Deserialize)])))]
pub struct Coefficient {
    /// Variable name
    pub var_name: String,
    /// Coefficient or SOS variable weight
    pub coefficient: f64,
}

impl PartialEq for Coefficient {
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        self.var_name == other.var_name && self.coefficient == other.coefficient
    }
}

impl Eq for Coefficient {}

impl TryFrom<Pairs<'_, Rule>> for Coefficient {
    type Error = LPParserError;

    #[inline]
    #[allow(clippy::unreachable, clippy::wildcard_enum_match_arm)]
    fn try_from(values: Pairs<'_, Rule>) -> Result<Self, LPParserError> {
        let (mut value, mut var_name) = (1.0, String::new());

        for item in values {
            match item.as_rule() {
                r if r.is_numeric() => {
                    value *= item.as_float()?;
                }
                Rule::VARIABLE_SUBSET | Rule::VARIABLE => {
                    var_name = item.as_str().to_owned();
                }
                _ => unreachable!("Unexpected rule encountered"),
            }
        }
        Ok(Self { var_name, coefficient: value })
    }
}
