use std::borrow::Cow;
use std::collections::HashMap;
use std::collections::hash_map::Entry;

use unique_id::Generator;
use unique_id::sequence::SequenceGenerator;

use crate::error::{LpParseError, LpResult};
use crate::lexer::Lexer;
use crate::lp::LpProblemParser;
use crate::model::{Constraint, Objective, Sense, Variable, VariableType};

/// Check if a floating-point value is effectively zero using both absolute
/// and relative epsilon comparisons.
///
/// This handles edge cases better than simple `value.abs() < f64::EPSILON`:
/// - For small values near zero, uses absolute epsilon (`f64::EPSILON`)
/// - For larger values, uses relative epsilon based on the reference magnitude
///
/// # Arguments
/// * `value` - The value to check
/// * `reference` - A reference magnitude for relative comparison (e.g., existing coefficient)
#[inline]
fn is_effectively_zero(value: f64, reference: f64) -> bool {
    let abs_value = value.abs();
    let abs_reference = reference.abs();

    // Absolute check for values near zero
    if abs_value < f64::EPSILON {
        return true;
    }

    // Relative check: value is negligible compared to reference
    if abs_reference > f64::EPSILON {
        // Use a reasonable relative tolerance (1e-10 is typical for numerical algorithms)
        const RELATIVE_EPSILON: f64 = 1e-10;
        return abs_value < abs_reference * RELATIVE_EPSILON;
    }

    false
}

#[cfg_attr(feature = "diff", derive(diff::Diff), diff(attr(#[derive(Debug, PartialEq)])))]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[derive(Debug, Default, PartialEq)]
/// Represents a Linear Programming (LP) problem.
///
/// The `LpProblem` struct encapsulates the components of an LP problem, including its name,
/// sense (e.g., minimisation, or maximisation), objectives, constraints, and variables.
///
/// # Attributes
///
/// * `#[cfg_attr(feature = "diff", derive(diff::Diff), diff(attr(#[derive(Debug, PartialEq)])))]`:
///   Enables the `diff` feature for comparing differences between instances of `LpProblem`.
/// * `#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]`:
///   Enables serialisation and deserialisation of `LpProblem` instances when the `serde` feature is active.
///
pub struct LpProblem<'a> {
    /// An optional reference to a string slice representing the name of the LP problem.
    pub name: Option<Cow<'a, str>>,
    /// The optimisation sense of the problem, indicating whether it is a minimisation or maximisation problem.
    pub sense: Sense,
    /// A `HashMap` where the keys are the names of the objectives and the values are `Objective` structs.
    pub objectives: HashMap<Cow<'a, str>, Objective<'a>>,
    /// A `HashMap` where the keys are the names of the constraints and the values are `Constraint` structs.
    pub constraints: HashMap<Cow<'a, str>, Constraint<'a>>,
    /// A `HashMap` where the keys are the names of the variables and the values are `Variable` structs.
    pub variables: HashMap<&'a str, Variable<'a>>,
}

impl<'a> LpProblem<'a> {
    #[must_use]
    #[inline]
    /// Initialise a new `Self`
    pub fn new() -> Self {
        Self::default()
    }

    /// Ensure a variable exists in the problem, creating it with the given type if not present.
    #[inline]
    fn ensure_variable_exists(&mut self, name: &'a str, var_type: Option<VariableType>) {
        if let Entry::Vacant(entry) = self.variables.entry(name) {
            let variable = var_type.map_or_else(|| Variable::new(name), |vt| Variable::new(name).with_var_type(vt));
            entry.insert(variable);
        }
    }

    #[must_use]
    #[inline]
    /// Override the problem name
    pub fn with_problem_name(self, problem_name: Cow<'a, str>) -> Self {
        Self { name: Some(problem_name), ..self }
    }

    #[must_use]
    #[inline]
    /// Override the problem sense
    pub fn with_sense(self, sense: Sense) -> Self {
        Self { sense, ..self }
    }

    #[must_use]
    #[inline]
    /// Returns the name of the LP Problem
    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    #[must_use]
    #[inline]
    /// Returns `true` if the `Self` a Minimize LP Problem
    pub const fn is_minimization(&self) -> bool {
        self.sense.is_minimisation()
    }

    #[must_use]
    #[inline]
    /// Returns the number of constraints contained within the Problem
    pub fn constraint_count(&self) -> usize {
        self.constraints.len()
    }

    #[must_use]
    #[inline]
    /// Returns the number of objectives contained within the Problem
    pub fn objective_count(&self) -> usize {
        self.objectives.len()
    }

    #[must_use]
    #[inline]
    /// Returns the number of variables contained within the Problem
    pub fn variable_count(&self) -> usize {
        self.variables.len()
    }

    #[inline]
    /// Parse a `Self` from a string slice
    ///
    /// # Errors
    ///
    /// Returns an error if the input string is not a valid LP file format
    pub fn parse(input: &'a str) -> LpResult<Self> {
        log::debug!("Starting to parse LP problem");
        Self::try_from(input)
    }

    #[inline]
    /// Add a new variable to the problem.
    ///
    /// If a variable with the same name already exists, it will be replaced.
    pub fn add_variable(&mut self, variable: Variable<'a>) {
        self.variables.insert(variable.name, variable);
    }

    #[inline]
    /// Add a new constraint to the problem.
    ///
    /// If a constraint with the same name already exists, it will be replaced.
    pub fn add_constraint(&mut self, constraint: Constraint<'a>) {
        let name = constraint.name().as_ref().to_owned();

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

        self.constraints.insert(Cow::Owned(name), constraint);
    }

    #[inline]
    /// Add a new objective to the problem.
    ///
    /// If an objective with the same name already exists, it will be replaced.
    pub fn add_objective(&mut self, objective: Objective<'a>) {
        for coeff in &objective.coefficients {
            self.ensure_variable_exists(coeff.name, None);
        }

        let name = objective.name.clone();
        self.objectives.insert(name, objective);
    }

    // LP Problem Modification Methods

