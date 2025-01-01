# Rust LP File Parser and Diff tool

[![Cargo Test](https://github.com/dandxy89/congenial-enigma/actions/workflows/cargo_test.yml/badge.svg)](https://github.com/dandxy89/congenial-enigma/actions/workflows/cargo_test.yml)
[![Crates.io](https://img.shields.io/crates/v/lp_parser_rs.svg)](https://crates.io/crates/lp_parser_rs)
[![Documentation](https://docs.rs/lp_parser_rs/badge.svg)](https://docs.rs/lp_parser_rs/)

## Overview

![Logo](resources/carbon.png)

A robust Rust parser for Linear Programming (LP) files, built on the [NOM](https://docs.rs/nom/latest/nom/) parsing framework. This crate provides comprehensive support for parsing and analysing LP files according to major industry specifications.

### Supported Specifications

- [IBM CPLEX v22.1.1](https://www.ibm.com/docs/en/icos/22.1.1?topic=cplex-lp-file-format-algebraic-representation)
- [FICO Xpress](https://www.fico.com/fico-xpress-optimization/docs/dms2020-03/solver/optimizer/HTML/chapter10_sec_section102.html)
- [Gurobi](https://www.gurobi.com/documentation/current/refman/lp_format.html)
- Mosek

## Features

### Core Functionality

- **Problem Definition**
  - Problem name and sense specification
  - Single and multi-objective optimization support
  - Comprehensive constraint handling

- **Variable Support**
  - Integer variables
  - General variables
  - Bounded variables (lower, upper, or both)
  - Free variables
  - Semi-continuous variables
  - Special Ordered Sets (SOS)

### Advanced Features

- **LP File Comparison (`diff` feature)**
  - Compare two LP files to identify:
    - Added elements
    - Removed elements
    - Modified components
  - Useful for model version control and validation

- **Serialization (`serde` feature)**
  - Full serialization support for all model structures
  - Compatible with various data formats
  - Enables integration with other tools and systems

## Quick Start

### Installation

Add to your `Cargo.toml`:

```toml
[dependencies]
lp_parser_rs = "x.y.z"
```

### Basic Usage

Clone and run with a sample file:

```bash
git clone https://github.com/dandxy89/lp_parser_rs.git
# Dissemble a single LP file
cargo run --bin lp_parser --release -- {{ /path/to/your/file.lp }}
# Compare two LP files (enabling the 'diff' feature)
cargo run --bin lp_parser --release --features diff -- {{ /path/to/your/file.lp }} {{ /path/to/your/other/file.lp }}
```

### Enable Optional Features

```toml
[dependencies]
lp_parser_rs = { version = "x.y.z", features = ["serde", "diff"] }
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
