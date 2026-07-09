"""parse_lp - A fast LP file format parser, writer, and modifier for Python, powered by Rust.

Parse, inspect, modify, and write Linear Programming (LP) files with full
round-trip compatibility. The public API is the ``LpParser`` class; see the
README and ``parse_lp.pyi`` stubs for the full method reference.
"""

from importlib.metadata import version as _version

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
