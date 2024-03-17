use std::str::FromStr;

#[derive(Debug, Default, PartialEq, Eq, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "diff", derive(diff::Diff), diff(attr(#[derive(Debug, PartialEq, serde::Serialize, serde::Deserialize)])))]
pub enum Sense {
    #[default]
    Minimize,
    Maximize,
}

#[derive(Debug, Default, PartialEq, Eq, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "diff", derive(diff::Diff), diff(attr(#[derive(Debug, PartialEq, serde::Serialize, serde::Deserialize)])))]
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
