"""parse_lp - A fast LP file format parser, writer, and modifier for Python, powered by Rust.

This package provides a high-performance parser for Linear Programming (LP) files,
leveraging Rust for speed and Python for ease of use. It supports parsing, modifying,
and writing LP files with full round-trip compatibility.

Key Features:
- Fast parsing of LP files of various formats
- Programmatic modification of LP problems (objectives, constraints, variables)
- Write LP problems back to standard LP format
- Export parsed data to CSV files
- Access to problem components (variables, constraints, objectives)
- Type hints for better IDE support

Example Usage:
    >>> from parse_lp import LpParser
    >>>
    >>> # Parse an LP file
    >>> parser = LpParser("optimization_problem.lp")
    >>> parser.parse()
    >>>
    >>> # Access problem information
    >>> print(f"Problem name: {parser.name}")
    >>> print(f"Variables: {parser.variable_count()}")
    >>> print(f"Constraints: {parser.constraint_count()}")
    >>>
    >>> # Modify the problem
    >>> parser.update_objective_coefficient("profit", "x1", 5.0)
    >>> parser.rename_variable("x2", "production")
    >>> parser.update_constraint_rhs("capacity", 100.0)
    >>> parser.update_variable_type("x1", "integer")
    >>>
    >>> # Write back to LP format
    >>> modified_lp = parser.to_lp_string()
    >>> parser.save_to_file("modified_problem.lp")
    >>>
    >>> # Export to CSV files
    >>> parser.to_csv("output/")

Modification Methods:
    Objective modifications:
    - update_objective_coefficient(obj_name, var_name, coefficient)
    - rename_objective(old_name, new_name)
    - remove_objective(obj_name)

    Constraint modifications:
    - update_constraint_coefficient(const_name, var_name, coefficient)
    - update_constraint_rhs(const_name, new_rhs)
    - rename_constraint(old_name, new_name)
    - remove_constraint(const_name)

    Variable modifications:
    - rename_variable(old_name, new_name)
    - update_variable_type(var_name, var_type)  # binary, integer, general, free, semicontinuous
    - remove_variable(var_name)

    Problem modifications:
    - set_problem_name(name)
    - set_sense(sense)  # maximize, minimize

"""

from importlib.metadata import version as _version

# pyrefly: ignore [missing-import]
from .parse_lp import (
    LpInvalidValueError,
    LpNotParsedError,
    LpObjectNotFoundError,
    LpParseError,
    LpParser,
)

# Type aliases are defined in parse_lp.pyi for static type checking.
# Users can import them with: from parse_lp.parse_lp import Objective, Constraint, etc.

__version__ = _version("parse_lp")
__all__ = [
    "LpInvalidValueError",
    "LpNotParsedError",
    "LpObjectNotFoundError",
    "LpParseError",
    "LpParser",
    "__version__",
]
