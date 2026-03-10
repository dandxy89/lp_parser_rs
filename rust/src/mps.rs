//! MPS file format parser.
//!
//! Parses MPS (Mathematical Programming System) files into the same
//! [`ParseResult`] used by the LP grammar, enabling seamless integration
//! with `LpProblem::from_parse_result`.
//!

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};

use crate::error::{LpParseError, LpResult};
use crate::lexer::{ParseResult, RawCoefficient, RawConstraint, RawObjective};
use crate::model::{ComparisonOp, SOSType, Sense, VariableType};

/// MPS section currently being parsed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MpsSection {
    Name,
    ObjSense,
    Rows,
    Columns,
    Rhs,
    Ranges,
    Bounds,
    Sos,
    Unsupported,
}

/// Row type from the ROWS section.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RowType {
    /// Free row (objective function).
    N,
    /// Less-than-or-equal constraint.
    L,
    /// Greater-than-or-equal constraint.
    G,
    /// Equality constraint.
    E,
}

/// Accumulated bound state for a single variable.
#[derive(Debug, Default)]
struct BoundAccumulator {
    lower: Option<f64>,
    upper: Option<f64>,
    fixed: Option<f64>,
    free: bool,
    binary: bool,
}

/// Strip `$` inline comments from a field list.
///
/// Per the CPLEX MPS spec, if Field 3 or Field 5 starts with `$`, the
/// remainder of the line is a comment. We check all fields from index 0
/// onward for simplicity — a `$`-prefixed field truncates everything after.
fn strip_dollar_comments<'a>(fields: &[&'a str]) -> Vec<&'a str> {
    debug_assert!(!fields.is_empty(), "strip_dollar_comments called with empty fields");

    let mut result = Vec::with_capacity(fields.len());
    for &field in fields {
        if field.starts_with('$') {
            break;
        }
        result.push(field);
    }

    debug_assert!(result.len() <= fields.len(), "result cannot exceed input length");
    result
}

/// Accumulated mutable state for the MPS parser.
///
/// Bundles all section-level state into a single struct so that helper
/// functions can accept `&mut MpsParseState` instead of many individual
/// mutable references.
struct MpsParseState<'input> {
    section: Option<MpsSection>,
    _problem_name: Option<&'input str>,
    sense: Sense,

    // ROWS section data
    objective_rows: Vec<&'input str>,
    row_types: HashMap<&'input str, RowType>,
    row_order: Vec<&'input str>,

    // COLUMNS section state
    columns: ColumnsState<'input>,

    // RHS section data
    rhs_values: HashMap<&'input str, f64>,
    rhs_vector_label: Option<&'input str>,

    // RANGES section data
    range_values: HashMap<&'input str, f64>,
    ranges_vector_label: Option<&'input str>,

    // BOUNDS section state
    bounds_state: BoundsState<'input>,
    bounds_vector_label: Option<&'input str>,

    // SOS section data
    sos_constraints: Vec<RawConstraint<'input>>,
    current_sos_name: Option<&'input str>,
    current_sos_type: Option<SOSType>,
    current_sos_weights: Vec<RawCoefficient<'input>>,

    has_rows: bool,
    has_columns: bool,
}

impl<'input> MpsParseState<'input> {
    fn new() -> Self {
        Self {
            section: None,
            _problem_name: None,
            sense: Sense::Minimize,
            objective_rows: Vec::new(),
            row_types: HashMap::new(),
            row_order: Vec::new(),
            columns: ColumnsState::default(),
            rhs_values: HashMap::new(),
            rhs_vector_label: None,
            range_values: HashMap::new(),
            ranges_vector_label: None,
            bounds_state: BoundsState::default(),
            bounds_vector_label: None,
            sos_constraints: Vec::new(),
            current_sos_name: None,
            current_sos_type: None,
            current_sos_weights: Vec::new(),
            has_rows: false,
            has_columns: false,
        }
    }

    /// Process an MPS section header line, updating parser state.
    ///
    /// Returns `Ok(true)` when ENDATA is reached, signalling the caller to stop.
    fn process_section_header(&mut self, line: &'input str, line_num: usize) -> LpResult<bool> {
        debug_assert!(!line.is_empty(), "process_section_header called with empty line");
        debug_assert!(line_num > 0, "line_num must be 1-based");

        let header = line.split_whitespace().next().unwrap_or("");
        match header.to_ascii_uppercase().as_str() {
            "NAME" => {
                self.section = Some(MpsSection::Name);
                // Extract problem name from remainder of line
                let rest = line[4..].trim();
                if !rest.is_empty() {
                    self._problem_name = Some(rest);
                }
            }
            "OBJSENSE" => {
                self.section = Some(MpsSection::ObjSense);
            }
            "ROWS" => {
                self.section = Some(MpsSection::Rows);
                self.has_rows = true;
            }
            "COLUMNS" => {
                self.section = Some(MpsSection::Columns);
                self.has_columns = true;
            }
            "RHS" => {
                self.section = Some(MpsSection::Rhs);
            }
            "RANGES" => {
                self.section = Some(MpsSection::Ranges);
            }
            "BOUNDS" => {
                self.section = Some(MpsSection::Bounds);
            }
            "SOS" => {
                self.section = Some(MpsSection::Sos);
            }
            "ENDATA" => {
                // Flush any pending SOS constraint
                flush_sos_constraint(
                    &mut self.sos_constraints,
                    &mut self.current_sos_name,
                    &mut self.current_sos_type,
                    &mut self.current_sos_weights,
                );
                return Ok(true);
            }
            "LAZYCONS" | "USERCUTS" | "QUADOBJ" | "QCMATRIX" | "QMATRIX" | "PWLOBJ" | "INDICATORS" | "GENCONS" | "SCENARIOS" => {
                log::warn!("Line {line_num}: unsupported section '{header}' will be skipped");
                self.section = Some(MpsSection::Unsupported);
            }
            _ => {
                return Err(LpParseError::parse_error(line_num, format!("Unknown section header: '{header}'")));
            }
        }
        Ok(false)
    }

