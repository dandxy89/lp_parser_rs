from typing import Literal, TypedDict

# Custom exception classes
class LpParseError(Exception):
    """Raised when parsing an LP file fails."""

    ...

class LpNotParsedError(Exception):
    """Raised when accessing data before calling parse()."""

    ...

class LpObjectNotFoundError(Exception):
    """Raised when a named object (variable, constraint, objective) is not found."""

    ...

class LpInvalidValueError(Exception):
    """Raised when an invalid value is provided (e.g., invalid sense or variable type)."""

    ...

# Type definitions for structured data
Sense = Literal["maximize", "minimize"]
SenseInput = Literal["maximize", "max", "minimize", "min"]
VariableType = Literal["binary", "integer", "general", "free", "semicontinuous"]

class Coefficient(TypedDict):
    """A coefficient in an objective or constraint."""

    name: str
    value: float

class Objective(TypedDict):
    """An objective function with its coefficients."""

    name: str
    coefficients: list[Coefficient]

class VariableBounds(TypedDict, total=False):
    """Bounds for a variable (all fields optional)."""

    lower: float
    upper: float

class VariableInfo(TypedDict):
    """Information about a variable."""

    name: str
    var_type: str
    bounds: VariableBounds | None

class StandardConstraint(TypedDict):
    """A standard linear constraint."""

    name: str
    type: Literal["standard"]
    coefficients: list[Coefficient]
    operator: str
    rhs: float

class SOSConstraint(TypedDict):
    """A Special Ordered Set (SOS) constraint."""

    name: str
    type: Literal["sos"]
    sos_type: str
    variables: list[str]

Constraint = StandardConstraint | SOSConstraint

class ComparisonResult(TypedDict):
    """Result of comparing two LP problems."""

    name_changed: bool
    sense_changed: bool
    variable_count_diff: int
    constraint_count_diff: int
    objective_count_diff: int
    added_variables: list[str]
    removed_variables: list[str]
    modified_variables: list[str]
    added_constraints: list[str]
    removed_constraints: list[str]

