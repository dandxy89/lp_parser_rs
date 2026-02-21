//! Summary detail widget.
//!
//! Shows file paths, optional problem-name and sense changes, a table of
//! per-section change counts, and a comparative structural analysis derived
//! from `ProblemAnalysis`.

use lp_parser_rs::analysis::{IssueSeverity, ProblemAnalysis};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::diff_model::{DiffCounts, DiffSummary, LpDiffReport};
use crate::widgets::{ARROW, MUTED};

/// Draw the summary content into `area` (no border — caller provides the border).
/// Returns the total content line count.
pub fn draw_summary(
    frame: &mut Frame,
    area: Rect,
    report: &LpDiffReport,
    summary: &DiffSummary,
    analysis1: &ProblemAnalysis,
    analysis2: &ProblemAnalysis,
    scroll: u16,
) -> usize {
    let mut lines: Vec<Line<'static>> = Vec::with_capacity(64);

    build_header(&mut lines, report);
    lines.push(Line::from(""));
    build_column_headings(&mut lines);
    build_section_rows(&mut lines, summary);
    build_separator(&mut lines);
    build_totals_row(&mut lines, summary);

    // Comparative Analysis
    lines.push(Line::from(""));
    section_heading(&mut lines, "Problem Dimensions");
    build_dimensions_table(&mut lines, analysis1, analysis2);

    lines.push(Line::from(""));
    section_heading(&mut lines, "Variable Types");
    build_variable_type_table(&mut lines, analysis1, analysis2);

    lines.push(Line::from(""));
    section_heading(&mut lines, "Constraint Types");
    build_constraint_type_table(&mut lines, analysis1, analysis2);

    lines.push(Line::from(""));
    section_heading(&mut lines, "Coefficient Scaling");
    build_coefficient_table(&mut lines, analysis1, analysis2);

    lines.push(Line::from(""));
    build_issues_section(&mut lines, report, analysis1, analysis2);

    let line_count = lines.len();
    let paragraph = Paragraph::new(lines).scroll((scroll, 0));
    frame.render_widget(paragraph, area);
    line_count
}

fn build_header(lines: &mut Vec<Line<'static>>, report: &LpDiffReport) {
    lines.push(Line::from(vec![
        Span::styled("  ", Style::default()),
        Span::styled(report.file1.clone(), Style::default()),
        Span::styled(ARROW, MUTED),
        Span::styled(report.file2.clone(), Style::default()),
    ]));

    if let Some((ref old, ref new)) = report.name_changed {
        let old_name = old.as_deref().unwrap_or("(unnamed)");
        let new_name = new.as_deref().unwrap_or("(unnamed)");
        lines.push(Line::from(vec![
            Span::styled("  Name:   ", Style::default().fg(Color::DarkGray)),
            Span::styled(format!("\"{old_name}\""), Style::default().fg(Color::Red)),
            Span::styled(ARROW, MUTED),
            Span::styled(format!("\"{new_name}\""), Style::default().fg(Color::Green)),
        ]));
    }

    if let Some((ref old_sense, ref new_sense)) = report.sense_changed {
        lines.push(Line::from(vec![
            Span::styled("  Sense:  ", Style::default().fg(Color::DarkGray)),
            Span::styled(format!("{old_sense}"), Style::default().fg(Color::Red)),
            Span::styled(ARROW, MUTED),
            Span::styled(format!("{new_sense}"), Style::default().fg(Color::Green)),
        ]));
    }
}

fn build_column_headings(lines: &mut Vec<Line<'static>>) {
    lines.push(Line::from(vec![Span::styled(
        format!("  {:<14}{:>7}{:>9}{:>12}{:>9}", "Section", "Added", "Removed", "Modified", "Total"),
        Style::default().fg(Color::DarkGray).add_modifier(Modifier::BOLD),
    )]));
}

fn build_section_rows(lines: &mut Vec<Line<'static>>, summary: &DiffSummary) {
    for (label, counts) in [("Variables", summary.variables), ("Constraints", summary.constraints), ("Objectives", summary.objectives)] {
        lines.push(format_count_row(label, &counts, false));
    }
}

fn build_separator(lines: &mut Vec<Line<'static>>) {
    lines.push(Line::from(vec![Span::styled("  ────────────────────────────────────────────────", Style::default().fg(Color::DarkGray))]));
}