    /// Dispatch a data line to the handler for the current section.
    fn dispatch_data_line(&mut self, line: &'input str, line_num: usize) -> LpResult<()> {
        debug_assert!(!line.is_empty(), "dispatch_data_line called with empty line");
        debug_assert!(line_num > 0, "line_num must be 1-based");

        let current_section = self.section.ok_or_else(|| LpParseError::parse_error(line_num, "Data line before any section header"))?;

        match current_section {
            MpsSection::Name => {
                // Some formats put the name on the next line
                if self._problem_name.is_none() {
                    let trimmed = line.trim();
                    if !trimmed.is_empty() {
                        self._problem_name = Some(trimmed);
                    }
                }
            }
            MpsSection::ObjSense => {
                let trimmed = line.trim();
                match trimmed.to_ascii_uppercase().as_str() {
                    "MIN" | "MINIMIZE" => self.sense = Sense::Minimize,
                    "MAX" | "MAXIMIZE" => self.sense = Sense::Maximize,
                    _ => {
                        return Err(LpParseError::parse_error(line_num, format!("Invalid OBJSENSE value: '{trimmed}'")));
                    }
                }
            }
            MpsSection::Rows => {
                parse_rows_line(line, line_num, &mut self.objective_rows, &mut self.row_types, &mut self.row_order)?;
            }
            MpsSection::Columns => {
                self.columns.parse_line(line, line_num, &self.row_types, &self.objective_rows)?;
            }
            MpsSection::Rhs => {
                parse_rhs_line(line, line_num, &self.row_types, &self.objective_rows, &mut self.rhs_values, &mut self.rhs_vector_label)?;
            }
            MpsSection::Ranges => {
                parse_ranges_line(line, line_num, &self.row_types, &mut self.range_values, &mut self.ranges_vector_label)?;
            }
            MpsSection::Bounds => {
                self.bounds_state.parse_line(
                    line,
                    line_num,
                    &mut self.columns.integer_vars,
                    &mut self.columns.integer_vars_set,
                    &mut self.bounds_vector_label,
                )?;
            }
            MpsSection::Unsupported => {
                // Skip data lines in unsupported sections
            }
            MpsSection::Sos => {
                parse_sos_line(
                    line,
                    line_num,
                    &mut self.sos_constraints,
                    &mut self.current_sos_name,
                    &mut self.current_sos_type,
                    &mut self.current_sos_weights,
                )?;
            }
        }
        Ok(())
    }

    /// Validate required sections and build the final [`ParseResult`].
    fn build_result(mut self) -> LpResult<ParseResult<'input>> {
        debug_assert!(self.has_rows || !self.has_columns, "COLUMNS without ROWS is inconsistent state");
        debug_assert!(!self.columns.in_integer_block, "unclosed INTORG/INTEND block at end of parse");

        if !self.has_rows {
            return Err(LpParseError::missing_section("ROWS"));
        }
        if !self.has_columns {
            return Err(LpParseError::missing_section("COLUMNS"));
        }

        // Warn about objective constant (RHS on N-row)
        for &obj_row in &self.objective_rows {
            if let Some(&value) = self.rhs_values.get(obj_row) {
                log::warn!(
                    "RHS value {value} on objective row '{obj_row}' represents an objective \
                     constant, which is not supported by the model and will be ignored"
                );
            }
        }

        let objectives = build_objectives(&self.objective_rows, &self.columns.coefficients, &self.columns.column_order);
        let constraints = build_constraints(
            &self.row_types,
            &self.row_order,
            &self.columns.coefficients,
            &self.columns.column_order,
            &self.rhs_values,
            &self.range_values,
        );
        let bounds = build_bounds(
            &self.bounds_state.accumulators,
            &self.bounds_state.order,
            &self.columns.column_order,
            &self.columns.integer_vars_set,
        );

        // Deduplicate variable lists
        let mut integer_seen: HashSet<&str> = HashSet::new();
        self.columns.integer_vars.retain(|v| integer_seen.insert(v));

        let mut binary_seen: HashSet<&str> = HashSet::new();
        self.bounds_state.binary_vars.retain(|v| binary_seen.insert(v));

        let mut semi_continuous_seen: HashSet<&str> = HashSet::new();
        self.bounds_state.semi_continuous_vars.retain(|v| semi_continuous_seen.insert(v));

        Ok(ParseResult {
            sense: self.sense,
            objectives,
            constraints,
            bounds,
            generals: Vec::new(),
            integers: self.columns.integer_vars,
            binaries: self.bounds_state.binary_vars,
            semi_continuous: self.bounds_state.semi_continuous_vars,
            sos: self.sos_constraints,
        })
    }
}

/// Parse an MPS-format string into a [`ParseResult`].
///
/// # Errors
///
/// Returns an error for malformed MPS content including missing required
/// sections, invalid row/bound types, number parse failures, and references
/// to undefined rows.
pub fn parse_mps<'input>(input: &'input str) -> LpResult<ParseResult<'input>> {
    debug_assert!(!input.is_empty(), "parse_mps called with empty input");
    debug_assert!(input.contains('\n'), "parse_mps input must contain at least one newline (multi-line MPS expected)");

    let mut state = MpsParseState::new();

    for (line_idx, line) in input.lines().enumerate() {
        let line_num = line_idx + 1;

        // Skip blank lines and comment lines (start with '*' at column 0)
        if line.trim().is_empty() || line.starts_with('*') {
            continue;
        }

        // Determine if this is a section header or data line
        let first_char = line.as_bytes().first().copied();
        let is_section_header = first_char.is_some_and(|c| !c.is_ascii_whitespace());

        if is_section_header {
            if state.process_section_header(line, line_num)? {
                break;
            }
            continue;
        }

        state.dispatch_data_line(line, line_num)?;
    }

    state.build_result()
}

