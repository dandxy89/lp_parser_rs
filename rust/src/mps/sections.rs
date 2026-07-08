use std::borrow::Cow;
use std::collections::hash_map::Entry;

use rustc_hash::{FxHashMap, FxHashSet};

use super::{BoundAccumulator, RowType, split_fields};
use crate::error::{LpParseError, LpResult};
use crate::lexer::{RawCoefficient, RawConstraint};
use crate::model::SOSType;

/// Iterate the whitespace-separated fields of an MPS data line, honouring `$`
/// inline comments (a `$`-prefixed field truncates the rest of the line).
fn data_fields(line: &str) -> impl Iterator<Item = &str> {
    line.split_whitespace().take_while(|f| !f.starts_with('$'))
}

/// Parse a single ROWS data line.
pub(super) fn parse_rows_line<'input>(
    line: &'input str,
    line_num: usize,
    objective_rows: &mut Vec<&'input str>,
    row_types: &mut FxHashMap<&'input str, RowType>,
    row_order: &mut Vec<&'input str>,
) -> LpResult<()> {
    debug_assert!(!line.is_empty(), "parse_rows_line called with empty line");
    debug_assert!(line_num > 0, "line_num must be 1-based");

    let (buf, len) = split_fields(line);
    let fields = &buf[..len];
    if fields.is_empty() {
        // A data line consisting solely of a `$` inline comment yields no fields.
        return Ok(());
    }
    if fields.len() < 2 {
        return Err(LpParseError::parse_error(line_num, format!("ROWS line requires type and name, got {} field(s)", fields.len())));
    }

    let row_type = match fields[0] {
        "N" | "n" => RowType::N,
        "L" | "l" => RowType::L,
        "G" | "g" => RowType::G,
        "E" | "e" => RowType::E,
        other => {
            return Err(LpParseError::parse_error(line_num, format!("Unknown row type: '{other}'")));
        }
    };

    let row_name = fields[1];

    // The MPS spec requires row names to be unique; a silent overwrite would
    // change the meaning of the model.
    if row_types.insert(row_name, row_type).is_some() {
        return Err(LpParseError::parse_error(line_num, format!("Duplicate row name: '{row_name}'")));
    }

    if row_type == RowType::N {
        if fields.len() > 2 {
            eprintln!(
                "Line {line_num}: N-row '{row_name}' has extra fields (priority/weight/tolerance) \
                 which are not supported and will be ignored"
            );
        }
        objective_rows.push(row_name);
    } else {
        row_order.push(row_name);
    }

    Ok(())
}

/// Mutable state for parsing the COLUMNS section.
#[derive(Default)]
pub(super) struct ColumnsState<'input> {
    /// Accumulated coefficient per (variable, row) pair. MPS allows split
    /// entries, so values are summed on duplicate keys.
    pub(super) coefficients: FxHashMap<(&'input str, &'input str), f64>,
    /// Per-row list of (column index, variable) pairs in first-insertion
    /// order. Lets the builders iterate only a row's nonzeros instead of
    /// probing every (row, column) combination.
    pub(super) row_entries: FxHashMap<&'input str, Vec<(u32, &'input str)>>,
    pub(super) column_order: Vec<&'input str>,
    pub(super) column_index: FxHashMap<&'input str, u32>,
    pub(super) in_integer_block: bool,
    pub(super) integer_vars: Vec<&'input str>,
    pub(super) integer_vars_set: FxHashSet<&'input str>,
}

