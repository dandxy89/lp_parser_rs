//! LP Parser CLI - Parse, analyze, convert, and solve Linear Programming files.

mod cli;

use std::io::Write;
use std::path::PathBuf;

use clap::Parser;
#[cfg(feature = "diff")]
use cli::DiffArgs;
use cli::{Cli, Commands, ConvertArgs, ConvertFormat, InfoArgs, OutputFormat, ParseArgs};
#[cfg(feature = "lp-solvers")]
use cli::{SolveArgs, Solver};
use lp_parser_rs::parser::parse_file;
use lp_parser_rs::problem::LpProblem;

fn cmd_parse(args: ParseArgs, verbose: u8, quiet: bool) -> Result<(), Box<dyn std::error::Error>> {
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

fn cmd_info(args: &InfoArgs, verbose: u8, quiet: bool) -> Result<(), Box<dyn std::error::Error>> {
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

fn write_info_text<W: Write>(writer: &mut W, problem: &LpProblem, args: &InfoArgs) -> Result<(), Box<dyn std::error::Error>> {
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
            lp_parser_rs::model::VariableType::Binary => binary_count += 1,
            lp_parser_rs::model::VariableType::Integer => integer_count += 1,
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
        for (name, obj) in &problem.objectives {
            writeln!(writer, "  {name}: {} terms", obj.coefficients.len())?;
        }
    }

    if args.constraints {
        writeln!(writer)?;
        writeln!(writer, "Constraints:")?;
        for (name, constr) in &problem.constraints {
            match constr {
                lp_parser_rs::model::Constraint::Standard { coefficients, operator, rhs, .. } => {
                    writeln!(writer, "  {name}: {} terms {operator} {rhs}", coefficients.len())?;
                }
                lp_parser_rs::model::Constraint::SOS { sos_type, weights, .. } => {
                    writeln!(writer, "  {name}: {sos_type} with {} variables", weights.len())?;
                }
            }
        }
    }

    if args.variables {
        writeln!(writer)?;
        writeln!(writer, "Variables:")?;
        for (name, var) in &problem.variables {
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
            lp_parser_rs::model::VariableType::Binary => binary_count += 1,
            lp_parser_rs::model::VariableType::Integer => integer_count += 1,
            _ => continuous_count += 1,
        }
    }

    let objectives = if args.objectives {
        Some(
            problem
                .objectives
                .iter()
                .map(|(name, obj)| ObjectiveInfo { name: name.to_string(), term_count: obj.coefficients.len() })
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
                .map(|(name, constr)| match constr {
                    lp_parser_rs::model::Constraint::Standard { coefficients, operator, rhs, .. } => ConstraintInfo {
                        name: name.to_string(),
                        constraint_type: "standard".to_string(),
                        details: format!("{} terms {} {}", coefficients.len(), operator, rhs),
                    },
                    lp_parser_rs::model::Constraint::SOS { sos_type, weights, .. } => ConstraintInfo {
                        name: name.to_string(),
                        constraint_type: "sos".to_string(),
                        details: format!("{} with {} variables", sos_type, weights.len()),
                    },
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
                .map(|(name, var)| VariableInfo { name: (*name).to_string(), var_type: format!("{:?}", var.var_type) })
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

#[cfg(feature = "diff")]
fn cmd_diff(args: DiffArgs, verbose: u8, quiet: bool) -> Result<(), Box<dyn std::error::Error>> {
    use diff::Diff;
    use lp_parser_rs::problem::LpProblemDiff;

    if !quiet && verbose > 0 {
        eprintln!("Comparing {} to {}", args.file1.display(), args.file2.display());
    }

    let content1 = parse_file(&args.file1)?;
    let problem1 = LpProblem::parse(&content1)?;

    let content2 = parse_file(&args.file2)?;
    let problem2 = LpProblem::parse(&content2)?;

    let difference: LpProblemDiff = problem1.diff(&problem2);

    let mut writer = OutputWriter::new(args.output)?;

    // Note: LpProblemDiff doesn't implement Serialize, so we only support text output
    // or build our own serializable representation
    match args.format {
        OutputFormat::Text => {
            write_diff_text(&mut writer, &difference, &problem2)?;
        }
        #[cfg(feature = "serde")]
        OutputFormat::Json | OutputFormat::Yaml => {
            let diff_output = build_diff_output(&difference, &problem2);
            match args.format {
                OutputFormat::Json => {
                    if args.pretty {
                        serde_json::to_writer_pretty(&mut writer, &diff_output)?;
                    } else {
                        serde_json::to_writer(&mut writer, &diff_output)?;
                    }
                    writeln!(writer)?;
                }
                OutputFormat::Yaml => {
                    serde_yaml::to_writer(&mut writer, &diff_output)?;
                }
                OutputFormat::Text => unreachable!(),
            }
        }
    }

    Ok(())
}

#[cfg(all(feature = "diff", feature = "serde"))]
#[derive(serde::Serialize)]
struct DiffOutput {
    variables_changed: Vec<String>,
    variables_removed: Vec<String>,
    constraints_changed: Vec<String>,
    constraints_removed: Vec<String>,
    objectives_changed: Vec<String>,
    objectives_removed: Vec<String>,
}

#[cfg(all(feature = "diff", feature = "serde"))]
fn build_diff_output(difference: &lp_parser_rs::problem::LpProblemDiff, problem2: &LpProblem) -> DiffOutput {
    use lp_parser_rs::model::{ConstraintDiff, VariableTypeDiff};

    let variables_changed: Vec<String> = difference
        .variables
        .altered
        .iter()
        .filter(|(_, v)| !matches!(v.var_type, VariableTypeDiff::NoChange))
        .filter_map(|(k, _)| problem2.variables.get(&**k).map(|_| (*k).to_string()))
        .collect();

    let variables_removed: Vec<String> = difference.variables.removed.iter().map(|k| (*k).to_string()).collect();

    let constraints_changed: Vec<String> = difference
        .constraints
        .altered
        .iter()
        .filter(|(_, v)| !matches!(v, ConstraintDiff::NoChange))
        .filter_map(|(k, _)| problem2.constraints.get(k).map(|_| k.to_string()))
        .collect();

    let constraints_removed: Vec<String> = difference.constraints.removed.iter().map(std::string::ToString::to_string).collect();

    let objectives_changed: Vec<String> =
        difference.objectives.altered.iter().filter_map(|(k, _)| problem2.objectives.get(k).map(|_| k.to_string())).collect();

    let objectives_removed: Vec<String> = difference.objectives.removed.iter().map(std::string::ToString::to_string).collect();

    DiffOutput { variables_changed, variables_removed, constraints_changed, constraints_removed, objectives_changed, objectives_removed }
}

#[cfg(feature = "diff")]
fn write_diff_text<W: Write>(
    writer: &mut W,
    difference: &lp_parser_rs::problem::LpProblemDiff,
    problem2: &LpProblem,
) -> Result<(), Box<dyn std::error::Error>> {
    use lp_parser_rs::model::{ConstraintDiff, VariableTypeDiff};

    let mut has_changes = false;

    // Variables altered
    for (k, v) in &difference.variables.altered {
        if !matches!(v.var_type, VariableTypeDiff::NoChange) {
            if let Some(v_name) = problem2.variables.get(&**k) {
                writeln!(writer, "Variable {k} changed: {v:?} -> {v_name:?}")?;
                has_changes = true;
            }
        }
    }

    // Variables removed
    for k in &difference.variables.removed {
        writeln!(writer, "Variable {k} removed")?;
        has_changes = true;
    }

    // Constraints altered
    for (k, v) in &difference.constraints.altered {
        if !matches!(v, ConstraintDiff::NoChange) {
            if let Some(c_name) = problem2.constraints.get(k) {
                writeln!(writer, "Constraint {k} changed: {v:?} -> {c_name:?}")?;
                has_changes = true;
            }
        }
    }

    // Constraints removed
    for k in &difference.constraints.removed {
        writeln!(writer, "Constraint {k} removed")?;
        has_changes = true;
    }

    // Objectives altered
    for (k, v) in &difference.objectives.altered {
        if let Some(o_name) = problem2.objectives.get(k) {
            writeln!(writer, "Objective {k} changed: {v:?} -> {o_name:?}")?;
            has_changes = true;
        }
    }

    // Objectives removed
    for k in &difference.objectives.removed {
        writeln!(writer, "Objective {k} removed")?;
        has_changes = true;
    }

    if !has_changes {
        writeln!(writer, "No differences found")?;
    }

    Ok(())
}

fn cmd_convert(args: ConvertArgs, verbose: u8, quiet: bool) -> Result<(), Box<dyn std::error::Error>> {
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
                std::fs::create_dir_all(&dir)?;
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
fn cmd_solve(args: SolveArgs, verbose: u8, quiet: bool) -> Result<(), Box<dyn std::error::Error>> {
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
                Status::Infeasible | Status::Unbounded | Status::NotSolved => {}
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
    Stdout(std::io::Stdout),
    File(std::fs::File),
}

impl OutputWriter {
    fn new(path: Option<PathBuf>) -> std::io::Result<Self> {
        match path {
            Some(p) => Ok(Self::File(std::fs::File::create(p)?)),
            None => Ok(Self::Stdout(std::io::stdout())),
        }
    }
}

impl Write for OutputWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match self {
            Self::Stdout(s) => s.write(buf),
            Self::File(f) => f.write(buf),
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        match self {
            Self::Stdout(s) => s.flush(),
            Self::File(f) => f.flush(),
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Parse(args) => cmd_parse(args, cli.verbose, cli.quiet),
        Commands::Info(args) => cmd_info(&args, cli.verbose, cli.quiet),
        #[cfg(feature = "diff")]
        Commands::Diff(args) => cmd_diff(args, cli.verbose, cli.quiet),
        Commands::Convert(args) => cmd_convert(args, cli.verbose, cli.quiet),
        #[cfg(feature = "lp-solvers")]
        Commands::Solve(args) => cmd_solve(args, cli.verbose, cli.quiet),
    }
}