    #[inline]
    /// Update a variable coefficient in an objective.
    ///
    /// If the variable doesn't exist in the objective, it will be added.
    /// If the coefficient value is 0.0, the variable will be removed from the objective.
    ///
    /// # Arguments
    ///
    /// * `objective_name` - Name of the objective to modify
    /// * `variable_name` - Name of the variable to update
    /// * `new_coefficient` - New coefficient value
    ///
    /// # Returns
    ///
    /// `Ok(())` if successful, or an error if the objective doesn't exist
    ///
    /// # Errors
    ///
    /// Returns an error if the specified objective does not exist
    pub fn update_objective_coefficient(&mut self, objective_name: &str, variable_name: &'a str, new_coefficient: f64) -> LpResult<()> {
        let objective = self
            .objectives
            .get_mut(objective_name)
            .ok_or_else(|| LpParseError::validation_error(format!("Objective '{objective_name}' not found")))?;

        // Find existing coefficient
        if let Some(coeff) = objective.coefficients.iter_mut().find(|c| c.name == variable_name) {
            let reference_value = coeff.value;
            if is_effectively_zero(new_coefficient, reference_value) {
                // Remove coefficient if value is effectively zero
                objective.coefficients.retain(|c| c.name != variable_name);
            } else {
                coeff.value = new_coefficient;
            }
        } else if !is_effectively_zero(new_coefficient, 1.0) {
            // Add new coefficient if it doesn't exist and value is non-zero
            objective.coefficients.push(crate::model::Coefficient { name: variable_name, value: new_coefficient });

            // Ensure variable exists using Entry API
            self.variables.entry(variable_name).or_insert_with(|| Variable::new(variable_name));
        }

        Ok(())
    }

    #[inline]
    /// Update a variable coefficient in a constraint.
    ///
    /// If the variable doesn't exist in the constraint, it will be added.
    /// If the coefficient value is 0.0, the variable will be removed from the constraint.
    ///
    /// # Arguments
    ///
    /// * `constraint_name` - Name of the constraint to modify
    /// * `variable_name` - Name of the variable to update
    /// * `new_coefficient` - New coefficient value
    ///
    /// # Returns
    ///
    /// `Ok(())` if successful, or an error if the constraint doesn't exist or is not a standard constraint
    ///
    /// # Errors
    ///
    /// Returns an error if the constraint does not exist or is an SOS constraint
    pub fn update_constraint_coefficient(&mut self, constraint_name: &str, variable_name: &'a str, new_coefficient: f64) -> LpResult<()> {
        let constraint = self
            .constraints
            .get_mut(constraint_name)
            .ok_or_else(|| LpParseError::validation_error(format!("Constraint '{constraint_name}' not found")))?;

        match constraint {
            Constraint::Standard { coefficients, .. } => {
                // Find existing coefficient
                if let Some(coeff) = coefficients.iter_mut().find(|c| c.name == variable_name) {
                    let reference_value = coeff.value;
                    if is_effectively_zero(new_coefficient, reference_value) {
                        // Remove coefficient if value is effectively zero
                        coefficients.retain(|c| c.name != variable_name);
                    } else {
                        coeff.value = new_coefficient;
                    }
                } else if !is_effectively_zero(new_coefficient, 1.0) {
                    // Add new coefficient if it doesn't exist and value is non-zero
                    coefficients.push(crate::model::Coefficient { name: variable_name, value: new_coefficient });

                    // Ensure variable exists using Entry API
                    self.variables.entry(variable_name).or_insert_with(|| Variable::new(variable_name));
                }
            }
            Constraint::SOS { .. } => {
                return Err(LpParseError::validation_error("Cannot update coefficients in SOS constraints using this method"));
            }
        }

        Ok(())
    }

    #[inline]
    /// Update the right-hand side value of a constraint.
    ///
    /// # Arguments
    ///
    /// * `constraint_name` - Name of the constraint to modify
    /// * `new_rhs` - New right-hand side value
    ///
    /// # Returns
    ///
    /// `Ok(())` if successful, or an error if the constraint doesn't exist or is not a standard constraint
    ///
    /// # Errors
    ///
    /// Returns an error if the constraint does not exist or is an SOS constraint
    pub fn update_constraint_rhs(&mut self, constraint_name: &str, new_rhs: f64) -> LpResult<()> {
        let constraint = self
            .constraints
            .get_mut(constraint_name)
            .ok_or_else(|| LpParseError::validation_error(format!("Constraint '{constraint_name}' not found")))?;

        match constraint {
            Constraint::Standard { rhs, .. } => {
                *rhs = new_rhs;
                Ok(())
            }
            Constraint::SOS { .. } => Err(LpParseError::validation_error("SOS constraints do not have right-hand side values")),
        }
    }

    #[inline]
    /// Update the operator of a constraint.
    ///
    /// # Arguments
    ///
    /// * `constraint_name` - Name of the constraint to modify
    /// * `new_operator` - New comparison operator
    ///
    /// # Returns
    ///
    /// `Ok(())` if successful, or an error if the constraint doesn't exist or is not a standard constraint
    ///
    /// # Errors
    ///
    /// Returns an error if the constraint does not exist or is an SOS constraint
    pub fn update_constraint_operator(&mut self, constraint_name: &str, new_operator: crate::model::ComparisonOp) -> LpResult<()> {
        let constraint = self
            .constraints
            .get_mut(constraint_name)
            .ok_or_else(|| LpParseError::validation_error(format!("Constraint '{constraint_name}' not found")))?;

        match constraint {
            Constraint::Standard { operator, .. } => {
                *operator = new_operator;
                Ok(())
            }
            Constraint::SOS { .. } => Err(LpParseError::validation_error("SOS constraints do not have comparison operators")),
        }
    }

