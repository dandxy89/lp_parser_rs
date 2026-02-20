//! Summary detail widget.
//!
//! Shows file paths, optional problem-name and sense changes, then a
//! table of per-section change counts. Rendered inside the detail panel
//! when the Summary section is active.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::diff_model::{DiffCounts, DiffSummary, LpDiffReport};

/// Draw the summary content into `area` (no border — caller provides the border).
pub fn draw_summary(frame: &mut Frame, area: Rect, report: &LpDiffReport, summary: &DiffSummary) {
    let chunks = Layout::vertical([
        Constraint::Length(4), // file-path / sense / name header
        Constraint::Length(1), // column headings
        Constraint::Length(3), // three section rows
        Constraint::Length(1), // separator
        Constraint::Length(1), // totals row
        Constraint::Min(0),    // bottom padding
    ])
    .split(area);

    draw_header(frame, chunks[0], report);
    draw_column_headings(frame, chunks[1]);
    draw_section_rows(frame, chunks[2], summary);
    draw_separator(frame, chunks[3]);
    draw_totals_row(frame, chunks[4], summary);
}

/// Header block: file paths and any sense / name changes.
fn draw_header(frame: &mut Frame, area: Rect, report: &LpDiffReport) {
    let mut lines = vec![Line::from(vec![
        Span::styled("  ", Style::default()),
        Span::styled(report.file1.as_str(), Style::default()),
        Span::styled("  \u{2192}  ", Style::default().fg(Color::DarkGray)),
        Span::styled(report.file2.as_str(), Style::default()),
    ])];

    if let Some((ref old, ref new)) = report.name_changed {
        let old_name = old.as_deref().unwrap_or("(unnamed)");
        let new_name = new.as_deref().unwrap_or("(unnamed)");
        lines.push(Line::from(vec![
            Span::styled("  Name:   ", Style::default().fg(Color::DarkGray)),
            Span::styled(format!("\"{old_name}\""), Style::default().fg(Color::Red)),
            Span::styled("  \u{2192}  ", Style::default().fg(Color::DarkGray)),
            Span::styled(format!("\"{new_name}\""), Style::default().fg(Color::Green)),
        ]));
    }

    if let Some((ref old_sense, ref new_sense)) = report.sense_changed {
        lines.push(Line::from(vec![
            Span::styled("  Sense:  ", Style::default().fg(Color::DarkGray)),
            Span::styled(format!("{old_sense}"), Style::default().fg(Color::Red)),
            Span::styled("  \u{2192}  ", Style::default().fg(Color::DarkGray)),
            Span::styled(format!("{new_sense}"), Style::default().fg(Color::Green)),
        ]));
    }

    frame.render_widget(Paragraph::new(lines), area);
}

/// Fixed-width column headings for the counts table.
fn draw_column_headings(frame: &mut Frame, area: Rect) {
    let heading = Line::from(vec![Span::styled(
        format!("  {:<14}{:>7}{:>9}{:>12}{:>9}", "Section", "Added", "Removed", "Modified", "Total"),
        Style::default().fg(Color::DarkGray).add_modifier(Modifier::BOLD),
    )]);
    frame.render_widget(Paragraph::new(heading), area);
}

/// The three section rows (Variables, Constraints, Objectives) — static, not selectable.
fn draw_section_rows(frame: &mut Frame, area: Rect, summary: &DiffSummary) {
    let lines: Vec<Line> = [("Variables", summary.variables), ("Constraints", summary.constraints), ("Objectives", summary.objectives)]
        .iter()
        .map(|(label, counts)| format_count_row(label, counts, false))
        .collect();

    frame.render_widget(Paragraph::new(lines), area);
}

/// Thin horizontal separator between the section rows and the totals row.
fn draw_separator(frame: &mut Frame, area: Rect) {
    let sep = Paragraph::new(Line::from(vec![Span::styled(
        "  ────────────────────────────────────────────────",
        Style::default().fg(Color::DarkGray),
    )]));
    frame.render_widget(sep, area);
}

/// Bold totals row below the separator.
fn draw_totals_row(frame: &mut Frame, area: Rect, summary: &DiffSummary) {
    let totals = DiffCounts {
        added: summary.variables.added + summary.constraints.added + summary.objectives.added,
        removed: summary.variables.removed + summary.constraints.removed + summary.objectives.removed,
        modified: summary.variables.modified + summary.constraints.modified + summary.objectives.modified,
        unchanged: summary.variables.unchanged + summary.constraints.unchanged + summary.objectives.unchanged,
    };
    frame.render_widget(Paragraph::new(format_count_row("TOTAL", &totals, true)), area);
}

/// Format a single counts row with fixed-width columns.
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
