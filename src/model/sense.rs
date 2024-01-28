use std::str::FromStr;

#[derive(Debug, Default, PartialEq, Eq, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "diff", derive(diff::Diff))]
#[cfg_attr(feature = "diff", diff(attr(
    #[derive(Debug, PartialEq, serde::Serialize, serde::Deserialize)]
)))]
pub enum Sense {
    #[default]
    Minimize,
    Maximize,
}

#[derive(Debug, Default, PartialEq, Eq, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "diff", derive(diff::Diff))]
#[cfg_attr(feature = "diff", diff(attr(
    #[derive(Debug, PartialEq, serde::Serialize, serde::Deserialize)]
)))]
pub enum Cmp {
    #[default]
    #[serde(rename = ">")]
    GreaterThan,
    #[serde(rename = "<")]
    LessThan,
    #[serde(rename = "=")]
    Equal,
    #[serde(rename = ">=")]
    GreaterOrEqual,
    #[serde(rename = "<=")]
    LessOrEqual,
}

impl FromStr for Cmp {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            ">=" => Ok(Self::GreaterOrEqual),
            "<=" => Ok(Self::LessOrEqual),
            ">" => Ok(Self::GreaterThan),
            "<" => Ok(Self::LessThan),
            "=" => Ok(Self::Equal),
            _ => Err(anyhow::anyhow!("Unrecognized comparison operator: {s}")),
        }
    }
}
