use std::borrow::Cow;
use std::collections::{HashMap, HashSet};

use super::{BoundAccumulator, RowType, strip_dollar_comments};
use crate::error::{LpParseError, LpResult};
use crate::lexer::{RawCoefficient, RawConstraint};
use crate::model::SOSType;

/// Parse a single ROWS data line.
pub(super) fn parse_rows_line<'input>(
    line: &'input str,
    line_num: usize,
    objective_rows: &mut Vec<&'input str>,
    row_types: &mut HashMap<&'input str, RowType>,
    row_order: &mut Vec<&'input str>,
) -> LpResult<()> {
    debug_assert!(!line.is_empty(), "parse_rows_line called with empty line");
    debug_assert!(line_num > 0, "line_num must be 1-based");

    let raw_fields: Vec<&str> = line.split_whitespace().collect();
    let fields = strip_dollar_comments(&raw_fields);
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

    if row_type == RowType::N {
        if fields.len() > 2 {
            log::warn!(
                "Line {line_num}: N-row '{row_name}' has extra fields (priority/weight/tolerance) \
                 which are not supported and will be ignored"
            );
        }
        objective_rows.push(row_name);
    } else {
        row_order.push(row_name);
    }

    row_types.insert(row_name, row_type);

    Ok(())
}

/// Mutable state for parsing the COLUMNS section.
#[derive(Default)]
pub(super) struct ColumnsState<'input> {
    pub(super) coefficients: HashMap<(&'input str, &'input str), f64>,
    pub(super) column_order: Vec<&'input str>,
    column_seen: HashSet<&'input str>,
    pub(super) in_integer_block: bool,
    pub(super) integer_vars: Vec<&'input str>,
    pub(super) integer_vars_set: HashSet<&'input str>,
}

impl<'input> ColumnsState<'input> {
    /// Parse a single COLUMNS data line.
    pub(super) fn parse_line(
        &mut self,
        line: &'input str,
        line_num: usize,
        row_types: &HashMap<&str, RowType>,
        objective_rows: &[&str],
    ) -> LpResult<()> {
        debug_assert!(!line.is_empty(), "ColumnsState::parse_line called with empty line");
        debug_assert!(line_num > 0, "line_num must be 1-based");

        let raw_fields: Vec<&'input str> = line.split_whitespace().collect();
        let fields = strip_dollar_comments(&raw_fields);
        if fields.is_empty() {
            return Ok(());
        }

        // Check for MARKER lines (integer block markers)
        if fields.len() >= 3 && fields[1] == "'MARKER'" {
            let marker_type = fields[2].trim_matches('\'');
            match marker_type {
                "INTORG" => self.in_integer_block = true,
                "INTEND" => self.in_integer_block = false,
                other => {
                    return Err(LpParseError::parse_error(line_num, format!("Unknown MARKER type: '{other}'")));
                }
            }
            return Ok(());
        }

        // Normal data line: var_name row_name value [row_name value]
        if fields.len() < 3 {
            return Err(LpParseError::parse_error(line_num, format!("COLUMNS data line requires at least 3 fields, got {}", fields.len())));
        }

        let var_name = fields[0];

        // Track column order
        if self.column_seen.insert(var_name) {
            self.column_order.push(var_name);
        }

        // Mark as integer if inside INTORG/INTEND block
        if self.in_integer_block && self.integer_vars_set.insert(var_name) {
            self.integer_vars.push(var_name);
        }

        // Parse first (row_name, value) pair
        self.parse_entry(fields[1], fields[2], var_name, line_num, row_types, objective_rows)?;

        // Parse optional second (row_name, value) pair
        if fields.len() >= 5 {
            self.parse_entry(fields[3], fields[4], var_name, line_num, row_types, objective_rows)?;
        }

        Ok(())
    }

    /// Parse a single (row_name, value) entry from a COLUMNS line.
    fn parse_entry(
        &mut self,
        row_name: &'input str,
        value_str: &str,
        var_name: &'input str,
        line_num: usize,
        row_types: &HashMap<&str, RowType>,
        objective_rows: &[&str],
    ) -> LpResult<()> {
        debug_assert!(!row_name.is_empty(), "parse_entry called with empty row_name");
        debug_assert!(!var_name.is_empty(), "parse_entry called with empty var_name");

        if !row_types.contains_key(row_name) && !objective_rows.contains(&row_name) {
            return Err(LpParseError::parse_error(line_num, format!("Reference to undefined row: '{row_name}'")));
        }

        let value: f64 = value_str.parse().map_err(|_| LpParseError::invalid_number(value_str, line_num))?;

        // Accumulate coefficient (additive -- MPS allows split entries)
        *self.coefficients.entry((var_name, row_name)).or_insert(0.0) += value;

        Ok(())
    }
}

