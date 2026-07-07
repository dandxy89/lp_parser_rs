//! MPS file writing and formatting utilities.
//!
//! This module writes an [`LpProblem`](crate::problem::LpProblem) back out in
//! MPS (Mathematical Programming System) format, mirroring the conventions of
//! the LP writer ([`crate::writer`]): a small options struct,
//! [`write_mps_string`](crate::mps::writer::write_mps_string) /
//! [`write_mps_string_with_options`](crate::mps::writer::write_mps_string_with_options)
//! entry points, and
//! a private tree of per-section builder functions. Output produced here is
//! designed to be read back by [`crate::mps::parse_mps`].
//!
//! # Formatting
//!
//! Output is **free-format** MPS: fields are whitespace-separated and padded
//! for readability rather than aligned to the strict fixed-column positions
//! of historical MPS. The reader (like most modern MPS parsers) only ever
//! splits on whitespace, so this is a purely cosmetic choice.
//!
//! # Sections emitted
//!
//! `NAME`, `OBJSENSE` (only when the sense is `Maximize` -- `Minimize` is the
//! MPS default and is left implicit), `ROWS`, `COLUMNS` (integer/general/binary
//! variables wrapped in `'MARKER'` `INTORG`/`INTEND` blocks), `RHS`, `BOUNDS`,
//! `SOS`, `ENDATA`.
//!
//! No `RANGES` section is ever written: [`LpProblem`](crate::problem::LpProblem)
//! has no first-class notion of a ranged constraint -- the MPS reader
//! flattens `RANGES` rows into two ordinary constraints at parse time, so
//! there is nothing to reconstruct a single ranged row from.
//!
//! # Objectives
//!
//! MPS represents exactly one objective (a single `N` row). If the problem
//! has more than one objective, [`write_mps_string`](crate::mps::writer::write_mps_string)
//! returns an error unless
//! [`allow_multiple_objectives`](crate::mps::writer::MpsWriterOptions::allow_multiple_objectives)
//! opts in to writing only the first objective (in insertion order). If the
//! problem has **no** objectives, a single empty `N` row is written under the
//! name [`EMPTY_OBJECTIVE_ROW_NAME`](crate::mps::writer::EMPTY_OBJECTIVE_ROW_NAME)
//! -- this is what [`parse_mps`](crate::mps::parse_mps) itself falls back to
//! when a file has no `N` rows, so the round trip is stable, but note that
//! re-parsing such a file yields a problem with **one** empty objective
//! rather than zero: an unavoidable asymmetry given MPS always has an
//! objective row.
//!
//! # Known round-trip limitations
//!
//! - [`General`](crate::model::VariableType::General) and
//!   [`Integer`](crate::model::VariableType::Integer) are both written
//!   identically (an `INTORG`/`INTEND` marker block plus an explicit `LO 0`
//!   bound, to avoid falling back to the MPS default integer bounds of
//!   `[0, 1]`). Re-parsing always yields `Integer`; the `General` designation
//!   is an LP-format-only distinction that has no MPS analogue.
//! - [`SemiContinuous`](crate::model::VariableType::SemiContinuous) carries no
//!   explicit upper bound in this model, but the MPS `SC` bound type requires
//!   one. A sentinel value
//!   ([`SEMI_CONTINUOUS_SENTINEL_UPPER`](crate::mps::writer::SEMI_CONTINUOUS_SENTINEL_UPPER))
//!   is written
//!   instead. Separately, the MPS reader currently resolves `SC` bounds to
//!   [`UpperBound`](crate::model::VariableType::UpperBound) rather than
//!   `SemiContinuous` (a pre-existing reader characteristic, not introduced
//!   by this writer), so semi-continuous variables do not round trip through
//!   MPS as `SemiContinuous`.
//! - Strict inequalities (`ComparisonOp::LT` / `ComparisonOp::GT`) have no MPS
//!   representation (only `L`/`G`/`E` rows exist); writing a problem with such
//!   a constraint returns an error.
//! - [`UpperBound`](crate::model::VariableType::UpperBound) with a negative
//!   value is written as an explicit `LO 0` followed by `UP`, rather than a
//!   bare `UP`. Per the MPS (CPLEX) convention the reader implements, a bare
//!   negative `UP` with no preceding `LO` implies a lower bound of `-inf`,
//!   which would silently change the feasible region; the explicit `LO 0`
//!   keeps it correct at the cost of re-parsing as `DoubleBound(0, ub)`
//!   rather than `UpperBound(ub)` (the same feasible region, a different
//!   variant).

use std::fmt::Write;

use indexmap::IndexMap;

use crate::error::{LpParseError, LpResult};
use crate::interner::NameId;
use crate::model::{ComparisonOp, Constraint, Objective, Sense, VariableType};
use crate::problem::LpProblem;
use crate::writer::write_number;

/// Row name used for the objective when the problem has zero objectives.
///
/// See the "Objectives" section of the module documentation.
pub const EMPTY_OBJECTIVE_ROW_NAME: &str = "OBJ";

/// Sentinel upper bound written for [`VariableType::SemiContinuous`] variables,
/// which carry no explicit upper bound in this model. `1e30` is the
/// conventional "infinity" sentinel used by CPLEX/Gurobi-style MPS files.
pub const SEMI_CONTINUOUS_SENTINEL_UPPER: f64 = 1e30;

/// Fixed vector label written in the RHS section (the first field of each
/// RHS data line). The MPS reader accepts any label; only the first
/// encountered vector is honoured, so a single constant label is sufficient.
const RHS_VECTOR_LABEL: &str = "RHS";