    #[inline]
    /// Rename a variable throughout the entire problem.
    ///
    /// This updates the variable name in all objectives, constraints, and the variables map.
    ///
    /// # Arguments
    ///
    /// * `old_name` - Current name of the variable
    /// * `new_name` - New name for the variable
    ///
    /// # Returns
    ///
    /// `Ok(())` if successful, or an error if the variable doesn't exist or the new name already exists
    ///
    /// # Errors
    ///
    /// Returns an error if the variable does not exist or the new name is already in use
    ///
    /// # Panics
    ///
    /// Panics if the variable exists in the map but cannot be removed (internal error)
    pub fn rename_variable(&mut self, old_name: &str, new_name: &'a str) -> LpResult<()> {
        // Check if old variable exists
        if !self.variables.contains_key(old_name) {
            return Err(LpParseError::validation_error(format!("Variable '{old_name}' not found")));
        }

        // Check if new name already exists
        if self.variables.contains_key(new_name) && old_name != new_name {
            return Err(LpParseError::validation_error(format!("Variable '{new_name}' already exists")));
        }

        // Update variable in variables map
        let variable = self.variables.remove(old_name).unwrap();
        let mut new_variable = Variable::new(new_name);
        new_variable.var_type = variable.var_type;
        self.variables.insert(new_name, new_variable);

        // Update variable name in all objectives
        for objective in self.objectives.values_mut() {
            for coeff in &mut objective.coefficients {
                if coeff.name == old_name {
                    coeff.name = new_name;
                }
            }
        }

        // Update variable name in all constraints
        for constraint in self.constraints.values_mut() {
            match constraint {
                Constraint::Standard { coefficients, .. } => {
                    for coeff in coefficients {
                        if coeff.name == old_name {
                            coeff.name = new_name;
                        }
                    }
                }
                Constraint::SOS { weights, .. } => {
                    for weight in weights {
                        if weight.name == old_name {
                            weight.name = new_name;
                        }
                    }
                }
            }
        }

        Ok(())
    }

    #[inline]
    /// Rename a constraint.
    ///
    /// # Arguments
    ///
    /// * `old_name` - Current name of the constraint
    /// * `new_name` - New name for the constraint
    ///
    /// # Returns
    ///
    /// `Ok(())` if successful, or an error if the constraint doesn't exist or the new name already exists
    ///
    /// # Errors
    ///
    /// Returns an error if the constraint does not exist or the new name is already in use
    ///
    /// # Panics
    ///
    /// Panics if the constraint exists in the map but cannot be removed (internal error)
    pub fn rename_constraint(&mut self, old_name: &str, new_name: &str) -> LpResult<()> {
        // Check if old constraint exists
        if !self.constraints.contains_key(old_name) {
            return Err(LpParseError::validation_error(format!("Constraint '{old_name}' not found")));
        }

        // Check if new name already exists
        if self.constraints.contains_key(new_name) && old_name != new_name {
            return Err(LpParseError::validation_error(format!("Constraint '{new_name}' already exists")));
        }

        // Move constraint to new name
        let mut constraint = self.constraints.remove(old_name).unwrap();

        // Update the constraint's internal name
        match &mut constraint {
            Constraint::Standard { name, .. } | Constraint::SOS { name, .. } => {
                *name = Cow::Owned(new_name.to_string());
            }
        }

        self.constraints.insert(Cow::Owned(new_name.to_string()), constraint);

        Ok(())
    }

    #[inline]
    /// Rename an objective.
    ///
    /// # Arguments
    ///
    /// * `old_name` - Current name of the objective
    /// * `new_name` - New name for the objective
    ///
    /// # Returns
    ///
    /// `Ok(())` if successful, or an error if the objective doesn't exist or the new name already exists
    ///
    /// # Errors
    ///
    /// Returns an error if the objective does not exist or the new name is already in use
    ///
    /// # Panics
    ///
    /// Panics if the objective exists in the map but cannot be removed (internal error)
    pub fn rename_objective(&mut self, old_name: &str, new_name: &str) -> LpResult<()> {
        // Check if old objective exists
        if !self.objectives.contains_key(old_name) {
            return Err(LpParseError::validation_error(format!("Objective '{old_name}' not found")));
        }

        // Check if new name already exists
        if self.objectives.contains_key(new_name) && old_name != new_name {
            return Err(LpParseError::validation_error(format!("Objective '{new_name}' already exists")));
        }

        // Move objective to new name
        let mut objective = self.objectives.remove(old_name).unwrap();
        objective.name = Cow::Owned(new_name.to_string());
        self.objectives.insert(Cow::Owned(new_name.to_string()), objective);

        Ok(())
    }

    #[inline]
    /// Remove a variable from the entire problem.
    ///
    /// This removes the variable from all objectives, constraints, and the variables map.
    /// Note: This may result in empty objectives or constraints.
    ///
    /// # Arguments
    ///
    /// * `variable_name` - Name of the variable to remove
    ///
    /// # Returns
    ///
    /// `Ok(())` if successful, or an error if the variable doesn't exist
    ///
    /// # Errors
    ///
    /// Returns an error if the variable does not exist
    pub fn remove_variable(&mut self, variable_name: &str) -> LpResult<()> {
        // Check if variable exists
        if !self.variables.contains_key(variable_name) {
            return Err(LpParseError::validation_error(format!("Variable '{variable_name}' not found")));
        }

        // Remove from variables map
        self.variables.remove(variable_name);

        // Remove from all objectives
        for objective in self.objectives.values_mut() {
            objective.coefficients.retain(|c| c.name != variable_name);
        }

        // Remove from all constraints
        for constraint in self.constraints.values_mut() {
            match constraint {
                Constraint::Standard { coefficients, .. } => {
                    coefficients.retain(|c| c.name != variable_name);
                }
                Constraint::SOS { weights, .. } => {
                    weights.retain(|w| w.name != variable_name);
                }
            }
        }

        Ok(())
    }

    #[inline]
    /// Remove a constraint from the problem.
    ///
    /// # Arguments
    ///
    /// * `constraint_name` - Name of the constraint to remove
    ///
    /// # Returns
    ///
    /// `Ok(())` if successful, or an error if the constraint doesn't exist
    ///
    /// # Errors
    ///
    /// Returns an error if the constraint does not exist
    pub fn remove_constraint(&mut self, constraint_name: &str) -> LpResult<()> {
        if self.constraints.remove(constraint_name).is_none() {
            return Err(LpParseError::validation_error(format!("Constraint '{constraint_name}' not found")));
        }
        Ok(())
    }

