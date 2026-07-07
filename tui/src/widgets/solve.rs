//! Solver overlay widgets — file picker, progress, results, and error display.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph};

use crate::app::{App, CachedDiffRow, SolveRenderCache};
use crate::solver::{
    DiffCounts, SolveDiffResult, SolveResult, VarDiffRow, rank_by_magnitude, rank_constraints_by_shadow_delta,
    rank_variables_by_reduced_cost_delta, status_is_infeasible,
};
use crate::state::{DiagnosisState, SolveState, SolveTab, SolveViewState};
use crate::theme::theme;
use crate::widgets::{panel_block, rule_str, spinner_frame, truncate_with_ellipsis, zebra_style};

/// Name column width for a table: the space left after `fixed` columns,
/// clamped so very narrow or very wide terminals stay readable.
fn name_column_width(inner_width: u16, fixed: usize) -> usize {
    (inner_width as usize).saturating_sub(fixed).clamp(12, 48)
}

/// Draw the solver overlay on top of the current frame, based on the current solve state.
pub fn draw_solve_overlay(frame: &mut Frame, area: Rect, app: &App) {
    // A zero-sized area is an environmental condition (shrunken terminal), not a
    // programming error: drawing into it is a no-op.
    if area.width == 0 || area.height == 0 {
        return;
    }
    match &app.solver.state {
        SolveState::Idle => {}
        SolveState::Picking => draw_picker(frame, area, app),
        SolveState::Running { file, started } => draw_running(frame, area, file, started.elapsed()),
        SolveState::RunningBoth { file1, file2, result1, result2, started } => {
            draw_running_both(frame, area, file1, file2, result1.is_some(), result2.is_some(), started.elapsed());
        }
        SolveState::Done(result) => draw_done(frame, area, result, &app.solver.view, &app.solver.render_cache, &app.solver.diagnosis),
        SolveState::DoneBoth(diff) => draw_done_both(frame, area, diff, &app.solver.view, &app.solver.render_cache, &app.solver.diagnosis),
        SolveState::Failed(error) => draw_failed(frame, area, error),
    }
}

fn draw_picker(frame: &mut Frame, area: Rect, app: &App) {
    let t = theme();
    let popup = super::centred_rect(area, 60, 10);
    let lines = vec![
        Line::from(""),
        Line::from(Span::styled("  Choose a file to solve:", Style::default().fg(t.text).add_modifier(Modifier::BOLD))),
        Line::from(""),
        Line::from(vec![
            Span::styled("  [1] ", Style::default().fg(t.accent).add_modifier(Modifier::BOLD)),
            Span::styled(app.file1_path.display().to_string(), Style::default().fg(t.text)),
        ]),
        Line::from(vec![
            Span::styled("  [2] ", Style::default().fg(t.accent).add_modifier(Modifier::BOLD)),
            Span::styled(app.file2_path.display().to_string(), Style::default().fg(t.text)),
        ]),
        Line::from(vec![
            Span::styled("  [3] ", Style::default().fg(t.secondary_accent).add_modifier(Modifier::BOLD)),
            Span::styled("Both (diff)", Style::default().fg(t.text)),
        ]),
        Line::from(""),
    ];

    let block = panel_block(Style::default().fg(t.accent).add_modifier(Modifier::BOLD))
        .title(Span::styled(" Solve LP ", Style::default().fg(t.accent).add_modifier(Modifier::BOLD)));

    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(Clear, popup);
    frame.render_widget(paragraph, popup);
}

fn draw_running(frame: &mut Frame, area: Rect, file: &str, elapsed: std::time::Duration) {
    let t = theme();
    let popup = super::centred_rect(area, 50, 5);
    let lines = vec![
        Line::from(""),
        Line::from(vec![
            Span::styled(format!("  {} Solving ", spinner_frame(elapsed)), Style::default().fg(t.warning).add_modifier(Modifier::BOLD)),
            Span::styled(file.to_owned(), Style::default().fg(t.text)),
            Span::styled(format!(" ({}s)", elapsed.as_secs()), Style::default().fg(t.warning)),
        ]),
        Line::from(""),
    ];

    let block = panel_block(Style::default().fg(t.warning).add_modifier(Modifier::BOLD))
        .title(Span::styled(" Solver ", Style::default().fg(t.warning).add_modifier(Modifier::BOLD)));

    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(Clear, popup);
    frame.render_widget(paragraph, popup);
}

fn draw_done(
    frame: &mut Frame,
    area: Rect,
    result: &SolveResult,
    view: &SolveViewState,
    cache: &SolveRenderCache,
    diagnosis: &DiagnosisState,
) {
    let t = theme();
    let popup_width = (area.width * 4 / 5).max(60).min(area.width);
    let popup_height = (area.height * 4 / 5).max(20).min(area.height);
    let popup = super::centred_rect(area, popup_width, popup_height);

    let active = view.tab;
    let scroll = view.scroll[active.index()];

    // Dynamic lines rebuilt each frame. Kept in owned locals so the reference
    // list below can borrow them alongside the cached tab lines.
    let tab_bar = build_tab_bar(active);
    let blank = Line::from("");
    let footer = Line::from(Span::styled("  1-5: tabs  Tab/S-Tab: cycle  j/k: scroll  y: yank  Esc: close", Style::default().fg(t.muted)));

    // Prefer cached tab lines; only build from scratch when the cache is absent
    // (should not happen in normal flow).
    let mut fallback: Vec<Line<'static>> = Vec::new();
    let cached: &[Line<'static>] = if let SolveRenderCache::Single(tabs) = cache {
        &tabs[active.index()]
    } else {
        let inner_width = popup_width.saturating_sub(2);
        match active {
            SolveTab::Summary => build_summary_tab(&mut fallback, result),
            SolveTab::Variables => build_variables_tab(&mut fallback, result, name_column_width(inner_width, 2 + 12 + 14)),
            SolveTab::Constraints => build_constraints_tab(&mut fallback, result, name_column_width(inner_width, 2 + 12 + 14)),
            SolveTab::Log => build_log_tab(&mut fallback, result),
            SolveTab::Duals => build_duals_tab(&mut fallback, result, name_column_width(inner_width, 2 + 14)),
        }
        &fallback
    };

    // The diagnosis block changes while the background solve runs, so it is
    // appended dynamically rather than baked into the render cache.
    let mut diag: Vec<Line<'static>> = Vec::new();
    if active == SolveTab::Summary && status_is_infeasible(&result.status) {
        append_diagnosis_block(&mut diag, diagnosis);
    }

    // Assemble the render order as references — cached line content is never cloned.
    let mut lines: Vec<&Line> = Vec::with_capacity(cached.len() + diag.len() + 4);
    lines.push(&tab_bar);
    lines.push(&blank);
    lines.extend(cached.iter());
    lines.extend(diag.iter());
    lines.push(&blank);
    lines.push(&footer);

    let block = panel_block(Style::default().fg(t.added).add_modifier(Modifier::BOLD))
        .title(Span::styled(" Solve Results ", Style::default().fg(t.added).add_modifier(Modifier::BOLD)));
    let inner = block.inner(popup);
    frame.render_widget(Clear, popup);
    frame.render_widget(block, popup);

    // Windowed render honouring vertical scroll (same idiom as `draw_summary`),
    // clipping long lines to width like the previous unwrapped `Paragraph`.
    let buf = frame.buffer_mut();
    for (i, line) in lines.iter().skip(scroll as usize).take(inner.height as usize).enumerate() {
        #[allow(clippy::cast_possible_truncation)] // i < inner.height (u16)
        let y = inner.y + i as u16;
        buf.set_line(inner.x, y, line, inner.width);
    }
}

