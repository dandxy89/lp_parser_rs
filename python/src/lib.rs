// Allow pedantic lints that are unavoidable due to PyO3 macro requirements
// syn v1 is used by diff_derive (transitive dep of diff-struct via lp_parser_rs) - unavoidable
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::multiple_crate_versions,
    clippy::needless_pass_by_value,
    clippy::unnecessary_wraps
)]

use std::path::{Path, PathBuf};

use lp_parser_rs::analysis::AnalysisConfig;
use lp_parser_rs::model::{Constraint, Sense, VariableType};
use lp_parser_rs::parser::parse_file;
use lp_parser_rs::problem::LpProblem;
use lp_parser_rs::writer::{LpWriterOptions, write_lp_string, write_lp_string_with_options};
use pyo3::create_exception;
use pyo3::exceptions::{PyFileExistsError, PyNotADirectoryError, PyRuntimeError};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList};

create_exception!(parse_lp, LpParseError, PyRuntimeError, "Raised when an LP file or problem cannot be parsed.");
create_exception!(parse_lp, LpNotParsedError, PyRuntimeError, "Raised when a method requires parse() to have been called first.");
create_exception!(
    parse_lp,
    LpObjectNotFoundError,
    PyRuntimeError,
    "Raised when a named variable, constraint or objective cannot be found."
);
create_exception!(parse_lp, LpInvalidValueError, PyRuntimeError, "Raised when an input value is invalid.");

#[pyclass]
pub struct LpParser {
    lp_file: String,
    parsed_content: Option<String>,
}

#[pymethods]
impl LpParser {
    #[new]
    #[pyo3(signature = (lp_file))]
    fn new(lp_file: String) -> PyResult<Self> {
        if !Path::new(&lp_file).is_file() {
            return Err(PyFileExistsError::new_err(format!("LP file '{lp_file}' does not exist or is not a file")));
        }

        Ok(Self { lp_file, parsed_content: None })
    }

    #[getter]
    fn lp_file(&self) -> String {
        self.lp_file.clone()
    }

    #[pyo3(text_signature = "($self)")]
    fn parse(&mut self) -> PyResult<()> {
        let input =
            parse_file(&PathBuf::from(&self.lp_file)).map_err(|err| LpParseError::new_err(format!("Unable to read LP file: {err}")))?;
        self.parsed_content = Some(input);
        Ok(())
    }

    #[allow(clippy::wrong_self_convention)]
    #[pyo3(text_signature = "($self, base_directory)")]
    fn to_csv(&mut self, base_directory: &str) -> PyResult<()> {
        if !Path::new(&base_directory).is_dir() {
            return Err(PyNotADirectoryError::new_err(format!("Path {base_directory} is not a directory.")));
        }

        // Parse if not already parsed
        if self.parsed_content.is_none() {
            self.parse()?;
        }

        let problem = self.get_problem()?;
        problem
            .to_csv(Path::new(base_directory))
            .map_err(|err| PyRuntimeError::new_err(format!("Unable to write to .csv files: {err}")))?;

        Ok(())
    }

    #[getter]
    fn name(&self) -> PyResult<Option<String>> {
        let problem = self.get_problem()?;
        Ok(problem.name.map(|n| {
            // Remove "Problem name: " prefix if present
            match n.strip_prefix("Problem name: ") {
                Some(stripped) => stripped.to_string(),
                None => n,
            }
        }))
    }

    #[getter]
    fn sense(&self) -> PyResult<String> {
        let problem = self.get_problem()?;
        Ok(match problem.sense {
            Sense::Maximize => "maximize".to_string(),
            Sense::Minimize => "minimize".to_string(),
        })
    }

    #[getter]
    fn objectives(&self, py: Python) -> PyResult<Py<PyAny>> {
        let problem = self.get_problem()?;
        let list = PyList::empty(py);

        for (name_id, obj) in &problem.objectives {
            let dict = PyDict::new(py);
            dict.set_item("name", problem.resolve(*name_id))?;

            let coeffs = PyList::empty(py);
            for coef in &obj.coefficients {
                let coef_dict = PyDict::new(py);
                coef_dict.set_item("name", problem.resolve(coef.name))?;
                coef_dict.set_item("value", coef.value)?;
                coeffs.append(coef_dict)?;
            }
            dict.set_item("coefficients", coeffs)?;
            list.append(dict)?;
        }

        Ok(list.into())
    }

