from typing import Literal, TypedDict

from typing_extensions import TypeAlias

# Custom exception classes
class LpParseError(Exception): ...
class LpNotParsedError(Exception): ...
class LpObjectNotFoundError(Exception): ...
class LpInvalidValueError(Exception): ...

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

class VariableBounds(TypedDict, total=False):
    lower: float
    upper: float

class VariableInfo(TypedDict):
    name: str
    var_type: str
    bounds: VariableBounds | None

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
    variables: list[str]

Constraint: TypeAlias = StandardConstraint | SOSConstraint

class ComparisonResult(TypedDict):
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
    def __init__(self, lp_file: str) -> None: ...
    @property
    def lp_file(self) -> str: ...
    @property
    def name(self) -> str | None: ...
    @property
    def sense(self) -> Sense | None: ...
    @property
    def objectives(self) -> list[Objective]: ...
    @property
    def constraints(self) -> list[Constraint]: ...
    @property
    def variables(self) -> dict[str, VariableInfo]: ...
    def parse(self) -> None: ...
    def to_csv(self, base_directory: str) -> None: ...
    def variable_count(self) -> int: ...
    def constraint_count(self) -> int: ...
    def objective_count(self) -> int: ...
    def compare(self, other: LpParser) -> ComparisonResult: ...
    def to_lp_string(self) -> str: ...
    def to_lp_string_with_options(
        self,
        *,
        include_problem_name: bool = True,
        max_line_length: int = 80,
        decimal_precision: int = 6,
        include_section_spacing: bool = True,
    ) -> str: ...
    def save_to_file(self, filepath: str) -> None: ...
    def update_objective_coefficient(
        self,
        objective_name: str,
        variable_name: str,
        coefficient: float,
    ) -> None: ...
    def rename_objective(self, old_name: str, new_name: str) -> None: ...
    def remove_objective(self, objective_name: str) -> None: ...
    def update_constraint_coefficient(
        self,
        constraint_name: str,
        variable_name: str,
        coefficient: float,
    ) -> None: ...
    def update_constraint_rhs(self, constraint_name: str, new_rhs: float) -> None: ...
    def rename_constraint(self, old_name: str, new_name: str) -> None: ...
    def remove_constraint(self, constraint_name: str) -> None: ...
    def rename_variable(self, old_name: str, new_name: str) -> None: ...
    def update_variable_type(self, variable_name: str, var_type: VariableType) -> None: ...
    def remove_variable(self, variable_name: str) -> None: ...
    def set_problem_name(self, name: str) -> None: ...
    def set_sense(self, sense: SenseInput) -> None: ...
