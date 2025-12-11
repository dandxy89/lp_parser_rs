//! LP file writing and formatting utilities.
//!
//! This module provides functionality to write `LpProblem` instances back to
//! standard LP file format. It supports all major LP file components including
//! objectives, constraints, bounds, and variable type declarations.
//!
//! # Example
//!
//! ```rust
//! use lp_parser_rs::{LpProblem, writer::write_lp_string};
//!
//! let problem = LpProblem::new()
//!     .with_problem_name("Example".into())
//!     .with_sense(lp_parser_rs::model::Sense::Maximize);
//!
//! let lp_content = write_lp_string(&problem).expect("failed to write LP");
//! println!("{}", lp_content);
//! ```

use std::fmt::Write;

use crate::error::{LpParseError, LpResult};
use crate::model::{Constraint, Objective, Variable, VariableType};
use crate::problem::LpProblem;

/// Options for controlling LP file output format
#[derive(Debug, Clone)]
pub struct LpWriterOptions {
    /// Include problem name comment at the top
    pub include_problem_name: bool,
    /// Maximum line length before wrapping coefficients
    pub max_line_length: usize,
    /// Number of decimal places for coefficients
    pub decimal_precision: usize,
    /// Include empty lines between sections
    pub include_section_spacing: bool,
}

impl Default for LpWriterOptions {
    fn default() -> Self {
        Self { include_problem_name: true, max_line_length: 80, decimal_precision: 6, include_section_spacing: true }
    }
}

/// Write an `LpProblem` to a string in standard LP format
///
/// # Arguments
///
/// * `problem` - The LP problem to write
///
/// # Returns
///
/// A string containing the LP file content in standard format
///
/// # Errors
///
/// Returns an error if the problem cannot be formatted (e.g., invalid structure)
pub fn write_lp_string(problem: &LpProblem) -> LpResult<String> {
    write_lp_string_with_options(problem, &LpWriterOptions::default())
}

/// Write an `LpProblem` to a string with custom formatting options
///
/// # Arguments
///
/// * `problem` - The LP problem to write
/// * `options` - Formatting options for the output
///
/// # Returns
///
/// A string containing the LP file content
///
/// # Errors
///
/// Returns an error if the problem cannot be formatted (e.g., invalid structure)
pub fn write_lp_string_with_options(problem: &LpProblem, options: &LpWriterOptions) -> LpResult<String> {
    let mut output = String::new();

    // Write problem name comment if requested
    if options.include_problem_name {
        if let Some(name) = problem.name() {
            writeln!(output, "\\Problem name: {name}")
                .map_err(|err| LpParseError::io_error(format!("Failed to write problem name: {err}")))?;
            if options.include_section_spacing {
                writeln!(output).map_err(|err| LpParseError::io_error(format!("Failed to write newline: {err}")))?;
            }
        }
    }

    // Write sense and objectives
    write_objectives_section(&mut output, problem, options)?;

    // Write constraints
    if !problem.constraints.is_empty() {
        if options.include_section_spacing {
            writeln!(output).map_err(|err| LpParseError::io_error(format!("Failed to write newline: {err}")))?;
        }
        write_constraints_section(&mut output, problem, options)?;
    }

    // Write bounds
    write_bounds_section(&mut output, problem, options)?;

    // Write variable type sections
    write_variable_types_sections(&mut output, problem, options)?;

    // Write end marker
    if options.include_section_spacing {
        writeln!(output).map_err(|err| LpParseError::io_error(format!("Failed to write newline: {err}")))?;
    }
    writeln!(output, "End").map_err(|err| LpParseError::io_error(format!("Failed to write end marker: {err}")))?;

    Ok(output)
}

/// Write the objectives section (sense + objectives)
fn write_objectives_section(output: &mut String, problem: &LpProblem, options: &LpWriterOptions) -> LpResult<()> {
    // Write sense
    writeln!(output, "{}", problem.sense).map_err(|err| LpParseError::io_error(format!("Failed to write sense: {err}")))?;

    // Write objectives
    for objective in problem.objectives.values() {
        write_objective(output, objective, options)?;
    }

    Ok(())
}

/// Write a single objective
fn write_objective(output: &mut String, objective: &Objective, options: &LpWriterOptions) -> LpResult<()> {
    write!(output, " {}: ", objective.name).map_err(|err| LpParseError::io_error(format!("Failed to write objective name: {err}")))?;

    write_coefficients_line(output, &objective.coefficients, options)?;
    writeln!(output).map_err(|err| LpParseError::io_error(format!("Failed to write newline: {err}")))?;

    Ok(())
}