/// Fixed vector label written in the BOUNDS section, analogous to
/// [`RHS_VECTOR_LABEL`].
const BOUNDS_VECTOR_LABEL: &str = "BOUND";

/// Options for controlling MPS file output format.
#[derive(Debug, Clone)]
pub struct MpsWriterOptions {
    /// Number of decimal places for numeric values (coefficients, RHS, bounds).
    pub decimal_precision: usize,
    /// If the problem has more than one objective, write only the first
    /// (in insertion order) instead of returning an error.
    pub allow_multiple_objectives: bool,
}

impl Default for MpsWriterOptions {
    fn default() -> Self {
        Self { decimal_precision: 6, allow_multiple_objectives: false }
    }
}

/// Write an `LpProblem` to a string in MPS format.
///
/// # Errors
///
/// Returns an error if the problem has more than one objective (see
/// [`MpsWriterOptions::allow_multiple_objectives`]) or contains a constraint
/// with a strict inequality operator (`<` or `>`), neither of which MPS can
/// represent.
pub fn write_mps_string(problem: &LpProblem) -> LpResult<String> {
    write_mps_string_with_options(problem, &MpsWriterOptions::default())
}

/// Write an `LpProblem` to a string in MPS format with custom options.
///
/// # Errors
///
/// See [`write_mps_string`].
pub fn write_mps_string_with_options(problem: &LpProblem, options: &MpsWriterOptions) -> LpResult<String> {
    let mut output = String::new();
    build_mps(&mut output, problem, options)?;
    Ok(output)
}

/// Build the full MPS document into `output`.
fn build_mps(output: &mut String, problem: &LpProblem, options: &MpsWriterOptions) -> LpResult<()> {
    let objective = select_objective(problem, options)?;
    let obj_row_name: &str = objective.map_or(EMPTY_OBJECTIVE_ROW_NAME, |o| problem.resolve(o.name));

    write_name_line(output, problem).expect("fmt::Write to String is infallible");

    if problem.sense == Sense::Maximize {
        writeln!(output, "OBJSENSE").expect("fmt::Write to String is infallible");
        writeln!(output, "    MAX").expect("fmt::Write to String is infallible");
    }

    write_rows_section(output, problem, obj_row_name)?;

    let columns = build_columns(problem, objective, obj_row_name);
    write_columns_section(output, problem, &columns, options).expect("fmt::Write to String is infallible");

    write_rhs_section(output, problem, obj_row_name, options).expect("fmt::Write to String is infallible");
    write_bounds_section(output, problem, options)?;
    write_sos_section(output, problem, options).expect("fmt::Write to String is infallible");

    writeln!(output, "ENDATA").expect("fmt::Write to String is infallible");
    Ok(())
}

/// Select the objective to write, applying the single-objective rule.
///
/// # Errors
///
/// Returns an error if the problem has more than one objective and
/// `options.allow_multiple_objectives` is `false`.
fn select_objective<'p>(problem: &'p LpProblem, options: &MpsWriterOptions) -> LpResult<Option<&'p Objective>> {
    match problem.objectives.len() {
        0 => Ok(None),
        1 => Ok(problem.objectives.values().next()),
        count if options.allow_multiple_objectives => {
            debug_assert!(count > 1, "count > 1 guaranteed by preceding match arms");
            Ok(problem.objectives.values().next())
        }
        count => Err(LpParseError::validation_error(format!(
            "MPS format supports a single objective, but the problem has {count} objectives; \
             set MpsWriterOptions::allow_multiple_objectives to write only the first"
        ))),
    }
}

/// Write the `NAME` section header line.
fn write_name_line(output: &mut String, problem: &LpProblem) -> std::fmt::Result {
    match problem.name() {
        Some(name) => writeln!(output, "NAME          {name}"),
        None => writeln!(output, "NAME"),
    }
}

/// Map a comparison operator to its MPS row type letter.
///
/// # Errors
///
/// Returns an error for strict inequalities (`<`, `>`), which MPS cannot
/// represent (only `L`/`G`/`E` rows exist).
fn row_type_letter(operator: ComparisonOp, constraint_name: &str) -> LpResult<char> {
    match operator {
        ComparisonOp::LTE => Ok('L'),
        ComparisonOp::GTE => Ok('G'),
        ComparisonOp::EQ => Ok('E'),
        ComparisonOp::LT | ComparisonOp::GT => Err(LpParseError::validation_error(format!(
            "constraint '{constraint_name}' uses strict inequality '{operator}', which MPS cannot represent"
        ))),
    }
}

/// Write the `ROWS` section: the objective's `N` row followed by one row per
/// standard constraint.
fn write_rows_section(output: &mut String, problem: &LpProblem, obj_row_name: &str) -> LpResult<()> {
    writeln!(output, "ROWS").expect("fmt::Write to String is infallible");
    writeln!(output, " N  {obj_row_name}").expect("fmt::Write to String is infallible");

    for constraint in problem.constraints.values() {
        if let Constraint::Standard { name, operator, .. } = constraint {
            let resolved_name = problem.resolve(*name);
            let letter = row_type_letter(*operator, resolved_name)?;
            writeln!(output, " {letter}  {resolved_name}").expect("fmt::Write to String is infallible");
        }
    }

    Ok(())
}

/// Whether a variable type must be wrapped in an `INTORG`/`INTEND` marker
/// block in the `COLUMNS` section.
const fn needs_marker(var_type: &VariableType) -> bool {
    matches!(var_type, VariableType::Integer | VariableType::General | VariableType::Binary)
}

