"""parse_lp - A fast LP file format parser for Python, powered by Rust.

This package provides a high-performance parser for Linear Programming (LP) files,
leveraging Rust for speed and Python for ease of use.

Key Features:
- Fast parsing of LP files of various formats
- Export parsed data to CSV files
- Access to problem components (variables, constraints, objectives)
- Compare two LP problems to find differences
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
    >>> # Export to CSV files
    >>> parser.to_csv("output/")
    >>>
    >>> # Compare two problems
    >>> parser2 = LpParser("modified_problem.lp")
    >>> parser2.parse()
    >>> diff = parser.compare(parser2)
    >>> print(f"Added variables: {diff['added_variables']}")

"""

from .parse_lp import LpParser

__version__ = "2.4.2"
__all__ = ["LpParser"]
