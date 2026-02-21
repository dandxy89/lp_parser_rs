//! Solver overlay widgets — file picker, progress, results, and error display.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

use crate::app::App;
use crate::solver::{ConstraintDiffRow, SolveDiffResult, VarDiffRow};
use crate::state::{SolveState, SolveTab, SolveViewState};

/// Pre-computed horizontal rule strings to avoid per-frame heap allocations from `repeat()`.
const RULE_60: &str = "──────────────────────────────────────────────────────────────";
const RULE_30: &str = "──────────────────────────────────";
// Variables tab: name_w(24) + value_w(14)*3 + 4 = 70
const RULE_70: &str = "────────────────────────────────────────────────────────────────────────";
// Constraints tab: name_w(22) + value_w(13)*4 + 6 = 80
const RULE_80: &str = "──────────────────────────────────────────────────────────────────────────────────";

/// Draw the solver overlay on top of the current frame, based on the current solve state.
pub fn draw_solve_overlay(frame: &mut Frame, area: Rect, app: &App) {
    debug_assert!(area.width > 0 && area.height > 0, "solve overlay area must be non-zero");
    match &app.solver.state {
        SolveState::Idle => {}
        SolveState::Picking => draw_picker(frame, area, app),
        SolveState::Running { file } => draw_running(frame, area, file),
        SolveState::RunningBoth { file1, file2, result1, result2 } => {
            draw_running_both(frame, area, file1, file2, result1.is_some(), result2.is_some());
        }
        SolveState::Done(result) => draw_done(frame, area, result, &app.solver.view),
        SolveState::DoneBoth(diff) => draw_done_both(frame, area, diff, &app.solver.view),
        SolveState::Failed(error) => draw_failed(frame, area, error),
    }
}

fn draw_picker(frame: &mut Frame, area: Rect, app: &App) {
    let popup = super::centred_rect(area, 60, 10);
    let lines = vec![
        Line::from(""),
        Line::from(Span::styled("  Choose a file to solve:", Style::default().fg(Color::White).add_modifier(Modifier::BOLD))),
        Line::from(""),
        Line::from(vec![
            Span::styled("  [1] ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            Span::styled(app.file1_path.display().to_string(), Style::default().fg(Color::White)),
        ]),
        Line::from(vec![
            Span::styled("  [2] ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            Span::styled(app.file2_path.display().to_string(), Style::default().fg(Color::White)),
        ]),
        Line::from(vec![
            Span::styled("  [3] ", Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD)),
            Span::styled("Both (diff)", Style::default().fg(Color::White)),
        ]),
        Line::from(""),
    ];

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
        .title(Span::styled(" Solve LP ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)));

    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(Clear, popup);
    frame.render_widget(paragraph, popup);
}

fn draw_running(frame: &mut Frame, area: Rect, file: &str) {
    let popup = super::centred_rect(area, 50, 5);
    let lines = vec![
        Line::from(""),
        Line::from(vec![
            Span::styled("  Solving ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            Span::styled(file.to_owned(), Style::default().fg(Color::White)),
            Span::styled("...", Style::default().fg(Color::Yellow)),
        ]),
        Line::from(""),
    ];

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))
        .title(Span::styled(" Solver ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)));

    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(Clear, popup);
    frame.render_widget(paragraph, popup);
}

fn draw_done(frame: &mut Frame, area: Rect, result: &crate::solver::SolveResult, view: &crate::state::SolveViewState) {
    let popup_width = (area.width * 4 / 5).max(60).min(area.width);
    let popup_height = (area.height * 4 / 5).max(20).min(area.height);
    let popup = super::centred_rect(area, popup_width, popup_height);

    let active = view.tab;
    let scroll = view.scroll[active.index()];

    // Build tab bar line.
    let tab_bar = build_tab_bar(active);

    // Build content for the active tab.
    let mut lines = vec![tab_bar, Line::from("")];

    match active {
        SolveTab::Summary => build_summary_tab(&mut lines, result),
        SolveTab::Variables => build_variables_tab(&mut lines, result),
        SolveTab::Constraints => build_constraints_tab(&mut lines, result),
        SolveTab::Log => build_log_tab(&mut lines, result),
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "  1-4: tabs  Tab/S-Tab: cycle  j/k: scroll  y: yank  Esc: close",
        Style::default().fg(Color::DarkGray),
    )));

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Green).add_modifier(Modifier::BOLD))
        .title(Span::styled(" Solve Results ", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)));

    let paragraph = Paragraph::new(lines).block(block).scroll((scroll, 0));
    frame.render_widget(Clear, popup);
    frame.render_widget(paragraph, popup);
}