    #[getter]
    fn constraints(&self, py: Python) -> PyResult<Py<PyAny>> {
        let problem = self.get_problem()?;
        let list = PyList::empty(py);

        for (name_id, constraint) in &problem.constraints {
            let dict = PyDict::new(py);
            dict.set_item("name", problem.resolve(*name_id))?;

            match constraint {
                Constraint::Standard { coefficients, operator, rhs, .. } => {
                    dict.set_item("type", "standard")?;

                    let coeffs = PyList::empty(py);
                    for coef in coefficients {
                        let coef_dict = PyDict::new(py);
                        coef_dict.set_item("name", problem.resolve(coef.name))?;
                        coef_dict.set_item("value", coef.value)?;
                        coeffs.append(coef_dict)?;
                    }
                    dict.set_item("coefficients", coeffs)?;
                    dict.set_item("operator", format!("{operator:?}"))?;
                    dict.set_item("rhs", rhs)?;
                }
                Constraint::SOS { weights, sos_type, .. } => {
                    dict.set_item("type", "sos")?;
                    dict.set_item("sos_type", format!("{sos_type:?}"))?;

                    let weights_list = PyList::empty(py);
                    for weight in weights {
                        let weight_dict = PyDict::new(py);
                        weight_dict.set_item("name", problem.resolve(weight.name))?;
                        weight_dict.set_item("value", weight.value)?;
                        weights_list.append(weight_dict)?;
                    }
                    dict.set_item("weights", weights_list)?;
                }
            }
            list.append(dict)?;
        }

        Ok(list.into())
    }

    #[getter]
    fn variables(&self, py: Python) -> PyResult<Py<PyAny>> {
        let problem = self.get_problem()?;
        let dict = PyDict::new(py);

        for (name_id, var) in &problem.variables {
            let resolved_name = problem.resolve(*name_id);
            let var_dict = PyDict::new(py);
            var_dict.set_item("name", resolved_name)?;
            var_dict.set_item("var_type", format!("{:?}", var.var_type))?;
            dict.set_item(resolved_name, var_dict)?;
        }

        Ok(dict.into())
    }

    fn variable_count(&self) -> PyResult<usize> {
        let problem = self.get_problem()?;
        Ok(problem.variable_count())
    }

    fn constraint_count(&self) -> PyResult<usize> {
        let problem = self.get_problem()?;
        Ok(problem.constraint_count())
    }

    fn objective_count(&self) -> PyResult<usize> {
        let problem = self.get_problem()?;
        Ok(problem.objective_count())
    }

