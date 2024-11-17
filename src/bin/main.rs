use std::{env, error::Error, path::PathBuf};

use lp_parser_rs::parse::{parse_file, parse_lp_file};

fn main() -> Result<(), Box<dyn Error>> {
    let path = env::args().nth(1).ok_or("Usage: lp_parser <PATH_TO_FILE>")?;
    let path = PathBuf::from(path);

    // Parse the file content
    let contents = parse_file(&path)?;

    // Parse the LP problem
    let problem = parse_lp_file(&contents)?;

    // Print the parsed LP problem
    println!("Parsed LP Problem:");
    if let Some(name) = problem.problem_name {
        println!("Problem name: {name}");
    }
    println!("Sense: {:?}", problem.problem_sense);
    println!("\nObjectives:");
    for obj in &problem.objectives {
        println!("  {}: {:?}", obj.name, obj.coefficients);
    }
    println!("\nConstraints:");
    for (name, constraint) in &problem.constraints {
        println!("  {name}: {constraint:?}");
    }
    println!("\nVariables:");
    for (name, var_type) in &problem.variables {
        println!("  {name}: {var_type:?}");
    }

    Ok(())
}
