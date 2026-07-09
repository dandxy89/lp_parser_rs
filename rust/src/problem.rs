use std::fmt::{Display, Formatter, Result as FmtResult, Write as _};

use indexmap::IndexMap;
use indexmap::map::Entry;

use crate::NUMERIC_EPSILON;
use crate::error::{LpParseError, LpResult};
use crate::interner::{NameId, NameInterner};
use crate::lexer::{Lexer, ParseResult, RawCoefficient, RawConstraint, RawObjective};
use crate::lp::LpProblemParser;
use crate::model::{Coefficient, Constraint, Objective, Sense, Variable, VariableType};
use crate::mps::{extract_mps_name, parse_mps};

/// Check if a floating-point value is effectively zero using both absolute
/// and relative epsilon comparisons.
#[inline]
fn is_effectively_zero(value: f64, reference: f64) -> bool {
    debug_assert!(value.is_finite(), "is_effectively_zero called with non-finite value: {value}");
    debug_assert!(reference.is_finite(), "is_effectively_zero called with non-finite reference: {reference}");
    let abs_value = value.abs();
    let abs_reference = reference.abs();

    if abs_value < f64::EPSILON {
        return true;
    }

    if abs_reference > f64::EPSILON {
        return abs_value < abs_reference * NUMERIC_EPSILON;
    }

    false
}

/// Apply a variable type to names, interning each and updating the variable map.
/// Only overrides if the variable doesn't already have explicit bounds set.
#[inline]
fn apply_variable_type(interner: &mut NameInterner, variables: &mut IndexMap<NameId, Variable>, names: &[&str], var_type: &VariableType) {
    for &name in names {
        let id = interner.intern(name);
        match variables.entry(id) {
            Entry::Occupied(mut entry) => {
                if matches!(entry.get().var_type, VariableType::Free) {
                    entry.get_mut().set_var_type(var_type.clone());
                }
            }
            Entry::Vacant(entry) => {
                entry.insert(Variable::new(id).with_var_type(var_type.clone()));
            }
        }
    }
}

/// Update a coefficient in a vector using index-based `swap_remove`.
#[inline]
fn update_coefficient_vec(coefficients: &mut Vec<Coefficient>, variable_id: NameId, new_value: f64) {
    if let Some(idx) = coefficients.iter().position(|c| c.name == variable_id) {
        let reference_value = coefficients[idx].value;
        if is_effectively_zero(new_value, reference_value) {
            coefficients.swap_remove(idx);
        } else {
            coefficients[idx].value = new_value;
        }
    } else if !is_effectively_zero(new_value, 1.0) {
        coefficients.push(Coefficient { name: variable_id, value: new_value });
    }
}

/// Extract the problem name from LP file header comments.
///
/// Supports multiple formats:
/// 1. `\Problem name: my_problem` or `\\Problem name: my_problem`
/// 2. `\* my_problem *\` (CPLEX block comment style)
///
/// Only the leading comment block is scanned: the scan stops at the first
/// non-comment, non-blank line, so files without a name comment don't pay
/// for a full-file scan.
fn extract_problem_name(input: &str) -> Option<String> {
    for line in input.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if !trimmed.starts_with('\\') {
            return None;
        }

        // Handle block comment format: \* name *\
        if let Some(inner) = trimmed.strip_prefix("\\*").and_then(|s| s.strip_suffix("*\\")) {
            let name = inner.trim();
            if !name.is_empty() {
                return Some(name.to_string());
            }
        }

        // Handle single/double backslash prefix
        let content = trimmed.strip_prefix("\\\\").or_else(|| trimmed.strip_prefix('\\'));

        if let Some(c) = content {
            let c = c.trim();
            let prefix = "problem name:";
            if c.get(..prefix.len()).is_some_and(|head| head.eq_ignore_ascii_case(prefix)) {
                return Some(c[prefix.len()..].trim().to_string());
            }
        }
    }
    None
}

/// Register variables from coefficient lists into the variables map.
#[inline]
fn register_variables_from_coefficients(
    variables: &mut IndexMap<NameId, Variable>,
    coefficients: &[Coefficient],
    var_type: Option<&VariableType>,
) {
    for coeff in coefficients {
        variables.entry(coeff.name).or_insert_with(|| {
            let v = Variable::new(coeff.name);
            if let Some(vt) = var_type { v.with_var_type(vt.clone()) } else { v }
        });
    }
}

