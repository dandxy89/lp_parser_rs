use std::collections::{HashMap, HashSet};

use super::builders::{build_bounds, build_constraints, build_objectives};
use super::sections::{
    BoundsState, ColumnsState, flush_sos_constraint, parse_ranges_line, parse_rhs_line, parse_rows_line, parse_sos_line,
};
use super::{MpsSection, RawCoefficient, RowType, SOSType};
use crate::error::{LpParseError, LpResult};
use crate::lexer::{ParseResult, RawConstraint};
use crate::model::Sense;

/// Accumulated mutable state for the MPS parser.
///
/// Bundles all section-level state into a single struct so that helper
/// functions can accept `&mut MpsParseState` instead of many individual
/// mutable references.
pub(super) struct MpsParseState<'input> {
    section: Option<MpsSection>,
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
                debug_assert!(line.len() >= 4, "NAME header must be at least 4 chars");
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
            MpsSection::Name => {}
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
                debug_assert!(line.len() >= 4, "NAME header must be at least 4 chars");
                let rest = line[4..].trim();
                if !rest.is_empty() {
                    return Some(rest.to_string());
                }
            }
        }
    }
    None
}
