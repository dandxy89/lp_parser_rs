//! LP Parser CLI - Parse, analyze, convert, and solve Linear Programming files.

mod cli;

use std::fs;
use std::io::{self, Stdout, Write};
use std::path::PathBuf;

use clap::Parser;
#[cfg(feature = "diff")]
use cli::DiffArgs;
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

#[cfg(feature = "diff")]
fn cmd_diff(args: DiffArgs, verbose: u8, quiet: bool) -> Result<(), BoxError> {
    use std::collections::{BTreeMap, BTreeSet, HashMap};

    use lp_parser_rs::interner::NameId;
    use lp_parser_rs::model::{Coefficient, Constraint};

    // Parse rename rules
    if args.rename.len() % 2 != 0 {
        return Err("--rename requires pairs of PATTERN REPLACEMENT".into());
    }
    let rules: Vec<(regex::Regex, String)> =
        args.rename.chunks_exact(2).map(|c| Ok::<_, BoxError>((regex::Regex::new(&c[0])?, c[1].clone()))).collect::<Result<_, _>>()?;

    let rewrite = |name: &str| -> String {
        let mut s = name.to_string();
        for (re, rep) in &rules {
            s = re.replace_all(&s, rep.as_str()).into_owned();
        }
        s
    };

    let abs_tol = args.abs_tol;
    let rel_tol = args.rel_tol;
    let differs = |a: f64, b: f64| -> bool {
        let diff = (a - b).abs();
        if diff == 0.0 {
            return false;
        }
        let scale = a.abs().max(b.abs());
        diff > abs_tol && diff > rel_tol * scale
    };

    if !quiet && verbose > 0 {
        eprintln!("Diffing {} vs {}", args.file1.display(), args.file2.display());
        eprintln!("abs_tol={abs_tol} rel_tol={rel_tol} rename_rules={}", rules.len());
    }

    let content1 = parse_file(&args.file1)?;
    let content2 = parse_file(&args.file2)?;
    let p1 = LpProblem::parse(&content1)?;
    let p2 = LpProblem::parse(&content2)?;

    // --- build canonical-name -> NameId maps --------------------------------
    let canon_ids = |problem: &LpProblem, ids: &[NameId]| -> HashMap<String, NameId> {
        ids.iter().map(|id| (rewrite(problem.resolve(*id)), *id)).collect()
    };

    let cvars1 = canon_ids(&p1, &p1.variables.keys().copied().collect::<Vec<_>>());
    let cvars2 = canon_ids(&p2, &p2.variables.keys().copied().collect::<Vec<_>>());
    let ccons1: HashMap<String, NameId> = p1.constraints.values().map(|c| (rewrite(p1.resolve(c.name())), c.name())).collect();
    let ccons2: HashMap<String, NameId> = p2.constraints.values().map(|c| (rewrite(p2.resolve(c.name())), c.name())).collect();
    let cobjs1 = canon_ids(&p1, &p1.objectives.keys().copied().collect::<Vec<_>>());
    let cobjs2 = canon_ids(&p2, &p2.objectives.keys().copied().collect::<Vec<_>>());

    let set_of = |m: &HashMap<String, NameId>| -> BTreeSet<String> { m.keys().cloned().collect() };
    let vars1 = set_of(&cvars1);
    let vars2 = set_of(&cvars2);
    let cons1 = set_of(&ccons1);
    let cons2 = set_of(&ccons2);
    let objs1 = set_of(&cobjs1);
    let objs2 = set_of(&cobjs2);

    let vars_added: Vec<String> = vars2.difference(&vars1).cloned().collect();
    let vars_removed: Vec<String> = vars1.difference(&vars2).cloned().collect();
    let cons_added: Vec<String> = cons2.difference(&cons1).cloned().collect();
    let cons_removed: Vec<String> = cons1.difference(&cons2).cloned().collect();
    let objs_added: Vec<String> = objs2.difference(&objs1).cloned().collect();
    let objs_removed: Vec<String> = objs1.difference(&objs2).cloned().collect();

    // Coefficient map keyed by canonical variable name.
    let coeff_map = |problem: &LpProblem, coeffs: &[Coefficient]| -> BTreeMap<String, f64> {
        coeffs.iter().map(|c| (rewrite(problem.resolve(c.name)), c.value)).collect()
    };

    // Count coefficients that changed value, were removed, or were added.
    let count_coeff_diffs = |m1: &BTreeMap<String, f64>, m2: &BTreeMap<String, f64>| -> usize {
        let mut diffs = 0usize;
        for (k, v1) in m1 {
            match m2.get(k) {
                Some(v2) if differs(*v1, *v2) => diffs += 1,
                None => diffs += 1,
                _ => {}
            }
        }
        diffs += m2.keys().filter(|k| !m1.contains_key(*k)).count();
        diffs
    };

    let mut cons_modified: Vec<(String, Vec<String>)> = Vec::new();
    for name in cons1.intersection(&cons2) {
        let c1 = &p1.constraints[&ccons1[name]];
        let c2 = &p2.constraints[&ccons2[name]];
        let mut changes = Vec::new();
        match (c1, c2) {
            (
                Constraint::Standard { coefficients: cf1, operator: op1, rhs: r1, .. },
                Constraint::Standard { coefficients: cf2, operator: op2, rhs: r2, .. },
            ) => {
                if op1 != op2 {
                    changes.push(format!("operator {op1} -> {op2}"));
                }
                if differs(*r1, *r2) {
                    changes.push(format!("rhs {r1} -> {r2}"));
                }
                let m1 = coeff_map(&p1, cf1);
                let m2 = coeff_map(&p2, cf2);
                let coef_diffs = count_coeff_diffs(&m1, &m2);
                if coef_diffs > 0 {
                    changes.push(format!("{coef_diffs} coefficient change(s)"));
                }
            }
            (Constraint::SOS { .. }, Constraint::SOS { .. }) => {
                if c1 != c2 {
                    changes.push("SOS definition changed".to_string());
                }
            }
            _ => changes.push("constraint kind changed (Standard <-> SOS)".to_string()),
        }
        if !changes.is_empty() {
            cons_modified.push((name.clone(), changes));
        }
    }

    let mut objs_modified: Vec<(String, Vec<String>)> = Vec::new();
    for name in objs1.intersection(&objs2) {
        let o1 = &p1.objectives[&cobjs1[name]];
        let o2 = &p2.objectives[&cobjs2[name]];
        let m1 = coeff_map(&p1, &o1.coefficients);
        let m2 = coeff_map(&p2, &o2.coefficients);
        let coef_diffs = count_coeff_diffs(&m1, &m2);
        if coef_diffs > 0 {
            objs_modified.push((name.clone(), vec![format!("{coef_diffs} coefficient change(s)")]));
        }
    }

    let mut vars_type_changed: Vec<(String, String, String)> = Vec::new();
    for name in vars1.intersection(&vars2) {
        let t1 = &p1.variables[&cvars1[name]].var_type;
        let t2 = &p2.variables[&cvars2[name]].var_type;
        if t1 != t2 {
            vars_type_changed.push((name.clone(), format!("{t1:?}"), format!("{t2:?}")));
        }
    }

    let mut writer = OutputWriter::new(args.output)?;
    match args.format {
        OutputFormat::Text => {
            writeln!(writer, "=== LP Diff ===")?;
            writeln!(writer, "file1: {}", args.file1.display())?;
            writeln!(writer, "file2: {}", args.file2.display())?;
            writeln!(writer, "abs_tol: {abs_tol}  rel_tol: {rel_tol}  rename_rules: {}", rules.len())?;
            writeln!(writer)?;
            writeln!(writer, "Sense: {:?} -> {:?}", p1.sense, p2.sense)?;
            writeln!(
                writer,
                "Counts: objectives {} -> {}, constraints {} -> {}, variables {} -> {}",
                p1.objective_count(),
                p2.objective_count(),
                p1.constraint_count(),
                p2.constraint_count(),
                p1.variable_count(),
                p2.variable_count()
            )?;
            writeln!(writer)?;

            let limit = 50usize;
            let fmt_list = |w: &mut OutputWriter, label: &str, items: &[String]| -> io::Result<()> {
                writeln!(w, "{label} ({}):", items.len())?;
                for name in items.iter().take(limit) {
                    writeln!(w, "  {name}")?;
                }
                if items.len() > limit {
                    writeln!(w, "  ... ({} more)", items.len() - limit)?;
                }
                Ok(())
            };

            fmt_list(&mut writer, "Variables added", &vars_added)?;
            fmt_list(&mut writer, "Variables removed", &vars_removed)?;
            writeln!(writer, "Variables with changed type ({}):", vars_type_changed.len())?;
            for (n, a, b) in vars_type_changed.iter().take(limit) {
                writeln!(writer, "  {n}: {a} -> {b}")?;
            }

            fmt_list(&mut writer, "Constraints added", &cons_added)?;
            fmt_list(&mut writer, "Constraints removed", &cons_removed)?;
            writeln!(writer, "Constraints modified ({}):", cons_modified.len())?;
            for (n, changes) in cons_modified.iter().take(limit) {
                writeln!(writer, "  {n}: {}", changes.join("; "))?;
            }
            if cons_modified.len() > limit {
                writeln!(writer, "  ... ({} more)", cons_modified.len() - limit)?;
            }

            fmt_list(&mut writer, "Objectives added", &objs_added)?;
            fmt_list(&mut writer, "Objectives removed", &objs_removed)?;
            writeln!(writer, "Objectives modified ({}):", objs_modified.len())?;
            for (n, changes) in objs_modified.iter().take(limit) {
                writeln!(writer, "  {n}: {}", changes.join("; "))?;
            }
        }
        #[cfg(feature = "serde")]
        OutputFormat::Json | OutputFormat::Yaml => {
            let summary = serde_json::json!({
                "file1": args.file1.display().to_string(),
                "file2": args.file2.display().to_string(),
                "abs_tol": abs_tol,
                "rel_tol": rel_tol,
                "rename_rule_count": rules.len(),
                "counts": {
                    "objectives": [p1.objective_count(), p2.objective_count()],
                    "constraints": [p1.constraint_count(), p2.constraint_count()],
                    "variables": [p1.variable_count(), p2.variable_count()],
                },
                "variables_added": vars_added,
                "variables_removed": vars_removed,
                "variables_type_changed": vars_type_changed,
                "constraints_added": cons_added,
                "constraints_removed": cons_removed,
                "constraints_modified": cons_modified,
                "objectives_added": objs_added,
                "objectives_removed": objs_removed,
                "objectives_modified": objs_modified,
            });
            match args.format {
                OutputFormat::Json => {
                    if args.pretty {
                        serde_json::to_writer_pretty(&mut writer, &summary)?;
                    } else {
                        serde_json::to_writer(&mut writer, &summary)?;
                    }
                    writeln!(writer)?;
                }
                OutputFormat::Yaml => {
                    serde_yaml::to_writer(&mut writer, &summary)?;
                }
                OutputFormat::Text => unreachable!(),
            }
        }
    }

    Ok(())
}

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