/// Build the tab bar line with the active tab highlighted.
fn build_tab_bar(active: SolveTab) -> Line<'static> {
    let mut spans = vec![Span::raw("  ")];
    for (i, tab) in SolveTab::ALL.iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled("  ", Style::default().fg(Color::DarkGray)));
        }
        let label = format!("[{}] {}", i + 1, tab.label());
        if *tab == active {
            spans.push(Span::styled(label, Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD | Modifier::UNDERLINED)));
        } else {
            spans.push(Span::styled(label, Style::default().fg(Color::DarkGray)));
        }
    }
    Line::from(spans)
}

fn build_summary_tab<'a>(lines: &mut Vec<Line<'a>>, result: &'a crate::solver::SolveResult) {
    lines.push(Line::from(vec![
        Span::styled("  Status:    ", Style::default().fg(Color::DarkGray)),
        Span::styled(&result.status, status_style(&result.status)),
    ]));

    if let Some(obj) = result.objective_value {
        lines.push(Line::from(vec![
            Span::styled("  Objective: ", Style::default().fg(Color::DarkGray)),
            Span::styled(format!("{obj}"), Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
        ]));
    }

    lines.push(Line::from(vec![
        Span::styled("  Time:      ", Style::default().fg(Color::DarkGray)),
        Span::styled(format!("{:.3}s", result.solve_time.as_secs_f64()), Style::default().fg(Color::Cyan)),
    ]));

    if result.skipped_sos > 0 {
        lines.push(Line::from(vec![
            Span::styled("  Warning:   ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            Span::styled(
                format!("{} SOS constraint(s) skipped (not supported by solver)", result.skipped_sos),
                Style::default().fg(Color::Yellow),
            ),
        ]));
    }

    if !result.variables.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled("  Variables:    ", Style::default().fg(Color::DarkGray)),
            Span::styled(format!("{}", result.variables.len()), Style::default().fg(Color::White)),
        ]));
    }

    if !result.shadow_prices.is_empty() {
        lines.push(Line::from(vec![
            Span::styled("  Constraints:  ", Style::default().fg(Color::DarkGray)),
            Span::styled(format!("{}", result.shadow_prices.len()), Style::default().fg(Color::White)),
        ]));
    }
}

fn build_variables_tab<'a>(lines: &mut Vec<Line<'a>>, result: &'a crate::solver::SolveResult) {
    if result.variables.is_empty() {
        lines.push(Line::from(Span::styled("  No variable values available.", Style::default().fg(Color::DarkGray))));
        return;
    }

    lines.push(Line::from(Span::styled(
        format!("  Variables ({})", result.variables.len()),
        Style::default().fg(Color::DarkGray).add_modifier(Modifier::BOLD),
    )));

    // Header
    lines.push(Line::from(vec![
        Span::styled(format!("  {:<30}", "Name"), Style::default().fg(Color::DarkGray).add_modifier(Modifier::BOLD)),
        Span::styled(format!("{:>12}", "Value"), Style::default().fg(Color::DarkGray).add_modifier(Modifier::BOLD)),
        Span::styled(format!("{:>14}", "Reduced Cost"), Style::default().fg(Color::DarkGray).add_modifier(Modifier::BOLD)),
    ]));
    lines
        .push(Line::from(Span::styled("  ────────────────────────────────────────────────────────", Style::default().fg(Color::DarkGray))));

    let has_reduced_costs = !result.reduced_costs.is_empty();
    for (i, (name, val)) in result.variables.iter().enumerate() {
        let val_style = if val.abs() < 1e-10 { Style::default().fg(Color::DarkGray) } else { Style::default().fg(Color::White) };
        let mut spans =
            vec![Span::styled(format!("  {name:<30}"), Style::default().fg(Color::White)), Span::styled(format!("{val:>12.6}"), val_style)];
        if has_reduced_costs {
            let reduced_cost = result.reduced_costs.get(i).map_or(0.0, |(_, v)| *v);
            let reduced_cost_style =
                if reduced_cost.abs() < 1e-10 { Style::default().fg(Color::DarkGray) } else { Style::default().fg(Color::Yellow) };
            spans.push(Span::styled(format!("{reduced_cost:>14.6}"), reduced_cost_style));
        }
        lines.push(Line::from(spans));
    }
}

