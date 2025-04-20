use std::path::{Path, PathBuf};

use lp_parser_rs::csv::LpCsvWriter as _;
use lp_parser_rs::parser::parse_file;
use lp_parser_rs::problem::LpProblem;
use pyo3::exceptions::{PyFileExistsError, PyNotADirectoryError, PyRuntimeError};
use pyo3::prelude::*;

#[pyclass]
pub struct LpParser {
    #[allow(dead_code)]
    lp_file: String,
}

#[pymethods]
impl LpParser {
    #[new]
    #[pyo3(signature = (lp_file))]
    fn new(lp_file: String) -> PyResult<Self> {
        if !Path::new(&lp_file).is_file() {
            return Err(PyFileExistsError::new_err("args"));
        }

        Ok(Self { lp_file })
    }

    #[getter]
    fn lp_file(&self) -> PyResult<String> {
        Ok(self.lp_file.clone())
    }

    #[pyo3(text_signature = "($self, base_directory)")]
    fn to_csv(&self, base_directory: &str) -> PyResult<()> {
        if !Path::new(&base_directory).is_dir() {
            return Err(PyNotADirectoryError::new_err(format!("Path {base_directory} is not a directory.")));
        }

        let input = parse_file(&PathBuf::from(&self.lp_file)).map_err(|_| PyRuntimeError::new_err("Unable to read LpFile."))?;
        let problem = LpProblem::parse(&input).map_err(|_| PyRuntimeError::new_err("Unable to parse LpProblem"))?;
        problem.to_csv(Path::new(base_directory)).map_err(|_| PyRuntimeError::new_err("Unable to write to .csv files"))?;

        Ok(())
    }
}

#[pymodule]
fn parse_lp(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<LpParser>()?;

    Ok(())
}