/// Build the tab bar line with the active tab highlighted.
fn build_tab_bar(active: SolveTab) -> Line<'static> {
    let t = theme();
    let mut spans = vec![Span::raw("  ")];
    for (i, tab) in SolveTab::ALL.iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled("  ", Style::default().fg(t.muted)));
        }
        let label = format!("[{}] {}", i + 1, tab.label());
        if *tab == active {
            spans.push(Span::styled(label, Style::default().fg(t.accent).add_modifier(Modifier::BOLD | Modifier::UNDERLINED)));
        } else {
            spans.push(Span::styled(label, Style::default().fg(t.muted)));
        }
    }
    Line::from(spans)
}

fn build_summary_tab(lines: &mut Vec<Line<'static>>, result: &SolveResult) {
    let t = theme();
    lines.push(Line::from(vec![
        Span::styled("  Status:    ", Style::default().fg(t.muted)),
        Span::styled(result.status.clone(), status_style(&result.status)),
    ]));

    if let Some(obj) = result.objective_value {
        lines.push(Line::from(vec![
            Span::styled("  Objective: ", Style::default().fg(t.muted)),
            Span::styled(format!("{obj}"), Style::default().fg(t.text).add_modifier(Modifier::BOLD)),
        ]));
    }

    if result.skipped_sos > 0 {
        lines.push(Line::from(vec![
            Span::styled("  Warning:   ", Style::default().fg(t.warning).add_modifier(Modifier::BOLD)),
            Span::styled(
                format!("{} SOS constraint(s) skipped — solution may not satisfy them", result.skipped_sos),
                Style::default().fg(t.warning),
            ),
        ]));
    }

    if !result.variables.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled("  Variables:    ", Style::default().fg(t.muted)),
            Span::styled(format!("{}", result.variables.len()), Style::default().fg(t.text)),
        ]));
    }

    if !result.shadow_prices.is_empty() {
        lines.push(Line::from(vec![
            Span::styled("  Constraints:  ", Style::default().fg(t.muted)),
            Span::styled(format!("{}", result.shadow_prices.len()), Style::default().fg(t.text)),
        ]));
    }

    // Timing breakdown.
    let total = result.build_time + result.solve_time + result.extract_time;
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled("  Timings", Style::default().fg(t.muted).add_modifier(Modifier::BOLD))));
    lines.push(Line::from(Span::styled(format!("  {}", rule_str(38)), Style::default().fg(t.muted))));
    lines.push(Line::from(vec![
        Span::styled("  Build:         ", Style::default().fg(t.muted)),
        Span::styled(format!("{:.3}s", result.build_time.as_secs_f64()), Style::default().fg(t.accent)),
    ]));
    lines.push(Line::from(vec![
        Span::styled("  Solve:         ", Style::default().fg(t.muted)),
        Span::styled(format!("{:.3}s", result.solve_time.as_secs_f64()), Style::default().fg(t.accent)),
    ]));
    lines.push(Line::from(vec![
        Span::styled("  Extract:       ", Style::default().fg(t.muted)),
        Span::styled(format!("{:.3}s", result.extract_time.as_secs_f64()), Style::default().fg(t.accent)),
    ]));
    lines.push(Line::from(vec![
        Span::styled("  Total:         ", Style::default().fg(t.muted)),
        Span::styled(format!("{:.3}s", total.as_secs_f64()), Style::default().fg(t.text).add_modifier(Modifier::BOLD)),
    ]));
}

fn build_variables_tab(lines: &mut Vec<Line<'static>>, result: &SolveResult, name_w: usize) {
    let t = theme();
    if result.variables.is_empty() {
        lines.push(Line::from(Span::styled("  No variable values available.", Style::default().fg(t.muted))));
        return;
    }

    lines.push(Line::from(Span::styled(
        format!("  Variables ({})", result.variables.len()),
        Style::default().fg(t.muted).add_modifier(Modifier::BOLD),
    )));

    // Header
    lines.push(Line::from(vec![
        Span::styled(format!("  {:<name_w$}", "Name"), Style::default().fg(t.muted).add_modifier(Modifier::BOLD)),
        Span::styled(format!("{:>12}", "Value"), Style::default().fg(t.muted).add_modifier(Modifier::BOLD)),
        Span::styled(format!("{:>14}", "Reduced Cost"), Style::default().fg(t.muted).add_modifier(Modifier::BOLD)),
    ]));
    lines.push(Line::from(Span::styled(format!("  {}", rule_str(name_w + 26)), Style::default().fg(t.muted))));

    let has_reduced_costs = !result.reduced_costs.is_empty();
    for (i, (name, val)) in result.variables.iter().enumerate() {
        let val_style = if val.abs() < 1e-10 { Style::default().fg(t.muted) } else { Style::default().fg(t.text) };
        let mut spans = vec![
            Span::styled(format!("  {:<name_w$}", truncate_with_ellipsis(name, name_w)), Style::default().fg(t.text)),
            Span::styled(format!("{val:>12.6}"), val_style),
        ];
        if has_reduced_costs {
            let reduced_cost = result.reduced_costs.get(i).map_or(0.0, |(_, v)| *v);
            let reduced_cost_style =
                if reduced_cost.abs() < 1e-10 { Style::default().fg(t.muted) } else { Style::default().fg(t.modified) };
            spans.push(Span::styled(format!("{reduced_cost:>14.6}"), reduced_cost_style));
        }
        lines.push(Line::from(spans).style(zebra_style(i)));
    }
}

fn build_constraints_tab(lines: &mut Vec<Line<'static>>, result: &SolveResult, name_w: usize) {
    let t = theme();
    if result.shadow_prices.is_empty() && result.row_values.is_empty() {
        lines.push(Line::from(Span::styled("  No constraint data available.", Style::default().fg(t.muted))));
        return;
    }

    lines.push(Line::from(Span::styled(
        format!("  Constraints ({})", result.shadow_prices.len()),
        Style::default().fg(t.muted).add_modifier(Modifier::BOLD),
    )));

    // Header
    lines.push(Line::from(vec![
        Span::styled(format!("  {:<name_w$}", "Name"), Style::default().fg(t.muted).add_modifier(Modifier::BOLD)),
        Span::styled(format!("{:>12}", "Activity"), Style::default().fg(t.muted).add_modifier(Modifier::BOLD)),
        Span::styled(format!("{:>14}", "Shadow Price"), Style::default().fg(t.muted).add_modifier(Modifier::BOLD)),
    ]));
    lines.push(Line::from(Span::styled(format!("  {}", rule_str(name_w + 26)), Style::default().fg(t.muted))));

    for (i, (name, shadow_price)) in result.shadow_prices.iter().enumerate() {
        let row_value = result.row_values.get(i).map_or(0.0, |(_, v)| *v);
        let row_value_style = if row_value.abs() < 1e-10 { Style::default().fg(t.muted) } else { Style::default().fg(t.text) };
        let shadow_price_style = if shadow_price.abs() < 1e-10 { Style::default().fg(t.muted) } else { Style::default().fg(t.modified) };
        lines.push(
            Line::from(vec![
                Span::styled(format!("  {:<name_w$}", truncate_with_ellipsis(name, name_w)), Style::default().fg(t.text)),
                Span::styled(format!("{row_value:>12.6}"), row_value_style),
                Span::styled(format!("{shadow_price:>14.6}"), shadow_price_style),
            ])
            .style(zebra_style(i)),
        );
    }
}

