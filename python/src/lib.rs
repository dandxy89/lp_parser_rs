use std::path::{Path, PathBuf};

use lp_parser_rs::csv::LpCsvWriter as _;
use lp_parser_rs::model::{Constraint, Sense};
use lp_parser_rs::parser::parse_file;
use lp_parser_rs::problem::LpProblem;
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
        self.parsed_content = Some(input.to_owned());
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
        Ok(problem.name.map(|n| n.to_string()))
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
    fn objectives(&self, py: Python) -> PyResult<PyObject> {
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
    fn constraints(&self, py: Python) -> PyResult<PyObject> {
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
    fn variables(&self, py: Python) -> PyResult<PyObject> {
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
    fn compare(&self, other: &LpParser, py: Python) -> PyResult<PyObject> {
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
}

impl LpParser {
    fn get_problem(&self) -> PyResult<LpProblem<'_>> {
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