    #[pyo3(text_signature = "($self, other)")]
    fn compare(&self, other: &Self, py: Python) -> PyResult<Py<PyAny>> {
        let p1 = self.get_problem()?;
        let p2 = other.get_problem()?;

        let dict = PyDict::new(py);

        // Compare basic properties
        dict.set_item("name_changed", p1.name != p2.name)?;
        dict.set_item("sense_changed", p1.sense != p2.sense)?;

        // Compare counts
        dict.set_item("variable_count_diff", p1.variable_count() as i32 - p2.variable_count() as i32)?;
        dict.set_item("constraint_count_diff", p1.constraint_count() as i32 - p2.constraint_count() as i32)?;
        dict.set_item("objective_count_diff", p1.objective_count() as i32 - p2.objective_count() as i32)?;

        // Find added/removed variables
        let added_vars = PyList::empty(py);
        let removed_vars = PyList::empty(py);
        let modified_vars = PyList::empty(py);

        for (name_id, var1) in &p1.variables {
            let resolved_name = p1.resolve(*name_id);
            if let Some(p2_name_id) = p2.get_name_id(resolved_name) {
                if let Some(var2) = p2.variables.get(&p2_name_id) {
                    if var1 != var2 {
                        modified_vars.append(resolved_name)?;
                    }
                } else {
                    removed_vars.append(resolved_name)?;
                }
            } else {
                removed_vars.append(resolved_name)?;
            }
        }

        for name_id in p2.variables.keys() {
            let resolved_name = p2.resolve(*name_id);
            let in_p1 = p1.get_name_id(resolved_name).is_some_and(|id| p1.variables.contains_key(&id));
            if !in_p1 {
                added_vars.append(resolved_name)?;
            }
        }

        dict.set_item("added_variables", added_vars)?;
        dict.set_item("removed_variables", removed_vars)?;
        dict.set_item("modified_variables", modified_vars)?;

        // Find added/removed constraints
        let added_constraints = PyList::empty(py);
        let removed_constraints = PyList::empty(py);

        for name_id in p1.constraints.keys() {
            let resolved_name = p1.resolve(*name_id);
            let in_p2 = p2.get_name_id(resolved_name).is_some_and(|id| p2.constraints.contains_key(&id));
            if !in_p2 {
                removed_constraints.append(resolved_name)?;
            }
        }

        for name_id in p2.constraints.keys() {
            let resolved_name = p2.resolve(*name_id);
            let in_p1 = p1.get_name_id(resolved_name).is_some_and(|id| p1.constraints.contains_key(&id));
            if !in_p1 {
                added_constraints.append(resolved_name)?;
            }
        }

        dict.set_item("added_constraints", added_constraints)?;
        dict.set_item("removed_constraints", removed_constraints)?;

        Ok(dict.into())
    }

    /// Write the current problem to LP format string
    #[pyo3(text_signature = "($self)")]
    fn to_lp_string(&self) -> PyResult<String> {
        let problem = self.get_problem()?;
        write_lp_string(&problem).map_err(|err| PyRuntimeError::new_err(format!("Failed to write LP string: {err}")))
    }

    /// Write the current problem to LP format string with custom options
    #[pyo3(signature = (*, include_problem_name=true, max_line_length=80, decimal_precision=6, include_section_spacing=true))]
    fn to_lp_string_with_options(
        &self,
        include_problem_name: bool,
        max_line_length: usize,
        decimal_precision: usize,
        include_section_spacing: bool,
    ) -> PyResult<String> {
        let problem = self.get_problem()?;
        let options = LpWriterOptions { include_problem_name, max_line_length, decimal_precision, include_section_spacing };
        write_lp_string_with_options(&problem, &options).map_err(|err| PyRuntimeError::new_err(format!("Failed to write LP string: {err}")))
    }

    /// Save the current problem to an LP file
    #[pyo3(text_signature = "($self, filepath)")]
    fn save_to_file(&self, filepath: String) -> PyResult<()> {
        let lp_content = self.to_lp_string()?;
        std::fs::write(&filepath, lp_content).map_err(|err| PyRuntimeError::new_err(format!("Failed to write file: {err}")))
    }

    /// Update coefficient in an objective
    #[pyo3(text_signature = "($self, objective_name, variable_name, coefficient)")]
    fn update_objective_coefficient(&mut self, objective_name: String, variable_name: String, coefficient: f64) -> PyResult<()> {
        let mut problem = self.get_problem()?;
        problem
            .update_objective_coefficient(&objective_name, &variable_name, coefficient)
            .map_err(|err| LpObjectNotFoundError::new_err(format!("Failed to update objective coefficient: {err}")))?;

        // Update the cached content
        let updated_content =
            write_lp_string(&problem).map_err(|err| PyRuntimeError::new_err(format!("Failed to serialize updated problem: {err}")))?;
        self.parsed_content = Some(updated_content);
        Ok(())
    }

    /// Rename an objective
    #[pyo3(text_signature = "($self, old_name, new_name)")]
    fn rename_objective(&mut self, old_name: String, new_name: String) -> PyResult<()> {
        let mut problem = self.get_problem()?;
        problem
            .rename_objective(&old_name, &new_name)
            .map_err(|err| LpObjectNotFoundError::new_err(format!("Failed to rename objective: {err}")))?;

        let updated_content =
            write_lp_string(&problem).map_err(|err| PyRuntimeError::new_err(format!("Failed to serialize updated problem: {err}")))?;
        self.parsed_content = Some(updated_content);
        Ok(())
    }