    #[inline]
    /// Remove an objective from the problem.
    ///
    /// # Arguments
    ///
    /// * `objective_name` - Name of the objective to remove
    ///
    /// # Returns
    ///
    /// `Ok(())` if successful, or an error if the objective doesn't exist
    ///
    /// # Errors
    ///
    /// Returns an error if the objective does not exist
    pub fn remove_objective(&mut self, objective_name: &str) -> LpResult<()> {
        if self.objectives.remove(objective_name).is_none() {
            return Err(LpParseError::validation_error(format!("Objective '{objective_name}' not found")));
        }
        Ok(())
    }

    #[inline]
    /// Update the type of a variable.
    ///
    /// # Arguments
    ///
    /// * `variable_name` - Name of the variable to modify
    /// * `new_type` - New variable type
    ///
    /// # Returns
    ///
    /// `Ok(())` if successful, or an error if the variable doesn't exist
    ///
    /// # Errors
    ///
    /// Returns an error if the variable does not exist
    pub fn update_variable_type(&mut self, variable_name: &str, new_type: VariableType) -> LpResult<()> {
        let variable = self
            .variables
            .get_mut(variable_name)
            .ok_or_else(|| LpParseError::validation_error(format!("Variable '{variable_name}' not found")))?;

        variable.var_type = new_type;
        Ok(())
    }

    #[inline]
    /// Get a list of all variables referenced in the problem.
    ///
    /// This includes variables from objectives, constraints, and the variables map.
    ///
    /// # Returns
    ///
    /// A vector of variable names
    #[must_use]
    pub fn get_all_variable_names(&self) -> Vec<&str> {
        let mut names = std::collections::HashSet::new();

        // Add from variables map
        for name in self.variables.keys() {
            names.insert(*name);
        }

        // Add from objectives
        for objective in self.objectives.values() {
            for coeff in &objective.coefficients {
                names.insert(coeff.name);
            }
        }

        // Add from constraints
        for constraint in self.constraints.values() {
            match constraint {
                Constraint::Standard { coefficients, .. } => {
                    for coeff in coefficients {
                        names.insert(coeff.name);
                    }
                }
                Constraint::SOS { weights, .. } => {
                    for weight in weights {
                        names.insert(weight.name);
                    }
                }
            }
        }

        let mut result: Vec<&str> = names.into_iter().collect();
        result.sort_unstable();
        result
    }
}

impl std::fmt::Display for LpProblem<'_> {
    #[inline]
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(problem_name) = &self.name {
            writeln!(f, "Problem name: {problem_name}")?;
        }
        writeln!(f, "Sense: {}", self.sense)?;
        writeln!(f, "Objectives: {}", self.objectives.len())?;
        writeln!(f, "Constraints: {}", self.constraints.len())?;
        writeln!(f, "Variables: {}", self.variables.len())?;

        Ok(())
    }
}

// ============================================================================
// Owned LpProblem Variant
// ============================================================================

use crate::model::{ConstraintOwned, ObjectiveOwned, VariableOwned};

/// Owned variant of [`LpProblem`] with no lifetime constraints.
///
/// This struct owns all its data, making it suitable for:
/// - Long-lived data structures that outlive the input string
/// - Mutation-heavy use cases where you need to modify names
/// - Serialization/deserialization without lifetime management
/// - Storing in collections or passing between threads
///
/// # Example
///
/// ```rust
/// use lp_parser::problem::{LpProblem, LpProblemOwned};
///
/// fn process_problem(input: &str) -> LpProblemOwned {
///     let problem = LpProblem::parse(input).unwrap();
///     problem.to_owned() // Convert to owned, input can be dropped
/// }
/// ```
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, PartialEq)]
pub struct LpProblemOwned {
    /// The name of the problem (owned).
    pub name: Option<String>,
    /// The optimization sense (minimize/maximize).
    pub sense: Sense,
    /// The objectives, keyed by name.
    pub objectives: HashMap<String, ObjectiveOwned>,
    /// The constraints, keyed by name.
    pub constraints: HashMap<String, ConstraintOwned>,
    /// The variables, keyed by name.
    pub variables: HashMap<String, VariableOwned>,
}

impl LpProblemOwned {
    /// Create a new empty owned problem with default sense (Minimize).
    #[must_use]
    pub fn new() -> Self {
        Self { name: None, sense: Sense::default(), objectives: HashMap::new(), constraints: HashMap::new(), variables: HashMap::new() }
    }

    /// Set the problem name.
    #[must_use]
    pub fn with_name(self, name: impl Into<String>) -> Self {
        Self { name: Some(name.into()), ..self }
    }

    /// Set the optimization sense.
    #[must_use]
    pub fn with_sense(self, sense: Sense) -> Self {
        Self { sense, ..self }
    }

    /// Returns the name of the problem.
    #[must_use]
    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    /// Returns true if this is a minimization problem.
    #[must_use]
    pub const fn is_minimization(&self) -> bool {
        self.sense.is_minimisation()
    }

    /// Returns the number of objectives.
    #[must_use]
    pub fn objective_count(&self) -> usize {
        self.objectives.len()
    }

    /// Returns the number of constraints.
    #[must_use]
    pub fn constraint_count(&self) -> usize {
        self.constraints.len()
    }

    /// Returns the number of variables.
    #[must_use]
    pub fn variable_count(&self) -> usize {
        self.variables.len()
    }

    /// Add a variable to the problem.
    pub fn add_variable(&mut self, variable: VariableOwned) {
        self.variables.insert(variable.name.clone(), variable);
    }

    /// Add an objective to the problem.
    pub fn add_objective(&mut self, objective: ObjectiveOwned) {
        self.objectives.insert(objective.name.clone(), objective);
    }

    /// Add a constraint to the problem.
    pub fn add_constraint(&mut self, constraint: ConstraintOwned) {
        let name = constraint.name().to_string();
        self.constraints.insert(name, constraint);
    }
}

