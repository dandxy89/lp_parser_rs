//! Non-interactive summary output for `--summary` mode.
//!
//! Prints a structured text report to stdout and exits without launching the TUI.

use crate::diff_model::LpDiffReport;

/// Print a structured summary of the diff report to stdout.
pub fn print_summary(report: &LpDiffReport) {
    let summary = report.summary();

    println!("LP Diff: {} vs {}", report.file1, report.file2);
    println!();

    let var = &summary.variables;
    let con = &summary.constraints;
    let obj = &summary.objectives;

    println!("Variables:    +{:<3} -{:<3} ~{:<3} ({} unchanged)", var.added, var.removed, var.modified, var.unchanged);
    println!("Constraints:  +{:<3} -{:<3} ~{:<3} ({} unchanged)", con.added, con.removed, con.modified, con.unchanged);
    println!("Objectives:   +{:<3} -{:<3} ~{:<3} ({} unchanged)", obj.added, obj.removed, obj.modified, obj.unchanged);
    println!();
    println!("Total: {} changes", summary.total_changes());
}
