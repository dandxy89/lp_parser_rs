//! Numerics detail widget (section key `5`).
//!
//! Surfaces the library's model-conditioning analysis side-by-side per file:
//! problem size and density, coefficient/RHS magnitude ranges, the range-ratio
//! conditioning headline, and analysis issues — including issues that are new
//! in file 2. Lines are pre-built and cached on `App` (like `summary_lines`)
//! and rebuilt whenever the report is rebuilt (tolerance change, watch reload).

use lp_parser_rs::analysis::{AnalysisIssue, IssueCategory, IssueSeverity, RangeStats};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

use crate::diff_model::LpDiffReport;
use crate::theme::theme;
use crate::widgets::{ARROW, gauge_bar};

/// Range-ratio threshold above which the value is styled as a warning.
/// Aligned with `AnalysisConfig::default().coefficient_ratio_threshold` (1e6).
pub(crate) const RATIO_WARN_THRESHOLD: f64 = 1e6;

/// Range-ratio threshold above which the value is styled as an error.
/// Aligned with `AnalysisConfig::default().large_coefficient_threshold` (1e9).
pub(crate) const RATIO_ERROR_THRESHOLD: f64 = 1e9;

/// Maximum number of "new issues in file 2" lines before truncating.
pub(crate) const MAX_NEW_ISSUES: usize = 20;

/// Label column width for the side-by-side tables.
const LABEL_WIDTH: usize = 18;

/// Value column width for the side-by-side tables.
const VALUE_WIDTH: usize = 20;

/// Map an issue category to a stable small integer for hashing.
/// (`IssueCategory` does not derive `Hash`, and the library cannot be changed here.)
const fn category_key(category: IssueCategory) -> u8 {
    match category {
        IssueCategory::InvalidBounds => 0,
        IssueCategory::NumericalScaling => 1,
        IssueCategory::EmptyConstraint => 2,
        IssueCategory::UnusedVariable => 3,
        IssueCategory::FixedVariable => 4,
        IssueCategory::SingletonConstraint => 5,
        IssueCategory::Other => 6,
    }
}

/// Indices (into `new`) of issues present in `new` but not in `old`.
///
/// Matching rule: an issue's identity is `(category, message)`. Messages embed
/// entity names (variable/constraint), which is exactly what distinguishes
/// per-entity issues; messages that embed computed numbers (e.g. a coefficient
/// ratio) will register as "new" when only the number moved — a few such false
/// positives are acceptable, since a moved ratio is itself worth surfacing.
/// Severity is excluded: it is derived from category + message in the library.
pub(crate) fn new_issue_indices(old: &[AnalysisIssue], new: &[AnalysisIssue]) -> Vec<usize> {
    let known: std::collections::HashSet<(u8, &str)> =
        old.iter().map(|issue| (category_key(issue.category), issue.message.as_str())).collect();
    new.iter()
        .enumerate()
        .filter(|(_, issue)| !known.contains(&(category_key(issue.category), issue.message.as_str())))
        .map(|(index, _)| index)
        .collect()
}