/// Intern a slice of raw coefficients into model coefficients.
///
/// Repeated terms for the same variable (`x + x`) are summed, matching
/// solver LP readers; first-occurrence order is preserved.
#[inline]
fn intern_coefficients(interner: &mut NameInterner, raw: &[RawCoefficient<'_>]) -> Vec<Coefficient> {
    let mut merged: IndexMap<NameId, f64> = IndexMap::with_capacity(raw.len());
    for rc in raw {
        *merged.entry(interner.intern(rc.name)).or_insert(0.0) += rc.value;
    }
    merged.into_iter().map(|(name, value)| Coefficient { name, value }).collect()
}

/// Intern a raw constraint into a model constraint.
#[inline]
fn intern_constraint(interner: &mut NameInterner, raw: &RawConstraint<'_>) -> Constraint {
    match raw {
        RawConstraint::Standard { name, coefficients, operator, rhs, byte_offset } => Constraint::Standard {
            name: interner.intern(name),
            coefficients: intern_coefficients(interner, coefficients),
            operator: *operator,
            rhs: *rhs,
            byte_offset: *byte_offset,
        },
        RawConstraint::SOS { name, sos_type, weights, byte_offset } => Constraint::SOS {
            name: interner.intern(name),
            sos_type: *sos_type,
            weights: intern_coefficients(interner, weights),
            byte_offset: *byte_offset,
        },
    }
}

/// Intern a raw objective into a model objective.
#[inline]
fn intern_objective(interner: &mut NameInterner, raw: &RawObjective<'_>) -> Objective {
    Objective {
        name: interner.intern(&raw.name),
        coefficients: intern_coefficients(interner, &raw.coefficients),
        constant: raw.constant,
        byte_offset: raw.byte_offset,
    }
}

/// Represents a Linear Programming (LP) problem.
///
/// All name strings are stored in the embedded [`NameInterner`] and referenced
/// by [`NameId`] throughout. This eliminates lifetime constraints and avoids
/// string duplication.
#[derive(Clone, Debug, Default)]
pub struct LpProblem {
    /// The problem name (from comments), not interned.
    pub name: Option<String>,
    /// The optimisation sense (minimise/maximise).
    pub sense: Sense,
    /// Objectives keyed by interned name.
    pub objectives: IndexMap<NameId, Objective>,
    /// Constraints keyed by interned name.
    pub constraints: IndexMap<NameId, Constraint>,
    /// Variables keyed by interned name.
    pub variables: IndexMap<NameId, Variable>,
    /// The name interner holding all interned strings.
    pub interner: NameInterner,
}

impl LpProblem {
    #[must_use]
    #[inline]
    /// Create a new empty `LpProblem`.
    pub fn new() -> Self {
        Self::default()
    }

    /// Intern a name string, returning its [`NameId`].
    /// Convenience wrapper around `self.interner.intern()`.
    #[inline]
    pub fn intern(&mut self, name: &str) -> NameId {
        self.interner.intern(name)
    }

    /// Resolve a [`NameId`] back to its string.
    /// Convenience wrapper around `self.interner.resolve()`.
    #[inline]
    #[must_use]
    pub fn resolve(&self, id: NameId) -> &str {
        self.interner.resolve(id)
    }

    /// Look up a [`NameId`] for a name string without interning.
    /// Returns `None` if the name has not been interned.
    #[inline]
    #[must_use]
    pub fn name_id(&self, name: &str) -> Option<NameId> {
        self.interner.get(name)
    }

    /// Ensure a variable exists in the problem, creating it with the given type if not present.
    #[inline]
    fn ensure_variable_exists(&mut self, name_id: NameId, var_type: Option<VariableType>) {
        if let Entry::Vacant(entry) = self.variables.entry(name_id) {
            let variable = var_type.map_or_else(|| Variable::new(name_id), |vt| Variable::new(name_id).with_var_type(vt));
            entry.insert(variable);
        }
    }

    #[must_use]
    #[inline]
    /// Override the problem name.
    pub fn with_problem_name(self, problem_name: impl Into<String>) -> Self {
        Self { name: Some(problem_name.into()), ..self }
    }

    #[must_use]
    #[inline]
    /// Override the problem sense.
    pub fn with_sense(self, sense: Sense) -> Self {
        Self { sense, ..self }
    }

    #[must_use]
    #[inline]
    /// Returns the name of the LP Problem.
    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    #[must_use]
    #[inline]
    /// Returns the number of constraints.
    pub fn constraint_count(&self) -> usize {
        self.constraints.len()
    }

    #[must_use]
    #[inline]
    /// Returns the number of objectives.
    pub fn objective_count(&self) -> usize {
        self.objectives.len()
    }

    #[must_use]
    #[inline]
    /// Returns the number of variables.
    pub fn variable_count(&self) -> usize {
        self.variables.len()
    }

    #[inline]
    /// Parse a `LpProblem` from a string slice (LP format).
    ///
    /// # Example
    ///
    /// ```rust
    /// use lp_parser_rs::LpProblem;
    ///
    /// let problem = LpProblem::parse("Minimize\n obj: x + 2 y\nSubject To\n c1: x + y >= 1\nEnd")?;
    /// assert_eq!(problem.variable_count(), 2);
    /// assert_eq!(problem.constraint_count(), 1);
    /// # Ok::<(), lp_parser_rs::LpParseError>(())
    /// ```
    ///
    /// # Errors
    ///
    /// Returns an error if the input string is not a valid LP file format.
    pub fn parse(input: &str) -> LpResult<Self> {
        Self::try_from(input)
    }

    /// Parse a `LpProblem` from an MPS-format string.
    ///
    /// # Example
    ///
    /// ```rust
    /// use lp_parser_rs::LpProblem;
    ///
    /// let input = "\
    /// NAME          example
    /// ROWS
    ///  N  COST
    ///  L  LIM1
    /// COLUMNS
    ///     X         COST      1.0   LIM1      1.0
    ///     Y         COST      2.0   LIM1      1.0
    /// RHS
    ///     RHS       LIM1      4.0
    /// ENDATA
    /// ";
    ///
    /// let problem = LpProblem::parse_mps(input)?;
    /// assert_eq!(problem.name(), Some("example"));
    /// assert_eq!(problem.variable_count(), 2);
    /// # Ok::<(), lp_parser_rs::LpParseError>(())
    /// ```
    ///
    /// # Errors
    ///
    /// Returns an error if the input string is not valid MPS format.
    pub fn parse_mps(input: &str) -> LpResult<Self> {
        // Empty-input validation is owned by the inner MPS parser.
        let problem_name = extract_mps_name(input);
        let parsed = parse_mps(input)?;
        Ok(from_parse_result(parsed, problem_name))
    }

    #[inline]
    /// Add a new variable to the problem.
    ///
    /// If a variable with the same name already exists, it will be replaced.
    pub fn add_variable(&mut self, variable: Variable) {
        debug_assert!(!self.interner.resolve(variable.name).is_empty(), "variable name must not be empty");
        self.variables.insert(variable.name, variable);
    }

    #[inline]
    /// Add a new constraint to the problem.
    ///
    /// If a constraint with the same name already exists, it will be replaced.
    pub fn add_constraint(&mut self, constraint: Constraint) {
        debug_assert!(!self.interner.resolve(constraint.name()).is_empty(), "constraint name must not be empty");
        let name_id = constraint.name();

        match &constraint {
            Constraint::Standard { coefficients, .. } => {
                for coeff in coefficients {
                    self.ensure_variable_exists(coeff.name, None);
                }
            }
            Constraint::SOS { weights, .. } => {
                for coeff in weights {
                    self.ensure_variable_exists(coeff.name, Some(VariableType::SOS));
                }
            }
        }

        self.constraints.insert(name_id, constraint);
    }

    #[inline]
    /// Add a new objective to the problem.
    ///
    /// If an objective with the same name already exists, it will be replaced.
    pub fn add_objective(&mut self, objective: Objective) {
        debug_assert!(!self.interner.resolve(objective.name).is_empty(), "objective name must not be empty");
        for coeff in &objective.coefficients {
            self.ensure_variable_exists(coeff.name, None);
        }

        let name_id = objective.name;
        self.objectives.insert(name_id, objective);
    }

    // LP Problem Modification Methods

    /// Update a variable coefficient in an objective.
    ///
    /// # Errors
    ///
    /// Returns an error if the specified objective does not exist.
    pub fn update_objective_coefficient(&mut self, objective_name: &str, variable_name: &str, new_coefficient: f64) -> LpResult<()> {
        debug_assert!(!variable_name.is_empty(), "variable_name must not be empty");
        debug_assert!(new_coefficient.is_finite(), "new_coefficient must be finite, got: {new_coefficient}");

        let obj_id = self
            .interner
            .get(objective_name)
            .ok_or_else(|| LpParseError::validation_error(format!("Objective '{objective_name}' not found")))?;

        let var_id = self.interner.intern(variable_name);

        let objective = self
            .objectives
            .get_mut(&obj_id)
            .ok_or_else(|| LpParseError::validation_error(format!("Objective '{objective_name}' not found")))?;

        update_coefficient_vec(&mut objective.coefficients, var_id, new_coefficient);

        if !is_effectively_zero(new_coefficient, 1.0) {
            self.variables.entry(var_id).or_insert_with(|| Variable::new(var_id));
        }

        Ok(())
    }

    /// Update a variable coefficient in a constraint.
    ///
    /// # Errors
    ///
    /// Returns an error if the constraint does not exist or is an SOS constraint.
    pub fn update_constraint_coefficient(&mut self, constraint_name: &str, variable_name: &str, new_coefficient: f64) -> LpResult<()> {
        debug_assert!(!variable_name.is_empty(), "variable_name must not be empty");
        debug_assert!(new_coefficient.is_finite(), "new_coefficient must be finite, got: {new_coefficient}");

        let con_id = self
            .interner
            .get(constraint_name)
            .ok_or_else(|| LpParseError::validation_error(format!("Constraint '{constraint_name}' not found")))?;

        let var_id = self.interner.intern(variable_name);

        let constraint = self
            .constraints
            .get_mut(&con_id)
            .ok_or_else(|| LpParseError::validation_error(format!("Constraint '{constraint_name}' not found")))?;

        match constraint {
            Constraint::Standard { coefficients, .. } => {
                update_coefficient_vec(coefficients, var_id, new_coefficient);

                if !is_effectively_zero(new_coefficient, 1.0) {
                    self.variables.entry(var_id).or_insert_with(|| Variable::new(var_id));
                }
            }
            Constraint::SOS { .. } => {
                return Err(LpParseError::validation_error("Cannot update coefficients in SOS constraints using this method"));
            }
        }

        Ok(())
    }

    /// Update the right-hand side value of a constraint.
    ///
    /// # Example
    ///
    /// Parse, mutate, and write back — the pattern shared by the whole
    /// mutation API (`rename_*`, `update_*`, `remove_*`):
    ///
    /// ```rust
    /// use lp_parser_rs::LpProblem;
    /// use lp_parser_rs::writer::write_lp_string;
    ///
    /// let mut problem = LpProblem::parse("Minimize\n obj: x\nSubject To\n c1: x >= 1\nEnd")?;
    /// problem.update_constraint_rhs("c1", 5.0)?;
    /// assert!(write_lp_string(&problem).contains("c1: x >= 5"));
    /// # Ok::<(), lp_parser_rs::LpParseError>(())
    /// ```
    ///
    /// # Errors
    ///
    /// Returns an error if the constraint does not exist or is an SOS constraint.
    pub fn update_constraint_rhs(&mut self, constraint_name: &str, new_rhs: f64) -> LpResult<()> {
        debug_assert!(!constraint_name.is_empty(), "constraint_name must not be empty");
        debug_assert!(new_rhs.is_finite(), "new_rhs must be finite, got: {new_rhs}");
        let con_id = self
            .interner
            .get(constraint_name)
            .ok_or_else(|| LpParseError::validation_error(format!("Constraint '{constraint_name}' not found")))?;

        let constraint = self
            .constraints
            .get_mut(&con_id)
            .ok_or_else(|| LpParseError::validation_error(format!("Constraint '{constraint_name}' not found")))?;

        match constraint {
            Constraint::Standard { rhs, .. } => {
                *rhs = new_rhs;
                Ok(())
            }
            Constraint::SOS { .. } => Err(LpParseError::validation_error("SOS constraints do not have right-hand side values")),
        }
    }

    /// Rename a variable throughout the entire problem.
    ///
    /// # Errors
    ///
    /// Returns an error if the variable does not exist or the new name is already in use.
    ///
    /// # Panics
    ///
    /// Panics if the internal state is inconsistent (variable passed filter but missing from map).
    pub fn rename_variable(&mut self, old_name: &str, new_name: &str) -> LpResult<()> {
        debug_assert!(!old_name.is_empty(), "old_name must not be empty");
        debug_assert!(!new_name.is_empty(), "new_name must not be empty");
        let old_id = self
            .interner
            .get(old_name)
            .filter(|id| self.variables.contains_key(id))
            .ok_or_else(|| LpParseError::validation_error(format!("Variable '{old_name}' not found")))?;

        let new_id = self.interner.intern(new_name);

        if old_id != new_id && self.variables.contains_key(&new_id) {
            return Err(LpParseError::validation_error(format!("Variable '{new_name}' already exists")));
        }

        let variable = self.variables.shift_remove(&old_id).expect("variable must exist: filter check passed");
        let mut new_variable = Variable::new(new_id);
        new_variable.var_type = variable.var_type;
        self.variables.insert(new_id, new_variable);

        // PERF: O(n*m) scan over all objectives and constraints to rename the variable.
        // Acceptable because rename is infrequent in typical LP workflows. For mutation-heavy
        // workloads, prefer batch operations or maintain a reverse index.
        for objective in self.objectives.values_mut() {
            for coeff in &mut objective.coefficients {
                if coeff.name == old_id {
                    coeff.name = new_id;
                }
            }
        }

        for constraint in self.constraints.values_mut() {
            match constraint {
                Constraint::Standard { coefficients, .. } => {
                    for coeff in coefficients {
                        if coeff.name == old_id {
                            coeff.name = new_id;
                        }
                    }
                }
                Constraint::SOS { weights, .. } => {
                    for weight in weights {
                        if weight.name == old_id {
                            weight.name = new_id;
                        }
                    }
                }
            }
        }

        debug_assert!(!self.variables.contains_key(&old_id), "postcondition: old_id must be gone from variables");
        debug_assert!(self.variables.contains_key(&new_id), "postcondition: new_id must be present in variables");
        Ok(())
    }

    /// Rename a constraint.
    ///
    /// # Errors
    ///
    /// Returns an error if the constraint does not exist or the new name is already in use.
    ///
    /// # Panics
    ///
    /// Panics if the internal state is inconsistent (constraint passed filter but missing from map).
    pub fn rename_constraint(&mut self, old_name: &str, new_name: &str) -> LpResult<()> {
        debug_assert!(!old_name.is_empty(), "old_name must not be empty");
        debug_assert!(!new_name.is_empty(), "new_name must not be empty");
        let old_id = self
            .interner
            .get(old_name)
            .filter(|id| self.constraints.contains_key(id))
            .ok_or_else(|| LpParseError::validation_error(format!("Constraint '{old_name}' not found")))?;

        let new_id = self.interner.intern(new_name);

        if old_id != new_id && self.constraints.contains_key(&new_id) {
            return Err(LpParseError::validation_error(format!("Constraint '{new_name}' already exists")));
        }

        let mut constraint = self.constraints.shift_remove(&old_id).expect("constraint must exist: filter check passed");

        match &mut constraint {
            Constraint::Standard { name, .. } | Constraint::SOS { name, .. } => {
                *name = new_id;
            }
        }

        self.constraints.insert(new_id, constraint);

        debug_assert!(!self.constraints.contains_key(&old_id), "postcondition: old_id must be gone from constraints");
        debug_assert!(self.constraints.contains_key(&new_id), "postcondition: new_id must be present in constraints");
        Ok(())
    }

    /// Rename an objective.
    ///
    /// # Errors
    ///
    /// Returns an error if the objective does not exist or the new name is already in use.
    ///
    /// # Panics
    ///
    /// Panics if the internal state is inconsistent (objective passed filter but missing from map).
    pub fn rename_objective(&mut self, old_name: &str, new_name: &str) -> LpResult<()> {
        debug_assert!(!old_name.is_empty(), "old_name must not be empty");
        debug_assert!(!new_name.is_empty(), "new_name must not be empty");
        let old_id = self
            .interner
            .get(old_name)
            .filter(|id| self.objectives.contains_key(id))
            .ok_or_else(|| LpParseError::validation_error(format!("Objective '{old_name}' not found")))?;

        let new_id = self.interner.intern(new_name);

        if old_id != new_id && self.objectives.contains_key(&new_id) {
            return Err(LpParseError::validation_error(format!("Objective '{new_name}' already exists")));
        }

        let mut objective = self.objectives.shift_remove(&old_id).expect("objective must exist: filter check passed");
        objective.name = new_id;
        self.objectives.insert(new_id, objective);

        debug_assert!(!self.objectives.contains_key(&old_id), "postcondition: old_id must be gone from objectives");
        debug_assert!(self.objectives.contains_key(&new_id), "postcondition: new_id must be present in objectives");
        Ok(())
    }

    /// Remove a variable from the entire problem.
    ///
    /// # Errors
    ///
    /// Returns an error if the variable does not exist.
    pub fn remove_variable(&mut self, variable_name: &str) -> LpResult<()> {
        debug_assert!(!variable_name.is_empty(), "variable_name must not be empty");
        let var_id = self
            .interner
            .get(variable_name)
            .filter(|id| self.variables.contains_key(id))
            .ok_or_else(|| LpParseError::validation_error(format!("Variable '{variable_name}' not found")))?;

        self.variables.shift_remove(&var_id);

        // PERF: O(n*m) scan over all objectives and constraints to remove the variable.
        // Acceptable because removal is infrequent in typical LP workflows. For mutation-heavy
        // workloads, prefer batch operations or maintain a reverse index.
        for objective in self.objectives.values_mut() {
            objective.coefficients.retain(|c| c.name != var_id);
        }

        for constraint in self.constraints.values_mut() {
            match constraint {
                Constraint::Standard { coefficients, .. } => {
                    coefficients.retain(|c| c.name != var_id);
                }
                Constraint::SOS { weights, .. } => {
                    weights.retain(|w| w.name != var_id);
                }
            }
        }

        debug_assert!(!self.variables.contains_key(&var_id), "postcondition: variable must be removed");
        Ok(())
    }

    /// Remove a constraint from the problem.
    ///
    /// # Errors
    ///
    /// Returns an error if the constraint does not exist.
    pub fn remove_constraint(&mut self, constraint_name: &str) -> LpResult<()> {
        debug_assert!(!constraint_name.is_empty(), "constraint_name must not be empty");
        let con_id = self
            .interner
            .get(constraint_name)
            .ok_or_else(|| LpParseError::validation_error(format!("Constraint '{constraint_name}' not found")))?;

        if self.constraints.shift_remove(&con_id).is_none() {
            return Err(LpParseError::validation_error(format!("Constraint '{constraint_name}' not found")));
        }
        Ok(())
    }

    /// Remove an objective from the problem.
    ///
    /// # Errors
    ///
    /// Returns an error if the objective does not exist.
    pub fn remove_objective(&mut self, objective_name: &str) -> LpResult<()> {
        debug_assert!(!objective_name.is_empty(), "objective_name must not be empty");
        let obj_id = self
            .interner
            .get(objective_name)
            .ok_or_else(|| LpParseError::validation_error(format!("Objective '{objective_name}' not found")))?;

        if self.objectives.shift_remove(&obj_id).is_none() {
            return Err(LpParseError::validation_error(format!("Objective '{objective_name}' not found")));
        }
        Ok(())
    }

    /// Update the type of a variable.
    ///
    /// # Errors
    ///
    /// Returns an error if the variable does not exist.
    pub fn update_variable_type(&mut self, variable_name: &str, new_type: VariableType) -> LpResult<()> {
        debug_assert!(!variable_name.is_empty(), "variable_name must not be empty");
        let var_id = self
            .interner
            .get(variable_name)
            .ok_or_else(|| LpParseError::validation_error(format!("Variable '{variable_name}' not found")))?;

        let variable = self
            .variables
            .get_mut(&var_id)
            .ok_or_else(|| LpParseError::validation_error(format!("Variable '{variable_name}' not found")))?;

        variable.var_type = new_type;
        Ok(())
    }
}

// Custom Serialize/Deserialize that resolves NameId → String on output and
// interns String → NameId on input. Inner model types (Coefficient, Constraint,
// Objective, Variable) are serialised through LpProblem — they don't need
// standalone serde impls.
#[cfg(feature = "serde")]
mod serde_support {
    use indexmap::IndexMap;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    use crate::interner::{NameId, NameInterner};
    use crate::model::{Coefficient, ComparisonOp, Constraint, Objective, SOSType, Sense, Variable, VariableType};
    use crate::problem::LpProblem;

    #[derive(Serialize, Deserialize)]
    struct SerdeCoefficient {
        name: String,
        value: f64,
    }

    #[derive(Serialize, Deserialize)]
    #[serde(tag = "type")]
    enum SerdeConstraint {
        Standard { name: String, coefficients: Vec<SerdeCoefficient>, operator: ComparisonOp, rhs: f64 },
        Sos { name: String, sos_type: SOSType, weights: Vec<SerdeCoefficient> },
    }

    #[derive(Serialize, Deserialize)]
    struct SerdeObjective {
        name: String,
        coefficients: Vec<SerdeCoefficient>,
        // Default keeps pre-constant serialised problems deserialisable.
        #[serde(default)]
        constant: f64,
    }

    #[derive(Serialize, Deserialize)]
    struct SerdeVariable {
        name: String,
        var_type: VariableType,
    }

    #[derive(Serialize, Deserialize)]
    struct SerdeLpProblem {
        #[serde(skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        sense: Sense,
        objectives: Vec<SerdeObjective>,
        constraints: Vec<SerdeConstraint>,
        variables: Vec<SerdeVariable>,
    }

    fn coeff_to_serde(c: &Coefficient, interner: &NameInterner) -> SerdeCoefficient {
        SerdeCoefficient { name: interner.resolve(c.name).to_string(), value: c.value }
    }

    fn coeffs_to_serde(coeffs: &[Coefficient], interner: &NameInterner) -> Vec<SerdeCoefficient> {
        coeffs.iter().map(|c| coeff_to_serde(c, interner)).collect()
    }

    fn coeff_from_serde(sc: &SerdeCoefficient, interner: &mut NameInterner) -> Coefficient {
        Coefficient { name: interner.intern(&sc.name), value: sc.value }
    }

    fn coeffs_from_serde(scs: &[SerdeCoefficient], interner: &mut NameInterner) -> Vec<Coefficient> {
        scs.iter().map(|sc| coeff_from_serde(sc, interner)).collect()
    }

    impl Serialize for LpProblem {
        fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
            let proxy = SerdeLpProblem {
                name: self.name.clone(),
                sense: self.sense.clone(),
                objectives: self
                    .objectives
                    .values()
                    .map(|obj| SerdeObjective {
                        name: self.interner.resolve(obj.name).to_string(),
                        coefficients: coeffs_to_serde(&obj.coefficients, &self.interner),
                        constant: obj.constant,
                    })
                    .collect(),
                constraints: self
                    .constraints
                    .values()
                    .map(|con| match con {
                        Constraint::Standard { name, coefficients, operator, rhs, .. } => SerdeConstraint::Standard {
                            name: self.interner.resolve(*name).to_string(),
                            coefficients: coeffs_to_serde(coefficients, &self.interner),
                            operator: *operator,
                            rhs: *rhs,
                        },
                        Constraint::SOS { name, sos_type, weights, .. } => SerdeConstraint::Sos {
                            name: self.interner.resolve(*name).to_string(),
                            sos_type: *sos_type,
                            weights: coeffs_to_serde(weights, &self.interner),
                        },
                    })
                    .collect(),
                variables: self
                    .variables
                    .values()
                    .map(|var| SerdeVariable { name: self.interner.resolve(var.name).to_string(), var_type: var.var_type.clone() })
                    .collect(),
            };
            proxy.serialize(serializer)
        }
    }

    impl<'de> Deserialize<'de> for LpProblem {
        fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
            let proxy = SerdeLpProblem::deserialize(deserializer)?;
            let mut interner = NameInterner::new();

            let objectives: IndexMap<NameId, Objective> = proxy
                .objectives
                .iter()
                .map(|so| {
                    let name_id = interner.intern(&so.name);
                    let obj = Objective {
                        name: name_id,
                        coefficients: coeffs_from_serde(&so.coefficients, &mut interner),
                        constant: so.constant,
                        byte_offset: None,
                    };
                    (name_id, obj)
                })
                .collect();

            let constraints: IndexMap<NameId, Constraint> = proxy
                .constraints
                .iter()
                .map(|sc| match sc {
                    SerdeConstraint::Standard { name, coefficients, operator, rhs } => {
                        let name_id = interner.intern(name);
                        let con = Constraint::Standard {
                            name: name_id,
                            coefficients: coeffs_from_serde(coefficients, &mut interner),
                            operator: *operator,
                            rhs: *rhs,
                            byte_offset: None,
                        };
                        (name_id, con)
                    }
                    SerdeConstraint::Sos { name, sos_type, weights } => {
                        let name_id = interner.intern(name);
                        let con = Constraint::SOS {
                            name: name_id,
                            sos_type: *sos_type,
                            weights: coeffs_from_serde(weights, &mut interner),
                            byte_offset: None,
                        };
                        (name_id, con)
                    }
                })
                .collect();

            let variables: IndexMap<NameId, Variable> = proxy
                .variables
                .iter()
                .map(|sv| {
                    let name_id = interner.intern(&sv.name);
                    let var = Variable::new(name_id).with_var_type(sv.var_type.clone());
                    (name_id, var)
                })
                .collect();

            Ok(Self { name: proxy.name, sense: proxy.sense, objectives, constraints, variables, interner })
        }
    }
}

