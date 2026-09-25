import os
from typing import Any, Literal, TypedDict

from typing_extensions import TypeAlias

# All three subclass RuntimeError, so existing `except RuntimeError` handlers
# keep working.
class LpParseError(RuntimeError):
    """Raised when LP or MPS input is not valid UTF-8 or cannot be parsed."""

class LpObjectNotFoundError(RuntimeError):
    """Raised when a named variable, constraint or objective does not exist."""

class LpInvalidValueError(RuntimeError):
    """Raised for an invalid argument, or an edit or write the problem cannot support."""

Sense: TypeAlias = Literal["maximize", "minimize"]
SenseInput: TypeAlias = Literal["maximize", "max", "minimize", "min"]
# Accepted by update_variable_type (case-insensitive).
VariableType: TypeAlias = Literal["continuous", "binary", "integer", "general", "free", "semicontinuous", "semiinteger"]
# Reported in VariableInfo["kind"].
VariableKind: TypeAlias = Literal["Continuous", "General", "Integer", "Binary", "SemiContinuous", "SemiInteger", "Sos"]
Operator: TypeAlias = Literal["GT", "GTE", "EQ", "LT", "LTE"]
Format: TypeAlias = Literal["lp", "mps"]
StrPath: TypeAlias = str | os.PathLike[str]

class Coefficient(TypedDict):
    name: str
    value: float

class QuadraticTerm(TypedDict):
    var1: str
    var2: str
    # The term's actual coefficient: an objective's LP `[ ... ] / 2` is applied.
    coefficient: float

class ObjectiveAttributes(TypedDict):
    # Gurobi multi-objective attributes (`Minimize multi-objectives`); None when unset.
    priority: int | None
    weight: float | None
    abs_tol: float | None
    rel_tol: float | None

class Objective(TypedDict):
    name: str
    coefficients: list[Coefficient]
    quadratic: list[QuadraticTerm]
    attributes: ObjectiveAttributes

class VariableInfo(TypedDict):
    name: str
    kind: VariableKind
    # None means no bound was declared on that side, so the format default
    # applies (in LP: lower 0, upper +inf). An explicit `free` bound is
    # reported as -inf / +inf, not None.
    lower: float | None
    upper: float | None

class LpDiffResult(TypedDict):
    # Old and new sense as "Maximize" / "Minimize"; None when unchanged.
    sense_changed: tuple[str, str] | None
    # *_added: only in the other problem; *_removed: only in this one.
    vars_added: list[str]
    vars_removed: list[str]
    # One (name, old_type, new_type) tuple per changed variable.
    vars_type_changed: list[tuple[str, str, str]]
    cons_added: list[str]
    cons_removed: list[str]
    # One (name, descriptions of each change) tuple per changed constraint.
    cons_modified: list[tuple[str, list[str]]]
    objs_added: list[str]
    objs_removed: list[str]
    objs_modified: list[tuple[str, list[str]]]
    is_empty: bool

# "lazy" / "user_cut" for the CPLEX `Lazy Constraints` / `User Cuts` sections.
ConstraintClass: TypeAlias = Literal["normal", "lazy", "user_cut"]

# Functional syntax: `class` is a Python keyword.
StandardConstraint = TypedDict(
    "StandardConstraint",
    {
        "name": str,
        "type": Literal["standard"],
        "coefficients": list[Coefficient],
        "operator": Operator,
        "rhs": float,
        "class": ConstraintClass,
    },
)

class SOSConstraint(TypedDict):
    name: str
    type: Literal["sos"]
    sos_type: Literal["S1", "S2"]
    weights: list[Coefficient]

# `b = 1 -> x + y <= 3`: the linear part holds whenever the (binary)
# indicator variable equals indicator_value.
IndicatorConstraint = TypedDict(
    "IndicatorConstraint",
    {
        "name": str,
        "type": Literal["indicator"],
        "indicator_variable": str,
        "indicator_value": Literal[0, 1],
        "coefficients": list[Coefficient],
        "operator": Operator,
        "rhs": float,
        "class": ConstraintClass,
    },
)