/// Count issues by severity: `(errors, warnings, infos)`.
pub(crate) fn count_by_severity(issues: &[AnalysisIssue]) -> (usize, usize, usize) {
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

/// Format a `RangeStats` as a min/max magnitude interval, or an em dash when empty.
pub(crate) fn format_range(range: &RangeStats) -> String {
    if range.count == 0 { "\u{2014}".to_owned() } else { format!("[{:.2e}, {:.2e}]", range.min, range.max) }
}

/// Ratio of max to min magnitude within a single `RangeStats`, when meaningful.
pub(crate) fn range_ratio(range: &RangeStats) -> Option<f64> {
    if range.count > 0 && range.min > 0.0 && range.max.is_finite() { Some(range.max / range.min) } else { None }
}

/// Format an optional ratio in scientific notation, em dash when absent.
pub(crate) fn format_ratio(ratio: Option<f64>) -> String {
    match ratio {
        Some(value) if value.is_finite() && value > 0.0 => format!("{value:.2e}"),
        _ => "\u{2014}".to_owned(),
    }
}

/// Colour for a range ratio: error above 1e9, warning above 1e6, plain otherwise.
fn ratio_colour(ratio: Option<f64>) -> Color {
    let t = theme();
    match ratio {
        Some(value) if value > RATIO_ERROR_THRESHOLD => t.error,
        Some(value) if value > RATIO_WARN_THRESHOLD => t.warning,
        _ => t.text,
    }
}

/// Number of fill cells in the inline gauge bars.
const GAUGE_CELLS: usize = 6;

/// Decades of ratio magnitude that fill the gauge completely (1e12).
const GAUGE_FULL_SCALE_DECADES: f64 = 12.0;

/// Gauge fraction for a ratio, log-scaled so a ratio of 1e12 fills the bar
/// (the 1e6 warning threshold sits at the halfway mark).
pub(crate) fn ratio_fraction(ratio: Option<f64>) -> Option<f64> {
    let value = ratio?;
    if !value.is_finite() || value < 1.0 {
        return None;
    }
    Some((value.log10() / GAUGE_FULL_SCALE_DECADES).clamp(0.0, 1.0))
}

/// Format a ratio value with a trailing severity gauge, padded to the value column.
fn ratio_cell(ratio: Option<f64>) -> String {
    let value = format_ratio(ratio);
    match ratio_fraction(ratio) {
        Some(fraction) => format!("{:>VALUE_WIDTH$}", format!("{value} {}", gauge_bar(fraction, GAUGE_CELLS))),
        None => format!("{value:>VALUE_WIDTH$}"),
    }
}

/// Format a density value with a trailing linear gauge, padded to the value column.
fn density_cell(density: f64) -> String {
    let value = format!("{:.4}%", density * 100.0);
    format!("{:>VALUE_WIDTH$}", format!("{value} {}", gauge_bar(density, GAUGE_CELLS)))
}

/// Build the cached styled lines for the Numerics detail panel.
pub fn build_numerics_lines(report: &LpDiffReport) -> Vec<Line<'static>> {
    let t = theme();
    let a = &report.analysis1;
    let b = &report.analysis2;
    let mut lines: Vec<Line<'static>> = Vec::with_capacity(48);

    // File header (mirrors the Summary header).
    lines.push(Line::from(vec![
        Span::raw("  "),
        Span::styled(report.file1.clone(), Style::default()),
        Span::styled(ARROW, Style::default().fg(t.muted)),
        Span::styled(report.file2.clone(), Style::default()),
    ]));
    lines.push(Line::from(""));

    // Problem size.
    heading(&mut lines, "Problem Size");
    column_header(&mut lines);
    value_row(&mut lines, "Variables", &a.summary.variable_count.to_string(), &b.summary.variable_count.to_string());
    value_row(&mut lines, "Constraints", &a.summary.constraint_count.to_string(), &b.summary.constraint_count.to_string());
    value_row(&mut lines, "Non-zeros", &a.summary.total_nonzeros.to_string(), &b.summary.total_nonzeros.to_string());
    value_row(&mut lines, "Density", &density_cell(a.summary.density), &density_cell(b.summary.density));
    value_row(
        &mut lines,
        "Vars/constraint",
        &format!("{}\u{2013}{}", a.sparsity.min_vars_per_constraint, a.sparsity.max_vars_per_constraint),
        &format!("{}\u{2013}{}", b.sparsity.min_vars_per_constraint, b.sparsity.max_vars_per_constraint),
    );
    lines.push(Line::from(""));

    // Coefficient magnitude ranges. `CoefficientAnalysis` provides objective and
    // constraint-matrix ranges; the RHS range lives on `ConstraintAnalysis`.
    // (No bounds range exists in the library, so none is shown.)
    heading(&mut lines, "Coefficient Ranges (|value|)");
    column_header(&mut lines);
    range_rows(&mut lines, "Objective", &a.coefficients.objective_coeff_range, &b.coefficients.objective_coeff_range);
    range_rows(&mut lines, "Matrix", &a.coefficients.constraint_coeff_range, &b.coefficients.constraint_coeff_range);
    range_rows(&mut lines, "RHS", &a.constraints.rhs_range, &b.constraints.rhs_range);

    // Conditioning headline: the overall max/min coefficient ratio across the
    // matrix and objective, as computed by the library.
    let overall_a = Some(a.coefficients.coefficient_ratio);
    let overall_b = Some(b.coefficients.coefficient_ratio);
    lines.push(Line::from(vec![
        Span::styled(format!("  {:<LABEL_WIDTH$}", "Overall ratio"), Style::default().fg(t.text).add_modifier(Modifier::BOLD)),
        Span::styled(ratio_cell(overall_a), Style::default().fg(ratio_colour(overall_a))),
        Span::styled(ratio_cell(overall_b), Style::default().fg(ratio_colour(overall_b))),
    ]));
    lines.push(Line::from(vec![Span::styled(
        format!("  ratio > {RATIO_WARN_THRESHOLD:.0e} risks precision loss; > {RATIO_ERROR_THRESHOLD:.0e} is likely to break solvers"),
        Style::default().fg(t.muted),
    )]));
    lines.push(Line::from(""));

    // Issues by severity, then the new-in-file-2 list.
    heading(&mut lines, "Issues");
    issue_count_row(&mut lines, "File A", count_by_severity(&a.issues));
    issue_count_row(&mut lines, "File B", count_by_severity(&b.issues));
    lines.push(Line::from(""));

    let file2_label = short_filename(&report.file2);
    let new_indices = new_issue_indices(&a.issues, &b.issues);
    lines.push(Line::from(vec![Span::styled(
        format!("  New issues in {file2_label}"),
        Style::default().fg(t.accent).add_modifier(Modifier::BOLD),
    )]));
    if new_indices.is_empty() {
        lines.push(Line::from(vec![Span::styled("  none", Style::default().fg(t.muted))]));
    } else {
        for &index in new_indices.iter().take(MAX_NEW_ISSUES) {
            let issue = &b.issues[index];
            lines.push(Line::from(vec![
                Span::styled(format!("  [{:<7}] ", issue.severity.to_string()), Style::default().fg(severity_colour(issue.severity))),
                Span::styled(issue.message.clone(), Style::default().fg(t.text)),
            ]));
        }
        if new_indices.len() > MAX_NEW_ISSUES {
            lines.push(Line::from(vec![Span::styled(
                format!("  \u{2026} ({} more)", new_indices.len() - MAX_NEW_ISSUES),
                Style::default().fg(t.muted),
            )]));
        }
    }

    lines
}