impl<'input> ColumnsState<'input> {
    /// Parse a single COLUMNS data line.
    ///
    /// The strict MPS format allows at most two (row, value) pairs per line,
    /// but free-format writers emit more; all pairs are parsed. A trailing row
    /// name without a value is an error rather than silent data loss.
    pub(super) fn parse_line(
        &mut self,
        line: &'input str,
        line_num: usize,
        row_types: &FxHashMap<&str, RowType>,
        objective_rows: &[&str],
    ) -> LpResult<()> {
        debug_assert!(!line.is_empty(), "ColumnsState::parse_line called with empty line");
        debug_assert!(line_num > 0, "line_num must be 1-based");

        let mut fields = data_fields(line);
        let Some(var_name) = fields.next() else {
            // A data line consisting solely of a `$` inline comment yields no fields.
            return Ok(());
        };
        let second = fields.next();

        // Check for MARKER lines (integer block markers)
        if second == Some("'MARKER'") {
            match fields.next().map(|f| f.trim_matches('\'')) {
                Some("INTORG") => self.in_integer_block = true,
                Some("INTEND") => self.in_integer_block = false,
                Some(other) => {
                    return Err(LpParseError::parse_error(line_num, format!("Unknown MARKER type: '{other}'")));
                }
                None => {
                    return Err(LpParseError::parse_error(line_num, "MARKER line is missing its INTORG/INTEND field"));
                }
            }
            return Ok(());
        }

        // Normal data line: var_name (row_name value)+
        let (Some(first_row), Some(first_value)) = (second, fields.next()) else {
            return Err(LpParseError::parse_error(
                line_num,
                "COLUMNS data line requires a variable name and at least one (row, value) pair",
            ));
        };

        // Track column order
        if let Entry::Vacant(entry) = self.column_index.entry(var_name) {
            // Column count fits in u32: an MPS file with > 4 billion columns
            // would exceed addressable memory long before this truncates.
            entry.insert(u32::try_from(self.column_order.len()).unwrap_or(u32::MAX));
            self.column_order.push(var_name);
        }

        // Mark as integer if inside INTORG/INTEND block
        if self.in_integer_block && self.integer_vars_set.insert(var_name) {
            self.integer_vars.push(var_name);
        }

        self.parse_entry(first_row, first_value, var_name, line_num, row_types, objective_rows)?;
        while let Some(row_name) = fields.next() {
            let Some(value) = fields.next() else {
                return Err(LpParseError::parse_error(line_num, format!("COLUMNS row '{row_name}' has no value field")));
            };
            self.parse_entry(row_name, value, var_name, line_num, row_types, objective_rows)?;
        }

        Ok(())
    }

    /// Parse a single (`row_name`, value) entry from a COLUMNS line.
    fn parse_entry(
        &mut self,
        row_name: &'input str,
        value_str: &str,
        var_name: &'input str,
        line_num: usize,
        row_types: &FxHashMap<&str, RowType>,
        objective_rows: &[&str],
    ) -> LpResult<()> {
        debug_assert!(!row_name.is_empty(), "parse_entry called with empty row_name");
        debug_assert!(!var_name.is_empty(), "parse_entry called with empty var_name");

        if !row_types.contains_key(row_name) && !objective_rows.contains(&row_name) {
            return Err(LpParseError::parse_error(line_num, format!("Reference to undefined row: '{row_name}'")));
        }

        let value: f64 = value_str.parse().map_err(|_| LpParseError::invalid_number(value_str, line_num))?;

        // Accumulate coefficient (additive -- MPS allows split entries)
        match self.coefficients.entry((var_name, row_name)) {
            Entry::Occupied(mut entry) => *entry.get_mut() += value,
            Entry::Vacant(entry) => {
                entry.insert(value);
                let col_idx = *self.column_index.get(var_name).expect("column index registered in parse_line before parse_entry");
                self.row_entries.entry(row_name).or_default().push((col_idx, var_name));
            }
        }

        Ok(())
    }
}

