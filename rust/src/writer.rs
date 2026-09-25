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
//! let lp_content = write_lp_string(&problem)?;
//! println!("{}", lp_content);
//! # Ok::<(), lp_parser_rs::LpParseError>(())
//! ```

use std::fmt::Write;

use logos::Logos;

use crate::NUMERIC_EPSILON;
use crate::error::{LpParseError, LpResult};
use crate::interner::{NameId, NameInterner};
use crate::lexer::Token;
use crate::model::{Coefficient, Constraint, ConstraintClass, GeneralFunction, Objective, ObjectiveAttributes, QuadraticTerm, Variable};
use crate::problem::LpProblem;

/// Options for controlling LP file output format
#[derive(Debug, Clone)]
pub struct LpWriterOptions {
    /// Include problem name comment at the top
    pub include_problem_name: bool,
    /// Maximum line length before wrapping coefficients
    pub max_line_length: usize,
    /// Number of decimal places for numeric values. `None` (the default)
    /// writes the shortest representation that parses back to the exact same
    /// `f64`; `Some(n)` rounds to `n` decimal places, which is lossy.
    pub decimal_precision: Option<usize>,
    /// Include empty lines between sections
    pub include_section_spacing: bool,
}

impl Default for LpWriterOptions {
    fn default() -> Self {
        Self { include_problem_name: true, max_line_length: 80, decimal_precision: None, include_section_spacing: true }
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
/// Returns a validation error if a name cannot be written as an LP identifier
/// that reads back unchanged -- see [`write_lp_string_with_options`].
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
/// Returns a validation error if an objective, constraint or variable name is
/// not a single LP identifier -- e.g. a keyword such as `free` or `st`, a name
/// starting with a digit, or one containing `:`, `<`, `=`, `+` or whitespace --
/// or if the problem name (when written) contains a line break. Writing such a
/// name would produce a file that fails to parse or means something else.
///
/// Objectives and constraints with no terms cannot be expressed in LP syntax
/// and are omitted; use [`write_lp_string_with_warnings`] to be told which.
pub fn write_lp_string_with_options(problem: &LpProblem, options: &LpWriterOptions) -> LpResult<String> {
    write_lp_string_with_warnings(problem, options).map(|(output, _omitted)| output)
}

/// Write an `LpProblem` to a string with custom formatting options, also
/// returning a warning for each objective or constraint that was omitted
/// because it has no terms (LP syntax has no way to write an empty expression).
///
/// The library never prints these itself, so the caller decides whether and
/// where to report them.
///
/// # Errors
///
/// See [`write_lp_string_with_options`].
// The only panic is the expect on fmt::Write to String, which is infallible.
#[allow(clippy::missing_panics_doc)]
pub fn write_lp_string_with_warnings(problem: &LpProblem, options: &LpWriterOptions) -> LpResult<(String, Vec<String>)> {
    validate_lp_names(problem, options)?;
    validate_numbers(problem)?;
    let mut output = String::new();
    build_lp(&mut output, problem, options).expect("fmt::Write to String is infallible");
    Ok((output, omitted_expressions(problem)))
}

/// Describe each objective or constraint the writer skips (see
/// [`write_objective`] and [`write_constraint`]).
fn omitted_expressions(problem: &LpProblem) -> Vec<String> {
    let objectives = problem
        .objectives
        .values()
        .filter(|o| is_empty_objective(o))
        .map(|o| format!("objective '{}' has no coefficients and was omitted from the LP output", problem.resolve(o.name)));
    let constraints = problem
        .constraints
        .values()
        .filter(|c| {
            matches!(c, Constraint::Standard { coefficients, .. } | Constraint::Indicator { coefficients, .. } if coefficients.is_empty())
        })
        .map(|c| format!("constraint '{}' has no coefficients and was omitted from the LP output", problem.resolve(c.name())));
    objectives.chain(constraints).collect()
}

/// An objective with neither terms nor a constant, which the writer omits.
fn is_empty_objective(objective: &Objective) -> bool {
    objective.coefficients.is_empty() && objective.quadratic.is_empty() && objective.constant == 0.0
}

/// Check that `name` lexes back as exactly one LP identifier equal to itself.
///
/// # Errors
///
/// Returns a validation error naming the offending `kind` and `name`.
pub(crate) fn check_lp_name(name: &str, kind: &str) -> LpResult<()> {
    // Lex without the context-sensitive keyword adapter: a keyword can read
    // back as a name in one position but not another (e.g. `bin` alone on a
    // line in a generals section), so reject every keyword outright.
    let mut tokens = Token::lexer(name);
    let is_identifier = matches!(tokens.next(), Some(Ok(Token::Identifier(ident))) if ident == name);
    if is_identifier && tokens.next().is_none() {
        Ok(())
    } else {
        Err(LpParseError::validation_error(format!(
            "{kind} name '{name}' cannot be written to LP: it does not read back as a single identifier \
             (keywords, leading digits, whitespace and characters such as ':', '<', '=', '+' are not allowed)"
        )))
    }
}

/// Validate every name the LP writer will emit (see [`check_lp_name`]).
fn validate_lp_names(problem: &LpProblem, options: &LpWriterOptions) -> LpResult<()> {
    if options.include_problem_name
        && let Some(name) = problem.name()
        && name.contains(['\n', '\r'])
    {
        return Err(LpParseError::validation_error(format!("problem name {name:?} cannot be written to LP: it contains a line break")));
    }

    // Ids are dense, so a flag per interned name replaces a hash set. An id
    // outside the table (from another interner) is checked every time, and
    // `resolve` rejects it exactly as before.
    let mut checked = vec![false; problem.interner.len()];
    let mut check = |id: NameId, kind: &str| -> LpResult<()> {
        match checked.get_mut(id.index()) {
            Some(true) => Ok(()),
            Some(seen) => {
                *seen = true;
                check_lp_name(problem.resolve(id), kind)
            }
            None => check_lp_name(problem.resolve(id), kind),
        }
    };
    for id in problem.variables.keys() {
        check(*id, "variable")?;
    }
    for objective in problem.objectives.values() {
        check(objective.name, "objective")?;
        for coeff in &objective.coefficients {
            check(coeff.name, "variable")?;
        }
        for term in &objective.quadratic {
            check(term.var1, "variable")?;
            check(term.var2, "variable")?;
        }
    }
    for constraint in problem.constraints.values() {
        check(constraint.name(), "constraint")?;
        let mut result = Ok(());
        constraint.for_each_variable(|id| {
            if result.is_ok() {
                result = check(id, "variable");
            }
        });
        result?;
    }
    Ok(())
}

/// Validate every numeric value a writer will emit.
///
/// `LpProblem` fields are public, so a hand-built problem can carry values no
/// file format can express. Rather than write `NaN` (or trip the debug
/// assertions in [`write_number`] / [`write_formatted_coefficient`]), reject
/// them up front: `NaN` anywhere, and infinite linear or quadratic
/// coefficients or objective constants. Infinite right-hand sides and bounds
/// stay legal; they are written as `inf` / `-inf`.
///
/// `pub(crate)` so the MPS writer shares the same checks.
///
/// # Errors
///
/// Returns a validation error naming the offending objective, constraint or
/// variable.
pub(crate) fn validate_numbers(problem: &LpProblem) -> LpResult<()> {
    let invalid = |kind: &str, id: NameId, what: &str, value: f64| {
        Err(LpParseError::validation_error(format!("{kind} '{}' has {what} {value}, which cannot be written", problem.resolve(id))))
    };
    let check_linear = |kind: &str, owner: NameId, coefficients: &[Coefficient]| -> LpResult<()> {
        match coefficients.iter().find(|c| !c.value.is_finite()) {
            Some(c) => invalid(kind, owner, &format!("a coefficient on '{}' of", problem.resolve(c.name)), c.value),
            None => Ok(()),
        }
    };
    let check_quadratic = |kind: &str, owner: NameId, terms: &[QuadraticTerm]| -> LpResult<()> {
        match terms.iter().find(|t| !t.coefficient.is_finite()) {
            Some(t) => invalid(kind, owner, "a quadratic coefficient of", t.coefficient),
            None => Ok(()),
        }
    };

    for objective in problem.objectives.values() {
        check_linear("objective", objective.name, &objective.coefficients)?;
        check_quadratic("objective", objective.name, &objective.quadratic)?;
        if !objective.constant.is_finite() {
            return invalid("objective", objective.name, "a constant of", objective.constant);
        }
        let ObjectiveAttributes { weight, abs_tol, rel_tol, .. } = objective.attributes;
        if let Some(value) = [weight, abs_tol, rel_tol].into_iter().flatten().find(|v| v.is_nan()) {
            return invalid("objective", objective.name, "an attribute of", value);
        }
    }
    for constraint in problem.constraints.values() {
        let name = constraint.name();
        match constraint {
            Constraint::Standard { coefficients, rhs, .. } | Constraint::Indicator { coefficients, rhs, .. } => {
                check_linear("constraint", name, coefficients)?;
                if rhs.is_nan() {
                    return invalid("constraint", name, "a right-hand side of", *rhs);
                }
            }
            Constraint::Quadratic { coefficients, quadratic, rhs, .. } => {
                check_linear("constraint", name, coefficients)?;
                check_quadratic("constraint", name, quadratic)?;
                if rhs.is_nan() {
                    return invalid("constraint", name, "a right-hand side of", *rhs);
                }
            }
            Constraint::SOS { weights, .. } => {
                if let Some(w) = weights.iter().find(|w| w.value.is_nan()) {
                    return invalid("SOS constraint", name, &format!("a weight on '{}' of", problem.resolve(w.name)), w.value);
                }
            }
            Constraint::General { function, .. } => {
                if let GeneralFunction::Max { constant: Some(c), .. } | GeneralFunction::Min { constant: Some(c), .. } = function
                    && c.is_nan()
                {
                    return invalid("general constraint", name, "a constant argument of", *c);
                }
            }
        }
    }
    for (id, variable) in &problem.variables {
        if let Some(value) = [variable.bounds.lower, variable.bounds.upper].into_iter().flatten().find(|v| v.is_nan()) {
            return invalid("variable", *id, "a bound of", value);
        }
    }
    Ok(())
}

/// Build the full LP document into `output`.
fn build_lp(output: &mut String, problem: &LpProblem, options: &LpWriterOptions) -> std::fmt::Result {
    // Write problem name comment if requested
    if options.include_problem_name
        && let Some(name) = problem.name()
    {
        writeln!(output, "\\Problem name: {name}")?;
        if options.include_section_spacing {
            writeln!(output)?;
        }
    }

    // Write sense and objectives
    write_objectives_section(output, problem, options)?;

    // Write constraints. The grammar requires a `Subject To` header even when
    // there are no constraints, so it is always written.
    if options.include_section_spacing {
        writeln!(output)?;
    }
    write_constraints_section(output, problem, options)?;

    // Lazy constraints and user cuts follow `Subject To` (CPLEX).
    write_classed_constraints_section(output, problem, options, ConstraintClass::Lazy, "Lazy Constraints")?;
    write_classed_constraints_section(output, problem, options, ConstraintClass::UserCut, "User Cuts")?;

    // Write bounds
    write_bounds_section(output, problem, options)?;

    // Write variable type sections
    write_variable_types_sections(output, problem, options)?;

    // Write SOS constraints (their own section; not valid `Subject To` syntax)
    write_sos_section(output, problem, options)?;

    // Gurobi general constraints have their own section too.
    write_general_constraints_section(output, problem, options)?;

    // Write end marker
    if options.include_section_spacing {
        writeln!(output)?;
    }
    writeln!(output, "End")
}

/// Write the objectives section (sense + objectives)
fn write_objectives_section(output: &mut String, problem: &LpProblem, options: &LpWriterOptions) -> std::fmt::Result {
    // Gurobi's multi-objective attributes need the `multi-objectives` marker.
    if problem.objectives.values().any(|o| !o.attributes.is_empty()) {
        writeln!(output, "{} multi-objectives", problem.sense)?;
    } else {
        writeln!(output, "{}", problem.sense)?;
    }

    for objective in problem.objectives.values() {
        write_objective(output, objective, &problem.interner, options)?;
    }

    Ok(())
}

/// Write a single objective
fn write_objective(output: &mut String, objective: &Objective, interner: &NameInterner, options: &LpWriterOptions) -> std::fmt::Result {
    let name = interner.resolve(objective.name);
    if is_empty_objective(objective) {
        // CPLEX accepts an empty objective, but a bare ` name: ` line adds
        // nothing; skip it (reported by `omitted_expressions`).
        return Ok(());
    }
    write!(output, " {name}: ")?;
    if !objective.attributes.is_empty() {
        write_objective_attributes(output, &objective.attributes, options)?;
        // Gurobi puts the expression on the line after the attributes.
        write!(output, "\n  ")?;
    }

    // Objective quadratics are written `[ ... ] / 2` (CPLEX, Gurobi), so the
    // stored coefficients are doubled inside the brackets.
    write_expression(output, &objective.coefficients, &objective.quadratic, QuadraticBlock::Halved, interner, options)?;

    if objective.constant != 0.0 {
        if objective.coefficients.is_empty() && objective.quadratic.is_empty() {
            write_number(output, objective.constant, options.decimal_precision)?;
        } else {
            write!(output, " {} ", if objective.constant < 0.0 { "-" } else { "+" })?;
            write_number(output, objective.constant.abs(), options.decimal_precision)?;
        }
    }
    writeln!(output)
}

/// Write Gurobi multi-objective attributes (`Priority=2 Weight=1 AbsTol=0
/// RelTol=0`), skipping unset ones.
fn write_objective_attributes(output: &mut String, attributes: &ObjectiveAttributes, options: &LpWriterOptions) -> std::fmt::Result {
    debug_assert!(!attributes.is_empty(), "only called for an objective with attributes");
    let mut separator = "";
    if let Some(priority) = attributes.priority {
        write!(output, "Priority={priority}")?;
        separator = " ";
    }
    for (label, value) in [("Weight", attributes.weight), ("AbsTol", attributes.abs_tol), ("RelTol", attributes.rel_tol)] {
        if let Some(value) = value {
            write!(output, "{separator}{label}=")?;
            write_number(output, value, options.decimal_precision)?;
            separator = " ";
        }
    }
    Ok(())
}

/// Write the constraints section (standard and indicator constraints; SOS
/// constraints belong in their own `SOS` section)
fn write_constraints_section(output: &mut String, problem: &LpProblem, options: &LpWriterOptions) -> std::fmt::Result {
    writeln!(output, "Subject To")?;

    for (id, constraint) in &problem.constraints {
        let in_subject_to = matches!(constraint, Constraint::Standard { .. } | Constraint::Indicator { .. } | Constraint::Quadratic { .. });
        if in_subject_to && problem.constraint_class(*id).is_normal() {
            write_constraint(output, constraint, &problem.interner, options)?;
        }
    }

    Ok(())
}

/// Write the constraints of one non-ordinary [`ConstraintClass`] under their
/// own section header (`Lazy Constraints` / `User Cuts`), if there are any.
fn write_classed_constraints_section(
    output: &mut String,
    problem: &LpProblem,
    options: &LpWriterOptions,
    class: ConstraintClass,
    header: &str,
) -> std::fmt::Result {
    debug_assert!(!class.is_normal(), "ordinary constraints belong under `Subject To`");
    if !problem.constraint_classes.values().any(|c| *c == class) {
        return Ok(());
    }
    let mut wrote_header = false;
    // Constraint order, not class-map order, so output follows the model.
    for (id, constraint) in &problem.constraints {
        if problem.constraint_class(*id) != class {
            continue;
        }
        if !wrote_header {
            if options.include_section_spacing {
                writeln!(output)?;
            }
            writeln!(output, "{header}")?;
            wrote_header = true;
        }
        write_constraint(output, constraint, &problem.interner, options)?;
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

/// Write the Gurobi `General Constraints` section.
fn write_general_constraints_section(output: &mut String, problem: &LpProblem, options: &LpWriterOptions) -> std::fmt::Result {
    let mut wrote_header = false;
    for constraint in problem.constraints.values() {
        if matches!(constraint, Constraint::General { .. }) {
            if !wrote_header {
                if options.include_section_spacing {
                    writeln!(output)?;
                }
                writeln!(output, "General Constraints")?;
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
                // A dangling ` name:  <= rhs` line is not valid LP syntax; the
                // omission is reported by `omitted_expressions`.
                return Ok(());
            }
            output.push(' ');
            output.push_str(resolved_name);
            output.push_str(": ");

            write_coefficients_line(output, coefficients, interner, options)?;

            write!(output, " {operator} ")?;
            write_number(output, *rhs, options.decimal_precision)?;
            output.push('\n');
            Ok(())
        }
        Constraint::General { name, resultant, function, .. } => {
            debug_assert!(!function.variables().is_empty(), "a general constraint has at least one variable argument");
            // Gurobi separates the parentheses and commas with spaces; without
            // them they would lex as part of the neighbouring names.
            write!(output, " {}: {} = {} (", interner.resolve(*name), interner.resolve(*resultant), function.keyword())?;
            for (i, variable) in function.variables().iter().enumerate() {
                write!(output, "{} {}", if i == 0 { "" } else { " ," }, interner.resolve(*variable))?;
            }
            if let Some(constant) = function.constant() {
                write!(output, " , ")?;
                write_number(output, constant, options.decimal_precision)?;
            }
            writeln!(output, " )")
        }
        Constraint::Quadratic { name, coefficients, quadratic, operator, rhs, .. } => {
            debug_assert!(!quadratic.is_empty(), "a quadratic constraint has quadratic terms");
            let resolved_name = interner.resolve(*name);
            write!(output, " {resolved_name}: ")?;

            write_expression(output, coefficients, quadratic, QuadraticBlock::Plain, interner, options)?;

            write!(output, " {operator} ")?;
            write_number(output, *rhs, options.decimal_precision)?;
            writeln!(output)
        }
        Constraint::Indicator { name, variable, active_value, coefficients, operator, rhs, .. } => {
            if coefficients.is_empty() {
                // As for a standard constraint: reported by `omitted_expressions`.
                return Ok(());
            }
            let resolved_name = interner.resolve(*name);
            let indicator = interner.resolve(*variable);
            write!(output, " {resolved_name}: {indicator} = {} -> ", u8::from(*active_value))?;

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
    let has_bounds = problem.variables.values().any(needs_bounds_declaration);

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

/// Whether a variable should appear in the Bounds section.
///
/// A variable with no declared bounds is omitted entirely: LP's default for it
/// is `[0, +inf)`, and writing anything at all would state a bound the input
/// never had. Emitting `x free` here — as this did while "free" and "no bounds
/// declared" shared a representation — silently widened every undeclared
/// variable's feasible region to include negatives on the way out.
///
/// A declared-free variable is emitted as `x free` whatever its kind: an
/// integer or general variable's default lower bound is still 0, so dropping
/// the declaration would narrow its range on the way through.
const fn needs_bounds_declaration(variable: &Variable) -> bool {
    !variable.bounds.is_unspecified()
}

/// Write bounds for a single variable
fn write_variable_bounds(output: &mut String, variable: &Variable, interner: &NameInterner, options: &LpWriterOptions) -> std::fmt::Result {
    if !needs_bounds_declaration(variable) {
        return Ok(());
    }

    let var_name = interner.resolve(variable.name);
    if variable.bounds.is_free() {
        writeln!(output, "{var_name} free")?;
        return Ok(());
    }
    match (variable.bounds.lower, variable.bounds.upper) {
        // Unreachable in practice: `needs_bounds_declaration` returns false for
        // an undeclared variable, so there is nothing to write.
        (None, None) => {}
        (Some(bound), None) => {
            write!(output, "{var_name} >= ")?;
            write_number(output, bound, options.decimal_precision)?;
            writeln!(output)?;
        }
        (None, Some(bound)) => {
            write!(output, "{var_name} <= ")?;
            write_number(output, bound, options.decimal_precision)?;
            writeln!(output)?;
        }
        (Some(lower), Some(upper)) => {
            write_number(output, lower, options.decimal_precision)?;
            write!(output, " <= {var_name} <= ")?;
            write_number(output, upper, options.decimal_precision)?;
            writeln!(output)?;
        }
    }

    Ok(())
}

/// Write variable type sections (binaries, integers, etc.)
fn write_variable_types_sections(output: &mut String, problem: &LpProblem, options: &LpWriterOptions) -> std::fmt::Result {
    use crate::model::VariableKind;

    // Group variables by kind, resolving names
    let mut binaries = Vec::new();
    let mut integers = Vec::new();
    let mut generals = Vec::new();
    let mut semi_continuous = Vec::new();

    for variable in problem.variables.values() {
        let var_name = problem.interner.resolve(variable.name);
        match variable.kind {
            VariableKind::Binary => binaries.push(var_name),
            VariableKind::Integer => integers.push(var_name),
            VariableKind::General => generals.push(var_name),
            VariableKind::SemiContinuous => semi_continuous.push(var_name),
            // CPLEX declares a semi-integer variable by listing it in both the
            // generals and the semi-continuous sections.
            VariableKind::SemiInteger => {
                generals.push(var_name);
                semi_continuous.push(var_name);
            }
            VariableKind::Continuous | VariableKind::Sos => {}
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
    write_expression(output, coefficients, &[], QuadraticBlock::Plain, interner, options)
}

/// How a quadratic block is written: in an objective it is `[ ... ] / 2` with
/// doubled coefficients, in a constraint plain `[ ... ]`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum QuadraticBlock {
    Halved,
    Plain,
}

/// Write linear terms followed by a quadratic block (if any), wrapping long
/// lines onto indented continuation lines.
fn write_expression(
    output: &mut String,
    coefficients: &[Coefficient],
    quadratic: &[QuadraticTerm],
    block: QuadraticBlock,
    interner: &NameInterner,
    options: &LpWriterOptions,
) -> std::fmt::Result {
    const CONTINUATION_INDENT: &str = "\n        ";
    let mut current_line_length: usize = 0;
    let mut pieces_written = 0usize;
    // Each piece is written straight into `output` from `start`; once its
    // real width is known, a line break is inserted in front of it if it
    // would overflow the line. Inserting shifts only the piece itself.
    let mut place = |output: &mut String, start: usize| {
        debug_assert!(start <= output.len(), "a piece starts within the output");
        let piece_len = output.len() - start;
        if current_line_length + piece_len > options.max_line_length && pieces_written > 0 {
            output.insert_str(start, CONTINUATION_INDENT);
            // The newline starts the continuation line; only the indent counts.
            current_line_length = CONTINUATION_INDENT.len() - 1;
        }
        current_line_length += piece_len;
        pieces_written += 1;
    };

    for (i, coeff) in coefficients.iter().enumerate() {
        let var_name = interner.resolve(coeff.name);
        let start = output.len();
        write_formatted_coefficient(output, var_name, coeff.value, i == 0, options.decimal_precision)?;
        place(output, start);
    }

    if quadratic.is_empty() {
        return Ok(());
    }
    let scale = if block == QuadraticBlock::Halved { 2.0 } else { 1.0 };
    let start = output.len();
    output.push_str(if coefficients.is_empty() { "[" } else { " + [" });
    place(output, start);
    for (i, term) in quadratic.iter().enumerate() {
        let start = output.len();
        let product = if term.is_square() {
            format!("{} ^ 2", interner.resolve(term.var1))
        } else {
            format!("{} * {}", interner.resolve(term.var1), interner.resolve(term.var2))
        };
        // A leading space separates the first term from the opening bracket.
        if i == 0 {
            output.push(' ');
        }
        write_formatted_coefficient(output, &product, term.coefficient * scale, i == 0, options.decimal_precision)?;
        place(output, start);
    }
    let start = output.len();
    output.push_str(if block == QuadraticBlock::Halved { " ] / 2" } else { " ]" });
    place(output, start);
    Ok(())
}

/// Write a formatted coefficient directly to the output buffer, avoiding intermediate `String` allocation.
///
/// `pub(crate)` so the `lp-solvers` compat adapter ([`crate::compat::lp_solvers`])
/// emits coefficients identically to the LP writer.
pub(crate) fn write_formatted_coefficient(
    output: &mut String,
    name: &str,
    value: f64,
    is_first: bool,
    precision: Option<usize>,
) -> std::fmt::Result {
    debug_assert!(!name.is_empty(), "coefficient name must not be empty");
    debug_assert!(value.is_finite(), "coefficient value must be finite, got: {value}");
    let abs_value = value.abs();
    let sign = if value < 0.0 { "-" } else { "+" };
    // In exact mode (`precision == None`) the output must read back as the
    // identical `f64`, so only an exact 1 may drop its coefficient; a
    // rounding precision already writes near-1 values as `1`.
    #[allow(clippy::float_cmp)]
    let is_one = match precision {
        None => abs_value == 1.0,
        Some(_) => (abs_value - 1.0).abs() < NUMERIC_EPSILON,
    };

    // Plain `push_str` rather than `write!`: this runs once per term, and
    // the formatting machinery dominated the writer's profile.
    if is_first {
        if value < 0.0 {
            output.push_str("- ");
        }
    } else {
        output.push(' ');
        output.push_str(sign);
        output.push(' ');
    }
    if !is_one {
        write_number(output, abs_value, precision)?;
        output.push(' ');
    }
    output.push_str(name);
    Ok(())
}

/// Magnitudes outside `[SCIENTIFIC_BELOW, SCIENTIFIC_FROM)` are written in
/// scientific notation, so neither `1e300` nor `1e-300` expands to hundreds of
/// digits.
const SCIENTIFIC_FROM: f64 = 1e16;
/// See [`SCIENTIFIC_FROM`].
const SCIENTIFIC_BELOW: f64 = 1e-5;

/// Write a number directly to the output buffer.
///
/// With `precision` of `None` the shortest representation that parses back to
/// the identical `f64` is written (plain decimal, or scientific notation for
/// very large or very small magnitudes). With `Some(n)` the value is rounded to
/// `n` decimal places: a bare integer when the value is whole, small enough
/// (|value| < 1e10) and round-trips through `i64`; otherwise a decimal with
/// trailing zeros trimmed. Zero, including negative zero and values that round
/// to zero, is always written as `0`.
///
/// `pub(crate)` so the MPS writer ([`crate::mps::writer`]) can reuse the same
/// numeric formatting instead of duplicating it.
#[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]
pub(crate) fn write_number(output: &mut String, value: f64, precision: Option<usize>) -> std::fmt::Result {
    debug_assert!(!value.is_nan(), "write_number called with NaN");
    if value.is_infinite() {
        // Infinite bounds are legitimate LP syntax; the lexer accepts `inf`/`-inf`.
        return output.write_str(if value > 0.0 { "inf" } else { "-inf" });
    }
    if value == 0.0 {
        // Covers -0.0, which would otherwise print as `-0`.
        return output.write_char('0');
    }

    let Some(precision) = precision else {
        let abs_value = value.abs();
        if abs_value < EXACT_INTEGER_LIMIT && value.fract() == 0.0 {
            // Below 2^53 a whole `f64` is an exact integer and its shortest
            // round-trip form is that integer's digits, as `{value}` prints.
            push_integer(output, value as i64);
            return Ok(());
        }
        return if (SCIENTIFIC_BELOW..SCIENTIFIC_FROM).contains(&abs_value) {
            write!(output, "{value}")
        } else {
            write!(output, "{value:e}")
        };
    };

    let is_whole_number = value.fract().abs() < f64::EPSILON;
    let is_safe_for_i64 = value >= (i64::MIN as f64) && value <= (i64::MAX as f64);

    if is_whole_number && is_safe_for_i64 && value.abs() < 1e10 {
        let cast = value as i64;
        debug_assert!((cast as f64 - value).abs() < 1.0, "i64 cast lost precision: {value} -> {cast}");
        push_integer(output, cast);
        Ok(())
    } else {
        let start = output.len();
        write!(output, "{value:.precision$}")?;
        if output[start..].contains('.') {
            let trimmed_len = start + output[start..].trim_end_matches('0').trim_end_matches('.').len();
            output.truncate(trimmed_len);
        }
        if &output[start..] == "-0" {
            // A tiny negative value rounded away entirely.
            output.truncate(start);
            output.push('0');
        }
        Ok(())
    }
}

/// 2^53: every whole `f64` of smaller magnitude is exactly representable as
/// an integer, and every integer up to it as an `f64`.
const EXACT_INTEGER_LIMIT: f64 = 9_007_199_254_740_992.0;

/// Append the decimal digits of `value`, as `write!(output, "{value}")` does.
#[allow(clippy::cast_possible_truncation)]
fn push_integer(output: &mut String, value: i64) {
    let mut digits = [0u8; 20];
    let mut start = digits.len();
    let mut rest = value.unsigned_abs();
    loop {
        start -= 1;
        // `rest % 10` is a single digit, so the cast cannot truncate.
        digits[start] = b'0' + (rest % 10) as u8;
        rest /= 10;
        if rest == 0 {
            break;
        }
    }
    if value < 0 {
        output.push('-');
    }
    output.push_str(std::str::from_utf8(&digits[start..]).expect("ASCII digits are valid UTF-8"));
}

/// Format a number with specified precision, removing trailing zeros.
/// Convenience wrapper around `write_number` for use in tests.
#[cfg(test)]
fn format_number(value: f64, precision: Option<usize>) -> String {
    let mut s = String::new();
    write_number(&mut s, value, precision).expect("write_number failed");
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Coefficient, ComparisonOp, Constraint, Objective, Sense, Variable, VariableBounds, VariableKind, VariableType};
    use crate::problem::LpProblem;

    #[test]
    #[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]
    fn whole_numbers_format_as_display_does() {
        let mut values = vec![1.0, 7.0, 10.0, 99.0, 100.0, 12_345.0, 1e15, 9_007_199_254_740_991.0, 9_007_199_254_740_992.0, 1e16];
        let mut x: u64 = 0x9E37_79B9_7F4A_7C15;
        for _ in 0..2000 {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            values.push((x >> (x % 60)) as f64);
        }
        for value in values.iter().filter(|v| **v != 0.0).flat_map(|v| [*v, -*v]) {
            let expected =
                if (SCIENTIFIC_BELOW..SCIENTIFIC_FROM).contains(&value.abs()) { format!("{value}") } else { format!("{value:e}") };
            assert_eq!(format_number(value, None), expected, "{value}");
            if value.abs() < 1e10 {
                assert_eq!(format_number(value, Some(3)), format!("{}", value as i64), "{value}");
            }
        }
        let mut digits = String::new();
        push_integer(&mut digits, i64::MIN);
        assert_eq!(digits, i64::MIN.to_string());
    }

    #[test]
    fn test_format_number() {
        assert_eq!(format_number(1.0, Some(6)), "1");
        assert_eq!(format_number(1.5, Some(6)), "1.5");
        assert_eq!(format_number(1.500_000, Some(6)), "1.5");
        assert_eq!(format_number(0.0, Some(6)), "0");
        assert_eq!(format_number(-1.0, Some(6)), "-1");
        assert_eq!(format_number(2.789, Some(2)), "2.79");
    }

    #[test]
    fn test_format_number_default_is_lossless() {
        assert_eq!(format_number(0.000_000_1, None), "1e-7");
        assert_eq!(format_number(1.234_567_89, None), "1.23456789");
        assert_eq!(format_number(3.0, None), "3");
        assert_eq!(format_number(-0.0, None), "0");
        assert_eq!(format_number(1e300, None), "1e300");
        assert_eq!(format_number(123_456_789_012.0, None), "123456789012");
        assert_eq!(format_number(0.1 + 0.2, None), "0.30000000000000004");
        // Negative zero, and anything rounding to it, never prints as `-0`.
        assert_eq!(format_number(-0.0, Some(6)), "0");
        assert_eq!(format_number(-0.000_000_1, Some(6)), "0");
    }

    #[test]
    #[allow(clippy::float_cmp)]
    fn test_default_options_round_trip_exact_coefficients() {
        let problem = LpProblem::parse("minimize\nobj: 0.0000001 x + 1.23456789 y\nsubject to\nc1: x + y >= 1e-12\nend\n").unwrap();
        let written = write_lp_string(&problem).unwrap();
        let reparsed = LpProblem::parse(&written).unwrap_or_else(|e| panic!("written LP must re-parse: {e}\n---\n{written}"));
        let obj = reparsed.objectives.values().next().unwrap();
        let values: Vec<f64> = obj.coefficients.iter().map(|c| c.value).collect();
        assert_eq!(values, vec![0.000_000_1, 1.234_567_89], "{written}");
        match reparsed.constraints.values().next().unwrap() {
            Constraint::Standard { rhs, .. } => assert_eq!(*rhs, 1e-12, "{written}"),
            _ => panic!("c1 must be a standard constraint"),
        }
    }

    #[test]
    fn test_format_coefficient() {
        fn fmt(name: &str, value: f64, is_first: bool, precision: Option<usize>) -> String {
            let mut buf = String::new();
            write_formatted_coefficient(&mut buf, name, value, is_first, precision).expect("write! to String cannot fail");
            buf
        }
        assert_eq!(fmt("x1", 1.0, true, None), "x1");
        assert_eq!(fmt("x2", -1.0, true, None), "- x2");
        assert_eq!(fmt("x3", 2.5, false, None), " + 2.5 x3");
        assert_eq!(fmt("x4", -3.7, false, None), " - 3.7 x4");
    }

    #[test]
    fn test_write_infinite_bounds() {
        let mut problem = LpProblem::new();
        let obj_id = problem.intern("obj");
        let x1 = problem.intern("x1");
        let x2 = problem.intern("x2");
        let x3 = problem.intern("x3");
        problem.add_objective(Objective {
            name: obj_id,
            coefficients: vec![Coefficient { name: x1, value: 1.0 }],
            constant: 0.0,
            quadratic: Vec::new(),
            attributes: crate::model::ObjectiveAttributes::default(),
            byte_offset: None,
        });
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

        let result = write_lp_string(&problem).unwrap();
        assert!(result.contains("x1 <= inf"), "got: {result}");
        assert!(result.contains("x2 >= -inf"), "got: {result}");
        assert!(result.contains("-inf <= x3 <= 5"), "got: {result}");

        // Infinite bounds must survive a round-trip.
        let reparsed = LpProblem::parse(&result).unwrap();
        let x1 = reparsed.name_id("x1").unwrap();
        let x2 = reparsed.name_id("x2").unwrap();
        let x3 = reparsed.name_id("x3").unwrap();
        assert_eq!(reparsed.variables[&x1].bounds, VariableBounds::upper(f64::INFINITY));
        assert_eq!(reparsed.variables[&x2].bounds, VariableBounds::lower(f64::NEG_INFINITY));
        assert_eq!(reparsed.variables[&x3].bounds, VariableBounds::range(f64::NEG_INFINITY, 5.0));
    }

    #[test]
    fn test_write_skips_empty_coefficient_expressions() {
        let mut problem = LpProblem::new();
        let obj_id = problem.intern("obj");
        let empty_obj_id = problem.intern("empty_obj");
        let x1 = problem.intern("x1");
        let c1 = problem.intern("c1");
        let empty_c = problem.intern("empty_c");
        problem.add_objective(Objective {
            name: obj_id,
            coefficients: vec![Coefficient { name: x1, value: 1.0 }],
            constant: 0.0,
            quadratic: Vec::new(),
            attributes: crate::model::ObjectiveAttributes::default(),
            byte_offset: None,
        });
        problem.add_objective(Objective {
            name: empty_obj_id,
            coefficients: vec![],
            constant: 0.0,
            quadratic: Vec::new(),
            attributes: crate::model::ObjectiveAttributes::default(),
            byte_offset: None,
        });
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

        let result = write_lp_string(&problem).unwrap();
        // Empty expressions cannot be represented in LP syntax; a dangling
        // ` name: ` line would make the output unparseable.
        assert!(!result.contains("empty_obj"), "got: {result}");
        assert!(!result.contains("empty_c"), "got: {result}");

        let reparsed = LpProblem::parse(&result).unwrap();
        assert_eq!(reparsed.objective_count(), 1);
        assert_eq!(reparsed.constraint_count(), 1);

        // The omissions are reported to the caller rather than printed.
        let (_, warnings) = write_lp_string_with_warnings(&problem, &LpWriterOptions::default()).unwrap();
        assert_eq!(warnings.len(), 2, "{warnings:?}");
        assert!(warnings[0].contains("empty_obj") && warnings[1].contains("empty_c"), "{warnings:?}");
    }

    #[test]
    fn test_write_empty_problem() {
        let problem = LpProblem::new();
        let result = write_lp_string(&problem).unwrap();

        assert!(result.contains("Minimize"));
        assert!(result.contains("End"));
    }

    #[test]
    fn test_check_lp_name() {
        for good in ["x", "x1", "e1", "E12", "x.y[1]", "a-b", "free_var", "stock", "_x"] {
            assert!(check_lp_name(good, "variable").is_ok(), "'{good}' is a valid LP identifier");
        }
        for bad in
            ["", "2x", "1e5", "free", "End", "st", "s.t.", "bin", "min", "S1", "inf", "a:b", "x<y", "a=b", "a+b", "a b", "x\\c", "x\n"]
        {
            assert!(check_lp_name(bad, "variable").is_err(), "'{bad}' must be rejected");
        }
    }

    #[test]
    fn test_unrepresentable_names_are_an_error_not_bad_output() {
        // An MPS column named `2x` used to be written as `obj: 2x + y`, which
        // re-parses as 2 * x.
        let mps = "NAME t\nROWS\n N obj\n L c1\nCOLUMNS\n    2x obj 1 c1 1\n    y obj 1 c1 1\nRHS\n    RHS c1 4\nENDATA\n";
        let problem = LpProblem::parse_mps(mps).expect("fixture must parse");
        let err = write_lp_string(&problem).expect_err("`2x` is not an LP identifier");
        assert!(err.to_string().contains("'2x'"), "error must name the offending variable: {err}");

        let mut problem = LpProblem::new();
        let free_id = problem.intern("free");
        let x_id = problem.intern("x");
        problem.add_constraint(Constraint::Standard {
            name: free_id,
            coefficients: vec![Coefficient { name: x_id, value: 1.0 }],
            operator: ComparisonOp::LTE,
            rhs: 1.0,
            byte_offset: None,
        });
        assert!(write_lp_string(&problem).is_err(), "a constraint named after a keyword must be rejected");

        let named = LpProblem::new().with_problem_name(String::from("two\nlines"));
        assert!(write_lp_string(&named).is_err(), "a line break in the problem name comment must be rejected");
        let options = LpWriterOptions { include_problem_name: false, ..LpWriterOptions::default() };
        assert!(write_lp_string_with_options(&named, &options).is_ok(), "the name is irrelevant when it is not written");
    }

    #[test]
    #[allow(clippy::float_cmp)]
    fn test_near_one_coefficient_keeps_its_value_in_exact_mode() {
        let mut out = String::new();
        write_formatted_coefficient(&mut out, "x", 1.000_000_000_01, true, None).unwrap();
        assert_eq!(out, "1.00000000001 x");
        out.clear();
        write_formatted_coefficient(&mut out, "x", -0.999_999_999_99, false, None).unwrap();
        assert_eq!(out, " - 0.99999999999 x");
        out.clear();
        write_formatted_coefficient(&mut out, "x", 1.0, true, None).unwrap();
        assert_eq!(out, "x");
        // A rounding precision collapses near-1 values to 1 anyway.
        out.clear();
        write_formatted_coefficient(&mut out, "x", 1.000_000_000_01, true, Some(6)).unwrap();
        assert_eq!(out, "x");

        let problem = LpProblem::parse("minimize\nobj: 1.00000000001 x\nsubject to\nc1: x <= 1\nend").unwrap();
        let reparsed = LpProblem::parse(&write_lp_string(&problem).unwrap()).unwrap();
        let obj = reparsed.objectives.values().next().unwrap();
        assert_eq!(obj.coefficients[0].value, 1.000_000_000_01, "exact mode must round-trip the coefficient");
    }

    #[test]
    fn test_non_finite_values_are_a_validation_error_not_a_panic() {
        fn problem_with(rhs: f64, coefficient: f64, bound: f64) -> LpProblem {
            let mut problem = LpProblem::parse("minimize\nobj: x\nsubject to\nc1: x <= 1\nend").expect("fixture must parse");
            let c1 = problem.name_id("c1").expect("c1 exists");
            let x = problem.name_id("x").expect("x exists");
            problem.constraints.insert(
                c1,
                Constraint::Standard {
                    name: c1,
                    coefficients: vec![Coefficient { name: x, value: coefficient }],
                    operator: ComparisonOp::LTE,
                    rhs,
                    byte_offset: None,
                },
            );
            problem.variables.get_mut(&x).expect("x registered").bounds.upper = Some(bound);
            problem
        }

        assert!(write_lp_string(&problem_with(1.0, 1.0, 4.0)).is_ok(), "finite control case must write");
        assert!(write_lp_string(&problem_with(1.0, 1.0, f64::INFINITY)).is_ok(), "an infinite bound is legal");
        assert!(write_lp_string(&problem_with(f64::INFINITY, 1.0, 4.0)).is_ok(), "an infinite RHS is legal");
        for (label, problem) in [
            ("NaN RHS", problem_with(f64::NAN, 1.0, 4.0)),
            ("NaN coefficient", problem_with(1.0, f64::NAN, 4.0)),
            ("infinite coefficient", problem_with(1.0, f64::INFINITY, 4.0)),
            ("NaN bound", problem_with(1.0, 1.0, f64::NAN)),
        ] {
            let err = write_lp_string(&problem).expect_err(label);
            assert!(matches!(err, LpParseError::ValidationError { .. }), "{label}: {err}");
            let err = crate::mps::writer::write_mps_string(&problem).expect_err(label);
            assert!(matches!(err, LpParseError::ValidationError { .. }), "{label}: {err}");
        }
    }

    #[test]
    fn test_problem_without_constraints_round_trips() {
        let problem = LpProblem::parse("min\n obj: x\nst\nbounds\n x <= 4\nend").expect("fixture must parse");
        assert_eq!(problem.constraint_count(), 0);
        let written = write_lp_string(&problem).unwrap();
        assert!(written.contains("Subject To"), "grammar requires the header:\n{written}");
        let reparsed = LpProblem::parse(&written).unwrap_or_else(|e| panic!("written LP must re-parse: {e}\n---\n{written}"));
        assert_eq!(reparsed.variable_count(), 1);
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
            constant: 0.0,
            quadratic: Vec::new(),
            attributes: crate::model::ObjectiveAttributes::default(),
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
            constant: 0.0,
            quadratic: Vec::new(),
            attributes: crate::model::ObjectiveAttributes::default(),
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
        assert_eq!(problem.variables.get(&x1_id).unwrap().kind, VariableKind::General);
        assert_eq!(problem.variables.get(&x2_id).unwrap().kind, VariableKind::General);

        // Write back to LP format
        let output = write_lp_string(&problem).unwrap();

        // Verify Generals section is present in the output
        assert!(output.contains("Generals"), "Output should contain a Generals section:\n{output}");
        assert!(output.contains("x1"), "Generals section should contain x1");
        assert!(output.contains("x2"), "Generals section should contain x2");

        // Re-parse and verify round-trip
        let reparsed = crate::problem::LpProblem::parse(&output).unwrap();
        let x1_id = reparsed.name_id("x1").unwrap();
        let x2_id = reparsed.name_id("x2").unwrap();
        assert_eq!(reparsed.variables.get(&x1_id).unwrap().kind, VariableKind::General);
        assert_eq!(reparsed.variables.get(&x2_id).unwrap().kind, VariableKind::General);
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
            assert_eq!((var.kind, var.bounds), (b.variables[&b_id].kind, b.variables[&b_id].bounds), "variable '{name}' type");
        }
    }

    /// The regression guard for the LP writer half of the free/undeclared
    /// conflation: an undeclared variable used to come back out as `x free`,
    /// which widened its feasible region to include negatives on every
    /// round-trip.
    #[test]
    fn an_undeclared_variable_is_not_written_as_free() {
        let source = "minimize\nobj: x + y\nsubject to\nc1: x + y >= 2\nbounds\ny free\nend\n";
        let problem = LpProblem::parse(source).expect("fixture must parse");
        let written = write_lp_string(&problem).unwrap();

        assert!(!written.contains("x free"), "x was never declared free:\n{written}");
        assert!(written.contains("y free"), "y was declared free and must stay so:\n{written}");

        // And it survives a second pass: x keeps LP's default, y keeps its
        // explicit freedom.
        let reparsed = LpProblem::parse(&written).expect("output must re-parse");
        let x = reparsed.name_id("x").expect("x present");
        let y = reparsed.name_id("y").expect("y present");
        assert!(reparsed.variables[&x].bounds.is_unspecified(), "x must stay undeclared");
        assert!(reparsed.variables[&y].bounds.is_free(), "y must stay free");
    }

    #[test]
    fn a_free_integer_variable_keeps_its_free_bound() {
        let source = "minimize\nobj: x + y\nsubject to\nc1: x + y >= -5\nbounds\nx free\ny free\ngenerals\nx\nintegers\ny\nend\n";
        let problem = LpProblem::parse(source).expect("fixture must parse");
        let written = write_lp_string(&problem).unwrap();

        assert!(written.contains("x free"), "general x was declared free:\n{written}");
        assert!(written.contains("y free"), "integer y was declared free:\n{written}");
        let reparsed = LpProblem::parse(&written).expect("output must re-parse");
        assert_problems_structurally_equal(&problem, &reparsed);
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

        // Sanity-check the parse picked up the interesting variable kinds and bounds.
        let var = |name: &str| original.variables[&original.name_id(name).unwrap()].clone();
        assert_eq!(var("x1").bounds, VariableBounds::lower(2.0));
        assert_eq!(var("x2").bounds, VariableBounds::upper(8.0));
        assert!(var("f").bounds.is_free());
        assert_eq!(var("g").bounds, VariableBounds::range(-1.5, 7.5));
        assert_eq!(var("b1").kind, VariableKind::Binary);
        assert_eq!(var("gen1").kind, VariableKind::General);
        assert_eq!(var("sc1").kind, VariableKind::SemiContinuous);
        assert_eq!(var("sw1").kind, VariableKind::Sos);

        let written = write_lp_string(&original).unwrap();
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
        problem.add_objective(Objective {
            name: obj_id,
            coefficients,
            constant: 0.0,
            quadratic: Vec::new(),
            attributes: crate::model::ObjectiveAttributes::default(),
            byte_offset: None,
        });
        let c1 = problem.intern("c1");
        let x0 = problem.name_id("very_long_variable_name_00").unwrap();
        problem.add_constraint(Constraint::Standard {
            name: c1,
            coefficients: vec![Coefficient { name: x0, value: 1.0 }],
            operator: ComparisonOp::GTE,
            rhs: 1.0,
            byte_offset: None,
        });

        let written = write_lp_string(&problem).unwrap();

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
        problem.add_objective(Objective {
            name: obj_id,
            coefficients: vec![Coefficient { name: x_id, value: 1.0 }],
            constant: 0.0,
            quadratic: Vec::new(),
            attributes: crate::model::ObjectiveAttributes::default(),
            byte_offset: None,
        });
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

        let written = write_lp_string(&problem).unwrap();

        // The Binaries section must span multiple lines under the 80-char limit.
        let binaries_start = written.find("Binaries").expect("Binaries section present");
        let section = &written[binaries_start..written.find("\nEnd").unwrap_or(written.len())];
        assert!(section.trim_end().lines().count() > 2, "Binaries section should wrap over multiple lines:\n{section}");

        let reparsed = LpProblem::parse(&written).unwrap_or_else(|e| panic!("wrapped LP must re-parse: {e}\n---\n{written}"));
        for name in &names {
            let id = reparsed.name_id(name).unwrap_or_else(|| panic!("binary '{name}' missing after round-trip"));
            assert_eq!(reparsed.variables[&id].kind, VariableKind::Binary, "variable '{name}'");
        }
    }

    #[test]
    fn test_write_number_large_whole_values() {
        // Whole values at or above 1e10 take the decimal path; trailing zeros
        // and the decimal point must be trimmed away.
        assert_eq!(format_number(1e12, Some(6)), "1000000000000");
        assert_eq!(format_number(-1e12, Some(6)), "-1000000000000");
        // Just below the 1e10 threshold: the integer fast path.
        assert_eq!(format_number(9_999_999_999.0, Some(6)), "9999999999");
    }

    #[test]
    fn test_writer_options_take_effect() {
        let input = "Minimize\n obj: 2.789 x + 1.111 y\nSubject To\n c1: x + y <= 3.14159\nEnd";
        let problem = LpProblem::parse(input).unwrap().with_problem_name(String::from("Opts"));

        let options = LpWriterOptions {
            include_problem_name: false,
            max_line_length: 10,
            decimal_precision: Some(2),
            include_section_spacing: false,
        };
        let written = write_lp_string_with_options(&problem, &options).unwrap();

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
        let default_written = write_lp_string(&problem).unwrap();
        assert!(default_written.contains("\\Problem name: Opts"));
        assert!(default_written.contains("\n\n"));
    }

    #[test]
    fn test_sos_s2_write_and_reparse() {
        let input = "Minimize\n obj: x + y\nSubject To\n c1: x + y >= 1\nSOS\n s2c: S2:: x:1 y:2.5\nEnd";
        let problem = LpProblem::parse(input).unwrap();

        let written = write_lp_string(&problem).unwrap();
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
            _ => panic!("s2c must re-parse as an SOS constraint"),
        }
    }

    #[test]
    fn test_multiple_objectives_round_trip() {
        let input = "Minimize\n obj1: x + 2 y\n obj2: 3 x\nSubject To\n c1: x + y >= 1\nEnd";
        let problem = LpProblem::parse(input).unwrap();
        assert_eq!(problem.objective_count(), 2);

        let written = write_lp_string(&problem).unwrap();
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

        let result = write_lp_string(&problem).unwrap();

        assert!(result.contains("Subject To"));
        assert!(result.contains("sos1: S1:: x1:1 x2:2 x3:3"));
        assert!(result.contains("End"));
    }
}
