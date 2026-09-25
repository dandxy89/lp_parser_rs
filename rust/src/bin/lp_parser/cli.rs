use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};

/// Parse, analyse, compare, convert and solve LP and MPS optimisation models
#[derive(Parser)]
#[command(name = "lp_parser")]
#[command(author, version, about, long_about = None)]
#[command(propagate_version = true)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,

    /// Print progress details to stderr
    #[arg(short, long, global = true)]
    pub verbose: bool,

    /// Suppress warnings and status messages on stderr
    #[arg(short, long, global = true, conflicts_with = "verbose")]
    pub quiet: bool,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Parse an LP or MPS file and display its structure
    Parse(ParseArgs),

    /// Show counts of objectives, constraints and variables by type
    Info(InfoArgs),

    /// Report problem statistics and likely modelling issues (invalid bounds,
    /// poor numerical scaling, empty or singleton constraints, unused variables).
    /// Exits 1 when any error-severity issue is found, 2 on failure.
    Analyze(AnalyzeArgs),

    /// Compare two LP or MPS files.
    /// Exits 0 when the problems match, 1 when they differ, 2 on failure.
    #[cfg(feature = "diff")]
    Diff(DiffArgs),

    /// Convert an LP or MPS file to another format
    Convert(ConvertArgs),

    /// Solve a problem with an external solver (CBC or GLPK) via lp-solvers
    #[cfg(feature = "lp-solvers")]
    Solve(SolveArgs),
}

#[derive(ValueEnum, Clone, Debug, Default)]
pub enum OutputFormat {
    /// Plain text output
    #[default]
    Text,
    /// JSON output
    #[cfg(feature = "serde")]
    Json,
    /// YAML output
    #[cfg(feature = "serde")]
    Yaml,
}

#[derive(clap::Args)]
pub struct ParseArgs {
    /// Path to the LP or MPS file (`.mps` extension reads MPS)
    pub file: PathBuf,

    /// Write output to file instead of stdout
    #[arg(short, long)]
    pub output: Option<PathBuf>,

    /// Output format
    #[arg(short, long, value_enum, default_value = "text")]
    pub format: OutputFormat,

    /// Pretty-print structured output (JSON only; YAML is unaffected)
    #[arg(long)]
    pub pretty: bool,
}

#[derive(clap::Args)]
#[allow(clippy::struct_excessive_bools)]
pub struct AnalyzeArgs {
    /// Path to the LP or MPS file (`.mps` extension reads MPS)
    pub file: PathBuf,

    /// Write output to file instead of stdout
    #[arg(short, long)]
    pub output: Option<PathBuf>,

    /// Output format
    #[arg(short, long, value_enum, default_value = "text")]
    pub format: OutputFormat,

    /// Pretty-print structured output (JSON only; YAML is unaffected)
    #[arg(long)]
    pub pretty: bool,

    /// Show only issues/warnings (skip full analysis output)
    #[arg(long)]
    pub issues_only: bool,

    /// Warn about coefficients whose magnitude exceeds this
    #[arg(long, default_value = "1000000000", value_parser = positive_finite)]
    pub large_coeff_threshold: f64,

    /// Warn about non-zero coefficients whose magnitude is below this
    #[arg(long, default_value = "0.000000001", value_parser = positive_finite)]
    pub small_coeff_threshold: f64,

    /// Warn when a right-hand side's magnitude exceeds this
    #[arg(long, default_value = "1000000000", value_parser = positive_finite)]
    pub large_rhs_threshold: f64,

    /// Warn when the largest over the smallest non-zero coefficient magnitude exceeds this
    #[arg(long, default_value = "1000000", value_parser = positive_finite)]
    pub ratio_threshold: f64,
}

/// Parse a threshold that must be a finite number greater than zero.
fn positive_finite(value: &str) -> Result<f64, String> {
    let parsed: f64 = value.parse().map_err(|err| format!("'{value}' is not a number: {err}"))?;
    if parsed.is_finite() && parsed > 0.0 { Ok(parsed) } else { Err(format!("'{value}' must be a finite number greater than zero")) }
}

