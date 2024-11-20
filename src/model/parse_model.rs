use pest::iterators::Pair;
use unique_id::sequence::SequenceGenerator;

use crate::{
    model::{
        constraint::Constraint,
        lp_error::LPParserError,
        lp_problem::{LPPart, LPProblem},
        objective::Objective,
        sense::Sense,
        sos::SOSClass,
        variable::get_bound,
        ParseResult,
    },
    Rule,
};

#[inline]
#[allow(clippy::wildcard_enum_match_arm)]
/// # Errors
/// Returns an error if the `compose` fails
pub fn compose(lp_pair: Pair<'_, Rule>, mut parsed: LPProblem, id_gen: &mut SequenceGenerator) -> Result<LPProblem, LPParserError> {
    match lp_pair.as_rule() {
        // Problem Name
        Rule::PROBLEM_NAME => return Ok(parsed.with_problem_name(lp_pair.as_str())),
        // Problem sense
        Rule::MIN_SENSE => return Ok(parsed.with_sense(Sense::Minimize)),
        Rule::MAX_SENSE => return Ok(parsed.with_sense(Sense::Maximize)),
        // Problem Objectives
        Rule::OBJECTIVES => {
            let parts: ParseResult<_> = lp_pair.into_inner().map(|obj_part| <Objective as LPPart>::try_into(obj_part, id_gen)).collect();
            parsed.add_objectives(parts?);
        }
        // Problem Constraints
        Rule::CONSTRAINTS => {
            for part in lp_pair.into_inner().map(|cons_part| <Constraint as LPPart>::try_into(cons_part, id_gen)) {
                if let Ok(constraint) = part {
                    parsed.add_constraint(constraint);
                } else {
                    log::warn!("Failed to parse constraint: {part:?}");
                }
            }
        }
        Rule::SOS => {
            let parts: ParseResult<_> = lp_pair.into_inner().map(|sos_part| <SOSClass as LPPart>::try_into(sos_part, id_gen)).collect();
            parsed.add_constraints(parts?);
        }
        // Problem Bounds
        Rule::BOUNDS => {
            for bound_pair in lp_pair.into_inner() {
                if let Some((name, kind)) = get_bound(&bound_pair) {
                    parsed.set_variable_bounds(name, kind);
                } else {
                    log::warn!("Failed to parse bound: {bound_pair:?}");
                }
            }
        }
        // Variable Bounds
        bound @ (Rule::INTEGERS | Rule::GENERALS | Rule::BINARIES | Rule::SEMI_CONTINUOUS) => {
            for rule in lp_pair.into_inner() {
                if matches!(rule.as_rule(), Rule::VARIABLE) {
                    parsed.set_variable_bounds(rule.as_str(), bound.into());
                } else {
                    log::warn!("Failed to variable bound: {rule:?}");
                }
            }
        }
        // Otherwise, skip!
        _ => (),
    }

    Ok(parsed)
}
