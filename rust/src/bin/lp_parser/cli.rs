//! CLI argument definitions for `lp_parser`.

use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};

/// LP Parser - Parse, analyze, convert, and solve Linear Programming files
#[derive(Parser)]
#[command(name = "lp_parser")]
#[command(author, version, about, long_about = None)]
#[command(propagate_version = true)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,

    /// Increase output verbosity
    #[arg(short, long, global = true, action = clap::ArgAction::Count)]
    pub verbose: u8,

    /// Suppress non-essential output
    #[arg(short, long, global = true)]
    pub quiet: bool,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Parse an LP file and display its structure
    Parse(ParseArgs),

    /// Show detailed statistics about an LP problem
    Info(InfoArgs),

    /// Compare two LP files
    #[cfg(feature = "diff")]
    Diff(DiffArgs),

    /// Convert LP file to another format
    Convert(ConvertArgs),

    /// Solve an LP problem using external solvers
    #[cfg(feature = "lp-solvers")]
    Solve(SolveArgs),
}

#[derive(ValueEnum, Clone, Debug, Default)]
pub enum OutputFormat {
    /// Plain text output
    #[default]
    Text,
    /// JSON output (requires 'serde' feature)
    #[cfg(feature = "serde")]
    Json,
    /// YAML output (requires 'serde' feature)
    #[cfg(feature = "serde")]
    Yaml,
}

#[derive(clap::Args)]
pub struct ParseArgs {
    /// Path to the LP file
    pub file: PathBuf,

    /// Write output to file instead of stdout
    #[arg(short, long)]
    pub output: Option<PathBuf>,

    /// Output format
    #[arg(short, long, value_enum, default_value = "text")]
    pub format: OutputFormat,

    /// Pretty-print structured output (for JSON/YAML)
    #[arg(long)]
    pub pretty: bool,
}

#[derive(clap::Args)]
#[allow(clippy::struct_excessive_bools)]
pub struct InfoArgs {
    /// Path to the LP file
    pub file: PathBuf,

    /// Write output to file instead of stdout
    #[arg(short, long)]
    pub output: Option<PathBuf>,

    /// Output format
    #[arg(short, long, value_enum, default_value = "text")]
    pub format: OutputFormat,

    /// Pretty-print structured output
    #[arg(long)]
    pub pretty: bool,

    /// List all variables with their types
    #[arg(long)]
    pub variables: bool,

    /// List all constraints
    #[arg(long)]
    pub constraints: bool,

    /// List all objectives
    #[arg(long)]
    pub objectives: bool,
}

#[cfg(feature = "diff")]
#[derive(clap::Args)]
pub struct DiffArgs {
    /// First LP file (base)
    pub file1: PathBuf,

    /// Second LP file (to compare against)
    pub file2: PathBuf,

    /// Write output to file instead of stdout
    #[arg(short, long)]
    pub output: Option<PathBuf>,

    /// Output format
    #[arg(short, long, value_enum, default_value = "text")]
    pub format: OutputFormat,

    /// Pretty-print structured output
    #[arg(long)]
    pub pretty: bool,
}

#[derive(ValueEnum, Clone, Debug, Default)]
pub enum ConvertFormat {
    /// LP file format
    #[default]
    Lp,
    /// CSV files (constraints.csv, objectives.csv, variables.csv)
    #[cfg(feature = "csv")]
    Csv,
    /// JSON format
    #[cfg(feature = "serde")]
    Json,
    /// YAML format
    #[cfg(feature = "serde")]
    Yaml,
}

#[derive(clap::Args)]
pub struct ConvertArgs {
    /// Path to the LP file
    pub file: PathBuf,

    /// Output file or directory (required for CSV)
    #[arg(short, long)]
    pub output: Option<PathBuf>,

    /// Target format
    #[arg(short, long, value_enum, default_value = "lp")]
    pub format: ConvertFormat,

    /// Pretty-print output (for JSON/YAML)
    #[arg(long)]
    pub pretty: bool,

    /// Decimal precision for numbers
    #[arg(long, default_value = "6")]
    pub precision: usize,

    /// Maximum line length before wrapping
    #[arg(long, default_value = "80")]
    pub max_line_length: usize,

    /// Omit problem name comment in LP output
    #[arg(long)]
    pub no_problem_name: bool,

    /// Compact output (no spacing between sections)
    #[arg(long)]
    pub compact: bool,
}

#[cfg(feature = "lp-solvers")]
#[derive(ValueEnum, Clone, Debug, Default)]
pub enum Solver {
    /// CBC solver
    #[default]
    Cbc,
    /// Gurobi solver
    Gurobi,
    /// CPLEX solver
    Cplex,
    /// GLPK solver
    Glpk,
}

#[cfg(feature = "lp-solvers")]
#[derive(clap::Args)]
pub struct SolveArgs {
    /// Path to the LP file
    pub file: PathBuf,

    /// Solver to use
    #[arg(short, long, value_enum, default_value = "cbc")]
    pub solver: Solver,

    /// Write solution to file instead of stdout
    #[arg(short, long)]
    pub output: Option<PathBuf>,

    /// Output format
    #[arg(short, long, value_enum, default_value = "text")]
    pub format: OutputFormat,

    /// Pretty-print structured output
    #[arg(long)]
    pub pretty: bool,
}
