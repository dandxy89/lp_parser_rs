// Allow pedantic lints that are unavoidable due to PyO3 macro requirements
#![allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap, clippy::needless_pass_by_value, clippy::unnecessary_wraps)]

use std::path::{Path, PathBuf};

use lp_parser_rs::analysis::AnalysisConfig;
use lp_parser_rs::model::{Constraint, Sense, VariableType};
use lp_parser_rs::parser::parse_file;
use lp_parser_rs::problem::LpProblem;
use lp_parser_rs::writer::{LpWriterOptions, write_lp_string_with_options};
use pyo3::create_exception;
use pyo3::exceptions::{PyFileNotFoundError, PyNotADirectoryError, PyRuntimeError};
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
    problem: Option<LpProblem>,
}

#[pymethods]
impl LpParser {
    #[new]
    #[pyo3(signature = (lp_file))]
    fn new(lp_file: String) -> PyResult<Self> {
        if !Path::new(&lp_file).is_file() {
            return Err(PyFileNotFoundError::new_err(format!("LP file '{lp_file}' does not exist or is not a file")));
        }

        Ok(Self { lp_file, problem: None })
    }

    #[getter]
    fn lp_file(&self) -> String {
        self.lp_file.clone()
    }

    #[pyo3(text_signature = "($self)")]
    fn parse(&mut self, py: Python) -> PyResult<()> {
        let path = PathBuf::from(&self.lp_file);
        // Release the GIL while reading and parsing so other Python threads
        // are not blocked by the heavy pure-Rust work.
        let problem = py.detach(move || {
            let input = parse_file(&path).map_err(|err| LpParseError::new_err(format!("Unable to read LP file: {err}")))?;
            LpProblem::parse(&input).map_err(|err| LpParseError::new_err(format!("Unable to parse LpProblem: {err}")))
        })?;
        self.problem = Some(problem);
        Ok(())
    }

    #[allow(clippy::wrong_self_convention)]
    #[pyo3(text_signature = "($self, base_directory)")]
    fn to_csv(&mut self, py: Python, base_directory: &str) -> PyResult<()> {
        if !Path::new(&base_directory).is_dir() {
            return Err(PyNotADirectoryError::new_err(format!("Path {base_directory} is not a directory.")));
        }

        // Parse if not already parsed
        if self.problem.is_none() {
            self.parse(py)?;
        }

        let problem = self.get_problem()?;
        problem
            .to_csv(Path::new(base_directory))
            .map_err(|err| PyRuntimeError::new_err(format!("Unable to write to .csv files: {err}")))?;

        Ok(())
    }

