# parse_lp

[![PyPI version](https://badge.fury.io/py/parse-lp.svg)](https://badge.fury.io/py/parse-lp)

Read, inspect, edit and write LP and MPS optimisation models from Python. The
parsing and writing are done by the Rust crate
[`lp_parser_rs`](https://github.com/dandxy89/lp_parser_rs).

- Parse LP files (including CPLEX and Gurobi extensions) and MPS files, from disk or from a string.
- Read objectives, constraints and variables as plain dicts and lists.
- Edit coefficients, right-hand sides, names, variable types and the sense, then write the result as LP or MPS.
- Compare two models with `diff`.
- Check a model for numerical and structural problems with `analyze`.
- Export to CSV.

Type stubs are included, so editors and type checkers know the shape of every returned dict.

## Installation

```bash
pip install parse_lp
```

Requires Python 3.9 or later.

## Quick start

```python
from parse_lp import LpParser

parser = LpParser("problem.lp")  # parsed straight away

print(parser.name, parser.sense)  # e.g. None maximize
print(parser.num_variables, parser.num_constraints)

parser.update_objective_coefficient("OBJ", "x1", 5.0)
parser.rename_variable("x2", "production")
parser.update_constraint_rhs("C1", 100.0)

parser.save_to_file("modified.lp")
parser.save_to_mps("modified.mps")
```

## Loading a model

```python
from parse_lp import LpParser

parser = LpParser("problem.lp")  # format from the extension: .mps is MPS, anything else LP
parser = LpParser.from_file("model.txt", format="mps")  # force the format
parser = LpParser.from_string("""
Minimize
 obj: x + 2 y
Subject To
 c1: x + y >= 1
End
""")
```

`from_string` takes `format="mps"` for MPS text. `parser.parse()` re-reads the
source file if it has changed on disk; it is not needed after construction and
raises `LpInvalidValueError` for a parser built from a string.

## Reading the model

`objectives`, `constraints` and `variables` build a fresh copy of the whole
collection on every access, and that copy does not follow later edits. Bind the
result once rather than reading the property in a loop. For a single item use
`get_constraint(name)` or `get_variable(name)`, and for sizes use
`num_objectives`, `num_constraints` and `num_variables`.

```python
c1 = parser.get_constraint("C1")
x1 = parser.get_variable("x1")

for objective in parser.objectives:
    print(objective["name"])
    for coef in objective["coefficients"]:
        print(f"  {coef['name']}: {coef['value']}")

for name, var in parser.variables.items():
    # Kind and bounds are independent: a variable can be Integer and bounded.
    print(name, var["kind"], var["lower"], var["upper"])

for constraint in parser.constraints:
    if constraint["type"] == "standard":
        print(constraint["name"], constraint["operator"], constraint["rhs"])
```

### Returned shapes

An objective:

```python
{
    "name": "OBJ",
    "coefficients": [{"name": "x1", "value": 1.0}, {"name": "x2", "value": 2.0}],
    "quadratic": [],  # [{"var1": "x", "var2": "y", "coefficient": 1.0}, ...]
    "attributes": {"priority": None, "weight": None, "abs_tol": None, "rel_tol": None},
}
```

A variable (values of the `variables` dict):

```python
{
    "name": "x2",
    # "Continuous", "General", "Integer", "Binary", "SemiContinuous", "SemiInteger" or "Sos"
    "kind": "Continuous",
    # None means no bound was declared on that side, so the format default
    # applies (LP: lower 0, upper +inf). A `free` variable reports -inf / inf.
    "lower": 0.0,
    "upper": 2.0,
}
```

Constraints. The `type` key tells you which shape you have:

```python
{
    "name": "C1",
    "type": "standard",
    "coefficients": [{"name": "x1", "value": 1.0}, {"name": "x2", "value": 1.0}],
    "operator": "LTE",  # "GT", "GTE", "EQ", "LT" or "LTE"
    "rhs": 3.0,
    "class": "normal",  # "lazy" or "user_cut" for those CPLEX sections
}
{
    "name": "s1",
    "type": "sos",
    "sos_type": "S1",  # or "S2"
    "weights": [{"name": "x1", "value": 1.0}, {"name": "x2", "value": 2.0}],
}
```

Indicator (`"indicator"`), quadratic (`"quadratic"`) and Gurobi general
(`"general"`) constraints have their own keys; see the `IndicatorConstraint`,
`QuadraticConstraint` and `GeneralConstraint` types in `parse_lp.pyi`.

## Editing the model

```python
parser.update_objective_coefficient("profit", "x1", 5.0)
parser.rename_objective("profit", "total_profit")

parser.update_constraint_coefficient("capacity", "x1", 2.0)
parser.update_constraint_rhs("capacity", 200.0)
parser.rename_constraint("capacity", "production_limit")

parser.rename_variable("x1", "production_a")
parser.update_variable_type("production_a", "integer")

parser.set_problem_name("modified")
parser.set_sense("minimize")
```

| Method | Notes |
| --- | --- |
| `update_objective_coefficient(objective_name, variable_name, coefficient)` | Adds the term if missing; 0 removes it |
| `rename_objective(old_name, new_name)` | |
| `remove_objective(objective_name)` | |
| `update_constraint_coefficient(constraint_name, variable_name, coefficient)` | Adds the term if missing; 0 removes it. Not for SOS or general constraints |
| `update_constraint_rhs(constraint_name, new_rhs)` | Not for SOS or general constraints |
| `rename_constraint(old_name, new_name)` | |
| `remove_constraint(constraint_name)` | |
| `rename_variable(old_name, new_name)` | Renames it everywhere |
| `update_variable_type(variable_name, var_type)` | See below |
| `remove_variable(variable_name)` | Removes every term that uses it |
| `set_problem_name(name)` | |
| `set_sense(sense)` | `"maximize"`/`"max"` or `"minimize"`/`"min"` |

`update_variable_type` accepts, in any case:

- `"continuous"`: changes only the kind and keeps declared bounds.
- `"binary"`, `"integer"`, `"general"`, `"semicontinuous"`, `"semiinteger"`:
  set the kind and clear declared bounds, so the format default applies (LP:
  lower bound 0). `"integer"` and `"general"` are the same.
- `"free"`: a bound rather than a kind. The variable becomes continuous with
  bounds `(-inf, +inf)`.

## Errors

A missing name raises `LpObjectNotFoundError`. An invalid argument, or an
edit the model cannot take (renaming onto an existing name, setting the
right-hand side of an SOS constraint, a non-finite coefficient), raises
`LpInvalidValueError`. Input that does not parse raises `LpParseError`. All
three subclass `RuntimeError`. File-system failures raise the usual `OSError`
subclasses, such as `FileNotFoundError` and `PermissionError`.

```python
from parse_lp import LpObjectNotFoundError

try:
    parser.update_constraint_rhs("no_such_constraint", 1.0)
except LpObjectNotFoundError as err:
    print(err)
```

## Writing

```python
text = parser.to_lp_string()
text = parser.to_lp_string(
    include_problem_name=False,
    max_line_length=120,
    decimal_precision=4,  # rounds, so the output is no longer exact
    include_section_spacing=False,
)
parser.save_to_file("out.lp")  # default formatting

mps = parser.to_mps_string()
parser.save_to_mps("out.mps")
```

MPS holds one objective. For a model with several, `to_mps_string` and
`save_to_mps` raise `LpInvalidValueError` unless you pass
`allow_multiple_objectives=True`, which writes only the first. Strict
inequalities (`<`, `>`) and Gurobi general constraints cannot be written to
MPS.

## Comparing two models

```python
before = LpParser("v1.lp")
after = LpParser("v2.lp")

changes = before.diff(after)
if not changes["is_empty"]:
    print("added:", changes["cons_added"])
    print("removed:", changes["cons_removed"])
    for name, details in changes["cons_modified"]:
        print(name, details)
```

The result also has `sense_changed`, `vars_added`, `vars_removed`,
`vars_type_changed`, `objs_added`, `objs_removed` and `objs_modified`. "Added"
means present only in the argument, "removed" only in the parser `diff` is
called on.

## Analysis

`analyze()` reports statistics and likely problems in a model before you send
it to a solver.

```python
analysis = parser.analyze()

summary = analysis["summary"]
print(summary["variable_count"], summary["constraint_count"], summary["total_nonzeros"])
print(f"density: {summary['density']:.4f}")

sparsity = analysis["sparsity"]
print(sparsity["min_vars_per_constraint"], sparsity["max_vars_per_constraint"])

print(analysis["variables"]["type_distribution"])

coeffs = analysis["coefficients"]
print(coeffs["constraint_coeff_range"])  # {"min": ..., "max": ..., "count": ...}
print(f"ratio: {coeffs['coefficient_ratio']:.2f}")

for issue in analysis["issues"]:
    # severity is "ERROR", "WARNING" or "INFO"
    print(f"[{issue['severity']}] {issue['category']}: {issue['message']}")
    if issue["details"]:
        print(f"  {issue['details']}")
```

The thresholds are keyword-only and must be finite and positive:

```python
analysis = parser.analyze(
    large_coeff_threshold=1e8,  # flag |coefficient| above this
    small_coeff_threshold=1e-10,  # flag |coefficient| below this
    ratio_threshold=1e5,  # flag if max/min |coefficient| exceeds this
    large_rhs_threshold=1e8,  # flag |right-hand side| above this
)
```

Issue categories: invalid bounds (lower > upper), numerical scaling (large or
small coefficients, large right-hand sides, a high coefficient ratio), empty
constraints, unused variables, fixed variables (lower = upper), singleton
constraints, and other warnings such as a model that looks over-constrained.

## CSV export

```python
import os

os.makedirs("output", exist_ok=True)  # the directory must exist
parser.to_csv("output")
# output/objectives.csv, output/constraints.csv, output/variables.csv
```

## Supported format features

LP:

- Minimise and maximise, including multiple objectives and Gurobi multi-objective attributes
- Constraints with `<=`, `>=`, `=`, `<`, `>`
- Bounds, including `free` and infinite bounds
- Integer, general, binary, semi-continuous and semi-integer variables
- SOS1 and SOS2 constraints
- Indicator constraints, quadratic objectives and constraints, Gurobi general constraints
- CPLEX lazy constraints and user cuts
- Problem names and comments, scientific notation

MPS: read as free format (whitespace-separated fields), so fixed-format files
work as long as names contain no spaces. Written through the same API.

## License

Licensed under either of Apache License, Version 2.0 or MIT license at your option.

## Contributing

Issues and pull requests are welcome at <https://github.com/dandxy89/lp_parser_rs>.

```bash
make develop       # build the extension into the local virtualenv
make unit-test     # run pytest
make check-python  # ruff and ty
```
