# parse_lp

A LP file format parser for Python, powered by Rust.

## Features

- **Complete LP Support**: Handles all standard LP file format features
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
        print(f"  Sense: {constraint['sense']}")
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
        "sos_type": "SOS1",  # or "SOS2"
        "variables": ["var1", "var2", "var3"]
    }
]
```

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