/// Per-variable list of (row name, coefficient) pairs, in the order rows are
/// encountered (objective first, then constraints in insertion order).
type ColumnEntries<'p> = IndexMap<NameId, Vec<(&'p str, f64)>>;

/// Build the per-variable COLUMNS entries.
///
/// Iterates the objective and constraints once (rather than probing every
/// (variable, row) pair) and groups coefficients by variable, preserving
/// [`LpProblem::variables`] insertion order.
///
/// Variables that require a marker block ([`needs_marker`]) but have no
/// coefficients anywhere (isolated integer/general/binary variables) still
/// need at least one COLUMNS entry to be registered as a column and picked
/// up by the reader's `INTORG`/`INTEND` tracking -- a zero-valued entry
/// against the objective row is synthesised for them.
fn build_columns<'p>(problem: &'p LpProblem, objective: Option<&'p Objective>, obj_row_name: &'p str) -> ColumnEntries<'p> {
    let mut columns: ColumnEntries<'p> = IndexMap::with_capacity(problem.variables.len());
    for name_id in problem.variables.keys() {
        columns.insert(*name_id, Vec::new());
    }

    if let Some(obj) = objective {
        for coeff in &obj.coefficients {
            debug_assert!(problem.variables.contains_key(&coeff.name), "objective coefficient must reference a registered variable");
            columns.entry(coeff.name).or_default().push((obj_row_name, coeff.value));
        }
    }

    for constraint in problem.constraints.values() {
        if let Constraint::Standard { name, coefficients, .. } = constraint {
            let row_name = problem.resolve(*name);
            for coeff in coefficients {
                debug_assert!(problem.variables.contains_key(&coeff.name), "constraint coefficient must reference a registered variable");
                columns.entry(coeff.name).or_default().push((row_name, coeff.value));
            }
        }
    }

    for (name_id, variable) in &problem.variables {
        if needs_marker(&variable.var_type) {
            let entries = columns.entry(*name_id).or_default();
            if entries.is_empty() {
                entries.push((obj_row_name, 0.0));
            }
        }
    }

    columns
}

/// Write the `COLUMNS` section, wrapping integer/general/binary variables in
/// `'MARKER'` `INTORG`/`INTEND` blocks.
fn write_columns_section(
    output: &mut String,
    problem: &LpProblem,
    columns: &ColumnEntries<'_>,
    options: &MpsWriterOptions,
) -> std::fmt::Result {
    writeln!(output, "COLUMNS")?;

    for (name_id, variable) in &problem.variables {
        let entries = columns.get(name_id).map_or([].as_slice(), Vec::as_slice);
        if entries.is_empty() {
            // No row references this variable and it doesn't need a marker
            // block: nothing to emit (it is still registered via BOUNDS).
            continue;
        }

        let var_name = problem.resolve(*name_id);
        let wrap = needs_marker(&variable.var_type);

        if wrap {
            writeln!(output, "    MARKER                 'MARKER'                 'INTORG'")?;
        }
        for &(row_name, value) in entries {
            write!(output, "    {var_name:<10} {row_name:<10} ")?;
            write_number(output, value, options.decimal_precision)?;
            writeln!(output)?;
        }
        if wrap {
            writeln!(output, "    MARKER                 'MARKER'                 'INTEND'")?;
        }
    }

    Ok(())
}

/// Write the `RHS` section. Zero-valued RHS entries are omitted -- the reader
/// already defaults missing rows to an RHS of zero.
fn write_rhs_section(output: &mut String, problem: &LpProblem, obj_row_name: &str, options: &MpsWriterOptions) -> std::fmt::Result {
    debug_assert!(!obj_row_name.is_empty(), "obj_row_name must not be empty");
    writeln!(output, "RHS")?;

    for constraint in problem.constraints.values() {
        if let Constraint::Standard { name, rhs, .. } = constraint {
            if *rhs == 0.0 {
                continue;
            }
            let resolved_name = problem.resolve(*name);
            write!(output, "    {RHS_VECTOR_LABEL:<10} {resolved_name:<10} ")?;
            write_number(output, *rhs, options.decimal_precision)?;
            writeln!(output)?;
        }
    }

    Ok(())
}

/// Write a single BOUNDS line with a numeric value.
fn write_bound_value(output: &mut String, bound_type: &str, var_name: &str, value: f64, precision: usize) -> std::fmt::Result {
    write!(output, " {bound_type} {BOUNDS_VECTOR_LABEL:<9} {var_name:<10} ")?;
    write_number(output, value, precision)?;
    writeln!(output)
}

/// Write a single BOUNDS line without a numeric value (`FR`, `BV`).
fn write_bound_flag(output: &mut String, bound_type: &str, var_name: &str) -> std::fmt::Result {
    writeln!(output, " {bound_type} {BOUNDS_VECTOR_LABEL:<9} {var_name}")
}

/// Build the validation error returned for a bound value that MPS cannot
/// represent (`NaN`, or an infinite value on the "wrong" side of a bound
/// that MPS has no flag for).
fn invalid_bound_error(var_name: &str, message: &str) -> LpParseError {
    LpParseError::validation_error(format!("variable '{var_name}' {message}"))
}