/// Parse a single RHS data line.
///
/// Only the first RHS vector is used; subsequent vectors with different labels
/// are silently skipped per the CPLEX spec. The vector label may be omitted
/// (it is a blank field in fixed-format files): when the first field names a
/// known row, the whole line is read as (row, value) pairs.
pub(super) fn parse_rhs_line<'input>(
    line: &'input str,
    line_num: usize,
    row_types: &FxHashMap<&str, RowType>,
    objective_rows: &[&str],
    rhs_values: &mut FxHashMap<&'input str, f64>,
    first_vector_label: &mut Option<&'input str>,
) -> LpResult<()> {
    debug_assert!(!line.is_empty(), "parse_rhs_line called with empty line");
    debug_assert!(line_num > 0, "line_num must be 1-based");

    let mut fields = data_fields(line);
    let Some(first) = fields.next() else {
        // A data line consisting solely of a `$` inline comment yields no fields.
        return Ok(());
    };

    // A vector label that collides with a row name is misread as label-less
    // here, but the following field then fails to parse as a value, so the
    // mistake is loud rather than silent.
    let is_row = row_types.contains_key(first) || objective_rows.contains(&first);
    let (label, mut pending_row) = if is_row { ("", Some(first)) } else { (first, None) };

    match *first_vector_label {
        None => *first_vector_label = Some(label),
        Some(seen) if seen != label => return Ok(()),
        _ => {}
    }

    let mut pairs = 0usize;
    while let Some(row_name) = pending_row.take().or_else(|| fields.next()) {
        let Some(value) = fields.next() else {
            return Err(LpParseError::parse_error(line_num, format!("RHS row '{row_name}' has no value field")));
        };
        parse_rhs_entry(row_name, value, line_num, row_types, objective_rows, rhs_values)?;
        pairs += 1;
    }
    if pairs == 0 {
        return Err(LpParseError::parse_error(line_num, "RHS data line requires at least one (row, value) pair"));
    }

    Ok(())
}

/// Parse a single (`row_name`, value) RHS entry.
fn parse_rhs_entry<'input>(
    row_name: &'input str,
    value_str: &str,
    line_num: usize,
    row_types: &FxHashMap<&str, RowType>,
    objective_rows: &[&str],
    rhs_values: &mut FxHashMap<&'input str, f64>,
) -> LpResult<()> {
    debug_assert!(!row_name.is_empty(), "parse_rhs_entry called with empty row_name");
    debug_assert!(line_num > 0, "line_num must be 1-based");

    if !row_types.contains_key(row_name) && !objective_rows.contains(&row_name) {
        return Err(LpParseError::parse_error(line_num, format!("RHS reference to undefined row: '{row_name}'")));
    }

    let value: f64 = value_str.parse().map_err(|_| LpParseError::invalid_number(value_str, line_num))?;

    rhs_values.insert(row_name, value);

    Ok(())
}

/// Parse a single RANGES data line.
///
/// Only the first RANGES vector is used; subsequent vectors with different
/// labels are silently skipped per the CPLEX spec. As with RHS, the vector
/// label may be omitted when the first field names a known row.
pub(super) fn parse_ranges_line<'input>(
    line: &'input str,
    line_num: usize,
    row_types: &FxHashMap<&str, RowType>,
    range_values: &mut FxHashMap<&'input str, f64>,
    first_vector_label: &mut Option<&'input str>,
) -> LpResult<()> {
    debug_assert!(!line.is_empty(), "parse_ranges_line called with empty line");
    debug_assert!(line_num > 0, "line_num must be 1-based");

    let mut fields = data_fields(line);
    let Some(first) = fields.next() else {
        // A data line consisting solely of a `$` inline comment yields no fields.
        return Ok(());
    };

    let is_row = row_types.contains_key(first);
    let (label, mut pending_row) = if is_row { ("", Some(first)) } else { (first, None) };

    match *first_vector_label {
        None => *first_vector_label = Some(label),
        Some(seen) if seen != label => return Ok(()),
        _ => {}
    }

    let mut pairs = 0usize;
    while let Some(row_name) = pending_row.take().or_else(|| fields.next()) {
        let Some(value) = fields.next() else {
            return Err(LpParseError::parse_error(line_num, format!("RANGES row '{row_name}' has no value field")));
        };
        parse_range_entry(row_name, value, line_num, row_types, range_values)?;
        pairs += 1;
    }
    if pairs == 0 {
        return Err(LpParseError::parse_error(line_num, "RANGES data line requires at least one (row, value) pair"));
    }

    Ok(())
}