#[derive(clap::Args)]
#[allow(clippy::struct_excessive_bools)]
pub struct InfoArgs {
    /// Path to the LP or MPS file (`.mps` extension reads MPS)
    pub file: PathBuf,

    /// Write output to file instead of stdout
    #[arg(short, long)]
    pub output: Option<PathBuf>,

    /// Output format
    #[arg(short, long, value_enum, default_value = "text")]
    pub format: OutputFormat,

    /// Pretty-print structured output (JSON only; YAML is unaffected)
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
    /// First LP or MPS file (base)
    pub file1: PathBuf,

    /// Second LP or MPS file (to compare against)
    pub file2: PathBuf,

    /// Write output to file instead of stdout
    #[arg(short, long)]
    pub output: Option<PathBuf>,

    /// Output format
    #[arg(short, long, value_enum, default_value = "text")]
    pub format: OutputFormat,

    /// Pretty-print structured output (JSON only; YAML is unaffected)
    #[arg(long)]
    pub pretty: bool,

    /// Absolute tolerance: numeric differences no larger than this are ignored.
    /// A change is reported only when it exceeds both tolerances
    #[arg(long, default_value_t = 0.0)]
    pub abs_tol: f64,

    /// Relative tolerance: differences no larger than this times max(|a|, |b|) are ignored
    #[arg(long, default_value_t = 0.0)]
    pub rel_tol: f64,

    /// Regex rewrite applied to names in BOTH files before matching.
    /// Takes two values: PATTERN REPLACEMENT. May be repeated; rules apply in order.
    /// Example: --rename '%>%\[\d+,\d+,[^]]*\]$' '%>%idx'
    #[arg(long, num_args = 2, value_names = ["PATTERN", "REPLACEMENT"], action = clap::ArgAction::Append)]
    pub rename: Vec<String>,
}

#[derive(ValueEnum, Clone, Debug, Default)]
pub enum ConvertFormat {
    /// LP file format
    #[default]
    Lp,
    /// MPS file format
    Mps,
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
    /// Path to the LP or MPS file (`.mps` extension reads MPS)
    pub file: PathBuf,

    /// Output file, or directory for CSV (required for CSV, created if missing)
    #[arg(short, long)]
    pub output: Option<PathBuf>,

    /// Target format
    #[arg(short, long, value_enum, default_value = "lp")]
    pub format: ConvertFormat,

    /// Pretty-print output (JSON only; YAML is unaffected)
    #[arg(long)]
    pub pretty: bool,

    /// Round numbers to this many decimal places in LP and MPS output (default: exact, shortest round-trip form)
    #[arg(long)]
    pub precision: Option<usize>,

    /// Line length at which LP expressions wrap (LP output only)
    #[arg(long, default_value = "80")]
    pub max_line_length: usize,

    /// Omit the problem name comment (LP output only)
    #[arg(long)]
    pub no_problem_name: bool,

    /// No blank lines between sections (LP output only)
    #[arg(long)]
    pub compact: bool,
}

#[cfg(feature = "lp-solvers")]
#[derive(ValueEnum, Clone, Debug, Default)]
pub enum Solver {
    /// CBC solver
    #[default]
    Cbc,
    /// GLPK solver
    Glpk,
}

#[cfg(feature = "lp-solvers")]
#[derive(clap::Args)]
pub struct SolveArgs {
    /// Path to the LP or MPS file (`.mps` extension reads MPS)
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

    /// Pretty-print structured output (JSON only; YAML is unaffected)
    #[arg(long)]
    pub pretty: bool,
}

#[cfg(test)]
mod tests {
    use super::positive_finite;

    #[test]
    fn thresholds_must_be_positive_and_finite() {
        assert!(positive_finite("1e9").is_ok());
        assert!(positive_finite("0.5").is_ok());
        for bad in ["0", "-1", "NaN", "inf", "-inf", "abc"] {
            assert!(positive_finite(bad).is_err(), "'{bad}' must be rejected");
        }
    }
}
