# parse_lp

[![PyPI version](https://badge.fury.io/py/parse-lp.svg)](https://badge.fury.io/py/parse-lp)

A LP file format parser, writer, and modifier for Python, powered by Rust.

## Features

- **Complete LP Support**: Handles all standard LP file format features
- **Problem Modification**: Programmatically modify objectives, constraints, and variables
- **LP File Writing**: Generate LP files from modified problems with round-trip compatibility
- **Easy Data Access**: Direct access to problem components (variables, constraints, objectives)
- **CSV Export**: Export parsed data to CSV files for further analysis
- **Problem Comparison**: Compare two LP problems to identify differences
- **Type Safety**: Full type hints for better IDE support and development experience

## Installation

```bash
pip install parse_lp
```

## Quick Start

```python
from parse_lp import LpParser

# Parse an LP file
parser = LpParser("path/to/problem.lp")
parser.parse()

# Access problem information
print(f"Problem: {parser.name}")
print(f"Sense: {parser.sense}")
print(f"Variables: {parser.variable_count()}")
print(f"Constraints: {parser.constraint_count()}")

# Modify the problem
parser.update_objective_coefficient("OBJ", "x1", 5.0)
parser.rename_variable("x2", "production")
parser.update_constraint_rhs("C1", 100.0)

# Write back to LP format
modified_lp = parser.to_lp_string()
parser.save_to_file("modified_problem.lp")

# Export to CSV files
parser.to_csv("output_directory/")
```

## Usage Examples

### Basic Parsing and Information

```python
from parse_lp import LpParser

parser = LpParser("optimization_problem.lp")
parser.parse()

# Get problem overview
print(f"Problem Name: {parser.name}")
print(f"Optimization Sense: {parser.sense}")
print(f"Variables: {parser.variable_count()}")
print(f"Constraints: {parser.constraint_count()}")
print(f"Objectives: {parser.objective_count()}")
```

### Accessing Problem Data

```python
# Access objectives
for i, objective in enumerate(parser.objectives):
    print(f"Objective {i+1}: {objective['name']}")
    for coef in objective['coefficients']:
        print(f"  {coef['name']}: {coef['value']}")

# Access variables
for var_name, var_info in parser.variables.items():
    print(f"Variable {var_name}:")
    print(f"  Type: {var_info['var_type']}")
    if 'bounds' in var_info:
        bounds = var_info['bounds']
        if 'lower' in bounds:
            print(f"  Lower bound: {bounds['lower']}")
        if 'upper' in bounds:
            print(f"  Upper bound: {bounds['upper']}")

# Access constraints
for constraint in parser.constraints:
    print(f"Constraint {constraint['name']}:")
    print(f"  Type: {constraint['type']}")
    if constraint['type'] == 'standard':
        print(f"  Operator: {constraint['operator']}")
        print(f"  RHS: {constraint['rhs']}")
        print(f"  Coefficients: {len(constraint['coefficients'])}")
```

### CSV Export

```python
import os

# Create output directory
os.makedirs("output", exist_ok=True)

# Export to CSV files
parser.to_csv("output/")

# Files created:
# - output/variables.csv
# - output/constraints.csv
# - output/objectives.csv
```

### Problem Modification

```python
from parse_lp import LpParser

# Parse an existing LP file
parser = LpParser("optimization_problem.lp")
parser.parse()

# Modify objectives
parser.update_objective_coefficient("profit", "x1", 5.0)
parser.rename_objective("profit", "total_profit")

# Modify constraints
parser.update_constraint_coefficient("capacity", "x1", 2.0)
parser.update_constraint_rhs("capacity", 200.0)
parser.rename_constraint("capacity", "production_limit")

# Modify variables
parser.rename_variable("x1", "production_a")
parser.update_variable_type("production_a", "integer")

# Set problem properties
parser.set_problem_name("Modified Optimization Problem")
parser.set_sense("minimize")

# Write back to LP format
modified_lp_content = parser.to_lp_string()
parser.save_to_file("modified_problem.lp")

# Verify round-trip compatibility
new_parser = LpParser("modified_problem.lp")
new_parser.parse()
print(f"Successfully modified and re-parsed: {new_parser.name}")
```

### Problem Comparison

```python
# Parse two different versions of a problem
parser1 = LpParser("problem_v1.lp")
parser1.parse()

parser2 = LpParser("problem_v2.lp")
parser2.parse()

# Compare the problems
diff = parser1.compare(parser2)

print("Comparison Results:")
print(f"Name changed: {diff['name_changed']}")
print(f"Sense changed: {diff['sense_changed']}")
print(f"Variable count difference: {diff['variable_count_diff']}")
print(f"Added variables: {diff['added_variables']}")
print(f"Removed variables: {diff['removed_variables']}")
print(f"Modified variables: {diff['modified_variables']}")
print(f"Added constraints: {diff['added_constraints']}")
print(f"Removed constraints: {diff['removed_constraints']}")
```

### Objectives Structure

```python
[
    {
        "name": "objective_name",
        "coefficients": [
            {"name": "variable_name", "value": 1.5},
            {"name": "another_var", "value": -2.0}
        ]
    }
]
```

### Variables Structure

```python
{
    "variable_name": {
        "name": "variable_name",
        "var_type": "Continuous",  # or "Binary", "Integer", etc.
        "bounds": {  # Optional
            "lower": 0.0,  # Optional
            "upper": 100.0  # Optional
        }
    }
}
```

### Constraints Structure

```python
[
    {
        "name": "constraint_name",
        "type": "standard",  # or "sos"
        "sense": "LessOrEqual",  # "Equal", "GreaterOrEqual"
        "rhs": 10.0,
        "coefficients": [
            {"name": "variable_name", "value": 2.0}
        ]
    },
    {
        "name": "sos_constraint",
        "type": "sos",
        "sos_type": "S1",  # or "S2"
        "weights": [
            {"name": "var1", "value": 1.0},
            {"name": "var2", "value": 2.0}
        ]
    }
]
```

## Modification Methods

### Objective Methods

- `update_objective_coefficient(obj_name, var_name, coefficient)` - Update or add coefficient
- `rename_objective(old_name, new_name)` - Rename an objective
- `remove_objective(obj_name)` - Remove an objective

### Constraint Methods

- `update_constraint_coefficient(const_name, var_name, coefficient)` - Update or add coefficient
- `update_constraint_rhs(const_name, new_rhs)` - Update right-hand side value
- `rename_constraint(old_name, new_name)` - Rename a constraint
- `remove_constraint(const_name)` - Remove a constraint

### Variable Methods

- `rename_variable(old_name, new_name)` - Rename variable across problem
- `update_variable_type(var_name, var_type)` - Change variable type
- `remove_variable(var_name)` - Remove variable from problem

### Problem Methods

- `set_problem_name(name)` - Set problem name
- `set_sense(sense)` - Set optimization sense ("maximize" or "minimize")

### Writing Methods

- `to_lp_string()` - Generate LP format string
- `to_lp_string_with_options(**options)` - Generate with custom formatting
- `save_to_file(filepath)` - Save to LP file

### Variable Types

Supported variable types for `update_variable_type()`:

- `"binary"` - Binary variables (0 or 1)
- `"integer"` - General integer variables
- `"general"` - Continuous variables (default)
- `"free"` - Free variables (no bounds)
- `"semicontinuous"` - Semi-continuous variables

## Supported LP Format Features

- Multiple objective functions
- Standard constraints (≤, =, ≥)
- Variable bounds
- Variable types (continuous, binary, integer)
- SOS (Special Ordered Sets) constraints
- Problem names and comments
- Scientific notation in coefficients

## License

Licensed under either of Apache License, Version 2.0 or MIT license at your option.

## Contributing

Issues and pull requests are welcome at: <https://github.com/dandxy89/lp_parser_rs>

```bash
make build
make install
make unit-test
```