/// Extract the problem name from MPS input (the NAME section line).
pub fn extract_mps_name(input: &str) -> Option<String> {
    debug_assert!(!input.is_empty(), "extract_mps_name called with empty input");
    debug_assert!(input.is_ascii() || input.is_char_boundary(0), "input must be valid UTF-8");

    for line in input.lines() {
        if line.trim().is_empty() || line.starts_with('*') {
            continue;
        }
        let first_char = line.as_bytes().first().copied();
        if first_char.is_some_and(|c| !c.is_ascii_whitespace()) {
            let header = line.split_whitespace().next().unwrap_or("");
            if header.eq_ignore_ascii_case("NAME") {
                let rest = line[4..].trim();
                if !rest.is_empty() {
                    return Some(rest.to_string());
                }
            }
        }
    }
    None
}

/// Parse a single ROWS data line.
fn parse_rows_line<'input>(
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
struct ColumnsState<'input> {
    coefficients: HashMap<(&'input str, &'input str), f64>,
    column_order: Vec<&'input str>,
    column_seen: HashSet<&'input str>,
    in_integer_block: bool,
    integer_vars: Vec<&'input str>,
    integer_vars_set: HashSet<&'input str>,
}

impl<'input> ColumnsState<'input> {
    /// Parse a single COLUMNS data line.
    fn parse_line(
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

        // Accumulate coefficient (additive — MPS allows split entries)
        *self.coefficients.entry((var_name, row_name)).or_insert(0.0) += value;

        Ok(())
    }
}

/// Parse a single RHS data line.
///
/// Only the first RHS vector is used; subsequent vectors with different labels
/// are silently skipped per the CPLEX spec.
fn parse_rhs_line<'input>(
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
fn parse_ranges_line<'input>(
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
struct BoundsState<'input> {
    accumulators: HashMap<&'input str, BoundAccumulator>,
    order: Vec<&'input str>,
    seen: HashSet<&'input str>,
    binary_vars: Vec<&'input str>,
    semi_continuous_vars: Vec<&'input str>,
}

