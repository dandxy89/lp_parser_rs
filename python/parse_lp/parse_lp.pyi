"""Type stubs for parse_lp module."""

from typing import Any, Dict, List, Optional

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
    def name(self) -> Optional[str]:
        """Get the name of the LP problem.

        Returns:
            The problem name if available and parsed, None otherwise.
        """
        ...

    @property
    def sense(self) -> Optional[str]:
        """Get the optimization sense of the problem.

        Returns:
            'maximize' or 'minimize' if parsed, None otherwise.
        """
        ...

    @property
    def objectives(self) -> List[Dict[str, Any]]:
        """Get the list of objective functions.

        Returns:
            List of dictionaries containing objective information:
            - name: Name of the objective
            - coefficients: List of coefficient dictionaries with 'name' and 'value'
        """
        ...

    @property
    def constraints(self) -> List[Dict[str, Any]]:
        """Get the list of constraints.

        Returns:
            List of dictionaries containing constraint information:
            - name: Name of the constraint
            - type: 'standard' or 'sos'
            - For standard constraints: coefficients, sense, rhs
            - For SOS constraints: sos_type, variables
        """
        ...

    @property
    def variables(self) -> Dict[str, Dict[str, Any]]:
        """Get the dictionary of variables.

        Returns:
            Dictionary mapping variable names to their properties:
            - name: Variable name
            - var_type: Type of variable (e.g., 'Continuous', 'Binary', 'Integer')
            - bounds: Optional dictionary with 'lower' and/or 'upper' bounds
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

    def compare(self, other: "LpParser") -> Dict[str, Any]:
        """Compare this LP problem with another.

        Both parsers must be parsed before comparison.

        Args:
            other: Another LpParser instance to compare with.

        Returns:
            Dictionary containing comparison results:
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
            RuntimeError: If either parser hasn't been parsed yet.
        """
        ...
