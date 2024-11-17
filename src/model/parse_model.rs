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
pub fn compose(pair: Pair<'_, Rule>, mut parsed: LPProblem, gen: &mut SequenceGenerator) -> Result<LPProblem, LPParserError> {
    match pair.as_rule() {
        // Problem Name
        Rule::PROBLEM_NAME => return Ok(parsed.with_problem_name(pair.as_str())),
        // Problem sense
        Rule::MIN_SENSE => return Ok(parsed.with_sense(Sense::Minimize)),
        Rule::MAX_SENSE => return Ok(parsed.with_sense(Sense::Maximize)),
        // Problem Objectives
        Rule::OBJECTIVES => {
            let parts: ParseResult<_> = pair.into_inner().map(|p| <Objective as LPPart>::try_into(p, gen)).collect();
            parsed.add_objective(parts?);
        }
        // Problem Constraints
        Rule::CONSTRAINTS => {
            let parts: ParseResult<_> = pair.into_inner().map(|p| <Constraint as LPPart>::try_into(p, gen)).collect();
            parsed.add_constraints(parts?);
        }
        Rule::SOS => {
            let parts: ParseResult<_> = pair.into_inner().map(|p| <SOSClass as LPPart>::try_into(p, gen)).collect();
            parsed.add_constraints(parts?);
        }
        // Problem Bounds
        Rule::BOUNDS => {
            for bound_pair in pair.into_inner() {
                if let Some((name, kind)) = get_bound(&bound_pair) {
                    parsed.set_variable_bounds(name, kind);
                }
            }
        }
        // Variable Bounds
        r @ (Rule::INTEGERS | Rule::GENERALS | Rule::BINARIES | Rule::SEMI_CONTINUOUS) => {
            for p in pair.into_inner() {
                if matches!(p.as_rule(), Rule::VARIABLE) {
                    parsed.set_variable_bounds(p.as_str(), r.into());
                }
            }
        }
        // Otherwise, skip!
        _ => (),
    }
    Ok(parsed)
}
