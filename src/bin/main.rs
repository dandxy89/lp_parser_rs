use std::{env, error::Error, path::PathBuf};

use lp_parser_rs::{parser::parse_file, problem::LpProblem};

fn dissemble_single_file(path: &str) -> Result<(), Box<dyn Error>> {
    let path = PathBuf::from(path);
    let input = parse_file(&path)?;

    let problem = LpProblem::parse(&input).unwrap();

    // Print the parsed LP problem
    println!("Parsed LP Problem:");
    if let Some(name) = problem.name() {
        println!("Problem name: {name}");
    }
    println!("Sense: {:?}", problem.sense);
    println!("Objectives count={}", problem.objective_count());
    println!("Constraint count={}", problem.constraint_count());
    println!("Variables count={}", problem.variable_count());

    Ok(())
}

#[cfg(feature = "diff")]
fn compare_lp_files(p1: &str, p2: &str) -> Result<(), Box<dyn Error>> {
    println!("Attempting to compare {p1} to {p2}");
    use diff::Diff;
    use lp_parser_rs::problem::LpProblemDiff;

    let path = PathBuf::from(p1);
    let input1 = parse_file(&path)?;
    let problem1 = LpProblem::parse(&input1).unwrap();

    let path = PathBuf::from(p2);
    let input2 = parse_file(&path)?;
    let problem2 = LpProblem::parse(&input2).unwrap();

    let difference: LpProblemDiff = problem1.diff(&problem2);

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

/// Parses and prints details of a single LP file or compares two LP files if the "diff" feature is enabled.
///
/// # Arguments
///
/// * `path` - A string slice that holds the path to the LP file.
///
/// # Returns
///
/// * `Result<(), Box<dyn Error>>` - Returns an Ok result if successful, or an error if parsing fails.
///
/// # Features
///
/// * If the "diff" feature is enabled, it can compare two LP files and print the differences in variables and constraints.
///
fn main() -> Result<(), Box<dyn Error>> {
    let mut args = env::args();
    args.next();
    let path = args.next().ok_or("Usage: nom_lp_parser <PATH_TO_FILE>")?;

    match (path, args.next()) {
        (p1, None) => dissemble_single_file(&p1),
        #[cfg(feature = "diff")]
        (p1, Some(p2)) => compare_lp_files(&p1, &p2),
        #[cfg(not(feature = "diff"))]
        (_, Some(_)) => Err("Diff feature not enabled".into()),
    }
}