impl Default for LpProblemOwned {
    fn default() -> Self {
        Self::new()
    }
}

impl<'a> From<&LpProblem<'a>> for LpProblemOwned {
    fn from(problem: &LpProblem<'a>) -> Self {
        Self {
            name: problem.name.as_ref().map(std::string::ToString::to_string),
            sense: problem.sense.clone(),
            objectives: problem.objectives.iter().map(|(k, v)| (k.to_string(), ObjectiveOwned::from(v))).collect(),
            constraints: problem.constraints.iter().map(|(k, v)| (k.to_string(), ConstraintOwned::from(v))).collect(),
            variables: problem.variables.iter().map(|(k, v)| ((*k).to_string(), VariableOwned::from(v))).collect(),
        }
    }
}

impl std::fmt::Display for LpProblemOwned {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(problem_name) = &self.name {
            writeln!(f, "Problem name: {problem_name}")?;
        }
        writeln!(f, "Sense: {}", self.sense)?;
        writeln!(f, "Objectives: {}", self.objectives.len())?;
        writeln!(f, "Constraints: {}", self.constraints.len())?;
        writeln!(f, "Variables: {}", self.variables.len())?;

        Ok(())
    }
}

impl LpProblem<'_> {
    /// Convert to an owned variant with no lifetime constraints.
    ///
    /// This is useful when you need to store the problem in a collection,
    /// pass it between threads, or keep it longer than the input string.
    #[must_use]
    pub fn to_owned(&self) -> LpProblemOwned {
        LpProblemOwned::from(self)
    }
}

impl<'a> TryFrom<&'a str> for LpProblem<'a> {
    type Error = LpParseError;

    #[inline]
    #[allow(clippy::too_many_lines)]
    fn try_from(input: &'a str) -> Result<Self, Self::Error> {
        log::debug!("Starting to parse LP problem with LALRPOP parser");

        // Extract problem name from comments before parsing
        // Supports multiple formats:
        // 1. "\Problem name: my_problem" or "\\Problem name: my_problem"
        // 2. "\* my_problem *\" (CPLEX block comment style)
        let problem_name: Option<Cow<'a, str>> = input.lines().find_map(|line| {
            let trimmed = line.trim();

            // Handle block comment format: \* name *\
            if trimmed.starts_with("\\*") && trimmed.ends_with("*\\") {
                let inner = trimmed.strip_prefix("\\*").unwrap().strip_suffix("*\\").unwrap();
                let name = inner.trim();
                if !name.is_empty() {
                    return Some(Cow::Borrowed(name));
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
                // Case-insensitive "Problem name:" match
                if c.to_lowercase().starts_with("problem name:") { Some(Cow::Borrowed(c["problem name:".len()..].trim())) } else { None }
            })
        });

        // Create lexer and parser
        let lexer = Lexer::new(input);
        let parser = LpProblemParser::new();

        // Parse the LP problem
        let (sense, objectives_vec, constraints_vec, bounds, generals, integers, binaries, semis, sos_constraints) =
            parser.parse(lexer).map_err(LpParseError::from)?;

        // ID generators for unnamed objectives and constraints
        let obj_gen = SequenceGenerator;
        let constraint_gen = SequenceGenerator;

        // Build objectives HashMap and collect variables
        let mut variables: HashMap<&'a str, Variable<'a>> = HashMap::new();
        let mut objectives: HashMap<Cow<'a, str>, Objective<'a>> = HashMap::new();

        for mut obj in objectives_vec {
            // Generate name if empty
            if obj.name.is_empty() {
                obj.name = Cow::Owned(format!("OBJ{}", obj_gen.next_id()));
            }

            // Extract variables from coefficients using Entry API
            for coeff in &obj.coefficients {
                variables.entry(coeff.name).or_insert_with(|| Variable::new(coeff.name));
            }

            objectives.insert(obj.name.clone(), obj);
        }

        // Build constraints HashMap and collect variables
        let mut constraints: HashMap<Cow<'a, str>, Constraint<'a>> = HashMap::new();

        for mut con in constraints_vec {
            // Generate name if empty
            let name = match &con {
                Constraint::Standard { name, .. } | Constraint::SOS { name, .. } => name.clone(),
            };

            let final_name = if name.is_empty() { Cow::Owned(format!("C{}", constraint_gen.next_id())) } else { name };

            // Update constraint with final name and extract variables using Entry API
            match &mut con {
                Constraint::Standard { name, coefficients, .. } => {
                    name.clone_from(&final_name);
                    for coeff in coefficients.iter() {
                        variables.entry(coeff.name).or_insert_with(|| Variable::new(coeff.name));
                    }
                }
                Constraint::SOS { name, weights, .. } => {
                    name.clone_from(&final_name);
                    for coeff in weights.iter() {
                        variables.entry(coeff.name).or_insert_with(|| Variable::new(coeff.name).with_var_type(VariableType::SOS));
                    }
                }
            }

            constraints.insert(final_name, con);
        }

        // Process bounds
        for (var_name, var_type) in bounds {
            match variables.entry(var_name) {
                Entry::Occupied(mut entry) => {
                    entry.get_mut().set_var_type(var_type);
                }
                Entry::Vacant(entry) => {
                    entry.insert(Variable::new(var_name).with_var_type(var_type));
                }
            }
        }

        // Process generals - only set type if variable doesn't already have explicit bounds
        for var_name in generals {
            match variables.entry(var_name) {
                Entry::Occupied(mut entry) => {
                    // Preserve existing bounds (DoubleBound, LowerBound, UpperBound)
                    if matches!(entry.get().var_type, VariableType::Free) {
                        entry.get_mut().set_var_type(VariableType::General);
                    }
                }
                Entry::Vacant(entry) => {
                    entry.insert(Variable::new(var_name).with_var_type(VariableType::General));
                }
            }
        }

        // Process integers - only set type if variable doesn't already have explicit bounds
        for var_name in integers {
            match variables.entry(var_name) {
                Entry::Occupied(mut entry) => {
                    // Preserve existing bounds (DoubleBound, LowerBound, UpperBound)
                    if matches!(entry.get().var_type, VariableType::Free) {
                        entry.get_mut().set_var_type(VariableType::Integer);
                    }
                }
                Entry::Vacant(entry) => {
                    entry.insert(Variable::new(var_name).with_var_type(VariableType::Integer));
                }
            }
        }

        // Process binaries - only set type if variable doesn't already have explicit bounds
        for var_name in binaries {
            match variables.entry(var_name) {
                Entry::Occupied(mut entry) => {
                    // Preserve existing bounds (DoubleBound, LowerBound, UpperBound)
                    if matches!(entry.get().var_type, VariableType::Free) {
                        entry.get_mut().set_var_type(VariableType::Binary);
                    }
                }
                Entry::Vacant(entry) => {
                    entry.insert(Variable::new(var_name).with_var_type(VariableType::Binary));
                }
            }
        }

        // Process semi-continuous - only set type if variable doesn't already have explicit bounds
        for var_name in semis {
            match variables.entry(var_name) {
                Entry::Occupied(mut entry) => {
                    // Preserve existing bounds (DoubleBound, LowerBound, UpperBound)
                    if matches!(entry.get().var_type, VariableType::Free) {
                        entry.get_mut().set_var_type(VariableType::SemiContinuous);
                    }
                }
                Entry::Vacant(entry) => {
                    entry.insert(Variable::new(var_name).with_var_type(VariableType::SemiContinuous));
                }
            }
        }

        // Process SOS constraints
        for mut sos in sos_constraints {
            let name = match &sos {
                Constraint::SOS { name, .. } => name.clone(),
                Constraint::Standard { .. } => continue,
            };

            let final_name = if name.is_empty() { Cow::Owned(format!("SOS{}", constraint_gen.next_id())) } else { name };

            if let Constraint::SOS { name, weights, .. } = &mut sos {
                name.clone_from(&final_name);
                for coeff in weights.iter() {
                    variables.entry(coeff.name).or_insert_with(|| Variable::new(coeff.name).with_var_type(VariableType::SOS));
                }
            }

            constraints.insert(final_name, sos);
        }

        Ok(LpProblem { name: problem_name, sense, objectives, constraints, variables })
    }
}