fn build_log_tab(lines: &mut Vec<Line<'static>>, result: &SolveResult) {
    const MAX_LOG_LINES: usize = 200;

    let t = theme();
    if result.solver_log.is_empty() {
        lines.push(Line::from(Span::styled("  No solver log available.", Style::default().fg(t.muted))));
        return;
    }

    lines.push(Line::from(Span::styled("  Solver Log:", Style::default().fg(t.muted).add_modifier(Modifier::BOLD))));
    lines.push(Line::from(Span::styled(format!("  {}", rule_str(38)), Style::default().fg(t.muted))));

    // Count total lines first, then skip/take to avoid collecting into a Vec.
    let total_lines = result.solver_log.lines().count();
    let skip = total_lines.saturating_sub(MAX_LOG_LINES);

    if total_lines > MAX_LOG_LINES {
        lines.push(Line::from(Span::styled(
            format!("  ... ({} lines truncated)", total_lines - MAX_LOG_LINES),
            Style::default().fg(t.warning),
        )));
    }

    for log_line in result.solver_log.lines().skip(skip) {
        lines.push(Line::from(Span::styled(format!("  {log_line}"), Style::default().fg(t.muted))));
    }
}

/// Maximum rows shown per ranked table in the Duals tab.
const MAX_DUAL_ROWS: usize = 100;

/// Maximum violated constraints shown in the infeasibility diagnosis block.
const MAX_VIOLATION_ROWS: usize = 15;

/// Build the single-solve Duals tab: constraints ranked by `|shadow price|`
/// (which constraints bind hardest) and variables ranked by `|reduced cost|`
/// (which variables are most expensive to force into the basis).
fn build_duals_tab(lines: &mut Vec<Line<'static>>, result: &SolveResult, name_w: usize) {
    let t = theme();
    if result.shadow_prices.is_empty() && result.reduced_costs.is_empty() {
        lines.push(Line::from(Span::styled("  No dual values available.", Style::default().fg(t.muted))));
        return;
    }

    let value_w = 14;

    // Constraints ranked by |shadow price|.
    lines.push(Line::from(Span::styled("  Constraints by |shadow price|", Style::default().fg(t.muted).add_modifier(Modifier::BOLD))));
    lines.push(Line::from(vec![
        Span::styled(format!("  {:<name_w$}", "Name"), Style::default().fg(t.muted).add_modifier(Modifier::BOLD)),
        Span::styled(format!("{:>value_w$}", "Shadow Price"), Style::default().fg(t.muted).add_modifier(Modifier::BOLD)),
    ]));
    lines.push(Line::from(Span::styled(format!("  {}", rule_str(name_w + value_w)), Style::default().fg(t.muted))));
    append_magnitude_ranked_rows(lines, &result.shadow_prices, name_w, value_w);

    lines.push(Line::from(""));

    // Variables ranked by |reduced cost|.
    lines.push(Line::from(Span::styled("  Variables by |reduced cost|", Style::default().fg(t.muted).add_modifier(Modifier::BOLD))));
    lines.push(Line::from(vec![
        Span::styled(format!("  {:<name_w$}", "Name"), Style::default().fg(t.muted).add_modifier(Modifier::BOLD)),
        Span::styled(format!("{:>value_w$}", "Reduced Cost"), Style::default().fg(t.muted).add_modifier(Modifier::BOLD)),
    ]));
    lines.push(Line::from(Span::styled(format!("  {}", rule_str(name_w + value_w)), Style::default().fg(t.muted))));
    append_magnitude_ranked_rows(lines, &result.reduced_costs, name_w, value_w);
}

/// Append `(name, value)` rows ranked by descending magnitude, capped at [`MAX_DUAL_ROWS`].
fn append_magnitude_ranked_rows(lines: &mut Vec<Line<'static>>, values: &[(String, f64)], name_w: usize, value_w: usize) {
    let t = theme();
    if values.is_empty() {
        lines.push(Line::from(Span::styled("  (none)", Style::default().fg(t.muted))));
        return;
    }
    let ranked = rank_by_magnitude(values);
    for (rank, &i) in ranked.iter().take(MAX_DUAL_ROWS).enumerate() {
        let (name, value) = &values[i];
        let value_style = if value.abs() < 1e-10 { Style::default().fg(t.muted) } else { Style::default().fg(t.modified) };
        lines.push(
            Line::from(vec![
                Span::styled(format!("  {:<name_w$}", truncate_with_ellipsis(name, name_w)), Style::default().fg(t.text)),
                Span::styled(format!("{value:>value_w$.6}"), value_style),
            ])
            .style(zebra_style(rank)),
        );
    }
    if ranked.len() > MAX_DUAL_ROWS {
        lines.push(Line::from(Span::styled(format!("  ... ({} more)", ranked.len() - MAX_DUAL_ROWS), Style::default().fg(t.warning))));
    }
}

