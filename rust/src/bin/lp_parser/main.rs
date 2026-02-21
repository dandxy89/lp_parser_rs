//! LP Parser CLI - Parse, analyze, convert, and solve Linear Programming files.

mod cli;

use std::fs;
use std::io::{self, Stdout, Write};
use std::path::PathBuf;

use clap::Parser;
use cli::{AnalyzeArgs, Cli, Commands, ConvertArgs, ConvertFormat, InfoArgs, OutputFormat, ParseArgs};
#[cfg(feature = "lp-solvers")]
use cli::{SolveArgs, Solver};
use lp_parser_rs::analysis::AnalysisConfig;
use lp_parser_rs::model::{Constraint, VariableType};
use lp_parser_rs::parser::parse_file;
use lp_parser_rs::problem::LpProblem;

type BoxError = Box<dyn std::error::Error>;

fn cmd_parse(args: ParseArgs, verbose: u8, quiet: bool) -> Result<(), BoxError> {
    let content = parse_file(&args.file)?;
    let problem = LpProblem::parse(&content)?;

    if !quiet && verbose > 0 {
        eprintln!("Parsed file: {}", args.file.display());
        eprintln!(
            "Problem: {} objectives, {} constraints, {} variables",
            problem.objective_count(),
            problem.constraint_count(),
            problem.variable_count()
        );
    }

    let mut writer = OutputWriter::new(args.output)?;

    match args.format {
        OutputFormat::Text => {
            writeln!(writer, "{problem}")?;
        }
        #[cfg(feature = "serde")]
        OutputFormat::Json => {
            if args.pretty {
                serde_json::to_writer_pretty(&mut writer, &problem)?;
            } else {
                serde_json::to_writer(&mut writer, &problem)?;
            }
            writeln!(writer)?;
        }
        #[cfg(feature = "serde")]
        OutputFormat::Yaml => {
            serde_yaml::to_writer(&mut writer, &problem)?;
        }
    }

    Ok(())
}

fn cmd_info(args: &InfoArgs, verbose: u8, quiet: bool) -> Result<(), BoxError> {
    let content = parse_file(&args.file)?;
    let problem = LpProblem::parse(&content)?;

    if !quiet && verbose > 0 {
        eprintln!("Analyzing file: {}", args.file.display());
    }

    let mut writer = OutputWriter::new(args.output.clone())?;

    match args.format {
        OutputFormat::Text => {
            write_info_text(&mut writer, &problem, args)?;
        }
        #[cfg(feature = "serde")]
        OutputFormat::Json => {
            let info = build_info_struct(&problem, args);
            if args.pretty {
                serde_json::to_writer_pretty(&mut writer, &info)?;
            } else {
                serde_json::to_writer(&mut writer, &info)?;
            }
            writeln!(writer)?;
        }
        #[cfg(feature = "serde")]
        OutputFormat::Yaml => {
            let info = build_info_struct(&problem, args);
            serde_yaml::to_writer(&mut writer, &info)?;
        }
    }

    Ok(())
}

fn cmd_analyze(args: AnalyzeArgs, verbose: u8, quiet: bool) -> Result<(), BoxError> {
    let content = parse_file(&args.file)?;
    let problem = LpProblem::parse(&content)?;

    if !quiet && verbose > 0 {
        eprintln!("Analyzing file: {}", args.file.display());
    }

    let config = AnalysisConfig {
        large_coefficient_threshold: args.large_coeff_threshold,
        small_coefficient_threshold: args.small_coeff_threshold,
        large_rhs_threshold: args.large_coeff_threshold,
        coefficient_ratio_threshold: args.ratio_threshold,
    };

    let analysis = problem.analyze_with_config(&config);

    let mut writer = OutputWriter::new(args.output)?;

    match args.format {
        OutputFormat::Text => {
            if args.issues_only {
                if analysis.issues.is_empty() {
                    writeln!(writer, "No issues detected.")?;
                } else {
                    writeln!(writer, "Issues Found: {}", analysis.issues.len())?;
                    for issue in &analysis.issues {
                        writeln!(writer, "  {issue}")?;
                    }
                }
            } else {
                write!(writer, "{analysis}")?;
            }
        }
        #[cfg(feature = "serde")]
        OutputFormat::Json => {
            if args.issues_only {
                if args.pretty {
                    serde_json::to_writer_pretty(&mut writer, &analysis.issues)?;
                } else {
                    serde_json::to_writer(&mut writer, &analysis.issues)?;
                }
            } else if args.pretty {
                serde_json::to_writer_pretty(&mut writer, &analysis)?;
            } else {
                serde_json::to_writer(&mut writer, &analysis)?;
            }
            writeln!(writer)?;
        }
        #[cfg(feature = "serde")]
        OutputFormat::Yaml => {
            if args.issues_only {
                serde_yaml::to_writer(&mut writer, &analysis.issues)?;
            } else {
                serde_yaml::to_writer(&mut writer, &analysis)?;
            }
        }
    }

    Ok(())
}

