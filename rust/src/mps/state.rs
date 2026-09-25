use rustc_hash::{FxHashMap, FxHashSet};

use super::builders::{build_bounds, build_constraints, build_objectives};
use super::sections::{
    BoundsState, ColumnsState, flush_sos_constraint, parse_ranges_line, parse_rhs_line, parse_rows_line, parse_sos_line,
};
use super::{MpsSection, RawCoefficient, RowType, SOSType};
use crate::error::{LpParseError, LpResult};
use crate::lexer::{ParseResult, RawConstraint, RawQuadraticTerm};
use crate::model::{ConstraintClass, Sense};

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
    row_types: FxHashMap<&'input str, RowType>,
    row_order: Vec<&'input str>,
    /// Rows declared in `LAZYCONS` / `USERCUTS` rather than `ROWS`.
    row_classes: FxHashMap<&'input str, ConstraintClass>,

    // COLUMNS section state
    columns: ColumnsState<'input>,

    // RHS section data
    rhs_values: FxHashMap<&'input str, f64>,
    rhs_vector_label: Option<&'input str>,

    // RANGES section data
    range_values: FxHashMap<&'input str, f64>,
    ranges_vector_label: Option<&'input str>,

    // BOUNDS section state
    bounds_state: BoundsState<'input>,
    bounds_vector_label: Option<&'input str>,

    // SOS section data
    sos_constraints: Vec<RawConstraint<'input>>,
    current_sos_name: Option<&'input str>,
    current_sos_type: Option<SOSType>,
    current_sos_weights: Vec<RawCoefficient<'input>>,

    // QUADOBJ / QMATRIX entries, already converted to term coefficients.
    objective_quadratic: Vec<RawQuadraticTerm<'input>>,
    // QCMATRIX entries per row (with the header's line number), in file order.
    constraint_quadratic: Vec<(&'input str, usize, Vec<RawQuadraticTerm<'input>>)>,
    /// Rows already given a `QCMATRIX` section (duplicate detection).
    constraint_quadratic_rows: FxHashSet<&'input str>,

    // INDICATORS section data: (row, indicator column, active value, line)
    indicators: Vec<(&'input str, &'input str, bool, usize)>,

    has_rows: bool,
    has_columns: bool,
}

