from typing import Any, Literal, TypedDict

from typing_extensions import TypeAlias

# Custom exception classes (all subclass RuntimeError for backwards compatibility)
class LpParseError(RuntimeError):
    """Raised when an LP file or problem cannot be parsed."""

class LpNotParsedError(RuntimeError):
    """Raised when a method requires parse() to have been called first."""

class LpObjectNotFoundError(RuntimeError):
    """Raised when a named variable, constraint or objective cannot be found."""

class LpInvalidValueError(RuntimeError):
    """Raised when an input value is invalid."""

# Type definitions for structured data
Sense: TypeAlias = Literal["maximize", "minimize"]
SenseInput: TypeAlias = Literal["maximize", "max", "minimize", "min"]
VariableType: TypeAlias = Literal["binary", "integer", "general", "free", "semicontinuous"]

class Coefficient(TypedDict):
    name: str
    value: float

class Objective(TypedDict):
    name: str
    coefficients: list[Coefficient]

class VariableInfo(TypedDict):
    name: str
    var_type: str

class StandardConstraint(TypedDict):
    name: str
    type: Literal["standard"]
    coefficients: list[Coefficient]
    operator: str
    rhs: float

class SOSConstraint(TypedDict):
    name: str
    type: Literal["sos"]
    sos_type: str
    weights: list[Coefficient]

Constraint: TypeAlias = StandardConstraint | SOSConstraint

# Analysis result dictionary as built in src/lib.rs; see analyze() docs for the
# keys (summary, sparsity, variables, constraints, coefficients, issues).
ProblemAnalysis: TypeAlias = dict[str, Any]

class LpParser:
    """Parser, modifier and writer for LP format files, powered by Rust."""

    def __init__(self, lp_file: str) -> None:
        """Create a parser for the given LP file path; raises FileNotFoundError if it is not a file."""

    @property
    def lp_file(self) -> str:
        """Path to the LP file backing this parser."""

    @property
    def name(self) -> str | None:
        """Problem name, if one is declared in the LP file."""

    @property
    def sense(self) -> Sense:
        """Optimisation sense, either 'maximize' or 'minimize'."""

    @property
    def objectives(self) -> list[Objective]:
        """List of objectives with their coefficients."""

    @property
    def constraints(self) -> list[Constraint]:
        """List of standard and SOS constraints."""

    @property
    def variables(self) -> dict[str, VariableInfo]:
        """Mapping of variable name to variable information."""

    def parse(self) -> None:
        """Read and parse the LP file; must be called before accessing problem data."""

    def to_csv(self, base_directory: str) -> None:
        """Export the problem to CSV files in the given directory (parses lazily if needed)."""

    def to_lp_string(
        self,
        *,
        include_problem_name: bool = True,
        max_line_length: int = 80,
        decimal_precision: int = 6,
        include_section_spacing: bool = True,
    ) -> str:
        """Write the current problem to an LP format string, with optional custom formatting."""

    def save_to_file(self, filepath: str) -> None:
        """Save the current problem to an LP file."""

    def update_objective_coefficient(
        self,
        objective_name: str,
        variable_name: str,
        coefficient: float,
    ) -> None:
        """Update or add a coefficient in an objective."""

    def rename_objective(self, old_name: str, new_name: str) -> None:
        """Rename an objective."""

    def remove_objective(self, objective_name: str) -> None:
        """Remove an objective."""

    def update_constraint_coefficient(
        self,
        constraint_name: str,
        variable_name: str,
        coefficient: float,
    ) -> None:
        """Update or add a coefficient in a constraint."""

    def update_constraint_rhs(self, constraint_name: str, new_rhs: float) -> None:
        """Update the right-hand side value of a constraint."""

    def rename_constraint(self, old_name: str, new_name: str) -> None:
        """Rename a constraint."""

    def remove_constraint(self, constraint_name: str) -> None:
        """Remove a constraint."""

    def rename_variable(self, old_name: str, new_name: str) -> None:
        """Rename a variable across all objectives and constraints."""

    def update_variable_type(self, variable_name: str, var_type: VariableType) -> None:
        """Change a variable's type (binary, integer, general, free, semicontinuous)."""

    def remove_variable(self, variable_name: str) -> None:
        """Remove a variable from all objectives and constraints."""

    def set_problem_name(self, name: str) -> None:
        """Set the problem name."""

    def set_sense(self, sense: SenseInput) -> None:
        """Set the optimisation sense ('maximize' or 'minimize')."""

    def analyze(
        self,
        *,
        large_coeff_threshold: float = 1e9,
        small_coeff_threshold: float = 1e-9,
        ratio_threshold: float = 1e6,
    ) -> ProblemAnalysis:
        """Perform comprehensive analysis of the problem (statistics, structure and issues)."""