/// Append a bold accent heading line.
fn heading(lines: &mut Vec<Line<'static>>, title: &str) {
    let t = theme();
    lines.push(Line::from(vec![Span::styled(format!("  {title}"), Style::default().fg(t.accent).add_modifier(Modifier::BOLD))]));
}

/// Append the File A / File B column header row with a separator rule.
fn column_header(lines: &mut Vec<Line<'static>>) {
    let t = theme();
    lines.push(Line::from(vec![Span::styled(
        format!("  {:<LABEL_WIDTH$}{:>VALUE_WIDTH$}{:>VALUE_WIDTH$}", "", "File A", "File B"),
        Style::default().fg(t.muted).add_modifier(Modifier::BOLD),
    )]));
    lines.push(Line::from(vec![Span::styled(
        format!("  {}", crate::widgets::rule_str(LABEL_WIDTH + VALUE_WIDTH * 2)),
        Style::default().fg(t.muted),
    )]));
}

/// Append a plain side-by-side value row.
fn value_row(lines: &mut Vec<Line<'static>>, label: &str, a: &str, b: &str) {
    let t = theme();
    lines.push(Line::from(vec![
        Span::styled(format!("  {label:<LABEL_WIDTH$}"), Style::default().fg(t.text)),
        Span::styled(format!("{a:>VALUE_WIDTH$}"), Style::default().fg(t.text)),
        Span::styled(format!("{b:>VALUE_WIDTH$}"), Style::default().fg(t.text)),
    ]));
}

/// Append two rows for a `RangeStats` pair: the min/max interval and its ratio,
/// the ratio coloured by the conditioning thresholds.
fn range_rows(lines: &mut Vec<Line<'static>>, label: &str, a: &RangeStats, b: &RangeStats) {
    let t = theme();
    value_row(lines, label, &format_range(a), &format_range(b));
    let ratio_a = range_ratio(a);
    let ratio_b = range_ratio(b);
    lines.push(Line::from(vec![
        Span::styled(format!("  {:<LABEL_WIDTH$}", format!("{label} ratio")), Style::default().fg(t.muted)),
        Span::styled(ratio_cell(ratio_a), Style::default().fg(ratio_colour(ratio_a))),
        Span::styled(ratio_cell(ratio_b), Style::default().fg(ratio_colour(ratio_b))),
    ]));
}

/// Append an issue-severity count row for one file.
fn issue_count_row(lines: &mut Vec<Line<'static>>, label: &str, (errors, warnings, infos): (usize, usize, usize)) {
    let t = theme();
    let count_style = |count: usize, colour: Color| if count > 0 { Style::default().fg(colour) } else { Style::default().fg(t.muted) };
    lines.push(Line::from(vec![
        Span::styled(format!("  {label}: "), Style::default().fg(t.muted)),
        Span::styled(format!("{errors} error{}", plural(errors)), count_style(errors, t.error)),
        Span::styled(", ", Style::default().fg(t.muted)),
        Span::styled(format!("{warnings} warning{}", plural(warnings)), count_style(warnings, t.warning)),
        Span::styled(", ", Style::default().fg(t.muted)),
        Span::styled(format!("{infos} info{}", plural(infos)), count_style(infos, t.info)),
    ]));
}

const fn plural(count: usize) -> &'static str {
    if count == 1 { "" } else { "s" }
}

fn severity_colour(severity: IssueSeverity) -> Color {
    let t = theme();
    match severity {
        IssueSeverity::Error => t.error,
        IssueSeverity::Warning => t.warning,
        IssueSeverity::Info => t.info,
    }
}

