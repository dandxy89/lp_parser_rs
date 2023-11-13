use std::str::FromStr;

use pest::iterators::Pair;

use crate::{
    common::RuleExt,
    model::{constraint::Constraint, lp_problem::LPProblem, objective::Objective, sense::Sense, sos::SOSClass, variable::VariableType},
    Rule,
};

#[allow(clippy::unwrap_used)]
fn compose_objective(pair: Pair<'_, Rule>) -> anyhow::Result<Objective> {
    let mut parts = pair.into_inner();
    let name = parts.next().unwrap().as_str().to_string();
    let coefficients: anyhow::Result<Vec<_>> = parts.map(|p| p.into_inner().try_into()).collect();
    Ok(Objective { name, coefficients: coefficients? })
}

#[allow(clippy::unwrap_used)]
fn compose_constraint(pair: Pair<'_, Rule>) -> anyhow::Result<Constraint> {
    let mut parts = pair.into_inner();
    let name = parts.next().unwrap().as_str().to_string();
    let mut coefficients: Vec<_> = vec![];
    while let Some(p) = parts.peek() {
        if p.as_rule().is_cmp() {
            break;
        }
        coefficients.push(parts.next().unwrap());
    }
    let coefficients: anyhow::Result<Vec<_>> = coefficients.into_iter().map(|p| p.into_inner().try_into()).collect();
    let sense = parts.next().unwrap().as_str().to_string();
    let rhs = parts.next().unwrap().as_str().parse()?;
    Ok(Constraint::Standard { name, coefficients: coefficients?, sense, rhs })
}

#[allow(clippy::unwrap_used)]
fn compose_sos(pair: Pair<'_, Rule>) -> anyhow::Result<Constraint> {
    let mut parts = pair.into_inner();
    let name = parts.next().unwrap().as_str().to_string();
    let kind = parts.next().unwrap().as_str().to_lowercase();
    let coefficients: anyhow::Result<Vec<_>> = parts.map(|p| p.into_inner().try_into()).collect();
    Ok(Constraint::SOS { name, kind: SOSClass::from_str(&kind)?, coefficients: coefficients? })
}

#[allow(clippy::wildcard_enum_match_arm, clippy::unwrap_used)]
fn get_bound(pair: Pair<'_, Rule>) -> Option<(&str, VariableType)> {
    match pair.as_rule() {
        Rule::LOWER_BOUND => {
            let mut parts = pair.into_inner();
            let name = parts.next().unwrap().as_str();
            let _ = parts.next();
            Some((name, VariableType::LB(parts.next().unwrap().as_str().parse().unwrap())))
        }
        Rule::LOWER_BOUND_REV => {
            let mut parts = pair.into_inner();
            let value = parts.next().unwrap().as_str().parse().unwrap();
            let _ = parts.next();
            Some((parts.next().unwrap().as_str(), VariableType::LB(value)))
        }
        Rule::UPPER_BOUND => {
            let mut parts = pair.into_inner();
            let name = parts.next().unwrap().as_str();
            let _ = parts.next();
            Some((name, VariableType::UB(parts.next().unwrap().as_str().parse().unwrap())))
        }
        Rule::BOUNDED => {
            let mut parts = pair.into_inner();
            let lb = parts.next().unwrap().as_str();
            let _ = parts.next();
            let name = parts.next().unwrap().as_str();
            let _ = parts.next();
            let ub = parts.next().unwrap().as_str();
            Some((name, VariableType::Bounded(lb.parse().unwrap(), ub.parse().unwrap(), false)))
        }
        Rule::FREE => {
            let mut parts = pair.into_inner();
            let name = parts.next().unwrap().as_str();
            Some((name, VariableType::Free))
        }
        _ => None,
    }
}

#[allow(clippy::wildcard_enum_match_arm)]
/// # Errors
/// Returns an error if the `compose` fails
pub fn compose(pair: Pair<'_, Rule>, mut parsed: LPProblem) -> anyhow::Result<LPProblem> {
    match pair.as_rule() {
        // Problem Name
        Rule::PROBLEM_NAME => return Ok(parsed.with_problem_name(pair.as_str())),
        // Problem sense
        Rule::MIN_SENSE => return Ok(parsed.with_sense(Sense::Minimize)),
        Rule::MAX_SENSE => return Ok(parsed.with_sense(Sense::Maximize)),
        // Problem Objectives
        Rule::OBJECTIVES => {
            let objectives: anyhow::Result<Vec<Objective>> = pair.into_inner().map(|inner_pair| compose_objective(inner_pair)).collect();
            parsed.add_objective(objectives?);
        }
        // Problem Constraints
        Rule::CONSTRAINTS => {
            let constraints: anyhow::Result<Vec<Constraint>> = pair.into_inner().map(|inner_pair| compose_constraint(inner_pair)).collect();
            parsed.add_constraints(constraints?);
        }
        Rule::SOS => {
            let constraints: anyhow::Result<Vec<Constraint>> = pair.into_inner().map(|inner_pair| compose_sos(inner_pair)).collect();
            parsed.add_constraints(constraints?);
        }
        // Problem Bounds
        Rule::BOUNDS => {
            for bound_pair in pair.into_inner() {
                if let Some((name, kind)) = get_bound(bound_pair) {
                    parsed.set_var_bounds(name, kind);
                }
            }
        }
        // Variable Bounds
        r @ (Rule::INTEGERS | Rule::GENERALS | Rule::BINARIES | Rule::SEMI_CONTINUOUS) => {
            for int_pair in pair.into_inner() {
                if matches!(int_pair.as_rule(), Rule::VARIABLE) {
                    parsed.set_var_bounds(int_pair.as_str(), r.into());
                }
            }
        }
        // Otherwise, skip!
        _ => (),
    }
    Ok(parsed)
}
