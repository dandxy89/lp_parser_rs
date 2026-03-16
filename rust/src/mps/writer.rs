//! MPS file format writer.
//!
//! Writes an [`LpProblem`] to standard MPS format, enabling MPS→MPS round-trip.

use std::collections::BTreeMap;
use std::fmt::Write;

use crate::NUMERIC_EPSILON;
use crate::error::{LpParseError, LpResult};
use crate::model::{Coefficient, Constraint, SOSType, Sense, VariableType};
use crate::problem::LpProblem;

/// Options for controlling MPS file output format.
#[derive(Debug, Clone)]
pub struct MpsWriterOptions {
    /// Number of decimal places for numeric values.
    pub decimal_precision: usize,
}

impl Default for MpsWriterOptions {
    fn default() -> Self {
        Self { decimal_precision: 12 }
    }
}

/// Write an `LpProblem` to a string in standard MPS format.
///
/// # Errors
///
/// Returns an error if the problem cannot be formatted.
pub fn write_mps_string(problem: &LpProblem) -> LpResult<String> {
    write_mps_string_with_options(problem, &MpsWriterOptions::default())
}

/// Write an `LpProblem` to a string in MPS format with custom options.
///
/// # Errors
///
/// Returns an error if the problem cannot be formatted.
pub fn write_mps_string_with_options(problem: &LpProblem, options: &MpsWriterOptions) -> LpResult<String> {
    let mut output = String::new();

    write_name_section(&mut output, problem)?;
    write_objsense_section(&mut output, problem)?;
    write_rows_section(&mut output, problem)?;
    write_columns_section(&mut output, problem, options)?;
    write_rhs_section(&mut output, problem, options)?;
    write_bounds_section(&mut output, problem, options)?;
    write_sos_section(&mut output, problem, options)?;

    writeln!(output, "ENDATA").map_err(fmt_err)?;

    Ok(output)
}

/// A (row_name, coefficient) entry for the COLUMNS section.
struct ColumnEntry<'a> {
    row_name: &'a str,
    value: f64,
}

/// Determine whether a variable type is integer for INTORG/INTEND marking.
const fn is_integer_type(var_type: &VariableType) -> bool {
    matches!(var_type, VariableType::Integer | VariableType::General | VariableType::Binary)
}

fn fmt_err(err: std::fmt::Error) -> LpParseError {
    LpParseError::io_error(format!("MPS write error: {err}"))
}