/// Write the bound line(s) for a single variable's [`VariableType`].
///
/// See the module documentation for the `Integer`/`General`/`SemiContinuous`
/// mapping caveats, and for the `Free`-default conversion caveat.
///
/// # Errors
///
/// Returns an error if a bound value is `NaN`, or is an infinite value MPS
/// has no flag for (e.g. `UpperBound(-inf)`, `LowerBound(+inf)`) -- see
/// [`write_upper_bound`], [`write_lower_bound`] and [`write_double_bound`].
fn write_variable_bound(output: &mut String, var_name: &str, var_type: &VariableType, precision: usize) -> LpResult<()> {
    match *var_type {
        VariableType::Free => {
            write_bound_flag(output, "FR", var_name).expect("fmt::Write to String is infallible");
            Ok(())
        }
        VariableType::LowerBound(lb) => write_lower_bound(output, var_name, lb, precision),
        VariableType::UpperBound(ub) => write_upper_bound(output, var_name, ub, precision),
        VariableType::DoubleBound(lb, ub) => write_double_bound(output, var_name, lb, ub, precision),
        VariableType::Binary => {
            write_bound_flag(output, "BV", var_name).expect("fmt::Write to String is infallible");
            Ok(())
        }
        // General has no MPS analogue: both collapse to an integer column
        // with an explicit LO 0 (see module docs).
        VariableType::Integer | VariableType::General => {
            write_bound_value(output, "LO", var_name, 0.0, precision).expect("fmt::Write to String is infallible");
            Ok(())
        }
        VariableType::SemiContinuous => {
            write_bound_value(output, "SC", var_name, SEMI_CONTINUOUS_SENTINEL_UPPER, precision)
                .expect("fmt::Write to String is infallible");
            Ok(())
        }
        // SOS-membership is not an explicit bound; leave the MPS default
        // ([0, +inf)) in place, matching the LP writer's treatment.
        VariableType::SOS => Ok(()),
    }
}

/// Write the bound line for a `LowerBound(lb)` variable.
///
/// The MPS reader maps a bare `MI`-only bound to `LowerBound(-inf)`, so that
/// case is written back as `MI` rather than fed to [`write_number`] (which
/// requires a finite value).
///
/// # Errors
///
/// Returns an error if `lb` is `NaN`, or `+inf` (a lower bound of `+inf` is
/// nonsensical -- it would leave the variable with an empty feasible region
/// unless the upper bound is also `+inf`, which is not representable as a
/// plain `LowerBound`).
fn write_lower_bound(output: &mut String, var_name: &str, lb: f64, precision: usize) -> LpResult<()> {
    if lb.is_nan() {
        return Err(invalid_bound_error(var_name, "has a NaN lower bound, which MPS cannot represent"));
    }
    if lb == f64::INFINITY {
        return Err(invalid_bound_error(var_name, "has a lower bound of +inf, which MPS cannot represent"));
    }
    if lb == f64::NEG_INFINITY {
        write_bound_flag(output, "MI", var_name).expect("fmt::Write to String is infallible");
        return Ok(());
    }
    write_bound_value(output, "LO", var_name, lb, precision).expect("fmt::Write to String is infallible");
    Ok(())
}

/// Write the bound line for an `UpperBound(ub)` variable.
///
/// When `ub` is negative, an explicit `LO 0` is written first. Per the MPS
/// (CPLEX) convention implemented by the reader, a bare `UP` with a negative
/// value and no preceding `LO` implies a lower bound of `-inf`, not the `0`
/// that `UpperBound` means in this model -- without the explicit `LO 0` the
/// round trip would silently widen the feasible region.
///
/// The MPS reader maps a bare `PL`-only bound to `UpperBound(+inf)`, so that
/// case is written back as `PL` rather than fed to [`write_number`] (which
/// requires a finite value).
///
/// # Errors
///
/// Returns an error if `ub` is `NaN`, or `-inf` (an upper bound of `-inf` is
/// nonsensical -- it would leave the variable with an empty feasible region
/// unless the lower bound is also `-inf`, which is not representable as a
/// plain `UpperBound`).
fn write_upper_bound(output: &mut String, var_name: &str, ub: f64, precision: usize) -> LpResult<()> {
    if ub.is_nan() {
        return Err(invalid_bound_error(var_name, "has a NaN upper bound, which MPS cannot represent"));
    }
    if ub == f64::NEG_INFINITY {
        return Err(invalid_bound_error(var_name, "has an upper bound of -inf, which MPS cannot represent"));
    }
    if ub == f64::INFINITY {
        write_bound_flag(output, "PL", var_name).expect("fmt::Write to String is infallible");
        return Ok(());
    }
    if ub < 0.0 {
        write_bound_value(output, "LO", var_name, 0.0, precision).expect("fmt::Write to String is infallible");
    }
    write_bound_value(output, "UP", var_name, ub, precision).expect("fmt::Write to String is infallible");
    Ok(())
}

