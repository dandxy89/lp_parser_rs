// Allow pedantic lints that are unavoidable due to PyO3 macro requirements
#![allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap, clippy::needless_pass_by_value, clippy::unnecessary_wraps)]

use std::path::{Path, PathBuf};

use lp_parser_rs::analysis::AnalysisConfig;
use lp_parser_rs::diff::DiffOptions;
use lp_parser_rs::model::{Constraint, QuadraticTerm, Sense, Variable, VariableType};
use lp_parser_rs::mps::writer::{MpsWriterOptions, write_mps_string_with_options};
use lp_parser_rs::problem::LpProblem;
use lp_parser_rs::writer::{LpWriterOptions, write_lp_string_with_options};
use lp_parser_rs::{ConstraintClass, EntityKind, LpParseError as CoreError, NameId, VariableKind};
use pyo3::create_exception;
use pyo3::exceptions::{PyNotADirectoryError, PyOSError, PyRuntimeError};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList};

create_exception!(parse_lp, LpParseError, PyRuntimeError, "Raised when LP or MPS input is not valid UTF-8 or cannot be parsed.");
create_exception!(parse_lp, LpObjectNotFoundError, PyRuntimeError, "Raised when a named variable, constraint or objective does not exist.");
create_exception!(
    parse_lp,
    LpInvalidValueError,
    PyRuntimeError,
    "Raised for an invalid argument, or an edit or write the problem cannot support."
);

