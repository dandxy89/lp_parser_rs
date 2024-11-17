use std::{env, error::Error, path::PathBuf};

use lp_parser_rs::parse::{parse_file, parse_lp_file};

fn main() -> Result<(), Box<dyn Error>> {
    // Get file path from command line argument
    let path = env::args().nth(1).ok_or("Usage: lp_parser <path_to_lp_file>")?;

    let path = PathBuf::from(path);

    // Parse the file content
    let contents = parse_file(&path)?;

    // Parse the LP problem
    let problem = parse_lp_file(&contents)?;

    println!("Parsed LP Problem:");
    println!("Problem name: {}", problem.problem_name);
    println!("Sense: {:?}", problem.problem_sense);
    println!("\nObjectives:");
    for obj in &problem.objectives {
        println!("  {}: {:?}", obj.name, obj.coefficients);
    }
    println!("\nConstraints:");
    for (name, constraint) in &problem.constraints {
        println!("  {}: {:?}", name, constraint);
    }
    println!("\nVariables:");
    for (name, var_type) in &problem.variables {
        println!("  {}: {:?}", name, var_type);
    }

    Ok(())
}
