"""Read, inspect, edit and write LP and MPS optimisation models.

The parsing and writing are done by the Rust crate ``lp_parser_rs``. Everything
goes through ``LpParser``; its methods raise ``LpParseError``,
``LpObjectNotFoundError`` or ``LpInvalidValueError`` (all ``RuntimeError``
subclasses), or ``OSError`` for file-system failures. The ``parse_lp.pyi``
stub documents the shape of every returned dict.
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
