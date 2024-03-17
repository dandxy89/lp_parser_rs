use std::str::FromStr;

use pest::iterators::Pair;
use unique_id::sequence::SequenceGenerator;

use crate::{
    common::RuleExt,
    model::{coefficient::Coefficient, get_name, lp_problem::LPPart, sense::Cmp, sos::SOSClass},
    Rule,
};

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "diff", derive(diff::Diff), diff(attr(#[derive(Debug, PartialEq, serde::Serialize, serde::Deserialize)])))]
pub enum Constraint {
    /// Standard LP constraint
    Standard { name: String, coefficients: Vec<Coefficient>, sense: Cmp, rhs: f64 },
    /// Special Order Set (SOS)
    SOS { name: String, kind: SOSClass, coefficients: Vec<Coefficient> },
}

impl PartialEq for Constraint {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (
                Self::Standard { name: l_name, coefficients: l_coefficients, sense: l_sense, rhs: l_rhs },
                Self::Standard { name: r_name, coefficients: r_coefficients, sense: r_sense, rhs: r_rhs },
            ) => l_name == r_name && l_coefficients == r_coefficients && l_sense == r_sense && l_rhs == r_rhs,
            (
                Self::SOS { name: l_name, kind: l_kind, coefficients: l_coefficients },
                Self::SOS { name: r_name, kind: r_kind, coefficients: r_coefficients },
            ) => l_name == r_name && l_kind == r_kind && l_coefficients == r_coefficients,
            _ => false,
        }
    }
}

impl Eq for Constraint {}

impl Constraint {
    #[must_use]
    pub fn name(&self) -> String {
        match self {
            Self::Standard { name, .. } | Self::SOS { name, .. } => name.to_string(),
        }
    }

    #[must_use]
    pub fn coefficients(&self) -> &[Coefficient] {
        match self {
            Self::Standard { coefficients, .. } | Self::SOS { coefficients, .. } => coefficients,
        }
    }
}

#[allow(clippy::unwrap_used)]
impl LPPart for Constraint {
    type Output = Constraint;

    fn try_into(pair: Pair<'_, Rule>, gen: &mut SequenceGenerator) -> anyhow::Result<Self> {
        let mut parts = pair.into_inner().peekable();
        // Constraint name can be omitted in LP files, so we need to handle that case
        let name = get_name(&mut parts, gen, Rule::CONSTRAINT_NAME);
        let mut coefficients: Vec<_> = vec![];
        while let Some(p) = parts.peek() {
            if p.as_rule().is_cmp() {
                break;
            }
            coefficients.push(parts.next().unwrap());
        }
        let coefficients: anyhow::Result<Vec<_>> = coefficients
            .into_iter()
            .filter(|p| !matches!(p.as_rule(), Rule::PLUS | Rule::MINUS))
            .map(|p| p.into_inner().try_into())
            .collect();
        let sense = Cmp::from_str(parts.next().unwrap().as_str())?;
        let rhs = parts.next().unwrap().as_str().parse()?;
        Ok(Constraint::Standard { name, coefficients: coefficients?, sense, rhs })
    }
}
