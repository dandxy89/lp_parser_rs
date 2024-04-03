use std::str::FromStr;

use pest::iterators::Pair;
use unique_id::sequence::SequenceGenerator;

use crate::{
    model::{constraint::Constraint, lp_problem::LPPart},
    Rule,
};

#[derive(Debug, PartialEq, Eq, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "diff", derive(diff::Diff), diff(attr(#[derive(Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)])))]
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
            _ => Err(anyhow::anyhow!("Invalid SOS class: {s}")),
        }
    }
}

impl LPPart for SOSClass {
    type Output = Constraint;

    fn try_into(pair: Pair<'_, Rule>, _: &mut SequenceGenerator) -> anyhow::Result<Self::Output> {
        let mut parts = pair.into_inner();
        let name = parts.next().unwrap().as_str().to_owned();
        let kind = parts.next().unwrap().as_str().to_lowercase();
        let coefficients: anyhow::Result<Vec<_>> = parts.map(|p| p.into_inner().try_into()).collect();
        Ok(Constraint::SOS { name, kind: Self::from_str(&kind)?, coefficients: coefficients? })
    }
}