fn build_constraints_tab<'a>(lines: &mut Vec<Line<'a>>, result: &'a crate::solver::SolveResult) {
    if result.shadow_prices.is_empty() && result.row_values.is_empty() {
        lines.push(Line::from(Span::styled("  No constraint data available.", Style::default().fg(Color::DarkGray))));
        return;
    }

    lines.push(Line::from(Span::styled(
        format!("  Constraints ({})", result.shadow_prices.len()),
        Style::default().fg(Color::DarkGray).add_modifier(Modifier::BOLD),
    )));

    // Header
    lines.push(Line::from(vec![
        Span::styled(format!("  {:<30}", "Name"), Style::default().fg(Color::DarkGray).add_modifier(Modifier::BOLD)),
        Span::styled(format!("{:>12}", "Activity"), Style::default().fg(Color::DarkGray).add_modifier(Modifier::BOLD)),
        Span::styled(format!("{:>14}", "Shadow Price"), Style::default().fg(Color::DarkGray).add_modifier(Modifier::BOLD)),
    ]));
    lines
        .push(Line::from(Span::styled("  ────────────────────────────────────────────────────────", Style::default().fg(Color::DarkGray))));

    for (i, (name, shadow_price)) in result.shadow_prices.iter().enumerate() {
        let row_value = result.row_values.get(i).map_or(0.0, |(_, v)| *v);
        let row_value_style =
            if row_value.abs() < 1e-10 { Style::default().fg(Color::DarkGray) } else { Style::default().fg(Color::White) };
        let shadow_price_style =
            if shadow_price.abs() < 1e-10 { Style::default().fg(Color::DarkGray) } else { Style::default().fg(Color::Yellow) };
        lines.push(Line::from(vec![
            Span::styled(format!("  {name:<30}"), Style::default().fg(Color::White)),
            Span::styled(format!("{row_value:>12.6}"), row_value_style),
            Span::styled(format!("{shadow_price:>14.6}"), shadow_price_style),
        ]));
    }
}

fn build_log_tab<'a>(lines: &mut Vec<Line<'a>>, result: &'a crate::solver::SolveResult) {
    if result.solver_log.is_empty() {
        lines.push(Line::from(Span::styled("  No solver log available.", Style::default().fg(Color::DarkGray))));
        return;
    }

    const MAX_LOG_LINES: usize = 200;

    lines.push(Line::from(Span::styled("  Solver Log:", Style::default().fg(Color::DarkGray).add_modifier(Modifier::BOLD))));
    lines.push(Line::from(Span::styled("  ────────────────────────────────────────", Style::default().fg(Color::DarkGray))));

    let total = result.solver_log.lines().count();

    if total > MAX_LOG_LINES {
        lines.push(Line::from(Span::styled(
            format!("  ... ({} lines truncated)", total - MAX_LOG_LINES),
            Style::default().fg(Color::Yellow),
        )));
    }

    for log_line in result.solver_log.lines().skip(total.saturating_sub(MAX_LOG_LINES)) {
        lines.push(Line::from(Span::styled(format!("  {log_line}"), Style::default().fg(Color::DarkGray))));
    }
}

