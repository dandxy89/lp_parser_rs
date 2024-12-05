use std::{env, error::Error, path::PathBuf};

use lp_parser_rs::{
    model::lp_problem::LPProblem,
    parse::{parse_file, parse_lp_file},
};

fn parse_lp(path: &str) -> Result<LPProblem, Box<dyn Error>> {
    let path = PathBuf::from(path);
    let file = parse_file(&path)?;
    let problem = parse_lp_file(&file)?;
    Ok(problem)
}

fn dissemble_single_file(path: &str) -> Result<(), Box<dyn Error>> {
    println!("Attempting to parse {path}");
    let problem = parse_lp(path)?;

    // Print the parsed LP problem
    println!("Parsed LP Problem:");
    if let Some(name) = problem.problem_name {
        println!("Problem name: {name}");
    }
    println!("Sense: {:?}", problem.problem_sense);
    println!("Objectives count={}", problem.objectives.len());
    println!("Constraint count={}", problem.constraints.len());
    println!("Variables count={}", problem.variables.len());

    Ok(())
}

#[cfg(feature = "diff")]
fn compare_lp_files(p1: &str, p2: &str) -> Result<(), Box<dyn Error>> {
    println!("Attempting to compare {p1} to {p2}");
    use diff::Diff;
    use lp_parser_rs::model::lp_problem::LPProblemDiff;

    let problem1 = parse_lp(p1)?;
    let problem2 = parse_lp(p2)?;

    let difference: LPProblemDiff = problem1.diff(&problem2);
    // Different variables
    difference.variables.altered.iter().for_each(|(k, v)| {
        println!("Variable {k} changed from {v:?} to {:?}", problem2.variables.get(k).unwrap());
    });
    // Remove variables
    difference.variables.removed.iter().for_each(|k| {
        println!("Variable {k} removed");
    });
    // Constraints altered
    difference.constraints.altered.iter().for_each(|(k, v)| {
        println!("Constraint {k} changed from {v:?} to {:?}", problem2.constraints.get(k).unwrap());
    });
    // Constraints removed
    difference.constraints.removed.iter().for_each(|k| {
        println!("Constraint {k} removed");
    });

    Ok(())
}

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = env::args();
    args.next();
    let path = args.next().ok_or("Usage: lp_parser <PATH_TO_FILE>")?;

    match (path, args.next()) {
        (p1, None) => dissemble_single_file(&p1),
        #[cfg(feature = "diff")]
        (p1, Some(p2)) => compare_lp_files(&p1, &p2),
        #[cfg(not(feature = "diff"))]
        (_, Some(_)) => Err("Diff feature not enabled".into()),
    }
}
