//! Non-interactive summary output for `--summary` mode.
//!
//! Prints a structured text report to stdout and exits without launching the TUI.

use lp_parser_rs::analysis::ProblemAnalysis;
use lp_parser_rs::problem::LpProblem;

use crate::diff_model::LpDiffReport;

/// Maximum number of analysis issues listed in the inspect `--summary` output.
const MAX_SUMMARY_ISSUES: usize = 20;

/// Print a structured summary of the diff report to stdout.
pub fn print_summary(report: &LpDiffReport) {
    let summary = report.summary();

    println!("LP Diff: {} vs {}", report.file1, report.file2);
    if !report.options_summary.is_default() {
        println!("Options: {}", report.options_summary);
    }
    println!();

    let var = &summary.variables;
    let con = &summary.constraints;
    let obj = &summary.objectives;

    println!("Variables:    +{:<3} -{:<3} ~{:<3} >{:<3} ({} unchanged)", var.added, var.removed, var.modified, var.renamed, var.unchanged);
    println!("Constraints:  +{:<3} -{:<3} ~{:<3} >{:<3} ({} unchanged)", con.added, con.removed, con.modified, con.renamed, con.unchanged);
    println!("Objectives:   +{:<3} -{:<3} ~{:<3} >{:<3} ({} unchanged)", obj.added, obj.removed, obj.modified, obj.renamed, obj.unchanged);
    println!();
    println!("Renamed: {} (counted once, excluded from added/removed)", summary.aggregate_counts().renamed);
    println!("Total: {} changes", summary.total_changes());
}

/// Print a structured single-file inspect summary to stdout: model counts and
/// the top structural-analysis issues.
pub fn print_inspect_summary(file: &str, problem: &LpProblem, analysis: &ProblemAnalysis) {
    println!("LP Inspect: {file}");
    if let Some(name) = problem.name() {
        println!("Name:  {name}");
    }
    println!("Sense: {}", problem.sense);
    println!();

    println!("Variables:    {}", problem.variable_count());
    println!("Constraints:  {}", problem.constraint_count());
    println!("Objectives:   {}", problem.objective_count());
    println!("Non-zeros:    {}", analysis.summary.total_nonzeros);
    println!("Density:      {:.4}%", analysis.summary.density * 100.0);
    println!();

    if analysis.issues.is_empty() {
        println!("Issues: none");
        return;
    }
    println!("Issues: {}", analysis.issues.len());
    for issue in analysis.issues.iter().take(MAX_SUMMARY_ISSUES) {
        println!("  [{}] {}", issue.severity, issue.message);
    }
    if analysis.issues.len() > MAX_SUMMARY_ISSUES {
        println!("  ... ({} more)", analysis.issues.len() - MAX_SUMMARY_ISSUES);
    }
}