/// Build the comparison Duals tab: constraints ranked by `|Δ shadow price|`
/// and variables ranked by `|Δ reduced cost|`. Rows missing a dual value on
/// both sides are excluded; one-sided rows rank by the present magnitude.
fn build_diff_duals_tab(lines: &mut Vec<Line<'static>>, diff: &SolveDiffResult, name_w: usize) {
    let t = theme();
    let value_w = 14;

    // Constraints ranked by |Δ shadow price|.
    lines.push(Line::from(Span::styled(
        "  Constraints by |\u{0394} shadow price|",
        Style::default().fg(t.muted).add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(vec![
        Span::styled(format!("  {:<name_w$}", "Name"), Style::default().fg(t.muted).add_modifier(Modifier::BOLD)),
        Span::styled(format!("{:>value_w$}", "Shadow 1"), Style::default().fg(t.muted).add_modifier(Modifier::BOLD)),
        Span::styled(format!("{:>value_w$}", "Shadow 2"), Style::default().fg(t.muted).add_modifier(Modifier::BOLD)),
        Span::styled(format!("{:>value_w$}", "\u{0394}"), Style::default().fg(t.muted).add_modifier(Modifier::BOLD)),
    ]));
    lines.push(Line::from(Span::styled(format!("  {}", rule_str(name_w + value_w * 3)), Style::default().fg(t.muted))));

    let ranked_constraints = rank_constraints_by_shadow_delta(&diff.constraint_diff);
    if ranked_constraints.is_empty() {
        lines.push(Line::from(Span::styled("  (no shadow prices on either side)", Style::default().fg(t.muted))));
    }
    for &i in ranked_constraints.iter().take(MAX_DUAL_ROWS) {
        let row = &diff.constraint_diff[i];
        let name = row.name(&diff.result1, &diff.result2);
        lines.push(format_dual_pair_line(name, row.shadow_price1, row.shadow_price2, name_w, value_w));
    }
    if ranked_constraints.len() > MAX_DUAL_ROWS {
        lines.push(Line::from(Span::styled(
            format!("  ... ({} more)", ranked_constraints.len() - MAX_DUAL_ROWS),
            Style::default().fg(t.warning),
        )));
    }

    lines.push(Line::from(""));

    // Variables ranked by |Δ reduced cost|.
    lines.push(Line::from(Span::styled(
        "  Variables by |\u{0394} reduced cost|",
        Style::default().fg(t.muted).add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(vec![
        Span::styled(format!("  {:<name_w$}", "Name"), Style::default().fg(t.muted).add_modifier(Modifier::BOLD)),
        Span::styled(format!("{:>value_w$}", "RC 1"), Style::default().fg(t.muted).add_modifier(Modifier::BOLD)),
        Span::styled(format!("{:>value_w$}", "RC 2"), Style::default().fg(t.muted).add_modifier(Modifier::BOLD)),
        Span::styled(format!("{:>value_w$}", "\u{0394}"), Style::default().fg(t.muted).add_modifier(Modifier::BOLD)),
    ]));
    lines.push(Line::from(Span::styled(format!("  {}", rule_str(name_w + value_w * 3)), Style::default().fg(t.muted))));

    let ranked_variables = rank_variables_by_reduced_cost_delta(&diff.variable_diff);
    if ranked_variables.is_empty() {
        lines.push(Line::from(Span::styled("  (no reduced costs on either side)", Style::default().fg(t.muted))));
    }
    for &i in ranked_variables.iter().take(MAX_DUAL_ROWS) {
        let row = &diff.variable_diff[i];
        let name = row.name(&diff.result1, &diff.result2);
        lines.push(format_dual_pair_line(name, row.reduced_cost1, row.reduced_cost2, name_w, value_w));
    }
    if ranked_variables.len() > MAX_DUAL_ROWS {
        lines.push(Line::from(Span::styled(
            format!("  ... ({} more)", ranked_variables.len() - MAX_DUAL_ROWS),
            Style::default().fg(t.warning),
        )));
    }
}

/// Format one Duals comparison row: name, both values (em-dash when absent),
/// and a signed delta coloured added/removed by direction when both present.
fn format_dual_pair_line(name: &str, v1: Option<f64>, v2: Option<f64>, name_w: usize, value_w: usize) -> Line<'static> {
    let t = theme();
    let dash = "\u{2014}";

    let name_style = match (v1, v2) {
        (None, Some(_)) => Style::default().fg(t.added),
        (Some(_), None) => Style::default().fg(t.removed),
        _ => Style::default().fg(t.text),
    };
    let v1_str = v1.map_or_else(|| format!("{dash:>value_w$}"), |v| format!("{v:>value_w$.6}"));
    let v2_str = v2.map_or_else(|| format!("{dash:>value_w$}"), |v| format!("{v:>value_w$.6}"));

    let mut spans = vec![
        Span::styled(format!("  {:<name_w$}", truncate_with_ellipsis(name, name_w)), name_style),
        Span::styled(v1_str, name_style),
        Span::styled(v2_str, name_style),
    ];
    if let (Some(a), Some(b)) = (v1, v2) {
        let d = b - a;
        let sign = if d > 0.0 { "+" } else { "" };
        let colour = if d > 0.0 {
            t.added
        } else if d < 0.0 {
            t.removed
        } else {
            t.muted
        };
        spans.push(Span::styled(format!("{:>value_w$}", format!("{sign}{d:.6}")), Style::default().fg(colour)));
    } else {
        spans.push(Span::styled(format!("{dash:>value_w$}"), name_style));
    }
    Line::from(spans)
}

/// Append the infeasibility diagnosis block to the Summary tab lines.
///
/// Shown only for infeasible results: a discoverability hint when idle, an
/// elapsed-seconds line while the elastic relaxation runs in the background,
/// and the ranked violation table (or error) once it finishes.
fn append_diagnosis_block(lines: &mut Vec<Line<'static>>, diagnosis: &DiagnosisState) {
    let t = theme();
    lines.push(Line::from(""));
    match diagnosis {
        DiagnosisState::Idle => {
            lines.push(Line::from(Span::styled("  press e to diagnose infeasibility", Style::default().fg(t.muted))));
        }
        DiagnosisState::Running { file, started } => {
            let elapsed = started.elapsed();
            lines.push(Line::from(Span::styled(
                format!("  {} Infeasibility diagnosis: diagnosing {file}\u{2026} ({}s)", spinner_frame(elapsed), elapsed.as_secs()),
                Style::default().fg(t.warning),
            )));
        }
        DiagnosisState::Done { file, diagnosis } => {
            lines.push(Line::from(Span::styled(
                format!("  Infeasibility diagnosis \u{2014} {file}"),
                Style::default().fg(t.warning).add_modifier(Modifier::BOLD),
            )));
            lines.push(Line::from(Span::styled(format!("  {}", rule_str(62)), Style::default().fg(t.muted))));
            lines.push(Line::from(vec![
                Span::styled("  Total violation: ", Style::default().fg(t.muted)),
                Span::styled(format!("{:.6}", diagnosis.total_violation), Style::default().fg(t.warning).add_modifier(Modifier::BOLD)),
                Span::styled(format!("  (elastic solve: {:.3}s)", diagnosis.solve_time.as_secs_f64()), Style::default().fg(t.muted)),
            ]));
            if !diagnosis.bound_conflicts.is_empty() {
                lines.push(Line::from(Span::styled(
                    format!(
                        "  Conflicting variable bounds ({} shown of {}):",
                        diagnosis.bound_conflicts.len().min(MAX_VIOLATION_ROWS),
                        diagnosis.bound_conflicts.len()
                    ),
                    Style::default().fg(t.muted),
                )));
                for (name, gap) in diagnosis.bound_conflicts.iter().take(MAX_VIOLATION_ROWS) {
                    lines.push(Line::from(vec![
                        Span::styled(format!("  {:<30}", truncate_with_ellipsis(name, 30)), Style::default().fg(t.text)),
                        Span::styled(format!("{gap:>14.6}"), Style::default().fg(t.warning)),
                        Span::styled("  (lower > upper)", Style::default().fg(t.muted)),
                    ]));
                }
                if diagnosis.bound_conflicts.len() > MAX_VIOLATION_ROWS {
                    lines.push(Line::from(Span::styled(
                        format!("  ... ({} more)", diagnosis.bound_conflicts.len() - MAX_VIOLATION_ROWS),
                        Style::default().fg(t.warning),
                    )));
                }
            }
            if diagnosis.violations.is_empty() {
                if diagnosis.bound_conflicts.is_empty() {
                    lines.push(Line::from(Span::styled(
                        "  No violated constraints found \u{2014} infeasibility may stem from variable bounds",
                        Style::default().fg(t.warning),
                    )));
                }
            } else {
                lines.push(Line::from(Span::styled(
                    format!(
                        "  Top violated constraints ({} shown of {}):",
                        diagnosis.violations.len().min(MAX_VIOLATION_ROWS),
                        diagnosis.violations.len()
                    ),
                    Style::default().fg(t.muted),
                )));
                for (name, amount) in diagnosis.violations.iter().take(MAX_VIOLATION_ROWS) {
                    lines.push(Line::from(vec![
                        Span::styled(format!("  {:<30}", truncate_with_ellipsis(name, 30)), Style::default().fg(t.text)),
                        Span::styled(format!("{amount:>14.6}"), Style::default().fg(t.warning)),
                    ]));
                }
                if diagnosis.violations.len() > MAX_VIOLATION_ROWS {
                    lines.push(Line::from(Span::styled(
                        format!("  ... ({} more)", diagnosis.violations.len() - MAX_VIOLATION_ROWS),
                        Style::default().fg(t.warning),
                    )));
                }
            }
        }
        DiagnosisState::Failed(error) => {
            lines.push(Line::from(Span::styled(format!("  Infeasibility diagnosis failed: {error}"), Style::default().fg(t.error))));
        }
    }
}

fn draw_running_both(frame: &mut Frame, area: Rect, file1: &str, file2: &str, done1: bool, done2: bool, elapsed: std::time::Duration) {
    let t = theme();
    let popup = super::centred_rect(area, 60, 7);
    let running_label = format!("solving\u{2026} ({}s)", elapsed.as_secs());
    let spinner = spinner_frame(elapsed);
    let icon1 = if done1 { "\u{2713}" } else { spinner };
    let status1 = if done1 { "done" } else { running_label.as_str() };
    let icon2 = if done2 { "\u{2713}" } else { spinner };
    let status2 = if done2 { "done" } else { running_label.as_str() };
    let style_done = Style::default().fg(t.added).add_modifier(Modifier::BOLD);
    let style_running = Style::default().fg(t.warning).add_modifier(Modifier::BOLD);

    let lines = vec![
        Line::from(""),
        Line::from(vec![
            Span::styled(format!("  {icon1} "), if done1 { style_done } else { style_running }),
            Span::styled(format!("{file1:<30}"), Style::default().fg(t.text)),
            Span::styled(status1, if done1 { style_done } else { style_running }),
        ]),
        Line::from(vec![
            Span::styled(format!("  {icon2} "), if done2 { style_done } else { style_running }),
            Span::styled(format!("{file2:<30}"), Style::default().fg(t.text)),
            Span::styled(status2, if done2 { style_done } else { style_running }),
        ]),
        Line::from(""),
    ];

    let block = panel_block(Style::default().fg(t.secondary_accent).add_modifier(Modifier::BOLD))
        .title(Span::styled(" Solver ", Style::default().fg(t.secondary_accent).add_modifier(Modifier::BOLD)));

    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(Clear, popup);
    frame.render_widget(paragraph, popup);
}

fn draw_done_both(
    frame: &mut Frame,
    area: Rect,
    diff: &SolveDiffResult,
    view: &SolveViewState,
    cache: &SolveRenderCache,
    diagnosis: &DiagnosisState,
) {
    let t = theme();
    let popup_width = (area.width * 4 / 5).max(60).min(area.width);
    let popup_height = (area.height * 4 / 5).max(20).min(area.height);
    let popup = super::centred_rect(area, popup_width, popup_height);

    let active = view.tab;
    let scroll = view.scroll[active.index()];

    let tab_bar = build_tab_bar(active);
    let mut lines = vec![tab_bar, Line::from("")];

    match active {
        SolveTab::Summary => {
            if let SolveRenderCache::Diff { summary, .. } = cache {
                lines.extend(summary.iter().cloned());
            } else {
                build_diff_summary_tab(&mut lines, diff);
            }
        }
        SolveTab::Variables => {
            if let SolveRenderCache::Diff { variable_rows, variable_count_label, .. } = cache {
                build_diff_variables_tab_cached(&mut lines, variable_count_label, variable_rows, view, scroll, popup_height);
            } else {
                build_diff_variables_tab(&mut lines, diff, view, scroll, popup_height);
            }
        }
        SolveTab::Constraints => {
            if let SolveRenderCache::Diff { constraint_rows, constraint_count_label, .. } = cache {
                build_diff_constraints_tab_cached(&mut lines, constraint_count_label, constraint_rows, view, scroll, popup_height);
            } else {
                build_diff_constraints_tab(&mut lines, diff, view, scroll, popup_height);
            }
        }
        SolveTab::Log => {
            if let SolveRenderCache::Diff { log, .. } = cache {
                lines.extend(log.iter().cloned());
            } else {
                build_diff_log_tab(&mut lines, diff);
            }
        }
        SolveTab::Duals => {
            if let SolveRenderCache::Diff { duals, .. } = cache {
                lines.extend(duals.iter().cloned());
            } else {
                build_diff_duals_tab(&mut lines, diff, name_column_width(popup_width.saturating_sub(2), 2 + 14 * 3));
            }
        }
    }

    // Diagnosis block for infeasible sides (`e` diagnoses file 1 when both are infeasible).
    if active == SolveTab::Summary && (status_is_infeasible(&diff.result1.status) || status_is_infeasible(&diff.result2.status)) {
        append_diagnosis_block(&mut lines, diagnosis);
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "  1-5: tabs  Tab/S-Tab: cycle  j/k: scroll  d: toggle diff  t/T: threshold  y: yank  Esc: close",
        Style::default().fg(t.muted),
    )));

    let block = panel_block(Style::default().fg(t.secondary_accent).add_modifier(Modifier::BOLD))
        .title(Span::styled(" Solve Comparison ", Style::default().fg(t.secondary_accent).add_modifier(Modifier::BOLD)));

    let paragraph = Paragraph::new(lines).block(block).scroll((scroll, 0));
    frame.render_widget(Clear, popup);
    frame.render_widget(paragraph, popup);
}

// ---------------------------------------------------------------------------
// Cache-building functions — called once when solve completes
// ---------------------------------------------------------------------------

/// Pre-format all 5 tab contents for a single solve result.
///
/// `inner_width` is the solve popup's inner width at solve completion; name
/// columns are sized from it (a later resize keeps these widths — acceptable,
/// since the cache exists to avoid per-frame rebuilding).
pub fn build_single_solve_cache(result: &SolveResult, inner_width: u16) -> [Vec<Line<'static>>; 5] {
    let table_name_w = name_column_width(inner_width, 2 + 12 + 14);
    let duals_name_w = name_column_width(inner_width, 2 + 14);
    let mut summary = Vec::new();
    build_summary_tab(&mut summary, result);
    let mut variables = Vec::new();
    build_variables_tab(&mut variables, result, table_name_w);
    let mut constraints = Vec::new();
    build_constraints_tab(&mut constraints, result, table_name_w);
    let mut log = Vec::new();
    build_log_tab(&mut log, result);
    let mut duals = Vec::new();
    build_duals_tab(&mut duals, result, duals_name_w);
    [summary, variables, constraints, log, duals]
}

/// Pre-format summary, log, duals, and per-row lines for a diff solve result.
pub fn build_diff_solve_cache(diff: &SolveDiffResult, inner_width: u16) -> SolveRenderCache {
    let mut summary = Vec::new();
    build_diff_summary_tab(&mut summary, diff);

    let mut log = Vec::new();
    build_diff_log_tab(&mut log, diff);

    let mut duals = Vec::new();
    build_diff_duals_tab(&mut duals, diff, name_column_width(inner_width, 2 + 14 * 3));

    // Variable/constraint diff rows keep fixed name widths: their headers are
    // rebuilt per frame by the cached-tab renderers and must stay aligned.
    let variable_rows: Vec<CachedDiffRow> = diff
        .variable_diff
        .iter()
        .filter_map(|row| {
            let name = row.name(&diff.result1, &diff.result2);
            let line = format_variable_diff_line(row, name, 24, 14)?;
            Some(CachedDiffRow { line, changed: row.changed })
        })
        .collect();

    let constraint_rows: Vec<CachedDiffRow> = diff
        .constraint_diff
        .iter()
        .filter_map(|row| {
            let name = row.name(&diff.result1, &diff.result2);
            let line = format_constraint_diff_line(row, name, 22, 13)?;
            Some(CachedDiffRow { line, changed: row.changed })
        })
        .collect();

    let variable_count_label = diff_counts_summary_label(&diff.variable_counts);
    let constraint_count_label = diff_counts_summary_label(&diff.constraint_counts);

    SolveRenderCache::Diff { summary, log, duals, variable_rows, constraint_rows, variable_count_label, constraint_count_label }
}

/// Format a single variable diff row as a `Line`. Returns `None` for `(None, None)` rows.
fn format_variable_diff_line(row: &VarDiffRow, name: &str, name_w: usize, value_w: usize) -> Option<Line<'static>> {
    let t = theme();
    let dash = "\u{2014}";

    let (name_style, value1_str, value2_str, delta_str, marker) = match (row.val1, row.val2) {
        (None, Some(v2)) => {
            (Style::default().fg(t.added), format!("{dash:>value_w$}"), format!("{v2:>value_w$.6}"), format!("{:>value_w$}", "(added)"), "")
        }
        (Some(v1), None) => (
            Style::default().fg(t.removed),
            format!("{v1:>value_w$.6}"),
            format!("{dash:>value_w$}"),
            format!("{:>value_w$}", "(removed)"),
            "",
        ),
        (Some(v1), Some(v2)) => {
            if row.changed {
                let d = v2 - v1;
                let sign = if d >= 0.0 { "+" } else { "" };
                (Style::default().fg(t.modified), format!("{v1:>value_w$.6}"), format!("{v2:>value_w$.6}"), format!("{sign}{d:>.6}"), " *")
            } else {
                let base = if v1.abs() < 1e-10 { Style::default().fg(t.muted) } else { Style::default().fg(t.text) };
                (base, format!("{v1:>value_w$.6}"), format!("{v2:>value_w$.6}"), String::new(), "")
            }
        }
        (None, None) => return None,
    };

    let mut spans = vec![
        Span::styled(format!("  {:<name_w$}", truncate_with_ellipsis(name, name_w)), name_style),
        Span::styled(value1_str, name_style),
        Span::styled(format!("  {value2_str}"), name_style),
    ];
    if !delta_str.is_empty() {
        spans.push(Span::styled(format!("  {delta_str}"), name_style));
    }
    if !marker.is_empty() {
        spans.push(Span::styled(marker.to_owned(), Style::default().fg(t.modified).add_modifier(Modifier::BOLD)));
    }
    Some(Line::from(spans))
}