    #[getter]
    fn name(&self) -> PyResult<Option<String>> {
        // extract_problem_name already stores the bare name, without the
        // "Problem name: " comment prefix.
        let problem = self.get_problem()?;
        Ok(problem.name.clone())
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
            dict.set_item("coefficients", coefficients_to_list(py, problem, &obj.coefficients)?)?;
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
                    dict.set_item("coefficients", coefficients_to_list(py, problem, coefficients)?)?;
                    dict.set_item("operator", format!("{operator:?}"))?;
                    dict.set_item("rhs", rhs)?;
                }
                Constraint::SOS { weights, sos_type, .. } => {
                    dict.set_item("type", "sos")?;
                    dict.set_item("sos_type", format!("{sos_type:?}"))?;
                    dict.set_item("weights", coefficients_to_list(py, problem, weights)?)?;
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

    /// Write the current problem to LP format string, with optional custom formatting
    #[pyo3(signature = (*, include_problem_name=true, max_line_length=80, decimal_precision=6, include_section_spacing=true))]
    fn to_lp_string(
        &self,
        include_problem_name: bool,
        max_line_length: usize,
        decimal_precision: usize,
        include_section_spacing: bool,
    ) -> PyResult<String> {
        let problem = self.get_problem()?;
        let options = LpWriterOptions { include_problem_name, max_line_length, decimal_precision, include_section_spacing };
        Ok(write_lp_string_with_options(problem, &options))
    }

    /// Save the current problem to an LP file
    #[pyo3(text_signature = "($self, filepath)")]
    fn save_to_file(&self, filepath: String) -> PyResult<()> {
        let problem = self.get_problem()?;
        let lp_content = write_lp_string_with_options(problem, &LpWriterOptions::default());
        std::fs::write(&filepath, lp_content).map_err(|err| PyRuntimeError::new_err(format!("Failed to write file: {err}")))
    }

    /// Update coefficient in an objective
    #[pyo3(text_signature = "($self, objective_name, variable_name, coefficient)")]
    fn update_objective_coefficient(&mut self, objective_name: String, variable_name: String, coefficient: f64) -> PyResult<()> {
        let problem = self.get_problem_mut()?;
        problem
            .update_objective_coefficient(&objective_name, &variable_name, coefficient)
            .map_err(|err| LpObjectNotFoundError::new_err(format!("Failed to update objective coefficient: {err}")))?;
        Ok(())
    }

    /// Rename an objective
    #[pyo3(text_signature = "($self, old_name, new_name)")]
    fn rename_objective(&mut self, old_name: String, new_name: String) -> PyResult<()> {
        let problem = self.get_problem_mut()?;
        problem
            .rename_objective(&old_name, &new_name)
            .map_err(|err| LpObjectNotFoundError::new_err(format!("Failed to rename objective: {err}")))?;

        Ok(())
    }

    /// Remove an objective
    #[pyo3(text_signature = "($self, objective_name)")]
    fn remove_objective(&mut self, objective_name: String) -> PyResult<()> {
        let problem = self.get_problem_mut()?;
        problem
            .remove_objective(&objective_name)
            .map_err(|err| LpObjectNotFoundError::new_err(format!("Failed to remove objective: {err}")))?;

        Ok(())
    }

    /// Update coefficient in a constraint
    #[pyo3(text_signature = "($self, constraint_name, variable_name, coefficient)")]
    fn update_constraint_coefficient(&mut self, constraint_name: String, variable_name: String, coefficient: f64) -> PyResult<()> {
        let problem = self.get_problem_mut()?;
        problem
            .update_constraint_coefficient(&constraint_name, &variable_name, coefficient)
            .map_err(|err| LpObjectNotFoundError::new_err(format!("Failed to update constraint coefficient: {err}")))?;

        Ok(())
    }

    /// Update the right-hand side value of a constraint
    #[pyo3(text_signature = "($self, constraint_name, new_rhs)")]
    fn update_constraint_rhs(&mut self, constraint_name: String, new_rhs: f64) -> PyResult<()> {
        let problem = self.get_problem_mut()?;
        problem
            .update_constraint_rhs(&constraint_name, new_rhs)
            .map_err(|err| LpObjectNotFoundError::new_err(format!("Failed to update constraint RHS: {err}")))?;

        Ok(())
    }

    /// Rename a constraint
    #[pyo3(text_signature = "($self, old_name, new_name)")]
    fn rename_constraint(&mut self, old_name: String, new_name: String) -> PyResult<()> {
        let problem = self.get_problem_mut()?;
        problem
            .rename_constraint(&old_name, &new_name)
            .map_err(|err| LpObjectNotFoundError::new_err(format!("Failed to rename constraint: {err}")))?;

        Ok(())
    }

    /// Remove a constraint
    #[pyo3(text_signature = "($self, constraint_name)")]
    fn remove_constraint(&mut self, constraint_name: String) -> PyResult<()> {
        let problem = self.get_problem_mut()?;
        problem
            .remove_constraint(&constraint_name)
            .map_err(|err| LpObjectNotFoundError::new_err(format!("Failed to remove constraint: {err}")))?;

        Ok(())
    }

    /// Rename a variable across all objectives and constraints
    #[pyo3(text_signature = "($self, old_name, new_name)")]
    fn rename_variable(&mut self, old_name: String, new_name: String) -> PyResult<()> {
        let problem = self.get_problem_mut()?;
        problem
            .rename_variable(&old_name, &new_name)
            .map_err(|err| LpObjectNotFoundError::new_err(format!("Failed to rename variable: {err}")))?;

        Ok(())
    }

    /// Update variable type (e.g., Binary, Integer, etc.)
    #[pyo3(text_signature = "($self, variable_name, var_type)")]
    fn update_variable_type(&mut self, variable_name: String, var_type: String) -> PyResult<()> {
        let problem = self.get_problem_mut()?;

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

        Ok(())
    }

    /// Remove a variable from all objectives and constraints
    #[pyo3(text_signature = "($self, variable_name)")]
    fn remove_variable(&mut self, variable_name: String) -> PyResult<()> {
        let problem = self.get_problem_mut()?;
        problem
            .remove_variable(&variable_name)
            .map_err(|err| LpObjectNotFoundError::new_err(format!("Failed to remove variable: {err}")))?;

        Ok(())
    }

    /// Set problem name
    #[pyo3(text_signature = "($self, name)")]
    fn set_problem_name(&mut self, name: String) -> PyResult<()> {
        let problem = self.get_problem_mut()?;
        problem.name = Some(name);

        Ok(())
    }

    /// Set problem sense (maximize or minimize)
    #[pyo3(text_signature = "($self, sense)")]
    fn set_sense(&mut self, sense: String) -> PyResult<()> {
        let problem = self.get_problem_mut()?;

        problem.sense = match sense.to_lowercase().as_str() {
            "maximize" | "max" => Sense::Maximize,
            "minimize" | "min" => Sense::Minimize,
            _ => return Err(LpInvalidValueError::new_err(format!("Invalid sense: {sense}. Use 'maximize' or 'minimize'"))),
        };

        Ok(())
    }

    /// Perform comprehensive analysis on the LP problem.
    ///
    /// Returns a dictionary containing:
    /// - summary: Basic statistics (counts, density, etc.)
    /// - sparsity: Sparsity metrics (variables per constraint)
    /// - variables: Variable analysis (type distribution, invalid bounds, etc.)
    /// - constraints: Constraint analysis (type distribution, empty/singleton)
    /// - coefficients: Coefficient range analysis
    /// - issues: List of detected issues/warnings
    ///
    /// Args:
    ///     `large_coeff_threshold`: Threshold for large coefficient warnings (default: 1e9)
    ///     `small_coeff_threshold`: Threshold for small coefficient warnings (default: 1e-9)
    ///     `ratio_threshold`: Coefficient ratio threshold for scaling warnings (default: 1e6)
    #[pyo3(signature = (*, large_coeff_threshold=1e9, small_coeff_threshold=1e-9, ratio_threshold=1e6))]
    fn analyze(&self, py: Python, large_coeff_threshold: f64, small_coeff_threshold: f64, ratio_threshold: f64) -> PyResult<Py<PyAny>> {
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

    fn __repr__(&self) -> String {
        format!("LpParser(lp_file='{}')", self.lp_file)
    }

    fn __str__(&self) -> String {
        let state = if self.problem.is_some() { "parsed" } else { "not parsed" };
        format!("LpParser for '{}' ({state})", self.lp_file)
    }
}

impl LpParser {
    fn get_problem(&self) -> PyResult<&LpProblem> {
        self.problem.as_ref().ok_or_else(|| LpNotParsedError::new_err("Must call parse() first"))
    }

    fn get_problem_mut(&mut self) -> PyResult<&mut LpProblem> {
        self.problem.as_mut().ok_or_else(|| LpNotParsedError::new_err("Must call parse() first"))
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

/// Build a list of `{name, value}` dicts from coefficients, resolving interned names.
fn coefficients_to_list<'py>(
    py: Python<'py>,
    problem: &LpProblem,
    coefficients: &[lp_parser_rs::model::Coefficient],
) -> PyResult<Bound<'py, PyList>> {
    let list = PyList::empty(py);
    for coef in coefficients {
        let dict = PyDict::new(py);
        dict.set_item("name", problem.resolve(coef.name))?;
        dict.set_item("value", coef.value)?;
        list.append(dict)?;
    }
    Ok(list)
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
