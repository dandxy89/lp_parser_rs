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
//! MPS default and is left implicit), `ROWS`, `LAZYCONS` / `USERCUTS` (CPLEX:
//! lazy constraints and user cuts, listed like `ROWS` and otherwise ordinary
//! rows in `COLUMNS`, `RHS` and `RANGES`), `COLUMNS` (integer/general/binary
//! variables wrapped in `'MARKER'` `INTORG`/`INTEND` blocks), `RHS`, `RANGES`
//! (see below), `BOUNDS`, `SOS`, `QUADOBJ` (the written objective's
//! quadratic terms, upper triangle of `Q` in `c'x + 1/2 x'Qx`), `QCMATRIX`
//! (one per quadratic constraint: the full symmetric `Q` of `a'x + x'Qx`),
//! `INDICATORS` (CPLEX: an indicator
//! constraint is an ordinary row plus an `IF row variable value` line),
//! `ENDATA`.
//!
//! # RANGES
//!
//! [`LpProblem`](crate::problem::LpProblem) has no first-class notion of a
//! ranged constraint: the MPS reader flattens each `RANGES` row `X` into two
//! ordinary constraints -- `X` (`>=` lower) and `X_rng` (`<=` upper) with
//! identical coefficients. This writer reverses that flattening: when a
//! constraint pair matches the reader's exact pattern (`X` is `>=`, `X_rng`
//! is `<=`, identical coefficient vectors, upper >= lower, both RHS finite),
//! it is re-emitted as a single `G` row with a `RANGES` entry of
//! `upper - lower`, so `MPS -> LpProblem -> MPS` preserves the section. An
//! LP-authored pair that happens to match the pattern is merged the same way;
//! that is semantically lossless (the feasible region and the re-parsed
//! constraint pair are identical), it only changes the MPS text shape.
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
//! - [`SemiContinuous`](crate::model::VariableKind::SemiContinuous): per the
//!   MPS specification the `SC` record's value is the variable's upper bound,
//!   so a finite upper bound is written there. A semi-continuous variable with
//!   no upper bound (or `+inf`) gets the conventional "infinite" sentinel
//!   (`SEMI_CONTINUOUS_SENTINEL_UPPER`, `1e30`), because the record requires a
//!   value. A lower bound, if any, is written as its own `LO` record first.
//!   [`SemiInteger`](crate::model::VariableKind::SemiInteger) is written the
//!   same way with an `SI` record.
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
//! - **Undeclared variables take the MPS default**: a variable that only ever
//!   appears in the objective or a constraint gets no `BOUNDS` entry, which
//!   MPS reads as `[0, +inf)` — the same default LP gives it. A variable
//!   actually declared `x free` carries an explicit `[-inf, +inf]` and is
//!   written as `FR`. The two used to share a representation, and this writer
//!   emitted `FR` for both, widening every undeclared variable's feasible
//!   region to include negatives on the way through.

use std::fmt::Write;

use indexmap::IndexMap;
use rustc_hash::{FxHashMap, FxHashSet};

use crate::error::{LpParseError, LpResult};
use crate::interner::NameId;
use crate::model::{Coefficient, ComparisonOp, Constraint, ConstraintClass, Objective, QuadraticTerm, Sense, VariableBounds, VariableKind};
use crate::problem::LpProblem;
use crate::writer::write_number;

/// Row name used for the objective when the problem has zero objectives.
///
/// See the "Objectives" section of the module documentation.
pub const EMPTY_OBJECTIVE_ROW_NAME: &str = "OBJ";

/// Sentinel `SC` bound value written for a semi-continuous variable with no
/// finite upper bound. `1e30` is the conventional "infinity" sentinel used by
/// CPLEX/Gurobi-style MPS files.
pub(crate) const SEMI_CONTINUOUS_SENTINEL_UPPER: f64 = 1e30;

/// Preferred vector label written in the RHS section (the first field of each
/// RHS data line). The MPS reader accepts any label and honours only the
/// first vector it sees, so one label is enough -- but a label equal to a row
/// name would be misread as a label-less line, so [`VectorLabels`] falls back
/// to a suffixed variant when this one is taken.
const RHS_VECTOR_LABEL: &str = "RHS";

/// Preferred vector label written in the BOUNDS section, analogous to
/// [`RHS_VECTOR_LABEL`] (it must not collide with a column name).
const BOUNDS_VECTOR_LABEL: &str = "BOUND";

/// Preferred vector label written in the RANGES section, analogous to
/// [`RHS_VECTOR_LABEL`].
const RANGES_VECTOR_LABEL: &str = "RNG";

/// The vector labels actually written, chosen so none collides with a name the
/// reader would take for a row (RHS, RANGES) or a column (BOUNDS).
struct VectorLabels {
    rhs: String,
    ranges: String,
    bounds: String,
}

impl VectorLabels {
    fn new(problem: &LpProblem, obj_row_name: &str) -> Self {
        let is_row = |label: &str| label == obj_row_name || problem.name_id(label).is_some_and(|id| problem.constraints.contains_key(&id));
        let is_column = |label: &str| problem.name_id(label).is_some_and(|id| problem.variables.contains_key(&id));
        Self {
            rhs: unused_label(RHS_VECTOR_LABEL, is_row),
            ranges: unused_label(RANGES_VECTOR_LABEL, is_row),
            bounds: unused_label(BOUNDS_VECTOR_LABEL, is_column),
        }
    }
}

/// Return `base`, or the first of `base1`, `base2`, ... for which `taken` is false.
fn unused_label(base: &str, taken: impl Fn(&str) -> bool) -> String {
    let mut label = base.to_string();
    let mut suffix = 1usize;
    while taken(&label) {
        label = format!("{base}{suffix}");
        suffix += 1;
    }
    debug_assert!(!taken(&label), "chosen label must be free");
    label
}

/// How a BOUNDS line is written: its vector label and numeric precision.
#[derive(Clone, Copy)]
struct BoundStyle<'a> {
    label: &'a str,
    precision: Option<usize>,
}