/// Write the constraints section
fn write_constraints_section(output: &mut String, problem: &LpProblem, options: &LpWriterOptions) -> LpResult<()> {
    writeln!(output, "Subject To").map_err(|err| LpParseError::io_error(format!("Failed to write constraints header: {err}")))?;

    for constraint in problem.constraints.values() {
        write_constraint(output, constraint, options)?;
    }

    Ok(())
}

/// Write a single constraint
fn write_constraint(output: &mut String, constraint: &Constraint, options: &LpWriterOptions) -> LpResult<()> {
    match constraint {
        Constraint::Standard { name, coefficients, operator, rhs } => {
            write!(output, " {name}: ").map_err(|err| LpParseError::io_error(format!("Failed to write constraint name: {err}")))?;

            write_coefficients_line(output, coefficients, options)?;

            writeln!(output, " {} {}", operator, format_number(*rhs, options.decimal_precision))
                .map_err(|err| LpParseError::io_error(format!("Failed to write constraint RHS: {err}")))?;
        }
        Constraint::SOS { name, sos_type, weights } => {
            write!(output, " {name}: {sos_type}:: ")
                .map_err(|err| LpParseError::io_error(format!("Failed to write SOS constraint: {err}")))?;

            for (i, weight) in weights.iter().enumerate() {
                if i > 0 {
                    write!(output, " ").map_err(|err| LpParseError::io_error(format!("Failed to write space: {err}")))?;
                }
                write!(output, "{}:{}", weight.name, format_number(weight.value, options.decimal_precision))
                    .map_err(|err| LpParseError::io_error(format!("Failed to write SOS weight: {err}")))?;
            }
            writeln!(output).map_err(|err| LpParseError::io_error(format!("Failed to write newline: {err}")))?;
        }
    }

    Ok(())
}

/// Write the bounds section
fn write_bounds_section(output: &mut String, problem: &LpProblem, options: &LpWriterOptions) -> LpResult<()> {
    let mut has_bounds = false;

    // First pass: check if we have any bounds to write
    for variable in problem.variables.values() {
        if needs_bounds_declaration(&variable.var_type) {
            has_bounds = true;
            break;
        }
    }

    if has_bounds {
        if options.include_section_spacing {
            writeln!(output).map_err(|err| LpParseError::io_error(format!("Failed to write newline: {err}")))?;
        }
        writeln!(output, "Bounds").map_err(|err| LpParseError::io_error(format!("Failed to write bounds header: {err}")))?;

        for variable in problem.variables.values() {
            write_variable_bounds(output, variable, options)?;
        }
    }

    Ok(())
}

/// Check if a variable type needs bounds declaration
const fn needs_bounds_declaration(var_type: &VariableType) -> bool {
    matches!(var_type, VariableType::LowerBound(_) | VariableType::UpperBound(_) | VariableType::DoubleBound(_, _) | VariableType::Free)
}

/// Write bounds for a single variable
fn write_variable_bounds(output: &mut String, variable: &Variable, options: &LpWriterOptions) -> LpResult<()> {
    match &variable.var_type {
        VariableType::Free => {
            writeln!(output, "{} free", variable.name)
                .map_err(|err| LpParseError::io_error(format!("Failed to write free variable: {err}")))?;
        }
        VariableType::LowerBound(bound) => {
            writeln!(output, "{} >= {}", variable.name, format_number(*bound, options.decimal_precision))
                .map_err(|err| LpParseError::io_error(format!("Failed to write lower bound: {err}")))?;
        }
        VariableType::UpperBound(bound) => {
            writeln!(output, "{} <= {}", variable.name, format_number(*bound, options.decimal_precision))
                .map_err(|err| LpParseError::io_error(format!("Failed to write upper bound: {err}")))?;
        }
        VariableType::DoubleBound(lower, upper) => {
            writeln!(
                output,
                "{} <= {} <= {}",
                format_number(*lower, options.decimal_precision),
                variable.name,
                format_number(*upper, options.decimal_precision)
            )
            .map_err(|err| LpParseError::io_error(format!("Failed to write double bound: {err}")))?;
        }
        _ => {} // Other types don't need bounds declarations
    }

    Ok(())
}