impl<'input> MpsParseState<'input> {
    fn new() -> Self {
        Self {
            section: None,
            sense: Sense::Minimize,
            objective_rows: Vec::new(),
            row_types: FxHashMap::default(),
            row_order: Vec::new(),
            row_classes: FxHashMap::default(),
            columns: ColumnsState::default(),
            rhs_values: FxHashMap::default(),
            rhs_vector_label: None,
            range_values: FxHashMap::default(),
            ranges_vector_label: None,
            bounds_state: BoundsState::default(),
            bounds_vector_label: None,
            sos_constraints: Vec::new(),
            current_sos_name: None,
            current_sos_type: None,
            current_sos_weights: Vec::new(),
            objective_quadratic: Vec::new(),
            constraint_quadratic: Vec::new(),
            constraint_quadratic_rows: FxHashSet::default(),
            indicators: Vec::new(),
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

        let header = line
            .split_whitespace()
            .next()
            .ok_or_else(|| LpParseError::parse_error(line_num, "Malformed section header: no token found on line"))?;
        match header.to_ascii_uppercase().as_str() {
            "NAME" => {
                self.section = Some(MpsSection::Name);
                // Extract problem name from remainder of line
                debug_assert!(line.len() >= 4, "NAME header must be at least 4 chars");
            }
            "OBJSENSE" => {
                self.section = Some(MpsSection::ObjSense);
                // Gurobi/CPLEX may write the sense on the header line itself:
                // `OBJSENSE MAXIMIZE` rather than on the following data line.
                if let Some(value) = line.split_whitespace().nth(1) {
                    self.sense = parse_objsense_value(value, line_num)?;
                }
            }
            "ROWS" => {
                self.section = Some(MpsSection::Rows);
                self.has_rows = true;
            }
            "LAZYCONS" => {
                self.section = Some(MpsSection::LazyCons);
                self.has_rows = true;
            }
            "USERCUTS" => {
                self.section = Some(MpsSection::UserCuts);
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
            "INDICATORS" => {
                self.section = Some(MpsSection::Indicators);
            }
            "QUADOBJ" => {
                self.section = Some(MpsSection::QuadObj);
            }
            "QMATRIX" => {
                self.section = Some(MpsSection::QMatrix);
            }
            "QCMATRIX" => {
                let row = line
                    .split_whitespace()
                    .nth(1)
                    .ok_or_else(|| LpParseError::parse_error(line_num, "QCMATRIX header must name its row"))?;
                match self.row_types.get(row) {
                    Some(RowType::N) | None => {
                        return Err(LpParseError::parse_error(
                            line_num,
                            format!("QCMATRIX references '{row}', which is not a constraint row"),
                        ));
                    }
                    Some(_) => {}
                }
                if !self.constraint_quadratic_rows.insert(row) {
                    return Err(LpParseError::parse_error(line_num, format!("row '{row}' has more than one QCMATRIX section")));
                }
                self.constraint_quadratic.push((row, line_num, Vec::new()));
                self.section = Some(MpsSection::QcMatrix);
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
            "PWLOBJ" | "GENCONS" | "SCENARIOS" => {
                eprintln!("Line {line_num}: unsupported section '{header}' will be skipped");
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
            MpsSection::ObjSense => {
                self.sense = parse_objsense_value(line.trim(), line_num)?;
            }
            MpsSection::Rows => {
                parse_rows_line(line, line_num, &mut self.objective_rows, &mut self.row_types, &mut self.row_order)?;
            }
            MpsSection::LazyCons | MpsSection::UserCuts => {
                let class = if current_section == MpsSection::LazyCons { ConstraintClass::Lazy } else { ConstraintClass::UserCut };
                let objective_count = self.objective_rows.len();
                let row_count = self.row_order.len();
                parse_rows_line(line, line_num, &mut self.objective_rows, &mut self.row_types, &mut self.row_order)?;
                if self.objective_rows.len() != objective_count {
                    return Err(LpParseError::parse_error(line_num, "an objective (N) row cannot be a lazy constraint or user cut"));
                }
                if let Some(&row_name) = self.row_order.get(row_count) {
                    self.row_classes.insert(row_name, class);
                }
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
                    &self.columns.column_index,
                    &mut self.columns.integer_vars,
                    &mut self.columns.integer_vars_set,
                    &mut self.bounds_vector_label,
                )?;
            }
            MpsSection::Name | MpsSection::Unsupported => {
                // NAME is captured by extract_mps_name; unsupported sections are skipped.
            }
            MpsSection::QuadObj | MpsSection::QMatrix | MpsSection::QcMatrix => {
                self.parse_quadratic_line(current_section, line, line_num)?;
            }
            MpsSection::Indicators => {
                self.parse_indicator_line(line, line_num)?;
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

    /// Parse one `column column value` line of a `QUADOBJ`, `QMATRIX` or
    /// `QCMATRIX` section.
    fn parse_quadratic_line(&mut self, current_section: MpsSection, line: &'input str, line_num: usize) -> LpResult<()> {
        let fields: Vec<&str> = line.split_whitespace().take_while(|f| !f.starts_with('$')).collect();
        let [var1, var2, value] = fields.as_slice() else {
            if fields.is_empty() {
                return Ok(());
            }
            return Err(LpParseError::parse_error(line_num, "quadratic entry must be 'column column value'"));
        };
        let value: f64 = value.parse().map_err(|_| LpParseError::invalid_number(*value, line_num))?;
        if !value.is_finite() {
            return Err(LpParseError::parse_error(line_num, format!("non-finite quadratic coefficient for '{var1}' * '{var2}'")));
        }
        // Convert a matrix entry into the coefficient of `var1 * var2`
        // (entries of the same pair are summed when interned).
        let coefficient = match current_section {
            // Upper triangle of Q in 1/2 x'Qx: Q_ii -> Q_ii / 2 x_i^2,
            // Q_ij (listed once) -> Q_ij x_i x_j.
            MpsSection::QuadObj if var1 == var2 => value / 2.0,
            // Full Q in 1/2 x'Qx: every entry contributes half.
            MpsSection::QMatrix => value / 2.0,
            // Off-diagonal QUADOBJ entries, and QCMATRIX (full Q in
            // x'Qx), contribute in full.
            _ => value,
        };
        let term = RawQuadraticTerm { var1, var2, coefficient };
        if current_section == MpsSection::QcMatrix {
            let Some((_, _, terms)) = self.constraint_quadratic.last_mut() else {
                unreachable!("the QCMATRIX header registers its row");
            };
            terms.push(term);
        } else {
            self.objective_quadratic.push(term);
        }
        Ok(())
    }

    /// Parse one `IF row column value` line of the `INDICATORS` section.
    fn parse_indicator_line(&mut self, line: &'input str, line_num: usize) -> LpResult<()> {
        let fields: Vec<&str> = line.split_whitespace().take_while(|f| !f.starts_with('$')).collect();
        match fields.as_slice() {
            [] => {}
            [kind, row, column, value] if kind.eq_ignore_ascii_case("IF") => {
                let active_value = match *value {
                    "1" => true,
                    "0" => false,
                    other => {
                        return Err(LpParseError::parse_error(line_num, format!("indicator value must be 0 or 1, got '{other}'")));
                    }
                };
                match self.row_types.get(row) {
                    Some(RowType::N) | None => {
                        return Err(LpParseError::parse_error(
                            line_num,
                            format!("INDICATORS references '{row}', which is not a constraint row"),
                        ));
                    }
                    Some(_) => {}
                }
                self.indicators.push((row, column, active_value, line_num));
            }
            _ => {
                return Err(LpParseError::parse_error(line_num, "INDICATORS line must be 'IF row column value'"));
            }
        }
        Ok(())
    }
    /// Validate required sections and build the final [`ParseResult`].
    fn build_result(mut self) -> LpResult<ParseResult<'input>> {
        // ENDATA flushes the last SOS set, but input without ENDATA is read
        // leniently, so flush here too (a no-op once ENDATA has flushed).
        flush_sos_constraint(
            &mut self.sos_constraints,
            &mut self.current_sos_name,
            &mut self.current_sos_type,
            &mut self.current_sos_weights,
        );
        debug_assert!(self.current_sos_weights.is_empty(), "all SOS weights must be flushed before building the result");

        if self.columns.in_integer_block {
            eprintln!("unclosed INTORG marker block at end of MPS input; trailing columns treated as integer");
        }

        if !self.has_rows {
            return Err(LpParseError::missing_section("ROWS"));
        }
        if !self.has_columns {
            return Err(LpParseError::missing_section("COLUMNS"));
        }

        // Sort each row's entries by column index so builders emit
        // coefficients in column order, matching the original file layout.
        for entries in self.columns.row_entries.values_mut() {
            entries.sort_unstable_by_key(|&(col_idx, _)| col_idx);
        }

        let objectives = build_objectives(&self.objective_rows, &self.columns, &self.rhs_values);
        let mut constraints =
            build_constraints(&self.row_types, &self.row_order, &self.row_classes, &self.columns, &self.rhs_values, &self.range_values);
        // Locate every row the INDICATORS / QCMATRIX sections name in one
        // pass, rather than a linear scan per entry. Rewriting a row in place
        // keeps its position, so one index serves both passes.
        let positions = if self.indicators.is_empty() && self.constraint_quadratic.is_empty() {
            FxHashMap::default()
        } else {
            locate_rows(&constraints, self.indicators.iter().map(|&(row, ..)| row).chain(self.constraint_quadratic_rows.iter().copied()))
        };
        apply_indicators(&mut constraints, &positions, &self.indicators, &self.range_values)?;
        apply_constraint_quadratics(&mut constraints, &positions, self.constraint_quadratic, &self.range_values)?;
        let mut objectives = objectives;
        if !self.objective_quadratic.is_empty() {
            // MPS has one objective row; its quadratic part belongs to it.
            let Some(first) = objectives.first_mut() else {
                unreachable!("build_objectives always yields at least one objective");
            };
            first.quadratic = self.objective_quadratic;
        }
        let bounds = build_bounds(
            &self.bounds_state.accumulators,
            &self.bounds_state.order,
            &self.columns.column_order,
            &self.columns.integer_vars_set,
        );

        // Deduplicate variable lists
        let mut integer_seen: FxHashSet<&str> = FxHashSet::default();
        self.columns.integer_vars.retain(|v| integer_seen.insert(v));

        let mut binary_seen: FxHashSet<&str> = FxHashSet::default();
        self.bounds_state.binary_vars.retain(|v| binary_seen.insert(v));

        let mut semi_continuous_seen: FxHashSet<&str> = FxHashSet::default();
        self.bounds_state.semi_continuous_vars.retain(|v| semi_continuous_seen.insert(v));

        Ok(ParseResult {
            sense: self.sense,
            objectives,
            constraints: constraints.normal,
            bounds,
            generals: Vec::new(),
            integers: self.columns.integer_vars,
            binaries: self.bounds_state.binary_vars,
            semi_continuous: self.bounds_state.semi_continuous_vars,
            sos: self.sos_constraints,
            lazy_constraints: constraints.lazy,
            user_cuts: constraints.user_cuts,
        })
    }
}

/// Position of a constraint: its bucket (see
/// [`ClassifiedConstraints::slot_mut`](super::builders::ClassifiedConstraints))
/// and its index within that bucket.
type RowPositions<'input> = FxHashMap<&'input str, (usize, usize)>;

/// Map each of the `wanted` row names to the position of the first
/// constraint carrying that name, in a single pass over `constraints`. Names
/// with no matching constraint are absent from the result.
fn locate_rows<'input>(
    constraints: &super::builders::ClassifiedConstraints<'input>,
    wanted: impl Iterator<Item = &'input str>,
) -> RowPositions<'input> {
    let mut found: FxHashMap<&'input str, Option<(usize, usize)>> = wanted.map(|row| (row, None)).collect();
    for (bucket_idx, bucket) in constraints.buckets().into_iter().enumerate() {
        for (idx, constraint) in bucket.iter().enumerate() {
            if let Some(slot) = found.get_mut(constraint.name())
                && slot.is_none()
            {
                *slot = Some((bucket_idx, idx));
            }
        }
    }
    found.into_iter().filter_map(|(row, position)| position.map(|p| (row, p))).collect()
}

/// Turn each row named in the `INDICATORS` section into an indicator
/// constraint whose linear part is that row.
///
/// # Errors
///
/// Returns an error for a ranged indicator row (a range is two rows, and an
/// indicator constrains exactly one) or a row given two indicators.
fn apply_indicators<'input>(
    constraints: &mut super::builders::ClassifiedConstraints<'input>,
    positions: &RowPositions<'input>,
    indicators: &[(&'input str, &'input str, bool, usize)],
    range_values: &FxHashMap<&'input str, f64>,
) -> LpResult<()> {
    let mut seen: FxHashSet<&str> = FxHashSet::default();
    for &(row, column, active_value, line_num) in indicators {
        if !seen.insert(row) {
            return Err(LpParseError::parse_error(line_num, format!("row '{row}' has more than one indicator")));
        }
        if range_values.contains_key(row) {
            return Err(LpParseError::parse_error(line_num, format!("indicator row '{row}' cannot have a RANGES entry")));
        }
        let &position =
            positions.get(row).ok_or_else(|| LpParseError::parse_error(line_num, format!("INDICATORS references unknown row '{row}'")))?;
        let slot = constraints.slot_mut(position);
        debug_assert!(slot.name() == row, "row position index must point at the named row");
        let RawConstraint::Standard { name, coefficients, operator, rhs, byte_offset } = slot else {
            return Err(LpParseError::parse_error(line_num, format!("row '{row}' cannot take an indicator")));
        };
        *slot = RawConstraint::Indicator {
            name: std::mem::take(name),
            variable: column,
            active_value,
            coefficients: std::mem::take(coefficients),
            operator: *operator,
            rhs: *rhs,
            byte_offset: *byte_offset,
        };
    }
    Ok(())
}

/// Turn each row with a `QCMATRIX` section into a quadratic constraint.
///
/// # Errors
///
/// Returns an error for a ranged row, a row that is also an indicator, or a
/// `QCMATRIX` section with no entries.
fn apply_constraint_quadratics<'input>(
    constraints: &mut super::builders::ClassifiedConstraints<'input>,
    positions: &RowPositions<'input>,
    quadratics: Vec<(&'input str, usize, Vec<RawQuadraticTerm<'input>>)>,
    range_values: &FxHashMap<&'input str, f64>,
) -> LpResult<()> {
    for (row, line_num, terms) in quadratics {
        if terms.is_empty() {
            return Err(LpParseError::parse_error(line_num, format!("QCMATRIX section for row '{row}' has no entries")));
        }
        if range_values.contains_key(row) {
            return Err(LpParseError::parse_error(line_num, format!("quadratic row '{row}' cannot have a RANGES entry")));
        }
        let &position =
            positions.get(row).ok_or_else(|| LpParseError::parse_error(line_num, format!("QCMATRIX references unknown row '{row}'")))?;
        let slot = constraints.slot_mut(position);
        debug_assert!(slot.name() == row, "row position index must point at the named row");
        let RawConstraint::Standard { name, coefficients, operator, rhs, byte_offset } = slot else {
            return Err(LpParseError::parse_error(line_num, format!("row '{row}' cannot be both an indicator and quadratic")));
        };
        *slot = RawConstraint::Quadratic {
            name: std::mem::take(name),
            coefficients: std::mem::take(coefficients),
            quadratic: terms,
            operator: *operator,
            rhs: *rhs,
            byte_offset: *byte_offset,
        };
    }
    Ok(())
}

/// Parse an OBJSENSE value (`MIN`/`MINIMIZE`/`MAX`/`MAXIMIZE`, case-insensitive).
fn parse_objsense_value(value: &str, line_num: usize) -> LpResult<Sense> {
    match value.to_ascii_uppercase().as_str() {
        "MIN" | "MINIMIZE" => Ok(Sense::Minimize),
        "MAX" | "MAXIMIZE" => Ok(Sense::Maximize),
        _ => Err(LpParseError::parse_error(line_num, format!("Invalid OBJSENSE value: '{value}'"))),
    }
}

/// Parse an MPS-format string into a [`ParseResult`].
///
/// # Errors
///
/// Returns an error for malformed MPS content including missing required
/// sections, invalid row/bound types, number parse failures, and references
/// to undefined rows.
pub fn parse_mps(input: &str) -> LpResult<ParseResult<'_>> {
    // Some Windows editors write a leading UTF-8 byte order mark.
    let input = input.strip_prefix('\u{FEFF}').unwrap_or(input);
    // Input is external, so empty input must be a runtime error rather than an assertion.
    if input.trim().is_empty() {
        return Err(LpParseError::parse_error(0, "MPS input is empty"));
    }

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

