use std::str::FromStr;

use crate::model::lp_error::LPParserError;

#[derive(Debug, Default, PartialEq, Eq, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "diff", derive(diff::Diff), diff(attr(#[derive(Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)])))]
/// Problem sense
pub enum Sense {
    Maximize,

    #[default]
    Minimize,
}

#[derive(Debug, Default, PartialEq, Eq, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "diff", derive(diff::Diff), diff(attr(#[derive(Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)])))]
pub enum Cmp {
    #[cfg_attr(feature = "serde", serde(rename = "="))]
    Equal,

    #[cfg_attr(feature = "serde", serde(rename = ">="))]
    GreaterOrEqual,

    #[default]
    #[cfg_attr(feature = "serde", serde(rename = ">"))]
    GreaterThan,

    #[cfg_attr(feature = "serde", serde(rename = "<="))]
    LessOrEqual,

    #[cfg_attr(feature = "serde", serde(rename = "<"))]
    LessThan,
}

impl FromStr for Cmp {
    type Err = LPParserError;

    #[inline]
    fn from_str(cmp_str: &str) -> Result<Self, Self::Err> {
        match cmp_str {
            ">=" => Ok(Self::GreaterOrEqual),
            "<=" => Ok(Self::LessOrEqual),
            ">" => Ok(Self::GreaterThan),
            "<" => Ok(Self::LessThan),
            "=" => Ok(Self::Equal),
            _ => Err(LPParserError::ComparisonError(cmp_str.to_owned())),
        }
    }
}