/// Parse a single RHS data line.
///
/// Only the first RHS vector is used; subsequent vectors with different labels
/// are silently skipped per the CPLEX spec.
pub(super) fn parse_rhs_line<'input>(
    line: &'input str,
    line_num: usize,
    row_types: &HashMap<&str, RowType>,
    objective_rows: &[&str],
    rhs_values: &mut HashMap<&'input str, f64>,
    first_vector_label: &mut Option<&'input str>,
) -> LpResult<()> {
    debug_assert!(!line.is_empty(), "parse_rhs_line called with empty line");
    debug_assert!(line_num > 0, "line_num must be 1-based");

    let raw_fields: Vec<&'input str> = line.split_whitespace().collect();
    let fields = strip_dollar_comments(&raw_fields);
    if fields.len() < 3 {
        return Err(LpParseError::parse_error(line_num, format!("RHS data line requires at least 3 fields, got {}", fields.len())));
    }

    let label = fields[0];
    match *first_vector_label {
        None => *first_vector_label = Some(label),
        Some(first) if first != label => return Ok(()),
        _ => {}
    }

    parse_rhs_entry(fields[1], fields[2], line_num, row_types, objective_rows, rhs_values)?;

    if fields.len() >= 5 {
        parse_rhs_entry(fields[3], fields[4], line_num, row_types, objective_rows, rhs_values)?;
    }

    Ok(())
}

/// Parse a single (row_name, value) RHS entry.
fn parse_rhs_entry<'input>(
    row_name: &'input str,
    value_str: &str,
    line_num: usize,
    row_types: &HashMap<&str, RowType>,
    objective_rows: &[&str],
    rhs_values: &mut HashMap<&'input str, f64>,
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
/// labels are silently skipped per the CPLEX spec.
pub(super) fn parse_ranges_line<'input>(
    line: &'input str,
    line_num: usize,
    row_types: &HashMap<&str, RowType>,
    range_values: &mut HashMap<&'input str, f64>,
    first_vector_label: &mut Option<&'input str>,
) -> LpResult<()> {
    debug_assert!(!line.is_empty(), "parse_ranges_line called with empty line");
    debug_assert!(line_num > 0, "line_num must be 1-based");

    let raw_fields: Vec<&'input str> = line.split_whitespace().collect();
    let fields = strip_dollar_comments(&raw_fields);
    if fields.len() < 3 {
        return Err(LpParseError::parse_error(line_num, format!("RANGES data line requires at least 3 fields, got {}", fields.len())));
    }

    let label = fields[0];
    match *first_vector_label {
        None => *first_vector_label = Some(label),
        Some(first) if first != label => return Ok(()),
        _ => {}
    }

    parse_range_entry(fields[1], fields[2], line_num, row_types, range_values)?;

    if fields.len() >= 5 {
        parse_range_entry(fields[3], fields[4], line_num, row_types, range_values)?;
    }

    Ok(())
}

/// Parse a single (row_name, value) RANGES entry.
fn parse_range_entry<'input>(
    row_name: &'input str,
    value_str: &str,
    line_num: usize,
    row_types: &HashMap<&str, RowType>,
    range_values: &mut HashMap<&'input str, f64>,
) -> LpResult<()> {
    debug_assert!(!row_name.is_empty(), "parse_range_entry called with empty row_name");
    debug_assert!(line_num > 0, "line_num must be 1-based");

    if !row_types.contains_key(row_name) {
        return Err(LpParseError::parse_error(line_num, format!("RANGES reference to undefined row: '{row_name}'")));
    }

    let value: f64 = value_str.parse().map_err(|_| LpParseError::invalid_number(value_str, line_num))?;

    range_values.insert(row_name, value);

    Ok(())
}

/// Mutable state for parsing the BOUNDS section.
#[derive(Default)]
pub(super) struct BoundsState<'input> {
    pub(super) accumulators: HashMap<&'input str, BoundAccumulator>,
    pub(super) order: Vec<&'input str>,
    seen: HashSet<&'input str>,
    pub(super) binary_vars: Vec<&'input str>,
    pub(super) semi_continuous_vars: Vec<&'input str>,
}