/// Parse a single (`row_name`, value) RANGES entry.
fn parse_range_entry<'input>(
    row_name: &'input str,
    value_str: &str,
    line_num: usize,
    row_types: &FxHashMap<&str, RowType>,
    range_values: &mut FxHashMap<&'input str, f64>,
) -> LpResult<()> {
    debug_assert!(!row_name.is_empty(), "parse_range_entry called with empty row_name");
    debug_assert!(line_num > 0, "line_num must be 1-based");

    match row_types.get(row_name) {
        None => {
            return Err(LpParseError::parse_error(line_num, format!("RANGES reference to undefined row: '{row_name}'")));
        }
        Some(RowType::N) => {
            return Err(LpParseError::parse_error(line_num, format!("RANGES entry on objective (N) row '{row_name}' is not allowed")));
        }
        Some(_) => {}
    }

    let value: f64 = value_str.parse().map_err(|_| LpParseError::invalid_number(value_str, line_num))?;

    range_values.insert(row_name, value);

    Ok(())
}

/// Mutable state for parsing the BOUNDS section.
#[derive(Default)]
pub(super) struct BoundsState<'input> {
    pub(super) accumulators: FxHashMap<&'input str, BoundAccumulator>,
    pub(super) order: Vec<&'input str>,
    seen: FxHashSet<&'input str>,
    pub(super) binary_vars: Vec<&'input str>,
    pub(super) semi_continuous_vars: Vec<&'input str>,
}

impl<'input> BoundsState<'input> {
    /// Parse a single BOUNDS data line.
    ///
    /// Only the first BOUNDS vector is used; subsequent vectors with different
    /// labels are silently skipped per the CPLEX spec. The vector label may be
    /// omitted: when the field after the bound type names a known column, the
    /// line is read as `TYPE var [value]`. Duplicate lower or upper bounds on
    /// the same variable are rejected.
    pub(super) fn parse_line(
        &mut self,
        line: &'input str,
        line_num: usize,
        columns: &FxHashMap<&str, u32>,
        integer_vars: &mut Vec<&'input str>,
        integer_vars_set: &mut FxHashSet<&'input str>,
        first_vector_label: &mut Option<&'input str>,
    ) -> LpResult<()> {
        debug_assert!(!line.is_empty(), "BoundsState::parse_line called with empty line");
        debug_assert!(line_num > 0, "line_num must be 1-based");

        let (buf, len) = split_fields(line);
        let fields = &buf[..len];
        if fields.is_empty() {
            // A data line consisting solely of a `$` inline comment yields no fields.
            return Ok(());
        }
        if fields.len() < 2 {
            return Err(LpParseError::parse_error(line_num, format!("BOUNDS line requires at least 2 fields, got {}", fields.len())));
        }

        let bound_type = fields[0];
        // Label-less detection: a bound label that collides with a column name
        // is misread as label-less, but such files are already ambiguous.
        let label_less = columns.contains_key(fields[1]);
        let (label, var_idx) = if label_less { ("", 1) } else { (fields[1], 2) };
        let Some(&var_name) = fields.get(var_idx) else {
            return Err(LpParseError::parse_error(line_num, "BOUNDS line requires a variable name"));
        };

        match *first_vector_label {
            None => *first_vector_label = Some(label),
            Some(seen) if seen != label => return Ok(()),
            _ => {}
        }

        // Track bound order
        if self.seen.insert(var_name) {
            self.order.push(var_name);
        }

        self.apply_bound(bound_type, var_name, fields.get(var_idx + 1).copied(), line_num, integer_vars, integer_vars_set)
    }