/// Format a single constraint diff row as a `Line`. Returns `None` for `(None, None)` rows.
fn format_constraint_diff_line(row: &crate::solver::ConstraintDiffRow, name: &str, name_w: usize, val_w: usize) -> Option<Line<'static>> {
    let t = theme();
    let dash = "\u{2014}";

    let (name_style, a1, a2, s1, s2, marker) = match (row.activity1, row.activity2) {
        (None, Some(_)) => {
            let style = Style::default().fg(t.added);
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
            let style = Style::default().fg(t.removed);
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
                    Style::default().fg(t.modified),
                    format!("{act1:>val_w$.4}"),
                    format!("{act2:>val_w$.4}"),
                    row.shadow_price1.map_or_else(String::new, |v| format!("{v:>val_w$.4}")),
                    row.shadow_price2.map_or_else(String::new, |v| format!("{v:>val_w$.4}")),
                    " *",
                )
            } else {
                let base = if act1.abs() < 1e-10 { Style::default().fg(t.muted) } else { Style::default().fg(t.text) };
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
        (None, None) => return None,
    };

    let mut spans = vec![
        Span::styled(format!("  {:<name_w$}", truncate_with_ellipsis(name, name_w)), name_style),
        Span::styled(a1, name_style),
        Span::styled(format!("  {a2}"), name_style),
        Span::styled(format!("  {s1}"), name_style),
        Span::styled(format!("  {s2}"), name_style),
    ];
    if !marker.is_empty() {
        spans.push(Span::styled(marker.to_owned(), Style::default().fg(t.modified).add_modifier(Modifier::BOLD)));
    }
    Some(Line::from(spans))
}

