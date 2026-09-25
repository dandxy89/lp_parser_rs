// Allow pedantic lints that are unavoidable due to PyO3 macro requirements
#![allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap, clippy::needless_pass_by_value, clippy::unnecessary_wraps)]

use std::path::{Path, PathBuf};

use lp_parser_rs::analysis::AnalysisConfig;
use lp_parser_rs::diff::DiffOptions;
use lp_parser_rs::model::{Constraint, QuadraticTerm, Sense, VariableType};
use lp_parser_rs::mps::writer::{MpsWriterOptions, write_mps_string_with_options};
use lp_parser_rs::problem::LpProblem;
use lp_parser_rs::writer::{LpWriterOptions, write_lp_string_with_options};
use lp_parser_rs::{ConstraintClass, EntityKind, LpParseError as CoreError, VariableKind};
use pyo3::create_exception;
use pyo3::exceptions::{PyNotADirectoryError, PyOSError, PyRuntimeError};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList};

create_exception!(parse_lp, LpParseError, PyRuntimeError, "Raised when an LP file or problem cannot be parsed.");
create_exception!(
    parse_lp,
    LpObjectNotFoundError,
    PyRuntimeError,
    "Raised when a named variable, constraint or objective cannot be found."
);
create_exception!(parse_lp, LpInvalidValueError, PyRuntimeError, "Raised when an input value is invalid.");

#[pyclass(module = "parse_lp")]
pub struct LpParser {
    lp_file: String,
    /// The file the problem was read from; `None` when built from a string.
    source_path: Option<PathBuf>,
    /// Source format, normalised to `"lp"` or `"mps"`, so `parse()` re-reads
    /// the file with the same parser.
    format: &'static str,
    problem: LpProblem,
}

#[pymethods]
impl LpParser {
    /// Construct a parser from a file, parsing it immediately.
    ///
    /// The format is inferred from the extension (`.mps` -> MPS, everything else
    /// -> LP); pass `format` to [`from_file`] to override it.
    #[new]
    #[pyo3(signature = (lp_file))]
    fn new(py: Python, lp_file: PathBuf) -> PyResult<Self> {
        Self::from_file(py, lp_file, None)
    }

    /// Construct a parser from an in-memory string, parsing it immediately.
    ///
    /// `format` is `"lp"` (default) or `"mps"`.
    #[staticmethod]
    #[pyo3(signature = (text, format="lp"))]
    fn from_string(py: Python, text: String, format: &str) -> PyResult<Self> {
        let format = normalise_format(format)?;
        let problem = py.detach(|| parse_source(&text, format))?;
        Ok(Self { lp_file: "<string>".to_string(), source_path: None, format, problem })
    }