/// Map a solver [`Status`] to its serialised string form.
#[cfg(all(feature = "lp-solvers", feature = "serde"))]
fn solve_status_str(status: lp_solvers::solvers::Status) -> &'static str {
    use lp_solvers::solvers::Status;
    match status {
        Status::Optimal => "optimal",
        Status::SubOptimal => "suboptimal",
        Status::Infeasible => "infeasible",
        Status::Unbounded => "unbounded",
        Status::NotSolved => "not_solved",
        Status::TimeLimit => "time_limit",
        Status::MipGap => "mip_gap",
    }
}

#[cfg(feature = "lp-solvers")]
fn cmd_solve(args: SolveArgs, verbose: u8, quiet: bool) -> Result<(), BoxError> {
    use lp_parser_rs::compat::lp_solvers::LpSolversCompat;
    use lp_solvers::solvers::{CbcSolver, GlpkSolver, SolverTrait, Status};

    let content = parse_file(&args.file)?;
    let problem = LpProblem::parse(&content)?;

    if !quiet && verbose > 0 {
        eprintln!("Loading problem: {}", args.file.display());
    }

    let compat = LpSolversCompat::try_new(&problem)?;

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
            let solution_json = serde_json::json!({
                "status": solve_status_str(solution.status),
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
            let solution_yaml = serde_json::json!({
                "status": solve_status_str(solution.status),
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
        Commands::Diff(args) => cmd_diff(args, cli.verbose, cli.quiet),
        Commands::Convert(args) => cmd_convert(args, cli.verbose, cli.quiet),
        #[cfg(feature = "lp-solvers")]
        Commands::Solve(args) => cmd_solve(args, cli.verbose, cli.quiet),
    }
}