// ---------------------------------------------------------------------------
// Cached diff tab rendering — uses pre-formatted row Lines
// ---------------------------------------------------------------------------

/// Visible data-row window `(first_index, count)` for a list whose header
/// occupies `header_count` lines, given the scroll offset and viewport height.
const fn window_range(header_count: usize, scroll: u16, visible_height: u16) -> (usize, usize) {
    let scroll = scroll as usize;
    (scroll.saturating_sub(header_count), (visible_height as usize).saturating_sub(header_count.saturating_sub(scroll)))
}

/// Append windowed cached row lines: visible rows get their cached `Line`,
/// off-screen rows a placeholder so scroll positions stay aligned.
fn push_cached_window(lines: &mut Vec<Line<'static>>, cached_rows: &[CachedDiffRow], diff_only: bool, scroll: u16, visible_height: u16) {
    let (first_visible_data, visible_data_count) = window_range(lines.len(), scroll, visible_height);
    let mut data_index: usize = 0;
    for cached in cached_rows {
        if diff_only && !cached.changed {
            continue;
        }
        if data_index >= first_visible_data && data_index < first_visible_data + visible_data_count {
            lines.push(cached.line.clone());
        } else {
            lines.push(Line::default());
        }
        data_index += 1;
    }
}

/// Render the variables diff tab using pre-formatted cached row lines.
fn build_diff_variables_tab_cached(
    lines: &mut Vec<Line<'static>>,
    count_label: &str,
    cached_rows: &[CachedDiffRow],
    view: &SolveViewState,
    scroll: u16,
    visible_height: u16,
) {
    let t = theme();
    let filter_label = diff_filter_label(view.diff_only, view.delta_threshold);
    let diff_only = view.diff_only;
    let total = cached_rows.len();

    lines.push(Line::from(Span::styled(
        format!("  Variables: {count_label} (of {total} total){filter_label}"),
        Style::default().fg(t.muted).add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(""));

    let name_w = 24;
    let value_w = 14;
    lines.push(Line::from(vec![
        Span::styled(format!("  {:<name_w$}", "Name"), Style::default().fg(t.muted).add_modifier(Modifier::BOLD)),
        Span::styled(format!("{:>value_w$}", "File 1"), Style::default().fg(t.muted).add_modifier(Modifier::BOLD)),
        Span::styled(format!("{:>value_w$}", "File 2"), Style::default().fg(t.muted).add_modifier(Modifier::BOLD)),
        Span::styled(format!("{:>value_w$}", "\u{0394}"), Style::default().fg(t.muted).add_modifier(Modifier::BOLD)),
    ]));
    lines.push(Line::from(Span::styled(format!("  {}", rule_str(72)), Style::default().fg(t.muted))));

    push_cached_window(lines, cached_rows, diff_only, scroll, visible_height);
}

/// Render the constraints diff tab using pre-formatted cached row lines.
fn build_diff_constraints_tab_cached(
    lines: &mut Vec<Line<'static>>,
    count_label: &str,
    cached_rows: &[CachedDiffRow],
    view: &SolveViewState,
    scroll: u16,
    visible_height: u16,
) {
    let t = theme();
    let filter_label = diff_filter_label(view.diff_only, view.delta_threshold);
    let diff_only = view.diff_only;
    let total = cached_rows.len();

    lines.push(Line::from(Span::styled(
        format!("  Constraints: {count_label} (of {total} total){filter_label}"),
        Style::default().fg(t.muted).add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(""));

    let name_w = 22;
    let value_w = 13;
    lines.push(Line::from(vec![
        Span::styled(format!("  {:<name_w$}", "Name"), Style::default().fg(t.muted).add_modifier(Modifier::BOLD)),
        Span::styled(format!("{:>value_w$}", "Activity 1"), Style::default().fg(t.muted).add_modifier(Modifier::BOLD)),
        Span::styled(format!("{:>value_w$}", "Activity 2"), Style::default().fg(t.muted).add_modifier(Modifier::BOLD)),
        Span::styled(format!("{:>value_w$}", "Shadow 1"), Style::default().fg(t.muted).add_modifier(Modifier::BOLD)),
        Span::styled(format!("{:>value_w$}", "Shadow 2"), Style::default().fg(t.muted).add_modifier(Modifier::BOLD)),
    ]));
    lines.push(Line::from(Span::styled(format!("  {}", rule_str(82)), Style::default().fg(t.muted))));

    push_cached_window(lines, cached_rows, diff_only, scroll, visible_height);
}

// ---------------------------------------------------------------------------
// Uncached diff tab rendering — fallback when cache is not available
// ---------------------------------------------------------------------------

/// Format the cached diff counts as a human-readable summary string.
fn diff_counts_summary_label(counts: &DiffCounts) -> String {
    let mut parts = Vec::new();
    if counts.modified > 0 {
        parts.push(format!("{} changed", counts.modified));
    }
    if counts.added > 0 {
        parts.push(format!("{} added", counts.added));
    }
    if counts.removed > 0 {
        parts.push(format!("{} removed", counts.removed));
    }
    if parts.is_empty() { "no differences".to_owned() } else { parts.join(", ") }
}

/// Format cached diff counts as description parts for the summary tab.
fn diff_counts_description_parts(counts: &DiffCounts, entity: &str) -> Vec<String> {
    let mut parts = Vec::new();
    if counts.modified > 0 {
        parts.push(format!("{} {entity} changed", counts.modified));
    }
    if counts.added > 0 {
        parts.push(format!("{} {entity} added", counts.added));
    }
    if counts.removed > 0 {
        parts.push(format!("{} {entity} removed", counts.removed));
    }
    parts
}

/// Format a count delta span for the diff summary comparison rows.
fn count_delta_span(count1: usize, count2: usize) -> Option<Span<'static>> {
    let t = theme();
    if count1 == count2 {
        return None;
    }
    #[allow(clippy::cast_possible_wrap)]
    let delta = count2 as i64 - count1 as i64;
    let sign = if delta > 0 { "+" } else { "" };
    let colour = if delta > 0 { t.added } else { t.removed };
    Some(Span::styled(format!("  \u{0394} {sign}{delta}"), Style::default().fg(colour)))
}

/// Format a delta string. Returns empty spans if no delta exceeds `threshold`.
fn delta_spans(v1: Option<f64>, v2: Option<f64>, threshold: f64) -> Vec<Span<'static>> {
    let t = theme();
    let (Some(a), Some(b)) = (v1, v2) else {
        return Vec::new();
    };
    let d = b - a;
    if d.abs() <= threshold {
        return Vec::new();
    }
    let sign = if d > 0.0 { "+" } else { "" };
    let colour = if d > 0.0 { t.added } else { t.removed };
    vec![Span::styled(format!("  \u{0394} {sign}{d:.6}"), Style::default().fg(colour))]
}

/// Render the comparison table header and metrics rows for the diff summary.
fn build_diff_summary_metrics(lines: &mut Vec<Line<'static>>, diff: &SolveDiffResult) {
    let t = theme();
    let r1 = &diff.result1;
    let r2 = &diff.result2;

    let label_w = 18;
    let col_w = 20;
    lines.push(Line::from(vec![
        Span::styled(format!("  {:<label_w$}", ""), Style::default()),
        Span::styled(format!("{:<col_w$}", "File 1"), Style::default().fg(t.muted).add_modifier(Modifier::BOLD)),
        Span::styled(format!("{:<col_w$}", "File 2"), Style::default().fg(t.muted).add_modifier(Modifier::BOLD)),
    ]));
    lines.push(Line::from(Span::styled(format!("  {}", rule_str(62)), Style::default().fg(t.muted))));

    // Status.
    lines.push(Line::from(vec![
        Span::styled(format!("  {:<label_w$}", "Status:"), Style::default().fg(t.muted)),
        Span::styled(format!("{:<col_w$}", r1.status), status_style(&r1.status)),
        Span::styled(format!("{:<col_w$}", r2.status), status_style(&r2.status)),
    ]));

    // Objective.
    let obj1_str = r1.objective_value.map_or_else(|| "N/A".to_owned(), |v| format!("{v:.6}"));
    let obj2_str = r2.objective_value.map_or_else(|| "N/A".to_owned(), |v| format!("{v:.6}"));
    let mut objective_spans = vec![
        Span::styled(format!("  {:<label_w$}", "Objective:"), Style::default().fg(t.muted)),
        Span::styled(format!("{obj1_str:<col_w$}"), Style::default().fg(t.text).add_modifier(Modifier::BOLD)),
        Span::styled(format!("{obj2_str:<col_w$}"), Style::default().fg(t.text).add_modifier(Modifier::BOLD)),
    ];
    objective_spans.extend(delta_spans(r1.objective_value, r2.objective_value, 0.0));
    lines.push(Line::from(objective_spans));

    // Variable counts.
    let variable_count1 = r1.variables.len();
    let variable_count2 = r2.variables.len();
    let mut variable_spans = vec![
        Span::styled(format!("  {:<label_w$}", "Variables:"), Style::default().fg(t.muted)),
        Span::styled(format!("{variable_count1:<col_w$}"), Style::default().fg(t.text)),
        Span::styled(format!("{variable_count2:<col_w$}"), Style::default().fg(t.text)),
    ];
    if let Some(delta) = count_delta_span(variable_count1, variable_count2) {
        variable_spans.push(delta);
    }
    lines.push(Line::from(variable_spans));

    // Constraint counts.
    let constraint_count1 = r1.shadow_prices.len();
    let constraint_count2 = r2.shadow_prices.len();
    let mut constraint_spans = vec![
        Span::styled(format!("  {:<label_w$}", "Constraints:"), Style::default().fg(t.muted)),
        Span::styled(format!("{constraint_count1:<col_w$}"), Style::default().fg(t.text)),
        Span::styled(format!("{constraint_count2:<col_w$}"), Style::default().fg(t.text)),
    ];
    if let Some(delta) = count_delta_span(constraint_count1, constraint_count2) {
        constraint_spans.push(delta);
    }
    lines.push(Line::from(constraint_spans));

    // Skipped SOS.
    if r1.skipped_sos > 0 || r2.skipped_sos > 0 {
        lines.push(Line::from(vec![
            Span::styled(format!("  {:<label_w$}", "Skipped SOS:"), Style::default().fg(t.muted)),
            Span::styled(format!("{:<col_w$}", r1.skipped_sos), Style::default().fg(t.text)),
            Span::styled(format!("{:<col_w$}", r2.skipped_sos), Style::default().fg(t.text)),
        ]));
    }

    // Timing breakdown.
    let total1 = r1.build_time + r1.solve_time + r1.extract_time;
    let total2 = r2.build_time + r2.solve_time + r2.extract_time;
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled("  Timings", Style::default().fg(t.muted).add_modifier(Modifier::BOLD))));
    lines.push(Line::from(Span::styled(format!("  {}", rule_str(62)), Style::default().fg(t.muted))));

    for (label, d1, d2) in
        [("Build:", r1.build_time, r2.build_time), ("Solve:", r1.solve_time, r2.solve_time), ("Extract:", r1.extract_time, r2.extract_time)]
    {
        lines.push(Line::from(vec![
            Span::styled(format!("  {label:<label_w$}"), Style::default().fg(t.muted)),
            Span::styled(format!("{:<col_w$}", format!("{:.3}s", d1.as_secs_f64())), Style::default().fg(t.accent)),
            Span::styled(format!("{:<col_w$}", format!("{:.3}s", d2.as_secs_f64())), Style::default().fg(t.accent)),
        ]));
    }

    lines.push(Line::from(vec![
        Span::styled(format!("  {:<label_w$}", "Total:"), Style::default().fg(t.muted)),
        Span::styled(
            format!("{:<col_w$}", format!("{:.3}s", total1.as_secs_f64())),
            Style::default().fg(t.text).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("{:<col_w$}", format!("{:.3}s", total2.as_secs_f64())),
            Style::default().fg(t.text).add_modifier(Modifier::BOLD),
        ),
    ]));

    lines.push(Line::from(Span::styled(format!("  {}", rule_str(62)), Style::default().fg(t.muted))));
    lines.push(Line::from(vec![
        Span::styled(format!("  {:<label_w$}", "Diff:"), Style::default().fg(t.muted)),
        Span::styled(format!("{:.3}s", diff.diff_time.as_secs_f64()), Style::default().fg(t.accent)),
    ]));
}