#[cfg(feature = "serde")]
impl<'de: 'a, 'a> serde::Deserialize<'de> for LpProblem<'a> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(serde::Deserialize)]
        #[serde(field_identifier, rename_all = "lowercase")]
        enum Field {
            Constraints,
            Name,
            Objectives,
            Sense,
            Variables,
        }

        struct LpProblemVisitor<'a>(std::marker::PhantomData<LpProblem<'a>>);

        impl<'de: 'a, 'a> serde::de::Visitor<'de> for LpProblemVisitor<'a> {
            type Value = LpProblem<'a>;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("struct LpProblem")
            }

            fn visit_map<V: serde::de::MapAccess<'de>>(self, mut map: V) -> Result<LpProblem<'a>, V::Error> {
                let mut name: Option<Cow<'_, str>> = None;
                let mut sense = None;
                let mut objectives = None;
                let mut constraints = None;
                let mut variables = None;

                while let Some(key) = map.next_key()? {
                    match key {
                        Field::Name => {
                            if name.is_some() {
                                return Err(serde::de::Error::duplicate_field("name"));
                            }
                            name = map.next_value()?;
                        }
                        Field::Sense => {
                            if sense.is_some() {
                                return Err(serde::de::Error::duplicate_field("sense"));
                            }
                            sense = Some(map.next_value()?);
                        }
                        Field::Objectives => {
                            if objectives.is_some() {
                                return Err(serde::de::Error::duplicate_field("objectives"));
                            }
                            objectives = Some(map.next_value()?);
                        }
                        Field::Constraints => {
                            if constraints.is_some() {
                                return Err(serde::de::Error::duplicate_field("constraints"));
                            }
                            constraints = Some(map.next_value()?);
                        }
                        Field::Variables => {
                            if variables.is_some() {
                                return Err(serde::de::Error::duplicate_field("variables"));
                            }
                            variables = Some(map.next_value()?);
                        }
                    }
                }

                Ok(LpProblem {
                    name,
                    sense: sense.unwrap_or_default(),
                    objectives: objectives.unwrap_or_default(),
                    constraints: constraints.unwrap_or_default(),
                    variables: variables.unwrap_or_default(),
                })
            }
        }

        const FIELDS: &[&str] = &["name", "sense", "objectives", "constraints", "variables"];
        deserializer.deserialize_struct("LpProblem", FIELDS, LpProblemVisitor(std::marker::PhantomData))
    }
}

#[cfg(test)]
mod test {
    use std::borrow::Cow;

    use crate::model::{Coefficient, ComparisonOp, Constraint, Objective, Sense, Variable, VariableType};
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
        // Small input
        let problem = LpProblem::try_from(SMALL_INPUT).unwrap();
        assert_eq!(problem.objectives.len(), 3);
        assert_eq!(problem.constraints.len(), 3);

        // Complete input
        let problem = LpProblem::try_from(COMPLETE_INPUT).unwrap();
        assert_eq!(problem.objectives.len(), 3);
        assert_eq!(problem.constraints.len(), 5);

