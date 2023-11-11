use std::{
    fs::File,
    io::{BufReader, Read},
    path::Path,
};

use crate::{
    model::{Constraint, LPDefinition, Objective, Sense},
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
        parsed_contents = build(pair, parsed_contents)?;
    }

    Ok(dbg!(parsed_contents))
}

#[allow(clippy::unwrap_used)]
fn build_objective(pair: Pair<'_, Rule>) -> anyhow::Result<Objective> {
    let mut components = pair.into_inner();
    let name = components.next().unwrap().as_str().to_string();
    let coefficients: anyhow::Result<Vec<_>> = components.map(|p| p.into_inner().try_into()).collect();
    Ok(Objective { name, coefficients: coefficients? })
}

fn build_constraint(_pair: Pair<'_, Rule>) -> anyhow::Result<Constraint> {
    // pub name: String,
    // pub coefficients: Vec<Coefficient>,
    // pub sense: String,
    // pub rhs: f64,
    unimplemented!()
}

#[allow(clippy::wildcard_enum_match_arm)]
fn build(pair: Pair<'_, Rule>, mut parsed: LPDefinition) -> anyhow::Result<LPDefinition> {
    match pair.as_rule() {
        // Problem sense
        Rule::MIN_SENSE => Ok(parsed.with_sense(Sense::Minimize)),
        Rule::MAX_SENSE => Ok(parsed.with_sense(Sense::Maximize)),
        // Problem Objectives
        Rule::OBJECTIVES => {
            let objectives: anyhow::Result<Vec<Objective>> = pair.into_inner().map(|inner_pair| build_objective(inner_pair)).collect();
            parsed.add_objective(objectives?);
            Ok(parsed)
        }
        // Problem Constraints
        // Rule::CONSTRAINTS => {
        //     let constraints: anyhow::Result<Vec<Constraint>> = pair.into_inner().map(|inner_pair| build_constraint(inner_pair)).collect();
        //     parsed.add_constraints(constraints?);
        //     Ok(parsed)
        // }
        // Problem Bounds
        // Problem Integers
        // Problem Generals
        // Problem Binaries
        _ => Ok(parsed),
    }
}

//     Rule::CONSTRAINT_EXPR => todo!(),
//     Rule::CONSTRAINT_NAME => todo!(),
//     Rule::CONSTRAINT => todo!(),
//     Rule::CONSTRAINTS => todo!(),
//     // Problem Bounds
//     Rule::BOUND_PREFIX => todo!(),
//     Rule::BOUND => todo!(),
//     Rule::BOUNDS => todo!(),
//     // Problem Integers
//     Rule::INTEGER_PREFIX => todo!(),
//     Rule::INTEGERS => todo!(),
//     // Problem Generals
//     Rule::GENERALS_PREFIX => todo!(),
//     Rule::GENERALS => todo!(),
//     // Problem Binaries
//     Rule::BINARIES_PREFIX => todo!(),
//     Rule::BINARIES => todo!(),
//     // Other
//     Rule::WHITESPACE => todo!(),
//     Rule::POS_INFINITY => todo!(),
//     Rule::NEG_INFINITY => todo!(),
//     Rule::FLOAT => todo!(),
//     Rule::PLUS => todo!(),
//     Rule::MINUS => todo!(),
//     Rule::OPERATOR => todo!(),
//     Rule::COLON => todo!(),
//     Rule::ASTERIX => todo!(),
//     Rule::FREE => todo!(),
//     Rule::END => todo!(),
//     Rule::COMMENT_TEXT => todo!(),
//     Rule::COMMENTS => todo!(),
//     Rule::PROBLEM_SENSE => todo!(),
//     Rule::VALID_CHARS => todo!(),
//     Rule::CONSTRAINT_PREFIX => todo!(),
//     Rule::VARIABLE => todo!(),
//     Rule::GT => todo!(),
//     Rule::GTE => todo!(),
//     Rule::LT => todo!(),
//     Rule::LTE => todo!(),
//     Rule::EQ => todo!(),
//     Rule::CMP => todo!(),
//     Rule::EOF => todo!(),
