use std::collections::HashSet;
use std::fmt::{Display, Formatter, Result as FmtResult};

use indexmap::IndexMap;
use indexmap::map::Entry;

use crate::NUMERIC_EPSILON;
use crate::error::{LpParseError, LpResult};
use crate::interner::{NameId, NameInterner};
use crate::lexer::{Lexer, ParseResult, RawCoefficient, RawConstraint, RawObjective};
use crate::lp::LpProblemParser;
use crate::model::{Coefficient, ComparisonOp, Constraint, Objective, Sense, Variable, VariableType};

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

/// Extract the problem name from LP file comments.
///
/// Supports multiple formats:
/// 1. `\Problem name: my_problem` or `\\Problem name: my_problem`
/// 2. `\* my_problem *\` (CPLEX block comment style)
fn extract_problem_name(input: &str) -> Option<String> {
    input.lines().find_map(|line| {
        let trimmed = line.trim();

        // Handle block comment format: \* name *\
        if let Some(inner) = trimmed.strip_prefix("\\*").and_then(|s| s.strip_suffix("*\\")) {
            let name = inner.trim();
            if !name.is_empty() {
                return Some(name.to_string());
            }
        }

        // Handle single/double backslash prefix
        let content = if trimmed.starts_with("\\\\") {
            trimmed.strip_prefix("\\\\")
        } else if trimmed.starts_with('\\') {
            trimmed.strip_prefix('\\')
        } else {
            None
        };

        content.and_then(|c| {
            let c = c.trim();
            let prefix = "problem name:";
            if c.len() >= prefix.len() && c[..prefix.len()].eq_ignore_ascii_case(prefix) {
                Some(c[prefix.len()..].trim().to_string())
            } else {
                None
            }
        })
    })
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
#[inline]
fn intern_coefficients(interner: &mut NameInterner, raw: &[RawCoefficient<'_>]) -> Vec<Coefficient> {
    raw.iter().map(|rc| Coefficient { name: interner.intern(rc.name), value: rc.value }).collect()
}

/// Intern a raw constraint into a model constraint.
#[inline]
fn intern_constraint(interner: &mut NameInterner, raw: &RawConstraint<'_>) -> Constraint {
    match raw {
        RawConstraint::Standard { name, coefficients, operator, rhs, byte_offset } => Constraint::Standard {
            name: interner.intern(name),
            coefficients: intern_coefficients(interner, coefficients),
            operator: operator.clone(),
            rhs: *rhs,
            byte_offset: *byte_offset,
        },
        RawConstraint::SOS { name, sos_type, weights, byte_offset } => Constraint::SOS {
            name: interner.intern(name),
            sos_type: sos_type.clone(),
            weights: intern_coefficients(interner, weights),
            byte_offset: *byte_offset,
        },
    }
}

/// Intern a raw objective into a model objective.
#[inline]
fn intern_objective(interner: &mut NameInterner, raw: &RawObjective<'_>) -> Objective {
    Objective { name: interner.intern(&raw.name), coefficients: intern_coefficients(interner, &raw.coefficients) }
}

/// Represents a Linear Programming (LP) problem.
///
/// All name strings are stored in the embedded [`NameInterner`] and referenced
/// by [`NameId`] throughout. This eliminates lifetime constraints and avoids
/// string duplication.
#[derive(Debug, Default)]
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
    pub fn get_name_id(&self, name: &str) -> Option<NameId> {
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
    /// Returns `true` if this is a minimisation problem.
    pub const fn is_minimization(&self) -> bool {
        self.sense.is_minimisation()
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
    /// Parse a `LpProblem` from a string slice.
    ///
    /// # Errors
    ///
    /// Returns an error if the input string is not a valid LP file format.
    pub fn parse(input: &str) -> LpResult<Self> {
        log::debug!("Starting to parse LP problem");
        Self::try_from(input)
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

    /// Update the operator of a constraint.
    ///
    /// # Errors
    ///
    /// Returns an error if the constraint does not exist or is an SOS constraint.
    pub fn update_constraint_operator(&mut self, constraint_name: &str, new_operator: ComparisonOp) -> LpResult<()> {
        debug_assert!(!constraint_name.is_empty(), "constraint_name must not be empty");
        let con_id = self
            .interner
            .get(constraint_name)
            .ok_or_else(|| LpParseError::validation_error(format!("Constraint '{constraint_name}' not found")))?;

        let constraint = self
            .constraints
            .get_mut(&con_id)
            .ok_or_else(|| LpParseError::validation_error(format!("Constraint '{constraint_name}' not found")))?;

        match constraint {
            Constraint::Standard { operator, .. } => {
                *operator = new_operator;
                Ok(())
            }
            Constraint::SOS { .. } => Err(LpParseError::validation_error("SOS constraints do not have comparison operators")),
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

    /// Get a sorted list of all variable name IDs referenced in the problem.
    #[must_use]
    pub fn get_all_variable_name_ids(&self) -> Vec<NameId> {
        let mut ids = HashSet::with_capacity(self.variables.len());

        for &id in self.variables.keys() {
            ids.insert(id);
        }

        for objective in self.objectives.values() {
            for coeff in &objective.coefficients {
                ids.insert(coeff.name);
            }
        }

        for constraint in self.constraints.values() {
            match constraint {
                Constraint::Standard { coefficients, .. } => {
                    for coeff in coefficients {
                        ids.insert(coeff.name);
                    }
                }
                Constraint::SOS { weights, .. } => {
                    for weight in weights {
                        ids.insert(weight.name);
                    }
                }
            }
        }

        let mut result: Vec<NameId> = ids.into_iter().collect();
        result.sort_by(|a, b| self.interner.resolve(*a).cmp(self.interner.resolve(*b)));
        debug_assert!(
            result.windows(2).all(|w| self.interner.resolve(w[0]) <= self.interner.resolve(w[1])),
            "postcondition: result must be sorted by resolved name"
        );
        result
    }

    /// Get a sorted list of all variable names referenced in the problem.
    #[must_use]
    pub fn get_all_variable_names(&self) -> Vec<&str> {
        self.get_all_variable_name_ids().iter().map(|id| self.interner.resolve(*id)).collect()
    }
}

// ── Serde support (feature-gated) ──────────────────────────────────────────
//
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

    // ── Intermediate serde types ────────────────────────────────────────

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

    // ── Conversion helpers ──────────────────────────────────────────────

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

    // ── Serialize ───────────────────────────────────────────────────────

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
                    })
                    .collect(),
                constraints: self
                    .constraints
                    .values()
                    .map(|con| match con {
                        Constraint::Standard { name, coefficients, operator, rhs, .. } => SerdeConstraint::Standard {
                            name: self.interner.resolve(*name).to_string(),
                            coefficients: coeffs_to_serde(coefficients, &self.interner),
                            operator: operator.clone(),
                            rhs: *rhs,
                        },
                        Constraint::SOS { name, sos_type, weights, .. } => SerdeConstraint::Sos {
                            name: self.interner.resolve(*name).to_string(),
                            sos_type: sos_type.clone(),
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

    // ── Deserialize ─────────────────────────────────────────────────────

    impl<'de> Deserialize<'de> for LpProblem {
        fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
            let proxy = SerdeLpProblem::deserialize(deserializer)?;
            let mut interner = NameInterner::new();

            let objectives: IndexMap<NameId, Objective> = proxy
                .objectives
                .iter()
                .map(|so| {
                    let name_id = interner.intern(&so.name);
                    let obj = Objective { name: name_id, coefficients: coeffs_from_serde(&so.coefficients, &mut interner) };
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
                            operator: operator.clone(),
                            rhs: *rhs,
                            byte_offset: None,
                        };
                        (name_id, con)
                    }
                    SerdeConstraint::Sos { name, sos_type, weights } => {
                        let name_id = interner.intern(name);
                        let con = Constraint::SOS {
                            name: name_id,
                            sos_type: sos_type.clone(),
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

impl TryFrom<&str> for LpProblem {
    type Error = LpParseError;

    fn try_from(input: &str) -> Result<Self, Self::Error> {
        log::debug!("Starting to parse LP problem with LALRPOP parser");

        let problem_name = extract_problem_name(input);

        let lexer = Lexer::new(input);
        let parser = LpProblemParser::new();
        let parsed = parser.parse(lexer).map_err(LpParseError::from)?;

        let estimated_names = parsed.objectives.len()
            + parsed.constraints.len()
            + parsed.bounds.len()
            + parsed.generals.len()
            + parsed.integers.len()
            + parsed.binaries.len();
        let mut interner = NameInterner::with_capacity(estimated_names.max(16));

        let mut variables: IndexMap<NameId, Variable> =
            IndexMap::with_capacity(parsed.bounds.len() + parsed.generals.len() + parsed.integers.len());
        let mut constraint_counter: u32 = 0;

        let objectives = intern_objectives(&mut interner, &parsed.objectives, &mut variables);
        let mut constraints = intern_constraints(&mut interner, &parsed.constraints, &mut variables, &mut constraint_counter);
        process_bounds(&mut interner, &parsed.bounds, &mut variables);
        process_variable_types(&mut interner, &parsed, &mut variables);
        intern_sos_constraints(&mut interner, &parsed.sos, &mut variables, &mut constraints, &mut constraint_counter);

        Ok(Self { name: problem_name, sense: parsed.sense, objectives, constraints, variables, interner })
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

    for raw_obj in raw_objectives {
        let mut obj = intern_objective(interner, raw_obj);

        if raw_obj.name == "__obj__" {
            obj_counter += 1;
            let auto_name = format!("OBJ{obj_counter}");
            obj.name = interner.intern(&auto_name);
        }

        register_variables_from_coefficients(variables, &obj.coefficients, None);
        objectives.insert(obj.name, obj);
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

    for raw_con in raw_constraints {
        let mut con = intern_constraint(interner, raw_con);
        let final_id = assign_constraint_name(interner, &mut con, constraint_counter, "C");
        register_constraint_variables(variables, &con);
        constraints.insert(final_id, con);
    }

    constraints
}

/// Process bounds declarations into the variables map.
fn process_bounds(interner: &mut NameInterner, bounds: &[(&str, VariableType)], variables: &mut IndexMap<NameId, Variable>) {
    for &(var_name, ref var_type) in bounds {
        let var_id = interner.intern(var_name);
        match variables.entry(var_id) {
            Entry::Occupied(mut entry) => entry.get_mut().set_var_type(var_type.clone()),
            Entry::Vacant(entry) => {
                entry.insert(Variable::new(var_id).with_var_type(var_type.clone()));
            }
        }
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
    for raw_sos_con in raw_sos {
        if matches!(raw_sos_con, RawConstraint::Standard { .. }) {
            continue;
        }
        let mut sos = intern_constraint(interner, raw_sos_con);
        let final_id = assign_constraint_name(interner, &mut sos, constraint_counter, "SOS");
        register_constraint_variables(variables, &sos);
        constraints.insert(final_id, sos);
    }
}

/// Assign a name to a constraint, generating one if unnamed.
/// Returns the final [`NameId`].
#[inline]
fn assign_constraint_name(interner: &mut NameInterner, constraint: &mut Constraint, counter: &mut u32, prefix: &str) -> NameId {
    let current_name = interner.resolve(constraint.name());
    let is_unnamed = current_name == "__c__" || current_name.is_empty();

    let final_id = if is_unnamed {
        *counter += 1;
        let auto_name = format!("{prefix}{}", *counter);
        interner.intern(&auto_name)
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
        assert!(problem.is_minimization());
        assert_eq!((problem.objective_count(), problem.constraint_count(), problem.variable_count()), (0, 0, 0));

        let problem = LpProblem::new().with_problem_name("test").with_sense(Sense::Maximize);
        assert_eq!(problem.name(), Some("test"));
        assert!(!problem.is_minimization());

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
        problem.add_objective(Objective { name: obj1, coefficients: vec![Coefficient { name: x3, value: 1.0 }] });
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
        let x1 = p.get_name_id("x1").unwrap();
        assert_eq!(p.variables[&x1].var_type, VariableType::Integer);

        let input = "minimize\nx1\nsubject to\nx1 <= 1\nbinaries\nx1\nend";
        let p = LpProblem::parse(input).unwrap();
        let x1 = p.get_name_id("x1").unwrap();
        assert_eq!(p.variables[&x1].var_type, VariableType::Binary);

        let input = "minimize\nx1\nsubject to\nx1 <= 1\ngenerals\nx1\nend";
        let p = LpProblem::parse(input).unwrap();
        let x1 = p.get_name_id("x1").unwrap();
        assert_eq!(p.variables[&x1].var_type, VariableType::General);

        let input = "minimize\nx1\nsubject to\nx1 <= 1\nsemi-continuous\nx1\nend";
        let p = LpProblem::parse(input).unwrap();
        let x1 = p.get_name_id("x1").unwrap();
        assert_eq!(p.variables[&x1].var_type, VariableType::SemiContinuous);

        // Bounds
        let p = LpProblem::parse("minimize\nx1 + x2\nsubject to\nx1 <= 10\nbounds\nx1 >= 0\nx2 <= 5\nend").unwrap();
        let x1 = p.get_name_id("x1").unwrap();
        let x2 = p.get_name_id("x2").unwrap();
        assert!(matches!(p.variables[&x1].var_type, VariableType::LowerBound(0.0)));
        assert!(matches!(p.variables[&x2].var_type, VariableType::UpperBound(5.0)));

        // Empty constraints section is valid
        assert!(LpProblem::parse("minimize\nx1\nsubject to\nend").is_ok());
    }

    #[test]
    fn test_parse_errors() {
        let invalid = [
            "",
            "   \n\t  ",
            "invalid_sense\nx1\nsubject to\nend",
            "minimize\nend",
            "minimize\nsubject to\nx1 <= 1\nend",
            "minimize\nx1\nsubject",
        ];
        for input in invalid {
            assert!(LpProblem::parse(input).is_err(), "Should fail: {input}");
        }
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
    fn test_update_coefficients() {
        let mut p = create_test_problem();

        p.update_objective_coefficient("obj1", "x1", 5.0).unwrap();
        p.update_objective_coefficient("obj1", "x3", 1.5).unwrap();
        p.update_objective_coefficient("obj1", "x2", 0.0).unwrap();

        let obj1 = p.get_name_id("obj1").unwrap();
        let x1 = p.get_name_id("x1").unwrap();
        let x3 = p.get_name_id("x3").unwrap();
        let coeffs: Vec<_> = p.objectives[&obj1].coefficients.iter().map(|c| (c.name, c.value)).collect();
        assert!(coeffs.contains(&(x1, 5.0)) && coeffs.contains(&(x3, 1.5)));
        let x2 = p.get_name_id("x2").unwrap();
        assert!(!coeffs.iter().any(|(n, _)| *n == x2));

        p.update_constraint_coefficient("c1", "x1", 3.0).unwrap();
        p.update_constraint_coefficient("c1", "x3", 2.5).unwrap();
        p.update_constraint_coefficient("c1", "x2", 0.0).unwrap();

        p.update_constraint_rhs("c1", 15.0).unwrap();
        p.update_constraint_operator("c1", ComparisonOp::GTE).unwrap();
        let c1 = p.get_name_id("c1").unwrap();
        if let Constraint::Standard { rhs, operator, .. } = p.constraints.get(&c1).unwrap() {
            assert_eq!((*rhs, operator), (15.0, &ComparisonOp::GTE));
        }

        assert!(p.update_objective_coefficient("nonexistent", "x1", 1.0).is_err());
        assert!(p.update_constraint_coefficient("nonexistent", "x1", 1.0).is_err());
    }

    #[test]
    #[allow(clippy::similar_names)]
    fn test_rename_operations() {
        let mut p = create_test_problem();

        p.rename_variable("x1", "new_x1").unwrap();
        let new_x1 = p.get_name_id("new_x1").unwrap();
        assert!(p.get_name_id("x1").is_none_or(|id| !p.variables.contains_key(&id)));
        assert!(p.variables.contains_key(&new_x1));
        let obj1 = p.get_name_id("obj1").unwrap();
        assert!(p.objectives[&obj1].coefficients.iter().any(|c| c.name == new_x1));

        p.rename_constraint("c1", "new_c1").unwrap();
        let new_c1 = p.get_name_id("new_c1").unwrap();
        assert!(p.constraints.contains_key(&new_c1));

        p.rename_objective("obj1", "new_obj1").unwrap();
        let new_obj1 = p.get_name_id("new_obj1").unwrap();
        assert!(p.objectives.contains_key(&new_obj1));

        assert!(p.rename_variable("nonexistent", "x").is_err());
        assert!(p.rename_variable("new_x1", "x2").is_err());
    }

    #[test]
    fn test_remove_operations() {
        let mut p = create_test_problem();

        p.remove_variable("x2").unwrap();
        assert!(p.get_name_id("x2").is_none_or(|id| !p.variables.contains_key(&id)));
        let obj1 = p.get_name_id("obj1").unwrap();
        let x2 = p.get_name_id("x2").unwrap();
        assert!(!p.objectives[&obj1].coefficients.iter().any(|c| c.name == x2));

        p.remove_constraint("c1").unwrap();
        assert!(p.get_name_id("c1").is_none_or(|id| !p.constraints.contains_key(&id)));

        p.remove_objective("obj1").unwrap();
        assert!(!p.objectives.contains_key(&obj1));

        assert!(p.remove_constraint("c1").is_err());
        assert!(p.remove_objective("obj1").is_err());
    }

    #[test]
    fn test_variable_type_update() {
        let mut p = create_test_problem();
        p.update_variable_type("x1", VariableType::Binary).unwrap();
        let x1 = p.get_name_id("x1").unwrap();
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
        assert!(p.update_constraint_operator("sos1", ComparisonOp::LTE).is_err());

        p.rename_constraint("sos1", "new_sos").unwrap();
        p.remove_constraint("new_sos").unwrap();
    }

    #[test]
    fn test_get_variable_names() {
        let p = create_test_problem();
        let names = p.get_all_variable_names();
        assert!(names.contains(&"x1") && names.contains(&"x2") && names.len() == 2);
    }
}
