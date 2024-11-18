use std::str::FromStr;

use pest::iterators::Pair;
use unique_id::sequence::SequenceGenerator;

use crate::{
    model::{coefficient::Coefficient, constraint::Constraint, lp_error::LPParserError, lp_problem::LPPart, ParseResult},
    Rule,
};

#[derive(Debug, PartialEq, Eq, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "diff", derive(diff::Diff), diff(attr(#[derive(Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)])))]
/// Special Ordered Sets
pub enum SOSClass {
    /// Special Ordered Sets of type 1 (SOS1 or S1)
    S1,

    /// Special Ordered Sets of type 2 (SOS2 or S2)
    S2,
}

impl FromStr for SOSClass {
    type Err = LPParserError;

    #[inline]
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "s1" | "s1::" => Ok(Self::S1),
            "s2" | "s2::" => Ok(Self::S2),
            _ => Err(LPParserError::SOSError(s.to_owned())),
        }
    }
}

impl LPPart for SOSClass {
    type Output = Constraint;

    #[inline]
    fn try_into(pair: Pair<'_, Rule>, _: &mut SequenceGenerator) -> Result<Self::Output, LPParserError> {
        let mut parts = pair.into_inner();
        let name = parts.next().unwrap().as_str().to_owned();

        let kind = parts.next().unwrap().as_str().to_lowercase();

        let coefficients: ParseResult<Coefficient> = parts.map(|p| p.into_inner().try_into()).collect();

        Ok(Constraint::SOS { name, kind: Self::from_str(&kind)?, coefficients: coefficients? })
    }
}