        #[cfg(feature = "serde")]
        {
            insta::assert_yaml_snapshot!("small_input", &LpProblem::try_from(SMALL_INPUT).unwrap(), {
                ".objectives" => insta::sorted_redaction(),
                ".constraints" => insta::sorted_redaction(),
                ".variables" => insta::sorted_redaction()
            });
            insta::assert_yaml_snapshot!("complete_input", &LpProblem::try_from(COMPLETE_INPUT).unwrap(), {
                ".objectives" => insta::sorted_redaction(),
                ".constraints" => insta::sorted_redaction(),
                ".variables" => insta::sorted_redaction()
            });
        }
    }

    #[cfg(feature = "serde")]
    #[test]
    fn test_serialization_lifecycle() {
        let problem = LpProblem::try_from(COMPLETE_INPUT).unwrap();
        let serialized = serde_json::to_string(&problem).unwrap();
        let _: LpProblem<'_> = serde_json::from_str(&serialized).unwrap();
    }

    #[test]
    fn test_problem_lifecycle() {
        // New problem defaults
        let problem = LpProblem::new();
        assert_eq!(problem.name(), None);
        assert!(problem.is_minimization());
        assert_eq!((problem.objective_count(), problem.constraint_count(), problem.variable_count()), (0, 0, 0));

        // Builder pattern
        let problem = LpProblem::new().with_problem_name(Cow::Borrowed("test")).with_sense(Sense::Maximize);
        assert_eq!(problem.name(), Some("test"));
        assert!(!problem.is_minimization());

        // Display formatting
        let display = format!("{problem}");
        assert!(display.contains("Problem name: test") && display.contains("Sense: Maximize"));
    }

    #[test]
    fn test_add_and_replace_elements() {
        let mut problem = LpProblem::new();

        // Add variable
        problem.add_variable(Variable::new("x1").with_var_type(VariableType::Binary));
        assert_eq!(problem.variable_count(), 1);

        // Replace variable
        problem.add_variable(Variable::new("x1").with_var_type(VariableType::Integer));
        assert_eq!(problem.variable_count(), 1);
        assert_eq!(problem.variables["x1"].var_type, VariableType::Integer);

        // Add constraint (auto-creates variables)
        problem.add_constraint(Constraint::Standard {
            name: Cow::Borrowed("c1"),
            coefficients: vec![Coefficient { name: "x1", value: 1.0 }, Coefficient { name: "x2", value: 2.0 }],
            operator: ComparisonOp::LTE,
            rhs: 5.0,
        });
        assert_eq!(problem.constraint_count(), 1);
        assert_eq!(problem.variable_count(), 2);

        // Add objective
        problem.add_objective(Objective { name: Cow::Borrowed("obj1"), coefficients: vec![Coefficient { name: "x3", value: 1.0 }] });
        assert_eq!(problem.objective_count(), 1);
        assert_eq!(problem.variable_count(), 3);

        // SOS constraint creates SOS-typed variables
        problem.add_constraint(Constraint::SOS {
            name: Cow::Borrowed("sos1"),
            sos_type: crate::model::SOSType::S1,
            weights: vec![Coefficient { name: "s1", value: 1.0 }],
        });
        assert_eq!(problem.variables["s1"].var_type, VariableType::SOS);
    }

    #[test]
    fn test_parsing_variations() {
        // Minimal
        let p = LpProblem::parse("minimize\nx1\nsubject to\nx1 <= 1\nend").unwrap();
        assert_eq!(p.sense, Sense::Minimize);
        assert_eq!((p.objective_count(), p.constraint_count()), (1, 1));

        // Maximize
        let p = LpProblem::parse("maximize\n2x1 + 3x2\nsubject to\nx1 + x2 <= 10\nend").unwrap();
        assert_eq!(p.sense, Sense::Maximize);

        // Multiple objectives and constraints
        let p = LpProblem::parse("minimize\nobj1: x1\nobj2: x2\nsubject to\nc1: x1 <= 10\nc2: x1 >= 0\nend").unwrap();
        assert_eq!((p.objective_count(), p.constraint_count()), (2, 2));

        // Variable types - test individually due to lifetime constraints
        let input = "minimize\nx1\nsubject to\nx1 <= 1\nintegers\nx1\nend";
        assert_eq!(LpProblem::parse(input).unwrap().variables["x1"].var_type, VariableType::Integer);

        let input = "minimize\nx1\nsubject to\nx1 <= 1\nbinaries\nx1\nend";
        assert_eq!(LpProblem::parse(input).unwrap().variables["x1"].var_type, VariableType::Binary);

        let input = "minimize\nx1\nsubject to\nx1 <= 1\ngenerals\nx1\nend";
        assert_eq!(LpProblem::parse(input).unwrap().variables["x1"].var_type, VariableType::General);

        let input = "minimize\nx1\nsubject to\nx1 <= 1\nsemi-continuous\nx1\nend";
        assert_eq!(LpProblem::parse(input).unwrap().variables["x1"].var_type, VariableType::SemiContinuous);

        // Bounds
        let p = LpProblem::parse("minimize\nx1 + x2\nsubject to\nx1 <= 10\nbounds\nx1 >= 0\nx2 <= 5\nend").unwrap();
        assert!(matches!(p.variables["x1"].var_type, VariableType::LowerBound(0.0)));
        assert!(matches!(p.variables["x2"].var_type, VariableType::UpperBound(5.0)));

        // Empty constraints section is valid
        assert!(LpProblem::parse("minimize\nx1\nsubject to\nend").is_ok());
    }

    #[test]
    fn test_parse_errors() {
        let invalid = [
            "",                                   // Empty
            "   \n\t  ",                          // Whitespace only
            "invalid_sense\nx1\nsubject to\nend", // Invalid sense
            "minimize\nend",                      // Missing subject to
            "minimize\nsubject to\nx1 <= 1\nend", // Empty objectives
            "minimize\nx1\nsubject",              // Incomplete header
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
        // Mixed tabs/spaces
        assert!(LpProblem::parse("minimize\n\tx1\t+\t x2 \nsubject to\n\t x1\t+ x2\t<=\t10\nend").is_ok());
        // Excessive newlines
        assert!(LpProblem::parse("\n\n\nminimize\n\n\nx1\n\n\nsubject to\n\n\nx1 <= 1\n\n\nend\n\n\n").is_ok());
        // Carriage returns
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
        // Infinity
        assert!(LpProblem::parse("minimize\n-inf x1\nsubject to\nx1 >= -infinity\nend").is_ok());
        // Zero values
        assert!(LpProblem::parse("minimize\n0x1 + 0x2\nsubject to\n0x1 + 0x2 = 0\nend").is_ok());
        // Extreme numbers
        let input = format!("minimize\n{}x1\nsubject to\nx1 <= {}\nend", f64::MAX, f64::MAX);
        assert!(LpProblem::parse(&input).is_ok());
    }

    #[test]
    fn test_name_patterns() {
        let mut problem = LpProblem::new();
        let names = ["x", "X1", "var_123", "x.1.2", "_var", "VAR123ABC"];
        for name in names {
            problem.add_variable(Variable::new(name));
        }
        assert_eq!(problem.variable_count(), names.len());
    }

    #[test]
    fn test_large_problem() {
        let mut problem = LpProblem::new();
        for i in 0..100 {
            let name: &'static str = Box::leak(format!("x{i}").into_boxed_str());
            problem.add_variable(Variable::new(name));
            problem.add_constraint(Constraint::Standard {
                name: Cow::Owned(format!("c{i}")),
                coefficients: vec![Coefficient { name, value: 1.0 }],
                operator: ComparisonOp::LTE,
                rhs: 10.0,
            });
        }
        assert_eq!(problem.variable_count(), 100);
        assert_eq!(problem.constraint_count(), 100);
    }
}

