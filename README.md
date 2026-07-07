# Rust LP File Parser, Writer, and Diff Tool

[![Cargo Test](https://github.com/dandxy89/lp_parser_rs/actions/workflows/cargo_test.yml/badge.svg)](https://github.com/dandxy89/lp_parser_rs/actions/workflows/cargo_test.yml)
[![Crates.io](https://img.shields.io/crates/v/lp_parser_rs.svg)](https://crates.io/crates/lp_parser_rs)
[![Documentation](https://docs.rs/lp_parser_rs/badge.svg)](https://docs.rs/lp_parser_rs/)
[![PyPI version](https://badge.fury.io/py/parse-lp.svg)](https://badge.fury.io/py/parse-lp)
[![PyPI Downloads](https://static.pepy.tech/personalized-badge/parse-lp?period=total&units=NONE&left_color=BLACK&right_color=GREEN&left_text=downloads)](https://pepy.tech/projects/parse-lp)

A robust Rust library and CLI for parsing, analysing, modifying, and writing Linear Programming (LP) files. Built on [LALRPOP](https://github.com/lalrpop/lalrpop); grammar lives in [`lp.lalrpop`](https://github.com/dandxy89/lp_parser_rs/blob/main/rust/src/lp.lalrpop).

Supported specifications: [IBM CPLEX v22.1.1](https://www.ibm.com/docs/en/icos/22.1.1?topic=cplex-lp-file-format-algebraic-representation), [FICO Xpress](https://www.fico.com/fico-xpress-optimization/docs/dms2020-03/solver/optimizer/HTML/chapter10_sec_section102.html), [Gurobi](https://www.gurobi.com/documentation/current/refman/lp_format.html), Mosek.

## Features

- **Parsing & writing** — round-trip LP files (parse → modify → write → parse) with configurable formatting; MPS files can also be written via [`mps::writer`](https://docs.rs/lp_parser_rs) (`write_mps_string`), letting LP and MPS problems convert to either format
- **Problem modification** — rename / update / remove objectives, constraints, variables, coefficients, and RHS values
- **Variable types** — integer, general, bounded, free, semi-continuous
- **Analysis** — statistics, matrix density, sparsity, coefficient ranges, issue detection with configurable thresholds
- **Diff** (`diff` feature) — structural and numeric comparison between two LP problems, callable from the library via [`LpProblem::diff`](https://docs.rs/lp_parser_rs) or `diff::compare`
- **Serialisation** (`serde` feature) — JSON / YAML support
- **External solvers** (`lp-solvers` feature) — CBC, Gurobi, CPLEX, GLPK via the [lp-solvers](https://crates.io/crates/lp-solvers) crate

## Library Usage

Add to `Cargo.toml`:

```toml
[dependencies]
lp_parser_rs = { version = "4.0.0", features = ["serde", "diff"] } # x-release-please-version
```

Parse and inspect:

```rust
use lp_parser_rs::{parser::parse_file, problem::LpProblem};
use std::path::Path;

let content = parse_file(Path::new("problem.lp"))?;
let problem = LpProblem::parse(&content)?;
println!("{} objectives, {} constraints, {} variables",
    problem.objective_count(), problem.constraint_count(), problem.variable_count());
```

Modify and write:

```rust
use lp_parser_rs::{problem::LpProblem, writer::write_lp_string, model::VariableType};

let mut problem = LpProblem::parse(&std::fs::read_to_string("problem.lp")?)?;

problem.update_objective_coefficient("profit", "x1", 5.0)?;
problem.rename_objective("profit", "total_profit")?;
problem.update_constraint_coefficient("capacity", "x1", 2.0)?;
problem.update_constraint_rhs("capacity", 200.0)?;
problem.rename_variable("x1", "production_a")?;
problem.update_variable_type("production_a", VariableType::Integer)?;

std::fs::write("modified.lp", write_lp_string(&problem))?;
```

Available modification methods on `LpProblem`: `update_objective_coefficient`, `rename_objective`, `remove_objective`, `update_constraint_coefficient`, `update_constraint_rhs`, `rename_constraint`, `remove_constraint`, `rename_variable`, `update_variable_type`, `remove_variable`.

Writer options: `write_lp_string_with_options(&problem, &LpWriterOptions { include_problem_name, max_line_length, decimal_precision, include_section_spacing })`.

## Command-Line Interface (`lp_parser`)

### Install

```bash
cargo install lp_parser_rs --all-features
# Or from source
git clone https://github.com/dandxy89/lp_parser_rs.git
cd lp_parser_rs/rust && cargo build --release --all-features
```

### Global options

| Flag                               | Description                            |
| ---------------------------------- | -------------------------------------- |
| `-v`, `--verbose`                  | Increase output verbosity (repeatable) |
| `-q`, `--quiet`                    | Suppress non-essential output          |
| `-h`, `--help` / `-V`, `--version` | Print help / version                   |

### `parse` — display file structure

| Option                | Default | Description                            |
| --------------------- | ------- | -------------------------------------- |
| `<FILE>`              | —       | Path to the LP file (required)         |
| `-o, --output <PATH>` | stdout  | Write output to file                   |
| `-f, --format <FMT>`  | `text`  | `text`, `json` (serde), `yaml` (serde) |
| `--pretty`            | off     | Pretty-print JSON/YAML                 |

```bash
lp_parser parse problem.lp
lp_parser parse problem.lp --format yaml -o problem.yaml
```

### `info` — summary statistics

Adds to the `parse` options:

| Option          | Description                         |
| --------------- | ----------------------------------- |
| `--variables`   | List all variables with their types |
| `--constraints` | List all constraints                |
| `--objectives`  | List all objectives                 |

```bash
lp_parser info problem.lp
lp_parser info problem.lp --variables --constraints --objectives
lp_parser info problem.lp --format json --pretty
```

### `analyze` — structural analysis & issue detection

Adds to the `parse` options:

| Option                        | Default | Description                                   |
| ----------------------------- | ------- | --------------------------------------------- |
| `--issues-only`               | off     | Skip full analysis; show warnings/errors only |
| `--large-coeff-threshold <F>` | `1e9`   | Warn on coefficients larger than this         |
| `--small-coeff-threshold <F>` | `1e-9`  | Warn on coefficients smaller than this        |
| `--ratio-threshold <F>`       | `1e6`   | Warn on coefficient scaling ratios above this |

```bash
lp_parser analyze problem.lp
lp_parser analyze problem.lp --issues-only
lp_parser analyze problem.lp --large-coeff-threshold 1e8 --ratio-threshold 1e5
lp_parser analyze problem.lp --format yaml -o analysis.yaml
```

<details>
<summary>Example output</summary>

```yaml
summary: { name: diet, sense: Minimize, objective_count: 1, constraint_count: 7, variable_count: 16, density: 0.571 }
variables: { type_distribution: { upper_bounded: 9, double_bounded: 7 }, discrete_variable_count: 0 }
constraints: { type_distribution: { equality: 7 }, rhs_range: { min: 30.0, max: 50000.0 } }
coefficients: { constraint_coeff_range: { min: 0.1, max: 3055.2 }, coefficient_ratio: 101840.0 }
issues: []
```
</details>

### `diff` — compare two LP files (requires `diff` feature)

| Option                      | Default | Description                                                              |
| --------------------------- | ------- | ------------------------------------------------------------------------ |
| `<FILE1> <FILE2>`           | —       | Base and comparison files                                                |
| `-o, --output <PATH>`       | stdout  | Write output to file                                                     |
| `-f, --format <FMT>`        | `text`  | `text`, `json`, `yaml`                                                   |
| `--pretty`                  | off     | Pretty-print structured output                                           |
| `--abs-tol <F>`             | `0.0`   | Absolute tolerance for numeric comparisons                               |
| `--rel-tol <F>`             | `0.0`   | Relative tolerance: `                                                    | a-b | ≤ rel_tol · max( | a | , | b | )` |
| `--rename <PATTERN> <REPL>` | —       | Regex rewrite applied to names in both files before matching; repeatable |

```bash
lp_parser diff old.lp new.lp
lp_parser diff old.lp new.lp --abs-tol 1e-6 --rel-tol 1e-9
lp_parser diff old.lp new.lp --rename '\[\d+\]$' '[N]' --format json --pretty
```

### `convert` — translate to another format

| Option                  | Default | Description                                 |
| ----------------------- | ------- | ------------------------------------------- |
| `<FILE>`                | —       | Path to the LP or MPS file (`.mps` extension reads MPS) |
| `-o, --output <PATH>`   | stdout  | Output file or directory (required for CSV) |
| `-f, --format <FMT>`    | `lp`    | `lp`, `mps`, `csv`, `json`, `yaml`          |
| `--pretty`              | off     | Pretty-print JSON/YAML                      |
| `--precision <N>`       | `6`     | Decimal precision for numbers               |
| `--max-line-length <N>` | `80`    | Line-wrap threshold for LP output            |
| `--no-problem-name`     | off     | Omit problem-name comment in LP output      |
| `--compact`             | off     | No section spacing                          |

```bash
lp_parser convert problem.lp --format lp --precision 4 --compact
lp_parser convert problem.lp --format csv --output ./out     # writes constraints.csv, objectives.csv, variables.csv
lp_parser convert problem.lp --format json --pretty -o problem.json
lp_parser convert problem.lp --format mps -o problem.mps     # LP -> MPS
lp_parser convert problem.mps --format mps -o rewritten.mps  # MPS -> MPS
```

> **Caveat (LP → MPS):** a variable that appears only in constraints and is
> never given an explicit bound defaults to `Free` internally, which is
> written out as an MPS `FR` bound. LP format's own default for such a
> variable is `[0, +inf)`, not free -- so an unbounded-looking variable can
> widen to include negative values once converted to MPS. Declare bounds
> explicitly (even a redundant `x >= 0`) in the LP source if this distinction
> matters to the downstream solver.

### `solve` — run an external solver (requires `lp-solvers` feature)

| Option                | Default | Description                      |
| --------------------- | ------- | -------------------------------- |
| `<FILE>`              | —       | Path to the LP file              |
| `-s, --solver <NAME>` | `cbc`   | `cbc`, `glpk`                    |
| `-o, --output <PATH>` | stdout  | Write solution to file           |
| `-f, --format <FMT>`  | `text`  | `text`, `json`, `yaml`           |
| `--pretty`            | off     | Pretty-print structured output   |

```bash
lp_parser solve problem.lp
lp_parser solve problem.lp --solver glpk
lp_parser solve problem.lp --format json --pretty
```

The selected solver binary must be installed on your `PATH`. The compatibility layer does **not** support multiple objectives (errors), strict inequalities (`<`, `>`), or SOS constraints (ignored with a warning).

## LP Model Explorer and Diff Viewer (`lp_diff`)

A terminal UI for exploring a single LP/MPS model or comparing two, with coefficient-level side-by-side diffs, fuzzy search, filtering, and integrated [HiGHS](https://highs.dev) solving. Built with [ratatui](https://ratatui.rs). Pass one file to inspect a model, or two files to diff them.

![lp_diff demo](https://raw.githubusercontent.com/dandxy89/lp_parser_rs/main/tui/assets/demo.gif)

```bash
cargo install --path tui
lp_diff model.lp                       # inspect a single model
lp_diff base.lp modified.lp            # diff two files
lp_diff model.lp --summary             # non-interactive single-model summary
lp_diff base.lp modified.lp --summary  # non-interactive diff summary
```

#### Tolerance & rename (parity with `lp_parser diff`)

`lp_diff` accepts the same name-rewrite and numeric-tolerance flags as the CLI `diff` command. Active options are shown on the Summary panel (and in `--summary` output) so results are reproducible.

| Option | Default | Description |
|--------|---------|-------------|
| `--abs-tol <F>` | `0.0` | Absolute tolerance for RHS & coefficient comparisons |
| `--rel-tol <F>` | `0.0` | Relative tolerance: `|a-b| ≤ rel_tol · max(|a|,|b|)` |
| `--rename <PATTERN> <REPL>` | — | Regex rewrite applied to names in both files before matching; repeatable |

```bash
# Collapse indexed names (e.g. x[1,2,foo] / x[9,9,baz] → x[idx]) so structural diffs survive renumbering
lp_diff base.lp modified.lp --rename '\[\d+,\d+,[^]]*\]$' '[idx]'

# Hide near-equal RHS/coefficient drift
lp_diff base.lp modified.lp --abs-tol 1e-4 --rel-tol 1e-6

# Combine with --summary for scripting
lp_diff base.lp modified.lp --summary --rename '\[\d+\]$' '[idx]' --abs-tol 1e-6
```

Highlights: three-panel layout, five sections (Summary / Variables / Constraints / Objectives / Numerics), filtering (`a`/`+`/`-`/`m`/`=`, plus `o` to ignore coefficient order), sort cycling (`s`), live tolerance cycling (`t`/`T`), telescope-style search (fuzzy, `r:` regex, `s:` substring), HiGHS solve-and-compare with infeasibility diagnosis (`S`), vim-style navigation and jumplist, clipboard yank (`y`/`Y`), CSV export (`w`), `?` for full help.

See [`tui/README.md`](https://github.com/dandxy89/lp_parser_rs/blob/main/tui/README.md) for the complete reference.

## Development

```bash
cargo insta test --all-features   # run tests
cargo insta review                # review snapshot changes
```

Test data sources: [Jplex](https://github.com/asbestian/jplex/blob/main/instances/afiro.lp), [LPWriter.jl](https://github.com/odow/LPWriter.jl/blob/master/test/model2.lp), [Lp-Parser](https://github.com/aphi/Lp-Parser).

## Contributing

Contributions are welcome — please open a Pull Request.
