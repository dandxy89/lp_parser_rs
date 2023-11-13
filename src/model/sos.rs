use std::str::FromStr;

#[derive(Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum SOSClass {
    S1,
    S2,
}

impl FromStr for SOSClass {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "s1" | "s1::" => Ok(Self::S1),
            "s2" | "s2::" => Ok(Self::S2),
            _ => Err(anyhow::anyhow!("Invalid SOS class: {}", s)),
        }
    }
}