/// Write variable type sections (binaries, integers, etc.)
fn write_variable_types_sections(output: &mut String, problem: &LpProblem, options: &LpWriterOptions) -> LpResult<()> {
    // Group variables by type
    let mut binaries = Vec::new();
    let mut integers = Vec::new();
    let mut semi_continuous = Vec::new();

    for variable in problem.variables.values() {
        match variable.var_type {
            VariableType::Binary => binaries.push(variable.name),
            VariableType::Integer => integers.push(variable.name),
            VariableType::SemiContinuous => semi_continuous.push(variable.name),
            _ => {} // Other types handled elsewhere
        }
    }

    // Write each section if it has variables
    if !binaries.is_empty() {
        write_variable_type_section(output, "Binaries", &binaries, options)?;
    }

    if !integers.is_empty() {
        write_variable_type_section(output, "Integers", &integers, options)?;
    }

    if !semi_continuous.is_empty() {
        write_variable_type_section(output, "Semi-Continuous", &semi_continuous, options)?;
    }

    Ok(())
}

/// Write a variable type section
fn write_variable_type_section(output: &mut String, section_name: &str, variables: &[&str], options: &LpWriterOptions) -> LpResult<()> {
    if options.include_section_spacing {
        writeln!(output).map_err(|err| LpParseError::io_error(format!("Failed to write newline: {err}")))?;
    }
    writeln!(output, "{section_name}").map_err(|err| LpParseError::io_error(format!("Failed to write section header: {err}")))?;

    // Write variables, potentially wrapping lines
    let mut current_line_length = 0;
    for (i, &var_name) in variables.iter().enumerate() {
        let separator = " ";
        let var_text = format!("{separator}{var_name}");

        if current_line_length + var_text.len() > options.max_line_length && i > 0 {
            writeln!(output).map_err(|err| LpParseError::io_error(format!("Failed to write newline: {err}")))?;
            write!(output, " {var_name}").map_err(|err| LpParseError::io_error(format!("Failed to write variable: {err}")))?;
            current_line_length = var_name.len() + 1;
        } else {
            write!(output, "{var_text}").map_err(|err| LpParseError::io_error(format!("Failed to write variable: {err}")))?;
            current_line_length += var_text.len();
        }
    }
    writeln!(output).map_err(|err| LpParseError::io_error(format!("Failed to write newline: {err}")))?;

    Ok(())
}

/// Write a line of coefficients with proper formatting
fn write_coefficients_line(output: &mut String, coefficients: &[crate::model::Coefficient], options: &LpWriterOptions) -> LpResult<()> {
    for (i, coeff) in coefficients.iter().enumerate() {
        let formatted_coeff = format_coefficient(coeff, i == 0, options.decimal_precision);
        write!(output, "{formatted_coeff}").map_err(|err| LpParseError::io_error(format!("Failed to write coefficient: {err}")))?;
    }

    Ok(())
}

/// Tolerance for checking if a coefficient is effectively 1.0
const COEFF_ONE_EPSILON: f64 = 1e-10;

/// Format a coefficient with proper sign handling
fn format_coefficient(coeff: &crate::model::Coefficient, is_first: bool, precision: usize) -> String {
    let abs_value = coeff.value.abs();
    let sign = if coeff.value < 0.0 { "-" } else { "+" };
    let is_one = (abs_value - 1.0).abs() < COEFF_ONE_EPSILON;

    if is_first {
        if coeff.value < 0.0 {
            if is_one { format!("- {}", coeff.name) } else { format!("- {} {}", format_number(abs_value, precision), coeff.name) }
        } else if is_one {
            coeff.name.to_string()
        } else {
            format!("{} {}", format_number(abs_value, precision), coeff.name)
        }
    } else if is_one {
        format!(" {sign} {}", coeff.name)
    } else {
        format!(" {sign} {} {}", format_number(abs_value, precision), coeff.name)
    }
}