    /// Apply a single bound directive to the accumulator for `var_name`.
    fn apply_bound(
        &mut self,
        bound_type: &str,
        var_name: &'input str,
        value_field: Option<&'input str>,
        line_num: usize,
        integer_vars: &mut Vec<&'input str>,
        integer_vars_set: &mut FxHashSet<&'input str>,
    ) -> LpResult<()> {
        debug_assert!(!bound_type.is_empty(), "apply_bound called with empty bound_type");
        debug_assert!(!var_name.is_empty(), "apply_bound called with empty var_name");

        let accumulator = self.accumulators.entry(var_name).or_default();
        let upper = bound_type.to_ascii_uppercase();

        match upper.as_str() {
            "LO" | "LI" => {
                if accumulator.lower.is_some() {
                    let label = if upper == "LI" { "(LI) " } else { "" };
                    return Err(LpParseError::invalid_bounds(var_name, format!("duplicate lower bound {label}at line {line_num}")));
                }
                let value = parse_bound_value(value_field, line_num, bound_type)?;
                accumulator.lower = Some(value);
                if upper == "LI" && integer_vars_set.insert(var_name) {
                    integer_vars.push(var_name);
                }
            }
            "UP" | "UI" => {
                if accumulator.upper.is_some() {
                    let label = if upper == "UI" { "(UI) " } else { "" };
                    return Err(LpParseError::invalid_bounds(var_name, format!("duplicate upper bound {label}at line {line_num}")));
                }
                let value = parse_bound_value(value_field, line_num, bound_type)?;
                accumulator.upper = Some(value);
                if upper == "UI" && integer_vars_set.insert(var_name) {
                    integer_vars.push(var_name);
                }
            }
            "FX" => {
                if accumulator.fixed.is_some() {
                    return Err(LpParseError::invalid_bounds(var_name, format!("duplicate fixed bound at line {line_num}")));
                }
                let value = parse_bound_value(value_field, line_num, bound_type)?;
                accumulator.fixed = Some(value);
            }
            "FR" => {
                accumulator.free = true;
            }
            "MI" => {
                if accumulator.lower.is_some() {
                    return Err(LpParseError::invalid_bounds(var_name, format!("duplicate lower bound (MI) at line {line_num}")));
                }
                accumulator.lower = Some(f64::NEG_INFINITY);
            }
            "PL" => {
                if accumulator.upper.is_some() {
                    return Err(LpParseError::invalid_bounds(var_name, format!("duplicate upper bound (PL) at line {line_num}")));
                }
                accumulator.upper = Some(f64::INFINITY);
            }
            "BV" => {
                accumulator.binary = true;
                self.binary_vars.push(var_name);
            }
            "SC" => {
                let value = parse_bound_value(value_field, line_num, bound_type)?;
                // Semi-continuity is represented by `VariableType::SemiContinuous`,
                // which carries no bound value: recording the SC upper bound in the
                // accumulator would resolve the variable to `UpperBound` instead and
                // lose the semi-continuity (it also broke the MPS round trip). The
                // bound value is dropped; warn when it is a meaningful finite bound
                // rather than the conventional 1e30/infinity sentinel.
                if value.is_finite() && value < crate::mps::writer::SEMI_CONTINUOUS_SENTINEL_UPPER {
                    eprintln!("line {line_num}: SC upper bound {value} on '{var_name}' cannot be represented and is dropped");
                }
                self.semi_continuous_vars.push(var_name);
            }
            "SI" => {
                // Semi-integer: the model has no semi-integer type, so the closest
                // representation is an integer variable with the given upper bound.
                let value = parse_bound_value(value_field, line_num, bound_type)?;
                accumulator.upper = Some(value);
                if integer_vars_set.insert(var_name) {
                    integer_vars.push(var_name);
                }
            }
            other => {
                return Err(LpParseError::parse_error(line_num, format!("Unknown bound type: '{other}'")));
            }
        }

        Ok(())
    }
}