impl Display for LpProblem {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        if let Some(problem_name) = self.name() {
            writeln!(f, "Problem name: {problem_name}")?;
        }
        writeln!(f, "Sense: {}", self.sense)?;
        writeln!(f, "Objectives: {}", self.objective_count())?;
        writeln!(f, "Constraints: {}", self.constraint_count())?;
        writeln!(f, "Variables: {}", self.variable_count())
    }
}

/// Convert a [`ParseResult`] into an [`LpProblem`], interning all names and
/// building the full model. Shared by both LP and MPS parse paths.
fn from_parse_result(parsed: ParseResult<'_>, problem_name: Option<String>) -> LpProblem {
    // An empty objective section with no constraints is degenerate but valid LP.
    debug_assert!(parsed.objectives.iter().all(|o| !o.name.is_empty()), "all objectives must have non-empty names");

    // Double the constraint count as a heuristic: one entry for the constraint
    // name itself, plus at least one new variable name per constraint on average.
    let estimated_names = parsed.objectives.len()
        + parsed.constraints.len() * 2
        + parsed.bounds.len()
        + parsed.generals.len()
        + parsed.integers.len()
        + parsed.binaries.len();
    let mut interner = NameInterner::with_capacity(estimated_names.max(16));

    let estimated_variables =
        parsed.bounds.len() + parsed.generals.len() + parsed.integers.len() + parsed.binaries.len() + parsed.constraints.len();
    let mut variables: IndexMap<NameId, Variable> = IndexMap::with_capacity(estimated_variables);
    let mut constraint_counter: u32 = 0;

    let objectives = intern_objectives(&mut interner, &parsed.objectives, &mut variables);
    let mut constraints = intern_constraints(&mut interner, &parsed.constraints, &mut variables, &mut constraint_counter);

    process_bounds(&mut interner, &parsed.bounds, &mut variables);
    process_variable_types(&mut interner, &parsed, &mut variables);
    intern_sos_constraints(&mut interner, &parsed.sos, &mut variables, &mut constraints, &mut constraint_counter);

    LpProblem { name: problem_name, sense: parsed.sense, objectives, constraints, variables, interner }
}