    /// Construct a parser from a file, parsing it immediately.
    ///
    /// The format is taken from `format` when given, otherwise inferred from the
    /// extension (`.mps` -> MPS, everything else -> LP).
    #[staticmethod]
    #[pyo3(signature = (path, format=None))]
    fn from_file(py: Python, path: PathBuf, format: Option<&str>) -> PyResult<Self> {
        let inferred = match format {
            Some(fmt) => normalise_format(fmt)?,
            None if path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("mps")) => "mps",
            None => "lp",
        };
        let file_path = path;
        let problem = py.detach(|| {
            let input = read_source(&file_path)?;
            parse_source(&input, inferred)
        })?;
        Ok(Self { lp_file: file_path.to_string_lossy().into_owned(), source_path: Some(file_path), format: inferred, problem })
    }

    #[getter]
    fn lp_file(&self) -> String {
        self.lp_file.clone()
    }

    /// Re-read and re-parse the source file, in the format it was first parsed
    /// as. Construction already parses, so this is only needed to pick up
    /// changes made to the file since.
    fn parse(&mut self, py: Python) -> PyResult<()> {
        let Some(path) = self.source_path.as_deref() else {
            return Err(LpInvalidValueError::new_err(
                "parse() re-reads the source file, but this parser was built from a string and has none",
            ));
        };
        let format = self.format;
        // Release the GIL while reading and parsing so other Python threads
        // are not blocked by the heavy pure-Rust work.
        self.problem = py.detach(|| {
            let input = read_source(path)?;
            parse_source(&input, format)
        })?;
        Ok(())
    }

    fn to_csv(&self, py: Python, base_directory: PathBuf) -> PyResult<()> {
        if !base_directory.is_dir() {
            return Err(PyNotADirectoryError::new_err(format!("Path {} is not a directory.", base_directory.display())));
        }

        let problem = &self.problem;
        py.detach(|| problem.to_csv(&base_directory).map_err(|err| csv_err(&base_directory, err)))
    }

    #[getter]
    fn name(&self) -> PyResult<Option<String>> {
        // extract_problem_name already stores the bare name, without the
        // "Problem name: " comment prefix.
        let problem = &self.problem;
        Ok(problem.name.clone())
    }

    #[getter]
    fn sense(&self) -> PyResult<String> {
        let problem = &self.problem;
        Ok(match problem.sense {
            Sense::Maximize => "maximize".to_string(),
            Sense::Minimize => "minimize".to_string(),
        })
    }

    #[getter]
    fn objectives(&self, py: Python) -> PyResult<Py<PyAny>> {
        let problem = &self.problem;
        let list = PyList::empty(py);

        for (name_id, obj) in &problem.objectives {
            let dict = PyDict::new(py);
            dict.set_item("name", problem.resolve(*name_id))?;
            dict.set_item("coefficients", coefficients_to_list(py, problem, &obj.coefficients)?)?;
            dict.set_item("quadratic", quadratic_to_list(py, problem, &obj.quadratic)?)?;
            // Gurobi multi-objective attributes; each is None when unset.
            let attributes = PyDict::new(py);
            attributes.set_item("priority", obj.attributes.priority)?;
            attributes.set_item("weight", obj.attributes.weight)?;
            attributes.set_item("abs_tol", obj.attributes.abs_tol)?;
            attributes.set_item("rel_tol", obj.attributes.rel_tol)?;
            dict.set_item("attributes", attributes)?;
            list.append(dict)?;
        }

        Ok(list.into())
    }

    #[getter]
    fn constraints(&self, py: Python) -> PyResult<Py<PyAny>> {
        let problem = &self.problem;
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
                    dict.set_item("class", constraint_class_name(problem.constraint_class(*name_id)))?;
                }
                Constraint::General { resultant, function, .. } => {
                    dict.set_item("type", "general")?;
                    dict.set_item("resultant", problem.resolve(*resultant))?;
                    dict.set_item("function", function.keyword())?;
                    let arguments: Vec<&str> = function.variables().iter().map(|v| problem.resolve(*v)).collect();
                    dict.set_item("arguments", arguments)?;
                    dict.set_item("constant", function.constant())?;
                }
                Constraint::Quadratic { coefficients, quadratic, operator, rhs, .. } => {
                    dict.set_item("type", "quadratic")?;
                    dict.set_item("coefficients", coefficients_to_list(py, problem, coefficients)?)?;
                    dict.set_item("quadratic", quadratic_to_list(py, problem, quadratic)?)?;
                    dict.set_item("operator", format!("{operator:?}"))?;
                    dict.set_item("rhs", rhs)?;
                    dict.set_item("class", constraint_class_name(problem.constraint_class(*name_id)))?;
                }
                Constraint::Indicator { variable, active_value, coefficients, operator, rhs, .. } => {
                    dict.set_item("type", "indicator")?;
                    dict.set_item("indicator_variable", problem.resolve(*variable))?;
                    dict.set_item("indicator_value", u8::from(*active_value))?;
                    dict.set_item("coefficients", coefficients_to_list(py, problem, coefficients)?)?;
                    dict.set_item("operator", format!("{operator:?}"))?;
                    dict.set_item("rhs", rhs)?;
                    dict.set_item("class", constraint_class_name(problem.constraint_class(*name_id)))?;
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
        let problem = &self.problem;
        let dict = PyDict::new(py);

        for (name_id, var) in &problem.variables {
            let resolved_name = problem.resolve(*name_id);
            let var_dict = PyDict::new(py);
            var_dict.set_item("name", resolved_name)?;
            // Structured kind + bounds rather than a Debug string: `lower`/`upper`
            // are `None` when undeclared on that side (the format default
            // applies), and -inf/+inf when explicitly free.
            var_dict.set_item("kind", var.kind.to_string())?;
            var_dict.set_item("lower", var.bounds.lower)?;
            var_dict.set_item("upper", var.bounds.upper)?;
            dict.set_item(resolved_name, var_dict)?;
        }

        Ok(dict.into())
    }

    /// Write the current problem to LP format string, with optional custom formatting
    #[pyo3(signature = (*, include_problem_name=true, max_line_length=80, decimal_precision=None, include_section_spacing=true))]
    fn to_lp_string(
        &self,
        py: Python,
        include_problem_name: bool,
        max_line_length: usize,
        decimal_precision: Option<usize>,
        include_section_spacing: bool,
    ) -> PyResult<String> {
        if max_line_length == 0 {
            return Err(LpInvalidValueError::new_err("max_line_length must be positive, got 0"));
        }
        let problem = &self.problem;
        let options = LpWriterOptions { include_problem_name, max_line_length, decimal_precision, include_section_spacing };
        // Release the GIL: writing is pure Rust and can take a while on large
        // problems.
        py.detach(|| write_lp_string_with_options(problem, &options)).map_err(|err| to_py_err("Unable to write LP", err))
    }

    /// Save the current problem to an LP file
    fn save_to_file(&self, py: Python, filepath: PathBuf) -> PyResult<()> {
        let problem = &self.problem;
        py.detach(|| {
            let lp_content =
                write_lp_string_with_options(problem, &LpWriterOptions::default()).map_err(|err| to_py_err("Unable to write LP", err))?;
            std::fs::write(&filepath, lp_content).map_err(|err| io_err(&filepath, &err))
        })
    }

    /// Write the current problem to an MPS format string.
    #[pyo3(signature = (*, decimal_precision=None, allow_multiple_objectives=false))]
    fn to_mps_string(&self, py: Python, decimal_precision: Option<usize>, allow_multiple_objectives: bool) -> PyResult<String> {
        let problem = &self.problem;
        let options = MpsWriterOptions { decimal_precision, allow_multiple_objectives };
        py.detach(|| write_mps_string_with_options(problem, &options)).map_err(|err| to_py_err("Unable to write MPS", err))
    }

    /// Save the current problem to an MPS file.
    #[pyo3(signature = (filepath, *, decimal_precision=None, allow_multiple_objectives=false))]
    fn save_to_mps(
        &self,
        py: Python,
        filepath: PathBuf,
        decimal_precision: Option<usize>,
        allow_multiple_objectives: bool,
    ) -> PyResult<()> {
        let problem = &self.problem;
        let options = MpsWriterOptions { decimal_precision, allow_multiple_objectives };
        py.detach(|| {
            let content = write_mps_string_with_options(problem, &options).map_err(|err| to_py_err("Unable to write MPS", err))?;
            std::fs::write(&filepath, content).map_err(|err| io_err(&filepath, &err))
        })
    }

    /// Compare this problem against another parser's problem.
    ///
    /// Returns a dict with `sense_changed`, `vars_added`, `vars_removed`, `vars_type_changed`,
    /// `cons_added`, `cons_removed`, `cons_modified`, `objs_added`,
    /// `objs_removed`, `objs_modified`, and `is_empty`.
    fn diff(&self, py: Python, other: &Self) -> PyResult<Py<PyAny>> {
        let problem = &self.problem;
        let other_problem = &other.problem;
        let result = py.detach(|| problem.diff(other_problem, &DiffOptions::default()));
        let is_empty = result.is_empty();

        let dict = PyDict::new(py);
        dict.set_item("sense_changed", result.sense_changed)?;
        dict.set_item("vars_added", result.vars_added)?;
        dict.set_item("vars_removed", result.vars_removed)?;
        dict.set_item("vars_type_changed", result.vars_type_changed)?;
        dict.set_item("cons_added", result.cons_added)?;
        dict.set_item("cons_removed", result.cons_removed)?;
        dict.set_item("cons_modified", result.cons_modified)?;
        dict.set_item("objs_added", result.objs_added)?;
        dict.set_item("objs_removed", result.objs_removed)?;
        dict.set_item("objs_modified", result.objs_modified)?;
        dict.set_item("is_empty", is_empty)?;
        Ok(dict.into())
    }

    /// Update coefficient in an objective
    fn update_objective_coefficient(&mut self, objective_name: String, variable_name: String, coefficient: f64) -> PyResult<()> {
        let problem = &mut self.problem;
        problem
            .update_objective_coefficient(&objective_name, &variable_name, coefficient)
            .map_err(|err| to_py_err("Failed to update objective coefficient", err))?;
        Ok(())
    }

    /// Rename an objective
    fn rename_objective(&mut self, old_name: String, new_name: String) -> PyResult<()> {
        let problem = &mut self.problem;
        problem.rename_objective(&old_name, &new_name).map_err(|err| to_py_err("Failed to rename objective", err))?;

        Ok(())
    }

    /// Remove an objective
    fn remove_objective(&mut self, objective_name: String) -> PyResult<()> {
        require_name("objective_name", &objective_name)?;
        let problem = &mut self.problem;
        problem.remove_objective(&objective_name).map_err(|err| to_py_err("Failed to remove objective", err))?;

        Ok(())
    }

    /// Update coefficient in a constraint
    fn update_constraint_coefficient(&mut self, constraint_name: String, variable_name: String, coefficient: f64) -> PyResult<()> {
        let problem = &mut self.problem;
        problem
            .update_constraint_coefficient(&constraint_name, &variable_name, coefficient)
            .map_err(|err| to_py_err("Failed to update constraint coefficient", err))?;

        Ok(())
    }

    /// Update the right-hand side value of a constraint
    fn update_constraint_rhs(&mut self, constraint_name: String, new_rhs: f64) -> PyResult<()> {
        let problem = &mut self.problem;
        problem.update_constraint_rhs(&constraint_name, new_rhs).map_err(|err| to_py_err("Failed to update constraint RHS", err))?;

        Ok(())
    }

    /// Rename a constraint
    fn rename_constraint(&mut self, old_name: String, new_name: String) -> PyResult<()> {
        let problem = &mut self.problem;
        problem.rename_constraint(&old_name, &new_name).map_err(|err| to_py_err("Failed to rename constraint", err))?;

        Ok(())
    }

    /// Remove a constraint
    fn remove_constraint(&mut self, constraint_name: String) -> PyResult<()> {
        require_name("constraint_name", &constraint_name)?;
        let problem = &mut self.problem;
        problem.remove_constraint(&constraint_name).map_err(|err| to_py_err("Failed to remove constraint", err))?;

        Ok(())
    }

    /// Rename a variable across all objectives and constraints
    fn rename_variable(&mut self, old_name: String, new_name: String) -> PyResult<()> {
        let problem = &mut self.problem;
        problem.rename_variable(&old_name, &new_name).map_err(|err| to_py_err("Failed to rename variable", err))?;

        Ok(())
    }

    /// Update variable type (e.g., Binary, Integer, etc.)
    ///
    /// `continuous` changes only the kind and keeps any declared bounds. The
    /// discrete kinds (`binary`, `integer`, `general`, `semicontinuous`,
    /// `semiinteger`) set
    /// the kind and clear declared bounds, so the format default applies.
    /// `free` is a bound, not a kind: it makes the variable continuous with
    /// bounds `(-inf, +inf)`.
    fn update_variable_type(&mut self, variable_name: String, var_type: String) -> PyResult<()> {
        require_name("variable_name", &variable_name)?;
        let problem = &mut self.problem;

        // Parse the variable type string
        let variable_type = match var_type.to_lowercase().as_str() {
            "continuous" => {
                let variable = problem
                    .name_id(&variable_name)
                    .and_then(|id| problem.variables.get_mut(&id))
                    .ok_or_else(|| CoreError::not_found(EntityKind::Variable, variable_name.as_str()))
                    .map_err(|err| to_py_err("Failed to update variable type", err))?;
                variable.set_kind(VariableKind::Continuous);
                return Ok(());
            }
            "binary" => VariableType::Binary,
            "integer" => VariableType::Integer,
            "general" => VariableType::General,
            "free" => VariableType::Free,
            "semicontinuous" => VariableType::SemiContinuous,
            "semiinteger" => VariableType::SemiInteger,
            _ => {
                return Err(LpInvalidValueError::new_err(format!(
                    "Unknown variable type: {var_type}. Supported types: continuous, binary, integer, general, free, semicontinuous, \
                     semiinteger",
                )));
            }
        };

        problem.update_variable_type(&variable_name, variable_type).map_err(|err| to_py_err("Failed to update variable type", err))?;

        Ok(())
    }

    /// Remove a variable from all objectives and constraints
    fn remove_variable(&mut self, variable_name: String) -> PyResult<()> {
        require_name("variable_name", &variable_name)?;
        let problem = &mut self.problem;
        problem.remove_variable(&variable_name).map_err(|err| to_py_err("Failed to remove variable", err))?;

        Ok(())
    }

    /// Set problem name
    fn set_problem_name(&mut self, name: String) -> PyResult<()> {
        let problem = &mut self.problem;
        problem.name = Some(name);

        Ok(())
    }

    /// Set problem sense (maximize or minimize)
    fn set_sense(&mut self, sense: String) -> PyResult<()> {
        let problem = &mut self.problem;

        problem.sense = match sense.to_lowercase().as_str() {
            "maximize" | "max" => Sense::Maximize,
            "minimize" | "min" => Sense::Minimize,
            _ => return Err(LpInvalidValueError::new_err(format!("Invalid sense: {sense}. Use 'maximize' ('max') or 'minimize' ('min')"))),
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
    ///     `large_rhs_threshold`: Threshold for large right-hand side warnings (default: 1e9)
    ///
    /// Every threshold must be finite and positive.
    #[pyo3(signature = (*, large_coeff_threshold=1e9, small_coeff_threshold=1e-9, ratio_threshold=1e6, large_rhs_threshold=1e9))]
    fn analyze(
        &self,
        py: Python,
        large_coeff_threshold: f64,
        small_coeff_threshold: f64,
        ratio_threshold: f64,
        large_rhs_threshold: f64,
    ) -> PyResult<Py<PyAny>> {
        for (name, value) in [
            ("large_coeff_threshold", large_coeff_threshold),
            ("small_coeff_threshold", small_coeff_threshold),
            ("ratio_threshold", ratio_threshold),
            ("large_rhs_threshold", large_rhs_threshold),
        ] {
            if !(value.is_finite() && value > 0.0) {
                return Err(LpInvalidValueError::new_err(format!("{name} must be finite and positive, got {value}")));
            }
        }
        let problem = &self.problem;
        let config = AnalysisConfig {
            large_coefficient_threshold: large_coeff_threshold,
            small_coefficient_threshold: small_coeff_threshold,
            large_rhs_threshold,
            coefficient_ratio_threshold: ratio_threshold,
        };
        let analysis = py.detach(|| problem.analyze_with_config(&config));
        analysis_to_dict(py, &analysis)
    }

    fn __repr__(&self) -> String {
        format!("LpParser(lp_file='{}', format='{}')", self.lp_file, self.format)
    }

    fn __str__(&self) -> String {
        format!("LpParser for '{}'", self.lp_file)
    }
}

/// Map a core [`CoreError`] onto the Python exception that matches its *kind*,
/// not the call site that produced it.
fn to_py_err(context: &str, err: CoreError) -> PyErr {
    let message = format!("{context}: {err}");
    match err {
        CoreError::NotFound { .. } => LpObjectNotFoundError::new_err(message),
        CoreError::AlreadyExists { .. }
        | CoreError::InvalidOperation { .. }
        | CoreError::InvalidBounds { .. }
        | CoreError::InvalidNumber { .. }
        | CoreError::ValidationError { .. } => LpInvalidValueError::new_err(message),
        CoreError::MissingSection { .. } | CoreError::ParseError { .. } => LpParseError::new_err(message),
        CoreError::IoError { .. } => PyOSError::new_err(message),
    }
}

/// Reject an empty name at the binding boundary with `LpInvalidValueError`,
/// before it reaches core methods that treat a non-empty name as a
/// precondition.
fn require_name(field: &str, value: &str) -> PyResult<()> {
    if value.is_empty() {
        return Err(LpInvalidValueError::new_err(format!("{field} must not be empty")));
    }
    Ok(())
}

/// Read a source file as UTF-8. Invalid UTF-8 is a problem with the file's
/// contents rather than an I/O failure, so it raises `LpParseError`; every
/// other failure goes through [`io_err`].
fn read_source(path: &Path) -> PyResult<String> {
    std::fs::read_to_string(path).map_err(|err| {
        if err.kind() == std::io::ErrorKind::InvalidData {
            LpParseError::new_err(format!(
                "Unable to read '{}': the file is not valid UTF-8 ({err}); re-encode it as UTF-8 (ASCII is a subset)",
                path.display()
            ))
        } else {
            io_err(path, &err)
        }
    })
}

/// Raise an I/O failure as `OSError`. Given an OS error code, Python's
/// `OSError(errno, strerror, filename)` picks the matching subclass
/// (`FileNotFoundError`, `PermissionError`, `IsADirectoryError`, ...).
fn io_err(path: &Path, err: &std::io::Error) -> PyErr {
    let filename = path.display().to_string();
    match err.raw_os_error() {
        Some(errno) => {
            // Rust appends " (os error N)"; Python already shows `[Errno N]`.
            let message = err.to_string();
            let strerror = message.split(" (os error").next().unwrap_or(&message).to_string();
            PyOSError::new_err((errno, strerror, filename))
        }
        None => PyOSError::new_err(format!("{filename}: {err}")),
    }
}

/// Map a `to_csv` failure onto a Python exception. The core returns a boxed
/// error that is either a `csv::Error` (from creating or writing a file) or a
/// bare `std::io::Error` (from flushing). I/O failures become `OSError`
/// subclasses via [`io_err`]; anything else is an invalid value.
fn csv_err(base_directory: &Path, err: Box<dyn std::error::Error>) -> PyErr {
    let err = match err.downcast::<std::io::Error>() {
        Ok(io) => return io_err(base_directory, &io),
        Err(err) => err,
    };
    match err.downcast::<csv::Error>() {
        Ok(csv_error) => match csv_error.kind() {
            csv::ErrorKind::Io(io) => io_err(base_directory, io),
            _ => LpInvalidValueError::new_err(format!("Unable to write to .csv files: {csv_error}")),
        },
        Err(err) => LpInvalidValueError::new_err(format!("Unable to write to .csv files: {err}")),
    }
}

/// Normalise a user-supplied format name (`"lp"` or `"mps"`, case-insensitive).
fn normalise_format(format: &str) -> PyResult<&'static str> {
    match format.to_lowercase().as_str() {
        "lp" => Ok("lp"),
        "mps" => Ok("mps"),
        other => Err(LpInvalidValueError::new_err(format!("Unknown format: {other}. Use 'lp' or 'mps'"))),
    }
}

