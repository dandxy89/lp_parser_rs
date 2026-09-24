"""parse_lp - A fast LP file format parser, writer, and modifier for Python, powered by Rust.

Parse, inspect, modify, and write Linear Programming (LP) files with full
round-trip compatibility. The public API is the ``LpParser`` class; see the
README and ``parse_lp.pyi`` stubs for the full method reference.
"""

from importlib.metadata import version as _version

from .parse_lp import (
    LpInvalidValueError,
    LpObjectNotFoundError,
    LpParseError,
    LpParser,
)

# Type aliases (Objective, Constraint, VariableInfo, ...) exist only in the
# parse_lp.pyi stub, not at runtime, so importing them from parse_lp.parse_lp
# fails outside a type checker. Import them inside an `if TYPE_CHECKING:` block
# and use them only in annotations.

__version__ = _version("parse_lp")
__all__ = [
    "LpInvalidValueError",
    "LpObjectNotFoundError",
    "LpParseError",
    "LpParser",
    "__version__",
]