fn write_info_text<W: Write>(writer: &mut W, problem: &LpProblem, args: &InfoArgs) -> Result<(), BoxError> {
    writeln!(writer, "=== LP Problem Info ===")?;

    if let Some(name) = problem.name() {
        writeln!(writer, "Name: {name}")?;
    }
    writeln!(writer, "Sense: {}", problem.sense)?;
    writeln!(writer, "Objectives: {}", problem.objective_count())?;
    writeln!(writer, "Constraints: {}", problem.constraint_count())?;
    writeln!(writer, "Variables: {}", problem.variable_count())?;

    // Count variable types
    let mut binary_count = 0;
    let mut integer_count = 0;
    let mut continuous_count = 0;

    for var in problem.variables.values() {
        match var.var_type {
            VariableType::Binary => binary_count += 1,
            VariableType::Integer => integer_count += 1,
            _ => continuous_count += 1,
        }
    }

    writeln!(writer)?;
    writeln!(writer, "Variable Types:")?;
    writeln!(writer, "  Continuous: {continuous_count}")?;
    writeln!(writer, "  Integer: {integer_count}")?;
    writeln!(writer, "  Binary: {binary_count}")?;

    if args.objectives {
        writeln!(writer)?;
        writeln!(writer, "Objectives:")?;
        for (name_id, obj) in &problem.objectives {
            let name = problem.resolve(*name_id);
            writeln!(writer, "  {name}: {} terms", obj.coefficients.len())?;
        }
    }

    if args.constraints {
        writeln!(writer)?;
        writeln!(writer, "Constraints:")?;
        for (name_id, constr) in &problem.constraints {
            let name = problem.resolve(*name_id);
            match constr {
                Constraint::Standard { coefficients, operator, rhs, .. } => {
                    writeln!(writer, "  {name}: {} terms {operator} {rhs}", coefficients.len())?;
                }
                Constraint::SOS { sos_type, weights, .. } => {
                    writeln!(writer, "  {name}: {sos_type} with {} variables", weights.len())?;
                }
            }
        }
    }

    if args.variables {
        writeln!(writer)?;
        writeln!(writer, "Variables:")?;
        for (name_id, var) in &problem.variables {
            let name = problem.resolve(*name_id);
            writeln!(writer, "  {name}: {:?}", var.var_type)?;
        }
    }

    Ok(())
}

#[cfg(feature = "serde")]
#[derive(serde::Serialize)]
struct ProblemInfo {
    name: Option<String>,
    sense: String,
    objective_count: usize,
    constraint_count: usize,
    variable_count: usize,
    variable_types: VariableTypeCounts,
    #[serde(skip_serializing_if = "Option::is_none")]
    objectives: Option<Vec<ObjectiveInfo>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    constraints: Option<Vec<ConstraintInfo>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    variables: Option<Vec<VariableInfo>>,
}

#[cfg(feature = "serde")]
#[derive(serde::Serialize)]
struct VariableTypeCounts {
    continuous: usize,
    integer: usize,
    binary: usize,
}

#[cfg(feature = "serde")]
#[derive(serde::Serialize)]
struct ObjectiveInfo {
    name: String,
    term_count: usize,
}

#[cfg(feature = "serde")]
#[derive(serde::Serialize)]
struct ConstraintInfo {
    name: String,
    constraint_type: String,
    details: String,
}

#[cfg(feature = "serde")]
#[derive(serde::Serialize)]
struct VariableInfo {
    name: String,
    var_type: String,
}