/// Parse LP or MPS source text into an [`LpProblem`], selecting the parser by
/// a format already passed through [`normalise_format`].
fn parse_source(text: &str, format: &'static str) -> PyResult<LpProblem> {
    debug_assert!(format == "lp" || format == "mps", "format must be normalised");
    if format == "mps" {
        LpProblem::parse_mps(text).map_err(|err| LpParseError::new_err(format!("Unable to parse MPS: {err}")))
    } else {
        LpProblem::parse(text).map_err(|err| LpParseError::new_err(format!("Unable to parse LpProblem: {err}")))
    }
}

/// Serialise a [`ProblemAnalysis`](lp_parser_rs::analysis::ProblemAnalysis) to a Python dict.
fn analysis_to_dict(py: Python, analysis: &lp_parser_rs::analysis::ProblemAnalysis) -> PyResult<Py<PyAny>> {
    // The struct field names match the public dict schema, so serialise the
    // whole analysis in one step.
    let dict = pythonize::pythonize(py, analysis).map_err(|err| PyRuntimeError::new_err(format!("Unable to serialise analysis: {err}")))?;
    // serde serialises the issue severity/category enums by their variant
    // names; the Python API instead exposes the human-readable Display form,
    // so overwrite the issues list to preserve that contract.
    dict.cast::<PyDict>()?.set_item("issues", issues_to_list(py, &analysis.issues)?)?;
    Ok(dict.into())
}