/// Format a number with specified precision, removing trailing zeros
#[allow(clippy::uninlined_format_args, clippy::cast_precision_loss, clippy::cast_possible_truncation)]
fn format_number(value: f64, precision: usize) -> String {
    // Check if value is a whole number and within safe i64 range for integer formatting
    let is_whole_number = value.fract().abs() < f64::EPSILON;
    let is_safe_for_i64 = value >= (i64::MIN as f64) && value <= (i64::MAX as f64);

    if is_whole_number && is_safe_for_i64 && value.abs() < 1e10 {
        // Integer value within safe range - format as integer
        format!("{}", value as i64)
    } else {
        // Decimal value or out of i64 range - format with precision and remove trailing zeros
        let formatted = format!("{:.precision$}", value, precision = precision);
        formatted.trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

#[cfg(test)]
mod tests {
    use std::borrow::Cow;

    use super::*;
    use crate::model::{Coefficient, ComparisonOp, Constraint, Objective, Sense, VariableType};
    use crate::problem::LpProblem;

    #[test]
    fn test_format_number() {
        assert_eq!(format_number(1.0, 6), "1");
        assert_eq!(format_number(1.5, 6), "1.5");
        assert_eq!(format_number(1.500_000, 6), "1.5");
        assert_eq!(format_number(0.0, 6), "0");
        assert_eq!(format_number(-1.0, 6), "-1");
        assert_eq!(format_number(2.789, 2), "2.79");
    }

    #[test]
    fn test_format_coefficient() {
        let coeff1 = Coefficient { name: "x1", value: 1.0 };
        assert_eq!(format_coefficient(&coeff1, true, 6), "x1");

        let coeff2 = Coefficient { name: "x2", value: -1.0 };
        assert_eq!(format_coefficient(&coeff2, true, 6), "- x2");

        let coeff3 = Coefficient { name: "x3", value: 2.5 };
        assert_eq!(format_coefficient(&coeff3, false, 6), " + 2.5 x3");

        let coeff4 = Coefficient { name: "x4", value: -3.7 };
        assert_eq!(format_coefficient(&coeff4, false, 6), " - 3.7 x4");
    }

    #[test]
    fn test_write_empty_problem() {
        let problem = LpProblem::new();
        let result = write_lp_string(&problem).unwrap();

        assert!(result.contains("Minimize"));
        assert!(result.contains("End"));
    }

    #[test]
    fn test_write_simple_problem() {
        let mut problem = LpProblem::new().with_problem_name("Test Problem".into()).with_sense(Sense::Maximize);

        // Add objective
        let objective = Objective {
            name: Cow::Borrowed("profit"),
            coefficients: vec![Coefficient { name: "x1", value: 3.0 }, Coefficient { name: "x2", value: 2.0 }],
        };
        problem.add_objective(objective);

        // Add constraint
        let constraint = Constraint::Standard {
            name: Cow::Borrowed("capacity"),
            coefficients: vec![Coefficient { name: "x1", value: 1.0 }, Coefficient { name: "x2", value: 1.0 }],
            operator: ComparisonOp::LTE,
            rhs: 100.0,
        };
        problem.add_constraint(constraint);

        let result = write_lp_string(&problem).unwrap();

        assert!(result.contains("\\Problem name: Test Problem"));
        assert!(result.contains("Maximize"));
        assert!(result.contains("profit: 3 x1 + 2 x2"));
        assert!(result.contains("Subject To"));
        assert!(result.contains("capacity: x1 + x2 <= 100"));
        assert!(result.contains("End"));
    }

    #[test]
    fn test_complete_lp_rewriting_workflow() {
        // This test demonstrates a complete workflow of:
        // 1. Parsing an LP file
        // 2. Modifying the problem
        // 3. Writing it back to LP format

        // Step 1: Parse an existing LP problem
        let original_lp = r"
Maximize
profit: 3 x1 + 2 x2

Subject To
capacity: x1 + x2 <= 100
material: 2 x1 + x2 <= 150

Bounds
x1 >= 0
x2 >= 0

End";

        let mut problem = crate::problem::LpProblem::parse(original_lp).unwrap();

        // Step 2: Modify the problem

        // Change objective coefficient
        problem.update_objective_coefficient("profit", "x1", 5.0).unwrap();

        // Add new variable to objective
        problem.update_objective_coefficient("profit", "x3", 1.5).unwrap();

        // Modify constraint
        problem.update_constraint_coefficient("capacity", "x3", 0.5).unwrap();
        problem.update_constraint_rhs("material", 200.0).unwrap();

        // Add new constraint
        let new_constraint = Constraint::Standard {
            name: Cow::Borrowed("demand"),
            coefficients: vec![Coefficient { name: "x1", value: 1.0 }],
            operator: ComparisonOp::GTE,
            rhs: 20.0,
        };
        problem.add_constraint(new_constraint);

        // Update variable types
        problem.update_variable_type("x1", VariableType::Integer).unwrap();
        problem.update_variable_type("x3", VariableType::Binary).unwrap();

        // Rename elements
        problem.rename_variable("x2", "production").unwrap();
        problem.rename_constraint("capacity", "resource_limit").unwrap();

        // Step 3: Write the modified problem back to LP format
        let result = write_lp_string(&problem).unwrap();

        assert!(result.contains("Maximize"));
        assert!(result.contains("5 x1"));
        assert!(result.contains("2 production"));
        assert!(result.contains("1.5 x3"));
        assert!(result.contains("resource_limit: x1 + production + 0.5 x3 <= 100"));
        assert!(result.contains("material: 2 x1 + production <= 200"));
        assert!(result.contains("demand: x1 >= 20"));
        assert!(result.contains("Integers"));
        assert!(result.contains("x1"));
        assert!(result.contains("Binaries"));
        assert!(result.contains("x3"));
        assert!(result.contains("End"));

        let reparsed_problem = crate::problem::LpProblem::parse(&result).unwrap();
        assert_eq!(reparsed_problem.sense, crate::model::Sense::Maximize);
        assert_eq!(reparsed_problem.constraint_count(), 3);
        assert_eq!(reparsed_problem.variable_count(), 3);
        assert!(reparsed_problem.variables.contains_key("production"));
        assert!(!reparsed_problem.variables.contains_key("x2"));
        assert!(reparsed_problem.constraints.contains_key("resource_limit"));
        assert!(!reparsed_problem.constraints.contains_key("capacity"));
    }

    #[test]
    fn test_write_problem_with_bounds_and_variable_types() {
        let mut problem = LpProblem::new().with_problem_name("Complex Problem".into()).with_sense(crate::model::Sense::Minimize);

        // Add objective
        let objective = Objective {
            name: Cow::Borrowed("cost"),
            coefficients: vec![
                Coefficient { name: "x1", value: 10.0 },
                Coefficient { name: "x2", value: 15.0 },
                Coefficient { name: "x3", value: 20.0 },
            ],
        };
        problem.add_objective(objective);

        // Add constraints
        let constraint1 = Constraint::Standard {
            name: Cow::Borrowed("resource1"),
            coefficients: vec![
                Coefficient { name: "x1", value: 1.0 },
                Coefficient { name: "x2", value: 2.0 },
                Coefficient { name: "x3", value: 1.0 },
            ],
            operator: ComparisonOp::LTE,
            rhs: 100.0,
        };
        problem.add_constraint(constraint1);

        // Set variable types and bounds
        problem.update_variable_type("x1", VariableType::DoubleBound(0.0, 50.0)).unwrap();
        problem.update_variable_type("x2", VariableType::Binary).unwrap();
        problem.update_variable_type("x3", VariableType::Integer).unwrap();

        let result = write_lp_string(&problem).unwrap();

        assert!(result.contains("\\Problem name: Complex Problem"));
        assert!(result.contains("Minimize"));
        assert!(result.contains("cost: 10 x1 + 15 x2 + 20 x3"));
        assert!(result.contains("Subject To"));
        assert!(result.contains("resource1: x1 + 2 x2 + x3 <= 100"));
        assert!(result.contains("Bounds"));
        assert!(result.contains("0 <= x1 <= 50"));
        assert!(result.contains("Binaries"));
        assert!(result.contains("x2"));
        assert!(result.contains("Integers"));
        assert!(result.contains("x3"));
        assert!(result.contains("End"));
    }

    #[test]
    fn test_write_with_sos_constraints() {
        let mut problem = LpProblem::new();

        // Add SOS constraint
        let sos_constraint = Constraint::SOS {
            name: Cow::Borrowed("sos1"),
            sos_type: crate::model::SOSType::S1,
            weights: vec![
                Coefficient { name: "x1", value: 1.0 },
                Coefficient { name: "x2", value: 2.0 },
                Coefficient { name: "x3", value: 3.0 },
            ],
        };
        problem.add_constraint(sos_constraint);

        let result = write_lp_string(&problem).unwrap();

        assert!(result.contains("Subject To"));
        assert!(result.contains("sos1: S1:: x1:1 x2:2 x3:3"));
        assert!(result.contains("End"));
    }
}