fn build_diff_summary_tab(lines: &mut Vec<Line<'static>>, diff: &SolveDiffResult) {
    let t = theme();
    build_diff_summary_metrics(lines, diff);

    // Summary of differences.
    lines.push(Line::from(""));

    let mut parts = diff_counts_description_parts(&diff.variable_counts, "variables");
    parts.extend(diff_counts_description_parts(&diff.constraint_counts, "constraints"));

    let label_w = 18;
    let summary = if parts.is_empty() { "No differences".to_owned() } else { parts.join(", ") };
    lines.push(Line::from(vec![
        Span::styled(format!("  {:<label_w$}", "Differences:"), Style::default().fg(t.muted)),
        Span::styled(summary, Style::default().fg(t.modified)),
    ]));
}

/// Render the diff-only toggle label with threshold for tabs with filtering.
fn diff_filter_label(diff_only: bool, threshold: f64) -> String {
    let toggle = if diff_only { "press d for all" } else { "press d for changed only" };
    if threshold == 0.0 { format!(" (threshold: exact, {toggle})") } else { format!(" (threshold: {threshold}, {toggle})") }
}

fn build_diff_variables_tab(
    lines: &mut Vec<Line<'static>>,
    diff: &SolveDiffResult,
    view: &SolveViewState,
    scroll: u16,
    visible_height: u16,
) {
    let t = theme();
    let counts = &diff.variable_counts;
    let summary = diff_counts_summary_label(counts);
    let filter_label = diff_filter_label(view.diff_only, view.delta_threshold);
    let diff_only = view.diff_only;

    lines.push(Line::from(Span::styled(
        format!("  Variables: {summary} (of {} total){filter_label}", counts.total),
        Style::default().fg(t.muted).add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(""));

    let name_w = 24;
    let value_w = 14;
    lines.push(Line::from(vec![
        Span::styled(format!("  {:<name_w$}", "Name"), Style::default().fg(t.muted).add_modifier(Modifier::BOLD)),
        Span::styled(format!("{:>value_w$}", "File 1"), Style::default().fg(t.muted).add_modifier(Modifier::BOLD)),
        Span::styled(format!("{:>value_w$}", "File 2"), Style::default().fg(t.muted).add_modifier(Modifier::BOLD)),
        Span::styled(format!("{:>value_w$}", "\u{0394}"), Style::default().fg(t.muted).add_modifier(Modifier::BOLD)),
    ]));
    lines.push(Line::from(Span::styled(format!("  {}", rule_str(72)), Style::default().fg(t.muted))));

    // Windowed rendering: only build styled Lines for visible data rows.
    let (first_visible_data, visible_data_count) = window_range(lines.len(), scroll, visible_height);
    let mut data_index: usize = 0;
    for row in &diff.variable_diff {
        if diff_only && !row.changed {
            continue;
        }
        if data_index >= first_visible_data && data_index < first_visible_data + visible_data_count {
            let name = row.name(&diff.result1, &diff.result2);
            if let Some(line) = format_variable_diff_line(row, name, name_w, value_w) {
                lines.push(line);
            }
        } else {
            lines.push(Line::default());
        }
        data_index += 1;
    }
}

fn build_diff_constraints_tab(
    lines: &mut Vec<Line<'static>>,
    diff: &SolveDiffResult,
    view: &SolveViewState,
    scroll: u16,
    visible_height: u16,
) {
    let t = theme();
    let counts = &diff.constraint_counts;
    let summary = diff_counts_summary_label(counts);
    let filter_label = diff_filter_label(view.diff_only, view.delta_threshold);
    let diff_only = view.diff_only;

    lines.push(Line::from(Span::styled(
        format!("  Constraints: {summary} (of {} total){filter_label}", counts.total),
        Style::default().fg(t.muted).add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(""));

    let name_w = 22;
    let value_w = 13;
    lines.push(Line::from(vec![
        Span::styled(format!("  {:<name_w$}", "Name"), Style::default().fg(t.muted).add_modifier(Modifier::BOLD)),
        Span::styled(format!("{:>value_w$}", "Activity 1"), Style::default().fg(t.muted).add_modifier(Modifier::BOLD)),
        Span::styled(format!("{:>value_w$}", "Activity 2"), Style::default().fg(t.muted).add_modifier(Modifier::BOLD)),
        Span::styled(format!("{:>value_w$}", "Shadow 1"), Style::default().fg(t.muted).add_modifier(Modifier::BOLD)),
        Span::styled(format!("{:>value_w$}", "Shadow 2"), Style::default().fg(t.muted).add_modifier(Modifier::BOLD)),
    ]));
    lines.push(Line::from(Span::styled(format!("  {}", rule_str(82)), Style::default().fg(t.muted))));

    // Windowed rendering: only build styled Lines for visible data rows.
    let (first_visible_data, visible_data_count) = window_range(lines.len(), scroll, visible_height);
    let mut data_index: usize = 0;
    for row in &diff.constraint_diff {
        if diff_only && !row.changed {
            continue;
        }
        if data_index >= first_visible_data && data_index < first_visible_data + visible_data_count {
            let name = row.name(&diff.result1, &diff.result2);
            if let Some(line) = format_constraint_diff_line(row, name, name_w, value_w) {
                lines.push(line);
            }
        } else {
            lines.push(Line::default());
        }
        data_index += 1;
    }
}

fn build_diff_log_tab(lines: &mut Vec<Line<'static>>, diff: &SolveDiffResult) {
    const MAX_LOG_LINES: usize = 100;

    let t = theme();

    // File 1 log — iterator skip/take to avoid per-frame Vec.
    lines.push(Line::from(Span::styled(
        format!("  \u{2500}\u{2500} File 1: {} \u{2500}{}", diff.file1_label, rule_str(38)),
        Style::default().fg(t.muted).add_modifier(Modifier::BOLD),
    )));
    append_truncated_log(lines, &diff.result1.solver_log, MAX_LOG_LINES, t);

    lines.push(Line::from(""));

    // File 2 log — iterator skip/take to avoid per-frame Vec.
    lines.push(Line::from(Span::styled(
        format!("  \u{2500}\u{2500} File 2: {} \u{2500}{}", diff.file2_label, rule_str(38)),
        Style::default().fg(t.muted).add_modifier(Modifier::BOLD),
    )));
    append_truncated_log(lines, &diff.result2.solver_log, MAX_LOG_LINES, t);
}

/// Append the last `max_lines` of a log string to the output, with a truncation notice if needed.
fn append_truncated_log(lines: &mut Vec<Line<'static>>, log: &str, max_lines: usize, t: &crate::theme::Theme) {
    let total = log.lines().count();
    let skip = total.saturating_sub(max_lines);

    if total > max_lines {
        lines.push(Line::from(Span::styled(format!("  ... ({} lines truncated)", total - max_lines), Style::default().fg(t.warning))));
    }

    for log_line in log.lines().skip(skip) {
        lines.push(Line::from(Span::styled(format!("  {log_line}"), Style::default().fg(t.muted))));
    }
}

fn draw_failed(frame: &mut Frame, area: Rect, err: &str) {
    let t = theme();
    let popup = super::centred_rect(area, 60, 8);
    let lines = vec![
        Line::from(""),
        Line::from(Span::styled("  Solve failed:", Style::default().fg(t.error).add_modifier(Modifier::BOLD))),
        Line::from(""),
        Line::from(Span::styled(format!("  {err}"), Style::default().fg(t.error))),
        Line::from(""),
        Line::from(Span::styled("  Press Esc to close", Style::default().fg(t.muted))),
    ];

    let block = panel_block(Style::default().fg(t.error).add_modifier(Modifier::BOLD))
        .title(Span::styled(" Solver Error ", Style::default().fg(t.error).add_modifier(Modifier::BOLD)));

    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(Clear, popup);
    frame.render_widget(paragraph, popup);
}

/// Pick a style colour based on the status string.
fn status_style(status: &str) -> Style {
    let t = theme();
    if status.contains("Optimal") {
        Style::default().fg(t.added).add_modifier(Modifier::BOLD)
    } else if status.contains("Infeasible") || status.contains("Unbounded") {
        Style::default().fg(t.error).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(t.warning).add_modifier(Modifier::BOLD)
    }
}
