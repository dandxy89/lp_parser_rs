use std::{
    fs::File,
    io::{BufReader, Read},
    path::Path,
};

use crate::{
    common::RuleExt,
    model::{Constraint, LPDefinition, Objective, Sense, VariableType},
    LParser, Rule,
};
use pest::{iterators::Pair, Parser};

/// # Errors
/// Returns an error if the `read_to_string` or `open` fails
pub fn parse_file(path: &Path) -> anyhow::Result<String> {
    let Ok(file) = File::open(path) else {
        anyhow::bail!("Could not open file at {path:?}");
    };
    let mut buf_reader = BufReader::new(file);
    let mut contents = String::new();
    buf_reader.read_to_string(&mut contents)?;

    Ok(contents)
}

/// # Errors
/// Returns an error if the parse fails
pub fn parse_lp_file(contents: &str) -> anyhow::Result<LPDefinition> {
    let mut parsed = LParser::parse(Rule::LP_FILE, contents)?;
    let Some(pair) = parsed.next() else {
        anyhow::bail!("Invalid LP file");
    };
    let mut parsed_contents = LPDefinition::default();
    for pair in pair.clone().into_inner() {
        parsed_contents = compose(pair, parsed_contents)?;
    }
    Ok(parsed_contents)
}

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
    Ok(Constraint { name, coefficients: coefficients?, sense, rhs })
}

#[allow(clippy::wildcard_enum_match_arm, clippy::unwrap_used)]
fn get_bound(pair: Pair<'_, Rule>) -> Option<(&str, VariableType)> {
    match pair.as_rule() {
        Rule::LOWER_BOUND => {
            let mut parts = pair.into_inner();
            let name = parts.next().unwrap().as_str().trim();
            let _ = parts.next();
            Some((name, VariableType::LB(parts.next().unwrap().as_str().trim().parse().unwrap())))
        }
        Rule::UPPER_BOUND => {
            let mut parts = pair.into_inner();
            let name = parts.next().unwrap().as_str().trim();
            let _ = parts.next();
            Some((name, VariableType::UB(parts.next().unwrap().as_str().trim().parse().unwrap())))
        }
        Rule::BOUNDED => {
            let mut parts = pair.into_inner();
            let lb = parts.next().unwrap().as_str().trim();
            let _ = parts.next();
            let name = parts.next().unwrap().as_str().trim();
            let _ = parts.next();
            let ub = parts.next().unwrap().as_str().trim();
            Some((name, VariableType::Bounded(lb.parse().unwrap(), ub.parse().unwrap())))
        }
        Rule::FREE => {
            let mut parts = pair.into_inner();
            let name = parts.next().unwrap().as_str().trim();
            Some((name, VariableType::Free))
        }
        _ => None,
    }
}

#[allow(clippy::wildcard_enum_match_arm)]
fn compose(pair: Pair<'_, Rule>, mut parsed: LPDefinition) -> anyhow::Result<LPDefinition> {
    match pair.as_rule() {
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
        // Problem Bounds
        Rule::BOUNDS => {
            for bound_pair in pair.into_inner() {
                if let Some((name, kind)) = get_bound(bound_pair) {
                    parsed.set_var_bounds(name, kind);
                }
            }
        }
        // Problem Integers
        Rule::INTEGERS => {
            for int_pair in pair.into_inner() {
                if matches!(int_pair.as_rule(), Rule::VARIABLE) {
                    parsed.set_var_bounds(int_pair.as_str(), VariableType::Integer);
                }
            }
        }
        // Problem Generals
        Rule::GENERALS => {
            for gen_pair in pair.into_inner() {
                if matches!(gen_pair.as_rule(), Rule::VARIABLE) {
                    parsed.set_var_bounds(gen_pair.as_str(), VariableType::General);
                }
            }
        }
        // Problem Binaries
        Rule::BINARIES => {
            for bin_pair in pair.into_inner() {
                if matches!(bin_pair.as_rule(), Rule::VARIABLE) {
                    parsed.set_var_bounds(bin_pair.as_str(), VariableType::Binary);
                }
            }
        }
        // Otherwise, skip!
        _ => (),
    }
    Ok(parsed)
}