/// Check that `name` survives the MPS reader's whitespace field splitting
/// unchanged: non-empty, no whitespace, no leading `$` (an inline comment),
/// and not the `'MARKER'` keyword.
///
/// # Errors
///
/// Returns a validation error naming the offending `kind` and `name`.
fn check_mps_name(name: &str, kind: &str) -> LpResult<()> {
    let representable = !name.is_empty() && !name.contains(char::is_whitespace) && !name.starts_with('$') && name != "'MARKER'";
    if representable {
        Ok(())
    } else {
        Err(LpParseError::validation_error(format!(
            "{kind} name '{name}' cannot be written to MPS: names must be non-empty, contain no whitespace, not start with '$' and not be 'MARKER'"
        )))
    }
}

/// Validate every row, column and SOS name the MPS writer will emit.
///
/// # Errors
///
/// See [`check_mps_name`]. SOS members named `S1`/`S2` are also rejected: the
/// reader takes such a line for a new set header.
fn validate_mps_names(problem: &LpProblem, obj_row_name: &str) -> LpResult<()> {
    check_mps_name(obj_row_name, "objective")?;
    if let Some(name) = problem.name()
        && name.contains(['\n', '\r'])
    {
        return Err(LpParseError::validation_error(format!("problem name {name:?} cannot be written to MPS: it contains a line break")));
    }
    for id in problem.variables.keys() {
        check_mps_name(problem.resolve(*id), "variable")?;
    }
    for constraint in problem.constraints.values() {
        check_mps_name(problem.resolve(constraint.name()), "constraint")?;
        if let Constraint::Indicator { variable, .. } = constraint {
            check_mps_name(problem.resolve(*variable), "variable")?;
        }
        if let Constraint::SOS { weights, .. } = constraint {
            for weight in weights {
                let member = problem.resolve(weight.name);
                if member.eq_ignore_ascii_case("S1") || member.eq_ignore_ascii_case("S2") {
                    return Err(LpParseError::validation_error(format!(
                        "SOS member '{member}' cannot be written to MPS: the reader would take it for a set header"
                    )));
                }
            }
        }
    }
    Ok(())
}

/// Options for controlling MPS file output format.
#[derive(Debug, Clone, Default)]
pub struct MpsWriterOptions {
    /// Number of decimal places for numeric values (coefficients, RHS, bounds).
    /// `None` (the default) writes the shortest representation that parses back
    /// to the exact same `f64`; `Some(n)` rounds to `n` places, which is lossy.
    pub decimal_precision: Option<usize>,
    /// If the problem has more than one objective, write only the first
    /// (in insertion order) instead of returning an error.
    pub allow_multiple_objectives: bool,
}

/// Write an `LpProblem` to a string in MPS format.
///
/// # Errors
///
/// Returns an error if the problem has more than one objective (see
/// [`MpsWriterOptions::allow_multiple_objectives`]), contains a constraint
/// with a strict inequality operator (`<` or `>`), or contains a Gurobi
/// general constraint, none of which this writer can represent.
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
    if let Some(obj) = objective
        && !obj.attributes.is_empty()
        && !options.allow_multiple_objectives
    {
        return Err(LpParseError::validation_error(format!(
            "objective '{}' has multi-objective attributes (priority, weight, tolerances), which this MPS writer cannot represent; \
             set MpsWriterOptions::allow_multiple_objectives to write the objective without them",
            problem.resolve(obj.name)
        )));
    }
    if let Some(general) = problem.constraints.values().find(|c| matches!(c, Constraint::General { .. })) {
        return Err(LpParseError::validation_error(format!(
            "general constraint '{}' cannot be written to MPS: this writer has no GENCONS support",
            problem.resolve(general.name())
        )));
    }
    let obj_row_name: &str = objective.map_or(EMPTY_OBJECTIVE_ROW_NAME, |o| problem.resolve(o.name));
    validate_mps_names(problem, obj_row_name)?;
    let range_pairs = detect_range_pairs(problem);
    let labels = VectorLabels::new(problem, obj_row_name);

    write_name_line(output, problem).expect("fmt::Write to String is infallible");

    if problem.sense == Sense::Maximize {
        writeln!(output, "OBJSENSE").expect("fmt::Write to String is infallible");
        writeln!(output, "    MAX").expect("fmt::Write to String is infallible");
    }

    write_rows_section(output, problem, obj_row_name, &range_pairs)?;

    let columns = build_columns(problem, objective, obj_row_name, &range_pairs);
    write_columns_section(output, problem, &columns, options).expect("fmt::Write to String is infallible");

    write_rhs_section(output, problem, objective, obj_row_name, &labels.rhs, options, &range_pairs)
        .expect("fmt::Write to String is infallible");
    write_ranges_section(output, problem, &labels.ranges, options, &range_pairs).expect("fmt::Write to String is infallible");
    write_bounds_section(output, problem, BoundStyle { label: &labels.bounds, precision: options.decimal_precision })?;
    write_sos_section(output, problem, options).expect("fmt::Write to String is infallible");
    write_quadratic_sections(output, problem, objective, options).expect("fmt::Write to String is infallible");
    write_indicators_section(output, problem).expect("fmt::Write to String is infallible");

    writeln!(output, "ENDATA").expect("fmt::Write to String is infallible");
    Ok(())
}

/// Constraint pairs that fold back into MPS `RANGES` entries.
///
/// See the "RANGES" section of the module documentation: `ranges` maps a base
/// constraint (`>=` lower) to its range value `upper - lower`, and `skip`
/// holds the `_rng` companion rows (`<=` upper) that must be omitted from the
/// `ROWS`, `COLUMNS`, and `RHS` sections because the single ranged row already
/// represents them.
#[derive(Default)]
struct RangePairs {
    ranges: FxHashMap<NameId, f64>,
    skip: FxHashSet<NameId>,
}

/// Return `true` if two coefficient vectors are equal as (variable, value)
/// sets. Values compare exactly: pairs produced by the reader's RANGES
/// flattening are bit-identical clones, and a near-miss simply doesn't pair.
fn coefficients_match(a: &[Coefficient], b: &[Coefficient]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let map: FxHashMap<NameId, f64> = a.iter().map(|c| (c.name, c.value)).collect();
    b.iter().all(|c| map.get(&c.name) == Some(&c.value))
}