#[cfg(test)]
mod modification_tests {
    use std::borrow::Cow;

    use crate::model::{Coefficient, ComparisonOp, Constraint, Objective, Sense, VariableType};
    use crate::problem::LpProblem;

    fn create_test_problem<'a>() -> LpProblem<'a> {
        let mut problem = LpProblem::new().with_sense(Sense::Minimize);
        problem.add_objective(Objective {
            name: Cow::Borrowed("obj1"),
            coefficients: vec![Coefficient { name: "x1", value: 2.0 }, Coefficient { name: "x2", value: 3.0 }],
        });
        problem.add_constraint(Constraint::Standard {
            name: Cow::Borrowed("c1"),
            coefficients: vec![Coefficient { name: "x1", value: 1.0 }, Coefficient { name: "x2", value: 1.0 }],
            operator: ComparisonOp::LTE,
            rhs: 10.0,
        });
        problem
    }

    #[test]
    fn test_update_coefficients() {
        let mut p = create_test_problem();

        // Objective: update, add, remove
        p.update_objective_coefficient("obj1", "x1", 5.0).unwrap();
        p.update_objective_coefficient("obj1", "x3", 1.5).unwrap();
        p.update_objective_coefficient("obj1", "x2", 0.0).unwrap();
        let coeffs: Vec<_> = p.objectives["obj1"].coefficients.iter().map(|c| (c.name, c.value)).collect();
        assert!(coeffs.contains(&("x1", 5.0)) && coeffs.contains(&("x3", 1.5)));
        assert!(!coeffs.iter().any(|(n, _)| *n == "x2"));

        // Constraint: update, add, remove
        p.update_constraint_coefficient("c1", "x1", 3.0).unwrap();
        p.update_constraint_coefficient("c1", "x3", 2.5).unwrap();
        p.update_constraint_coefficient("c1", "x2", 0.0).unwrap();

        // RHS and operator
        p.update_constraint_rhs("c1", 15.0).unwrap();
        p.update_constraint_operator("c1", ComparisonOp::GTE).unwrap();
        if let Constraint::Standard { rhs, operator, .. } = p.constraints.get("c1").unwrap() {
            assert_eq!((*rhs, operator), (15.0, &ComparisonOp::GTE));
        }

        // Errors
        assert!(p.update_objective_coefficient("nonexistent", "x1", 1.0).is_err());
        assert!(p.update_constraint_coefficient("nonexistent", "x1", 1.0).is_err());
    }

    #[test]
    fn test_rename_operations() {
        let mut p = create_test_problem();

        // Variable rename propagates everywhere
        p.rename_variable("x1", "new_x1").unwrap();
        assert!(!p.variables.contains_key("x1") && p.variables.contains_key("new_x1"));
        assert!(p.objectives["obj1"].coefficients.iter().any(|c| c.name == "new_x1"));

        // Constraint rename
        p.rename_constraint("c1", "new_c1").unwrap();
        assert!(!p.constraints.contains_key("c1") && p.constraints.contains_key("new_c1"));

        // Objective rename
        p.rename_objective("obj1", "new_obj1").unwrap();
        assert!(!p.objectives.contains_key("obj1") && p.objectives.contains_key("new_obj1"));

        // Errors - nonexistent and name collision
        assert!(p.rename_variable("nonexistent", "x").is_err());
        assert!(p.rename_variable("new_x1", "x2").is_err()); // Name already exists
    }

    #[test]
    fn test_remove_operations() {
        let mut p = create_test_problem();

        p.remove_variable("x2").unwrap();
        assert!(!p.variables.contains_key("x2"));
        assert!(!p.objectives["obj1"].coefficients.iter().any(|c| c.name == "x2"));

        p.remove_constraint("c1").unwrap();
        assert!(!p.constraints.contains_key("c1"));

        p.remove_objective("obj1").unwrap();
        assert!(!p.objectives.contains_key("obj1"));

        // Errors for nonexistent
        assert!(p.remove_constraint("c1").is_err());
        assert!(p.remove_objective("obj1").is_err());
    }

    #[test]
    fn test_variable_type_update() {
        let mut p = create_test_problem();
        p.update_variable_type("x1", VariableType::Binary).unwrap();
        assert_eq!(p.variables["x1"].var_type, VariableType::Binary);
    }

    #[test]
    fn test_sos_constraint_restrictions() {
        let mut p = LpProblem::new();
        p.add_constraint(Constraint::SOS {
            name: Cow::Borrowed("sos1"),
            sos_type: crate::model::SOSType::S1,
            weights: vec![Coefficient { name: "x1", value: 1.0 }],
        });

        // Can't modify SOS coefficients/rhs/operator
        assert!(p.update_constraint_coefficient("sos1", "x1", 3.0).is_err());
        assert!(p.update_constraint_rhs("sos1", 5.0).is_err());
        assert!(p.update_constraint_operator("sos1", ComparisonOp::LTE).is_err());

        // But can rename and remove
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