/// Write the bound line(s) for a `DoubleBound(lb, ub)` variable, collapsing
/// to `FX`/`FR`/`MI`/`LO`+`PL` where the general two-line `LO`+`UP` form is
/// unnecessary.
///
/// A finite lower bound paired with an infinite upper bound is written as
/// `LO` + an explicit `PL` (rather than just `LO` alone): `PL` sets the
/// accumulated upper bound to `+inf` on read-back, so the pair round-trips
/// as `DoubleBound(lb, +inf)` again. Omitting `PL` would leave the upper
/// bound unset, and the reader would collapse the result to a plain
/// `LowerBound(lb)` -- semantically identical, but a different variant.
///
/// # Errors
///
/// Returns an error if either bound is `NaN`, or if `lb` is `+inf` or `ub`
/// is `-inf` (nonsensical combinations that MPS's `FR`/`MI`/`PL` flags
/// cannot represent).
fn write_double_bound(output: &mut String, var_name: &str, lb: f64, ub: f64, precision: usize) -> LpResult<()> {
    if lb.is_nan() || ub.is_nan() {
        return Err(invalid_bound_error(var_name, "has a NaN double bound, which MPS cannot represent"));
    }
    if lb == f64::INFINITY || ub == f64::NEG_INFINITY {
        return Err(invalid_bound_error(
            var_name,
            &format!("has a nonsensical double bound ({lb}, {ub}), which MPS cannot represent"),
        ));
    }

    #[allow(clippy::float_cmp)]
    if lb == ub {
        write_bound_value(output, "FX", var_name, lb, precision).expect("fmt::Write to String is infallible");
        return Ok(());
    }
    match (lb.is_infinite() && lb < 0.0, ub.is_infinite() && ub > 0.0) {
        (true, true) => write_bound_flag(output, "FR", var_name).expect("fmt::Write to String is infallible"),
        (true, false) => {
            write_bound_flag(output, "MI", var_name).expect("fmt::Write to String is infallible");
            write_bound_value(output, "UP", var_name, ub, precision).expect("fmt::Write to String is infallible");
        }
        (false, true) => {
            write_bound_value(output, "LO", var_name, lb, precision).expect("fmt::Write to String is infallible");
            write_bound_flag(output, "PL", var_name).expect("fmt::Write to String is infallible");
        }
        (false, false) => {
            write_bound_value(output, "LO", var_name, lb, precision).expect("fmt::Write to String is infallible");
            write_bound_value(output, "UP", var_name, ub, precision).expect("fmt::Write to String is infallible");
        }
    }
    Ok(())
}

/// Write the `BOUNDS` section, one entry per variable (in declaration order).
///
/// # Errors
///
/// See [`write_variable_bound`].
fn write_bounds_section(output: &mut String, problem: &LpProblem, options: &MpsWriterOptions) -> LpResult<()> {
    if problem.variables.is_empty() {
        return Ok(());
    }

    writeln!(output, "BOUNDS").expect("fmt::Write to String is infallible");
    for (name_id, variable) in &problem.variables {
        let var_name = problem.resolve(*name_id);
        write_variable_bound(output, var_name, &variable.var_type, options.decimal_precision)?;
    }

    Ok(())
}

/// Write the `SOS` section, if the problem has any SOS constraints.
fn write_sos_section(output: &mut String, problem: &LpProblem, options: &MpsWriterOptions) -> std::fmt::Result {
    let has_sos = problem.constraints.values().any(|c| matches!(c, Constraint::SOS { .. }));
    if !has_sos {
        return Ok(());
    }

    writeln!(output, "SOS")?;
    for constraint in problem.constraints.values() {
        if let Constraint::SOS { name, sos_type, weights, .. } = constraint {
            writeln!(output, " {sos_type} {}", problem.resolve(*name))?;
            for weight in weights {
                write!(output, "    {:<10} ", problem.resolve(weight.name))?;
                write_number(output, weight.value, options.decimal_precision)?;
                writeln!(output)?;
            }
        }
    }

    Ok(())
}