# `x + [ x ^ 2 + 2 x * y ] <= 4`
QuadraticConstraint = TypedDict(
    "QuadraticConstraint",
    {
        "name": str,
        "type": Literal["quadratic"],
        "coefficients": list[Coefficient],
        "quadratic": list[QuadraticTerm],
        "operator": Operator,
        "rhs": float,
        "class": ConstraintClass,
    },
)

# Gurobi `General Constraints`: `resultant = FUNCTION ( arguments )`.
class GeneralConstraint(TypedDict):
    name: str
    type: Literal["general"]
    resultant: str
    function: Literal["MAX", "MIN", "ABS", "AND", "OR"]
    arguments: list[str]
    constant: float | None  # MAX / MIN only

Constraint: TypeAlias = (
    StandardConstraint | SOSConstraint | IndicatorConstraint | QuadraticConstraint | GeneralConstraint
)

# Nested dict returned by analyze(); see its docstring for the keys.
ProblemAnalysis: TypeAlias = dict[str, Any]

class LpParser:
    """A parsed LP or MPS problem that can be inspected, edited and written back out.

    `LpParser(path)` reads and parses the file straight away, inferring the
    format from the extension (`.mps` is MPS, anything else is LP). Use
    `LpParser.from_file` to choose the format explicitly, or
    `LpParser.from_string` to parse text already in memory.

    A missing or unreadable file raises `OSError` (for example
    `FileNotFoundError`, or `IsADirectoryError` for a directory). A file that
    is not valid UTF-8 or does not parse raises `LpParseError`.

    All of the library's own exceptions (`LpParseError`,
    `LpObjectNotFoundError`, `LpInvalidValueError`) subclass `RuntimeError`.
    """

    def __init__(self, lp_file: StrPath) -> None: ...
    @staticmethod
    def from_string(text: str, format: Format = "lp") -> LpParser:
        """Parse LP or MPS text held in memory.

        Useful when the model comes from a generator, a database or a test
        rather than a file. `format` is `"lp"` (the default) or `"mps"`, in any
        case. The resulting parser has `lp_file == "<string>"` and no source
        file, so `parse()` cannot be called on it.

        Raises `LpParseError` if the text does not parse and
        `LpInvalidValueError` for an unknown `format`.
        """

    @staticmethod
    def from_file(path: StrPath, format: Format | None = None) -> LpParser:
        """Read and parse a file, optionally forcing the format.

        Same as `LpParser(path)`, but `format` (`"lp"` or `"mps"`, in any case)
        overrides the extension check. Use it for MPS files that do not end in
        `.mps`, such as `model.mps.txt` or files with no extension.

        Raises `OSError` if the file cannot be read, `LpParseError` if it is
        not valid UTF-8 or does not parse, and `LpInvalidValueError` for an
        unknown `format`.
        """

    @property
    def lp_file(self) -> str:
        """The source path as given to the constructor, or `"<string>"` for a parser built with `from_string`."""

    @property
    def name(self) -> str | None:
        """The problem name, or `None` if the source did not declare one.

        In LP files this is read from a leading comment such as
        `\\Problem name: diet` or `\\* diet *\\`; in MPS files from the `NAME`
        record.
        """

    @property
    def sense(self) -> Sense:
        """The optimisation sense: `"maximize"` or `"minimize"`."""

    @property
    def objectives(self) -> list[Objective]:
        """Every objective as a list of dicts, in file order.

        Each dict has `name`, `coefficients` (a list of `{name, value}`),
        `quadratic` (a list of `{var1, var2, coefficient}`, with the LP
        `[ ... ] / 2` already applied) and `attributes` (Gurobi multi-objective
        `priority`, `weight`, `abs_tol`, `rel_tol`, each `None` when unset).

        The list is rebuilt on every access and does not track later edits, so
        bind it to a variable rather than reading the property in a loop.
        """

    @property
    def constraints(self) -> list[Constraint]:
        """Every constraint as a list of dicts, in file order.

        The `type` key says which shape a dict has: `"standard"`, `"sos"`,
        `"indicator"`, `"quadratic"` or `"general"`. See the type stubs for the
        keys of each.

        The list is rebuilt on every access and does not track later edits.
        Use `get_constraint` for a single lookup and `num_constraints` for the
        count.
        """

    @property
    def variables(self) -> dict[str, VariableInfo]:
        """Every variable as a dict keyed by name.

        Each value has `name`, `kind` (such as `"Continuous"` or `"Binary"`),
        `lower` and `upper`. A bound is `None` when it was never declared, so
        the format default applies (LP: lower 0, upper +inf); a `free`
        variable reports `-inf` and `inf`.

        The dict is rebuilt on every access and does not track later edits.
        Use `get_variable` for a single lookup and `num_variables` for the
        count.
        """

    def get_constraint(self, name: str) -> Constraint:
        """Return one constraint by name, in the same shape as an entry of `constraints`, without building the whole list.

        Raises `LpObjectNotFoundError` if there is no such constraint and
        `LpInvalidValueError` if `name` is empty.
        """

    def get_variable(self, name: str) -> VariableInfo:
        """Return one variable by name, in the same shape as a value of `variables`, without building the whole dict.

        Raises `LpObjectNotFoundError` if there is no such variable and
        `LpInvalidValueError` if `name` is empty.
        """

    @property
    def num_objectives(self) -> int:
        """Number of objectives. Cheaper than `len(parser.objectives)`."""

    @property
    def num_constraints(self) -> int:
        """Number of constraints. Cheaper than `len(parser.constraints)`."""

    @property
    def num_variables(self) -> int:
        """Number of variables. Cheaper than `len(parser.variables)`."""

    def parse(self) -> None:
        """Re-read and re-parse the source file in the format it was first parsed as.

        Construction already parses the file, so call this only to pick up
        changes made to it on disk since. Any in-memory edits are discarded.

        Raises `LpInvalidValueError` for a parser built with `from_string`,
        which has no file to re-read, and otherwise the same exceptions as the
        constructor. On failure the previously parsed problem is kept.
        """

    def to_csv(self, base_directory: StrPath) -> None:
        """Write the problem as three CSV files for inspection in a spreadsheet or dataframe.

        Creates (or overwrites) `objectives.csv`, `constraints.csv` and
        `variables.csv` in `base_directory`, which must already exist.
        Constraints have one row per variable; quadratic terms appear as
        `x*y`.

        Raises `NotADirectoryError` if `base_directory` is not an existing
        directory and `OSError` if a file cannot be written.
        """

    def to_lp_string(
        self,
        *,
        include_problem_name: bool = True,
        max_line_length: int = 80,
        decimal_precision: int | None = None,
        include_section_spacing: bool = True,
    ) -> str:
        """Serialise the current problem, including any edits, to LP text.

        With the defaults the output parses back to the same problem.
        `include_problem_name` writes a `\\Problem name:` comment at the top.
        `max_line_length` is where long expressions wrap. `decimal_precision`
        rounds every number to that many decimal places, which loses
        precision; `None` writes the shortest form that reads back exactly.
        `include_section_spacing` puts a blank line between sections.

        Raises `LpInvalidValueError` if `max_line_length` is 0 or the problem
        holds something LP cannot express, such as a name containing
        characters LP does not allow.
        """

    def save_to_file(self, filepath: StrPath) -> None:
        """Write the current problem, including any edits, to an LP file.

        Uses the default `to_lp_string` formatting and overwrites an existing
        file. For custom formatting, write the result of `to_lp_string`
        yourself.

        Raises `OSError` if the file cannot be written (for example
        `FileNotFoundError` when the parent directory is missing) and
        `LpInvalidValueError` as `to_lp_string` does.
        """

    def to_mps_string(self, *, decimal_precision: int | None = None, allow_multiple_objectives: bool = False) -> str:
        """Serialise the current problem, including any edits, to MPS text.

        Use this to hand the model to a solver that prefers MPS, or to convert
        LP files to MPS. `decimal_precision` behaves as in `to_lp_string`.

        MPS holds a single objective, so a problem with several raises
        `LpInvalidValueError` unless `allow_multiple_objectives` is true, in
        which case only the first is written (without any Gurobi
        multi-objective attributes). Strict inequalities (`<`, `>`) and
        general constraints cannot be written either and also raise
        `LpInvalidValueError`.
        """

    def save_to_mps(
        self, filepath: StrPath, *, decimal_precision: int | None = None, allow_multiple_objectives: bool = False
    ) -> None:
        """Write the current problem to an MPS file, overwriting any existing one.

        Takes the same options, and raises the same errors, as
        `to_mps_string`, plus `OSError` if the file cannot be written.
        """

    def diff(self, other: LpParser) -> LpDiffResult:
        """Compare this problem (the old one) with `other` (the new one) by name.

        Useful for checking what a model generator or a set of edits actually
        changed. Returns a dict:

        - `sense_changed`: `(old, new)` such as `("Minimize", "Maximize")`, or
          `None` if the sense is the same.
        - `vars_added`, `cons_added`, `objs_added`: names only in `other`.
        - `vars_removed`, `cons_removed`, `objs_removed`: names only in `self`.
        - `vars_type_changed`: `(name, old_type, new_type)` tuples.
        - `cons_modified`, `objs_modified`: `(name, changes)` tuples, where
          `changes` is a list of human-readable descriptions.
        - `is_empty`: true when nothing differs.
        """

    def update_objective_coefficient(
        self,
        objective_name: str,
        variable_name: str,
        coefficient: float,
    ) -> None:
        """Set a variable's coefficient in an objective.

        Replaces the coefficient if the variable is already in the objective
        and adds the term if not; a variable not yet in the problem is
        declared as continuous. A coefficient of 0 removes the term.

        Raises `LpObjectNotFoundError` if the objective does not exist and
        `LpInvalidValueError` if `variable_name` is empty or `coefficient` is
        not finite.
        """

    def rename_objective(self, old_name: str, new_name: str) -> None:
        """Rename an objective.

        Raises `LpObjectNotFoundError` if `old_name` does not exist and
        `LpInvalidValueError` if `new_name` is already in use or either name
        is empty.
        """

    def remove_objective(self, objective_name: str) -> None:
        """Remove an objective. Its variables stay in the problem.

        Raises `LpObjectNotFoundError` if the objective does not exist and
        `LpInvalidValueError` if `objective_name` is empty.
        """

    def update_constraint_coefficient(
        self,
        constraint_name: str,
        variable_name: str,
        coefficient: float,
    ) -> None:
        """Set a variable's coefficient in a constraint.

        Works on the linear part of standard, indicator and quadratic
        constraints. Replaces the coefficient if the variable is already
        present and adds the term if not; a variable not yet in the problem is
        declared as continuous. A coefficient of 0 removes the term.

        Raises `LpObjectNotFoundError` if the constraint does not exist and
        `LpInvalidValueError` for an SOS or general constraint, an empty
        `variable_name` or a non-finite `coefficient`.
        """

    def update_constraint_rhs(self, constraint_name: str, new_rhs: float) -> None:
        """Set the right-hand side of a standard, indicator or quadratic constraint.

        Raises `LpObjectNotFoundError` if the constraint does not exist and
        `LpInvalidValueError` for an SOS or general constraint, an empty name
        or a non-finite `new_rhs`.
        """

    def rename_constraint(self, old_name: str, new_name: str) -> None:
        """Rename a constraint. Its position and class (normal, lazy or user cut) are kept.

        Raises `LpObjectNotFoundError` if `old_name` does not exist and
        `LpInvalidValueError` if `new_name` is already in use or either name
        is empty.
        """

    def remove_constraint(self, constraint_name: str) -> None:
        """Remove a constraint. Its variables stay in the problem.

        Raises `LpObjectNotFoundError` if the constraint does not exist and
        `LpInvalidValueError` if `constraint_name` is empty.
        """

    def rename_variable(self, old_name: str, new_name: str) -> None:
        """Rename a variable everywhere it appears: objectives, constraints and its bounds and type declarations.

        Raises `LpObjectNotFoundError` if `old_name` does not exist and
        `LpInvalidValueError` if `new_name` is already in use or either name
        is empty.
        """

    def update_variable_type(self, variable_name: str, var_type: VariableType) -> None:
        """Change a variable's type. `var_type` is case-insensitive.

        - `"continuous"` changes only the kind and keeps declared bounds.
        - `"binary"`, `"integer"`, `"general"`, `"semicontinuous"` and
          `"semiinteger"` set the kind and clear declared bounds, so the
          format default applies (LP: lower 0, upper +inf). `"integer"` and
          `"general"` both mean a general integer variable.
        - `"free"` is a bound rather than a kind: the variable becomes
          continuous with bounds `(-inf, +inf)`.

        Raises `LpObjectNotFoundError` if the variable does not exist and
        `LpInvalidValueError` for an unknown `var_type` or an empty name.
        """

    def remove_variable(self, variable_name: str) -> None:
        """Remove a variable and every term that uses it, in objectives, constraints (including SOS weights) and declarations.

        Constraints left with no terms are kept. Raises
        `LpObjectNotFoundError` if the variable does not exist and
        `LpInvalidValueError` if the name is empty or the variable is the
        indicator of an indicator constraint or appears in a general
        constraint; remove those constraints first.
        """

    def set_problem_name(self, name: str) -> None:
        """Set the problem name written by `to_lp_string` and `to_mps_string`."""

    def set_sense(self, sense: SenseInput) -> None:
        """Set the optimisation sense.

        Accepts `"maximize"`, `"max"`, `"minimize"` or `"min"`, in any case.
        Raises `LpInvalidValueError` for anything else.
        """

    def analyze(
        self,
        *,
        large_coeff_threshold: float = 1e9,
        small_coeff_threshold: float = 1e-9,
        ratio_threshold: float = 1e6,
        large_rhs_threshold: float = 1e9,
    ) -> ProblemAnalysis:
        """Collect statistics about the problem and flag likely modelling or numerical problems before handing it to a solver.

        Returns a dict with these keys:

        - `summary`: name, sense, counts, `total_nonzeros` and `density`.
        - `sparsity`: minimum and maximum variables per constraint.
        - `variables`: `type_distribution` and lists of free, fixed, unused
          and invalidly bounded variables.
        - `constraints`: `type_distribution`, empty and singleton constraints,
          `rhs_range` and an SOS summary.
        - `coefficients`: `constraint_coeff_range`, `objective_coeff_range`
          (each `{min, max, count}`), `coefficient_ratio` and the lists of
          large and small coefficients.
        - `issues`: a list of `{severity, category, message, details}` dicts,
          where `severity` is `"ERROR"`, `"WARNING"` or `"INFO"`.

        The keyword-only thresholds control what is flagged: coefficients
        above `large_coeff_threshold` or below `small_coeff_threshold` (in
        absolute value), a max/min coefficient ratio above `ratio_threshold`,
        and right-hand sides above `large_rhs_threshold`.

        Raises `LpInvalidValueError` if a threshold is not finite and positive,
        or if `small_coeff_threshold` exceeds `large_coeff_threshold`.
        """
