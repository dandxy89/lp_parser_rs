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
//!     .with_problem_name("Example")
//!     .with_sense(lp_parser_rs::model::Sense::Maximize);
//!
//! let lp_content = write_lp_string(&problem);
//! println!("{}", lp_content);
//! ```

use std::fmt::Write;

use crate::NUMERIC_EPSILON;
use crate::interner::NameInterner;
use crate::model::{Coefficient, Constraint, Objective, Variable, VariableType};
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
#[must_use]
pub fn write_lp_string(problem: &LpProblem) -> String {
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
#[must_use]
// The only panic is the expect on fmt::Write to String, which is infallible.
#[allow(clippy::missing_panics_doc)]
pub fn write_lp_string_with_options(problem: &LpProblem, options: &LpWriterOptions) -> String {
    let mut output = String::new();
    build_lp(&mut output, problem, options).expect("fmt::Write to String is infallible");
    output
}

/// Build the full LP document into `output`.
fn build_lp(output: &mut String, problem: &LpProblem, options: &LpWriterOptions) -> std::fmt::Result {
    // Write problem name comment if requested
    if options.include_problem_name {
        if let Some(name) = problem.name() {
            writeln!(output, "\\Problem name: {name}")?;
            if options.include_section_spacing {
                writeln!(output)?;
            }
        }
    }

    // Write sense and objectives
    write_objectives_section(output, problem, options)?;

    // Write constraints
    if !problem.constraints.is_empty() {
        if options.include_section_spacing {
            writeln!(output)?;
        }
        write_constraints_section(output, problem, options)?;
    }

    // Write bounds
    write_bounds_section(output, problem, options)?;

    // Write variable type sections
    write_variable_types_sections(output, problem, options)?;

    // Write SOS constraints (their own section; not valid `Subject To` syntax)
    write_sos_section(output, problem, options)?;

    // Write end marker
    if options.include_section_spacing {
        writeln!(output)?;
    }
    writeln!(output, "End")
}

/// Write the objectives section (sense + objectives)
fn write_objectives_section(output: &mut String, problem: &LpProblem, options: &LpWriterOptions) -> std::fmt::Result {
    writeln!(output, "{}", problem.sense)?;

    for objective in problem.objectives.values() {
        write_objective(output, objective, &problem.interner, options)?;
    }

    Ok(())
}

/// Write a single objective
fn write_objective(output: &mut String, objective: &Objective, interner: &NameInterner, options: &LpWriterOptions) -> std::fmt::Result {
    let name = interner.resolve(objective.name);
    if objective.coefficients.is_empty() {
        // A dangling ` name: ` line is not valid LP syntax.
        eprintln!("objective '{name}' has no coefficients and will be omitted from the LP output");
        return Ok(());
    }
    write!(output, " {name}: ")?;

    write_coefficients_line(output, &objective.coefficients, interner, options)?;
    writeln!(output)
}

/// Write the constraints section (standard constraints only; SOS constraints
/// belong in their own `SOS` section)
fn write_constraints_section(output: &mut String, problem: &LpProblem, options: &LpWriterOptions) -> std::fmt::Result {
    writeln!(output, "Subject To")?;

    for constraint in problem.constraints.values() {
        if matches!(constraint, Constraint::Standard { .. }) {
            write_constraint(output, constraint, &problem.interner, options)?;
        }
    }

    Ok(())
}

/// Write the SOS section. The LP grammar only accepts SOS constraints inside a
/// dedicated `SOS` section (after bounds and variable-type sections), so they
/// must not be emitted under `Subject To`.
fn write_sos_section(output: &mut String, problem: &LpProblem, options: &LpWriterOptions) -> std::fmt::Result {
    let mut wrote_header = false;
    for constraint in problem.constraints.values() {
        if matches!(constraint, Constraint::SOS { .. }) {
            if !wrote_header {
                if options.include_section_spacing {
                    writeln!(output)?;
                }
                writeln!(output, "SOS")?;
                wrote_header = true;
            }
            write_constraint(output, constraint, &problem.interner, options)?;
        }
    }
    Ok(())
}

/// Write a single constraint
fn write_constraint(output: &mut String, constraint: &Constraint, interner: &NameInterner, options: &LpWriterOptions) -> std::fmt::Result {
    match constraint {
        Constraint::Standard { name, coefficients, operator, rhs, .. } => {
            let resolved_name = interner.resolve(*name);
            if coefficients.is_empty() {
                // A dangling ` name:  <= rhs` line is not valid LP syntax.
                eprintln!("constraint '{resolved_name}' has no coefficients and will be omitted from the LP output");
                return Ok(());
            }
            write!(output, " {resolved_name}: ")?;

            write_coefficients_line(output, coefficients, interner, options)?;

            write!(output, " {operator} ")?;
            write_number(output, *rhs, options.decimal_precision)?;
            writeln!(output)
        }
        Constraint::SOS { name, sos_type, weights, .. } => {
            let resolved_name = interner.resolve(*name);
            write!(output, " {resolved_name}: {sos_type}:: ")?;

            for (i, weight) in weights.iter().enumerate() {
                if i > 0 {
                    write!(output, " ")?;
                }
                let var_name = interner.resolve(weight.name);
                write!(output, "{var_name}:")?;
                write_number(output, weight.value, options.decimal_precision)?;
            }
            writeln!(output)
        }
    }
}

/// Write the bounds section
fn write_bounds_section(output: &mut String, problem: &LpProblem, options: &LpWriterOptions) -> std::fmt::Result {
    let has_bounds = problem.variables.values().any(|v| needs_bounds_declaration(&v.var_type));

    if has_bounds {
        if options.include_section_spacing {
            writeln!(output)?;
        }
        writeln!(output, "Bounds")?;

        for variable in problem.variables.values() {
            write_variable_bounds(output, variable, &problem.interner, options)?;
        }
    }

    Ok(())
}

/// Check if a variable type needs bounds declaration
const fn needs_bounds_declaration(var_type: &VariableType) -> bool {
    matches!(var_type, VariableType::LowerBound(_) | VariableType::UpperBound(_) | VariableType::DoubleBound(_, _) | VariableType::Free)
}

/// Write bounds for a single variable
fn write_variable_bounds(output: &mut String, variable: &Variable, interner: &NameInterner, options: &LpWriterOptions) -> std::fmt::Result {
    let var_name = interner.resolve(variable.name);
    match &variable.var_type {
        VariableType::Free => {
            writeln!(output, "{var_name} free")?;
        }
        VariableType::LowerBound(bound) => {
            write!(output, "{var_name} >= ")?;
            write_number(output, *bound, options.decimal_precision)?;
            writeln!(output)?;
        }
        VariableType::UpperBound(bound) => {
            write!(output, "{var_name} <= ")?;
            write_number(output, *bound, options.decimal_precision)?;
            writeln!(output)?;
        }
        VariableType::DoubleBound(lower, upper) => {
            write_number(output, *lower, options.decimal_precision)?;
            write!(output, " <= {var_name} <= ")?;
            write_number(output, *upper, options.decimal_precision)?;
            writeln!(output)?;
        }
        _ => {} // Other types don't need bounds declarations
    }

    Ok(())
}

/// Write variable type sections (binaries, integers, etc.)
fn write_variable_types_sections(output: &mut String, problem: &LpProblem, options: &LpWriterOptions) -> std::fmt::Result {
    // Group variables by type, resolving names
    let mut binaries = Vec::new();
    let mut integers = Vec::new();
    let mut generals = Vec::new();
    let mut semi_continuous = Vec::new();

    for variable in problem.variables.values() {
        let var_name = problem.interner.resolve(variable.name);
        match variable.var_type {
            VariableType::Binary => binaries.push(var_name),
            VariableType::Integer => integers.push(var_name),
            VariableType::General => generals.push(var_name),
            VariableType::SemiContinuous => semi_continuous.push(var_name),
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

    if !generals.is_empty() {
        write_variable_type_section(output, "Generals", &generals, options)?;
    }

    if !semi_continuous.is_empty() {
        write_variable_type_section(output, "Semi-Continuous", &semi_continuous, options)?;
    }

    Ok(())
}

/// Write a variable type section
fn write_variable_type_section(output: &mut String, section_name: &str, variables: &[&str], options: &LpWriterOptions) -> std::fmt::Result {
    if options.include_section_spacing {
        writeln!(output)?;
    }
    writeln!(output, "{section_name}")?;

    // Write variables, potentially wrapping lines
    let mut current_line_length = 0;
    for (i, &var_name) in variables.iter().enumerate() {
        let var_len = 1 + var_name.len(); // " " + name

        if current_line_length + var_len > options.max_line_length && i > 0 {
            writeln!(output)?;
            write!(output, " {var_name}")?;
            current_line_length = var_len;
        } else {
            write!(output, " {var_name}")?;
            current_line_length += var_len;
        }
    }
    writeln!(output)
}

/// Write a line of coefficients with proper formatting
fn write_coefficients_line(
    output: &mut String,
    coefficients: &[Coefficient],
    interner: &NameInterner,
    options: &LpWriterOptions,
) -> std::fmt::Result {
    const CONTINUATION_INDENT: &str = "        ";
    let mut current_line_length: usize = 0;
    let mut piece = String::new();

    for (i, coeff) in coefficients.iter().enumerate() {
        let var_name = interner.resolve(coeff.name);

        // Format into a scratch buffer so wrapping decisions use the real width.
        piece.clear();
        write_formatted_coefficient(&mut piece, var_name, coeff.value, i == 0, options.decimal_precision)?;

        if current_line_length + piece.len() > options.max_line_length && i > 0 {
            writeln!(output)?;
            write!(output, "{CONTINUATION_INDENT}")?;
            current_line_length = CONTINUATION_INDENT.len();
        }

        output.push_str(&piece);
        current_line_length += piece.len();
    }

    Ok(())
}

/// Write a formatted coefficient directly to the output buffer, avoiding intermediate `String` allocation.
fn write_formatted_coefficient(output: &mut String, name: &str, value: f64, is_first: bool, precision: usize) -> std::fmt::Result {
    debug_assert!(!name.is_empty(), "coefficient name must not be empty");
    debug_assert!(value.is_finite(), "coefficient value must be finite, got: {value}");
    let abs_value = value.abs();
    let sign = if value < 0.0 { "-" } else { "+" };
    let is_one = (abs_value - 1.0).abs() < NUMERIC_EPSILON;

    if is_first {
        if value < 0.0 {
            if is_one {
                write!(output, "- {name}")
            } else {
                write!(output, "- ")?;
                write_number(output, abs_value, precision)?;
                write!(output, " {name}")
            }
        } else if is_one {
            write!(output, "{name}")
        } else {
            write_number(output, abs_value, precision)?;
            write!(output, " {name}")
        }
    } else if is_one {
        write!(output, " {sign} {name}")
    } else {
        write!(output, " {sign} ")?;
        write_number(output, abs_value, precision)?;
        write!(output, " {name}")
    }
}

/// Write a number with specified precision directly to the output buffer: a bare
/// integer when the value is whole, small enough (|value| < 1e10) and round-trips
/// through `i64`; otherwise a decimal with trailing zeros trimmed.
///
/// `pub(crate)` so the MPS writer ([`crate::mps::writer`]) can reuse the same
/// numeric formatting instead of duplicating it.
#[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]
pub(crate) fn write_number(output: &mut String, value: f64, precision: usize) -> std::fmt::Result {
    debug_assert!(!value.is_nan(), "write_number called with NaN");
    if value.is_infinite() {
        // Infinite bounds are legitimate LP syntax; the lexer accepts `inf`/`-inf`.
        return output.write_str(if value > 0.0 { "inf" } else { "-inf" });
    }
    let is_whole_number = value.fract().abs() < f64::EPSILON;
    let is_safe_for_i64 = value >= (i64::MIN as f64) && value <= (i64::MAX as f64);

    if is_whole_number && is_safe_for_i64 && value.abs() < 1e10 {
        let cast = value as i64;
        debug_assert!((cast as f64 - value).abs() < 1.0, "i64 cast lost precision: {value} -> {cast}");
        write!(output, "{cast}")
    } else {
        let start = output.len();
        write!(output, "{value:.precision$}")?;
        if output[start..].contains('.') {
            let trimmed_len = start + output[start..].trim_end_matches('0').trim_end_matches('.').len();
            output.truncate(trimmed_len);
        }
        Ok(())
    }
}

/// Format a number with specified precision, removing trailing zeros.
/// Convenience wrapper around `write_number` for use in tests.
#[cfg(test)]
fn format_number(value: f64, precision: usize) -> String {
    let mut s = String::new();
    write_number(&mut s, value, precision).expect("write_number failed");
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Coefficient, ComparisonOp, Constraint, Objective, Sense, Variable, VariableType};
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
        fn fmt(name: &str, value: f64, is_first: bool, precision: usize) -> String {
            let mut buf = String::new();
            write_formatted_coefficient(&mut buf, name, value, is_first, precision).expect("write! to String cannot fail");
            buf
        }
        assert_eq!(fmt("x1", 1.0, true, 6), "x1");
        assert_eq!(fmt("x2", -1.0, true, 6), "- x2");
        assert_eq!(fmt("x3", 2.5, false, 6), " + 2.5 x3");
        assert_eq!(fmt("x4", -3.7, false, 6), " - 3.7 x4");
    }

    #[test]
    fn test_write_infinite_bounds() {
        let mut problem = LpProblem::new();
        let obj_id = problem.intern("obj");
        let x1 = problem.intern("x1");
        let x2 = problem.intern("x2");
        let x3 = problem.intern("x3");
        problem.add_objective(Objective { name: obj_id, coefficients: vec![Coefficient { name: x1, value: 1.0 }], byte_offset: None });
        let c1 = problem.intern("c1");
        problem.add_constraint(Constraint::Standard {
            name: c1,
            coefficients: vec![Coefficient { name: x1, value: 1.0 }],
            operator: ComparisonOp::LTE,
            rhs: 10.0,
            byte_offset: None,
        });
        problem.add_variable(Variable::new(x1).with_var_type(VariableType::UpperBound(f64::INFINITY)));
        problem.add_variable(Variable::new(x2).with_var_type(VariableType::LowerBound(f64::NEG_INFINITY)));
        problem.add_variable(Variable::new(x3).with_var_type(VariableType::DoubleBound(f64::NEG_INFINITY, 5.0)));

        let result = write_lp_string(&problem);
        assert!(result.contains("x1 <= inf"), "got: {result}");
        assert!(result.contains("x2 >= -inf"), "got: {result}");
        assert!(result.contains("-inf <= x3 <= 5"), "got: {result}");

        // Infinite bounds must survive a round-trip.
        let reparsed = LpProblem::parse(&result).unwrap();
        let x1 = reparsed.name_id("x1").unwrap();
        let x2 = reparsed.name_id("x2").unwrap();
        let x3 = reparsed.name_id("x3").unwrap();
        assert_eq!(reparsed.variables[&x1].var_type, VariableType::UpperBound(f64::INFINITY));
        assert_eq!(reparsed.variables[&x2].var_type, VariableType::LowerBound(f64::NEG_INFINITY));
        assert_eq!(reparsed.variables[&x3].var_type, VariableType::DoubleBound(f64::NEG_INFINITY, 5.0));
    }

    #[test]
    fn test_write_skips_empty_coefficient_expressions() {
        let mut problem = LpProblem::new();
        let obj_id = problem.intern("obj");
        let empty_obj_id = problem.intern("empty_obj");
        let x1 = problem.intern("x1");
        let c1 = problem.intern("c1");
        let empty_c = problem.intern("empty_c");
        problem.add_objective(Objective { name: obj_id, coefficients: vec![Coefficient { name: x1, value: 1.0 }], byte_offset: None });
        problem.add_objective(Objective { name: empty_obj_id, coefficients: vec![], byte_offset: None });
        problem.add_constraint(Constraint::Standard {
            name: c1,
            coefficients: vec![Coefficient { name: x1, value: 1.0 }],
            operator: ComparisonOp::LTE,
            rhs: 10.0,
            byte_offset: None,
        });
        problem.add_constraint(Constraint::Standard {
            name: empty_c,
            coefficients: vec![],
            operator: ComparisonOp::LTE,
            rhs: 5.0,
            byte_offset: None,
        });

        let result = write_lp_string(&problem);
        // Empty expressions cannot be represented in LP syntax; a dangling
        // ` name: ` line would make the output unparseable.
        assert!(!result.contains("empty_obj"), "got: {result}");
        assert!(!result.contains("empty_c"), "got: {result}");

        let reparsed = LpProblem::parse(&result).unwrap();
        assert_eq!(reparsed.objective_count(), 1);
        assert_eq!(reparsed.constraint_count(), 1);
    }

    #[test]
    fn test_write_empty_problem() {
        let problem = LpProblem::new();
        let result = write_lp_string(&problem);

        assert!(result.contains("Minimize"));
        assert!(result.contains("End"));
    }

    #[test]
    fn test_write_simple_problem() {
        let mut problem = LpProblem::new().with_problem_name(String::from("Test Problem")).with_sense(Sense::Maximize);

        // Intern names and build types
        let profit_id = problem.intern("profit");
        let x1_id = problem.intern("x1");
        let x2_id = problem.intern("x2");
        let capacity_id = problem.intern("capacity");

        // Add objective
        let objective = Objective {
            name: profit_id,
            coefficients: vec![Coefficient { name: x1_id, value: 3.0 }, Coefficient { name: x2_id, value: 2.0 }],
            byte_offset: None,
        };
        problem.add_objective(objective);

        // Add constraint
        let constraint = Constraint::Standard {
            name: capacity_id,
            coefficients: vec![Coefficient { name: x1_id, value: 1.0 }, Coefficient { name: x2_id, value: 1.0 }],
            operator: ComparisonOp::LTE,
            rhs: 100.0,
            byte_offset: None,
        };
        problem.add_constraint(constraint);

        let result = write_lp_string(&problem);

        assert!(result.contains("\\Problem name: Test Problem"));
        assert!(result.contains("Maximize"));
        assert!(result.contains("profit: 3 x1 + 2 x2"));
        assert!(result.contains("Subject To"));
        assert!(result.contains("capacity: x1 + x2 <= 100"));
        assert!(result.contains("End"));
    }

    #[test]
    fn test_complete_lp_rewriting_workflow() {
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
        problem.update_objective_coefficient("profit", "x1", 5.0).unwrap();
        problem.update_objective_coefficient("profit", "x3", 1.5).unwrap();
        problem.update_constraint_coefficient("capacity", "x3", 0.5).unwrap();
        problem.update_constraint_rhs("material", 200.0).unwrap();

        // Add new constraint using interned names
        let demand_id = problem.intern("demand");
        let x1_id = problem.name_id("x1").unwrap();
        let new_constraint = Constraint::Standard {
            name: demand_id,
            coefficients: vec![Coefficient { name: x1_id, value: 1.0 }],
            operator: ComparisonOp::GTE,
            rhs: 20.0,
            byte_offset: None,
        };
        problem.add_constraint(new_constraint);

        // Update variable types
        problem.update_variable_type("x1", VariableType::Integer).unwrap();
        problem.update_variable_type("x3", VariableType::Binary).unwrap();

        // Rename elements
        problem.rename_variable("x2", "production").unwrap();
        problem.rename_constraint("capacity", "resource_limit").unwrap();

        // Step 3: Write the modified problem back to LP format
        let result = write_lp_string(&problem);

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
        assert!(reparsed_problem.name_id("production").and_then(|id| reparsed_problem.variables.get(&id)).is_some());
        assert!(reparsed_problem.name_id("x2").and_then(|id| reparsed_problem.variables.get(&id)).is_none());
        assert!(reparsed_problem.name_id("resource_limit").and_then(|id| reparsed_problem.constraints.get(&id)).is_some());
        assert!(reparsed_problem.name_id("capacity").and_then(|id| reparsed_problem.constraints.get(&id)).is_none());
    }

    #[test]
    fn test_write_problem_with_bounds_and_variable_types() {
        let mut problem = LpProblem::new().with_problem_name(String::from("Complex Problem")).with_sense(crate::model::Sense::Minimize);

        let cost_id = problem.intern("cost");
        let x1_id = problem.intern("x1");
        let x2_id = problem.intern("x2");
        let x3_id = problem.intern("x3");
        let resource1_id = problem.intern("resource1");

        // Add objective
        let objective = Objective {
            name: cost_id,
            coefficients: vec![
                Coefficient { name: x1_id, value: 10.0 },
                Coefficient { name: x2_id, value: 15.0 },
                Coefficient { name: x3_id, value: 20.0 },
            ],
            byte_offset: None,
        };
        problem.add_objective(objective);

        // Add constraints
        let constraint1 = Constraint::Standard {
            name: resource1_id,
            coefficients: vec![
                Coefficient { name: x1_id, value: 1.0 },
                Coefficient { name: x2_id, value: 2.0 },
                Coefficient { name: x3_id, value: 1.0 },
            ],
            operator: ComparisonOp::LTE,
            rhs: 100.0,
            byte_offset: None,
        };
        problem.add_constraint(constraint1);

        // Set variable types and bounds
        problem.update_variable_type("x1", VariableType::DoubleBound(0.0, 50.0)).unwrap();
        problem.update_variable_type("x2", VariableType::Binary).unwrap();
        problem.update_variable_type("x3", VariableType::Integer).unwrap();

        let result = write_lp_string(&problem);

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
    fn test_generals_round_trip() {
        let input = r"
Minimize
obj: x1 + 2 x2 + 3 x3

Subject To
c1: x1 + x2 + x3 <= 10

Generals
x1
x2

End";

        let problem = crate::problem::LpProblem::parse(input).unwrap();

        // Verify the parsed variables are General
        let x1_id = problem.name_id("x1").unwrap();
        let x2_id = problem.name_id("x2").unwrap();
        assert_eq!(problem.variables.get(&x1_id).unwrap().var_type, VariableType::General);
        assert_eq!(problem.variables.get(&x2_id).unwrap().var_type, VariableType::General);

        // Write back to LP format
        let output = write_lp_string(&problem);

        // Verify Generals section is present in the output
        assert!(output.contains("Generals"), "Output should contain a Generals section:\n{output}");
        assert!(output.contains("x1"), "Generals section should contain x1");
        assert!(output.contains("x2"), "Generals section should contain x2");

        // Re-parse and verify round-trip
        let reparsed = crate::problem::LpProblem::parse(&output).unwrap();
        let x1_id = reparsed.name_id("x1").unwrap();
        let x2_id = reparsed.name_id("x2").unwrap();
        assert_eq!(reparsed.variables.get(&x1_id).unwrap().var_type, VariableType::General);
        assert_eq!(reparsed.variables.get(&x2_id).unwrap().var_type, VariableType::General);
    }

    /// Assert that two problems are structurally identical: same sense, and the
    /// same objectives, constraints, and variable types (matched by name).
    #[allow(clippy::float_cmp)]
    fn assert_problems_structurally_equal(a: &LpProblem, b: &LpProblem) {
        assert_eq!(a.sense, b.sense, "sense must match");
        assert_eq!(a.objective_count(), b.objective_count(), "objective count");
        assert_eq!(a.constraint_count(), b.constraint_count(), "constraint count");
        assert_eq!(a.variable_count(), b.variable_count(), "variable count");

        for (id, obj) in &a.objectives {
            let name = a.resolve(*id);
            let b_id = b.name_id(name).unwrap_or_else(|| panic!("objective '{name}' missing after round-trip"));
            let coeffs_a: Vec<(&str, f64)> = obj.coefficients.iter().map(|c| (a.resolve(c.name), c.value)).collect();
            let coeffs_b: Vec<(&str, f64)> = b.objectives[&b_id].coefficients.iter().map(|c| (b.resolve(c.name), c.value)).collect();
            assert_eq!(coeffs_a, coeffs_b, "objective '{name}' coefficients");
        }

        for (id, con) in &a.constraints {
            let name = a.resolve(*id);
            let b_id = b.name_id(name).unwrap_or_else(|| panic!("constraint '{name}' missing after round-trip"));
            match (con, &b.constraints[&b_id]) {
                (
                    Constraint::Standard { coefficients: ca, operator: oa, rhs: ra, .. },
                    Constraint::Standard { coefficients: cb, operator: ob, rhs: rb, .. },
                ) => {
                    assert_eq!(oa, ob, "constraint '{name}' operator");
                    assert_eq!(ra, rb, "constraint '{name}' rhs");
                    let coeffs_a: Vec<(&str, f64)> = ca.iter().map(|c| (a.resolve(c.name), c.value)).collect();
                    let coeffs_b: Vec<(&str, f64)> = cb.iter().map(|c| (b.resolve(c.name), c.value)).collect();
                    assert_eq!(coeffs_a, coeffs_b, "constraint '{name}' coefficients");
                }
                (Constraint::SOS { sos_type: ta, weights: wa, .. }, Constraint::SOS { sos_type: tb, weights: wb, .. }) => {
                    assert_eq!(ta, tb, "constraint '{name}' SOS type");
                    let weights_a: Vec<(&str, f64)> = wa.iter().map(|c| (a.resolve(c.name), c.value)).collect();
                    let weights_b: Vec<(&str, f64)> = wb.iter().map(|c| (b.resolve(c.name), c.value)).collect();
                    assert_eq!(weights_a, weights_b, "constraint '{name}' SOS weights");
                }
                _ => panic!("constraint '{name}' changed kind after round-trip"),
            }
        }

        for (id, var) in &a.variables {
            let name = a.resolve(*id);
            let b_id = b.name_id(name).unwrap_or_else(|| panic!("variable '{name}' missing after round-trip"));
            assert_eq!(var.var_type, b.variables[&b_id].var_type, "variable '{name}' type");
        }
    }

    #[test]
    fn test_round_trip_full_structural_equality() {
        // Covers negative and fractional coefficients, all three operators,
        // single-sided bounds on different variables, a free variable, a
        // double bound, binaries/generals/semi-continuous sections, and both
        // SOS types.
        let input = "\
Maximize
 obj: 2.5 x1 - 3 x2 + x3 + 0.5 f - g + sc1
Subject To
 c_lte: x1 + 2 x2 <= 10
 c_gte: - x1 + 0.25 x3 >= -2.5
 c_eq: x2 + x3 = 4
Bounds
 x1 >= 2
 x2 <= 8
 f free
 -1.5 <= g <= 7.5
Binaries
 b1
Generals
 gen1
Semi-Continuous
 sc1
SOS
 sos_a: S1:: sw1:1 sw2:2.5
 sos_b: S2:: sw3:3 sw4:4
End
";
        let original = LpProblem::parse(input).unwrap();

        // Sanity-check the parse picked up the interesting variable types.
        let vt = |name: &str| original.variables[&original.name_id(name).unwrap()].var_type.clone();
        assert_eq!(vt("x1"), VariableType::LowerBound(2.0));
        assert_eq!(vt("x2"), VariableType::UpperBound(8.0));
        assert_eq!(vt("f"), VariableType::Free);
        assert_eq!(vt("g"), VariableType::DoubleBound(-1.5, 7.5));
        assert_eq!(vt("b1"), VariableType::Binary);
        assert_eq!(vt("gen1"), VariableType::General);
        assert_eq!(vt("sc1"), VariableType::SemiContinuous);
        assert_eq!(vt("sw1"), VariableType::SOS);

        let written = write_lp_string(&original);
        let reparsed = LpProblem::parse(&written).unwrap_or_else(|e| panic!("written LP must re-parse: {e}\n---\n{written}"));

        assert_problems_structurally_equal(&original, &reparsed);
    }

    #[test]
    fn test_objective_line_wrapping_round_trip() {
        let mut problem = LpProblem::new();
        let obj_id = problem.intern("obj");
        let coefficients: Vec<Coefficient> = (0..12)
            .map(|i| {
                let id = problem.intern(&format!("very_long_variable_name_{i:02}"));
                Coefficient { name: id, value: f64::from(i + 1) * 1.5 }
            })
            .collect();
        problem.add_objective(Objective { name: obj_id, coefficients, byte_offset: None });
        let c1 = problem.intern("c1");
        let x0 = problem.name_id("very_long_variable_name_00").unwrap();
        problem.add_constraint(Constraint::Standard {
            name: c1,
            coefficients: vec![Coefficient { name: x0, value: 1.0 }],
            operator: ComparisonOp::GTE,
            rhs: 1.0,
            byte_offset: None,
        });

        let written = write_lp_string(&problem);

        // The objective must wrap: continuation lines start with the indent.
        assert!(written.contains("\n        "), "objective should wrap with continuation indent:\n{written}");
        let longest = written.lines().map(str::len).max().unwrap();
        // Each wrapped line stays near the limit (a single piece may overhang).
        assert!(longest < 120, "no line should be wildly over the 80-char limit, longest was {longest}:\n{written}");

        let reparsed = LpProblem::parse(&written).unwrap_or_else(|e| panic!("wrapped LP must re-parse: {e}\n---\n{written}"));
        assert_problems_structurally_equal(&problem, &reparsed);
    }

    #[test]
    fn test_binaries_section_wrapping_round_trip() {
        let mut problem = LpProblem::new();
        let obj_id = problem.intern("obj");
        let x_id = problem.intern("x");
        problem.add_objective(Objective { name: obj_id, coefficients: vec![Coefficient { name: x_id, value: 1.0 }], byte_offset: None });
        let c1 = problem.intern("c1");
        problem.add_constraint(Constraint::Standard {
            name: c1,
            coefficients: vec![Coefficient { name: x_id, value: 1.0 }],
            operator: ComparisonOp::LTE,
            rhs: 1.0,
            byte_offset: None,
        });
        let names: Vec<String> = (0..10).map(|i| format!("binary_variable_with_a_long_name_{i:02}")).collect();
        for name in &names {
            let id = problem.intern(name);
            problem.add_variable(Variable::new(id).with_var_type(VariableType::Binary));
        }

        let written = write_lp_string(&problem);

        // The Binaries section must span multiple lines under the 80-char limit.
        let binaries_start = written.find("Binaries").expect("Binaries section present");
        let section = &written[binaries_start..written.find("\nEnd").unwrap_or(written.len())];
        assert!(section.trim_end().lines().count() > 2, "Binaries section should wrap over multiple lines:\n{section}");

        let reparsed = LpProblem::parse(&written).unwrap_or_else(|e| panic!("wrapped LP must re-parse: {e}\n---\n{written}"));
        for name in &names {
            let id = reparsed.name_id(name).unwrap_or_else(|| panic!("binary '{name}' missing after round-trip"));
            assert_eq!(reparsed.variables[&id].var_type, VariableType::Binary, "variable '{name}'");
        }
    }

    #[test]
    fn test_write_number_large_whole_values() {
        // Whole values at or above 1e10 take the decimal path; trailing zeros
        // and the decimal point must be trimmed away.
        assert_eq!(format_number(1e12, 6), "1000000000000");
        assert_eq!(format_number(-1e12, 6), "-1000000000000");
        // Just below the 1e10 threshold: the integer fast path.
        assert_eq!(format_number(9_999_999_999.0, 6), "9999999999");
    }

    #[test]
    fn test_writer_options_take_effect() {
        let input = "Minimize\n obj: 2.789 x + 1.111 y\nSubject To\n c1: x + y <= 3.14159\nEnd";
        let problem = LpProblem::parse(input).unwrap().with_problem_name(String::from("Opts"));

        let options =
            LpWriterOptions { include_problem_name: false, max_line_length: 10, decimal_precision: 2, include_section_spacing: false };
        let written = write_lp_string_with_options(&problem, &options);

        // include_problem_name = false: no name comment.
        assert!(!written.contains("Problem name"), "problem name must be omitted:\n{written}");
        // include_section_spacing = false: no blank lines anywhere.
        assert!(!written.contains("\n\n"), "no blank lines expected:\n{written}");
        // decimal_precision = 2: coefficients and rhs rounded to two places.
        assert!(written.contains("2.79 x"), "coefficient must round to 2 dp:\n{written}");
        assert!(written.contains("1.11 y"), "coefficient must round to 2 dp:\n{written}");
        assert!(written.contains("3.14"), "rhs must round to 2 dp:\n{written}");
        assert!(!written.contains("3.14159"), "full-precision rhs must not appear:\n{written}");
        // max_line_length = 10: the two-term objective wraps onto a
        // continuation line.
        let obj_line = written.lines().find(|l| l.contains("obj:")).expect("objective line present");
        assert!(!obj_line.contains('y'), "objective should wrap before the second term:\n{written}");
        assert!(written.lines().any(|l| l.starts_with("        ") && l.contains('y')), "continuation line expected:\n{written}");

        // Defaults for contrast: name comment and section spacing present.
        let default_written = write_lp_string(&problem);
        assert!(default_written.contains("\\Problem name: Opts"));
        assert!(default_written.contains("\n\n"));
    }

    #[test]
    fn test_sos_s2_write_and_reparse() {
        let input = "Minimize\n obj: x + y\nSubject To\n c1: x + y >= 1\nSOS\n s2c: S2:: x:1 y:2.5\nEnd";
        let problem = LpProblem::parse(input).unwrap();

        let written = write_lp_string(&problem);
        assert!(written.contains("S2::"), "S2 marker expected:\n{written}");
        // SOS constraints must live in their own section, not under Subject To.
        let subject_to = written.find("Subject To").unwrap();
        let sos_section = written.find("\nSOS\n").expect("dedicated SOS section expected");
        assert!(sos_section > subject_to, "SOS section must come after Subject To:\n{written}");

        let reparsed = LpProblem::parse(&written).unwrap_or_else(|e| panic!("written LP must re-parse: {e}\n---\n{written}"));
        let id = reparsed.name_id("s2c").unwrap();
        match &reparsed.constraints[&id] {
            Constraint::SOS { sos_type, weights, .. } => {
                assert_eq!(*sos_type, crate::model::SOSType::S2);
                assert_eq!(weights.len(), 2);
            }
            Constraint::Standard { .. } => panic!("s2c must re-parse as an SOS constraint"),
        }
    }

    #[test]
    fn test_multiple_objectives_round_trip() {
        let input = "Minimize\n obj1: x + 2 y\n obj2: 3 x\nSubject To\n c1: x + y >= 1\nEnd";
        let problem = LpProblem::parse(input).unwrap();
        assert_eq!(problem.objective_count(), 2);

        let written = write_lp_string(&problem);
        let reparsed = LpProblem::parse(&written).unwrap_or_else(|e| panic!("written LP must re-parse: {e}\n---\n{written}"));

        assert_eq!(reparsed.objective_count(), 2);
        assert_problems_structurally_equal(&problem, &reparsed);
    }

    #[test]
    fn test_write_with_sos_constraints() {
        let mut problem = LpProblem::new();

        let sos1_id = problem.intern("sos1");
        let x1_id = problem.intern("x1");
        let x2_id = problem.intern("x2");
        let x3_id = problem.intern("x3");

        // Add SOS constraint
        let sos_constraint = Constraint::SOS {
            name: sos1_id,
            sos_type: crate::model::SOSType::S1,
            weights: vec![
                Coefficient { name: x1_id, value: 1.0 },
                Coefficient { name: x2_id, value: 2.0 },
                Coefficient { name: x3_id, value: 3.0 },
            ],
            byte_offset: None,
        };
        problem.add_constraint(sos_constraint);

        let result = write_lp_string(&problem);

        assert!(result.contains("Subject To"));
        assert!(result.contains("sos1: S1:: x1:1 x2:2 x3:3"));
        assert!(result.contains("End"));
    }
}
