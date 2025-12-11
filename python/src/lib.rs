// Allow pedantic lints that are unavoidable due to PyO3 macro requirements
#![allow(clippy::unnecessary_wraps, clippy::needless_pass_by_value, clippy::cast_possible_truncation, clippy::cast_possible_wrap)]

use std::borrow::Cow;
use std::path::{Path, PathBuf};

use lp_parser_rs::csv::LpCsvWriter as _;
use lp_parser_rs::model::{Constraint, Sense, VariableType};
use lp_parser_rs::parser::parse_file;
use lp_parser_rs::problem::LpProblem;
use lp_parser_rs::writer::{LpWriterOptions, write_lp_string, write_lp_string_with_options};
use pyo3::exceptions::{PyFileExistsError, PyNotADirectoryError, PyRuntimeError};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList};

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
            return Err(PyFileExistsError::new_err("args"));
        }

        Ok(Self { lp_file, parsed_content: None })
    }

    #[getter]
    fn lp_file(&self) -> PyResult<String> {
        Ok(self.lp_file.clone())
    }

    #[pyo3(text_signature = "($self)")]
    fn parse(&mut self) -> PyResult<()> {
        let input = parse_file(&PathBuf::from(&self.lp_file)).map_err(|_| PyRuntimeError::new_err("Unable to read LpFile."))?;
        self.parsed_content = Some(input.clone());
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
        problem.to_csv(Path::new(base_directory)).map_err(|_| PyRuntimeError::new_err("Unable to write to .csv files"))?;

        Ok(())
    }

    #[getter]
    fn name(&self) -> PyResult<Option<String>> {
        let problem = self.get_problem()?;
        Ok(problem.name.map(|n| {
            // Remove "Problem name: " prefix if present
            let name_str = n.to_string();
            if name_str.starts_with("Problem name: ") {
                name_str.strip_prefix("Problem name: ").unwrap_or(&name_str).to_string()
            } else {
                name_str
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

        for obj in problem.objectives.values() {
            let dict = PyDict::new(py);
            dict.set_item("name", obj.name.as_ref())?;

            let coeffs = PyList::empty(py);
            for coef in &obj.coefficients {
                let coef_dict = PyDict::new(py);
                coef_dict.set_item("name", coef.name)?;
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

        for (name, constraint) in &problem.constraints {
            let dict = PyDict::new(py);
            dict.set_item("name", name.as_ref())?;

            match constraint {
                Constraint::Standard { coefficients, operator, rhs, .. } => {
                    dict.set_item("type", "standard")?;

                    let coeffs = PyList::empty(py);
                    for coef in coefficients {
                        let coef_dict = PyDict::new(py);
                        coef_dict.set_item("name", coef.name)?;
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
                        weight_dict.set_item("name", weight.name)?;
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

        for (name, var) in &problem.variables {
            let var_dict = PyDict::new(py);
            var_dict.set_item("name", var.name)?;
            var_dict.set_item("var_type", format!("{:?}", var.var_type))?;
            dict.set_item(name, var_dict)?;
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
    fn compare(&self, other: &LpParser, py: Python) -> PyResult<Py<PyAny>> {
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
        let vars1 = &p1.variables;
        let vars2 = &p2.variables;

        let added_vars = PyList::empty(py);
        let removed_vars = PyList::empty(py);
        let modified_vars = PyList::empty(py);

        for (name, var1) in vars1 {
            if !vars2.contains_key(name) {
                removed_vars.append(name)?;
            } else if let Some(var2) = vars2.get(name) {
                if var1 != var2 {
                    modified_vars.append(name)?;
                }
            }
        }

        for name in vars2.keys() {
            if !vars1.contains_key(name) {
                added_vars.append(name)?;
            }
        }

        dict.set_item("added_variables", added_vars)?;
        dict.set_item("removed_variables", removed_vars)?;
        dict.set_item("modified_variables", modified_vars)?;

        // Find added/removed constraints
        let constraints1 = &p1.constraints;
        let constraints2 = &p2.constraints;

        let added_constraints = PyList::empty(py);
        let removed_constraints = PyList::empty(py);

        for name in constraints1.keys() {
            if !constraints2.contains_key(name) {
                removed_constraints.append(name.as_ref())?;
            }
        }

        for name in constraints2.keys() {
            if !constraints1.contains_key(name) {
                added_constraints.append(name.as_ref())?;
            }
        }

        dict.set_item("added_constraints", added_constraints)?;
        dict.set_item("removed_constraints", removed_constraints)?;

        Ok(dict.into())
    }

    #[allow(clippy::wrong_self_convention)]
    /// Write the current problem to LP format string
    #[pyo3(text_signature = "($self)")]
    fn to_lp_string(&mut self) -> PyResult<String> {
        let problem = self.get_mutable_problem()?;
        write_lp_string(&problem).map_err(|err| PyRuntimeError::new_err(format!("Failed to write LP string: {err}")))
    }

    #[allow(clippy::wrong_self_convention)]
    /// Write the current problem to LP format string with custom options
    #[pyo3(signature = (*, include_problem_name=true, max_line_length=80, decimal_precision=6, include_section_spacing=true))]
    fn to_lp_string_with_options(
        &mut self,
        include_problem_name: bool,
        max_line_length: usize,
        decimal_precision: usize,
        include_section_spacing: bool,
    ) -> PyResult<String> {
        let problem = self.get_mutable_problem()?;
        let options = LpWriterOptions { include_problem_name, max_line_length, decimal_precision, include_section_spacing };
        write_lp_string_with_options(&problem, &options).map_err(|err| PyRuntimeError::new_err(format!("Failed to write LP string: {err}")))
    }

    /// Save the current problem to an LP file
    #[pyo3(text_signature = "($self, filepath)")]
    fn save_to_file(&mut self, filepath: String) -> PyResult<()> {
        let lp_content = self.to_lp_string()?;
        std::fs::write(&filepath, lp_content).map_err(|err| PyRuntimeError::new_err(format!("Failed to write file: {err}")))
    }

    /// Update coefficient in an objective
    #[pyo3(text_signature = "($self, objective_name, variable_name, coefficient)")]
    fn update_objective_coefficient(&mut self, objective_name: String, variable_name: String, coefficient: f64) -> PyResult<()> {
        let mut problem = self.get_mutable_problem()?;
        problem
            .update_objective_coefficient(&objective_name, &variable_name, coefficient)
            .map_err(|err| PyRuntimeError::new_err(format!("Failed to update objective coefficient: {err}")))?;

        // Update the cached content
        let updated_content =
            write_lp_string(&problem).map_err(|err| PyRuntimeError::new_err(format!("Failed to serialize updated problem: {err}")))?;
        self.parsed_content = Some(updated_content);
        Ok(())
    }

    /// Rename an objective
    #[pyo3(text_signature = "($self, old_name, new_name)")]
    fn rename_objective(&mut self, old_name: String, new_name: String) -> PyResult<()> {
        let mut problem = self.get_mutable_problem()?;
        problem
            .rename_objective(&old_name, &new_name)
            .map_err(|err| PyRuntimeError::new_err(format!("Failed to rename objective: {err}")))?;

        let updated_content =
            write_lp_string(&problem).map_err(|err| PyRuntimeError::new_err(format!("Failed to serialize updated problem: {err}")))?;
        self.parsed_content = Some(updated_content);
        Ok(())
    }

    /// Remove an objective
    #[pyo3(text_signature = "($self, objective_name)")]
    fn remove_objective(&mut self, objective_name: String) -> PyResult<()> {
        let mut problem = self.get_mutable_problem()?;
        problem.remove_objective(&objective_name).map_err(|err| PyRuntimeError::new_err(format!("Failed to remove objective: {err}")))?;

        let updated_content =
            write_lp_string(&problem).map_err(|err| PyRuntimeError::new_err(format!("Failed to serialize updated problem: {err}")))?;
        self.parsed_content = Some(updated_content);
        Ok(())
    }

    /// Update coefficient in a constraint
    #[pyo3(text_signature = "($self, constraint_name, variable_name, coefficient)")]
    fn update_constraint_coefficient(&mut self, constraint_name: String, variable_name: String, coefficient: f64) -> PyResult<()> {
        let mut problem = self.get_mutable_problem()?;
        problem
            .update_constraint_coefficient(&constraint_name, &variable_name, coefficient)
            .map_err(|err| PyRuntimeError::new_err(format!("Failed to update constraint coefficient: {err}")))?;

        let updated_content =
            write_lp_string(&problem).map_err(|err| PyRuntimeError::new_err(format!("Failed to serialize updated problem: {err}")))?;
        self.parsed_content = Some(updated_content);
        Ok(())
    }

    /// Update the right-hand side value of a constraint
    #[pyo3(text_signature = "($self, constraint_name, new_rhs)")]
    fn update_constraint_rhs(&mut self, constraint_name: String, new_rhs: f64) -> PyResult<()> {
        let mut problem = self.get_mutable_problem()?;
        problem
            .update_constraint_rhs(&constraint_name, new_rhs)
            .map_err(|err| PyRuntimeError::new_err(format!("Failed to update constraint RHS: {err}")))?;

        let updated_content =
            write_lp_string(&problem).map_err(|err| PyRuntimeError::new_err(format!("Failed to serialize updated problem: {err}")))?;
        self.parsed_content = Some(updated_content);
        Ok(())
    }

    /// Rename a constraint
    #[pyo3(text_signature = "($self, old_name, new_name)")]
    fn rename_constraint(&mut self, old_name: String, new_name: String) -> PyResult<()> {
        let mut problem = self.get_mutable_problem()?;
        problem
            .rename_constraint(&old_name, &new_name)
            .map_err(|err| PyRuntimeError::new_err(format!("Failed to rename constraint: {err}")))?;

        let updated_content =
            write_lp_string(&problem).map_err(|err| PyRuntimeError::new_err(format!("Failed to serialize updated problem: {err}")))?;
        self.parsed_content = Some(updated_content);
        Ok(())
    }

    /// Remove a constraint
    #[pyo3(text_signature = "($self, constraint_name)")]
    fn remove_constraint(&mut self, constraint_name: String) -> PyResult<()> {
        let mut problem = self.get_mutable_problem()?;
        problem
            .remove_constraint(&constraint_name)
            .map_err(|err| PyRuntimeError::new_err(format!("Failed to remove constraint: {err}")))?;

        let updated_content =
            write_lp_string(&problem).map_err(|err| PyRuntimeError::new_err(format!("Failed to serialize updated problem: {err}")))?;
        self.parsed_content = Some(updated_content);
        Ok(())
    }

    /// Rename a variable across all objectives and constraints
    #[pyo3(text_signature = "($self, old_name, new_name)")]
    fn rename_variable(&mut self, old_name: String, new_name: String) -> PyResult<()> {
        let mut problem = self.get_mutable_problem()?;
        problem
            .rename_variable(&old_name, &new_name)
            .map_err(|err| PyRuntimeError::new_err(format!("Failed to rename variable: {err}")))?;

        let updated_content =
            write_lp_string(&problem).map_err(|err| PyRuntimeError::new_err(format!("Failed to serialize updated problem: {err}")))?;
        self.parsed_content = Some(updated_content);
        Ok(())
    }

    /// Update variable type (e.g., Binary, Integer, etc.)
    #[pyo3(text_signature = "($self, variable_name, var_type)")]
    fn update_variable_type(&mut self, variable_name: String, var_type: String) -> PyResult<()> {
        let mut problem = self.get_mutable_problem()?;

        // Parse the variable type string
        let variable_type = match var_type.to_lowercase().as_str() {
            "binary" => VariableType::Binary,
            "integer" => VariableType::Integer,
            "general" => VariableType::General,
            "free" => VariableType::Free,
            "semicontinuous" | "semi_continuous" => VariableType::SemiContinuous,
            _ => {
                return Err(PyRuntimeError::new_err(format!(
                    "Unknown variable type: {var_type}. Supported types: binary, integer, general, free, semicontinuous",
                )));
            }
        };

        problem
            .update_variable_type(&variable_name, variable_type)
            .map_err(|err| PyRuntimeError::new_err(format!("Failed to update variable type: {err}")))?;

        let updated_content =
            write_lp_string(&problem).map_err(|err| PyRuntimeError::new_err(format!("Failed to serialize updated problem: {err}")))?;
        self.parsed_content = Some(updated_content);
        Ok(())
    }

    /// Remove a variable from all objectives and constraints
    #[pyo3(text_signature = "($self, variable_name)")]
    fn remove_variable(&mut self, variable_name: String) -> PyResult<()> {
        let mut problem = self.get_mutable_problem()?;
        problem.remove_variable(&variable_name).map_err(|err| PyRuntimeError::new_err(format!("Failed to remove variable: {err}")))?;

        let updated_content =
            write_lp_string(&problem).map_err(|err| PyRuntimeError::new_err(format!("Failed to serialize updated problem: {err}")))?;
        self.parsed_content = Some(updated_content);
        Ok(())
    }

    /// Set problem name
    #[pyo3(text_signature = "($self, name)")]
    fn set_problem_name(&mut self, name: String) -> PyResult<()> {
        let mut problem = self.get_mutable_problem()?;
        problem.name = Some(Cow::Owned(name));

        let updated_content =
            write_lp_string(&problem).map_err(|err| PyRuntimeError::new_err(format!("Failed to serialize updated problem: {err}")))?;
        self.parsed_content = Some(updated_content);
        Ok(())
    }

    /// Set problem sense (maximize or minimize)
    #[pyo3(text_signature = "($self, sense)")]
    fn set_sense(&mut self, sense: String) -> PyResult<()> {
        let mut problem = self.get_mutable_problem()?;

        problem.sense = match sense.to_lowercase().as_str() {
            "maximize" | "max" => Sense::Maximize,
            "minimize" | "min" => Sense::Minimize,
            _ => return Err(PyRuntimeError::new_err(format!("Invalid sense: {sense}. Use 'maximize' or 'minimize'"))),
        };

        let updated_content =
            write_lp_string(&problem).map_err(|err| PyRuntimeError::new_err(format!("Failed to serialize updated problem: {err}")))?;
        self.parsed_content = Some(updated_content);
        Ok(())
    }
}

impl LpParser {
    fn get_problem(&self) -> PyResult<LpProblem<'_>> {
        if let Some(ref content) = self.parsed_content {
            LpProblem::parse(content).map_err(|_| PyRuntimeError::new_err("Unable to parse LpProblem"))
        } else {
            Err(PyRuntimeError::new_err("Must call parse() first"))
        }
    }

    fn get_mutable_problem(&mut self) -> PyResult<LpProblem<'_>> {
        if let Some(ref content) = self.parsed_content {
            LpProblem::parse(content).map_err(|_| PyRuntimeError::new_err("Unable to parse LpProblem"))
        } else {
            Err(PyRuntimeError::new_err("Must call parse() first"))
        }
    }
}

#[pymodule]
fn parse_lp(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<LpParser>()?;

    Ok(())
}