/// Extract the filename from a path string for compact display.
fn short_filename(path: &str) -> String {
    std::path::Path::new(path).file_name().map_or_else(|| path.to_owned(), |name| name.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn issue(severity: IssueSeverity, category: IssueCategory, message: &str) -> AnalysisIssue {
        AnalysisIssue { severity, category, message: message.to_owned(), details: None }
    }

    #[test]
    fn test_new_issues_empty_lists() {
        assert!(new_issue_indices(&[], &[]).is_empty());
    }

    #[test]
    fn test_new_issues_all_new_when_old_empty() {
        let new =
            [issue(IssueSeverity::Warning, IssueCategory::NumericalScaling, "a"), issue(IssueSeverity::Info, IssueCategory::Other, "b")];
        assert_eq!(new_issue_indices(&[], &new), vec![0, 1]);
    }

    #[test]
    fn test_new_issues_identical_lists_yield_none() {
        let old = [issue(IssueSeverity::Warning, IssueCategory::EmptyConstraint, "Constraint 'c1' has no variables")];
        let new = [issue(IssueSeverity::Warning, IssueCategory::EmptyConstraint, "Constraint 'c1' has no variables")];
        assert!(new_issue_indices(&old, &new).is_empty());
    }

    #[test]
    fn test_new_issues_same_message_different_category_is_new() {
        let old = [issue(IssueSeverity::Info, IssueCategory::FixedVariable, "x")];
        let new = [issue(IssueSeverity::Info, IssueCategory::UnusedVariable, "x")];
        assert_eq!(new_issue_indices(&old, &new), vec![0]);
    }

    #[test]
    fn test_new_issues_severity_does_not_affect_identity() {
        // Severity is derived from category + message; identity ignores it.
        let old = [issue(IssueSeverity::Warning, IssueCategory::Other, "same")];
        let new = [issue(IssueSeverity::Error, IssueCategory::Other, "same")];
        assert!(new_issue_indices(&old, &new).is_empty());
    }

    #[test]
    fn test_new_issues_mixed() {
        let old = [
            issue(IssueSeverity::Warning, IssueCategory::NumericalScaling, "ratio 1e7"),
            issue(IssueSeverity::Info, IssueCategory::FixedVariable, "x fixed"),
        ];
        let new = [
            issue(IssueSeverity::Info, IssueCategory::FixedVariable, "x fixed"),
            issue(IssueSeverity::Warning, IssueCategory::NumericalScaling, "ratio 1e9"), // message moved → new
            issue(IssueSeverity::Error, IssueCategory::InvalidBounds, "y bounds"),
        ];
        assert_eq!(new_issue_indices(&old, &new), vec![1, 2]);
    }

    #[test]
    fn test_count_by_severity() {
        let issues = [
            issue(IssueSeverity::Error, IssueCategory::InvalidBounds, "a"),
            issue(IssueSeverity::Warning, IssueCategory::Other, "b"),
            issue(IssueSeverity::Warning, IssueCategory::Other, "c"),
            issue(IssueSeverity::Info, IssueCategory::FixedVariable, "d"),
        ];
        assert_eq!(count_by_severity(&issues), (1, 2, 1));
    }

    #[test]
    fn test_range_ratio() {
        let range = RangeStats { min: 1e-3, max: 1e4, count: 5 };
        let ratio = range_ratio(&range).expect("ratio defined for positive min");
        assert!((ratio - 1e7).abs() < 1.0);
        // Empty range and zero min have no meaningful ratio.
        assert!(range_ratio(&RangeStats::default()).is_none());
        assert!(range_ratio(&RangeStats { min: 0.0, max: 5.0, count: 2 }).is_none());
    }

    #[test]
    fn test_format_helpers() {
        assert_eq!(format_range(&RangeStats::default()), "\u{2014}");
        assert_eq!(format_ratio(None), "\u{2014}");
        assert_eq!(format_ratio(Some(2.5e6)), "2.50e6");
    }

    #[test]
    fn test_ratio_fraction_log_scale() {
        assert!(ratio_fraction(None).is_none());
        assert!(ratio_fraction(Some(0.5)).is_none());
        assert!(ratio_fraction(Some(f64::INFINITY)).is_none());
        let exact = |ratio: f64, expected: f64| {
            let fraction = ratio_fraction(Some(ratio)).expect("fraction defined for ratio >= 1");
            assert!((fraction - expected).abs() < 1e-9, "ratio {ratio} gave fraction {fraction}, expected {expected}");
        };
        exact(1.0, 0.0); // log10(1) = 0
        exact(1e6, 0.5); // warning threshold sits halfway
        exact(1e12, 1.0); // full scale
        exact(1e15, 1.0); // clamped beyond full scale
    }
}