/// Detect constraint pairs matching the reader's RANGES flattening pattern:
/// `X` (`>=` lower) plus `X_rng` (`<=` upper) with identical coefficients,
/// finite RHS values, and `upper >= lower`.
fn detect_range_pairs(problem: &LpProblem) -> RangePairs {
    let mut pairs = RangePairs::default();

    for (name_id, constraint) in &problem.constraints {
        let Constraint::Standard { name, coefficients, operator: ComparisonOp::LTE, rhs: upper_rhs, .. } = constraint else {
            continue;
        };
        let Some(base_name) = problem.resolve(*name).strip_suffix("_rng") else {
            continue;
        };
        let Some(base_id) = problem.name_id(base_name) else {
            continue;
        };
        let Some(Constraint::Standard { coefficients: base_coefficients, operator: ComparisonOp::GTE, rhs: lower_rhs, .. }) =
            problem.constraints.get(&base_id)
        else {
            continue;
        };
        if !upper_rhs.is_finite() || !lower_rhs.is_finite() || upper_rhs < lower_rhs {
            continue;
        }
        // One ranged row has one class, so both halves must share it.
        if problem.constraint_class(*name_id) != problem.constraint_class(base_id) {
            continue;
        }
        if !coefficients_match(base_coefficients, coefficients) {
            continue;
        }

        pairs.ranges.insert(base_id, upper_rhs - lower_rhs);
        pairs.skip.insert(*name_id);
    }

    debug_assert_eq!(pairs.ranges.len(), pairs.skip.len(), "every range entry must have exactly one skipped companion row");
    pairs
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
/// ordinary standard constraint, then the `LAZYCONS` and `USERCUTS` sections
/// (same line format) for lazy constraints and user cuts. Ranged companion
/// rows are omitted (see [`RangePairs`]).
fn write_rows_section(output: &mut String, problem: &LpProblem, obj_row_name: &str, range_pairs: &RangePairs) -> LpResult<()> {
    writeln!(output, "ROWS").expect("fmt::Write to String is infallible");
    writeln!(output, " N  {obj_row_name}").expect("fmt::Write to String is infallible");

    for (class, header) in
        [(ConstraintClass::Normal, None), (ConstraintClass::Lazy, Some("LAZYCONS")), (ConstraintClass::UserCut, Some("USERCUTS"))]
    {
        let mut wrote_header = false;
        for (name_id, constraint) in &problem.constraints {
            if range_pairs.skip.contains(name_id) || problem.constraint_class(*name_id) != class {
                continue;
            }
            if let Some((_, operator, _)) = row_parts(constraint) {
                let name = constraint.name();
                if let Some(header) = header
                    && !wrote_header
                {
                    writeln!(output, "{header}").expect("fmt::Write to String is infallible");
                    wrote_header = true;
                }
                let resolved_name = problem.resolve(name);
                let letter = row_type_letter(operator, resolved_name)?;
                writeln!(output, " {letter}  {resolved_name}").expect("fmt::Write to String is infallible");
            }
        }
    }

    Ok(())
}

/// Whether a variable type must be wrapped in an `INTORG`/`INTEND` marker
/// block in the `COLUMNS` section.
const fn needs_marker(kind: VariableKind) -> bool {
    // Key off kind, not the lossy legacy VariableType: an integer variable with
    // explicit bounds collapses to DoubleBound under var_type(), which would
    // otherwise be mistaken for continuous and written without a marker block.
    kind.is_integer()
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
fn build_columns<'p>(
    problem: &'p LpProblem,
    objective: Option<&'p Objective>,
    obj_row_name: &'p str,
    range_pairs: &RangePairs,
) -> ColumnEntries<'p> {
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

    for (constraint_id, constraint) in &problem.constraints {
        if range_pairs.skip.contains(constraint_id) {
            continue; // The base row already carries these coefficients.
        }
        if let Some((coefficients, _, _)) = row_parts(constraint) {
            let row_name = problem.resolve(constraint.name());
            for coeff in coefficients {
                debug_assert!(problem.variables.contains_key(&coeff.name), "constraint coefficient must reference a registered variable");
                columns.entry(coeff.name).or_default().push((row_name, coeff.value));
            }
        }
    }

    // An indicator variable, or one that only appears in quadratic terms, must
    // still be a column for the INDICATORS / QUADOBJ / QCMATRIX sections to
    // name it.
    let mut needs_column: FxHashSet<NameId> = FxHashSet::default();
    for constraint in problem.constraints.values() {
        match constraint {
            Constraint::Indicator { variable, .. } => {
                needs_column.insert(*variable);
            }
            Constraint::Quadratic { quadratic, .. } => needs_column.extend(quadratic.iter().flat_map(|t| [t.var1, t.var2])),
            Constraint::Standard { .. } | Constraint::SOS { .. } => {}
            Constraint::General { .. } => unreachable!("general constraints are rejected before columns are built"),
        }
    }
    if let Some(obj) = objective {
        needs_column.extend(obj.quadratic.iter().flat_map(|t| [t.var1, t.var2]));
    }
    for (name_id, variable) in &problem.variables {
        if needs_marker(variable.kind) || needs_column.contains(name_id) {
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
        let wrap = needs_marker(variable.kind);

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
/// already defaults missing rows to an RHS of zero. Ranged companion rows are
/// omitted (their upper RHS is carried by the `RANGES` section). An objective
/// constant is written as a negated RHS entry on the objective row, per the
/// CPLEX MPS specification.
fn write_rhs_section(
    output: &mut String,
    problem: &LpProblem,
    objective: Option<&Objective>,
    obj_row_name: &str,
    label: &str,
    options: &MpsWriterOptions,
    range_pairs: &RangePairs,
) -> std::fmt::Result {
    debug_assert!(!obj_row_name.is_empty(), "obj_row_name must not be empty");
    writeln!(output, "RHS")?;

    if let Some(obj) = objective
        && obj.constant != 0.0
    {
        write!(output, "    {label:<10} {obj_row_name:<10} ")?;
        write_number(output, -obj.constant, options.decimal_precision)?;
        writeln!(output)?;
    }

    for (constraint_id, constraint) in &problem.constraints {
        if range_pairs.skip.contains(constraint_id) {
            continue;
        }
        if let Some((_, _, rhs)) = row_parts(constraint) {
            if rhs == 0.0 {
                continue;
            }
            let resolved_name = problem.resolve(constraint.name());
            write!(output, "    {label:<10} {resolved_name:<10} ")?;
            write_number(output, rhs, options.decimal_precision)?;
            writeln!(output)?;
        }
    }

    Ok(())
}

/// Write the `RANGES` section for detected constraint pairs (see
/// [`RangePairs`]). Omitted entirely when there are no pairs.
fn write_ranges_section(
    output: &mut String,
    problem: &LpProblem,
    label: &str,
    options: &MpsWriterOptions,
    range_pairs: &RangePairs,
) -> std::fmt::Result {
    if range_pairs.ranges.is_empty() {
        return Ok(());
    }
    writeln!(output, "RANGES")?;

    // Iterate constraints (not the hash map) for deterministic output order.
    for constraint_id in problem.constraints.keys() {
        if let Some(range_value) = range_pairs.ranges.get(constraint_id) {
            let resolved_name = problem.resolve(*constraint_id);
            write!(output, "    {label:<10} {resolved_name:<10} ")?;
            write_number(output, *range_value, options.decimal_precision)?;
            writeln!(output)?;
        }
    }

    Ok(())
}

/// Write a single BOUNDS line with a numeric value.
fn write_bound_value(output: &mut String, bound_type: &str, var_name: &str, value: f64, style: BoundStyle<'_>) -> std::fmt::Result {
    let label = style.label;
    write!(output, " {bound_type} {label:<9} {var_name:<10} ")?;
    write_number(output, value, style.precision)?;
    writeln!(output)
}

/// Write a single BOUNDS line without a numeric value (`FR`, `BV`).
fn write_bound_flag(output: &mut String, bound_type: &str, var_name: &str, style: BoundStyle<'_>) -> std::fmt::Result {
    let label = style.label;
    writeln!(output, " {bound_type} {label:<9} {var_name}")
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
fn write_variable_bound(
    output: &mut String,
    var_name: &str,
    kind: VariableKind,
    bounds: VariableBounds,
    style: BoundStyle<'_>,
) -> LpResult<()> {
    // Kinds with a dedicated MPS bound record win over the bound shape: the
    // record already carries the bounds implied by the kind.
    match kind {
        VariableKind::Binary => {
            // Binary is canonically [0, 1]; emit BV regardless of any redundant
            // or contradictory explicit bounds carried alongside the kind.
            write_bound_flag(output, "BV", var_name, style).expect("fmt::Write to String is infallible");
            return Ok(());
        }
        VariableKind::SemiContinuous | VariableKind::SemiInteger => {
            // The SC / SI record carries the upper bound only, so any lower
            // bound needs its own LO record first; without it the round trip
            // would silently widen the variable's range down to zero.
            let record = if kind == VariableKind::SemiInteger { "SI" } else { "SC" };
            if let Some(lb) = bounds.lower {
                write_lower_bound(output, var_name, lb, style)?;
            }
            // The SC value is the upper bound (MPS specification); `+inf` and
            // "no upper bound" both map to the conventional infinite sentinel.
            let upper = match bounds.upper {
                None | Some(f64::INFINITY) => SEMI_CONTINUOUS_SENTINEL_UPPER,
                Some(ub) if ub.is_nan() => {
                    return Err(invalid_bound_error(var_name, "has a NaN semi-continuous upper bound, which MPS cannot represent"));
                }
                Some(f64::NEG_INFINITY) => {
                    return Err(invalid_bound_error(var_name, "has a semi-continuous upper bound of -inf, which MPS cannot represent"));
                }
                Some(ub) => ub,
            };
            debug_assert!(upper.is_finite(), "{record} bound value must be finite, got {upper}");
            write_bound_value(output, record, var_name, upper, style).expect("fmt::Write to String is infallible");
            return Ok(());
        }
        // SOS membership is not itself a bound, but such a variable may still
        // carry ordinary bounds — fall through and write them.
        VariableKind::Sos | VariableKind::Continuous | VariableKind::Integer | VariableKind::General => {}
    }

    match (bounds.lower, bounds.upper) {
        (Some(lb), Some(ub)) => write_double_bound(output, var_name, lb, ub, style),
        (Some(lb), None) => write_lower_bound(output, var_name, lb, style),
        (None, Some(ub)) => write_upper_bound(output, var_name, ub, style),
        // Unbounded. Integer and General have no MPS analogue of their own:
        // both collapse to an integer column with an explicit LO 0 (see module
        // docs). An unbounded SOS member keeps the MPS default ([0, +inf)),
        // matching the LP writer's treatment.
        (None, None) => {
            match kind {
                VariableKind::Integer | VariableKind::General => {
                    write_bound_value(output, "LO", var_name, 0.0, style).expect("fmt::Write to String is infallible");
                }
                // No bound was declared, so say nothing: MPS's own default for
                // a column with no BOUNDS entry is [0, +inf), which is exactly
                // what the source meant. Writing `FR` here would state a bound
                // the input never had. An explicit `x free` does not reach this
                // arm — it carries [-inf, +inf] and is written as `FR` by
                // `write_double_bound`.
                VariableKind::Continuous | VariableKind::Sos => {}
                VariableKind::Binary | VariableKind::SemiContinuous | VariableKind::SemiInteger => unreachable!("handled above"),
            }
            Ok(())
        }
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
fn write_lower_bound(output: &mut String, var_name: &str, lb: f64, style: BoundStyle<'_>) -> LpResult<()> {
    if lb.is_nan() {
        return Err(invalid_bound_error(var_name, "has a NaN lower bound, which MPS cannot represent"));
    }
    if lb == f64::INFINITY {
        return Err(invalid_bound_error(var_name, "has a lower bound of +inf, which MPS cannot represent"));
    }
    if lb == f64::NEG_INFINITY {
        write_bound_flag(output, "MI", var_name, style).expect("fmt::Write to String is infallible");
        return Ok(());
    }
    write_bound_value(output, "LO", var_name, lb, style).expect("fmt::Write to String is infallible");
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
fn write_upper_bound(output: &mut String, var_name: &str, ub: f64, style: BoundStyle<'_>) -> LpResult<()> {
    if ub.is_nan() {
        return Err(invalid_bound_error(var_name, "has a NaN upper bound, which MPS cannot represent"));
    }
    if ub == f64::NEG_INFINITY {
        return Err(invalid_bound_error(var_name, "has an upper bound of -inf, which MPS cannot represent"));
    }
    if ub == f64::INFINITY {
        write_bound_flag(output, "PL", var_name, style).expect("fmt::Write to String is infallible");
        return Ok(());
    }
    if ub < 0.0 {
        write_bound_value(output, "LO", var_name, 0.0, style).expect("fmt::Write to String is infallible");
    }
    write_bound_value(output, "UP", var_name, ub, style).expect("fmt::Write to String is infallible");
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
fn write_double_bound(output: &mut String, var_name: &str, lb: f64, ub: f64, style: BoundStyle<'_>) -> LpResult<()> {
    if lb.is_nan() || ub.is_nan() {
        return Err(invalid_bound_error(var_name, "has a NaN double bound, which MPS cannot represent"));
    }
    if lb == f64::INFINITY || ub == f64::NEG_INFINITY {
        return Err(invalid_bound_error(var_name, &format!("has a nonsensical double bound ({lb}, {ub}), which MPS cannot represent")));
    }

    #[allow(clippy::float_cmp)]
    if lb == ub {
        write_bound_value(output, "FX", var_name, lb, style).expect("fmt::Write to String is infallible");
        return Ok(());
    }
    match (lb.is_infinite() && lb < 0.0, ub.is_infinite() && ub > 0.0) {
        (true, true) => write_bound_flag(output, "FR", var_name, style).expect("fmt::Write to String is infallible"),
        (true, false) => {
            write_bound_flag(output, "MI", var_name, style).expect("fmt::Write to String is infallible");
            write_bound_value(output, "UP", var_name, ub, style).expect("fmt::Write to String is infallible");
        }
        (false, true) => {
            write_bound_value(output, "LO", var_name, lb, style).expect("fmt::Write to String is infallible");
            write_bound_flag(output, "PL", var_name, style).expect("fmt::Write to String is infallible");
        }
        (false, false) => {
            write_bound_value(output, "LO", var_name, lb, style).expect("fmt::Write to String is infallible");
            write_bound_value(output, "UP", var_name, ub, style).expect("fmt::Write to String is infallible");
        }
    }
    Ok(())
}

/// Write the `BOUNDS` section, one entry per variable (in declaration order).
///
/// # Errors
///
/// See [`write_variable_bound`].
fn write_bounds_section(output: &mut String, problem: &LpProblem, style: BoundStyle<'_>) -> LpResult<()> {
    if problem.variables.is_empty() {
        return Ok(());
    }

    writeln!(output, "BOUNDS").expect("fmt::Write to String is infallible");
    for (name_id, variable) in &problem.variables {
        let var_name = problem.resolve(*name_id);
        write_variable_bound(output, var_name, variable.kind, variable.bounds, style)?;
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

/// The linear row an MPS `ROWS` entry carries for a constraint: standard,
/// indicator (its linear part) and quadratic (its linear part, the quadratic
/// terms going to `QCMATRIX`) constraints; `None` for SOS.
fn row_parts(constraint: &Constraint) -> Option<(&[Coefficient], ComparisonOp, f64)> {
    match constraint {
        Constraint::Quadratic { coefficients, operator, rhs, .. } => Some((coefficients, *operator, *rhs)),
        other => other.linear_row(),
    }
}

/// Write one `row column value` line of a quadratic section.
fn write_quadratic_entry(
    output: &mut String,
    problem: &LpProblem,
    var1: NameId,
    var2: NameId,
    value: f64,
    options: &MpsWriterOptions,
) -> std::fmt::Result {
    let (a, b) = (problem.resolve(var1), problem.resolve(var2));
    write!(output, "    {a:<10} {b:<10} ")?;
    write_number(output, value, options.decimal_precision)?;
    writeln!(output)
}

/// Write the `QUADOBJ` section for the written objective and one `QCMATRIX`
/// section per quadratic constraint.
///
/// `QUADOBJ` stores the upper triangle of `Q` in `c'x + 1/2 x'Qx`: a square
/// term `c x^2` is `Q_xx = 2c`, a product `c x y` is `Q_xy = c`. `QCMATRIX`
/// stores the full symmetric `Q` of `a'x + x'Qx`: `c x^2` is `Q_xx = c` and
/// `c x y` is `Q_xy = Q_yx = c / 2`.
fn write_quadratic_sections(
    output: &mut String,
    problem: &LpProblem,
    objective: Option<&Objective>,
    options: &MpsWriterOptions,
) -> std::fmt::Result {
    if let Some(obj) = objective
        && !obj.quadratic.is_empty()
    {
        writeln!(output, "QUADOBJ")?;
        for term in &obj.quadratic {
            let value = if term.is_square() { 2.0 * term.coefficient } else { term.coefficient };
            write_quadratic_entry(output, problem, term.var1, term.var2, value, options)?;
        }
    }
    for constraint in problem.constraints.values() {
        let Constraint::Quadratic { name, quadratic, .. } = constraint else {
            continue;
        };
        writeln!(output, "QCMATRIX   {}", problem.resolve(*name))?;
        for QuadraticTerm { var1, var2, coefficient } in quadratic {
            if var1 == var2 {
                write_quadratic_entry(output, problem, *var1, *var2, *coefficient, options)?;
            } else {
                write_quadratic_entry(output, problem, *var1, *var2, coefficient / 2.0, options)?;
                write_quadratic_entry(output, problem, *var2, *var1, coefficient / 2.0, options)?;
            }
        }
    }
    Ok(())
}

/// Write the `INDICATORS` section (CPLEX): one `IF row variable value` line
/// per indicator constraint, whose linear part is an ordinary row.
fn write_indicators_section(output: &mut String, problem: &LpProblem) -> std::fmt::Result {
    let mut wrote_header = false;
    for constraint in problem.constraints.values() {
        if let Constraint::Indicator { name, variable, active_value, .. } = constraint {
            if !wrote_header {
                writeln!(output, "INDICATORS")?;
                wrote_header = true;
            }
            let row = problem.resolve(*name);
            let column = problem.resolve(*variable);
            writeln!(output, " IF {row:<10} {column:<10} {}", u8::from(*active_value))?;
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
    use crate::model::{Coefficient, ComparisonOp, SOSType, VariableBounds, VariableKind, VariableType};
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
            constant: 0.0,
            quadratic: Vec::new(),
            attributes: crate::model::ObjectiveAttributes::default(),
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
            constant: 0.0,
            quadratic: Vec::new(),
            attributes: crate::model::ObjectiveAttributes::default(),
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
        assert_eq!(x1.kind, VariableKind::Integer);

        let x2 = &reparsed.variables[&reparsed.name_id("x2").unwrap()];
        assert_eq!(x2.bounds, VariableBounds::range(0.0, 50.0));

        let x3 = &reparsed.variables[&reparsed.name_id("x3").unwrap()];
        assert_eq!(x3.kind, VariableKind::Binary);

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
        problem.add_objective(Objective {
            name: obj_id,
            coefficients: vec![Coefficient { name: x1_id, value: 1.0 }],
            constant: 0.0,
            quadratic: Vec::new(),
            attributes: crate::model::ObjectiveAttributes::default(),
            byte_offset: None,
        });
        problem.update_variable_type("x1", VariableType::DoubleBound(5.5, f64::INFINITY)).unwrap();

        let output = write_mps_string(&problem).unwrap();
        assert!(output.contains("PL"));

        let reparsed = LpProblem::parse_mps(&output).unwrap();
        let x1 = &reparsed.variables[&reparsed.name_id("x1").unwrap()];
        assert_eq!(x1.bounds, VariableBounds::range(5.5, f64::INFINITY));
    }

    #[test]
    fn test_negative_upper_bound_keeps_zero_lower_bound() {
        let mut problem = LpProblem::new();
        let obj_id = problem.intern("obj");
        let x1_id = problem.intern("x1");
        problem.add_objective(Objective {
            name: obj_id,
            coefficients: vec![Coefficient { name: x1_id, value: 1.0 }],
            constant: 0.0,
            quadratic: Vec::new(),
            attributes: crate::model::ObjectiveAttributes::default(),
            byte_offset: None,
        });
        problem.update_variable_type("x1", VariableType::UpperBound(-5.0)).unwrap();

        let output = write_mps_string(&problem).unwrap();

        let reparsed = LpProblem::parse_mps(&output).unwrap();
        let x1 = &reparsed.variables[&reparsed.name_id("x1").unwrap()];
        // Documented variant collapse: same feasible region (lower 0), but
        // DoubleBound rather than UpperBound (see module docs).
        assert_eq!(x1.bounds, VariableBounds::range(0.0, -5.0));
    }

    #[test]
    fn test_multiple_objectives_error_by_default() {
        let mut problem = LpProblem::new();
        let a = problem.intern("a");
        let b = problem.intern("b");
        let x1_id = problem.intern("x1");
        problem.add_objective(Objective {
            name: a,
            coefficients: vec![Coefficient { name: x1_id, value: 1.0 }],
            constant: 0.0,
            quadratic: Vec::new(),
            attributes: crate::model::ObjectiveAttributes::default(),
            byte_offset: None,
        });
        problem.add_objective(Objective {
            name: b,
            coefficients: vec![Coefficient { name: x1_id, value: 2.0 }],
            constant: 0.0,
            quadratic: Vec::new(),
            attributes: crate::model::ObjectiveAttributes::default(),
            byte_offset: None,
        });

        let err = write_mps_string(&problem).unwrap_err();
        assert!(matches!(err, LpParseError::ValidationError { .. }));
    }

    #[test]
    fn test_multiple_objectives_allowed_writes_first() {
        let mut problem = LpProblem::new();
        let a = problem.intern("a");
        let b = problem.intern("b");
        let x1_id = problem.intern("x1");
        problem.add_objective(Objective {
            name: a,
            coefficients: vec![Coefficient { name: x1_id, value: 1.0 }],
            constant: 0.0,
            quadratic: Vec::new(),
            attributes: crate::model::ObjectiveAttributes::default(),
            byte_offset: None,
        });
        problem.add_objective(Objective {
            name: b,
            coefficients: vec![Coefficient { name: x1_id, value: 2.0 }],
            constant: 0.0,
            quadratic: Vec::new(),
            attributes: crate::model::ObjectiveAttributes::default(),
            byte_offset: None,
        });

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
        problem.add_objective(Objective {
            name: obj_id,
            coefficients: vec![],
            constant: 0.0,
            quadratic: Vec::new(),
            attributes: crate::model::ObjectiveAttributes::default(),
            byte_offset: None,
        });
        let x1_id = problem.intern("x1");
        problem.add_variable(crate::model::Variable::new(x1_id).with_var_type(VariableType::General));

        let output = write_mps_string(&problem).unwrap();
        let reparsed = LpProblem::parse_mps(&output).unwrap();
        let x1 = &reparsed.variables[&reparsed.name_id("x1").unwrap()];
        assert_eq!(x1.kind, VariableKind::Integer);
    }

    #[test]
    fn test_ranges_round_trip() {
        // A RANGES row is flattened by the reader into `X` (>=) plus `X_rng`
        // (<=); the writer must fold the pair back into a single row with a
        // RANGES entry so the section survives MPS -> LpProblem -> MPS.
        let input = "\
NAME        rngtest
ROWS
 N  obj
 G  lim1
 L  lim2
COLUMNS
    x1        obj       1
    x1        lim1      1
    x1        lim2      2
RHS
    RHS       lim1      2
    RHS       lim2      10
RANGES
    RNG       lim1      4
    RNG       lim2      3
ENDATA
";
        let problem = LpProblem::parse_mps(input).unwrap();
        assert_eq!(problem.constraint_count(), 4, "two ranged rows must flatten into four constraints");

        let output = write_mps_string(&problem).unwrap();
        assert!(output.contains("RANGES"), "RANGES section must be re-emitted:\n{output}");
        assert!(!output.contains("lim1_rng"), "companion rows must fold back into the RANGES entry:\n{output}");

        let reparsed = LpProblem::parse_mps(&output).unwrap();
        assert_eq!(reparsed.constraint_count(), problem.constraint_count());
        for (name, expected_op, expected_rhs) in [
            ("lim1", ComparisonOp::GTE, 2.0),
            ("lim1_rng", ComparisonOp::LTE, 6.0),
            ("lim2", ComparisonOp::GTE, 7.0),
            ("lim2_rng", ComparisonOp::LTE, 10.0),
        ] {
            let id = reparsed.name_id(name).unwrap_or_else(|| panic!("constraint '{name}' missing after round trip"));
            let Some(Constraint::Standard { operator, rhs, .. }) = reparsed.constraints.get(&id) else {
                panic!("constraint '{name}' must be a standard constraint");
            };
            assert_eq!(*operator, expected_op, "operator mismatch for '{name}'");
            assert_eq!(*rhs, expected_rhs, "rhs mismatch for '{name}'");
        }
    }

    #[test]
    fn test_user_authored_rng_suffix_not_merged_when_structurally_different() {
        // A user constraint that merely ends in `_rng` must NOT be folded into
        // a RANGES entry unless it exactly matches the reader's flattening
        // pattern (identical coefficients, >=/<= pairing, upper >= lower).
        let input = "\
Minimize
 obj: x + y
Subject To
 c1: x + y >= 1
 c1_rng: x + 2 y <= 5
End
";
        let problem = LpProblem::parse(input).unwrap();
        let output = write_mps_string(&problem).unwrap();
        assert!(!output.contains("RANGES"), "structurally different pair must not merge:\n{output}");
        assert!(output.contains("c1_rng"), "companion row must be written as an ordinary row:\n{output}");
    }

    #[test]
    fn test_semi_continuous_round_trips() {
        let mut problem = LpProblem::new();
        let obj_id = problem.intern("obj");
        problem.add_objective(Objective {
            name: obj_id,
            coefficients: vec![],
            constant: 0.0,
            quadratic: Vec::new(),
            attributes: crate::model::ObjectiveAttributes::default(),
            byte_offset: None,
        });
        let x1_id = problem.intern("x1");
        problem.add_variable(crate::model::Variable::new(x1_id).with_var_type(VariableType::SemiContinuous));

        let output = write_mps_string(&problem).unwrap();
        assert!(output.contains("SC"));

        let reparsed = LpProblem::parse_mps(&output).unwrap();
        let x1 = &reparsed.variables[&reparsed.name_id("x1").unwrap()];
        assert_eq!(x1.kind, VariableKind::SemiContinuous);
    }

    #[test]
    fn vector_labels_avoid_row_and_column_names() {
        // A row named `RHS` used to be written as `RHS RHS 1`, which the
        // reader takes for a label-less line and rejects.
        let mut problem = LpProblem::new();
        let x_id = problem.intern("BOUND");
        let y_id = problem.intern("y");
        let obj_id = problem.intern("obj");
        problem.add_objective(Objective {
            name: obj_id,
            coefficients: vec![Coefficient { name: x_id, value: 1.0 }, Coefficient { name: y_id, value: 1.0 }],
            constant: 0.0,
            quadratic: Vec::new(),
            attributes: crate::model::ObjectiveAttributes::default(),
            byte_offset: None,
        });
        // `RNG` / `RNG_rng` fold into a RANGES entry; `RHS` is an ordinary row.
        for (name, operator, rhs) in [("RNG", ComparisonOp::GTE, 1.0), ("RNG_rng", ComparisonOp::LTE, 3.0), ("RHS", ComparisonOp::GTE, 2.0)]
        {
            let id = problem.intern(name);
            problem.add_constraint(Constraint::Standard {
                name: id,
                coefficients: vec![Coefficient { name: x_id, value: 1.0 }, Coefficient { name: y_id, value: 1.0 }],
                operator,
                rhs,
                byte_offset: None,
            });
        }
        problem.add_variable(crate::model::Variable::new(x_id).with_bounds(VariableBounds::upper(5.0)));

        let output = write_mps_string(&problem).expect("must write");
        assert!(output.contains("RHS1"), "RHS label must avoid the row named RHS:\n{output}");
        assert!(output.contains("RNG1"), "RANGES label must avoid the row named RNG:\n{output}");
        assert!(output.contains("BOUND1"), "BOUNDS label must avoid the column named BOUND:\n{output}");

        let reparsed = LpProblem::parse_mps(&output).unwrap_or_else(|e| panic!("written MPS must re-parse: {e}\n{output}"));
        let rhs_of = |name: &str| match &reparsed.constraints[&reparsed.name_id(name).unwrap()] {
            Constraint::Standard { rhs, .. } => *rhs,
            _ => panic!("{name} must be a standard row"),
        };
        assert_eq!(rhs_of("RHS"), 2.0);
        assert_eq!(rhs_of("RNG"), 1.0);
        assert_eq!(rhs_of("RNG_rng"), 3.0);
        let bound = reparsed.variables[&reparsed.name_id("BOUND").unwrap()].bounds;
        assert_eq!(bound.upper, Some(5.0));
    }

    #[test]
    fn names_the_reader_cannot_split_are_rejected() {
        for bad in ["two words", "$comment", "'MARKER'"] {
            let mut problem = LpProblem::new();
            let id = problem.intern(bad);
            problem.add_variable(crate::model::Variable::new(id));
            assert!(write_mps_string(&problem).is_err(), "variable '{bad}' must be rejected");
        }

        let mut problem = LpProblem::new();
        let set_id = problem.intern("set");
        let member_id = problem.intern("s1");
        problem.add_constraint(Constraint::SOS {
            name: set_id,
            sos_type: SOSType::S1,
            weights: vec![Coefficient { name: member_id, value: 1.0 }],
            byte_offset: None,
        });
        assert!(write_mps_string(&problem).is_err(), "an SOS member named like a set header must be rejected");
    }

    #[test]
    fn semi_continuous_upper_bound_is_the_sc_value() {
        // The BOUNDS lines written for a semi-continuous `x`, whitespace-normalised.
        let sc_line = |bounds: VariableBounds| -> LpResult<Vec<String>> {
            let mut problem = LpProblem::new();
            let x_id = problem.intern("x");
            problem.add_variable(crate::model::Variable::new(x_id).with_kind(VariableKind::SemiContinuous).with_bounds(bounds));
            let output = write_mps_string(&problem)?;
            let bounds_section = output.split("BOUNDS\n").nth(1).expect("BOUNDS section present");
            Ok(bounds_section
                .lines()
                .take_while(|l| l.starts_with(' '))
                .map(|l| l.split_whitespace().collect::<Vec<_>>().join(" "))
                .collect())
        };

        assert_eq!(sc_line(VariableBounds::range(2.0, 50.0)).unwrap(), ["LO BOUND x 2", "SC BOUND x 50"]);
        assert_eq!(sc_line(VariableBounds::upper(f64::INFINITY)).unwrap(), ["SC BOUND x 1e30"]);
        assert_eq!(sc_line(VariableBounds::default()).unwrap(), ["SC BOUND x 1e30"]);
        assert!(sc_line(VariableBounds::upper(f64::NAN)).is_err());
        assert!(sc_line(VariableBounds::upper(f64::NEG_INFINITY)).is_err());
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
        assert_eq!(x1.bounds, VariableBounds::upper(f64::INFINITY));

        let output = write_mps_string(&problem).unwrap();
        assert!(output.contains(" PL BOUND"));
        assert!(!output.contains("inf"), "must not leak a raw `inf` literal into the bounds line");

        let reparsed = LpProblem::parse_mps(&output).unwrap();
        let x1 = &reparsed.variables[&reparsed.name_id("x1").unwrap()];
        assert_eq!(x1.bounds, VariableBounds::upper(f64::INFINITY));
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
        assert_eq!(x1.bounds, VariableBounds::lower(f64::NEG_INFINITY));

        let output = write_mps_string(&problem).unwrap();
        assert!(output.contains(" MI BOUND"));
        assert!(!output.contains("inf"), "must not leak a raw `inf` literal into the bounds line");

        let reparsed = LpProblem::parse_mps(&output).unwrap();
        let x1 = &reparsed.variables[&reparsed.name_id("x1").unwrap()];
        assert_eq!(x1.bounds, VariableBounds::lower(f64::NEG_INFINITY));
    }

    #[test]
    fn nan_upper_bound_returns_validation_error() {
        let mut problem = LpProblem::new();
        let obj_id = problem.intern("obj");
        let x1_id = problem.intern("x1");
        problem.add_objective(Objective {
            name: obj_id,
            coefficients: vec![Coefficient { name: x1_id, value: 1.0 }],
            constant: 0.0,
            quadratic: Vec::new(),
            attributes: crate::model::ObjectiveAttributes::default(),
            byte_offset: None,
        });
        problem.update_variable_type("x1", VariableType::UpperBound(f64::NAN)).unwrap();

        let err = write_mps_string(&problem).unwrap_err();
        assert!(matches!(err, LpParseError::ValidationError { .. }));
    }

    #[test]
    fn nan_lower_bound_returns_validation_error() {
        let mut problem = LpProblem::new();
        let obj_id = problem.intern("obj");
        let x1_id = problem.intern("x1");
        problem.add_objective(Objective {
            name: obj_id,
            coefficients: vec![Coefficient { name: x1_id, value: 1.0 }],
            constant: 0.0,
            quadratic: Vec::new(),
            attributes: crate::model::ObjectiveAttributes::default(),
            byte_offset: None,
        });
        problem.update_variable_type("x1", VariableType::LowerBound(f64::NAN)).unwrap();

        let err = write_mps_string(&problem).unwrap_err();
        assert!(matches!(err, LpParseError::ValidationError { .. }));
    }

    #[test]
    fn nonsensical_upper_bound_negative_infinity_returns_validation_error() {
        let mut problem = LpProblem::new();
        let obj_id = problem.intern("obj");
        let x1_id = problem.intern("x1");
        problem.add_objective(Objective {
            name: obj_id,
            coefficients: vec![Coefficient { name: x1_id, value: 1.0 }],
            constant: 0.0,
            quadratic: Vec::new(),
            attributes: crate::model::ObjectiveAttributes::default(),
            byte_offset: None,
        });
        problem.update_variable_type("x1", VariableType::UpperBound(f64::NEG_INFINITY)).unwrap();

        let err = write_mps_string(&problem).unwrap_err();
        assert!(matches!(err, LpParseError::ValidationError { .. }));
    }

    #[test]
    fn nonsensical_lower_bound_positive_infinity_returns_validation_error() {
        let mut problem = LpProblem::new();
        let obj_id = problem.intern("obj");
        let x1_id = problem.intern("x1");
        problem.add_objective(Objective {
            name: obj_id,
            coefficients: vec![Coefficient { name: x1_id, value: 1.0 }],
            constant: 0.0,
            quadratic: Vec::new(),
            attributes: crate::model::ObjectiveAttributes::default(),
            byte_offset: None,
        });
        problem.update_variable_type("x1", VariableType::LowerBound(f64::INFINITY)).unwrap();

        let err = write_mps_string(&problem).unwrap_err();
        assert!(matches!(err, LpParseError::ValidationError { .. }));
    }

    #[test]
    fn nan_double_bound_returns_validation_error() {
        let mut problem = LpProblem::new();
        let obj_id = problem.intern("obj");
        let x1_id = problem.intern("x1");
        problem.add_objective(Objective {
            name: obj_id,
            coefficients: vec![Coefficient { name: x1_id, value: 1.0 }],
            constant: 0.0,
            quadratic: Vec::new(),
            attributes: crate::model::ObjectiveAttributes::default(),
            byte_offset: None,
        });
        problem.update_variable_type("x1", VariableType::DoubleBound(f64::NAN, 5.0)).unwrap();

        let err = write_mps_string(&problem).unwrap_err();
        assert!(matches!(err, LpParseError::ValidationError { .. }));
    }

    #[test]
    fn nonsensical_double_bound_returns_validation_error() {
        let mut problem = LpProblem::new();
        let obj_id = problem.intern("obj");
        let x1_id = problem.intern("x1");
        problem.add_objective(Objective {
            name: obj_id,
            coefficients: vec![Coefficient { name: x1_id, value: 1.0 }],
            constant: 0.0,
            quadratic: Vec::new(),
            attributes: crate::model::ObjectiveAttributes::default(),
            byte_offset: None,
        });
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
        assert_eq!(x1.bounds, VariableBounds::range(0.0, 20.0));
    }

    /// The regression guard for the MPS writer half of the free/undeclared
    /// conflation, which this module's docs used to carry as an accepted
    /// caveat: an undeclared variable was emitted as `FR`, widening its
    /// feasible region to include negatives on the way to another solver.
    #[test]
    fn an_undeclared_variable_gets_no_bounds_entry() {
        let source = "minimize\nobj: x + y\nsubject to\nc1: x + y >= 2\nbounds\ny free\nend\n";
        let problem = LpProblem::parse(source).expect("fixture must parse");
        let written = write_mps_string(&problem).expect("must write");

        assert!(written.contains(" FR "), "the declared-free y must be written as FR:\n{written}");
        let free_lines: Vec<&str> = written.lines().filter(|line| line.contains(" FR ")).collect();
        assert_eq!(free_lines.len(), 1, "only y is free, got:\n{written}");
        assert!(free_lines[0].ends_with('y'), "the FR bound must be y's, got {:?}", free_lines[0]);
    }

    #[test]
    fn snapshot_representative_problem() {
        let problem = build_problem_with_bounds_and_sos();
        let output = write_mps_string(&problem).unwrap();
        insta::assert_snapshot!(output);
    }
}