fn build_totals_row(lines: &mut Vec<Line<'static>>, summary: &DiffSummary) {
    let totals = DiffCounts {
        added: summary.variables.added + summary.constraints.added + summary.objectives.added,
        removed: summary.variables.removed + summary.constraints.removed + summary.objectives.removed,
        modified: summary.variables.modified + summary.constraints.modified + summary.objectives.modified,
        unchanged: summary.variables.unchanged + summary.constraints.unchanged + summary.objectives.unchanged,
    };
    lines.push(format_count_row("TOTAL", &totals, true));
}

fn format_count_row(label: &str, counts: &DiffCounts, is_total: bool) -> Line<'static> {
    let label_style =
        if is_total { Style::default().fg(Color::White).add_modifier(Modifier::BOLD) } else { Style::default().fg(Color::White) };
    let total_style =
        if is_total { Style::default().fg(Color::White).add_modifier(Modifier::BOLD) } else { Style::default().fg(Color::White) };

    Line::from(vec![
        Span::styled(format!("  {label:<14}"), label_style),
        Span::styled(format!("{:>7}", counts.added), Style::default().fg(Color::Green)),
        Span::styled(format!("{:>9}", counts.removed), Style::default().fg(Color::Red)),
        Span::styled(format!("{:>12}", counts.modified), Style::default().fg(Color::Yellow)),
        Span::styled(format!("{:>9}", counts.total()), total_style),
    ])
}

/// Render a section heading with underline.
fn section_heading(lines: &mut Vec<Line<'static>>, title: &str) {
    lines.push(Line::from(vec![Span::styled(format!("  {title}"), Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))]));
}

/// Render a three-column comparison header row.
fn comparison_header(lines: &mut Vec<Line<'static>>, label_width: usize) {
    lines.push(Line::from(vec![Span::styled(
        format!("  {:<label_width$}{:>12}{:>12}{:>12}", "", "File A", "File B", "Delta"),
        Style::default().fg(Color::DarkGray).add_modifier(Modifier::BOLD),
    )]));
}

/// Render a comparison row with usize values and a delta.
#[allow(clippy::cast_possible_wrap)] // values are LP problem dimensions, never close to i64::MAX
fn comparison_row_usize(lines: &mut Vec<Line<'static>>, label: &str, label_width: usize, a: usize, b: usize) {
    let delta = b as i64 - a as i64;
    let delta_str = format_delta_i64(delta);
    let delta_colour = delta_colour_i64(delta);

    lines.push(Line::from(vec![
        Span::styled(format!("  {label:<label_width$}"), Style::default().fg(Color::White)),
        Span::styled(format!("{a:>12}"), Style::default().fg(Color::White)),
        Span::styled(format!("{b:>12}"), Style::default().fg(Color::White)),
        Span::styled(format!("{delta_str:>12}"), Style::default().fg(delta_colour)),
    ]));
}

/// Render a comparison row with f64 percentage values and a delta.
fn comparison_row_pct(lines: &mut Vec<Line<'static>>, label: &str, label_width: usize, a: f64, b: f64) {
    let delta = b - a;
    let delta_str = if delta.abs() < 1e-10 { "\u{2014}".to_string() } else { format!("{delta:+.2}%") };
    let delta_colour = if delta.abs() < 1e-10 {
        Color::DarkGray
    } else if delta > 0.0 {
        Color::Green
    } else {
        Color::Red
    };

    lines.push(Line::from(vec![
        Span::styled(format!("  {label:<label_width$}"), Style::default().fg(Color::White)),
        Span::styled(format!("{:>11.2}%", a * 100.0), Style::default().fg(Color::White)),
        Span::styled(format!("{:>11.2}%", b * 100.0), Style::default().fg(Color::White)),
        Span::styled(format!("{delta_str:>12}"), Style::default().fg(delta_colour)),
    ]));
}

/// Render a two-column row (no delta) with string values.
fn comparison_row_str(lines: &mut Vec<Line<'static>>, label: &str, label_width: usize, a: &str, b: &str) {
    lines.push(Line::from(vec![
        Span::styled(format!("  {label:<label_width$}"), Style::default().fg(Color::White)),
        Span::styled(format!("{a:>12}"), Style::default().fg(Color::White)),
        Span::styled(format!("{b:>12}"), Style::default().fg(Color::White)),
    ]));
}

fn format_delta_i64(delta: i64) -> String {
    match delta.cmp(&0) {
        std::cmp::Ordering::Equal => "\u{2014}".to_string(),
        std::cmp::Ordering::Greater => format!("+{delta}"),
        std::cmp::Ordering::Less => format!("{delta}"),
    }
}

const fn delta_colour_i64(delta: i64) -> Color {
    if delta == 0 {
        Color::DarkGray
    } else if delta > 0 {
        Color::Green
    } else {
        Color::Red
    }
}