impl<'input> BoundsState<'input> {
    /// Parse a single BOUNDS data line.
    ///
    /// Only the first BOUNDS vector is used; subsequent vectors with different
    /// labels are silently skipped per the CPLEX spec. Duplicate lower or upper
    /// bounds on the same variable are rejected.
    pub(super) fn parse_line(
        &mut self,
        line: &'input str,
        line_num: usize,
        integer_vars: &mut Vec<&'input str>,
        integer_vars_set: &mut HashSet<&'input str>,
        first_vector_label: &mut Option<&'input str>,
    ) -> LpResult<()> {
        debug_assert!(!line.is_empty(), "BoundsState::parse_line called with empty line");
        debug_assert!(line_num > 0, "line_num must be 1-based");

        let raw_fields: Vec<&'input str> = line.split_whitespace().collect();
        let fields = strip_dollar_comments(&raw_fields);
        if fields.len() < 3 {
            return Err(LpParseError::parse_error(line_num, format!("BOUNDS line requires at least 3 fields, got {}", fields.len())));
        }

        let bound_type = fields[0];
        let label = fields[1];
        let var_name = fields[2];

        match *first_vector_label {
            None => *first_vector_label = Some(label),
            Some(first) if first != label => return Ok(()),
            _ => {}
        }

        // Track bound order
        if self.seen.insert(var_name) {
            self.order.push(var_name);
        }

        self.apply_bound(bound_type, var_name, &fields, line_num, integer_vars, integer_vars_set)
    }

    /// Apply a single bound directive to the accumulator for `var_name`.
    fn apply_bound(
        &mut self,
        bound_type: &str,
        var_name: &'input str,
        fields: &[&'input str],
        line_num: usize,
        integer_vars: &mut Vec<&'input str>,
        integer_vars_set: &mut HashSet<&'input str>,
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
                let value = parse_bound_value(fields.get(3), line_num, bound_type)?;
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
                let value = parse_bound_value(fields.get(3), line_num, bound_type)?;
                accumulator.upper = Some(value);
                if upper == "UI" && integer_vars_set.insert(var_name) {
                    integer_vars.push(var_name);
                }
            }
            "FX" => {
                if accumulator.fixed.is_some() {
                    return Err(LpParseError::invalid_bounds(var_name, format!("duplicate fixed bound at line {line_num}")));
                }
                let value = parse_bound_value(fields.get(3), line_num, bound_type)?;
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
            "SC" | "SI" => {
                let value = parse_bound_value(fields.get(3), line_num, bound_type)?;
                accumulator.upper = Some(value);
                self.semi_continuous_vars.push(var_name);
                if upper == "SI" && integer_vars_set.insert(var_name) {
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
fn parse_bound_value(field: Option<&&str>, line_num: usize, bound_type: &str) -> LpResult<f64> {
    debug_assert!(line_num > 0, "line_num must be 1-based");
    debug_assert!(!bound_type.is_empty(), "parse_bound_value called with empty bound_type");

    let value_str = field.ok_or_else(|| LpParseError::parse_error(line_num, format!("Bound type '{bound_type}' requires a value")))?;

    value_str.parse().map_err(|_| LpParseError::invalid_number(*value_str, line_num))
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

    // Check if this is an SOS set header line (e.g., "S1" or "S2")
    let upper = trimmed.to_ascii_uppercase();
    if upper.starts_with("S1") || upper.starts_with("S2") {
        // Flush previous SOS constraint
        flush_sos_constraint(sos_constraints, current_name, current_type, current_weights);

        let sos_type = if upper.starts_with("S1") { SOSType::S1 } else { SOSType::S2 };

        // The rest of the header might contain a name
        let fields: Vec<&str> = trimmed.split_whitespace().collect();
        let name = if fields.len() > 1 { fields[1] } else { "" };

        *current_type = Some(sos_type);
        *current_name = Some(if name.is_empty() {
            // Use a default name extracted from the trimmed string
            fields[0]
        } else {
            // Use the name portion from the original line
            let name_start = line.find(name).expect("SOS name from split_whitespace must exist in original line");
            &line[name_start..name_start + name.len()]
        });

        return Ok(());
    }

    // SOS weight entry: var_name weight
    let fields: Vec<&'input str> = line.split_whitespace().collect();
    if fields.len() >= 2 {
        let var_name = fields[0];
        let weight: f64 = fields[1].parse().map_err(|_| LpParseError::invalid_number(fields[1], line_num))?;
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
    let had_pending = current_name.is_some() || current_type.is_some();

    if let (Some(name), Some(sos_type)) = (*current_name, *current_type) {
        if !current_weights.is_empty() {
            sos_constraints.push(RawConstraint::SOS {
                name: Cow::Borrowed(name),
                sos_type,
                weights: std::mem::take(current_weights),
                byte_offset: None,
            });
        }
        *current_name = None;
        *current_type = None;
    }

    debug_assert!(current_name.is_none(), "current_name must be None after flush");
    debug_assert!(current_type.is_none() || !had_pending, "current_type must be None after flushing a pending constraint");
}