#[cfg(feature = "serde")]
fn build_info_struct(problem: &LpProblem, args: &InfoArgs) -> ProblemInfo {
    let mut binary_count = 0;
    let mut integer_count = 0;
    let mut continuous_count = 0;

    for var in problem.variables.values() {
        match var.var_type {
            VariableType::Binary => binary_count += 1,
            VariableType::Integer => integer_count += 1,
            _ => continuous_count += 1,
        }
    }

    let objectives = if args.objectives {
        Some(
            problem
                .objectives
                .iter()
                .map(|(name_id, obj)| ObjectiveInfo { name: problem.resolve(*name_id).to_string(), term_count: obj.coefficients.len() })
                .collect(),
        )
    } else {
        None
    };

    let constraints = if args.constraints {
        Some(
            problem
                .constraints
                .iter()
                .map(|(name_id, constr)| {
                    let name = problem.resolve(*name_id).to_string();
                    match constr {
                        Constraint::Standard { coefficients, operator, rhs, .. } => ConstraintInfo {
                            name,
                            constraint_type: "standard".to_string(),
                            details: format!("{} terms {} {}", coefficients.len(), operator, rhs),
                        },
                        Constraint::SOS { sos_type, weights, .. } => ConstraintInfo {
                            name,
                            constraint_type: "sos".to_string(),
                            details: format!("{} with {} variables", sos_type, weights.len()),
                        },
                    }
                })
                .collect(),
        )
    } else {
        None
    };

    let variables = if args.variables {
        Some(
            problem
                .variables
                .iter()
                .map(|(name_id, var)| VariableInfo { name: problem.resolve(*name_id).to_string(), var_type: format!("{:?}", var.var_type) })
                .collect(),
        )
    } else {
        None
    };

    ProblemInfo {
        name: problem.name().map(String::from),
        sense: format!("{}", problem.sense),
        objective_count: problem.objective_count(),
        constraint_count: problem.constraint_count(),
        variable_count: problem.variable_count(),
        variable_types: VariableTypeCounts { continuous: continuous_count, integer: integer_count, binary: binary_count },
        objectives,
        constraints,
        variables,
    }
}

// Note: diff feature is temporarily disabled pending Phase 4 NameId serde/diff support
// #[cfg(feature = "diff")]
// fn cmd_diff(...) { ... }

// Note: diff feature temporarily disabled pending Phase 4 NameId diff support

fn cmd_convert(args: ConvertArgs, verbose: u8, quiet: bool) -> Result<(), BoxError> {
    use lp_parser_rs::writer::{LpWriterOptions, write_lp_string_with_options};

    let content = parse_file(&args.file)?;
    let problem = LpProblem::parse(&content)?;

    if !quiet && verbose > 0 {
        eprintln!("Converting file: {}", args.file.display());
    }

    match args.format {
        ConvertFormat::Lp => {
            let options = LpWriterOptions {
                include_problem_name: !args.no_problem_name,
                max_line_length: args.max_line_length,
                decimal_precision: args.precision,
                include_section_spacing: !args.compact,
            };
            let output = write_lp_string_with_options(&problem, &options)?;

            let mut writer = OutputWriter::new(args.output)?;
            write!(writer, "{output}")?;
        }
        #[cfg(feature = "csv")]
        ConvertFormat::Csv => {
            use lp_parser_rs::csv::LpCsvWriter;

            let dir = args.output.ok_or("CSV output requires --output directory")?;
            if !dir.exists() {
                fs::create_dir_all(&dir)?;
            }
            problem.to_csv(&dir)?;

            if !quiet {
                eprintln!("CSV files written to: {}", dir.display());
            }
        }
        #[cfg(feature = "serde")]
        ConvertFormat::Json => {
            let mut writer = OutputWriter::new(args.output)?;
            if args.pretty {
                serde_json::to_writer_pretty(&mut writer, &problem)?;
            } else {
                serde_json::to_writer(&mut writer, &problem)?;
            }
            writeln!(writer)?;
        }
        #[cfg(feature = "serde")]
        ConvertFormat::Yaml => {
            let mut writer = OutputWriter::new(args.output)?;
            serde_yaml::to_writer(&mut writer, &problem)?;
        }
    }

    Ok(())
}