/// Build a list of `{name, value}` dicts from coefficients, resolving interned names.
/// Quadratic terms as `[{"var1", "var2", "coefficient"}]`; each coefficient is
/// the term's actual coefficient (an objective's LP `/ 2` already applied).
fn quadratic_to_list<'py>(py: Python<'py>, problem: &LpProblem, terms: &[QuadraticTerm]) -> PyResult<Bound<'py, PyList>> {
    let list = PyList::empty(py);
    for term in terms {
        let dict = PyDict::new(py);
        dict.set_item("var1", problem.resolve(term.var1))?;
        dict.set_item("var2", problem.resolve(term.var2))?;
        dict.set_item("coefficient", term.coefficient)?;
        list.append(dict)?;
    }
    Ok(list)
}

/// The Python spelling of a constraint class (`"normal"`, `"lazy"`, `"user_cut"`).
const fn constraint_class_name(class: ConstraintClass) -> &'static str {
    match class {
        ConstraintClass::Normal => "normal",
        ConstraintClass::Lazy => "lazy",
        ConstraintClass::UserCut => "user_cut",
    }
}

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
    m.add("LpObjectNotFoundError", m.py().get_type::<LpObjectNotFoundError>())?;
    m.add("LpInvalidValueError", m.py().get_type::<LpInvalidValueError>())?;

    Ok(())
}