#[cfg(test)]
// Coefficients/bounds must round-trip bit-exactly through the writer and
// reader, so these tests intentionally compare floats strictly.
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;
    use crate::model::{Coefficient, ComparisonOp, SOSType};
    use crate::mps::parse_mps;

    fn build_problem_with_bounds_and_sos() -> LpProblem {
        let mut problem = LpProblem::new().with_problem_name(String::from("Sample")).with_sense(Sense::Maximize);

        let profit_id = problem.intern("profit");
        let x1_id = problem.intern("x1");
        let x2_id = problem.intern("x2");
        let x3_id = problem.intern("x3");
        let capacity_id = problem.intern("capacity");
        let sos1_id = problem.intern("sos1");

        problem.add_objective(Objective {
            name: profit_id,
            coefficients: vec![
                Coefficient { name: x1_id, value: 3.0 },
                Coefficient { name: x2_id, value: 2.0 },
                Coefficient { name: x3_id, value: 1.0 },
            ],
            byte_offset: None,
        });

        problem.add_constraint(Constraint::Standard {
            name: capacity_id,
            coefficients: vec![
                Coefficient { name: x1_id, value: 1.0 },
                Coefficient { name: x2_id, value: 1.0 },
                Coefficient { name: x3_id, value: 1.0 },
            ],
            operator: ComparisonOp::LTE,
            rhs: 100.0,
            byte_offset: None,
        });

        problem.update_variable_type("x1", VariableType::Integer).unwrap();
        problem.update_variable_type("x2", VariableType::DoubleBound(0.0, 50.0)).unwrap();
        problem.update_variable_type("x3", VariableType::Binary).unwrap();

        problem.add_constraint(Constraint::SOS {
            name: sos1_id,
            sos_type: SOSType::S1,
            weights: vec![Coefficient { name: x1_id, value: 1.0 }, Coefficient { name: x2_id, value: 2.0 }],
            byte_offset: None,
        });

        problem
    }

    #[test]
    fn test_write_empty_problem() {
        let problem = LpProblem::new();
        let result = write_mps_string(&problem).unwrap();

        assert!(result.contains("NAME"));
        assert!(result.contains(&format!(" N  {EMPTY_OBJECTIVE_ROW_NAME}")));
        assert!(result.contains("ENDATA"));

        // Documented asymmetry: zero objectives in, one (empty) objective out.
        let reparsed = LpProblem::parse_mps(&result).unwrap();
        assert_eq!(reparsed.objective_count(), 1);
    }

    #[test]
    fn test_write_simple_problem_and_reparse() {
        let mut problem = LpProblem::new().with_problem_name(String::from("Test Problem")).with_sense(Sense::Maximize);

        let profit_id = problem.intern("profit");
        let x1_id = problem.intern("x1");
        let x2_id = problem.intern("x2");
        let capacity_id = problem.intern("capacity");

        problem.add_objective(Objective {
            name: profit_id,
            coefficients: vec![Coefficient { name: x1_id, value: 3.0 }, Coefficient { name: x2_id, value: 2.0 }],
            byte_offset: None,
        });
        problem.add_constraint(Constraint::Standard {
            name: capacity_id,
            coefficients: vec![Coefficient { name: x1_id, value: 1.0 }, Coefficient { name: x2_id, value: 1.0 }],
            operator: ComparisonOp::LTE,
            rhs: 100.0,
            byte_offset: None,
        });

        let output = write_mps_string(&problem).unwrap();
        assert!(output.contains("OBJSENSE"));
        assert!(output.contains("MAX"));
        assert!(output.contains(" N  profit"));
        assert!(output.contains(" L  capacity"));

        let reparsed = LpProblem::parse_mps(&output).unwrap();
        assert_eq!(reparsed.sense, Sense::Maximize);
        assert_eq!(reparsed.objective_count(), 1);
        assert_eq!(reparsed.constraint_count(), 1);
        assert_eq!(reparsed.variable_count(), 2);

        let capacity = reparsed.constraints.get(&reparsed.name_id("capacity").unwrap()).unwrap();
        if let Constraint::Standard { rhs, operator, .. } = capacity {
            assert_eq!(*rhs, 100.0);
            assert_eq!(*operator, ComparisonOp::LTE);
        } else {
            panic!("expected Standard constraint");
        }
    }

    #[test]
    fn test_write_bounds_and_integrality_round_trip() {
        let problem = build_problem_with_bounds_and_sos();
        let output = write_mps_string(&problem).unwrap();

        assert!(output.contains("MARKER"));
        assert!(output.contains("INTORG"));
        assert!(output.contains("INTEND"));
        assert!(output.contains("BV"));
        assert!(output.contains("SOS"));

        let reparsed = LpProblem::parse_mps(&output).unwrap();
        assert_eq!(reparsed.variable_count(), 3);
        assert_eq!(reparsed.constraint_count(), 2); // 1 standard + 1 SOS

        let x1 = &reparsed.variables[&reparsed.name_id("x1").unwrap()];
        assert_eq!(x1.var_type, VariableType::Integer);

        let x2 = &reparsed.variables[&reparsed.name_id("x2").unwrap()];
        assert_eq!(x2.var_type, VariableType::DoubleBound(0.0, 50.0));

        let x3 = &reparsed.variables[&reparsed.name_id("x3").unwrap()];
        assert_eq!(x3.var_type, VariableType::Binary);

        let sos = reparsed.constraints.get(&reparsed.name_id("sos1").unwrap()).unwrap();
        if let Constraint::SOS { sos_type, weights, .. } = sos {
            assert_eq!(*sos_type, SOSType::S1);
            assert_eq!(weights.len(), 2);
        } else {
            panic!("expected SOS constraint");
        }
    }

    #[test]
    fn test_double_bound_infinite_upper_round_trips_as_double_bound() {
        let mut problem = LpProblem::new();
        let obj_id = problem.intern("obj");
        let x1_id = problem.intern("x1");
        problem.add_objective(Objective { name: obj_id, coefficients: vec![Coefficient { name: x1_id, value: 1.0 }], byte_offset: None });
        problem.update_variable_type("x1", VariableType::DoubleBound(5.5, f64::INFINITY)).unwrap();

        let output = write_mps_string(&problem).unwrap();
        assert!(output.contains("PL"));

        let reparsed = LpProblem::parse_mps(&output).unwrap();
        let x1 = &reparsed.variables[&reparsed.name_id("x1").unwrap()];
        assert_eq!(x1.var_type, VariableType::DoubleBound(5.5, f64::INFINITY));
    }

    #[test]
    fn test_negative_upper_bound_keeps_zero_lower_bound() {
        let mut problem = LpProblem::new();
        let obj_id = problem.intern("obj");
        let x1_id = problem.intern("x1");
        problem.add_objective(Objective { name: obj_id, coefficients: vec![Coefficient { name: x1_id, value: 1.0 }], byte_offset: None });
        problem.update_variable_type("x1", VariableType::UpperBound(-5.0)).unwrap();

        let output = write_mps_string(&problem).unwrap();

        let reparsed = LpProblem::parse_mps(&output).unwrap();
        let x1 = &reparsed.variables[&reparsed.name_id("x1").unwrap()];
        // Documented variant collapse: same feasible region (lower 0), but
        // DoubleBound rather than UpperBound (see module docs).
        assert_eq!(x1.var_type, VariableType::DoubleBound(0.0, -5.0));
    }

    #[test]
    fn test_multiple_objectives_error_by_default() {
        let mut problem = LpProblem::new();
        let a = problem.intern("a");
        let b = problem.intern("b");
        let x1_id = problem.intern("x1");
        problem.add_objective(Objective { name: a, coefficients: vec![Coefficient { name: x1_id, value: 1.0 }], byte_offset: None });
        problem.add_objective(Objective { name: b, coefficients: vec![Coefficient { name: x1_id, value: 2.0 }], byte_offset: None });

        let err = write_mps_string(&problem).unwrap_err();
        assert!(matches!(err, LpParseError::ValidationError { .. }));
    }

    #[test]
    fn test_multiple_objectives_allowed_writes_first() {
        let mut problem = LpProblem::new();
        let a = problem.intern("a");
        let b = problem.intern("b");
        let x1_id = problem.intern("x1");
        problem.add_objective(Objective { name: a, coefficients: vec![Coefficient { name: x1_id, value: 1.0 }], byte_offset: None });
        problem.add_objective(Objective { name: b, coefficients: vec![Coefficient { name: x1_id, value: 2.0 }], byte_offset: None });

        let options = MpsWriterOptions { allow_multiple_objectives: true, ..MpsWriterOptions::default() };
        let output = write_mps_string_with_options(&problem, &options).unwrap();

        let reparsed = LpProblem::parse_mps(&output).unwrap();
        assert_eq!(reparsed.objective_count(), 1);
        assert!(reparsed.name_id("a").is_some());
    }

    #[test]
    fn test_strict_inequality_returns_error() {
        let mut problem = LpProblem::new();
        let x1_id = problem.intern("x1");
        let c1 = problem.intern("c1");
        problem.add_constraint(Constraint::Standard {
            name: c1,
            coefficients: vec![Coefficient { name: x1_id, value: 1.0 }],
            operator: ComparisonOp::LT,
            rhs: 5.0,
            byte_offset: None,
        });

        let err = write_mps_string(&problem).unwrap_err();
        assert!(matches!(err, LpParseError::ValidationError { .. }));
    }

    #[test]
    fn test_isolated_integer_variable_registers_as_column() {
        // A general/integer variable with no coefficients anywhere must still
        // round-trip as Integer, not fall back to the MPS [0, 1] default.
        let mut problem = LpProblem::new();
        let obj_id = problem.intern("obj");
        problem.add_objective(Objective { name: obj_id, coefficients: vec![], byte_offset: None });
        let x1_id = problem.intern("x1");
        problem.add_variable(crate::model::Variable::new(x1_id).with_var_type(VariableType::General));

        let output = write_mps_string(&problem).unwrap();
        let reparsed = LpProblem::parse_mps(&output).unwrap();
        let x1 = &reparsed.variables[&reparsed.name_id("x1").unwrap()];
        assert_eq!(x1.var_type, VariableType::Integer);
    }

    #[test]
    fn test_semi_continuous_round_trip_is_lossy_upper_bound() {
        // Documented limitation: SemiContinuous round-trips as UpperBound.
        let mut problem = LpProblem::new();
        let obj_id = problem.intern("obj");
        problem.add_objective(Objective { name: obj_id, coefficients: vec![], byte_offset: None });
        let x1_id = problem.intern("x1");
        problem.add_variable(crate::model::Variable::new(x1_id).with_var_type(VariableType::SemiContinuous));

        let output = write_mps_string(&problem).unwrap();
        assert!(output.contains("SC"));

        let reparsed = LpProblem::parse_mps(&output).unwrap();
        let x1 = &reparsed.variables[&reparsed.name_id("x1").unwrap()];
        assert_eq!(x1.var_type, VariableType::UpperBound(SEMI_CONTINUOUS_SENTINEL_UPPER));
    }

    #[test]
    fn pl_only_bound_round_trips_as_upper_bound_infinity() {
        // A `PL`-only bound (no `LO`) resolves to `UpperBound(+inf)` on
        // parse; the writer must emit it back as a bare `PL`, not feed
        // `+inf` to `write_number` (regression test for the panic/invalid
        // `UP BOUND x inf` output this used to produce).
        let input = "\
NAME        pltest
ROWS
 N  obj
 L  c1
COLUMNS
    x1        obj       1
    x1        c1        1
RHS
    RHS_V     c1        10
BOUNDS
 PL BOUND     x1
ENDATA
";
        let problem = LpProblem::parse_mps(input).unwrap();
        let x1 = &problem.variables[&problem.name_id("x1").unwrap()];
        assert_eq!(x1.var_type, VariableType::UpperBound(f64::INFINITY));

        let output = write_mps_string(&problem).unwrap();
        assert!(output.contains(" PL BOUND"));
        assert!(!output.contains("inf"), "must not leak a raw `inf` literal into the bounds line");

        let reparsed = LpProblem::parse_mps(&output).unwrap();
        let x1 = &reparsed.variables[&reparsed.name_id("x1").unwrap()];
        assert_eq!(x1.var_type, VariableType::UpperBound(f64::INFINITY));
    }

    #[test]
    fn mi_only_bound_round_trips_as_lower_bound_negative_infinity() {
        // Mirror of the `PL`-only case: an `MI`-only bound resolves to
        // `LowerBound(-inf)` on parse and must be written back as a bare
        // `MI`.
        let input = "\
NAME        mitest
ROWS
 N  obj
 L  c1
COLUMNS
    x1        obj       1
    x1        c1        1
RHS
    RHS_V     c1        10
BOUNDS
 MI BOUND     x1
ENDATA
";
        let problem = LpProblem::parse_mps(input).unwrap();
        let x1 = &problem.variables[&problem.name_id("x1").unwrap()];
        assert_eq!(x1.var_type, VariableType::LowerBound(f64::NEG_INFINITY));

        let output = write_mps_string(&problem).unwrap();
        assert!(output.contains(" MI BOUND"));
        assert!(!output.contains("inf"), "must not leak a raw `inf` literal into the bounds line");

        let reparsed = LpProblem::parse_mps(&output).unwrap();
        let x1 = &reparsed.variables[&reparsed.name_id("x1").unwrap()];
        assert_eq!(x1.var_type, VariableType::LowerBound(f64::NEG_INFINITY));
    }

    #[test]
    fn nan_upper_bound_returns_validation_error() {
        let mut problem = LpProblem::new();
        let obj_id = problem.intern("obj");
        let x1_id = problem.intern("x1");
        problem.add_objective(Objective { name: obj_id, coefficients: vec![Coefficient { name: x1_id, value: 1.0 }], byte_offset: None });
        problem.update_variable_type("x1", VariableType::UpperBound(f64::NAN)).unwrap();

        let err = write_mps_string(&problem).unwrap_err();
        assert!(matches!(err, LpParseError::ValidationError { .. }));
    }

    #[test]
    fn nan_lower_bound_returns_validation_error() {
        let mut problem = LpProblem::new();
        let obj_id = problem.intern("obj");
        let x1_id = problem.intern("x1");
        problem.add_objective(Objective { name: obj_id, coefficients: vec![Coefficient { name: x1_id, value: 1.0 }], byte_offset: None });
        problem.update_variable_type("x1", VariableType::LowerBound(f64::NAN)).unwrap();

        let err = write_mps_string(&problem).unwrap_err();
        assert!(matches!(err, LpParseError::ValidationError { .. }));
    }

    #[test]
    fn nonsensical_upper_bound_negative_infinity_returns_validation_error() {
        let mut problem = LpProblem::new();
        let obj_id = problem.intern("obj");
        let x1_id = problem.intern("x1");
        problem.add_objective(Objective { name: obj_id, coefficients: vec![Coefficient { name: x1_id, value: 1.0 }], byte_offset: None });
        problem.update_variable_type("x1", VariableType::UpperBound(f64::NEG_INFINITY)).unwrap();

        let err = write_mps_string(&problem).unwrap_err();
        assert!(matches!(err, LpParseError::ValidationError { .. }));
    }

    #[test]
    fn nonsensical_lower_bound_positive_infinity_returns_validation_error() {
        let mut problem = LpProblem::new();
        let obj_id = problem.intern("obj");
        let x1_id = problem.intern("x1");
        problem.add_objective(Objective { name: obj_id, coefficients: vec![Coefficient { name: x1_id, value: 1.0 }], byte_offset: None });
        problem.update_variable_type("x1", VariableType::LowerBound(f64::INFINITY)).unwrap();

        let err = write_mps_string(&problem).unwrap_err();
        assert!(matches!(err, LpParseError::ValidationError { .. }));
    }

    #[test]
    fn nan_double_bound_returns_validation_error() {
        let mut problem = LpProblem::new();
        let obj_id = problem.intern("obj");
        let x1_id = problem.intern("x1");
        problem.add_objective(Objective { name: obj_id, coefficients: vec![Coefficient { name: x1_id, value: 1.0 }], byte_offset: None });
        problem.update_variable_type("x1", VariableType::DoubleBound(f64::NAN, 5.0)).unwrap();

        let err = write_mps_string(&problem).unwrap_err();
        assert!(matches!(err, LpParseError::ValidationError { .. }));
    }

    #[test]
    fn nonsensical_double_bound_returns_validation_error() {
        let mut problem = LpProblem::new();
        let obj_id = problem.intern("obj");
        let x1_id = problem.intern("x1");
        problem.add_objective(Objective { name: obj_id, coefficients: vec![Coefficient { name: x1_id, value: 1.0 }], byte_offset: None });
        // Lower bound of +inf paired with a finite upper bound is an empty,
        // unrepresentable feasible region.
        problem.update_variable_type("x1", VariableType::DoubleBound(f64::INFINITY, 5.0)).unwrap();

        let err = write_mps_string(&problem).unwrap_err();
        assert!(matches!(err, LpParseError::ValidationError { .. }));
    }

    #[test]
    fn mps_round_trip_preserves_mps_fixture() {
        let input = "\
NAME        test
ROWS
 N  obj
 L  c1
 G  c2
 E  c3
COLUMNS
    x1        obj       1
    x1        c1        2
    x1        c2        1
    x1        c3        1
    x2        obj       2
    x2        c1        1
RHS
    RHS_V     c1        10
    RHS_V     c2        1
    RHS_V     c3        4
BOUNDS
 LO BOUND     x1        0
 UP BOUND     x1        20
ENDATA
";
        let original = parse_mps(input).unwrap();
        let problem = LpProblem::parse_mps(input).unwrap();
        let output = write_mps_string(&problem).unwrap();
        let reparsed = LpProblem::parse_mps(&output).unwrap();

        assert_eq!(reparsed.variable_count(), problem.variable_count());
        assert_eq!(reparsed.constraint_count(), problem.constraint_count());
        assert_eq!(reparsed.objective_count(), problem.objective_count());
        assert_eq!(reparsed.sense, problem.sense);
        assert_eq!(original.constraints.len(), reparsed.constraint_count());

        let x1 = &reparsed.variables[&reparsed.name_id("x1").unwrap()];
        assert_eq!(x1.var_type, VariableType::DoubleBound(0.0, 20.0));
    }

    #[test]
    fn snapshot_representative_problem() {
        let problem = build_problem_with_bounds_and_sos();
        let output = write_mps_string(&problem).unwrap();
        insta::assert_snapshot!(output);
    }
}