    /// Remove an objective
    #[pyo3(text_signature = "($self, objective_name)")]
    fn remove_objective(&mut self, objective_name: String) -> PyResult<()> {
        let mut problem = self.get_problem()?;
        problem
            .remove_objective(&objective_name)
            .map_err(|err| LpObjectNotFoundError::new_err(format!("Failed to remove objective: {err}")))?;

        let updated_content =
            write_lp_string(&problem).map_err(|err| PyRuntimeError::new_err(format!("Failed to serialize updated problem: {err}")))?;
        self.parsed_content = Some(updated_content);
        Ok(())
    }

    /// Update coefficient in a constraint
    #[pyo3(text_signature = "($self, constraint_name, variable_name, coefficient)")]
    fn update_constraint_coefficient(&mut self, constraint_name: String, variable_name: String, coefficient: f64) -> PyResult<()> {
        let mut problem = self.get_problem()?;
        problem
            .update_constraint_coefficient(&constraint_name, &variable_name, coefficient)
            .map_err(|err| LpObjectNotFoundError::new_err(format!("Failed to update constraint coefficient: {err}")))?;

        let updated_content =
            write_lp_string(&problem).map_err(|err| PyRuntimeError::new_err(format!("Failed to serialize updated problem: {err}")))?;
        self.parsed_content = Some(updated_content);
        Ok(())
    }

    /// Update the right-hand side value of a constraint
    #[pyo3(text_signature = "($self, constraint_name, new_rhs)")]
    fn update_constraint_rhs(&mut self, constraint_name: String, new_rhs: f64) -> PyResult<()> {
        let mut problem = self.get_problem()?;
        problem
            .update_constraint_rhs(&constraint_name, new_rhs)
            .map_err(|err| LpObjectNotFoundError::new_err(format!("Failed to update constraint RHS: {err}")))?;

        let updated_content =
            write_lp_string(&problem).map_err(|err| PyRuntimeError::new_err(format!("Failed to serialize updated problem: {err}")))?;
        self.parsed_content = Some(updated_content);
        Ok(())
    }

    /// Rename a constraint
    #[pyo3(text_signature = "($self, old_name, new_name)")]
    fn rename_constraint(&mut self, old_name: String, new_name: String) -> PyResult<()> {
        let mut problem = self.get_problem()?;
        problem
            .rename_constraint(&old_name, &new_name)
            .map_err(|err| LpObjectNotFoundError::new_err(format!("Failed to rename constraint: {err}")))?;

        let updated_content =
            write_lp_string(&problem).map_err(|err| PyRuntimeError::new_err(format!("Failed to serialize updated problem: {err}")))?;
        self.parsed_content = Some(updated_content);
        Ok(())
    }

    /// Remove a constraint
    #[pyo3(text_signature = "($self, constraint_name)")]
    fn remove_constraint(&mut self, constraint_name: String) -> PyResult<()> {
        let mut problem = self.get_problem()?;
        problem
            .remove_constraint(&constraint_name)
            .map_err(|err| LpObjectNotFoundError::new_err(format!("Failed to remove constraint: {err}")))?;

        let updated_content =
            write_lp_string(&problem).map_err(|err| PyRuntimeError::new_err(format!("Failed to serialize updated problem: {err}")))?;
        self.parsed_content = Some(updated_content);
        Ok(())
    }

    /// Rename a variable across all objectives and constraints
    #[pyo3(text_signature = "($self, old_name, new_name)")]
    fn rename_variable(&mut self, old_name: String, new_name: String) -> PyResult<()> {
        let mut problem = self.get_problem()?;
        problem
            .rename_variable(&old_name, &new_name)
            .map_err(|err| LpObjectNotFoundError::new_err(format!("Failed to rename variable: {err}")))?;

        let updated_content =
            write_lp_string(&problem).map_err(|err| PyRuntimeError::new_err(format!("Failed to serialize updated problem: {err}")))?;
        self.parsed_content = Some(updated_content);
        Ok(())
    }

