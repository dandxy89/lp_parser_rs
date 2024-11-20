use pest::iterators::Pair;
use unique_id::sequence::SequenceGenerator;

use crate::{
    model::{coefficient::Coefficient, get_name, lp_error::LPParserError, lp_problem::LPPart, ParseResult},
    Rule,
};

#[derive(Debug, PartialEq, Eq, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "diff", derive(diff::Diff), diff(attr(#[derive(Debug, PartialEq, serde::Serialize, serde::Deserialize)])))]
/// Problem objective
pub struct Objective {
    /// Objective coefficients
    pub coefficients: Vec<Coefficient>,
    /// Optional objective name - if missing it will be auto-generated (`obj_x1`, `obj_x2`, ...)
    pub name: String,
}

impl LPPart for Objective {
    type Output = Self;

    #[inline]
    fn try_into(pair: Pair<'_, Rule>, id_gen: &mut SequenceGenerator) -> Result<Self, LPParserError> {
        let mut parts = pair.into_inner().peekable();

        // Objective name can be omitted in LP files, so we need to handle that case
        let name = get_name(&mut parts, id_gen, Rule::OBJECTIVE_NAME);
        let coefficients: ParseResult<_> = parts.map(|obj_part| obj_part.into_inner().try_into()).collect();

        Ok(Self { name, coefficients: coefficients? })
    }
}