class LpParser:
    """A fast LP file format parser.

    This class provides functionality to parse Linear Programming (LP) files
    and access the parsed data or export it to CSV format.

    Attributes:
        lp_file: Path to the LP file being parsed.
        name: Name of the LP problem (after parsing).
        sense: Optimization sense ('maximize' or 'minimize').
        objectives: List of objective functions with coefficients.
        constraints: List of constraints with their properties.
        variables: Dictionary of variables with their properties.

    Example:
        >>> from parse_lp import LpParser
        >>> parser = LpParser("path/to/problem.lp")
        >>> parser.parse()  # Parse the file
        >>> print(f"Problem: {parser.name}")
        >>> print(f"Variables: {parser.variable_count()}")
        >>> parser.to_csv("output/directory/")
    """

    def __init__(self, lp_file: str) -> None:
        """Initialize the LP parser with a file path.

        Args:
            lp_file: Path to the LP file to parse.

        Raises:
            FileExistsError: If the specified file does not exist.
        """
        ...

    @property
    def lp_file(self) -> str:
        """Get the path to the LP file.

        Returns:
            The path to the LP file being parsed.
        """
        ...

    @property
    def name(self) -> str | None:
        """Get the name of the LP problem.

        Returns:
            The problem name if available and parsed, None otherwise.
        """
        ...

    @property
    def sense(self) -> Sense | None:
        """Get the optimization sense of the problem.

        Returns:
            'maximize' or 'minimize' if parsed, None otherwise.
        """
        ...

    @property
    def objectives(self) -> list[Objective]:
        """Get the list of objective functions.

        Returns:
            List of Objective TypedDicts containing:
            - name: Name of the objective
            - coefficients: List of Coefficient dicts with 'name' and 'value'
        """
        ...

    @property
    def constraints(self) -> list[Constraint]:
        """Get the list of constraints.

        Returns:
            List of Constraint TypedDicts (StandardConstraint or SOSConstraint):
            - name: Name of the constraint
            - type: 'standard' or 'sos'
            - For standard constraints: coefficients, operator, rhs
            - For SOS constraints: sos_type, variables
        """
        ...

    @property
    def variables(self) -> dict[str, VariableInfo]:
        """Get the dictionary of variables.

        Returns:
            Dictionary mapping variable names to VariableInfo TypedDicts:
            - name: Variable name
            - var_type: Type of variable (e.g., 'Continuous', 'Binary', 'Integer')
            - bounds: Optional VariableBounds with 'lower' and/or 'upper'
        """
        ...

    def parse(self) -> None:
        """Parse the LP file.

        This method reads and parses the LP file, storing the problem
        data internally for later access.

        Raises:
            RuntimeError: If the LP file cannot be read or parsed.
        """
        ...

    def to_csv(self, base_directory: str) -> None:
        """Export the parsed LP problem to CSV files.

        This method parses the LP file (if not already parsed) and exports
        the problem data (variables, constraints, and objectives) to separate
        CSV files in the specified directory.

        Args:
            base_directory: Path to the directory where CSV files will be saved.
                           The directory must exist.

        Raises:
            NotADirectoryError: If base_directory is not a valid directory.
            RuntimeError: If the LP file cannot be read or parsed, or if
                         CSV files cannot be written.
        """
        ...

    def variable_count(self) -> int:
        """Get the number of variables in the problem.

        Returns:
            Number of variables, or 0 if not parsed.
        """
        ...

    def constraint_count(self) -> int:
        """Get the number of constraints in the problem.

        Returns:
            Number of constraints, or 0 if not parsed.
        """
        ...

    def objective_count(self) -> int:
        """Get the number of objective functions in the problem.

        Returns:
            Number of objectives, or 0 if not parsed.
        """
        ...

    def compare(self, other: LpParser) -> ComparisonResult:
        """Compare this LP problem with another.

        Both parsers must be parsed before comparison.

        Args:
            other: Another LpParser instance to compare with.

        Returns:
            ComparisonResult TypedDict containing:
            - name_changed: Whether problem names differ
            - sense_changed: Whether optimization senses differ
            - variable_count_diff: Difference in variable counts
            - constraint_count_diff: Difference in constraint counts
            - objective_count_diff: Difference in objective counts
            - added_variables: Variables in 'other' but not in 'self'
            - removed_variables: Variables in 'self' but not in 'other'
            - modified_variables: Variables that exist in both but differ
            - added_constraints: Constraints in 'other' but not in 'self'
            - removed_constraints: Constraints in 'self' but not in 'other'

        Raises:
            LpNotParsedError: If either parser hasn't been parsed yet.
        """
        ...

    def to_lp_string(self) -> str:
        """Write the current problem to LP format string.

        Returns:
            The LP problem as a formatted string.

        Raises:
            RuntimeError: If the problem cannot be serialized.
        """
        ...

    def to_lp_string_with_options(
        self,
        *,
        include_problem_name: bool = True,
        max_line_length: int = 80,
        decimal_precision: int = 6,
        include_section_spacing: bool = True,
    ) -> str:
        """Write the current problem to LP format string with custom options.

        Args:
            include_problem_name: Whether to include the problem name comment.
            max_line_length: Maximum line length for output formatting.
            decimal_precision: Number of decimal places for coefficients.
            include_section_spacing: Whether to add blank lines between sections.

        Returns:
            The LP problem as a formatted string.

        Raises:
            RuntimeError: If the problem cannot be serialized.
        """
        ...

    def save_to_file(self, filepath: str) -> None:
        """Save the current problem to an LP file.

        Args:
            filepath: Path to the output file.

        Raises:
            RuntimeError: If the file cannot be written.
        """
        ...

    def update_objective_coefficient(
        self, objective_name: str, variable_name: str, coefficient: float
    ) -> None:
        """Update a coefficient in an objective function.

        If the variable doesn't exist in the objective, it will be added.

        Args:
            objective_name: Name of the objective to modify.
            variable_name: Name of the variable whose coefficient to update.
            coefficient: New coefficient value.

        Raises:
            RuntimeError: If the objective is not found or update fails.
        """
        ...

    def rename_objective(self, old_name: str, new_name: str) -> None:
        """Rename an objective function.

        Args:
            old_name: Current name of the objective.
            new_name: New name for the objective.

        Raises:
            RuntimeError: If the objective is not found.
        """
        ...

    def remove_objective(self, objective_name: str) -> None:
        """Remove an objective function from the problem.

        Args:
            objective_name: Name of the objective to remove.

        Raises:
            RuntimeError: If the objective is not found.
        """
        ...

    def update_constraint_coefficient(
        self, constraint_name: str, variable_name: str, coefficient: float
    ) -> None:
        """Update a coefficient in a constraint.

        If the variable doesn't exist in the constraint, it will be added.

        Args:
            constraint_name: Name of the constraint to modify.
            variable_name: Name of the variable whose coefficient to update.
            coefficient: New coefficient value.

        Raises:
            RuntimeError: If the constraint is not found or update fails.
        """
        ...

    def update_constraint_rhs(self, constraint_name: str, new_rhs: float) -> None:
        """Update the right-hand side value of a constraint.

        Args:
            constraint_name: Name of the constraint to modify.
            new_rhs: New right-hand side value.

        Raises:
            RuntimeError: If the constraint is not found.
        """
        ...

    def rename_constraint(self, old_name: str, new_name: str) -> None:
        """Rename a constraint.

        Args:
            old_name: Current name of the constraint.
            new_name: New name for the constraint.

        Raises:
            RuntimeError: If the constraint is not found.
        """
        ...

    def remove_constraint(self, constraint_name: str) -> None:
        """Remove a constraint from the problem.

        Args:
            constraint_name: Name of the constraint to remove.

        Raises:
            RuntimeError: If the constraint is not found.
        """
        ...

    def rename_variable(self, old_name: str, new_name: str) -> None:
        """Rename a variable across all objectives and constraints.

        Args:
            old_name: Current name of the variable.
            new_name: New name for the variable.

        Raises:
            RuntimeError: If the variable is not found.
        """
        ...

    def update_variable_type(self, variable_name: str, var_type: VariableType) -> None:
        """Update the type of a variable.

        Args:
            variable_name: Name of the variable to modify.
            var_type: New type for the variable. Supported types:
                'binary', 'integer', 'general', 'free', 'semicontinuous'.

        Raises:
            LpObjectNotFoundError: If the variable is not found.
            LpInvalidValueError: If the type is invalid.
        """
        ...

    def remove_variable(self, variable_name: str) -> None:
        """Remove a variable from all objectives and constraints.

        Args:
            variable_name: Name of the variable to remove.

        Raises:
            RuntimeError: If the variable is not found.
        """
        ...

    def set_problem_name(self, name: str) -> None:
        """Set the problem name.

        Args:
            name: New name for the problem.
        """
        ...

    def set_sense(self, sense: SenseInput) -> None:
        """Set the optimization sense.

        Args:
            sense: 'maximize', 'max', 'minimize', or 'min'.

        Raises:
            LpInvalidValueError: If the sense value is invalid.
        """
        ...