    /// Update variable type (e.g., Binary, Integer, etc.)
    #[pyo3(text_signature = "($self, variable_name, var_type)")]
    fn update_variable_type(&mut self, variable_name: String, var_type: String) -> PyResult<()> {
        let mut problem = self.get_problem()?;

        // Parse the variable type string
        let variable_type = match var_type.to_lowercase().as_str() {
            "binary" => VariableType::Binary,
            "integer" => VariableType::Integer,
            "general" => VariableType::General,
            "free" => VariableType::Free,
            "semicontinuous" | "semi_continuous" => VariableType::SemiContinuous,
            _ => {
                return Err(LpInvalidValueError::new_err(format!(
                    "Unknown variable type: {var_type}. Supported types: binary, integer, general, free, semicontinuous",
                )));
            }
        };

        problem
            .update_variable_type(&variable_name, variable_type)
            .map_err(|err| LpObjectNotFoundError::new_err(format!("Failed to update variable type: {err}")))?;

        let updated_content =
            write_lp_string(&problem).map_err(|err| PyRuntimeError::new_err(format!("Failed to serialize updated problem: {err}")))?;
        self.parsed_content = Some(updated_content);
        Ok(())
    }

    /// Remove a variable from all objectives and constraints
    #[pyo3(text_signature = "($self, variable_name)")]
    fn remove_variable(&mut self, variable_name: String) -> PyResult<()> {
        let mut problem = self.get_problem()?;
        problem
            .remove_variable(&variable_name)
            .map_err(|err| LpObjectNotFoundError::new_err(format!("Failed to remove variable: {err}")))?;

        let updated_content =
            write_lp_string(&problem).map_err(|err| PyRuntimeError::new_err(format!("Failed to serialize updated problem: {err}")))?;
        self.parsed_content = Some(updated_content);
        Ok(())
    }

    /// Set problem name
    #[pyo3(text_signature = "($self, name)")]
    fn set_problem_name(&mut self, name: String) -> PyResult<()> {
        let mut problem = self.get_problem()?;
        problem.name = Some(name);

        let updated_content =
            write_lp_string(&problem).map_err(|err| PyRuntimeError::new_err(format!("Failed to serialize updated problem: {err}")))?;
        self.parsed_content = Some(updated_content);
        Ok(())
    }

    /// Set problem sense (maximize or minimize)
    #[pyo3(text_signature = "($self, sense)")]
    fn set_sense(&mut self, sense: String) -> PyResult<()> {
        let mut problem = self.get_problem()?;

        problem.sense = match sense.to_lowercase().as_str() {
            "maximize" | "max" => Sense::Maximize,
            "minimize" | "min" => Sense::Minimize,
            _ => return Err(LpInvalidValueError::new_err(format!("Invalid sense: {sense}. Use 'maximize' or 'minimize'"))),
        };

        let updated_content =
            write_lp_string(&problem).map_err(|err| PyRuntimeError::new_err(format!("Failed to serialize updated problem: {err}")))?;
        self.parsed_content = Some(updated_content);
        Ok(())
    }

    /// Perform comprehensive analysis on the LP problem.
    ///
    /// Returns a dictionary containing:
    /// - summary: Basic statistics (counts, density, etc.)
    /// - sparsity: Sparsity metrics (variables per constraint, connectivity)
    /// - variables: Variable analysis (type distribution, invalid bounds, etc.)
    /// - constraints: Constraint analysis (type distribution, empty/singleton)
    /// - coefficients: Coefficient range analysis
    /// - objectives: Objective analysis
    /// - issues: List of detected issues/warnings
    #[pyo3(text_signature = "($self)")]
    fn analyze(&self, py: Python) -> PyResult<Py<PyAny>> {
        let problem = self.get_problem()?;
        let analysis = problem.analyze_with_config(&AnalysisConfig::default());
        self.analysis_to_dict(py, &analysis)
    }

