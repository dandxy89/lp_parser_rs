use std::str::FromStr;

use crate::model::lp_error::LPParserError;

#[derive(Debug, Default, PartialEq, Eq, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "diff", derive(diff::Diff), diff(attr(#[derive(Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)])))]
/// Problem sense
pub enum Sense {
    #[default]
    Minimize,

    Maximize,
}

#[derive(Debug, Default, PartialEq, Eq, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "diff", derive(diff::Diff), diff(attr(#[derive(Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)])))]
pub enum Cmp {
    #[default]
    #[cfg_attr(feature = "serde", serde(rename = ">"))]
    GreaterThan,

    #[cfg_attr(feature = "serde", serde(rename = "<"))]
    LessThan,

    #[cfg_attr(feature = "serde", serde(rename = "="))]
    Equal,

    #[cfg_attr(feature = "serde", serde(rename = ">="))]
    GreaterOrEqual,

    #[cfg_attr(feature = "serde", serde(rename = "<="))]
    LessOrEqual,
}

impl FromStr for Cmp {
    type Err = LPParserError;

    #[inline]
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            ">=" => Ok(Self::GreaterOrEqual),
            "<=" => Ok(Self::LessOrEqual),
            ">" => Ok(Self::GreaterThan),
            "<" => Ok(Self::LessThan),
            "=" => Ok(Self::Equal),
            _ => Err(LPParserError::ComparisonError(s.to_owned())),
        }
    }
}