impl TryFrom<&str> for LpProblem {
    type Error = LpParseError;

    fn try_from(input: &str) -> Result<Self, Self::Error> {
        let problem_name = extract_problem_name(input);

        let lexer = Lexer::new(input);
        let parser = LpProblemParser::new();
        let parsed = parser.parse(lexer).map_err(LpParseError::from)?;

        Ok(from_parse_result(parsed, problem_name))
    }
}

/// Intern raw objectives, assigning auto-names to unnamed ones.
fn intern_objectives(
    interner: &mut NameInterner,
    raw_objectives: &[RawObjective<'_>],
    variables: &mut IndexMap<NameId, Variable>,
) -> IndexMap<NameId, Objective> {
    let mut objectives = IndexMap::with_capacity(raw_objectives.len());
    let mut obj_counter: u32 = 0;
    let mut name_buf = String::with_capacity(16);

    for raw_obj in raw_objectives {
        let mut obj = intern_objective(interner, raw_obj);

        if raw_obj.name == "__obj__" {
            obj_counter += 1;
            name_buf.clear();
            write!(name_buf, "OBJ{obj_counter}").expect("writing to String cannot fail");
            obj.name = interner.intern(&name_buf);
        }

        register_variables_from_coefficients(variables, &obj.coefficients, None);
        let name = obj.name;
        if objectives.insert(name, obj).is_some() {
            eprintln!("duplicate objective name '{}': the later definition replaces the earlier one", interner.resolve(name));
        }
    }

    objectives
}

/// Intern raw constraints, assigning auto-names to unnamed ones.
fn intern_constraints(
    interner: &mut NameInterner,
    raw_constraints: &[RawConstraint<'_>],
    variables: &mut IndexMap<NameId, Variable>,
    constraint_counter: &mut u32,
) -> IndexMap<NameId, Constraint> {
    let mut constraints = IndexMap::with_capacity(raw_constraints.len());
    let mut name_buf = String::with_capacity(16);

    for raw_con in raw_constraints {
        let mut con = intern_constraint(interner, raw_con);
        let final_id = assign_constraint_name(interner, &mut con, constraint_counter, "C", &mut name_buf);
        register_constraint_variables(variables, &con);
        if constraints.insert(final_id, con).is_some() {
            eprintln!("duplicate constraint name '{}': the later definition replaces the earlier one", interner.resolve(final_id));
        }
    }

    constraints
}

/// Process bounds declarations into the variables map.
fn process_bounds(interner: &mut NameInterner, bounds: &[(&str, VariableType)], variables: &mut IndexMap<NameId, Variable>) {
    for &(var_name, ref var_type) in bounds {
        let var_id = interner.intern(var_name);
        match variables.entry(var_id) {
            Entry::Occupied(mut entry) => {
                let merged = merge_bound_types(&entry.get().var_type, var_type);
                entry.get_mut().set_var_type(merged);
            }
            Entry::Vacant(entry) => {
                entry.insert(Variable::new(var_id).with_var_type(var_type.clone()));
            }
        }
    }
}

/// Merge a new bound declaration with a variable's existing type.
///
/// Complementary single-sided bounds (`x >= lb` on one line, `x <= ub` on
/// another) combine into a [`VariableType::DoubleBound`]; every other
/// combination keeps last-declaration-wins semantics.
fn merge_bound_types(existing: &VariableType, new: &VariableType) -> VariableType {
    match (existing, new) {
        (VariableType::LowerBound(lb), VariableType::UpperBound(ub)) | (VariableType::UpperBound(ub), VariableType::LowerBound(lb)) => {
            VariableType::DoubleBound(*lb, *ub)
        }
        _ => new.clone(),
    }
}

/// Apply generals, integers, binaries, and semi-continuous type declarations.
fn process_variable_types(interner: &mut NameInterner, parsed: &ParseResult<'_>, variables: &mut IndexMap<NameId, Variable>) {
    apply_variable_type(interner, variables, &parsed.generals, &VariableType::General);
    apply_variable_type(interner, variables, &parsed.integers, &VariableType::Integer);
    apply_variable_type(interner, variables, &parsed.binaries, &VariableType::Binary);
    apply_variable_type(interner, variables, &parsed.semi_continuous, &VariableType::SemiContinuous);
}

/// Intern SOS constraints and add them to the constraints map.
fn intern_sos_constraints(
    interner: &mut NameInterner,
    raw_sos: &[RawConstraint<'_>],
    variables: &mut IndexMap<NameId, Variable>,
    constraints: &mut IndexMap<NameId, Constraint>,
    constraint_counter: &mut u32,
) {
    let mut name_buf = String::with_capacity(16);
    for raw_sos_con in raw_sos {
        if matches!(raw_sos_con, RawConstraint::Standard { .. }) {
            continue;
        }
        let mut sos = intern_constraint(interner, raw_sos_con);
        let final_id = assign_constraint_name(interner, &mut sos, constraint_counter, "SOS", &mut name_buf);
        register_constraint_variables(variables, &sos);
        constraints.insert(final_id, sos);
    }
}

/// Assign a name to a constraint, generating one if unnamed.
/// Returns the final [`NameId`].
#[inline]
fn assign_constraint_name(
    interner: &mut NameInterner,
    constraint: &mut Constraint,
    counter: &mut u32,
    prefix: &str,
    name_buf: &mut String,
) -> NameId {
    let current_name = interner.resolve(constraint.name());
    let is_unnamed = current_name == "__c__" || current_name.is_empty();

    let final_id = if is_unnamed {
        *counter += 1;
        name_buf.clear();
        write!(name_buf, "{prefix}{}", *counter).expect("writing to String cannot fail");
        interner.intern(name_buf)
    } else {
        constraint.name()
    };

    match constraint {
        Constraint::Standard { name, .. } | Constraint::SOS { name, .. } => {
            *name = final_id;
        }
    }

    final_id
}

/// Register variables referenced by a constraint into the variables map.
#[inline]
fn register_constraint_variables(variables: &mut IndexMap<NameId, Variable>, constraint: &Constraint) {
    match constraint {
        Constraint::Standard { coefficients, .. } => {
            register_variables_from_coefficients(variables, coefficients, None);
        }
        Constraint::SOS { weights, .. } => {
            register_variables_from_coefficients(variables, weights, Some(&VariableType::SOS));
        }
    }
}

#[cfg(test)]
mod test {
    use crate::model::{Coefficient, ComparisonOp, Constraint, Objective, SOSType, Sense, Variable, VariableType};
    use crate::problem::LpProblem;

    const COMPLETE_INPUT: &str = "\\ This file has been generated by Author
\\ ENCODING=ISO-8859-1
\\Problem name: diet
Minimize
 obj1: -0.5 x - 2y - 8z
 obj2: y + x + z
 obj3: 10z - 2.5x
       + y
subject to:
c1:  3 x1 + x2 + 2 x3 = 30
c2:  2 x1 + x2 + 3 x3 + x4 >= 15
c3:  2 x2 + 3 x4 <= 25
bounds
x1 free
x2 >= 1
100 <= x2dfsdf <= -1
Integers
X31
X32
Generals
Route_A_1
Route_A_2
Route_A_3
Binary
V8
Semi-Continuous
 y
SOS
csos1: S1:: V1:1 V3:2 V5:3
csos2: S2:: V2:2 V4:1 V5:2.5
End";

    const SMALL_INPUT: &str = "\\ This file has been generated by Author
\\ ENCODING=ISO-8859-1
\\Problem name: diet
Minimize
 obj1: -0.5 x - 2y - 8z
 obj2: y + x + z
 obj3: 10z - 2.5x
       + y
subject to:
c1:  3 x1 + x2 + 2 x3 = 30
c2:  2 x1 + x2 + 3 x3 + x4 >= 15
c3:  2 x2 + 3 x4 <= 25
bounds
x1 free
x2 >= 1
100 <= x2dfsdf <= -1
End";

    #[test]
    fn test_parse_inputs() {
        let problem = LpProblem::try_from(SMALL_INPUT).unwrap();
        assert_eq!(problem.objectives.len(), 3);
        assert_eq!(problem.constraints.len(), 3);

        let problem = LpProblem::try_from(COMPLETE_INPUT).unwrap();
        assert_eq!(problem.objectives.len(), 3);
        assert_eq!(problem.constraints.len(), 5);
    }

    #[test]
    fn test_problem_lifecycle() {
        let problem = LpProblem::new();
        assert_eq!(problem.name(), None);
        assert_eq!(problem.sense, Sense::Minimize);
        assert_eq!((problem.objective_count(), problem.constraint_count(), problem.variable_count()), (0, 0, 0));

        let problem = LpProblem::new().with_problem_name("test").with_sense(Sense::Maximize);
        assert_eq!(problem.name(), Some("test"));
        assert_eq!(problem.sense, Sense::Maximize);

        let display = format!("{problem}");
        assert!(display.contains("Problem name: test") && display.contains("Sense: Maximize"));
    }

    #[test]
    fn test_add_and_replace_elements() {
        let mut problem = LpProblem::new();
        let x1 = problem.intern("x1");
        let x2 = problem.intern("x2");
        let x3 = problem.intern("x3");
        let s1 = problem.intern("s1");

        // Add variable
        problem.add_variable(Variable::new(x1).with_var_type(VariableType::Binary));
        assert_eq!(problem.variable_count(), 1);

        // Replace variable
        problem.add_variable(Variable::new(x1).with_var_type(VariableType::Integer));
        assert_eq!(problem.variable_count(), 1);
        assert_eq!(problem.variables[&x1].var_type, VariableType::Integer);

        // Add constraint (auto-creates variables)
        let c1 = problem.intern("c1");
        problem.add_constraint(Constraint::Standard {
            name: c1,
            coefficients: vec![Coefficient { name: x1, value: 1.0 }, Coefficient { name: x2, value: 2.0 }],
            operator: ComparisonOp::LTE,
            rhs: 5.0,
            byte_offset: None,
        });
        assert_eq!(problem.constraint_count(), 1);
        assert_eq!(problem.variable_count(), 2);

        // Add objective
        let obj1 = problem.intern("obj1");
        problem.add_objective(Objective {
            name: obj1,
            coefficients: vec![Coefficient { name: x3, value: 1.0 }],
            constant: 0.0,
            byte_offset: None,
        });
        assert_eq!(problem.objective_count(), 1);
        assert_eq!(problem.variable_count(), 3);

        // SOS constraint creates SOS-typed variables
        let sos1 = problem.intern("sos1");
        problem.add_constraint(Constraint::SOS {
            name: sos1,
            sos_type: SOSType::S1,
            weights: vec![Coefficient { name: s1, value: 1.0 }],
            byte_offset: None,
        });
        assert_eq!(problem.variables[&s1].var_type, VariableType::SOS);
    }

    #[test]
    fn test_parsing_variations() {
        let p = LpProblem::parse("minimize\nx1\nsubject to\nx1 <= 1\nend").unwrap();
        assert_eq!(p.sense, Sense::Minimize);
        assert_eq!((p.objective_count(), p.constraint_count()), (1, 1));

        let p = LpProblem::parse("maximize\n2x1 + 3x2\nsubject to\nx1 + x2 <= 10\nend").unwrap();
        assert_eq!(p.sense, Sense::Maximize);

        let p = LpProblem::parse("minimize\nobj1: x1\nobj2: x2\nsubject to\nc1: x1 <= 10\nc2: x1 >= 0\nend").unwrap();
        assert_eq!((p.objective_count(), p.constraint_count()), (2, 2));

        // Variable types
        let input = "minimize\nx1\nsubject to\nx1 <= 1\nintegers\nx1\nend";
        let p = LpProblem::parse(input).unwrap();
        let x1 = p.name_id("x1").unwrap();
        assert_eq!(p.variables[&x1].var_type, VariableType::Integer);

        let input = "minimize\nx1\nsubject to\nx1 <= 1\nbinaries\nx1\nend";
        let p = LpProblem::parse(input).unwrap();
        let x1 = p.name_id("x1").unwrap();
        assert_eq!(p.variables[&x1].var_type, VariableType::Binary);

        let input = "minimize\nx1\nsubject to\nx1 <= 1\ngenerals\nx1\nend";
        let p = LpProblem::parse(input).unwrap();
        let x1 = p.name_id("x1").unwrap();
        assert_eq!(p.variables[&x1].var_type, VariableType::General);

        let input = "minimize\nx1\nsubject to\nx1 <= 1\nsemi-continuous\nx1\nend";
        let p = LpProblem::parse(input).unwrap();
        let x1 = p.name_id("x1").unwrap();
        assert_eq!(p.variables[&x1].var_type, VariableType::SemiContinuous);

        // Bounds
        let p = LpProblem::parse("minimize\nx1 + x2\nsubject to\nx1 <= 10\nbounds\nx1 >= 0\nx2 <= 5\nend").unwrap();
        let x1 = p.name_id("x1").unwrap();
        let x2 = p.name_id("x2").unwrap();
        assert!(matches!(p.variables[&x1].var_type, VariableType::LowerBound(0.0)));
        assert!(matches!(p.variables[&x2].var_type, VariableType::UpperBound(5.0)));

        // Empty constraints section is valid
        assert!(LpProblem::parse("minimize\nx1\nsubject to\nend").is_ok());
    }

    // Parsed coefficients round-trip bit-exactly from source text.
    #[allow(clippy::float_cmp)]
    #[test]
    fn test_duplicate_variable_terms_sum() {
        // Repeated terms for the same variable sum, matching solver LP readers.
        let p = LpProblem::parse("minimize\nobj: x1 + x1\nsubject to\nc1: 2 x1 + 3 x1 - x1 <= 8\nend").unwrap();
        let obj = p.objectives.values().next().unwrap();
        assert_eq!(obj.coefficients.len(), 1);
        assert_eq!(obj.coefficients[0].value, 2.0);
        let Constraint::Standard { coefficients, .. } = p.constraints.values().next().unwrap() else {
            panic!("expected standard constraint");
        };
        assert_eq!(coefficients.len(), 1);
        assert_eq!(coefficients[0].value, 4.0);
    }

    // Parsed values round-trip bit-exactly from source text.
    #[allow(clippy::float_cmp)]
    #[test]
    fn test_duplicate_names_last_wins() {
        // A repeated constraint name replaces the earlier definition.
        let p = LpProblem::parse("minimize\nx1\nsubject to\nc1: x1 <= 1\nc1: x1 >= 2\nend").unwrap();
        assert_eq!(p.constraint_count(), 1);
        let Constraint::Standard { operator, rhs, .. } = p.constraints.values().next().unwrap() else {
            panic!("expected standard constraint");
        };
        assert_eq!(*operator, ComparisonOp::GTE);
        assert_eq!(*rhs, 2.0);

        // Same for objective names.
        let p = LpProblem::parse("minimize\nobj: x1\nobj: 3 x2\nsubject to\nc1: x1 <= 1\nend").unwrap();
        assert_eq!(p.objective_count(), 1);
        let obj = p.objectives.values().next().unwrap();
        assert_eq!(obj.coefficients.len(), 1);
        assert_eq!(obj.coefficients[0].value, 3.0);
    }

    #[test]
    fn test_keyword_as_constraint_label_is_rejected() {
        // `end` lexes as the End keyword, so it cannot silently become a
        // constraint name; the parse must fail rather than misparse.
        assert!(LpProblem::parse("minimize\nx1\nsubject to\nend: x1 <= 1\nend").is_err());
    }

    #[test]
    fn test_separate_bound_lines_merge_into_double_bound() {
        // `x >= lb` followed by `x <= ub` must merge, not overwrite.
        let p = LpProblem::parse("minimize\nx1\nsubject to\nc1: x1 <= 10\nbounds\nx1 >= 2\nx1 <= 8\nend").unwrap();
        let x1 = p.name_id("x1").unwrap();
        assert_eq!(p.variables[&x1].var_type, VariableType::DoubleBound(2.0, 8.0));

        // Reverse declaration order merges too.
        let p = LpProblem::parse("minimize\nx1\nsubject to\nc1: x1 <= 10\nbounds\nx1 <= 8\nx1 >= 2\nend").unwrap();
        let x1 = p.name_id("x1").unwrap();
        assert_eq!(p.variables[&x1].var_type, VariableType::DoubleBound(2.0, 8.0));

        // Same-side redeclaration keeps last-wins semantics.
        let p = LpProblem::parse("minimize\nx1\nsubject to\nc1: x1 <= 10\nbounds\nx1 >= 2\nx1 >= 3\nend").unwrap();
        let x1 = p.name_id("x1").unwrap();
        assert_eq!(p.variables[&x1].var_type, VariableType::LowerBound(3.0));
    }

    #[test]
    fn test_parse_errors() {
        let invalid = [
            "",
            "   \n\t  ",
            "invalid_sense\nx1\nsubject to\nend",
            "minimize\nend",
            "minimize\nx1\nsubject",
            "minimize\nx1 x2\nsubject to\nend",
            "minimize\nx1\nsubject to\nx1 <= x2\nend",
            "minimize\nx1\nsubject to\nx1 2 <= 1\nend",
        ];
        for input in invalid {
            assert!(LpProblem::parse(input).is_err(), "Should fail: {input}");
        }

        // An empty objective is valid per the CPLEX spec.
        let p = LpProblem::parse("minimize\nsubject to\nx1 <= 1\nend").unwrap();
        assert_eq!((p.objective_count(), p.constraint_count()), (0, 1));
    }

    #[test]
    fn test_case_insensitivity() {
        for (min, st, end) in [("MINIMIZE", "SUBJECT TO", "END"), ("Minimize", "Subject To", "End"), ("MiNiMiZe", "SuBjEcT tO", "EnD")] {
            assert!(LpProblem::parse(&format!("{min}\nx1\n{st}\nx1 <= 1\n{end}")).is_ok());
        }
    }

    #[test]
    fn test_whitespace_handling() {
        assert!(LpProblem::parse("minimize\n\tx1\t+\t x2 \nsubject to\n\t x1\t+ x2\t<=\t10\nend").is_ok());
        assert!(LpProblem::parse("\n\n\nminimize\n\n\nx1\n\n\nsubject to\n\n\nx1 <= 1\n\n\nend\n\n\n").is_ok());
        assert!(LpProblem::parse("minimize\r\nx1\r\nsubject to\r\nx1 <= 1\r\nend\r\n").is_ok());
    }

    #[test]
    fn test_scientific_notation() {
        let inputs = [
            "minimize\n1e10x1\nsubject to\nx1 <= 1e10\nend",
            "minimize\n1E-10x1\nsubject to\nx1 <= 1E-10\nend",
            "minimize\n2.5e+3x1\nsubject to\nx1 <= 2.5e+3\nend",
            "minimize\n1.23456789e+100x1\nsubject to\nx1 <= 1.23456789e+100\nend",
        ];
        for input in inputs {
            assert_eq!(LpProblem::parse(input).unwrap().objective_count(), 1);
        }
    }

    #[test]
    fn test_special_values() {
        assert!(LpProblem::parse("minimize\n-inf x1\nsubject to\nx1 >= -infinity\nend").is_ok());
        assert!(LpProblem::parse("minimize\n0x1 + 0x2\nsubject to\n0x1 + 0x2 = 0\nend").is_ok());
        let input = format!("minimize\n{}x1\nsubject to\nx1 <= {}\nend", f64::MAX, f64::MAX);
        assert!(LpProblem::parse(&input).is_ok());
    }

    #[test]
    fn test_name_patterns() {
        let mut problem = LpProblem::new();
        let names = ["x", "X1", "var_123", "x.1.2", "_var", "VAR123ABC"];
        for name in names {
            let id = problem.intern(name);
            problem.add_variable(Variable::new(id));
        }
        assert_eq!(problem.variable_count(), names.len());
    }

    #[test]
    fn test_large_problem() {
        let mut problem = LpProblem::new();
        for i in 0..100 {
            let var_name = format!("x{i}");
            let con_name = format!("c{i}");
            let var_id = problem.intern(&var_name);
            let con_id = problem.intern(&con_name);
            problem.add_variable(Variable::new(var_id));
            problem.add_constraint(Constraint::Standard {
                name: con_id,
                coefficients: vec![Coefficient { name: var_id, value: 1.0 }],
                operator: ComparisonOp::LTE,
                rhs: 10.0,
                byte_offset: None,
            });
        }
        assert_eq!(problem.variable_count(), 100);
        assert_eq!(problem.constraint_count(), 100);
    }

    #[test]
    fn test_fixed_bounds_forms() {
        // `x1 = 5` fixes the variable at 5 via a degenerate double bound.
        let p = LpProblem::parse("minimize\nx1\nsubject to\nc1: x1 <= 10\nbounds\nx1 = 5\nend").unwrap();
        let x1 = p.name_id("x1").unwrap();
        assert_eq!(p.variables[&x1].var_type, VariableType::DoubleBound(5.0, 5.0));

        // The reversed `5 = x1` form is accepted too.
        let p = LpProblem::parse("minimize\nx1\nsubject to\nc1: x1 <= 10\nbounds\n5 = x1\nend").unwrap();
        let x1 = p.name_id("x1").unwrap();
        assert_eq!(p.variables[&x1].var_type, VariableType::DoubleBound(5.0, 5.0));
    }

    #[test]
    fn test_reversed_upper_bound() {
        // `5 >= x1` reads as an upper bound of 5 on x1.
        let p = LpProblem::parse("minimize\nx1\nsubject to\nc1: x1 <= 10\nbounds\n5 >= x1\nend").unwrap();
        let x1 = p.name_id("x1").unwrap();
        assert_eq!(p.variables[&x1].var_type, VariableType::UpperBound(5.0));
    }

    // Parsed values round-trip bit-exactly from source text.
    #[allow(clippy::float_cmp)]
    #[test]
    fn test_objective_constant() {
        // Trailing constant term (CPLEX: the objective may include a constant).
        let p = LpProblem::parse("minimize\nobj: x + 2 y + 10\nsubject to\nc1: x >= 1\nend").unwrap();
        let obj = p.objectives.values().next().unwrap();
        assert_eq!(obj.constant, 10.0);
        assert_eq!(obj.coefficients.len(), 2);

        // A trailing constant followed by another named objective binds as a
        // constant, not as a coefficient of the next objective's name.
        let p = LpProblem::parse("minimize\nobj1: x + 5\nobj2: y\nsubject to\nc1: x >= 1\nend").unwrap();
        let objs: Vec<_> = p.objectives.values().collect();
        assert_eq!(objs.len(), 2);
        assert_eq!(objs[0].constant, 5.0);
        assert_eq!(objs[0].coefficients.len(), 1);
        assert_eq!(objs[1].constant, 0.0);
        assert_eq!(objs[1].coefficients.len(), 1);
    }

    #[test]
    fn test_empty_objective() {
        // CPLEX permits an empty objective function, named or not.
        let p = LpProblem::parse("minimize\nsubject to\nc1: x + y >= 1\nend").unwrap();
        assert_eq!(p.objective_count(), 0);
        let p = LpProblem::parse("minimize\nobj:\nsubject to\nc1: x + y >= 1\nend").unwrap();
        assert_eq!(p.objective_count(), 1);
        assert!(p.objectives.values().next().unwrap().coefficients.is_empty());
    }

    // Parsed values round-trip bit-exactly from source text.
    #[allow(clippy::float_cmp)]
    #[test]
    fn test_constraint_lhs_constant_folds_into_rhs() {
        let p = LpProblem::parse("minimize\nobj: x\nsubject to\nc1: x + 2 <= 10\nend").unwrap();
        let c1 = p.name_id("c1").unwrap();
        let Constraint::Standard { rhs, .. } = &p.constraints[&c1] else { panic!("expected standard constraint") };
        assert_eq!(*rhs, 8.0);
    }

    #[test]
    fn test_eq_prefixed_operators() {
        // CPLEX accepts `=<` and `=>` as synonyms of `<=` and `>=`.
        let p = LpProblem::parse("minimize\nobj: x\nsubject to\nc1: x =< 10\nc2: x => 1\nend").unwrap();
        let c1 = p.name_id("c1").unwrap();
        let c2 = p.name_id("c2").unwrap();
        let Constraint::Standard { operator, .. } = &p.constraints[&c1] else { panic!("expected standard constraint") };
        assert_eq!(*operator, ComparisonOp::LTE);
        let Constraint::Standard { operator, .. } = &p.constraints[&c2] else { panic!("expected standard constraint") };
        assert_eq!(*operator, ComparisonOp::GTE);
    }

    // Parsed values round-trip bit-exactly from source text.
    #[allow(clippy::float_cmp)]
    #[test]
    fn test_ranged_and_flipped_constraints() {
        // `lo <= expr <= hi` expands into two constraints, like MPS RANGES.
        let p = LpProblem::parse("minimize\nobj: x + y\nsubject to\nc1: -2 <= x + y <= 10\nend").unwrap();
        assert_eq!(p.constraint_count(), 2);
        let lower = p.name_id("c1").unwrap();
        let upper = p.name_id("c1_rng").unwrap();
        let Constraint::Standard { operator, rhs, .. } = &p.constraints[&lower] else { panic!("expected standard constraint") };
        assert_eq!((*operator, *rhs), (ComparisonOp::GTE, -2.0));
        let Constraint::Standard { operator, rhs, .. } = &p.constraints[&upper] else { panic!("expected standard constraint") };
        assert_eq!((*operator, *rhs), (ComparisonOp::LTE, 10.0));

        // Reversed range: `hi >= expr >= lo`.
        let p = LpProblem::parse("minimize\nobj: x\nsubject to\nc1: 10 >= x >= 2\nend").unwrap();
        assert_eq!(p.constraint_count(), 2);

        // Flipped single constraint: `10 >= x` normalises to `x <= 10`.
        let p = LpProblem::parse("minimize\nobj: x\nsubject to\nc1: 10 >= x\nend").unwrap();
        let c1 = p.name_id("c1").unwrap();
        let Constraint::Standard { operator, rhs, .. } = &p.constraints[&c1] else { panic!("expected standard constraint") };
        assert_eq!((*operator, *rhs), (ComparisonOp::LTE, 10.0));

        // A flipped/ranged entry must not swallow the following constraint.
        let p = LpProblem::parse("minimize\nobj: x\nsubject to\nc1: x <= 10\ny + z >= 3\nend").unwrap();
        assert_eq!(p.constraint_count(), 2);
    }

    #[test]
    fn test_backslash_comment_inside_token_stream() {
        // Per CPLEX, `\` starts a comment anywhere on a line, even glued to a name.
        let p = LpProblem::parse("minimize\nobj: x\\y + z\nsubject to\nc1: x >= 1\nend").unwrap();
        assert!(p.name_id("x").is_some());
        assert!(p.name_id("x\\y").is_none());
    }

    // Parsed RHS values round-trip bit-exactly from source text.
    #[allow(clippy::float_cmp)]
    #[test]
    fn test_strict_comparison_operators() {
        let p = LpProblem::parse("minimize\nx1\nsubject to\nc1: x1 < 1\nc2: x1 > 1\nend").unwrap();
        let c1 = p.name_id("c1").unwrap();
        let c2 = p.name_id("c2").unwrap();
        let Constraint::Standard { operator, rhs, .. } = &p.constraints[&c1] else {
            panic!("expected standard constraint");
        };
        assert_eq!(*operator, ComparisonOp::LT);
        assert_eq!(*rhs, 1.0);
        let Constraint::Standard { operator, .. } = &p.constraints[&c2] else {
            panic!("expected standard constraint");
        };
        assert_eq!(*operator, ComparisonOp::GT);
    }

    #[test]
    fn test_strict_and_reversed_bounds() {
        // Strict operators in single bounds are synonyms of `<=` / `>=`.
        let p = LpProblem::parse("minimize\nx1\nsubject to\nc1: x1 >= 0\nbounds\nx1 < 5\nend").unwrap();
        let x1 = p.name_id("x1").unwrap();
        assert_eq!(p.variables[&x1].var_type, VariableType::UpperBound(5.0));

        let p = LpProblem::parse("minimize\nx1\nsubject to\nc1: x1 <= 9\nbounds\nx1 > 1\nend").unwrap();
        let x1 = p.name_id("x1").unwrap();
        assert_eq!(p.variables[&x1].var_type, VariableType::LowerBound(1.0));

        // Reversed double bound: `10 >= x1 >= 0`.
        let p = LpProblem::parse("minimize\nx1\nsubject to\nc1: x1 >= 0\nbounds\n10 >= x1 >= 0\nend").unwrap();
        let x1 = p.name_id("x1").unwrap();
        assert_eq!(p.variables[&x1].var_type, VariableType::DoubleBound(0.0, 10.0));
    }

    #[test]
    fn test_strict_double_bounds() {
        // All mixes of strict and non-strict operators collapse to a DoubleBound.
        for bound_line in ["0 < x1 < 5", "0 <= x1 < 5", "0 < x1 <= 5"] {
            let input = format!("minimize\nx1\nsubject to\nc1: x1 <= 10\nbounds\n{bound_line}\nend");
            let p = LpProblem::parse(&input).unwrap();
            let x1 = p.name_id("x1").unwrap();
            assert_eq!(p.variables[&x1].var_type, VariableType::DoubleBound(0.0, 5.0), "failed for bound line: {bound_line}");
        }
    }

    #[test]
    fn test_double_colon_constraint_separator() {
        // `::` is accepted as a named-constraint separator, as some writers emit it.
        let p = LpProblem::parse("minimize\nx1\nsubject to\nc1:: x1 <= 1\nend").unwrap();
        assert_eq!(p.constraint_count(), 1);
        let c1 = p.name_id("c1").unwrap();
        assert!(matches!(&p.constraints[&c1], Constraint::Standard { operator: ComparisonOp::LTE, .. }));
    }

    #[test]
    fn test_missing_end_keyword() {
        // The trailing `End` keyword is optional.
        let p = LpProblem::parse("minimize\nx1\nsubject to\nc1: x1 <= 1").unwrap();
        assert_eq!((p.objective_count(), p.constraint_count()), (1, 1));
    }

    #[test]
    fn test_repeated_optional_sections_extend() {
        // Two `bounds` blocks both apply.
        let p = LpProblem::parse("minimize\nx1 + x2\nsubject to\nc1: x1 <= 10\nbounds\nx1 >= 1\nbounds\nx2 <= 5\nend").unwrap();
        let x1 = p.name_id("x1").unwrap();
        let x2 = p.name_id("x2").unwrap();
        assert_eq!(p.variables[&x1].var_type, VariableType::LowerBound(1.0));
        assert_eq!(p.variables[&x2].var_type, VariableType::UpperBound(5.0));

        // Interleaved binaries/generals/binaries blocks all accumulate.
        let p = LpProblem::parse("minimize\nx1\nsubject to\nc1: x1 <= 1\nbinaries\nb1\ngenerals\ng1\nbinaries\nb2\nend").unwrap();
        let b1 = p.name_id("b1").unwrap();
        let b2 = p.name_id("b2").unwrap();
        let g1 = p.name_id("g1").unwrap();
        assert_eq!(p.variables[&b1].var_type, VariableType::Binary);
        assert_eq!(p.variables[&b2].var_type, VariableType::Binary);
        assert_eq!(p.variables[&g1].var_type, VariableType::General);
    }

    #[test]
    fn test_section_aliases_full_parse() {
        // `max`, `st`, `bound`, `gen`, and `bin` are all accepted aliases.
        let p = LpProblem::parse("max\nx1\nst\nc1: x1 <= 1\nbound\nx1 <= 4\ngen\ng1\nbin\nb1\nend").unwrap();
        assert_eq!(p.sense, Sense::Maximize);
        let x1 = p.name_id("x1").unwrap();
        let g1 = p.name_id("g1").unwrap();
        let b1 = p.name_id("b1").unwrap();
        assert_eq!(p.variables[&x1].var_type, VariableType::UpperBound(4.0));
        assert_eq!(p.variables[&g1].var_type, VariableType::General);
        assert_eq!(p.variables[&b1].var_type, VariableType::Binary);

        // `such that` is a multi-word alias for `subject to`.
        let p = LpProblem::parse("minimize\nx1\nsuch that\nc1: x1 <= 1\nend").unwrap();
        assert_eq!(p.constraint_count(), 1);
    }

    #[test]
    fn test_auto_generated_names() {
        // An unnamed objective resolves to OBJ1; unnamed constraints to C1, C2, ...
        let p = LpProblem::parse("minimize\nx1\nsubject to\nx1 <= 1\nx1 >= 0\nend").unwrap();
        let obj1 = p.name_id("OBJ1").expect("auto-generated objective name missing");
        assert!(p.objectives.contains_key(&obj1));
        let c1 = p.name_id("C1").expect("auto-generated constraint name C1 missing");
        let c2 = p.name_id("C2").expect("auto-generated constraint name C2 missing");
        assert!(p.constraints.contains_key(&c1));
        assert!(p.constraints.contains_key(&c2));
    }

    // Infinity comparisons are exact by definition.
    #[allow(clippy::float_cmp)]
    #[test]
    fn test_overflowing_literal_becomes_infinite_coefficient() {
        // 1e400 overflows f64 and saturates to positive infinity rather than erroring.
        let p = LpProblem::parse("minimize\n1e400 x1\nsubject to\nc1: x1 <= 1\nend").unwrap();
        let obj = p.objectives.values().next().unwrap();
        assert_eq!(obj.coefficients.len(), 1);
        assert_eq!(obj.coefficients[0].value, f64::INFINITY);
    }

    #[test]
    fn test_empty_variable_type_sections() {
        // `generals`/`binaries` headers with zero identifiers still parse.
        let p = LpProblem::parse("minimize\nx1\nsubject to\nc1: x1 <= 1\ngenerals\nbinaries\nend").unwrap();
        assert_eq!(p.constraint_count(), 1);
        let x1 = p.name_id("x1").unwrap();
        assert_eq!(p.variables[&x1].var_type, VariableType::Free);
    }

    // Parsed RHS values round-trip bit-exactly from source text.
    #[allow(clippy::float_cmp)]
    #[test]
    fn test_signed_rhs_values() {
        let p = LpProblem::parse("minimize\nx1\nsubject to\nc1: x1 <= +5\nc2: x1 >= -5\nc3: x1 <= inf\nend").unwrap();
        let expected = [("c1", 5.0), ("c2", -5.0), ("c3", f64::INFINITY)];
        for (name, want) in expected {
            let id = p.name_id(name).unwrap();
            let Constraint::Standard { rhs, .. } = &p.constraints[&id] else {
                panic!("expected standard constraint for {name}");
            };
            assert_eq!(*rhs, want, "wrong RHS for {name}");
        }
    }

    #[test]
    fn test_lexer_error_surfaces_as_parse_error() {
        // A standalone `|` cannot start any token, so the lexer error must
        // propagate out of `parse` rather than panic or be swallowed.
        assert!(LpProblem::parse("minimize\nx1\nsubject to\nc1: x1 <= | 1\nend").is_err());
        assert!(LpProblem::parse("minimize\n| x1\nsubject to\nc1: x1 <= 1\nend").is_err());
    }
}

#[cfg(test)]
mod modification_tests {
    use crate::model::{Coefficient, ComparisonOp, Constraint, Objective, SOSType, Sense, VariableType};
    use crate::problem::LpProblem;

    fn create_test_problem() -> LpProblem {
        let mut problem = LpProblem::new().with_sense(Sense::Minimize);
        let x1 = problem.intern("x1");
        let x2 = problem.intern("x2");
        let obj1 = problem.intern("obj1");
        let c1 = problem.intern("c1");

        problem.add_objective(Objective {
            name: obj1,
            coefficients: vec![Coefficient { name: x1, value: 2.0 }, Coefficient { name: x2, value: 3.0 }],
            constant: 0.0,
            byte_offset: None,
        });
        problem.add_constraint(Constraint::Standard {
            name: c1,
            coefficients: vec![Coefficient { name: x1, value: 1.0 }, Coefficient { name: x2, value: 1.0 }],
            operator: ComparisonOp::LTE,
            rhs: 10.0,
            byte_offset: None,
        });
        problem
    }

    #[test]
    // The updated RHS must round-trip bit-exactly, so compare floats strictly.
    #[allow(clippy::float_cmp)]
    fn test_update_coefficients() {
        let mut p = create_test_problem();

        p.update_objective_coefficient("obj1", "x1", 5.0).unwrap();
        p.update_objective_coefficient("obj1", "x3", 1.5).unwrap();
        p.update_objective_coefficient("obj1", "x2", 0.0).unwrap();

        let obj1 = p.name_id("obj1").unwrap();
        let x1 = p.name_id("x1").unwrap();
        let x3 = p.name_id("x3").unwrap();
        let coeffs: Vec<_> = p.objectives[&obj1].coefficients.iter().map(|c| (c.name, c.value)).collect();
        assert!(coeffs.contains(&(x1, 5.0)) && coeffs.contains(&(x3, 1.5)));
        let x2 = p.name_id("x2").unwrap();
        assert!(!coeffs.iter().any(|(n, _)| *n == x2));

        p.update_constraint_coefficient("c1", "x1", 3.0).unwrap();
        p.update_constraint_coefficient("c1", "x3", 2.5).unwrap();
        p.update_constraint_coefficient("c1", "x2", 0.0).unwrap();

        p.update_constraint_rhs("c1", 15.0).unwrap();
        let c1 = p.name_id("c1").unwrap();
        if let Constraint::Standard { rhs, .. } = p.constraints.get(&c1).unwrap() {
            assert_eq!(*rhs, 15.0);
        }

        assert!(p.update_objective_coefficient("nonexistent", "x1", 1.0).is_err());
        assert!(p.update_constraint_coefficient("nonexistent", "x1", 1.0).is_err());
    }

    #[test]
    #[allow(clippy::similar_names)]
    fn test_rename_operations() {
        let mut p = create_test_problem();

        p.rename_variable("x1", "new_x1").unwrap();
        let new_x1 = p.name_id("new_x1").unwrap();
        assert!(p.name_id("x1").is_none_or(|id| !p.variables.contains_key(&id)));
        assert!(p.variables.contains_key(&new_x1));
        let obj1 = p.name_id("obj1").unwrap();
        assert!(p.objectives[&obj1].coefficients.iter().any(|c| c.name == new_x1));

        p.rename_constraint("c1", "new_c1").unwrap();
        let new_c1 = p.name_id("new_c1").unwrap();
        assert!(p.constraints.contains_key(&new_c1));

        p.rename_objective("obj1", "new_obj1").unwrap();
        let new_obj1 = p.name_id("new_obj1").unwrap();
        assert!(p.objectives.contains_key(&new_obj1));

        assert!(p.rename_variable("nonexistent", "x").is_err());
        assert!(p.rename_variable("new_x1", "x2").is_err());
    }

    #[test]
    fn test_remove_operations() {
        let mut p = create_test_problem();

        p.remove_variable("x2").unwrap();
        assert!(p.name_id("x2").is_none_or(|id| !p.variables.contains_key(&id)));
        let obj1 = p.name_id("obj1").unwrap();
        let x2 = p.name_id("x2").unwrap();
        assert!(!p.objectives[&obj1].coefficients.iter().any(|c| c.name == x2));

        p.remove_constraint("c1").unwrap();
        assert!(p.name_id("c1").is_none_or(|id| !p.constraints.contains_key(&id)));

        p.remove_objective("obj1").unwrap();
        assert!(!p.objectives.contains_key(&obj1));

        assert!(p.remove_constraint("c1").is_err());
        assert!(p.remove_objective("obj1").is_err());
    }

    #[test]
    fn test_variable_type_update() {
        let mut p = create_test_problem();
        p.update_variable_type("x1", VariableType::Binary).unwrap();
        let x1 = p.name_id("x1").unwrap();
        assert_eq!(p.variables[&x1].var_type, VariableType::Binary);
    }

    #[test]
    fn test_sos_constraint_restrictions() {
        let mut p = LpProblem::new();
        let sos1 = p.intern("sos1");
        let x1 = p.intern("x1");
        p.add_constraint(Constraint::SOS {
            name: sos1,
            sos_type: SOSType::S1,
            weights: vec![Coefficient { name: x1, value: 1.0 }],
            byte_offset: None,
        });

        assert!(p.update_constraint_coefficient("sos1", "x1", 3.0).is_err());
        assert!(p.update_constraint_rhs("sos1", 5.0).is_err());

        p.rename_constraint("sos1", "new_sos").unwrap();
        p.remove_constraint("new_sos").unwrap();
    }
}
