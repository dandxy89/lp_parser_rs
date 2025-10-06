# Rust LP File Parser, Writer, and Diff Tool

[![Cargo Test](https://github.com/dandxy89/congenial-enigma/actions/workflows/cargo_test.yml/badge.svg)](https://github.com/dandxy89/congenial-enigma/actions/workflows/cargo_test.yml)
[![Crates.io](https://img.shields.io/crates/v/lp_parser_rs.svg)](https://crates.io/crates/lp_parser_rs)
[![Documentation](https://docs.rs/lp_parser_rs/badge.svg)](https://docs.rs/lp_parser_rs/)
[![PyPI version](https://badge.fury.io/py/parse-lp.svg)](https://badge.fury.io/py/parse-lp)
[![PyPI Downloads](https://static.pepy.tech/personalized-badge/parse-lp?period=total&units=NONE&left_color=BLACK&right_color=GREEN&left_text=downloads)](https://pepy.tech/projects/parse-lp)

## Overview

A robust Rust library for parsing, modifying, and writing Linear Programming (LP) files. Built on the [NOM](https://docs.rs/nom/latest/nom/) parsing framework, this crate provides comprehensive support for the LP file format with the ability to parse, programmatically modify, and regenerate LP files according to major industry specifications.

### Supported Specifications

- [IBM CPLEX v22.1.1](https://www.ibm.com/docs/en/icos/22.1.1?topic=cplex-lp-file-format-algebraic-representation)
- [FICO Xpress](https://www.fico.com/fico-xpress-optimization/docs/dms2020-03/solver/optimizer/HTML/chapter10_sec_section102.html)
- [Gurobi](https://www.gurobi.com/documentation/current/refman/lp_format.html)
- Mosek

## Features

### Core Functionality

- **Problem Definition**
  - Problem name and sense specification
  - Single and multi-objective optimisation support
  - Comprehensive constraint handling

- **Variable Support**
  - Integer, general, bounded, free, semi-continuous variables

- **LP File Writing and Modification**
  - Generate LP files from parsed problems
  - Modify objectives, constraints, and variables programmatically
  - Round-trip compatibility (parse → modify → write → parse)
  - Maintain proper LP format specifications

### Advanced Features

- **LP File Comparison (`diff` feature)**
  - Identify added, removed, and modified elements
  - Useful for model version control and validation

- **Serialisation (`serde` feature)**
  - Full serialisation support for all model structures
  - Compatible with various data formats
  - Enables integration with other tools and systems

## Quick Start

### Installation

Add to your `Cargo.toml`:

```toml
[dependencies]
lp_parser_rs = "2.4.1"
```

### Basic Usage

Clone and run with a sample file:

```bash
git clone https://github.com/dandxy89/lp_parser_rs.git
# Parse a single LP file
cargo run --bin lp_parser --release -- {{ /path/to/your/file.lp }}
# Compare two LP files (enabling the 'diff' feature)
cargo run --bin lp_parser --release --features diff -- {{ /path/to/your/file.lp }} {{ /path/to/your/other/file.lp }}
```

Using the library directly:

```rust
use lp_parser_rs::{parser::parse_file, problem::LpProblem};
use std::path::Path;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Parse LP file content
    let content = parse_file(Path::new("problem.lp"))?;

    // Parse into LP problem structure
    let problem = LpProblem::parse(&content)?;

    // Access problem components
    println!("Problem name: {:?}", problem.name());
    println!("Objective count: {}", problem.objective_count());
    println!("Constraint count: {}", problem.constraint_count());
    println!("Variable count: {}", problem.variable_count());

    Ok(())
}
```

### LP File Writing and Modification

```rust
use lp_parser_rs::{problem::LpProblem, writer::write_lp_string, model::*};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Parse an existing LP file
    let lp_content = std::fs::read_to_string("problem.lp")?;
    let mut problem = LpProblem::parse(&lp_content)?;

    // Modify objectives
    problem.update_objective_coefficient("profit", "x1", 5.0)?;
    problem.rename_objective("profit", "total_profit")?;

    // Modify constraints
    problem.update_constraint_coefficient("capacity", "x1", 2.0)?;
    problem.update_constraint_rhs("capacity", 200.0)?;
    problem.rename_constraint("capacity", "production_limit")?;

    // Modify variables
    problem.rename_variable("x1", "production_a")?;
    problem.update_variable_type("production_a", VariableType::Integer)?;

    // Write back to LP format
    let modified_lp = write_lp_string(&problem)?;
    std::fs::write("modified_problem.lp", modified_lp)?;

    Ok(())
}
```

### Enable Optional Features

```toml
[dependencies]
lp_parser_rs = { version = "2.4.1", features = ["serde", "diff"] }
```

## API Reference

### Problem Modification Methods

The `LpProblem` struct provides comprehensive methods for modifying LP problems:

#### Objective Modifications
- `update_objective_coefficient(objective_name, variable_name, coefficient)` - Update or add a coefficient in an objective
- `rename_objective(old_name, new_name)` - Rename an objective
- `remove_objective(objective_name)` - Remove an objective

#### Constraint Modifications
- `update_constraint_coefficient(constraint_name, variable_name, coefficient)` - Update or add a coefficient in a constraint
- `update_constraint_rhs(constraint_name, new_rhs)` - Update the right-hand side value
- `rename_constraint(old_name, new_name)` - Rename a constraint
- `remove_constraint(constraint_name)` - Remove a constraint

#### Variable Modifications
- `rename_variable(old_name, new_name)` - Rename a variable across all objectives and constraints
- `update_variable_type(variable_name, new_type)` - Change variable type (Binary, Integer, etc.)
- `remove_variable(variable_name)` - Remove a variable from all objectives and constraints

### Writing LP Files

```rust
use lp_parser::writer::{write_lp_string, write_lp_string_with_options, LpWriterOptions};

// Write with default options
let lp_content = write_lp_string(&problem)?;

// Write with custom options
let options = LpWriterOptions {
    include_problem_name: true,
    max_line_length: 80,
    decimal_precision: 6,
    include_section_spacing: true,
};
let lp_content = write_lp_string_with_options(&problem, &options)?;
```

## Development

### Testing

The project uses snapshot testing via `insta` for reliable test management:

```bash
# Run all tests with all features enabled
cargo insta test --all-features

# Review snapshot changes
cargo insta review
```

## Test Data Sources

The test suite includes data from various open-source projects:

- [Jplex](https://github.com/asbestian/jplex/blob/main/instances/afiro.lp)
- [LPWriter.jl](https://github.com/odow/LPWriter.jl/blob/master/test/model2.lp)
- [Lp-Parser](https://github.com/aphi/Lp-Parser)

## Contributing

Contributions are welcome! Please feel free to submit a Pull Request.
