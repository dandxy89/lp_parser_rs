//! Summary detail widget.
//!
//! Shows file paths, optional problem-name and sense changes, a table of
//! per-section change counts, and a comparative structural analysis derived
//! from `ProblemAnalysis`.

use lp_parser_rs::analysis::{IssueSeverity, ProblemAnalysis};
use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

use crate::diff_model::{DiffCounts, DiffSummary, LpDiffReport};
use crate::theme::theme;
use crate::widgets::numerics::format_scientific;
use crate::widgets::{ARROW, muted, rule_str, severity_colour, short_filename};

/// Build pre-formatted summary lines. Called once at startup since report data never changes.
pub fn build_summary_lines(
    report: &LpDiffReport,
    summary: &DiffSummary,
    analysis1: &ProblemAnalysis,
    analysis2: &ProblemAnalysis,
) -> Vec<Line<'static>> {
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

    lines
}

/// Draw the summary content into `area` using pre-built lines (no border — caller provides the border).
///
/// Uses O(visible) windowed rendering instead of cloning all lines into a `Paragraph`.
/// Returns the total content line count.
pub fn draw_summary(frame: &mut Frame, area: Rect, cached_lines: &[Line<'static>], scroll: u16) -> usize {
    // A zero-sized area is an environmental condition (shrunken terminal), not a
    // programming error: drawing into it is a no-op.
    if area.width == 0 || area.height == 0 {
        return 0;
    }
    let line_count = cached_lines.len();
    let skip = scroll as usize;
    let visible = area.height as usize;
    let buf: &mut Buffer = frame.buffer_mut();

    for (i, line) in cached_lines.iter().skip(skip).take(visible).enumerate() {
        #[allow(clippy::cast_possible_truncation)] // i < visible <= area.height (u16)
        let y = area.y + i as u16;
        debug_assert!(y < area.y + area.height, "summary line y={y} exceeds area bounds");
        buf.set_line(area.x, y, line, area.width);
    }

    line_count
}

fn build_header(lines: &mut Vec<Line<'static>>, report: &LpDiffReport) {
    let t = theme();
    lines.push(Line::from(vec![
        Span::styled("  ", Style::default()),
        Span::styled(report.file1.clone(), Style::default()),
        Span::styled(ARROW, muted()),
        Span::styled(report.file2.clone(), Style::default()),
    ]));

    if let Some((ref old, ref new)) = report.name_changed {
        let old_name = old.as_deref().unwrap_or("(unnamed)");
        let new_name = new.as_deref().unwrap_or("(unnamed)");
        lines.push(Line::from(vec![
            Span::styled("  Name:   ", Style::default().fg(t.muted)),
            Span::styled(format!("\"{old_name}\""), Style::default().fg(t.removed)),
            Span::styled(ARROW, muted()),
            Span::styled(format!("\"{new_name}\""), Style::default().fg(t.added)),
        ]));
    }

    if let Some((ref old_sense, ref new_sense)) = report.sense_changed {
        lines.push(Line::from(vec![
            Span::styled("  Sense:  ", Style::default().fg(t.muted)),
            Span::styled(format!("{old_sense}"), Style::default().fg(t.removed)),
            Span::styled(ARROW, muted()),
            Span::styled(format!("{new_sense}"), Style::default().fg(t.added)),
        ]));
    }

    // Compare options — only render when something non-default is active.
    if !report.options_summary.is_default() {
        lines.push(Line::from(vec![
            Span::styled("  Compare: ", Style::default().fg(t.muted)),
            Span::styled(report.options_summary.to_string(), Style::default().fg(t.accent)),
        ]));
    }
}

fn build_column_headings(lines: &mut Vec<Line<'static>>) {
    let t = theme();
    lines.push(Line::from(vec![Span::styled(
        format!("  {:<14}{:>7}{:>9}{:>12}{:>9}{:>9}", "Section", "Added", "Removed", "Modified", "Renamed", "Total"),
        Style::default().fg(t.muted).add_modifier(Modifier::BOLD),
    )]));
    lines.push(Line::from(vec![Span::styled(format!("  {}", rule_str(60)), Style::default().fg(t.muted))]));
}

fn build_section_rows(lines: &mut Vec<Line<'static>>, summary: &DiffSummary) {
    for (label, counts) in [("Variables", summary.variables), ("Constraints", summary.constraints), ("Objectives", summary.objectives)] {
        lines.push(format_count_row(label, &counts, false));
    }
}

fn build_separator(lines: &mut Vec<Line<'static>>) {
    let t = theme();
    lines.push(Line::from(vec![Span::styled(format!("  {}", rule_str(60)), Style::default().fg(t.muted))]));
}

fn build_totals_row(lines: &mut Vec<Line<'static>>, summary: &DiffSummary) {
    let totals = summary.aggregate_counts();
    lines.push(format_count_row("TOTAL", &totals, true));
}

fn format_count_row(label: &str, counts: &DiffCounts, is_total: bool) -> Line<'static> {
    let t = theme();
    let label_style = if is_total { Style::default().fg(t.text).add_modifier(Modifier::BOLD) } else { Style::default().fg(t.text) };
    let total_style = if is_total { Style::default().fg(t.text).add_modifier(Modifier::BOLD) } else { Style::default().fg(t.text) };

    Line::from(vec![
        Span::styled(format!("  {label:<14}"), label_style),
        Span::styled(format!("{:>7}", counts.added), Style::default().fg(t.added)),
        Span::styled(format!("{:>9}", counts.removed), Style::default().fg(t.removed)),
        Span::styled(format!("{:>12}", counts.modified), Style::default().fg(t.modified)),
        Span::styled(format!("{:>9}", counts.renamed), Style::default().fg(t.info)),
        Span::styled(format!("{:>9}", counts.total()), total_style),
    ])
}