#[cfg(feature = "lp-solvers")]
fn cmd_solve(args: SolveArgs, verbose: u8, quiet: bool) -> Result<(), BoxError> {
    use lp_parser_rs::compat::lp_solvers::ToLpSolvers;
    use lp_solvers::solvers::{CbcSolver, GlpkSolver, SolverTrait, Status};

    let content = parse_file(&args.file)?;
    let problem = LpProblem::parse(&content)?;

    if !quiet && verbose > 0 {
        eprintln!("Loading problem: {}", args.file.display());
    }

    let compat = problem.to_lp_solvers()?;

    // Print warnings
    for warning in compat.warnings() {
        if !quiet {
            eprintln!("Warning: {warning}");
        }
    }

    if !quiet && verbose > 0 {
        eprintln!("Solving with {:?}...", args.solver);
    }

    let solution = match args.solver {
        Solver::Cbc => {
            let solver = CbcSolver::new();
            solver.run(&compat)?
        }
        Solver::Glpk => {
            let solver = GlpkSolver::new();
            solver.run(&compat)?
        }
        Solver::Gurobi | Solver::Cplex => {
            return Err(
                format!("{:?} solver requires commercial license - use 'cbc' or 'glpk' for open-source alternatives", args.solver).into()
            );
        }
    };

    let mut writer = OutputWriter::new(args.output)?;

    match args.format {
        OutputFormat::Text => {
            writeln!(writer, "=== Solution ===")?;
            writeln!(writer, "Status: {:?}", solution.status)?;
            match solution.status {
                Status::Optimal | Status::SubOptimal => {
                    writeln!(writer)?;
                    writeln!(writer, "Variables:")?;
                    for (name, value) in &solution.results {
                        writeln!(writer, "  {name} = {value}")?;
                    }
                }
                Status::TimeLimit | Status::MipGap | Status::Infeasible | Status::Unbounded | Status::NotSolved => {}
            }
        }
        #[cfg(feature = "serde")]
        OutputFormat::Json => {
            let status_str = match solution.status {
                Status::Optimal => "optimal",
                Status::SubOptimal => "suboptimal",
                Status::Infeasible => "infeasible",
                Status::Unbounded => "unbounded",
                Status::NotSolved => "not_solved",
                Status::TimeLimit => "time_limit",
                Status::MipGap => "mip_gap",
            };
            let solution_json = serde_json::json!({
                "status": status_str,
                "variables": solution.results
            });
            if args.pretty {
                serde_json::to_writer_pretty(&mut writer, &solution_json)?;
            } else {
                serde_json::to_writer(&mut writer, &solution_json)?;
            }
            writeln!(writer)?;
        }
        #[cfg(feature = "serde")]
        OutputFormat::Yaml => {
            let status_str = match solution.status {
                Status::Optimal => "optimal",
                Status::SubOptimal => "suboptimal",
                Status::Infeasible => "infeasible",
                Status::Unbounded => "unbounded",
                Status::NotSolved => "not_solved",
                Status::TimeLimit => "time_limit",
                Status::MipGap => "mip_gap",
            };
            let solution_yaml = serde_json::json!({
                "status": status_str,
                "variables": solution.results
            });
            serde_yaml::to_writer(&mut writer, &solution_yaml)?;
        }
    }

    Ok(())
}

enum OutputWriter {
    Stdout(Stdout),
    File(fs::File),
}

impl OutputWriter {
    fn new(path: Option<PathBuf>) -> io::Result<Self> {
        match path {
            Some(p) => Ok(Self::File(fs::File::create(p)?)),
            None => Ok(Self::Stdout(io::stdout())),
        }
    }
}

impl Write for OutputWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self {
            Self::Stdout(s) => s.write(buf),
            Self::File(f) => f.write(buf),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match self {
            Self::Stdout(s) => s.flush(),
            Self::File(f) => f.flush(),
        }
    }
}

fn main() -> Result<(), BoxError> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Parse(args) => cmd_parse(args, cli.verbose, cli.quiet),
        Commands::Info(args) => cmd_info(&args, cli.verbose, cli.quiet),
        Commands::Analyze(args) => cmd_analyze(args, cli.verbose, cli.quiet),
        #[cfg(feature = "diff")]
        Commands::Diff(_args) => Err("diff feature temporarily disabled pending NameId diff support".into()),
        Commands::Convert(args) => cmd_convert(args, cli.verbose, cli.quiet),
        #[cfg(feature = "lp-solvers")]
        Commands::Solve(args) => cmd_solve(args, cli.verbose, cli.quiet),
    }
}
