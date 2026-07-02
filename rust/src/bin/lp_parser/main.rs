//! LP Parser CLI - Parse, analyze, convert, and solve Linear Programming files.

mod cli;

#[cfg(feature = "diff")]
use std::collections::{BTreeMap, BTreeSet, HashMap};
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
#[cfg(feature = "diff")]
use lp_parser_rs::interner::NameId;
#[cfg(feature = "diff")]
use lp_parser_rs::model::Coefficient;
use lp_parser_rs::model::{Constraint, VariableType};
use lp_parser_rs::parser::parse_file;
use lp_parser_rs::problem::LpProblem;

type BoxError = Box<dyn std::error::Error>;

fn cmd_parse(args: ParseArgs, verbose: bool, quiet: bool) -> Result<(), BoxError> {
    let content = parse_file(&args.file)?;
    let problem = LpProblem::parse(&content)?;

    if !quiet && verbose {
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

fn cmd_info(args: &InfoArgs, verbose: bool, quiet: bool) -> Result<(), BoxError> {
    let content = parse_file(&args.file)?;
    let problem = LpProblem::parse(&content)?;

    if !quiet && verbose {
        eprintln!("Analyzing file: {}", args.file.display());
    }

    let mut writer = OutputWriter::new(args.output.clone())?;

    match args.format {
        OutputFormat::Text => {
            write_info_text(&mut writer, &problem, args)?;
        }
        #[cfg(feature = "serde")]
        OutputFormat::Json => {
            let info = build_info_value(&problem, args);
            if args.pretty {
                serde_json::to_writer_pretty(&mut writer, &info)?;
            } else {
                serde_json::to_writer(&mut writer, &info)?;
            }
            writeln!(writer)?;
        }
        #[cfg(feature = "serde")]
        OutputFormat::Yaml => {
            let info = build_info_value(&problem, args);
            serde_yaml::to_writer(&mut writer, &info)?;
        }
    }

    Ok(())
}

fn cmd_analyze(args: AnalyzeArgs, verbose: bool, quiet: bool) -> Result<(), BoxError> {
    let content = parse_file(&args.file)?;
    let problem = LpProblem::parse(&content)?;

    if !quiet && verbose {
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

/// Count variables by type, returning `(continuous, integer, binary)`.
fn count_variable_types(problem: &LpProblem) -> (usize, usize, usize) {
    let mut continuous = 0;
    let mut integer = 0;
    let mut binary = 0;

    for var in problem.variables.values() {
        match var.var_type {
            VariableType::Binary => binary += 1,
            VariableType::Integer => integer += 1,
            _ => continuous += 1,
        }
    }

    (continuous, integer, binary)
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

    let (continuous_count, integer_count, binary_count) = count_variable_types(problem);

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

/// Build the structured `info` output as a JSON value (also serialised as YAML).
#[cfg(feature = "serde")]
fn build_info_value(problem: &LpProblem, args: &InfoArgs) -> serde_json::Value {
    let (continuous, integer, binary) = count_variable_types(problem);

    let mut info = serde_json::json!({
        "name": problem.name(),
        "sense": problem.sense.to_string(),
        "objective_count": problem.objective_count(),
        "constraint_count": problem.constraint_count(),
        "variable_count": problem.variable_count(),
        "variable_types": { "continuous": continuous, "integer": integer, "binary": binary },
    });

    if args.objectives {
        info["objectives"] = problem
            .objectives
            .iter()
            .map(|(name_id, obj)| serde_json::json!({ "name": problem.resolve(*name_id), "term_count": obj.coefficients.len() }))
            .collect();
    }

    if args.constraints {
        info["constraints"] = problem
            .constraints
            .iter()
            .map(|(name_id, constr)| {
                let (constraint_type, details) = match constr {
                    Constraint::Standard { coefficients, operator, rhs, .. } => {
                        ("standard", format!("{} terms {operator} {rhs}", coefficients.len()))
                    }
                    Constraint::SOS { sos_type, weights, .. } => ("sos", format!("{sos_type} with {} variables", weights.len())),
                };
                serde_json::json!({ "name": problem.resolve(*name_id), "constraint_type": constraint_type, "details": details })
            })
            .collect();
    }

    if args.variables {
        info["variables"] = problem
            .variables
            .iter()
            .map(|(name_id, var)| serde_json::json!({ "name": problem.resolve(*name_id), "var_type": format!("{:?}", var.var_type) }))
            .collect();
    }

    info
}

/// Absolute and relative tolerances for treating two floats as different.
#[cfg(feature = "diff")]
#[derive(Clone, Copy)]
struct DiffTol {
    abs: f64,
    rel: f64,
}

#[cfg(feature = "diff")]
impl DiffTol {
    /// Return true if `a` and `b` differ beyond both tolerances.
    fn differ(self, a: f64, b: f64) -> bool {
        let diff = (a - b).abs();
        if diff == 0.0 {
            return false;
        }
        let scale = a.abs().max(b.abs());
        diff > self.abs && diff > self.rel * scale
    }
}

/// The computed differences between two LP problems, keyed by canonical name.
#[cfg(feature = "diff")]
struct LpDiff {
    vars_added: Vec<String>,
    vars_removed: Vec<String>,
    vars_type_changed: Vec<(String, String, String)>,
    cons_added: Vec<String>,
    cons_removed: Vec<String>,
    cons_modified: Vec<(String, Vec<String>)>,
    objs_added: Vec<String>,
    objs_removed: Vec<String>,
    objs_modified: Vec<(String, Vec<String>)>,
}

/// Apply each rename rule in turn, returning the canonical form of `name`.
#[cfg(feature = "diff")]
fn apply_rename_rules(name: &str, rules: &[(regex::Regex, String)]) -> String {
    let mut s = name.to_string();
    for (re, rep) in rules {
        s = re.replace_all(&s, rep.as_str()).into_owned();
    }
    s
}

/// Build a coefficient map keyed by canonical variable name.
#[cfg(feature = "diff")]
fn coeff_map(problem: &LpProblem, coeffs: &[Coefficient], rules: &[(regex::Regex, String)]) -> BTreeMap<String, f64> {
    coeffs.iter().map(|c| (apply_rename_rules(problem.resolve(c.name), rules), c.value)).collect()
}

/// Count coefficients that changed value, were removed, or were added.
#[cfg(feature = "diff")]
fn count_coeff_diffs(m1: &BTreeMap<String, f64>, m2: &BTreeMap<String, f64>, tol: DiffTol) -> usize {
    let mut diffs = 0usize;
    for (k, v1) in m1 {
        match m2.get(k) {
            Some(v2) if tol.differ(*v1, *v2) => diffs += 1,
            None => diffs += 1,
            _ => {}
        }
    }
    diffs += m2.keys().filter(|k| !m1.contains_key(*k)).count();
    diffs
}

/// Describe how each common constraint changed (operator, rhs, coefficients).
#[cfg(feature = "diff")]
fn diff_modified_constraints(
    p1: &LpProblem,
    p2: &LpProblem,
    ccons1: &HashMap<String, NameId>,
    ccons2: &HashMap<String, NameId>,
    common: &[String],
    rules: &[(regex::Regex, String)],
    tol: DiffTol,
) -> Vec<(String, Vec<String>)> {
    let mut modified = Vec::new();
    for name in common {
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
                if tol.differ(*r1, *r2) {
                    changes.push(format!("rhs {r1} -> {r2}"));
                }
                let coef_diffs = count_coeff_diffs(&coeff_map(p1, cf1, rules), &coeff_map(p2, cf2, rules), tol);
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
            modified.push((name.clone(), changes));
        }
    }
    modified
}

/// Describe how each common objective's coefficients changed.
#[cfg(feature = "diff")]
fn diff_modified_objectives(
    p1: &LpProblem,
    p2: &LpProblem,
    cobjs1: &HashMap<String, NameId>,
    cobjs2: &HashMap<String, NameId>,
    common: &[String],
    rules: &[(regex::Regex, String)],
    tol: DiffTol,
) -> Vec<(String, Vec<String>)> {
    let mut modified = Vec::new();
    for name in common {
        let o1 = &p1.objectives[&cobjs1[name]];
        let o2 = &p2.objectives[&cobjs2[name]];
        let coef_diffs = count_coeff_diffs(&coeff_map(p1, &o1.coefficients, rules), &coeff_map(p2, &o2.coefficients, rules), tol);
        if coef_diffs > 0 {
            modified.push((name.clone(), vec![format!("{coef_diffs} coefficient change(s)")]));
        }
    }
    modified
}

/// Compare two parsed problems, returning the structural and numeric diff.
#[cfg(feature = "diff")]
// The paired 1/2-suffixed bindings are the domain language of a two-file diff.
#[allow(clippy::similar_names)]
fn compute_lp_diff(p1: &LpProblem, p2: &LpProblem, rules: &[(regex::Regex, String)], tol: DiffTol) -> LpDiff {
    let canon = |problem: &LpProblem, ids: Vec<NameId>| -> HashMap<String, NameId> {
        ids.iter().map(|id| (apply_rename_rules(problem.resolve(*id), rules), *id)).collect()
    };

    let cvars1 = canon(p1, p1.variables.keys().copied().collect());
    let cvars2 = canon(p2, p2.variables.keys().copied().collect());
    let ccons1: HashMap<String, NameId> =
        p1.constraints.values().map(|c| (apply_rename_rules(p1.resolve(c.name()), rules), c.name())).collect();
    let ccons2: HashMap<String, NameId> =
        p2.constraints.values().map(|c| (apply_rename_rules(p2.resolve(c.name()), rules), c.name())).collect();
    let cobjs1 = canon(p1, p1.objectives.keys().copied().collect());
    let cobjs2 = canon(p2, p2.objectives.keys().copied().collect());

    let set_of = |m: &HashMap<String, NameId>| -> BTreeSet<String> { m.keys().cloned().collect() };
    let vars1 = set_of(&cvars1);
    let vars2 = set_of(&cvars2);
    let cons1 = set_of(&ccons1);
    let cons2 = set_of(&ccons2);
    let objs1 = set_of(&cobjs1);
    let objs2 = set_of(&cobjs2);

    // Sorted intersections keep modified-section output deterministic.
    let cons_common: Vec<String> = cons1.intersection(&cons2).cloned().collect();
    let objs_common: Vec<String> = objs1.intersection(&objs2).cloned().collect();

    let mut vars_type_changed = Vec::new();
    for name in vars1.intersection(&vars2) {
        let t1 = &p1.variables[&cvars1[name]].var_type;
        let t2 = &p2.variables[&cvars2[name]].var_type;
        if t1 != t2 {
            vars_type_changed.push((name.clone(), format!("{t1:?}"), format!("{t2:?}")));
        }
    }

    LpDiff {
        vars_added: vars2.difference(&vars1).cloned().collect(),
        vars_removed: vars1.difference(&vars2).cloned().collect(),
        vars_type_changed,
        cons_added: cons2.difference(&cons1).cloned().collect(),
        cons_removed: cons1.difference(&cons2).cloned().collect(),
        cons_modified: diff_modified_constraints(p1, p2, &ccons1, &ccons2, &cons_common, rules, tol),
        objs_added: objs2.difference(&objs1).cloned().collect(),
        objs_removed: objs1.difference(&objs2).cloned().collect(),
        objs_modified: diff_modified_objectives(p1, p2, &cobjs1, &cobjs2, &objs_common, rules, tol),
    }
}

/// Render the diff in human-readable text form.
#[cfg(feature = "diff")]
fn write_diff_text(
    writer: &mut OutputWriter,
    args: &DiffArgs,
    p1: &LpProblem,
    p2: &LpProblem,
    diff: &LpDiff,
    rule_count: usize,
    tol: DiffTol,
) -> io::Result<()> {
    writeln!(writer, "=== LP Diff ===")?;
    writeln!(writer, "file1: {}", args.file1.display())?;
    writeln!(writer, "file2: {}", args.file2.display())?;
    writeln!(writer, "abs_tol: {}  rel_tol: {}  rename_rules: {rule_count}", tol.abs, tol.rel)?;
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

    fmt_list(writer, "Variables added", &diff.vars_added)?;
    fmt_list(writer, "Variables removed", &diff.vars_removed)?;
    writeln!(writer, "Variables with changed type ({}):", diff.vars_type_changed.len())?;
    for (n, a, b) in diff.vars_type_changed.iter().take(limit) {
        writeln!(writer, "  {n}: {a} -> {b}")?;
    }

    fmt_list(writer, "Constraints added", &diff.cons_added)?;
    fmt_list(writer, "Constraints removed", &diff.cons_removed)?;
    writeln!(writer, "Constraints modified ({}):", diff.cons_modified.len())?;
    for (n, changes) in diff.cons_modified.iter().take(limit) {
        writeln!(writer, "  {n}: {}", changes.join("; "))?;
    }
    if diff.cons_modified.len() > limit {
        writeln!(writer, "  ... ({} more)", diff.cons_modified.len() - limit)?;
    }

    fmt_list(writer, "Objectives added", &diff.objs_added)?;
    fmt_list(writer, "Objectives removed", &diff.objs_removed)?;
    writeln!(writer, "Objectives modified ({}):", diff.objs_modified.len())?;
    for (n, changes) in diff.objs_modified.iter().take(limit) {
        writeln!(writer, "  {n}: {}", changes.join("; "))?;
    }

    Ok(())
}

/// Build the JSON/YAML representation of the diff.
#[cfg(all(feature = "diff", feature = "serde"))]
fn build_diff_json(args: &DiffArgs, p1: &LpProblem, p2: &LpProblem, diff: &LpDiff, rule_count: usize, tol: DiffTol) -> serde_json::Value {
    serde_json::json!({
        "file1": args.file1.display().to_string(),
        "file2": args.file2.display().to_string(),
        "abs_tol": tol.abs,
        "rel_tol": tol.rel,
        "rename_rule_count": rule_count,
        "counts": {
            "objectives": [p1.objective_count(), p2.objective_count()],
            "constraints": [p1.constraint_count(), p2.constraint_count()],
            "variables": [p1.variable_count(), p2.variable_count()],
        },
        "variables_added": diff.vars_added,
        "variables_removed": diff.vars_removed,
        "variables_type_changed": diff.vars_type_changed,
        "constraints_added": diff.cons_added,
        "constraints_removed": diff.cons_removed,
        "constraints_modified": diff.cons_modified,
        "objectives_added": diff.objs_added,
        "objectives_removed": diff.objs_removed,
        "objectives_modified": diff.objs_modified,
    })
}

#[cfg(feature = "diff")]
fn cmd_diff(args: &DiffArgs, verbose: bool, quiet: bool) -> Result<(), BoxError> {
    // Rename rules arrive as a flat list of PATTERN REPLACEMENT pairs.
    if args.rename.len() % 2 != 0 {
        return Err("--rename requires pairs of PATTERN REPLACEMENT".into());
    }
    let rules: Vec<(regex::Regex, String)> =
        args.rename.chunks_exact(2).map(|c| Ok::<_, BoxError>((regex::Regex::new(&c[0])?, c[1].clone()))).collect::<Result<_, _>>()?;

    let tol = DiffTol { abs: args.abs_tol, rel: args.rel_tol };

    if !quiet && verbose {
        eprintln!("Diffing {} vs {}", args.file1.display(), args.file2.display());
        eprintln!("abs_tol={} rel_tol={} rename_rules={}", tol.abs, tol.rel, rules.len());
    }

    let content1 = parse_file(&args.file1)?;
    let content2 = parse_file(&args.file2)?;
    let p1 = LpProblem::parse(&content1)?;
    let p2 = LpProblem::parse(&content2)?;

    let diff = compute_lp_diff(&p1, &p2, &rules, tol);

    let mut writer = OutputWriter::new(args.output.clone())?;
    match args.format {
        OutputFormat::Text => write_diff_text(&mut writer, args, &p1, &p2, &diff, rules.len(), tol)?,
        #[cfg(feature = "serde")]
        OutputFormat::Json | OutputFormat::Yaml => {
            let summary = build_diff_json(args, &p1, &p2, &diff, rules.len(), tol);
            match args.format {
                OutputFormat::Json => {
                    if args.pretty {
                        serde_json::to_writer_pretty(&mut writer, &summary)?;
                    } else {
                        serde_json::to_writer(&mut writer, &summary)?;
                    }
                    writeln!(writer)?;
                }
                OutputFormat::Yaml => serde_yaml::to_writer(&mut writer, &summary)?,
                OutputFormat::Text => unreachable!(),
            }
        }
    }

    Ok(())
}

fn cmd_convert(args: ConvertArgs, verbose: bool, quiet: bool) -> Result<(), BoxError> {
    use lp_parser_rs::writer::{LpWriterOptions, write_lp_string_with_options};

    let content = parse_file(&args.file)?;
    let problem = LpProblem::parse(&content)?;

    if !quiet && verbose {
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
            let output = write_lp_string_with_options(&problem, &options);

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

/// Map a solver [`Status`](lp_solvers::solvers::Status) to its serialised string form.
#[cfg(all(feature = "lp-solvers", feature = "serde"))]
const fn solve_status_str(status: &lp_solvers::solvers::Status) -> &'static str {
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
fn cmd_solve(args: SolveArgs, verbose: bool, quiet: bool) -> Result<(), BoxError> {
    use lp_parser_rs::compat::lp_solvers::LpSolversCompat;
    use lp_solvers::solvers::{CbcSolver, GlpkSolver, SolverTrait, Status};

    let content = parse_file(&args.file)?;
    let problem = LpProblem::parse(&content)?;

    if !quiet && verbose {
        eprintln!("Loading problem: {}", args.file.display());
    }

    let compat = LpSolversCompat::try_new(&problem)?;

    // Print warnings
    for warning in compat.warnings() {
        if !quiet {
            eprintln!("Warning: {warning}");
        }
    }

    if !quiet && verbose {
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
                "status": solve_status_str(&solution.status),
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
                "status": solve_status_str(&solution.status),
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
        Commands::Diff(args) => cmd_diff(&args, cli.verbose, cli.quiet),
        Commands::Convert(args) => cmd_convert(args, cli.verbose, cli.quiet),
        #[cfg(feature = "lp-solvers")]
        Commands::Solve(args) => cmd_solve(args, cli.verbose, cli.quiet),
    }
}