/// Format a number, stripping trailing zeros. Handles infinity.
fn write_mps_number(output: &mut String, value: f64, precision: usize) -> std::fmt::Result {
    if value == f64::INFINITY {
        return write!(output, "1e30");
    }
    if value == f64::NEG_INFINITY {
        return write!(output, "-1e30");
    }
    debug_assert!(value.is_finite(), "write_mps_number called with NaN value");

    let is_whole = value.fract().abs() < f64::EPSILON;
    let is_safe = value >= (i64::MIN as f64) && value <= (i64::MAX as f64);

    if is_whole && is_safe && value.abs() < 1e15 {
        #[allow(clippy::cast_possible_truncation)]
        let cast = value as i64;
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

// --- Section writers ---

fn write_name_section(output: &mut String, problem: &LpProblem) -> LpResult<()> {
    let name = problem.name().unwrap_or("");
    writeln!(output, "NAME          {name}").map_err(fmt_err)?;
    Ok(())
}

fn write_objsense_section(output: &mut String, problem: &LpProblem) -> LpResult<()> {
    if problem.sense == Sense::Maximize {
        writeln!(output, "OBJSENSE").map_err(fmt_err)?;
        writeln!(output, "    MAX").map_err(fmt_err)?;
    }
    Ok(())
}

fn write_rows_section(output: &mut String, problem: &LpProblem) -> LpResult<()> {
    writeln!(output, "ROWS").map_err(fmt_err)?;

    // Objective rows (N type)
    for objective in problem.objectives.values() {
        let name = problem.resolve(objective.name);
        writeln!(output, " N  {name}").map_err(fmt_err)?;
    }

    // Constraint rows (L/G/E type) — skip SOS constraints
    for constraint in problem.constraints.values() {
        match constraint {
            Constraint::Standard { name, operator, .. } => {
                let row_type = match operator {
                    crate::model::ComparisonOp::LTE | crate::model::ComparisonOp::LT => "L",
                    crate::model::ComparisonOp::GTE | crate::model::ComparisonOp::GT => "G",
                    crate::model::ComparisonOp::EQ => "E",
                };
                let resolved = problem.resolve(*name);
                writeln!(output, " {row_type}  {resolved}").map_err(fmt_err)?;
            }
            Constraint::SOS { .. } => {}
        }
    }

    Ok(())
}

fn write_columns_section(output: &mut String, problem: &LpProblem, options: &MpsWriterOptions) -> LpResult<()> {
    writeln!(output, "COLUMNS").map_err(fmt_err)?;

    // Build variable → [(row_name, coeff)] map, preserving variable insertion order.
    // Use BTreeMap keyed on resolved name for deterministic output.
    let mut column_map: BTreeMap<&str, Vec<ColumnEntry<'_>>> = BTreeMap::new();

    // Collect from objectives
    for objective in problem.objectives.values() {
        let row_name = problem.resolve(objective.name);
        collect_coefficients(&mut column_map, &objective.coefficients, row_name, problem);
    }

    // Collect from standard constraints (skip SOS)
    for constraint in problem.constraints.values() {
        if let Constraint::Standard { name, coefficients, .. } = constraint {
            let row_name = problem.resolve(*name);
            collect_coefficients(&mut column_map, coefficients, row_name, problem);
        }
    }

    // Partition into non-integer and integer variables
    let mut non_integer_vars: Vec<(&str, &Vec<ColumnEntry<'_>>)> = Vec::new();
    let mut integer_vars: Vec<(&str, &Vec<ColumnEntry<'_>>)> = Vec::new();

    for (var_name, entries) in &column_map {
        let var_id = problem.get_name_id(var_name);
        let is_int = var_id.and_then(|id| problem.variables.get(&id)).is_some_and(|v| is_integer_type(&v.var_type));

        if is_int {
            integer_vars.push((var_name, entries));
        } else {
            non_integer_vars.push((var_name, entries));
        }
    }

    // Write non-integer variables
    for (var_name, entries) in &non_integer_vars {
        write_column_entries(output, var_name, entries, options)?;
    }

    // Write integer variables wrapped in INTORG/INTEND markers
    if !integer_vars.is_empty() {
        writeln!(output, "    MARK0000  'MARKER'                 'INTORG'").map_err(fmt_err)?;
        for (var_name, entries) in &integer_vars {
            write_column_entries(output, var_name, entries, options)?;
        }
        writeln!(output, "    MARK0001  'MARKER'                 'INTEND'").map_err(fmt_err)?;
    }

    Ok(())
}

fn collect_coefficients<'a>(
    column_map: &mut BTreeMap<&'a str, Vec<ColumnEntry<'a>>>,
    coefficients: &[Coefficient],
    row_name: &'a str,
    problem: &'a LpProblem,
) {
    for coeff in coefficients {
        let var_name = problem.resolve(coeff.name);
        column_map.entry(var_name).or_default().push(ColumnEntry { row_name, value: coeff.value });
    }
}

/// Write column entries for a single variable, packing up to 2 entries per line.
fn write_column_entries(output: &mut String, var_name: &str, entries: &[ColumnEntry<'_>], options: &MpsWriterOptions) -> LpResult<()> {
    debug_assert!(!entries.is_empty(), "write_column_entries called with empty entries for {var_name}");

    let mut i = 0;
    while i < entries.len() {
        // One entry per line for simplicity and robustness with long names
        write!(output, "    {var_name}  ").map_err(fmt_err)?;
        write_mps_field_pair(output, entries[i].row_name, entries[i].value, options)?;
        writeln!(output).map_err(fmt_err)?;
        i += 1;
    }

    Ok(())
}

/// Write a (name, value) field pair with guaranteed whitespace separation.
fn write_mps_field_pair(output: &mut String, name: &str, value: f64, options: &MpsWriterOptions) -> LpResult<()> {
    write!(output, "{name}  ").map_err(fmt_err)?;
    write_mps_number(output, value, options.decimal_precision).map_err(fmt_err)?;
    Ok(())
}

fn write_rhs_section(output: &mut String, problem: &LpProblem, options: &MpsWriterOptions) -> LpResult<()> {
    // Collect non-zero RHS values
    let mut rhs_entries: Vec<(&str, f64)> = Vec::new();

    for constraint in problem.constraints.values() {
        if let Constraint::Standard { name, rhs, .. } = constraint {
            if rhs.abs() > NUMERIC_EPSILON {
                rhs_entries.push((problem.resolve(*name), *rhs));
            }
        }
    }

    if rhs_entries.is_empty() {
        return Ok(());
    }

    writeln!(output, "RHS").map_err(fmt_err)?;

    for (row_name, value) in &rhs_entries {
        write!(output, "    RHS  ").map_err(fmt_err)?;
        write_mps_field_pair(output, row_name, *value, options)?;
        writeln!(output).map_err(fmt_err)?;
    }

    Ok(())
}

fn write_bounds_section(output: &mut String, problem: &LpProblem, options: &MpsWriterOptions) -> LpResult<()> {
    let mut has_bounds = false;

    for variable in problem.variables.values() {
        if needs_mps_bounds(&variable.var_type) {
            has_bounds = true;
            break;
        }
    }

    if !has_bounds {
        return Ok(());
    }

    writeln!(output, "BOUNDS").map_err(fmt_err)?;

    for variable in problem.variables.values() {
        let var_name = problem.resolve(variable.name);

        match &variable.var_type {
            VariableType::Free => {
                writeln!(output, " FR BOUND     {var_name}").map_err(fmt_err)?;
            }
            VariableType::Binary => {
                writeln!(output, " BV BOUND     {var_name}").map_err(fmt_err)?;
            }
            VariableType::Integer | VariableType::General => {
                // Integer/General variables get bounds via INTORG/INTEND in COLUMNS.
                // Write explicit LO 0 to override the default [0, 1] integer default.
                write!(output, " LI BOUND     {var_name}  ").map_err(fmt_err)?;
                write_mps_number(output, 0.0, options.decimal_precision).map_err(fmt_err)?;
                writeln!(output).map_err(fmt_err)?;
            }
            VariableType::LowerBound(lb) => {
                write!(output, " LO BOUND     {var_name}  ").map_err(fmt_err)?;
                write_mps_number(output, *lb, options.decimal_precision).map_err(fmt_err)?;
                writeln!(output).map_err(fmt_err)?;
            }
            VariableType::UpperBound(ub) => {
                write!(output, " UP BOUND     {var_name}  ").map_err(fmt_err)?;
                write_mps_number(output, *ub, options.decimal_precision).map_err(fmt_err)?;
                writeln!(output).map_err(fmt_err)?;
            }
            VariableType::DoubleBound(lb, ub) => {
                // Fixed bound (lb == ub)
                if (*lb - *ub).abs() < NUMERIC_EPSILON {
                    write!(output, " FX BOUND     {var_name}  ").map_err(fmt_err)?;
                    write_mps_number(output, *lb, options.decimal_precision).map_err(fmt_err)?;
                    writeln!(output).map_err(fmt_err)?;
                } else {
                    // Lower bound: MI for -inf, LO otherwise
                    if lb.is_infinite() && *lb < 0.0 {
                        writeln!(output, " MI BOUND     {var_name}").map_err(fmt_err)?;
                    } else {
                        write!(output, " LO BOUND     {var_name}  ").map_err(fmt_err)?;
                        write_mps_number(output, *lb, options.decimal_precision).map_err(fmt_err)?;
                        writeln!(output).map_err(fmt_err)?;
                    }
                    // Upper bound: skip for +inf, UP otherwise
                    if ub.is_finite() {
                        write!(output, " UP BOUND     {var_name}  ").map_err(fmt_err)?;
                        write_mps_number(output, *ub, options.decimal_precision).map_err(fmt_err)?;
                        writeln!(output).map_err(fmt_err)?;
                    }
                }
            }
            VariableType::SemiContinuous => {
                write!(output, " SC BOUND     {var_name}  ").map_err(fmt_err)?;
                write_mps_number(output, 0.0, options.decimal_precision).map_err(fmt_err)?;
                writeln!(output).map_err(fmt_err)?;
            }
            VariableType::SOS => {}
        }
    }

    Ok(())
}

/// Whether a variable type needs an entry in the BOUNDS section.
const fn needs_mps_bounds(var_type: &VariableType) -> bool {
    matches!(
        var_type,
        VariableType::Free
            | VariableType::Binary
            | VariableType::Integer
            | VariableType::General
            | VariableType::LowerBound(_)
            | VariableType::UpperBound(_)
            | VariableType::DoubleBound(_, _)
            | VariableType::SemiContinuous
    )
}

fn write_sos_section(output: &mut String, problem: &LpProblem, options: &MpsWriterOptions) -> LpResult<()> {
    let sos_constraints: Vec<_> = problem.constraints.values().filter(|c| matches!(c, Constraint::SOS { .. })).collect();

    if sos_constraints.is_empty() {
        return Ok(());
    }

    writeln!(output, "SOS").map_err(fmt_err)?;

    for constraint in sos_constraints {
        if let Constraint::SOS { name, sos_type, weights, .. } = constraint {
            let resolved_name = problem.resolve(*name);
            let type_str = match sos_type {
                SOSType::S1 => "S1",
                SOSType::S2 => "S2",
            };
            writeln!(output, " {type_str} {resolved_name}").map_err(fmt_err)?;

            for weight in weights {
                let var_name = problem.resolve(weight.name);
                write!(output, "    {var_name:<10}").map_err(fmt_err)?;
                write_mps_number(output, weight.value, options.decimal_precision).map_err(fmt_err)?;
                writeln!(output).map_err(fmt_err)?;
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Coefficient, Objective};

    #[test]
    fn test_write_mps_number_integers() {
        let mut buf = String::new();
        write_mps_number(&mut buf, 0.0, 12).unwrap();
        assert_eq!(buf, "0");

        buf.clear();
        write_mps_number(&mut buf, 42.0, 12).unwrap();
        assert_eq!(buf, "42");

        buf.clear();
        write_mps_number(&mut buf, -7.0, 12).unwrap();
        assert_eq!(buf, "-7");
    }

    #[test]
    fn test_write_mps_number_fractions() {
        let mut buf = String::new();
        write_mps_number(&mut buf, 1.5, 12).unwrap();
        assert_eq!(buf, "1.5");

        buf.clear();
        write_mps_number(&mut buf, -0.333, 6).unwrap();
        assert_eq!(buf, "-0.333");
    }

    #[test]
    fn test_write_minimal_problem() {
        let mut problem = LpProblem::new();
        let obj_id = problem.intern("obj");
        let x1_id = problem.intern("x1");
        let c1_id = problem.intern("c1");

        problem.add_objective(Objective { name: obj_id, coefficients: vec![Coefficient { name: x1_id, value: 1.0 }], byte_offset: None });
        problem.add_constraint(Constraint::Standard {
            name: c1_id,
            coefficients: vec![Coefficient { name: x1_id, value: 2.0 }],
            operator: crate::model::ComparisonOp::LTE,
            rhs: 10.0,
            byte_offset: None,
        });

        let mps = write_mps_string(&problem).unwrap();
        assert!(mps.contains("NAME"));
        assert!(mps.contains("ROWS"));
        assert!(mps.contains(" N  obj"));
        assert!(mps.contains(" L  c1"));
        assert!(mps.contains("COLUMNS"));
        assert!(mps.contains("x1"));
        assert!(mps.contains("RHS"));
        assert!(mps.contains("10"));
        assert!(mps.contains("ENDATA"));
    }
}