/// Render a section heading with underline.
fn section_heading(lines: &mut Vec<Line<'static>>, title: &str) {
    let t = theme();
    lines.push(Line::from(vec![Span::styled(format!("  {title}"), Style::default().fg(t.accent).add_modifier(Modifier::BOLD))]));
}

/// Render a three-column comparison header row with a separator rule.
fn comparison_header(lines: &mut Vec<Line<'static>>, label_width: usize) {
    let t = theme();
    lines.push(Line::from(vec![Span::styled(
        format!("  {:<label_width$}{:>12}{:>12}{:>12}", "", "File A", "File B", "Delta"),
        Style::default().fg(t.muted).add_modifier(Modifier::BOLD),
    )]));
    lines.push(Line::from(vec![Span::styled(format!("  {}", rule_str(label_width + 36)), Style::default().fg(t.muted))]));
}

/// Render a comparison row with usize values and a delta.
#[allow(clippy::cast_possible_wrap)] // values are LP problem dimensions, never close to i64::MAX
fn comparison_row_usize(lines: &mut Vec<Line<'static>>, label: &str, label_width: usize, a: usize, b: usize) {
    let t = theme();
    let delta = b as i64 - a as i64;
    let delta_str = format_delta_i64(delta);
    let delta_colour = delta_colour_i64(delta);

    lines.push(Line::from(vec![
        Span::styled(format!("  {label:<label_width$}"), Style::default().fg(t.text)),
        Span::styled(format!("{a:>12}"), Style::default().fg(t.text)),
        Span::styled(format!("{b:>12}"), Style::default().fg(t.text)),
        Span::styled(format!("{delta_str:>12}"), Style::default().fg(delta_colour)),
    ]));
}

/// Render a comparison row with f64 percentage values and a delta.
fn comparison_row_pct(lines: &mut Vec<Line<'static>>, label: &str, label_width: usize, a: f64, b: f64) {
    let t = theme();
    let delta = b - a;
    let delta_str = if delta.abs() < 1e-10 { "\u{2014}".to_string() } else { format!("{delta:+.2}%") };
    let delta_colour = if delta.abs() < 1e-10 {
        t.muted
    } else if delta > 0.0 {
        t.added
    } else {
        t.removed
    };

    lines.push(Line::from(vec![
        Span::styled(format!("  {label:<label_width$}"), Style::default().fg(t.text)),
        Span::styled(format!("{:>11.2}%", a * 100.0), Style::default().fg(t.text)),
        Span::styled(format!("{:>11.2}%", b * 100.0), Style::default().fg(t.text)),
        Span::styled(format!("{delta_str:>12}"), Style::default().fg(delta_colour)),
    ]));
}

/// Render a two-column row (no delta) with string values.
fn comparison_row_str(lines: &mut Vec<Line<'static>>, label: &str, label_width: usize, a: &str, b: &str) {
    let t = theme();
    lines.push(Line::from(vec![
        Span::styled(format!("  {label:<label_width$}"), Style::default().fg(t.text)),
        Span::styled(format!("{a:>12}"), Style::default().fg(t.text)),
        Span::styled(format!("{b:>12}"), Style::default().fg(t.text)),
    ]));
}

fn format_delta_i64(delta: i64) -> String {
    match delta.cmp(&0) {
        std::cmp::Ordering::Equal => "\u{2014}".to_string(),
        std::cmp::Ordering::Greater => format!("+{delta}"),
        std::cmp::Ordering::Less => format!("{delta}"),
    }
}