fn draw_running_both(frame: &mut Frame, area: Rect, file1: &str, file2: &str, done1: bool, done2: bool) {
    let popup = super::centred_rect(area, 60, 7);
    let icon1 = if done1 { "\u{2713}" } else { "\u{22ef}" };
    let status1 = if done1 { "done" } else { "solving..." };
    let icon2 = if done2 { "\u{2713}" } else { "\u{22ef}" };
    let status2 = if done2 { "done" } else { "solving..." };
    let style_done = Style::default().fg(Color::Green).add_modifier(Modifier::BOLD);
    let style_running = Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD);

    let lines = vec![
        Line::from(""),
        Line::from(vec![
            Span::styled(format!("  {icon1} "), if done1 { style_done } else { style_running }),
            Span::styled(format!("{file1:<30}"), Style::default().fg(Color::White)),
            Span::styled(status1, if done1 { style_done } else { style_running }),
        ]),
        Line::from(vec![
            Span::styled(format!("  {icon2} "), if done2 { style_done } else { style_running }),
            Span::styled(format!("{file2:<30}"), Style::default().fg(Color::White)),
            Span::styled(status2, if done2 { style_done } else { style_running }),
        ]),
        Line::from(""),
    ];

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD))
        .title(Span::styled(" Solver ", Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD)));

    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(Clear, popup);
    frame.render_widget(paragraph, popup);
}

fn draw_done_both(frame: &mut Frame, area: Rect, diff: &SolveDiffResult, view: &SolveViewState) {
    let popup_width = (area.width * 4 / 5).max(60).min(area.width);
    let popup_height = (area.height * 4 / 5).max(20).min(area.height);
    let popup = super::centred_rect(area, popup_width, popup_height);

    let active = view.tab;
    let scroll = view.scroll[active.index()];

    let tab_bar = build_tab_bar(active);
    let mut lines = vec![tab_bar, Line::from("")];

    match active {
        SolveTab::Summary => build_diff_summary_tab(&mut lines, diff),
        SolveTab::Variables => build_diff_variables_tab(&mut lines, diff, view.diff_only),
        SolveTab::Constraints => build_diff_constraints_tab(&mut lines, diff, view.diff_only),
        SolveTab::Log => build_diff_log_tab(&mut lines, diff),
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "  1-4: tabs  Tab/S-Tab: cycle  j/k: scroll  d: toggle diff  y: yank  Esc: close",
        Style::default().fg(Color::DarkGray),
    )));

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD))
        .title(Span::styled(" Solve Comparison ", Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD)));

    let paragraph = Paragraph::new(lines).block(block).scroll((scroll, 0));
    frame.render_widget(Clear, popup);
    frame.render_widget(paragraph, popup);
}

/// Format a delta string. Returns empty spans if no delta.
fn delta_spans(v1: Option<f64>, v2: Option<f64>) -> Vec<Span<'static>> {
    let (a, b) = match (v1, v2) {
        (Some(a), Some(b)) => (a, b),
        _ => return Vec::new(),
    };
    let d = b - a;
    if d.abs() < 1e-10 {
        return Vec::new();
    }
    let sign = if d > 0.0 { "+" } else { "" };
    let colour = if d > 0.0 { Color::Green } else { Color::Red };
    vec![Span::styled(format!("  \u{0394} {sign}{d:.6}"), Style::default().fg(colour))]
}

/// Counts for a diff tab summary (shared between variables and constraints).
struct DiffTabCounts {
    total: usize,
    added: usize,
    removed: usize,
    modified: usize,
}

impl DiffTabCounts {
    /// Format the counts as a human-readable summary string.
    fn summary_label(&self) -> String {
        let mut parts = Vec::new();
        if self.modified > 0 {
            parts.push(format!("{} changed", self.modified));
        }
        if self.added > 0 {
            parts.push(format!("{} added", self.added));
        }
        if self.removed > 0 {
            parts.push(format!("{} removed", self.removed));
        }
        if parts.is_empty() { "no differences".to_owned() } else { parts.join(", ") }
    }

    /// Format as a description for the differences row in the summary tab.
    fn description_parts(&self, entity: &str) -> Vec<String> {
        let mut parts = Vec::new();
        if self.modified > 0 {
            parts.push(format!("{} {entity} changed", self.modified));
        }
        if self.added > 0 {
            parts.push(format!("{} {entity} added", self.added));
        }
        if self.removed > 0 {
            parts.push(format!("{} {entity} removed", self.removed));
        }
        parts
    }
}

/// Count variable-level diff statistics from a solve diff result in a single pass.
fn count_variable_diffs(diff: &SolveDiffResult) -> DiffTabCounts {
    let mut counts = DiffTabCounts { total: diff.variable_diff.len(), added: 0, removed: 0, modified: 0 };
    for row in &diff.variable_diff {
        if row.val1.is_none() {
            counts.added += 1;
        } else if row.val2.is_none() {
            counts.removed += 1;
        } else if row.changed {
            counts.modified += 1;
        }
    }
    counts
}