fn build_dimensions_table(lines: &mut Vec<Line<'static>>, a: &ProblemAnalysis, b: &ProblemAnalysis) {
    const W: usize = 18;
    comparison_header(lines, W);
    comparison_row_usize(lines, "Variables", W, a.summary.variable_count, b.summary.variable_count);
    comparison_row_usize(lines, "Constraints", W, a.summary.constraint_count, b.summary.constraint_count);
    comparison_row_usize(lines, "Non-zeros", W, a.summary.total_nonzeros, b.summary.total_nonzeros);
    comparison_row_pct(lines, "Density", W, a.summary.density, b.summary.density);

    // Sparsity range (no delta, just two-column)
    let sparsity_a = format!("{}\u{2013}{}", a.sparsity.min_vars_per_constraint, a.sparsity.max_vars_per_constraint);
    let sparsity_b = format!("{}\u{2013}{}", b.sparsity.min_vars_per_constraint, b.sparsity.max_vars_per_constraint);
    comparison_row_str(lines, "Vars/constraint", W, &sparsity_a, &sparsity_b);
}

fn build_variable_type_table(lines: &mut Vec<Line<'static>>, a: &ProblemAnalysis, b: &ProblemAnalysis) {
    const W: usize = 18;
    comparison_header(lines, W);
    let va = &a.variables.type_distribution;
    let vb = &b.variables.type_distribution;

    comparison_row_usize(lines, "Binary", W, va.binary, vb.binary);
    comparison_row_usize(lines, "Integer", W, va.integer, vb.integer);
    comparison_row_usize(lines, "General", W, va.general, vb.general);
    comparison_row_usize(lines, "Free", W, va.free, vb.free);
    comparison_row_usize(lines, "Lower-bounded", W, va.lower_bounded, vb.lower_bounded);
    comparison_row_usize(lines, "Upper-bounded", W, va.upper_bounded, vb.upper_bounded);
    comparison_row_usize(lines, "Double-bounded", W, va.double_bounded, vb.double_bounded);
    comparison_row_usize(lines, "Semi-continuous", W, va.semi_continuous, vb.semi_continuous);
}

fn build_constraint_type_table(lines: &mut Vec<Line<'static>>, a: &ProblemAnalysis, b: &ProblemAnalysis) {
    const W: usize = 18;
    comparison_header(lines, W);
    let ca = &a.constraints.type_distribution;
    let cb = &b.constraints.type_distribution;

    comparison_row_usize(lines, "Equality (=)", W, ca.equality, cb.equality);
    comparison_row_usize(lines, "<= constraints", W, ca.less_than_equal, cb.less_than_equal);
    comparison_row_usize(lines, ">= constraints", W, ca.greater_than_equal, cb.greater_than_equal);
    comparison_row_usize(lines, "< constraints", W, ca.less_than, cb.less_than);
    comparison_row_usize(lines, "> constraints", W, ca.greater_than, cb.greater_than);
    comparison_row_usize(lines, "SOS1", W, ca.sos1, cb.sos1);
    comparison_row_usize(lines, "SOS2", W, ca.sos2, cb.sos2);
}

fn format_range(r: &lp_parser_rs::analysis::RangeStats) -> String {
    if r.count == 0 { "\u{2014}".to_string() } else { format!("[{:.1e}, {:.1e}]", r.min, r.max) }
}

fn build_coefficient_table(lines: &mut Vec<Line<'static>>, a: &ProblemAnalysis, b: &ProblemAnalysis) {
    const W: usize = 18;
    // Two-column header (ranges don't have a meaningful delta)
    lines.push(Line::from(vec![Span::styled(
        format!("  {:<W$}{:>16}{:>16}", "", "File A", "File B"),
        Style::default().fg(Color::DarkGray).add_modifier(Modifier::BOLD),
    )]));

    // Coefficient range
    let coeff_a = format_range(&a.coefficients.constraint_coeff_range);
    let coeff_b = format_range(&b.coefficients.constraint_coeff_range);
    lines.push(Line::from(vec![
        Span::styled(format!("  {:<W$}", "Coeff range"), Style::default().fg(Color::White)),
        Span::styled(format!("{coeff_a:>16}"), Style::default().fg(Color::White)),
        Span::styled(format!("{coeff_b:>16}"), Style::default().fg(Color::White)),
    ]));

    // Coefficient ratio
    let ratio_a = format_scientific(a.coefficients.coefficient_ratio);
    let ratio_b = format_scientific(b.coefficients.coefficient_ratio);
    lines.push(Line::from(vec![
        Span::styled(format!("  {:<W$}", "Coeff ratio"), Style::default().fg(Color::White)),
        Span::styled(format!("{ratio_a:>16}"), Style::default().fg(Color::White)),
        Span::styled(format!("{ratio_b:>16}"), Style::default().fg(Color::White)),
    ]));

    // RHS range
    let rhs_a = format_range(&a.constraints.rhs_range);
    let rhs_b = format_range(&b.constraints.rhs_range);
    lines.push(Line::from(vec![
        Span::styled(format!("  {:<W$}", "RHS range"), Style::default().fg(Color::White)),
        Span::styled(format!("{rhs_a:>16}"), Style::default().fg(Color::White)),
        Span::styled(format!("{rhs_b:>16}"), Style::default().fg(Color::White)),
    ]));
}