        let reached_end = if is_section_header {
            state.process_section_header(line, line_num)
        } else {
            state.dispatch_data_line(line, line_num).map(|()| false)
        };
        if reached_end.map_err(|err| locate_error(err, input, line))? {
            break;
        }
    }

    state.build_result()
}

/// Re-anchor an error raised while parsing `line` at a byte offset in `input`.
///
/// The section parsers only know the 1-based line number and report it in the
/// `position` field; the public contract of that field is a byte offset (as
/// for LP input), and a parse error gains line/column source context from it.
fn locate_error(err: LpParseError, input: &str, line: &str) -> LpParseError {
    // `line` is a subslice of `input` (from `str::lines`), so the pointer
    // difference is its byte offset.
    let line_start = (line.as_ptr() as usize).wrapping_sub(input.as_ptr() as usize);
    debug_assert!(line_start + line.len() <= input.len(), "line must be a subslice of input");

    match err {
        LpParseError::ParseError { message, .. } => LpParseError::parse_error(line_start, message).with_source(input),
        LpParseError::InvalidNumber { value, .. } => {
            let offset = line.find(value.as_str()).unwrap_or(0);
            LpParseError::invalid_number(value, line_start + offset)
        }
        other => other,
    }
}

/// Extract the problem name from MPS input (the NAME section line).
///
/// # Example
///
/// ```rust
/// use lp_parser_rs::extract_mps_name;
///
/// assert_eq!(extract_mps_name("NAME  afiro\nROWS\n"), Some("afiro".to_string()));
/// assert_eq!(extract_mps_name("ROWS\n"), None);
/// ```
#[must_use]
pub fn extract_mps_name(input: &str) -> Option<String> {
    for line in input.lines() {
        if line.trim().is_empty() || line.starts_with('*') {
            continue;
        }
        let first_char = line.as_bytes().first().copied();
        if first_char.is_some_and(|c| !c.is_ascii_whitespace()) {
            // A header line always has a first token (the line is non-blank and starts
            // with a non-whitespace byte); skip the line rather than default silently.
            let Some(header) = line.split_whitespace().next() else { continue };
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