impl<'input> BoundsState<'input> {
    /// Parse a single BOUNDS data line.
    ///
    /// Only the first BOUNDS vector is used; subsequent vectors with different
    /// labels are silently skipped per the CPLEX spec. Duplicate lower or upper
    /// bounds on the same variable are rejected.
    fn parse_line(
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
fn parse_sos_line<'input>(
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
            let name_start = line.find(name).unwrap_or(0);
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
fn flush_sos_constraint<'input>(
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

/// Build objective(s) from the parsed MPS data.
///
/// Produces one `RawObjective` per N-row, supporting multi-objective MPS files.
fn build_objectives<'input>(
    objective_rows: &[&'input str],
    coefficients: &HashMap<(&'input str, &'input str), f64>,
    column_order: &[&'input str],
) -> Vec<RawObjective<'input>> {
    debug_assert!(objective_rows.iter().all(|r| !r.is_empty()), "objective_rows must not contain empty row names");

    if objective_rows.is_empty() {
        return vec![RawObjective { name: Cow::Borrowed("__obj__"), coefficients: Vec::new(), byte_offset: None }];
    }

    let mut objectives = Vec::with_capacity(objective_rows.len());
    for &obj_row in objective_rows {
        let mut objective_coefficients = Vec::new();
        for &var_name in column_order {
            if let Some(&value) = coefficients.get(&(var_name, obj_row)) {
                objective_coefficients.push(RawCoefficient { name: var_name, value });
            }
        }
        objectives.push(RawObjective { name: Cow::Borrowed(obj_row), coefficients: objective_coefficients, byte_offset: None });
    }

    debug_assert!(objectives.len() == objective_rows.len(), "should produce one objective per N-row");
    objectives
}

/// Build constraints from the parsed MPS data, including RANGES expansion.
///
/// For rows with a RANGES entry, the single constraint is expanded into two
/// constraints to represent both bounds:
/// - **G row**: original `>= rhs`, plus `<= rhs + |range|`
/// - **L row**: original `<= rhs`, plus `>= rhs - |range|`
/// - **E row, positive range**: `>= rhs` and `<= rhs + range`
/// - **E row, negative range**: `<= rhs` and `>= rhs + range`
fn build_constraints<'input>(
    row_types: &HashMap<&'input str, RowType>,
    row_order: &[&'input str],
    coefficients: &HashMap<(&'input str, &'input str), f64>,
    column_order: &[&'input str],
    rhs_values: &HashMap<&'input str, f64>,
    range_values: &HashMap<&'input str, f64>,
) -> Vec<RawConstraint<'input>> {
    debug_assert!(row_order.iter().all(|r| row_types.contains_key(r)), "every row in row_order must have a type in row_types");

    let mut constraints = Vec::with_capacity(row_order.len());

    for &row_name in row_order {
        let row_type = row_types.get(row_name).copied().unwrap_or(RowType::E);
        debug_assert!(row_type != RowType::N, "N-type rows should not appear in row_order");

        let operator = match row_type {
            RowType::L => ComparisonOp::LTE,
            RowType::G => ComparisonOp::GTE,
            RowType::E => ComparisonOp::EQ,
            RowType::N => unreachable!("N-type rows filtered above"),
        };

        let mut row_coeffs = Vec::new();
        for &var_name in column_order {
            if let Some(&value) = coefficients.get(&(var_name, row_name)) {
                row_coeffs.push(RawCoefficient { name: var_name, value });
            }
        }

        let rhs = rhs_values.get(row_name).copied().unwrap_or(0.0);

        // Check for RANGES entry on this row
        if let Some(&range_val) = range_values.get(row_name) {
            // Expand into two constraints based on row type and range value
            let (lower_rhs, upper_rhs) = match row_type {
                RowType::G => (rhs, rhs + range_val.abs()),
                RowType::L => (rhs - range_val.abs(), rhs),
                RowType::E => {
                    if range_val >= 0.0 {
                        (rhs, rhs + range_val)
                    } else {
                        (rhs + range_val, rhs)
                    }
                }
                RowType::N => unreachable!("N-type rows filtered above"),
            };

            // Emit the lower-bound constraint (GTE)
            constraints.push(RawConstraint::Standard {
                name: Cow::Borrowed(row_name),
                coefficients: row_coeffs.clone(),
                operator: ComparisonOp::GTE,
                rhs: lower_rhs,
                byte_offset: None,
            });

            // Emit the upper-bound constraint (LTE)
            constraints.push(RawConstraint::Standard {
                name: Cow::Owned(format!("{row_name}_rng")),
                coefficients: row_coeffs,
                operator: ComparisonOp::LTE,
                rhs: upper_rhs,
                byte_offset: None,
            });
        } else {
            constraints.push(RawConstraint::Standard {
                name: Cow::Borrowed(row_name),
                coefficients: row_coeffs,
                operator,
                rhs,
                byte_offset: None,
            });
        }
    }

    debug_assert!(constraints.len() >= row_order.len(), "constraints cannot be fewer than rows (ranges add extra)");
    constraints
}

/// Build bounds from accumulated bound data.
///
/// Applies MPS default bounds: variables without explicit BOUNDS entries get
/// `[0, +inf]`. Integer variables (INTORG/INTEND) without explicit bounds get
/// `[0, 1]`. When an UP bound is negative with no explicit LO, the lower
/// bound is set to `-inf` per CPLEX spec.
fn build_bounds<'input>(
    bound_accumulators: &HashMap<&'input str, BoundAccumulator>,
    bound_order: &[&'input str],
    column_order: &[&'input str],
    integer_vars: &HashSet<&'input str>,
) -> Vec<(&'input str, VariableType)> {
    debug_assert!(bound_order.iter().all(|v| bound_accumulators.contains_key(v)), "every variable in bound_order must have an accumulator");

    let mut bounds = Vec::with_capacity(bound_order.len() + column_order.len());

    // First, emit bounds for variables with explicit BOUNDS entries
    let mut has_explicit_bounds: HashSet<&str> = HashSet::with_capacity(bound_order.len());

    for &var_name in bound_order {
        has_explicit_bounds.insert(var_name);

        let Some(accumulator) = bound_accumulators.get(var_name) else {
            continue;
        };

        let var_type = if accumulator.binary {
            VariableType::Binary
        } else if accumulator.free {
            VariableType::Free
        } else if let Some(fixed) = accumulator.fixed {
            VariableType::DoubleBound(fixed, fixed)
        } else {
            match (accumulator.lower, accumulator.upper) {
                (Some(lo), Some(hi)) => VariableType::DoubleBound(lo, hi),
                (Some(lo), None) => VariableType::LowerBound(lo),
                (None, Some(hi)) => {
                    // CPLEX spec: UP < 0 with no LO implies lower = -inf
                    if hi < 0.0 { VariableType::DoubleBound(f64::NEG_INFINITY, hi) } else { VariableType::UpperBound(hi) }
                }
                (None, None) => continue, // No bounds to emit
            }
        };

        bounds.push((var_name, var_type));
    }

    // Apply MPS default bounds for variables without explicit BOUNDS entries
    for &var_name in column_order {
        if has_explicit_bounds.contains(var_name) {
            continue;
        }

        if integer_vars.contains(var_name) {
            // Integer variables default to [0, 1]
            bounds.push((var_name, VariableType::DoubleBound(0.0, 1.0)));
        } else {
            // Continuous variables default to [0, +inf]
            bounds.push((var_name, VariableType::LowerBound(0.0)));
        }
    }

    debug_assert!(
        !bounds.is_empty() || (bound_order.is_empty() && column_order.is_empty()),
        "bounds should be non-empty when there are variables"
    );
    bounds
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_minimal_mps() {
        let input = "\
NAME        test
ROWS
 N  obj
 L  c1
COLUMNS
    x1        obj       1
    x1        c1        2
RHS
    RHS_V     c1        10
ENDATA
";
        let result = parse_mps(input).unwrap();
        assert_eq!(result.sense, Sense::Minimize);
        assert_eq!(result.objectives.len(), 1);
        assert_eq!(result.objectives[0].coefficients.len(), 1);
        assert_eq!(result.objectives[0].coefficients[0].name, "x1");
        assert_eq!(result.objectives[0].coefficients[0].value, 1.0);
        assert_eq!(result.constraints.len(), 1);
        if let RawConstraint::Standard { name, operator, rhs, .. } = &result.constraints[0] {
            assert_eq!(name.as_ref(), "c1");
            assert_eq!(*operator, ComparisonOp::LTE);
            assert_eq!(*rhs, 10.0);
        } else {
            panic!("Expected Standard constraint");
        }
    }

    #[test]
    fn test_objsense_max() {
        let input = "\
NAME        test
OBJSENSE
  MAX
ROWS
 N  obj
 L  c1
COLUMNS
    x1        obj       1
    x1        c1        1
RHS
    RHS_V     c1        5
ENDATA
";
        let result = parse_mps(input).unwrap();
        assert_eq!(result.sense, Sense::Maximize);
    }

    #[test]
    fn test_integer_markers() {
        let input = "\
NAME        test
ROWS
 N  obj
 L  c1
COLUMNS
    MARK0000  'MARKER'                 'INTORG'
    x1        obj       1
    x1        c1        2
    MARK0001  'MARKER'                 'INTEND'
    x2        obj       3
    x2        c1        4
ENDATA
";
        let result = parse_mps(input).unwrap();
        assert!(result.integers.contains(&"x1"));
        assert!(!result.integers.contains(&"x2"));
    }

    #[test]
    fn test_bound_types() {
        let input = "\
NAME        test
ROWS
 N  obj
 L  c1
COLUMNS
    x1        obj       1
    x1        c1        1
    x2        obj       1
    x2        c1        1
    x3        obj       1
    x3        c1        1
    x4        obj       1
    x4        c1        1
RHS
    RHS_V     c1        10
BOUNDS
 FR BOUND     x1
 LO BOUND     x2        5
 UP BOUND     x2        15
 BV BOUND     x3
 FX BOUND     x4        7
ENDATA
";
        let result = parse_mps(input).unwrap();

        // x1 = Free
        assert!(result.bounds.iter().any(|(n, t)| *n == "x1" && *t == VariableType::Free));
        // x2 = DoubleBound(5, 15)
        assert!(result.bounds.iter().any(|(n, t)| *n == "x2" && *t == VariableType::DoubleBound(5.0, 15.0)));
        // x3 = Binary
        assert!(result.bounds.iter().any(|(n, t)| *n == "x3" && *t == VariableType::Binary));
        assert!(result.binaries.contains(&"x3"));
        // x4 = Fixed = DoubleBound(7, 7)
        assert!(result.bounds.iter().any(|(n, t)| *n == "x4" && *t == VariableType::DoubleBound(7.0, 7.0)));
    }

    #[test]
    fn test_multiple_constraint_types() {
        let input = "\
NAME        test
ROWS
 N  obj
 L  c1
 G  c2
 E  c3
COLUMNS
    x1        obj       1
    x1        c1        1
    x1        c2        2
    x1        c3        3
RHS
    RHS_V     c1        10
    RHS_V     c2        5
    RHS_V     c3        7
ENDATA
";
        let result = parse_mps(input).unwrap();
        assert_eq!(result.constraints.len(), 3);

        let ops: Vec<ComparisonOp> = result
            .constraints
            .iter()
            .filter_map(|c| if let RawConstraint::Standard { operator, .. } = c { Some(*operator) } else { None })
            .collect();
        assert_eq!(ops, vec![ComparisonOp::LTE, ComparisonOp::GTE, ComparisonOp::EQ]);
    }

    #[test]
    fn test_missing_rows_section() {
        let input = "\
NAME        test
COLUMNS
    x1        obj       1
ENDATA
";
        let result = parse_mps(input);
        assert!(result.is_err());
    }

    #[test]
    fn test_missing_columns_section() {
        let input = "\
NAME        test
ROWS
 N  obj
ENDATA
";
        let result = parse_mps(input);
        assert!(result.is_err());
    }

    #[test]
    fn test_two_entries_per_line() {
        let input = "\
NAME        test
ROWS
 N  obj
 L  c1
 L  c2
COLUMNS
    x1        c1        1          c2        2
RHS
    RHS_V     c1        10         c2        20
ENDATA
";
        let result = parse_mps(input).unwrap();
        assert_eq!(result.constraints.len(), 2);

        if let RawConstraint::Standard { rhs, .. } = &result.constraints[0] {
            assert_eq!(*rhs, 10.0);
        }
        if let RawConstraint::Standard { rhs, .. } = &result.constraints[1] {
            assert_eq!(*rhs, 20.0);
        }
    }

    #[test]
    fn test_extract_mps_name() {
        let input = "NAME        my_problem\nROWS\n N  obj\n";
        assert_eq!(extract_mps_name(input), Some("my_problem".to_string()));
    }

    #[test]
    fn test_comment_lines_skipped() {
        let input = "\
* This is a comment
NAME        test
* Another comment
ROWS
 N  obj
 L  c1
COLUMNS
    x1        obj       1
    x1        c1        1
ENDATA
";
        let result = parse_mps(input).unwrap();
        assert_eq!(result.objectives.len(), 1);
    }

    #[test]
    fn test_blank_lines_skipped() {
        let input = "\
NAME        test

ROWS
 N  obj
 L  c1

COLUMNS
    x1        obj       1
    x1        c1        1

ENDATA
";
        let result = parse_mps(input).unwrap();
        assert_eq!(result.constraints.len(), 1);
    }

    #[test]
    fn test_semi_continuous_bounds() {
        let input = "\
NAME        test
ROWS
 N  obj
 L  c1
COLUMNS
    x1        obj       1
    x1        c1        1
    x2        obj       1
    x2        c1        1
BOUNDS
 SC BOUND     x1        100
 SI BOUND     x2        200
ENDATA
";
        let result = parse_mps(input).unwrap();
        assert!(result.semi_continuous.contains(&"x1"));
        assert!(result.semi_continuous.contains(&"x2"));
        assert!(!result.integers.contains(&"x1"));
        assert!(result.integers.contains(&"x2"));
    }

    #[test]
    fn test_default_rhs_zero() {
        let input = "\
NAME        test
ROWS
 N  obj
 L  c1
COLUMNS
    x1        obj       1
    x1        c1        1
ENDATA
";
        let result = parse_mps(input).unwrap();
        if let RawConstraint::Standard { rhs, .. } = &result.constraints[0] {
            assert_eq!(*rhs, 0.0);
        }
    }

    // --- New spec-compliance tests ---

    #[test]
    fn test_default_bounds_zero_to_inf() {
        let input = "\
NAME        test
ROWS
 N  obj
 L  c1
COLUMNS
    x1        obj       1
    x1        c1        1
    x2        obj       2
    x2        c1        3
ENDATA
";
        let result = parse_mps(input).unwrap();

        // Variables with no BOUNDS entry should default to LowerBound(0.0)
        assert!(
            result.bounds.iter().any(|(n, t)| *n == "x1" && *t == VariableType::LowerBound(0.0)),
            "x1 should have default LowerBound(0.0), got: {:?}",
            result.bounds.iter().find(|(n, _)| *n == "x1")
        );
        assert!(
            result.bounds.iter().any(|(n, t)| *n == "x2" && *t == VariableType::LowerBound(0.0)),
            "x2 should have default LowerBound(0.0), got: {:?}",
            result.bounds.iter().find(|(n, _)| *n == "x2")
        );
    }

    #[test]
    fn test_integer_default_bounds_zero_to_one() {
        let input = "\
NAME        test
ROWS
 N  obj
 L  c1
COLUMNS
    MARK0000  'MARKER'                 'INTORG'
    y1        obj       1
    y1        c1        2
    MARK0001  'MARKER'                 'INTEND'
ENDATA
";
        let result = parse_mps(input).unwrap();

        // INTORG/INTEND variable with no BOUNDS should default to [0, 1]
        assert!(
            result.bounds.iter().any(|(n, t)| *n == "y1" && *t == VariableType::DoubleBound(0.0, 1.0)),
            "y1 should have default DoubleBound(0.0, 1.0), got: {:?}",
            result.bounds.iter().find(|(n, _)| *n == "y1")
        );
    }

    #[test]
    fn test_negative_upper_implies_mi() {
        let input = "\
NAME        test
ROWS
 N  obj
 L  c1
COLUMNS
    x1        obj       1
    x1        c1        1
BOUNDS
 UP BOUND     x1        -5
ENDATA
";
        let result = parse_mps(input).unwrap();

        // UP < 0 with no LO should produce DoubleBound(-inf, -5)
        assert!(
            result.bounds.iter().any(|(n, t)| *n == "x1" && *t == VariableType::DoubleBound(f64::NEG_INFINITY, -5.0)),
            "x1 should have DoubleBound(-inf, -5.0), got: {:?}",
            result.bounds.iter().find(|(n, _)| *n == "x1")
        );
    }

    #[test]
    fn test_dollar_inline_comment() {
        let input = "\
NAME        test
ROWS
 N  obj
 L  c1
COLUMNS
    x1        obj       1          $ this is a comment
    x1        c1        2
RHS
    RHS_V     c1        10         $ comment here too
ENDATA
";
        let result = parse_mps(input).unwrap();
        assert_eq!(result.objectives[0].coefficients.len(), 1);
        assert_eq!(result.objectives[0].coefficients[0].value, 1.0);
        assert_eq!(result.constraints.len(), 1);
        if let RawConstraint::Standard { rhs, .. } = &result.constraints[0] {
            assert_eq!(*rhs, 10.0);
        }
    }

    #[test]
    fn test_multiple_n_rows() {
        let input = "\
NAME        test
ROWS
 N  obj1
 N  obj2
 L  c1
COLUMNS
    x1        obj1      1
    x1        obj2      2
    x1        c1        3
RHS
    RHS_V     c1        10
ENDATA
";
        let result = parse_mps(input).unwrap();

        // Two N-rows should produce two objectives
        assert_eq!(result.objectives.len(), 2);
        assert_eq!(result.objectives[0].name.as_ref(), "obj1");
        assert_eq!(result.objectives[0].coefficients[0].value, 1.0);
        assert_eq!(result.objectives[1].name.as_ref(), "obj2");
        assert_eq!(result.objectives[1].coefficients[0].value, 2.0);
    }

    #[test]
    fn test_ranges_section_g_row() {
        // G row with range r: lower = rhs, upper = rhs + |r|
        let input = "\
NAME        test
ROWS
 N  obj
 G  c1
COLUMNS
    x1        obj       1
    x1        c1        1
RHS
    RHS_V     c1        5
RANGES
    RNG_V     c1        10
ENDATA
";
        let result = parse_mps(input).unwrap();

        // Should expand into two constraints
        assert_eq!(result.constraints.len(), 2);

        // First: c1 >= 5 (the lower bound)
        if let RawConstraint::Standard { name, operator, rhs, .. } = &result.constraints[0] {
            assert_eq!(name.as_ref(), "c1");
            assert_eq!(*operator, ComparisonOp::GTE);
            assert_eq!(*rhs, 5.0);
        } else {
            panic!("Expected Standard constraint");
        }

        // Second: c1_rng <= 15 (the upper bound = rhs + |range|)
        if let RawConstraint::Standard { name, operator, rhs, .. } = &result.constraints[1] {
            assert_eq!(name.as_ref(), "c1_rng");
            assert_eq!(*operator, ComparisonOp::LTE);
            assert_eq!(*rhs, 15.0);
        } else {
            panic!("Expected Standard constraint");
        }
    }

    #[test]
    fn test_ranges_section_l_row() {
        // L row with range r: lower = rhs - |r|, upper = rhs
        let input = "\
NAME        test
ROWS
 N  obj
 L  c1
COLUMNS
    x1        obj       1
    x1        c1        1
RHS
    RHS_V     c1        20
RANGES
    RNG_V     c1        8
ENDATA
";
        let result = parse_mps(input).unwrap();

        assert_eq!(result.constraints.len(), 2);

        // First: c1 >= 12 (lower = rhs - |range|)
        if let RawConstraint::Standard { name, operator, rhs, .. } = &result.constraints[0] {
            assert_eq!(name.as_ref(), "c1");
            assert_eq!(*operator, ComparisonOp::GTE);
            assert_eq!(*rhs, 12.0);
        } else {
            panic!("Expected Standard constraint");
        }

        // Second: c1_rng <= 20 (upper = rhs)
        if let RawConstraint::Standard { name, operator, rhs, .. } = &result.constraints[1] {
            assert_eq!(name.as_ref(), "c1_rng");
            assert_eq!(*operator, ComparisonOp::LTE);
            assert_eq!(*rhs, 20.0);
        } else {
            panic!("Expected Standard constraint");
        }
    }

    #[test]
    fn test_ranges_section_e_row() {
        // E row, positive range: lower = rhs, upper = rhs + range
        let input = "\
NAME        test
ROWS
 N  obj
 E  c1
 E  c2
COLUMNS
    x1        obj       1
    x1        c1        1
    x1        c2        1
RHS
    RHS_V     c1        10
    RHS_V     c2        10
RANGES
    RNG_V     c1        5
    RNG_V     c2        -3
ENDATA
";
        let result = parse_mps(input).unwrap();

        // 2 rows * 2 constraints each = 4 constraints
        assert_eq!(result.constraints.len(), 4);

        // c1 (positive range 5): lower=10, upper=15
        if let RawConstraint::Standard { name, operator, rhs, .. } = &result.constraints[0] {
            assert_eq!(name.as_ref(), "c1");
            assert_eq!(*operator, ComparisonOp::GTE);
            assert_eq!(*rhs, 10.0);
        }
        if let RawConstraint::Standard { name, operator, rhs, .. } = &result.constraints[1] {
            assert_eq!(name.as_ref(), "c1_rng");
            assert_eq!(*operator, ComparisonOp::LTE);
            assert_eq!(*rhs, 15.0);
        }

        // c2 (negative range -3): lower=7 (rhs+range), upper=10 (rhs)
        if let RawConstraint::Standard { name, operator, rhs, .. } = &result.constraints[2] {
            assert_eq!(name.as_ref(), "c2");
            assert_eq!(*operator, ComparisonOp::GTE);
            assert_eq!(*rhs, 7.0);
        }
        if let RawConstraint::Standard { name, operator, rhs, .. } = &result.constraints[3] {
            assert_eq!(name.as_ref(), "c2_rng");
            assert_eq!(*operator, ComparisonOp::LTE);
            assert_eq!(*rhs, 10.0);
        }
    }

    #[test]
    fn test_objective_rhs_no_crash() {
        // RHS on N-row should not crash — it logs a warning but is otherwise ignored
        let input = "\
NAME        test
ROWS
 N  obj
 L  c1
COLUMNS
    x1        obj       1
    x1        c1        1
RHS
    RHS_V     obj       42
    RHS_V     c1        10
ENDATA
";
        let result = parse_mps(input).unwrap();
        assert_eq!(result.objectives.len(), 1);
        assert_eq!(result.constraints.len(), 1);
    }

    #[test]
    fn test_explicit_bounds_override_default() {
        // Variables with explicit bounds should not get default [0, +inf]
        let input = "\
NAME        test
ROWS
 N  obj
 L  c1
COLUMNS
    x1        obj       1
    x1        c1        1
    x2        obj       2
    x2        c1        3
BOUNDS
 FR BOUND     x1
ENDATA
";
        let result = parse_mps(input).unwrap();

        // x1 has explicit FR bound
        assert!(result.bounds.iter().any(|(n, t)| *n == "x1" && *t == VariableType::Free));
        // x2 has no explicit bounds — should get default LowerBound(0.0)
        assert!(result.bounds.iter().any(|(n, t)| *n == "x2" && *t == VariableType::LowerBound(0.0)));
    }

    #[test]
    fn test_negative_upper_with_explicit_lower() {
        // UP < 0 WITH explicit LO should NOT override to -inf
        let input = "\
NAME        test
ROWS
 N  obj
 L  c1
COLUMNS
    x1        obj       1
    x1        c1        1
BOUNDS
 LO BOUND     x1        -10
 UP BOUND     x1        -5
ENDATA
";
        let result = parse_mps(input).unwrap();

        assert!(
            result.bounds.iter().any(|(n, t)| *n == "x1" && *t == VariableType::DoubleBound(-10.0, -5.0)),
            "x1 should have DoubleBound(-10.0, -5.0), got: {:?}",
            result.bounds.iter().find(|(n, _)| *n == "x1")
        );
    }

    #[test]
    fn test_dollar_comment_in_bounds() {
        let input = "\
NAME        test
ROWS
 N  obj
 L  c1
COLUMNS
    x1        obj       1
    x1        c1        1
BOUNDS
 LO BOUND     x1        5    $ lower bound comment
 UP BOUND     x1        15   $ upper bound comment
ENDATA
";
        let result = parse_mps(input).unwrap();
        assert!(result.bounds.iter().any(|(n, t)| *n == "x1" && *t == VariableType::DoubleBound(5.0, 15.0)));
    }

    #[test]
    fn test_first_rhs_vector_only() {
        // Two RHS vectors — only the first (RHS1) should be used
        let input = "\
NAME        test
ROWS
 N  obj
 L  c1
 L  c2
COLUMNS
    x1        obj       1
    x1        c1        1
    x1        c2        1
RHS
    RHS1      c1        10
    RHS1      c2        20
    RHS2      c1        99
    RHS2      c2        99
ENDATA
";
        let result = parse_mps(input).unwrap();

        // c1 should have RHS=10 (from RHS1), not 99 (from RHS2)
        if let RawConstraint::Standard { name, rhs, .. } = &result.constraints[0] {
            assert_eq!(name.as_ref(), "c1");
            assert_eq!(*rhs, 10.0);
        } else {
            panic!("Expected Standard constraint");
        }

        // c2 should have RHS=20 (from RHS1), not 99 (from RHS2)
        if let RawConstraint::Standard { name, rhs, .. } = &result.constraints[1] {
            assert_eq!(name.as_ref(), "c2");
            assert_eq!(*rhs, 20.0);
        } else {
            panic!("Expected Standard constraint");
        }
    }

    #[test]
    fn test_first_bounds_vector_only() {
        // Two BOUNDS vectors — only the first (BND1) should be used
        let input = "\
NAME        test
ROWS
 N  obj
 L  c1
COLUMNS
    x1        obj       1
    x1        c1        1
BOUNDS
 UP BND1      x1        10
 UP BND2      x1        99
ENDATA
";
        let result = parse_mps(input).unwrap();

        // x1 should have UP=10 from BND1, not 99 from BND2
        assert!(
            result.bounds.iter().any(|(n, t)| *n == "x1" && *t == VariableType::UpperBound(10.0)),
            "x1 should have UpperBound(10.0), got: {:?}",
            result.bounds.iter().find(|(n, _)| *n == "x1")
        );
    }

    #[test]
    fn test_first_ranges_vector_only() {
        // Two RANGES vectors — only the first (RNG1) should be used
        let input = "\
NAME        test
ROWS
 N  obj
 G  c1
COLUMNS
    x1        obj       1
    x1        c1        1
RHS
    RHS_V     c1        5
RANGES
    RNG1      c1        10
    RNG2      c1        99
ENDATA
";
        let result = parse_mps(input).unwrap();

        // Range should expand using RNG1 value (10), not RNG2 (99)
        assert_eq!(result.constraints.len(), 2);
        if let RawConstraint::Standard { rhs, .. } = &result.constraints[1] {
            // upper = rhs + |range| = 5 + 10 = 15
            assert_eq!(*rhs, 15.0);
        } else {
            panic!("Expected Standard constraint");
        }
    }

    #[test]
    fn test_duplicate_lower_bound_rejected() {
        let input = "\
NAME        test
ROWS
 N  obj
 L  c1
COLUMNS
    x1        obj       1
    x1        c1        1
BOUNDS
 LO BOUND     x1        5
 LO BOUND     x1        10
ENDATA
";
        let result = parse_mps(input);
        assert!(result.is_err(), "Duplicate LO bound should be rejected");
        let err = result.unwrap_err().to_string();
        assert!(err.contains("duplicate lower bound"), "Error should mention duplicate: {err}");
    }

    #[test]
    fn test_duplicate_upper_bound_rejected() {
        let input = "\
NAME        test
ROWS
 N  obj
 L  c1
COLUMNS
    x1        obj       1
    x1        c1        1
BOUNDS
 UP BOUND     x1        10
 UP BOUND     x1        20
ENDATA
";
        let result = parse_mps(input);
        assert!(result.is_err(), "Duplicate UP bound should be rejected");
        let err = result.unwrap_err().to_string();
        assert!(err.contains("duplicate upper bound"), "Error should mention duplicate: {err}");
    }

    #[test]
    fn test_duplicate_fixed_bound_rejected() {
        let input = "\
NAME        test
ROWS
 N  obj
 L  c1
COLUMNS
    x1        obj       1
    x1        c1        1
BOUNDS
 FX BOUND     x1        5
 FX BOUND     x1        10
ENDATA
";
        let result = parse_mps(input);
        assert!(result.is_err(), "Duplicate FX bound should be rejected");
    }

    #[test]
    fn test_n_row_extra_fields_accepted() {
        // Gurobi-style N-row with priority/weight/tolerance fields
        let input = "\
NAME        test
ROWS
 N  OBJ0 2 1 0 0
 L  c1
COLUMNS
    x1        OBJ0      1
    x1        c1        1
ENDATA
";
        let result = parse_mps(input).unwrap();
        assert_eq!(result.objectives.len(), 1);
        // The extra fields are ignored but parsing succeeds
        assert_eq!(result.objectives[0].name.as_ref(), "OBJ0");
    }

    #[test]
    fn test_unsupported_section_skipped() {
        let input = "\
NAME        test
ROWS
 N  obj
 L  c1
COLUMNS
    x1        obj       1
    x1        c1        1
QUADOBJ
    x1  x1  2.0
RHS
    RHS_V     c1        10
ENDATA
";
        let result = parse_mps(input).unwrap();
        assert_eq!(result.objectives.len(), 1);
        assert_eq!(result.constraints.len(), 1);
    }

    #[test]
    fn test_multiple_unsupported_sections_skipped() {
        let input = "\
NAME        test
ROWS
 N  obj
 L  c1
COLUMNS
    x1        obj       1
    x1        c1        1
INDICATORS
    IF c1 x1 1
PWLOBJ
    x1 3 0.0 0.0 1.0 1.0 2.0 3.0
GENCONS
    gc0: MIN x1 x1 x1
SCENARIOS
    scenario1
RHS
    RHS_V     c1        10
ENDATA
";
        let result = parse_mps(input).unwrap();
        assert_eq!(result.constraints.len(), 1);
    }
}