/// A parsed LP or MPS problem that can be inspected, edited and written back out.
///
/// `LpParser(path)` reads and parses the file straight away, inferring the
/// format from the extension (`.mps` is MPS, anything else is LP). Use
/// `LpParser.from_file` to choose the format explicitly, or
/// `LpParser.from_string` to parse text already in memory.
///
/// A missing or unreadable file raises `OSError` (for example
/// `FileNotFoundError`, or `IsADirectoryError` for a directory). A file that
/// is not valid UTF-8 or does not parse raises `LpParseError`.
///
/// All of the library's own exceptions (`LpParseError`,
/// `LpObjectNotFoundError`, `LpInvalidValueError`) subclass `RuntimeError`.
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
    // PyO3 shows the struct's doc comment as the class docstring, so the
    // constructor is documented there rather than here.
    #[new]
    #[pyo3(signature = (lp_file))]
    fn new(py: Python, lp_file: PathBuf) -> PyResult<Self> {
        Self::from_file(py, lp_file, None)
    }

    /// Parse LP or MPS text held in memory.
    ///
    /// Useful when the model comes from a generator, a database or a test
    /// rather than a file. `format` is `"lp"` (the default) or `"mps"`, in any
    /// case. The resulting parser has `lp_file == "<string>"` and no source
    /// file, so `parse()` cannot be called on it.
    ///
    /// Raises `LpParseError` if the text does not parse and
    /// `LpInvalidValueError` for an unknown `format`.
    #[staticmethod]
    #[pyo3(signature = (text, format="lp"))]
    fn from_string(py: Python, text: String, format: &str) -> PyResult<Self> {
        let format = normalise_format(format)?;
        let problem = py.detach(|| parse_source(&text, format))?;
        Ok(Self { lp_file: "<string>".to_string(), source_path: None, format, problem })
    }

    /// Read and parse a file, optionally forcing the format.
    ///
    /// Same as `LpParser(path)`, but `format` (`"lp"` or `"mps"`, in any case)
    /// overrides the extension check. Use it for MPS files that do not end in
    /// `.mps`, such as `model.mps.txt` or files with no extension.
    ///
    /// Raises `OSError` if the file cannot be read, `LpParseError` if it is
    /// not valid UTF-8 or does not parse, and `LpInvalidValueError` for an
    /// unknown `format`.
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

    /// The source path as given to the constructor, or `"<string>"` for a
    /// parser built with `from_string`.
    #[getter]
    fn lp_file(&self) -> String {
        self.lp_file.clone()
    }

    /// Re-read and re-parse the source file in the format it was first parsed as.
    ///
    /// Construction already parses the file, so call this only to pick up
    /// changes made to it on disk since. Any in-memory edits are discarded.
    ///
    /// Raises `LpInvalidValueError` for a parser built with `from_string`,
    /// which has no file to re-read, and otherwise the same exceptions as the
    /// constructor. On failure the previously parsed problem is kept.
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

    /// Write the problem as three CSV files for inspection in a spreadsheet or
    /// dataframe.
    ///
    /// Creates (or overwrites) `objectives.csv`, `constraints.csv` and
    /// `variables.csv` in `base_directory`, which must already exist.
    /// Constraints have one row per variable; quadratic terms appear as
    /// `x*y`.
    ///
    /// Raises `NotADirectoryError` if `base_directory` is not an existing
    /// directory and `OSError` if a file cannot be written.
    fn to_csv(&self, py: Python, base_directory: PathBuf) -> PyResult<()> {
        if !base_directory.is_dir() {
            return Err(PyNotADirectoryError::new_err(format!("Path {} is not a directory.", base_directory.display())));
        }

        let problem = &self.problem;
        py.detach(|| problem.to_csv(&base_directory).map_err(|err| csv_err(&base_directory, err)))
    }

    /// The problem name, or `None` if the source did not declare one.
    ///
    /// In LP files this is read from a leading comment such as
    /// `\Problem name: diet` or `\* diet *\`; in MPS files from the `NAME`
    /// record.
    #[getter]
    fn name(&self) -> PyResult<Option<String>> {
        // extract_problem_name already stores the bare name, without the
        // "Problem name: " comment prefix.
        let problem = &self.problem;
        Ok(problem.name.clone())
    }

    /// The optimisation sense: `"maximize"` or `"minimize"`.
    #[getter]
    fn sense(&self) -> PyResult<String> {
        let problem = &self.problem;
        Ok(match problem.sense {
            Sense::Maximize => "maximize".to_string(),
            Sense::Minimize => "minimize".to_string(),
        })
    }

    /// Every objective as a list of dicts, in file order.
    ///
    /// Each dict has `name`, `coefficients` (a list of `{name, value}`),
    /// `quadratic` (a list of `{var1, var2, coefficient}`, with the LP
    /// `[ ... ] / 2` already applied) and `attributes` (Gurobi multi-objective
    /// `priority`, `weight`, `abs_tol`, `rel_tol`, each `None` when unset).
    ///
    /// The list is rebuilt on every access and does not track later edits, so
    /// bind it to a variable rather than reading the property in a loop.
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

    /// Every constraint as a list of dicts, in file order.
    ///
    /// The `type` key says which shape a dict has: `"standard"`, `"sos"`,
    /// `"indicator"`, `"quadratic"` or `"general"`. See the type stubs for the
    /// keys of each.
    ///
    /// The list is rebuilt on every access and does not track later edits.
    /// Use `get_constraint` for a single lookup and `num_constraints` for the
    /// count.
    #[getter]
    fn constraints(&self, py: Python) -> PyResult<Py<PyAny>> {
        let problem = &self.problem;
        let list = PyList::empty(py);
        for (name_id, constraint) in &problem.constraints {
            list.append(constraint_to_dict(py, problem, *name_id, constraint)?)?;
        }
        Ok(list.into())
    }

    /// Every variable as a dict keyed by name.
    ///
    /// Each value has `name`, `kind` (such as `"Continuous"` or `"Binary"`),
    /// `lower` and `upper`. A bound is `None` when it was never declared, so
    /// the format default applies (LP: lower 0, upper +inf); a `free`
    /// variable reports `-inf` and `inf`.
    ///
    /// The dict is rebuilt on every access and does not track later edits.
    /// Use `get_variable` for a single lookup and `num_variables` for the
    /// count.
    #[getter]
    fn variables(&self, py: Python) -> PyResult<Py<PyAny>> {
        let problem = &self.problem;
        let dict = PyDict::new(py);
        for (name_id, variable) in &problem.variables {
            dict.set_item(problem.resolve(*name_id), variable_to_dict(py, problem, *name_id, variable)?)?;
        }
        Ok(dict.into())
    }

    /// Return one constraint by name, in the same shape as an entry of
    /// `constraints`, without building the whole list.
    ///
    /// Raises `LpObjectNotFoundError` if there is no such constraint and
    /// `LpInvalidValueError` if `name` is empty.
    fn get_constraint(&self, py: Python, name: String) -> PyResult<Py<PyAny>> {
        require_name("name", &name)?;
        let problem = &self.problem;
        let (name_id, constraint) =
            problem
                .name_id(&name)
                .and_then(|id| problem.constraints.get(&id).map(|constraint| (id, constraint)))
                .ok_or_else(|| to_py_err("Failed to get constraint", CoreError::not_found(EntityKind::Constraint, name.as_str())))?;
        Ok(constraint_to_dict(py, problem, name_id, constraint)?.into())
    }

    /// Return one variable by name, in the same shape as a value of
    /// `variables`, without building the whole dict.
    ///
    /// Raises `LpObjectNotFoundError` if there is no such variable and
    /// `LpInvalidValueError` if `name` is empty.
    fn get_variable(&self, py: Python, name: String) -> PyResult<Py<PyAny>> {
        require_name("name", &name)?;
        let problem = &self.problem;
        let (name_id, variable) = problem
            .name_id(&name)
            .and_then(|id| problem.variables.get(&id).map(|variable| (id, variable)))
            .ok_or_else(|| to_py_err("Failed to get variable", CoreError::not_found(EntityKind::Variable, name.as_str())))?;
        Ok(variable_to_dict(py, problem, name_id, variable)?.into())
    }

    /// Number of objectives. Cheaper than `len(parser.objectives)`.
    #[getter]
    fn num_objectives(&self) -> usize {
        self.problem.objectives.len()
    }

    /// Number of constraints. Cheaper than `len(parser.constraints)`.
    #[getter]
    fn num_constraints(&self) -> usize {
        self.problem.constraints.len()
    }

    /// Number of variables. Cheaper than `len(parser.variables)`.
    #[getter]
    fn num_variables(&self) -> usize {
        self.problem.variables.len()
    }

    /// Serialise the current problem, including any edits, to LP text.
    ///
    /// With the defaults the output parses back to the same problem.
    /// `include_problem_name` writes a `\Problem name:` comment at the top.
    /// `max_line_length` is where long expressions wrap. `decimal_precision`
    /// rounds every number to that many decimal places, which loses
    /// precision; `None` writes the shortest form that reads back exactly.
    /// `include_section_spacing` puts a blank line between sections.
    ///
    /// Raises `LpInvalidValueError` if `max_line_length` is 0 or the problem
    /// holds something LP cannot express, such as a name containing
    /// characters LP does not allow.
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

    /// Write the current problem, including any edits, to an LP file.
    ///
    /// Uses the default `to_lp_string` formatting and overwrites an existing
    /// file. For custom formatting, write the result of `to_lp_string`
    /// yourself.
    ///
    /// Raises `OSError` if the file cannot be written (for example
    /// `FileNotFoundError` when the parent directory is missing) and
    /// `LpInvalidValueError` as `to_lp_string` does.
    fn save_to_file(&self, py: Python, filepath: PathBuf) -> PyResult<()> {
        let problem = &self.problem;
        py.detach(|| {
            let lp_content =
                write_lp_string_with_options(problem, &LpWriterOptions::default()).map_err(|err| to_py_err("Unable to write LP", err))?;
            std::fs::write(&filepath, lp_content).map_err(|err| io_err(&filepath, &err))
        })
    }

    /// Serialise the current problem, including any edits, to MPS text.
    ///
    /// Use this to hand the model to a solver that prefers MPS, or to convert
    /// LP files to MPS. `decimal_precision` behaves as in `to_lp_string`.
    ///
    /// MPS holds a single objective, so a problem with several raises
    /// `LpInvalidValueError` unless `allow_multiple_objectives` is true, in
    /// which case only the first is written (without any Gurobi
    /// multi-objective attributes). Strict inequalities (`<`, `>`) and
    /// general constraints cannot be written either and also raise
    /// `LpInvalidValueError`.
    #[pyo3(signature = (*, decimal_precision=None, allow_multiple_objectives=false))]
    fn to_mps_string(&self, py: Python, decimal_precision: Option<usize>, allow_multiple_objectives: bool) -> PyResult<String> {
        let problem = &self.problem;
        let options = MpsWriterOptions { decimal_precision, allow_multiple_objectives };
        py.detach(|| write_mps_string_with_options(problem, &options)).map_err(|err| to_py_err("Unable to write MPS", err))
    }

    /// Write the current problem to an MPS file, overwriting any existing one.
    ///
    /// Takes the same options, and raises the same errors, as
    /// `to_mps_string`, plus `OSError` if the file cannot be written.
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

    /// Compare this problem (the old one) with `other` (the new one) by name.
    ///
    /// Useful for checking what a model generator or a set of edits actually
    /// changed. Returns a dict:
    ///
    /// - `sense_changed`: `(old, new)` such as `("Minimize", "Maximize")`, or
    ///   `None` if the sense is the same.
    /// - `vars_added`, `cons_added`, `objs_added`: names only in `other`.
    /// - `vars_removed`, `cons_removed`, `objs_removed`: names only in `self`.
    /// - `vars_type_changed`: `(name, old_type, new_type)` tuples.
    /// - `cons_modified`, `objs_modified`: `(name, changes)` tuples, where
    ///   `changes` is a list of human-readable descriptions.
    /// - `is_empty`: true when nothing differs.
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

    /// Set a variable's coefficient in an objective.
    ///
    /// Replaces the coefficient if the variable is already in the objective
    /// and adds the term if not; a variable not yet in the problem is
    /// declared as continuous. A coefficient of 0 removes the term.
    ///
    /// Raises `LpObjectNotFoundError` if the objective does not exist and
    /// `LpInvalidValueError` if `variable_name` is empty or `coefficient` is
    /// not finite.
    fn update_objective_coefficient(&mut self, objective_name: String, variable_name: String, coefficient: f64) -> PyResult<()> {
        let problem = &mut self.problem;
        problem
            .update_objective_coefficient(&objective_name, &variable_name, coefficient)
            .map_err(|err| to_py_err("Failed to update objective coefficient", err))?;
        Ok(())
    }

    /// Rename an objective.
    ///
    /// Raises `LpObjectNotFoundError` if `old_name` does not exist and
    /// `LpInvalidValueError` if `new_name` is already in use or either name
    /// is empty.
    fn rename_objective(&mut self, old_name: String, new_name: String) -> PyResult<()> {
        let problem = &mut self.problem;
        problem.rename_objective(&old_name, &new_name).map_err(|err| to_py_err("Failed to rename objective", err))?;

        Ok(())
    }

    /// Remove an objective. Its variables stay in the problem.
    ///
    /// Raises `LpObjectNotFoundError` if the objective does not exist and
    /// `LpInvalidValueError` if `objective_name` is empty.
    fn remove_objective(&mut self, objective_name: String) -> PyResult<()> {
        require_name("objective_name", &objective_name)?;
        let problem = &mut self.problem;
        problem.remove_objective(&objective_name).map_err(|err| to_py_err("Failed to remove objective", err))?;

        Ok(())
    }

    /// Set a variable's coefficient in a constraint.
    ///
    /// Works on the linear part of standard, indicator and quadratic
    /// constraints. Replaces the coefficient if the variable is already
    /// present and adds the term if not; a variable not yet in the problem is
    /// declared as continuous. A coefficient of 0 removes the term.
    ///
    /// Raises `LpObjectNotFoundError` if the constraint does not exist and
    /// `LpInvalidValueError` for an SOS or general constraint, an empty
    /// `variable_name` or a non-finite `coefficient`.
    fn update_constraint_coefficient(&mut self, constraint_name: String, variable_name: String, coefficient: f64) -> PyResult<()> {
        let problem = &mut self.problem;
        problem
            .update_constraint_coefficient(&constraint_name, &variable_name, coefficient)
            .map_err(|err| to_py_err("Failed to update constraint coefficient", err))?;

        Ok(())
    }

    /// Set the right-hand side of a standard, indicator or quadratic
    /// constraint.
    ///
    /// Raises `LpObjectNotFoundError` if the constraint does not exist and
    /// `LpInvalidValueError` for an SOS or general constraint, an empty name
    /// or a non-finite `new_rhs`.
    fn update_constraint_rhs(&mut self, constraint_name: String, new_rhs: f64) -> PyResult<()> {
        let problem = &mut self.problem;
        problem.update_constraint_rhs(&constraint_name, new_rhs).map_err(|err| to_py_err("Failed to update constraint RHS", err))?;

        Ok(())
    }

    /// Rename a constraint. Its position and class (normal, lazy or user cut)
    /// are kept.
    ///
    /// Raises `LpObjectNotFoundError` if `old_name` does not exist and
    /// `LpInvalidValueError` if `new_name` is already in use or either name
    /// is empty.
    fn rename_constraint(&mut self, old_name: String, new_name: String) -> PyResult<()> {
        let problem = &mut self.problem;
        problem.rename_constraint(&old_name, &new_name).map_err(|err| to_py_err("Failed to rename constraint", err))?;

        Ok(())
    }

    /// Remove a constraint. Its variables stay in the problem.
    ///
    /// Raises `LpObjectNotFoundError` if the constraint does not exist and
    /// `LpInvalidValueError` if `constraint_name` is empty.
    fn remove_constraint(&mut self, constraint_name: String) -> PyResult<()> {
        require_name("constraint_name", &constraint_name)?;
        let problem = &mut self.problem;
        problem.remove_constraint(&constraint_name).map_err(|err| to_py_err("Failed to remove constraint", err))?;

        Ok(())
    }

    /// Rename a variable everywhere it appears: objectives, constraints and
    /// its bounds and type declarations.
    ///
    /// Raises `LpObjectNotFoundError` if `old_name` does not exist and
    /// `LpInvalidValueError` if `new_name` is already in use or either name
    /// is empty.
    fn rename_variable(&mut self, old_name: String, new_name: String) -> PyResult<()> {
        let problem = &mut self.problem;
        problem.rename_variable(&old_name, &new_name).map_err(|err| to_py_err("Failed to rename variable", err))?;

        Ok(())
    }

    /// Change a variable's type. `var_type` is case-insensitive.
    ///
    /// - `"continuous"` changes only the kind and keeps declared bounds.
    /// - `"binary"`, `"integer"`, `"general"`, `"semicontinuous"` and
    ///   `"semiinteger"` set the kind and clear declared bounds, so the
    ///   format default applies (LP: lower 0, upper +inf). `"integer"` and
    ///   `"general"` both mean a general integer variable.
    /// - `"free"` is a bound rather than a kind: the variable becomes
    ///   continuous with bounds `(-inf, +inf)`.
    ///
    /// Raises `LpObjectNotFoundError` if the variable does not exist and
    /// `LpInvalidValueError` for an unknown `var_type` or an empty name.
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

    /// Remove a variable and every term that uses it, in objectives,
    /// constraints (including SOS weights) and declarations.
    ///
    /// Constraints left with no terms are kept. Raises
    /// `LpObjectNotFoundError` if the variable does not exist and
    /// `LpInvalidValueError` if the name is empty or the variable is the
    /// indicator of an indicator constraint or appears in a general
    /// constraint; remove those constraints first.
    fn remove_variable(&mut self, variable_name: String) -> PyResult<()> {
        require_name("variable_name", &variable_name)?;
        let problem = &mut self.problem;
        problem.remove_variable(&variable_name).map_err(|err| to_py_err("Failed to remove variable", err))?;

        Ok(())
    }

    /// Set the problem name written by `to_lp_string` and `to_mps_string`.
    fn set_problem_name(&mut self, name: String) -> PyResult<()> {
        let problem = &mut self.problem;
        problem.name = Some(name);

        Ok(())
    }

    /// Set the optimisation sense.
    ///
    /// Accepts `"maximize"`, `"max"`, `"minimize"` or `"min"`, in any case.
    /// Raises `LpInvalidValueError` for anything else.
    fn set_sense(&mut self, sense: String) -> PyResult<()> {
        let problem = &mut self.problem;

        problem.sense = match sense.to_lowercase().as_str() {
            "maximize" | "max" => Sense::Maximize,
            "minimize" | "min" => Sense::Minimize,
            _ => return Err(LpInvalidValueError::new_err(format!("Invalid sense: {sense}. Use 'maximize' ('max') or 'minimize' ('min')"))),
        };

        Ok(())
    }

    /// Collect statistics about the problem and flag likely modelling or
    /// numerical problems before handing it to a solver.
    ///
    /// Returns a dict with these keys:
    ///
    /// - `summary`: name, sense, counts, `total_nonzeros` and `density`.
    /// - `sparsity`: minimum and maximum variables per constraint.
    /// - `variables`: `type_distribution` and lists of free, fixed, unused
    ///   and invalidly bounded variables.
    /// - `constraints`: `type_distribution`, empty and singleton constraints,
    ///   `rhs_range` and an SOS summary.
    /// - `coefficients`: `constraint_coeff_range`, `objective_coeff_range`
    ///   (each `{min, max, count}`), `coefficient_ratio` and the lists of
    ///   large and small coefficients.
    /// - `issues`: a list of `{severity, category, message, details}` dicts,
    ///   where `severity` is `"ERROR"`, `"WARNING"` or `"INFO"`.
    ///
    /// The keyword-only thresholds control what is flagged: coefficients
    /// above `large_coeff_threshold` or below `small_coeff_threshold` (in
    /// absolute value), a max/min coefficient ratio above `ratio_threshold`,
    /// and right-hand sides above `large_rhs_threshold`.
    ///
    /// Raises `LpInvalidValueError` if a threshold is not finite and positive,
    /// or if `small_coeff_threshold` exceeds `large_coeff_threshold`.
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
        // The core treats this as a precondition (debug_assert), so reject it
        // here rather than let a debug build panic.
        if small_coeff_threshold > large_coeff_threshold {
            return Err(LpInvalidValueError::new_err(format!(
                "small_coeff_threshold ({small_coeff_threshold}) must not exceed large_coeff_threshold ({large_coeff_threshold})"
            )));
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

/// Build the Python dict for one constraint, as exposed by `constraints` and
/// `get_constraint`.
fn constraint_to_dict<'py>(py: Python<'py>, problem: &LpProblem, name_id: NameId, constraint: &Constraint) -> PyResult<Bound<'py, PyDict>> {
    let dict = PyDict::new(py);
    dict.set_item("name", problem.resolve(name_id))?;

    match constraint {
        Constraint::Standard { coefficients, operator, rhs, .. } => {
            dict.set_item("type", "standard")?;
            dict.set_item("coefficients", coefficients_to_list(py, problem, coefficients)?)?;
            dict.set_item("operator", format!("{operator:?}"))?;
            dict.set_item("rhs", rhs)?;
            dict.set_item("class", constraint_class_name(problem.constraint_class(name_id)))?;
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
            dict.set_item("class", constraint_class_name(problem.constraint_class(name_id)))?;
        }
        Constraint::Indicator { variable, active_value, coefficients, operator, rhs, .. } => {
            dict.set_item("type", "indicator")?;
            dict.set_item("indicator_variable", problem.resolve(*variable))?;
            dict.set_item("indicator_value", u8::from(*active_value))?;
            dict.set_item("coefficients", coefficients_to_list(py, problem, coefficients)?)?;
            dict.set_item("operator", format!("{operator:?}"))?;
            dict.set_item("rhs", rhs)?;
            dict.set_item("class", constraint_class_name(problem.constraint_class(name_id)))?;
        }
        Constraint::SOS { weights, sos_type, .. } => {
            dict.set_item("type", "sos")?;
            dict.set_item("sos_type", format!("{sos_type:?}"))?;
            dict.set_item("weights", coefficients_to_list(py, problem, weights)?)?;
        }
    }
    Ok(dict)
}

/// Build the Python dict for one variable, as exposed by `variables` and
/// `get_variable`.
fn variable_to_dict<'py>(py: Python<'py>, problem: &LpProblem, name_id: NameId, variable: &Variable) -> PyResult<Bound<'py, PyDict>> {
    let dict = PyDict::new(py);
    dict.set_item("name", problem.resolve(name_id))?;
    // Structured kind + bounds rather than a Debug string: `lower`/`upper`
    // are `None` when undeclared on that side (the format default
    // applies), and -inf/+inf when explicitly free.
    dict.set_item("kind", variable.kind.to_string())?;
    dict.set_item("lower", variable.bounds.lower)?;
    dict.set_item("upper", variable.bounds.upper)?;
    Ok(dict)
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