fn delta_colour_i64(delta: i64) -> Color {
    let t = theme();
    match delta.cmp(&0) {
        std::cmp::Ordering::Equal => t.muted,
        std::cmp::Ordering::Greater => t.added,
        std::cmp::Ordering::Less => t.removed,
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

fn build_coefficient_table(lines: &mut Vec<Line<'static>>, a: &ProblemAnalysis, b: &ProblemAnalysis) {
    const W: usize = 18;

    let t = theme();
    // Two-column header (ranges don't have a meaningful delta)
    lines.push(Line::from(vec![Span::styled(
        format!("  {:<W$}{:>16}{:>16}", "", "File A", "File B"),
        Style::default().fg(t.muted).add_modifier(Modifier::BOLD),
    )]));
    lines.push(Line::from(vec![Span::styled(format!("  {}", rule_str(W + 32)), Style::default().fg(t.muted))]));

    // Coefficient range
    let coeff_a = crate::widgets::numerics::format_range_prec(&a.coefficients.constraint_coeff_range, 1);
    let coeff_b = crate::widgets::numerics::format_range_prec(&b.coefficients.constraint_coeff_range, 1);
    lines.push(Line::from(vec![
        Span::styled(format!("  {:<W$}", "Coeff range"), Style::default().fg(t.text)),
        Span::styled(format!("{coeff_a:>16}"), Style::default().fg(t.text)),
        Span::styled(format!("{coeff_b:>16}"), Style::default().fg(t.text)),
    ]));

    // Coefficient ratio
    let ratio_a = format_scientific(a.coefficients.coefficient_ratio);
    let ratio_b = format_scientific(b.coefficients.coefficient_ratio);
    lines.push(Line::from(vec![
        Span::styled(format!("  {:<W$}", "Coeff ratio"), Style::default().fg(t.text)),
        Span::styled(format!("{ratio_a:>16}"), Style::default().fg(t.text)),
        Span::styled(format!("{ratio_b:>16}"), Style::default().fg(t.text)),
    ]));

    // RHS range
    let rhs_a = crate::widgets::numerics::format_range_prec(&a.constraints.rhs_range, 1);
    let rhs_b = crate::widgets::numerics::format_range_prec(&b.constraints.rhs_range, 1);
    lines.push(Line::from(vec![
        Span::styled(format!("  {:<W$}", "RHS range"), Style::default().fg(t.text)),
        Span::styled(format!("{rhs_a:>16}"), Style::default().fg(t.text)),
        Span::styled(format!("{rhs_b:>16}"), Style::default().fg(t.text)),
    ]));
}

fn build_issues_section(lines: &mut Vec<Line<'static>>, report: &LpDiffReport, analysis1: &ProblemAnalysis, analysis2: &ProblemAnalysis) {
    let t = theme();
    let (err1, warn1, info1) = count_issues(&analysis1.issues);
    let (err2, warn2, info2) = count_issues(&analysis2.issues);

    section_heading(lines, "Issues");

    // Summary counts line
    lines.push(Line::from(vec![
        Span::styled("  File A: ", Style::default().fg(t.muted)),
        issue_count_span(err1, "error", t.error),
        Span::styled(", ", Style::default().fg(t.muted)),
        issue_count_span(warn1, "warning", t.warning),
        Span::styled(", ", Style::default().fg(t.muted)),
        issue_count_span(info1, "info", t.info),
        Span::styled("  \u{2502}  ", Style::default().fg(t.muted)),
        Span::styled("File B: ", Style::default().fg(t.muted)),
        issue_count_span(err2, "error", t.error),
        Span::styled(", ", Style::default().fg(t.muted)),
        issue_count_span(warn2, "warning", t.warning),
        Span::styled(", ", Style::default().fg(t.muted)),
        issue_count_span(info2, "info", t.info),
    ]));

    if analysis1.issues.is_empty() && analysis2.issues.is_empty() {
        lines.push(Line::from(vec![Span::styled("  No issues detected", Style::default().fg(t.muted))]));
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
    let t = theme();
    let plural = if count == 1 { "" } else { "s" };
    let style = if count > 0 { Style::default().fg(colour) } else { Style::default().fg(t.muted) };
    Span::styled(format!("{count} {label}{plural}"), style)
}

fn format_issue_line(file_label: &str, issue: &lp_parser_rs::analysis::AnalysisIssue) -> Line<'static> {
    let t = theme();
    let colour = severity_colour(issue.severity);
    let severity_tag = format!("{}", issue.severity);
    Line::from(vec![
        Span::styled(format!("  [{severity_tag:<7}] "), Style::default().fg(colour).add_modifier(Modifier::BOLD)),
        Span::styled(format!("{file_label}: "), Style::default().fg(t.muted)),
        Span::styled(issue.message.clone(), Style::default().fg(t.text)),
    ])
}