fn format_scientific(v: f64) -> String {
    if v == 0.0 || !v.is_finite() { "\u{2014}".to_string() } else { format!("{v:.2e}") }
}

fn build_issues_section(lines: &mut Vec<Line<'static>>, report: &LpDiffReport, analysis1: &ProblemAnalysis, analysis2: &ProblemAnalysis) {
    let (err1, warn1, info1) = count_issues(&analysis1.issues);
    let (err2, warn2, info2) = count_issues(&analysis2.issues);

    section_heading(lines, "Issues");

    // Summary counts line
    lines.push(Line::from(vec![
        Span::styled("  File A: ", Style::default().fg(Color::DarkGray)),
        issue_count_span(err1, "error", Color::Red),
        Span::styled(", ", Style::default().fg(Color::DarkGray)),
        issue_count_span(warn1, "warning", Color::Yellow),
        Span::styled(", ", Style::default().fg(Color::DarkGray)),
        issue_count_span(info1, "info", Color::Blue),
        Span::styled("  \u{2502}  ", Style::default().fg(Color::DarkGray)),
        Span::styled("File B: ", Style::default().fg(Color::DarkGray)),
        issue_count_span(err2, "error", Color::Red),
        Span::styled(", ", Style::default().fg(Color::DarkGray)),
        issue_count_span(warn2, "warning", Color::Yellow),
        Span::styled(", ", Style::default().fg(Color::DarkGray)),
        issue_count_span(info2, "info", Color::Blue),
    ]));

    if analysis1.issues.is_empty() && analysis2.issues.is_empty() {
        lines.push(Line::from(vec![Span::styled("  No issues detected", Style::default().fg(Color::DarkGray))]));
        return;
    }

    lines.push(Line::from(""));

    // File A issues
    let label_a = short_filename(&report.file1);
    for issue in &analysis1.issues {
        lines.push(format_issue_line(&label_a, issue));
    }

    // File B issues
    let label_b = short_filename(&report.file2);
    for issue in &analysis2.issues {
        lines.push(format_issue_line(&label_b, issue));
    }
}

fn count_issues(issues: &[lp_parser_rs::analysis::AnalysisIssue]) -> (usize, usize, usize) {
    let mut errors = 0;
    let mut warnings = 0;
    let mut infos = 0;
    for issue in issues {
        match issue.severity {
            IssueSeverity::Error => errors += 1,
            IssueSeverity::Warning => warnings += 1,
            IssueSeverity::Info => infos += 1,
        }
    }
    (errors, warnings, infos)
}

fn issue_count_span(count: usize, label: &str, colour: Color) -> Span<'static> {
    let plural = if count == 1 { "" } else { "s" };
    let style = if count > 0 { Style::default().fg(colour) } else { Style::default().fg(Color::DarkGray) };
    Span::styled(format!("{count} {label}{plural}"), style)
}

const fn severity_colour(severity: IssueSeverity) -> Color {
    match severity {
        IssueSeverity::Error => Color::Red,
        IssueSeverity::Warning => Color::Yellow,
        IssueSeverity::Info => Color::Blue,
    }
}

fn format_issue_line(file_label: &str, issue: &lp_parser_rs::analysis::AnalysisIssue) -> Line<'static> {
    let colour = severity_colour(issue.severity);
    let severity_tag = format!("{}", issue.severity);
    Line::from(vec![
        Span::styled(format!("  [{severity_tag:<7}] "), Style::default().fg(colour).add_modifier(Modifier::BOLD)),
        Span::styled(format!("{file_label}: "), Style::default().fg(Color::DarkGray)),
        Span::styled(issue.message.clone(), Style::default().fg(Color::White)),
    ])
}

/// Extract the filename from a path string for compact display.
fn short_filename(path: &str) -> String {
    std::path::Path::new(path).file_name().map_or_else(|| path.to_string(), |f| f.to_string_lossy().into_owned())
}