    /// Perform analysis with custom thresholds.
    ///
    /// Args:
    ///     `large_coeff_threshold`: Threshold for large coefficient warnings (default: 1e9)
    ///     `small_coeff_threshold`: Threshold for small coefficient warnings (default: 1e-9)
    ///     `ratio_threshold`: Coefficient ratio threshold for scaling warnings (default: 1e6)
    #[pyo3(signature = (*, large_coeff_threshold=1e9, small_coeff_threshold=1e-9, ratio_threshold=1e6))]
    fn analyze_with_config(
        &self,
        py: Python,
        large_coeff_threshold: f64,
        small_coeff_threshold: f64,
        ratio_threshold: f64,
    ) -> PyResult<Py<PyAny>> {
        let problem = self.get_problem()?;
        let config = AnalysisConfig {
            large_coefficient_threshold: large_coeff_threshold,
            small_coefficient_threshold: small_coeff_threshold,
            large_rhs_threshold: large_coeff_threshold,
            coefficient_ratio_threshold: ratio_threshold,
        };
        let analysis = problem.analyze_with_config(&config);
        self.analysis_to_dict(py, &analysis)
    }

    /// Get only the issues/warnings from the analysis.
    ///
    /// Returns a list of issue dictionaries, each containing:
    /// - severity: "ERROR", "WARNING", or "INFO"
    /// - category: Issue category
    /// - message: Human-readable message
    /// - details: Optional additional details
    #[pyo3(text_signature = "($self)")]
    fn get_issues(&self, py: Python) -> PyResult<Py<PyAny>> {
        let problem = self.get_problem()?;
        let analysis = problem.analyze();
        Ok(issues_to_list(py, &analysis.issues)?.into())
    }

    fn __repr__(&self) -> String {
        format!("LpParser(lp_file='{}')", self.lp_file)
    }

    fn __str__(&self) -> String {
        let state = if self.parsed_content.is_some() { "parsed" } else { "not parsed" };
        format!("LpParser for '{}' ({state})", self.lp_file)
    }
}

impl LpParser {
    fn get_problem(&self) -> PyResult<LpProblem> {
        self.parsed_content.as_ref().map_or_else(
            || Err(LpNotParsedError::new_err("Must call parse() first")),
            |content| LpProblem::parse(content).map_err(|err| LpParseError::new_err(format!("Unable to parse LpProblem: {err}"))),
        )
    }

    #[allow(clippy::unused_self)]
    fn analysis_to_dict(&self, py: Python, analysis: &lp_parser_rs::analysis::ProblemAnalysis) -> PyResult<Py<PyAny>> {
        // The struct field names match the public dict schema, so serialise the
        // whole analysis in one step.
        let dict =
            pythonize::pythonize(py, analysis).map_err(|err| PyRuntimeError::new_err(format!("Unable to serialise analysis: {err}")))?;
        // serde serialises the issue severity/category enums by their variant
        // names; the Python API instead exposes the human-readable Display form,
        // so overwrite the issues list to preserve that contract.
        dict.cast::<PyDict>()?.set_item("issues", issues_to_list(py, &analysis.issues)?)?;
        Ok(dict.into())
    }
}

/// Build the Python representation of analysis issues, using the human-readable
/// Display form of the severity and category enums (not their serde names).
fn issues_to_list<'py>(py: Python<'py>, issues: &[lp_parser_rs::analysis::AnalysisIssue]) -> PyResult<Bound<'py, PyList>> {
    let list = PyList::empty(py);
    for issue in issues {
        let issue_dict = PyDict::new(py);
        issue_dict.set_item("severity", issue.severity.to_string())?;
        issue_dict.set_item("category", issue.category.to_string())?;
        issue_dict.set_item("message", &issue.message)?;
        issue_dict.set_item("details", &issue.details)?;
        list.append(issue_dict)?;
    }
    Ok(list)
}

#[pymodule]
fn parse_lp(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<LpParser>()?;
    m.add("LpParseError", m.py().get_type::<LpParseError>())?;
    m.add("LpNotParsedError", m.py().get_type::<LpNotParsedError>())?;
    m.add("LpObjectNotFoundError", m.py().get_type::<LpObjectNotFoundError>())?;
    m.add("LpInvalidValueError", m.py().get_type::<LpInvalidValueError>())?;

    Ok(())
}