/// Count constraint-level diff statistics from a solve diff result in a single pass.
fn count_constraint_diffs(diff: &SolveDiffResult) -> DiffTabCounts {
    let mut counts = DiffTabCounts { total: diff.constraint_diff.len(), added: 0, removed: 0, modified: 0 };
    for row in &diff.constraint_diff {
        if row.activity1.is_none() {
            counts.added += 1;
        } else if row.activity2.is_none() {
            counts.removed += 1;
        } else if row.changed {
            counts.modified += 1;
        }
    }
    counts
}

/// Format a count delta span for the diff summary comparison rows.
fn count_delta_span(count1: usize, count2: usize) -> Option<Span<'static>> {
    if count1 == count2 {
        return None;
    }
    #[allow(clippy::cast_possible_wrap)]
    let delta = count2 as i64 - count1 as i64;
    let sign = if delta > 0 { "+" } else { "" };
    let colour = if delta > 0 { Color::Green } else { Color::Red };
    Some(Span::styled(format!("  \u{0394} {sign}{delta}"), Style::default().fg(colour)))
}

/// Render the comparison table header and metrics rows for the diff summary.
fn build_diff_summary_metrics(lines: &mut Vec<Line<'static>>, diff: &SolveDiffResult) {
    let r1 = &diff.result1;
    let r2 = &diff.result2;

    let label_w = 18;
    let col_w = 20;
    lines.push(Line::from(vec![
        Span::styled(format!("  {:<label_w$}", ""), Style::default()),
        Span::styled(format!("{:<col_w$}", "File 1"), Style::default().fg(Color::DarkGray).add_modifier(Modifier::BOLD)),
        Span::styled(format!("{:<col_w$}", "File 2"), Style::default().fg(Color::DarkGray).add_modifier(Modifier::BOLD)),
    ]));
    lines.push(Line::from(Span::styled(format!("  {RULE_60}"), Style::default().fg(Color::DarkGray))));

    // Status.
    lines.push(Line::from(vec![
        Span::styled(format!("  {:<label_w$}", "Status:"), Style::default().fg(Color::DarkGray)),
        Span::styled(format!("{:<col_w$}", &r1.status), status_style(&r1.status)),
        Span::styled(format!("{:<col_w$}", &r2.status), status_style(&r2.status)),
    ]));

    // Objective.
    let obj1_str = r1.objective_value.map_or_else(|| "N/A".to_owned(), |v| format!("{v:.6}"));
    let obj2_str = r2.objective_value.map_or_else(|| "N/A".to_owned(), |v| format!("{v:.6}"));
    let mut objective_spans = vec![
        Span::styled(format!("  {:<label_w$}", "Objective:"), Style::default().fg(Color::DarkGray)),
        Span::styled(format!("{obj1_str:<col_w$}"), Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
        Span::styled(format!("{obj2_str:<col_w$}"), Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
    ];
    objective_spans.extend(delta_spans(r1.objective_value, r2.objective_value));
    lines.push(Line::from(objective_spans));

    // Time.
    let time1 = format!("{:.3}s", r1.solve_time.as_secs_f64());
    let time2 = format!("{:.3}s", r2.solve_time.as_secs_f64());
    lines.push(Line::from(vec![
        Span::styled(format!("  {:<label_w$}", "Time:"), Style::default().fg(Color::DarkGray)),
        Span::styled(format!("{time1:<col_w$}"), Style::default().fg(Color::Cyan)),
        Span::styled(format!("{time2:<col_w$}"), Style::default().fg(Color::Cyan)),
    ]));

    // Variable counts.
    let variable_count1 = r1.variables.len();
    let variable_count2 = r2.variables.len();
    let mut variable_spans = vec![
        Span::styled(format!("  {:<label_w$}", "Variables:"), Style::default().fg(Color::DarkGray)),
        Span::styled(format!("{variable_count1:<col_w$}"), Style::default().fg(Color::White)),
        Span::styled(format!("{variable_count2:<col_w$}"), Style::default().fg(Color::White)),
    ];
    if let Some(delta) = count_delta_span(variable_count1, variable_count2) {
        variable_spans.push(delta);
    }
    lines.push(Line::from(variable_spans));

    // Constraint counts.
    let constraint_count1 = r1.shadow_prices.len();
    let constraint_count2 = r2.shadow_prices.len();
    let mut constraint_spans = vec![
        Span::styled(format!("  {:<label_w$}", "Constraints:"), Style::default().fg(Color::DarkGray)),
        Span::styled(format!("{constraint_count1:<col_w$}"), Style::default().fg(Color::White)),
        Span::styled(format!("{constraint_count2:<col_w$}"), Style::default().fg(Color::White)),
    ];
    if let Some(delta) = count_delta_span(constraint_count1, constraint_count2) {
        constraint_spans.push(delta);
    }
    lines.push(Line::from(constraint_spans));

    // Skipped SOS.
    if r1.skipped_sos > 0 || r2.skipped_sos > 0 {
        lines.push(Line::from(vec![
            Span::styled(format!("  {:<label_w$}", "Skipped SOS:"), Style::default().fg(Color::DarkGray)),
            Span::styled(format!("{:<col_w$}", r1.skipped_sos), Style::default().fg(Color::White)),
            Span::styled(format!("{:<col_w$}", r2.skipped_sos), Style::default().fg(Color::White)),
        ]));
    }
}

fn build_diff_summary_tab(lines: &mut Vec<Line<'static>>, diff: &SolveDiffResult) {
    build_diff_summary_metrics(lines, diff);

    // Summary of differences.
    lines.push(Line::from(""));

    let variable_counts = count_variable_diffs(diff);
    let constraint_counts = count_constraint_diffs(diff);

    let mut parts = variable_counts.description_parts("variables");
    parts.extend(constraint_counts.description_parts("constraints"));

    let label_w = 18;
    let summary = if parts.is_empty() { "No differences".to_owned() } else { parts.join(", ") };
    lines.push(Line::from(vec![
        Span::styled(format!("  {:<label_w$}", "Differences:"), Style::default().fg(Color::DarkGray)),
        Span::styled(summary, Style::default().fg(Color::Yellow)),
    ]));
}

/// Render the diff-only toggle label for tabs with filtering.
fn diff_filter_label(diff_only: bool) -> &'static str {
    if diff_only { " (showing changed only, press d for all)" } else { " (showing all, press d for changed only)" }
}

fn build_diff_variables_tab(lines: &mut Vec<Line<'static>>, diff: &SolveDiffResult, diff_only: bool) {
    let counts = count_variable_diffs(diff);
    let summary = counts.summary_label();
    let filter_label = diff_filter_label(diff_only);

    lines.push(Line::from(Span::styled(
        format!("  Variables: {summary} (of {} total){filter_label}", counts.total),
        Style::default().fg(Color::DarkGray).add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(""));

    let name_w = 24;
    let value_w = 14;
    lines.push(Line::from(vec![
        Span::styled(format!("  {:<name_w$}", "Name"), Style::default().fg(Color::DarkGray).add_modifier(Modifier::BOLD)),
        Span::styled(format!("{:>value_w$}", "File 1"), Style::default().fg(Color::DarkGray).add_modifier(Modifier::BOLD)),
        Span::styled(format!("{:>value_w$}", "File 2"), Style::default().fg(Color::DarkGray).add_modifier(Modifier::BOLD)),
        Span::styled(format!("{:>value_w$}", "\u{0394}"), Style::default().fg(Color::DarkGray).add_modifier(Modifier::BOLD)),
    ]));
    lines.push(Line::from(Span::styled(format!("  {RULE_70}"), Style::default().fg(Color::DarkGray))));

    for row in &diff.variable_diff {
        if diff_only && !row.changed {
            continue;
        }
        build_variable_diff_line(lines, row, name_w, value_w);
    }
}

fn build_variable_diff_line(lines: &mut Vec<Line<'static>>, row: &VarDiffRow, name_w: usize, value_w: usize) {
    let dash = "\u{2014}";

    let (name_style, value1_str, value2_str, delta_str, marker) = match (row.val1, row.val2) {
        (None, Some(v2)) => (
            Style::default().fg(Color::Green),
            format!("{dash:>value_w$}"),
            format!("{v2:>value_w$.6}"),
            format!("{:>value_w$}", "(added)"),
            "",
        ),
        (Some(v1), None) => (
            Style::default().fg(Color::Red),
            format!("{v1:>value_w$.6}"),
            format!("{dash:>value_w$}"),
            format!("{:>value_w$}", "(removed)"),
            "",
        ),
        (Some(v1), Some(v2)) => {
            if row.changed {
                let d = v2 - v1;
                let sign = if d >= 0.0 { "+" } else { "" };
                (
                    Style::default().fg(Color::Yellow),
                    format!("{v1:>value_w$.6}"),
                    format!("{v2:>value_w$.6}"),
                    format!("{sign}{d:>.6}"),
                    " *",
                )
            } else {
                let base = if v1.abs() < 1e-10 { Style::default().fg(Color::DarkGray) } else { Style::default().fg(Color::White) };
                (base, format!("{v1:>value_w$.6}"), format!("{v2:>value_w$.6}"), String::new(), "")
            }
        }
        (None, None) => return,
    };

    let mut spans = vec![
        Span::styled(format!("  {:<name_w$}", row.name), name_style),
        Span::styled(value1_str, name_style),
        Span::styled(format!("  {value2_str}"), name_style),
    ];
    if !delta_str.is_empty() {
        spans.push(Span::styled(format!("  {delta_str}"), name_style));
    }
    if !marker.is_empty() {
        spans.push(Span::styled(marker.to_owned(), Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)));
    }
    lines.push(Line::from(spans));
}

fn build_diff_constraints_tab(lines: &mut Vec<Line<'static>>, diff: &SolveDiffResult, diff_only: bool) {
    let counts = count_constraint_diffs(diff);
    let summary = counts.summary_label();
    let filter_label = diff_filter_label(diff_only);

    lines.push(Line::from(Span::styled(
        format!("  Constraints: {summary} (of {} total){filter_label}", counts.total),
        Style::default().fg(Color::DarkGray).add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(""));

    let name_w = 22;
    let value_w = 13;
    lines.push(Line::from(vec![
        Span::styled(format!("  {:<name_w$}", "Name"), Style::default().fg(Color::DarkGray).add_modifier(Modifier::BOLD)),
        Span::styled(format!("{:>value_w$}", "Activity 1"), Style::default().fg(Color::DarkGray).add_modifier(Modifier::BOLD)),
        Span::styled(format!("{:>value_w$}", "Activity 2"), Style::default().fg(Color::DarkGray).add_modifier(Modifier::BOLD)),
        Span::styled(format!("{:>value_w$}", "Shadow 1"), Style::default().fg(Color::DarkGray).add_modifier(Modifier::BOLD)),
        Span::styled(format!("{:>value_w$}", "Shadow 2"), Style::default().fg(Color::DarkGray).add_modifier(Modifier::BOLD)),
    ]));
    lines.push(Line::from(Span::styled(format!("  {RULE_80}"), Style::default().fg(Color::DarkGray))));

    for row in &diff.constraint_diff {
        if diff_only && !row.changed {
            continue;
        }
        build_constraint_diff_line(lines, row, name_w, value_w);
    }
}

fn build_constraint_diff_line(lines: &mut Vec<Line<'static>>, row: &ConstraintDiffRow, name_w: usize, val_w: usize) {
    let dash = "\u{2014}";

    let (name_style, a1, a2, s1, s2, marker) = match (row.activity1, row.activity2) {
        (None, Some(_)) => {
            let style = Style::default().fg(Color::Green);
            (
                style,
                format!("{dash:>val_w$}"),
                row.activity2.map_or_else(String::new, |v| format!("{v:>val_w$.4}")),
                format!("{dash:>val_w$}"),
                row.shadow_price2.map_or_else(String::new, |v| format!("{v:>val_w$.4}")),
                "",
            )
        }
        (Some(_), None) => {
            let style = Style::default().fg(Color::Red);
            (
                style,
                row.activity1.map_or_else(String::new, |v| format!("{v:>val_w$.4}")),
                format!("{dash:>val_w$}"),
                row.shadow_price1.map_or_else(String::new, |v| format!("{v:>val_w$.4}")),
                format!("{dash:>val_w$}"),
                "",
            )
        }
        (Some(act1), Some(act2)) => {
            if row.changed {
                (
                    Style::default().fg(Color::Yellow),
                    format!("{act1:>val_w$.4}"),
                    format!("{act2:>val_w$.4}"),
                    row.shadow_price1.map_or_else(String::new, |v| format!("{v:>val_w$.4}")),
                    row.shadow_price2.map_or_else(String::new, |v| format!("{v:>val_w$.4}")),
                    " *",
                )
            } else {
                let base = if act1.abs() < 1e-10 { Style::default().fg(Color::DarkGray) } else { Style::default().fg(Color::White) };
                (
                    base,
                    format!("{act1:>val_w$.4}"),
                    format!("{act2:>val_w$.4}"),
                    row.shadow_price1.map_or_else(String::new, |v| format!("{v:>val_w$.4}")),
                    row.shadow_price2.map_or_else(String::new, |v| format!("{v:>val_w$.4}")),
                    "",
                )
            }
        }
        (None, None) => return,
    };

    let mut spans = vec![
        Span::styled(format!("  {:<name_w$}", row.name), name_style),
        Span::styled(a1, name_style),
        Span::styled(format!("  {a2}"), name_style),
        Span::styled(format!("  {s1}"), name_style),
        Span::styled(format!("  {s2}"), name_style),
    ];
    if !marker.is_empty() {
        spans.push(Span::styled(marker.to_owned(), Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)));
    }
    lines.push(Line::from(spans));
}

fn build_diff_log_tab(lines: &mut Vec<Line<'static>>, diff: &SolveDiffResult) {
    const MAX_LOG_LINES: usize = 100;

    // File 1 log.
    lines.push(Line::from(Span::styled(
        format!("  \u{2500}\u{2500} File 1: {} \u{2500}{RULE_30}", diff.file1_label),
        Style::default().fg(Color::DarkGray).add_modifier(Modifier::BOLD),
    )));

    let total1 = diff.result1.solver_log.lines().count();
    if total1 > MAX_LOG_LINES {
        lines.push(Line::from(Span::styled(
            format!("  ... ({} lines truncated)", total1 - MAX_LOG_LINES),
            Style::default().fg(Color::Yellow),
        )));
    }
    for log_line in diff.result1.solver_log.lines().skip(total1.saturating_sub(MAX_LOG_LINES)) {
        lines.push(Line::from(Span::styled(format!("  {log_line}"), Style::default().fg(Color::DarkGray))));
    }

    lines.push(Line::from(""));

    // File 2 log.
    lines.push(Line::from(Span::styled(
        format!("  \u{2500}\u{2500} File 2: {} \u{2500}{RULE_30}", diff.file2_label),
        Style::default().fg(Color::DarkGray).add_modifier(Modifier::BOLD),
    )));

    let total2 = diff.result2.solver_log.lines().count();
    if total2 > MAX_LOG_LINES {
        lines.push(Line::from(Span::styled(
            format!("  ... ({} lines truncated)", total2 - MAX_LOG_LINES),
            Style::default().fg(Color::Yellow),
        )));
    }
    for log_line in diff.result2.solver_log.lines().skip(total2.saturating_sub(MAX_LOG_LINES)) {
        lines.push(Line::from(Span::styled(format!("  {log_line}"), Style::default().fg(Color::DarkGray))));
    }
}

fn draw_failed(frame: &mut Frame, area: Rect, err: &str) {
    let popup = super::centred_rect(area, 60, 8);
    let lines = vec![
        Line::from(""),
        Line::from(Span::styled("  Solve failed:", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD))),
        Line::from(""),
        Line::from(Span::styled(format!("  {err}"), Style::default().fg(Color::Red))),
        Line::from(""),
        Line::from(Span::styled("  Press Esc to close", Style::default().fg(Color::DarkGray))),
    ];

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Red).add_modifier(Modifier::BOLD))
        .title(Span::styled(" Solver Error ", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)));

    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(Clear, popup);
    frame.render_widget(paragraph, popup);
}

/// Pick a style colour based on the status string.
fn status_style(status: &str) -> Style {
    if status.contains("Optimal") {
        Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)
    } else if status.contains("Infeasible") || status.contains("Unbounded") {
        Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
    }
}