/// Parse the value field from a BOUNDS line.
fn parse_bound_value(field: Option<&str>, line_num: usize, bound_type: &str) -> LpResult<f64> {
    debug_assert!(line_num > 0, "line_num must be 1-based");
    debug_assert!(!bound_type.is_empty(), "parse_bound_value called with empty bound_type");

    let value_str = field.ok_or_else(|| LpParseError::parse_error(line_num, format!("Bound type '{bound_type}' requires a value")))?;

    value_str.parse().map_err(|_| LpParseError::invalid_number(value_str, line_num))
}

/// Parse a single SOS data line.
pub(super) fn parse_sos_line<'input>(
    line: &'input str,
    line_num: usize,
    sos_constraints: &mut Vec<RawConstraint<'input>>,
    current_name: &mut Option<&'input str>,
    current_type: &mut Option<SOSType>,
    current_weights: &mut Vec<RawCoefficient<'input>>,
) -> LpResult<()> {
    debug_assert!(!line.is_empty(), "parse_sos_line called with empty line");
    debug_assert!(line_num > 0, "line_num must be 1-based");

    let trimmed = line.trim();

    // An SOS set header line has "S1"/"S2" as its first whole token. A prefix
    // match would misread a weight entry for a variable named e.g. "S1X" as a
    // new set header, so compare the full token.
    let mut fields = trimmed.split_whitespace();
    let Some(type_token) = fields.next() else {
        return Ok(());
    };
    let upper = type_token.to_ascii_uppercase();
    if upper == "S1" || upper == "S2" {
        // Flush previous SOS constraint
        flush_sos_constraint(sos_constraints, current_name, current_type, current_weights);

        let sos_type = if upper == "S1" { SOSType::S1 } else { SOSType::S2 };
        // The rest of the header might contain a name; the fields borrow from
        // `line`, so no re-slicing or allocation is needed.
        let name = fields.next().unwrap_or("");

        *current_type = Some(sos_type);
        *current_name = Some(if name.is_empty() { type_token } else { name });

        return Ok(());
    }

    // SOS weight entry: var_name weight
    let mut fields = line.split_whitespace();
    if let (Some(var_name), Some(weight_field)) = (fields.next(), fields.next()) {
        let weight: f64 = weight_field.parse().map_err(|_| LpParseError::invalid_number(weight_field, line_num))?;
        current_weights.push(RawCoefficient { name: var_name, value: weight });
    } else if !trimmed.is_empty() {
        return Err(LpParseError::parse_error(line_num, format!("SOS entry requires variable name and weight, got: '{trimmed}'")));
    }

    Ok(())
}

/// Flush the current SOS constraint into the results vector.
pub(super) fn flush_sos_constraint<'input>(
    sos_constraints: &mut Vec<RawConstraint<'input>>,
    current_name: &mut Option<&'input str>,
    current_type: &mut Option<SOSType>,
    current_weights: &mut Vec<RawCoefficient<'input>>,
) {
    if let (Some(name), Some(sos_type)) = (*current_name, *current_type) {
        if current_weights.is_empty() {
            eprintln!("SOS set '{name}' declares no weight entries and will be dropped");
        } else {
            sos_constraints.push(RawConstraint::SOS {
                name: Cow::Borrowed(name),
                sos_type,
                weights: std::mem::take(current_weights),
                byte_offset: None,
            });
        }
        *current_name = None;
        *current_type = None;
    } else if !current_weights.is_empty() {
        // Weight entries seen before any set header cannot belong to a set;
        // clearing them stops them bleeding into the next declared set.
        eprintln!("{} SOS weight entr(ies) with no preceding SOS set header were ignored", current_weights.len());
        current_weights.clear();
    }

    debug_assert!(current_name.is_none(), "current_name must be None after flush");
}
