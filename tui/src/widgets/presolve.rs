//! Presolve rule picker overlay: choose which rewrites to apply, then compare.
//!
//! Opened with `P`. Each rule is a solution-preserving rewrite (see
//! [`crate::presolve`]); on confirm the app rewrites the baseline problem and
//! launches an original-vs-rewritten comparison solve, so the objective values
//! can be checked against each other and the solve times compared.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph};

use crate::app::App;
use crate::highs_presolve::HighsPresolveReport;
use crate::presolve::{PresolveStats, Rule};
use crate::theme::theme;
use crate::widgets::{centred_rect, panel_block};

/// Overlay dimensions: wide enough for a rule label plus its one-line detail,
/// and for the key hint that sits on the bottom border.
const POPUP_WIDTH: u16 = 92;
/// Border, the rule rows, the detail line, the last-run block, and the key hint.
const POPUP_HEIGHT: u16 = 18;

/// Draw the presolve rule picker on top of the current frame.
pub fn draw_presolve(frame: &mut Frame, area: Rect, app: &App) {
    let Some(cursor) = app.presolve_cursor else {
        return;
    };
    if area.width == 0 || area.height == 0 {
        return;
    }
    debug_assert!(cursor < Rule::COUNT, "presolve cursor {cursor} out of range");

    let t = theme();
    let popup = centred_rect(area, POPUP_WIDTH.min(area.width), POPUP_HEIGHT.min(area.height));
    frame.render_widget(Clear, popup);

    let mut lines: Vec<Line<'_>> = Vec::with_capacity(usize::from(POPUP_HEIGHT));

    for (index, rule) in Rule::ALL.iter().enumerate() {
        let selected = index == cursor;
        let on = app.presolve_rules[index];
        let marker = if selected { "\u{25b8} " } else { "  " };
        let checkbox = if on { "[x] " } else { "[ ] " };
        let label_style = match (on, selected) {
            (true, true) => Style::default().fg(t.accent).add_modifier(Modifier::BOLD),
            (true, false) => Style::default().fg(t.text),
            (false, true) => Style::default().fg(t.muted).add_modifier(Modifier::BOLD),
            (false, false) => Style::default().fg(t.muted),
        };
        lines.push(Line::from(vec![
            Span::styled(marker, Style::default().fg(t.accent)),
            Span::styled(checkbox, Style::default().fg(if on { t.added } else { t.muted })),
            Span::styled(rule.label(), label_style),
        ]));
    }

    // Detail for the highlighted rule, so the list stays scannable.
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(format!("  {}", Rule::ALL[cursor].detail()), Style::default().fg(t.muted))));

    if let Some(stats) = &app.last_presolve {
        lines.push(Line::from(""));
        lines.extend(last_run_lines(stats, t.muted, t.text));
    }

    let block = panel_block(Style::default().fg(t.accent))
        .title(Span::styled(" Rewrite: presolve & compare solves ", Style::default().fg(t.accent).add_modifier(Modifier::BOLD)));
    frame.render_widget(Paragraph::new(lines).block(block), popup);

    // The hint sits on the bottom border so the rule list keeps the full body.
    let hint = " j/k \u{2022} space toggle \u{2022} a all/none \u{2022} Enter solve \u{2022} l log \u{2022} H HiGHS's own \u{2022} w .lp \u{2022} Esc ";
    let hint_width = u16::try_from(hint.chars().count()).unwrap_or(u16::MAX);
    if popup.width > hint_width && popup.height > 0 {
        let hint_area = Rect { x: popup.x + 2, y: popup.bottom() - 1, width: hint_width, height: 1 };
        frame.render_widget(Paragraph::new(Line::from(Span::styled(hint, Style::default().fg(t.muted)))), hint_area);
    }
}

/// Build the presolve log pane's lines: the same summary the picker shows,
/// followed by every action the run recorded.
///
/// The counters say how much the rewrite did; these lines say to what, which is
/// the question you have when a rewritten model does not solve to the objective
/// you expected.
pub fn log_lines(stats: &PresolveStats) -> Vec<Line<'static>> {
    let t = theme();
    let mut lines = last_run_lines(stats, t.muted, t.text);

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        format!("  {} action(s), oldest first", stats.log.len()),
        Style::default().fg(t.accent).add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(""));

    if stats.log.is_empty() {
        lines.push(Line::from(Span::styled("  no rule fired \u{2014} the model is already reduced", Style::default().fg(t.muted))));
    }
    for entry in &stats.log {
        // Colour by verb: what left the model reads differently from what was
        // only narrowed or rescaled.
        let colour = if entry.starts_with("INFEASIBLE") || entry.contains("row removed") {
            t.removed
        } else if entry.contains(" fix col ") || entry.contains(" unused col ") {
            t.accent
        } else {
            t.text
        };
        lines.push(Line::from(Span::styled(format!("  {entry}"), Style::default().fg(colour))));
    }
    lines
}

/// Build the lines for the `HiGHS` presolve report: what the *solver* removes
/// from the file as written, before any of our rewrites.
///
/// Rows and columns are listed separately because `HiGHS` genuinely removes
/// both, where our rewrite only ever fixes a column.
pub fn highs_log_lines(report: &HighsPresolveReport) -> Vec<Line<'static>> {
    let t = theme();
    let mut lines = vec![
        Line::from(Span::styled(format!("  {}", report.headline()), Style::default().fg(t.text))),
        Line::from(Span::styled(
            format!(
                "  {} rows, {} cols in \u{2192} {} rows, {} cols out",
                report.rows_before, report.cols_before, report.rows_after, report.cols_after
            ),
            Style::default().fg(t.muted),
        )),
    ];
    if report.skipped_sos > 0 {
        lines.push(Line::from(Span::styled(
            format!("  {} SOS set(s) are not part of the model HiGHS sees", report.skipped_sos),
            Style::default().fg(t.muted),
        )));
    }

    let mut section = |title: String, names: &[String]| {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(format!("  {title}"), Style::default().fg(t.accent).add_modifier(Modifier::BOLD))));
        lines.push(Line::from(""));
        for name in names {
            lines.push(Line::from(Span::styled(format!("  {name}"), Style::default().fg(t.removed))));
        }
    };
    section(format!("{} row(s) removed", report.removed_rows.len()), &report.removed_rows);
    section(format!("{} column(s) removed", report.removed_cols.len()), &report.removed_cols);

    if report.infeasible {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "  presolve proved the model infeasible, so there is no reduced model to compare against",
            Style::default().fg(t.removed),
        )));
    }
    lines
}

/// Draw the presolve log pane over the current frame.
///
/// Takes `&mut App` so the scroll offset can be clamped to the real content
/// height once the visible window is known, matching the diagnostics pane.
pub fn draw_presolve_log(frame: &mut Frame, area: Rect, app: &mut App) {
    let Some(pane) = &mut app.presolve_log else {
        return;
    };
    if area.width == 0 || area.height == 0 {
        return;
    }
    let t = theme();

    // Near-full-screen: the log lines are wide, and a rewrite worth inspecting
    // has more of them than a popup could hold.
    let popup = centred_rect(area, area.width.saturating_sub(4).max(1), area.height.saturating_sub(2).max(1));

    let inner_height = popup.height.saturating_sub(2) as usize;
    let max_scroll = u16::try_from(pane.lines.len().saturating_sub(inner_height)).unwrap_or(u16::MAX);
    pane.scroll = pane.scroll.min(max_scroll);

    let border_style = Style::default().fg(t.accent).add_modifier(Modifier::BOLD);
    let title = " Presolve log  (j/k scroll \u{2022} w write .txt \u{2022} Esc close) ";
    let block = panel_block(border_style).title(Span::styled(title, border_style));

    frame.render_widget(Clear, popup);
    frame.render_widget(Paragraph::new(pane.lines.clone()).block(block).scroll((pane.scroll, 0)), popup);
}

/// Summary of the previous run: the headline plus a per-pass breakdown, so the
/// cascade between rules is visible rather than just its total.
fn last_run_lines(stats: &PresolveStats, muted: ratatui::style::Color, text: ratatui::style::Color) -> Vec<Line<'static>> {
    let mut lines = vec![Line::from(vec![
        Span::styled("  last run  ", Style::default().fg(muted)),
        Span::styled(stats.headline(), Style::default().fg(text)),
    ])];

    for (index, pass) in stats.per_pass.iter().enumerate() {
        lines.push(Line::from(Span::styled(
            format!(
                "    pass {}: -{} rows, {} cols fixed, {} bounds, -{} nnz, {}r/{}c scaled",
                index + 1,
                pass.rows_removed,
                pass.cols_fixed,
                pass.bounds_tightened,
                pass.terms_removed,
                pass.rows_scaled,
                pass.cols_scaled
            ),
            Style::default().fg(muted),
        )));
    }
    lines
}
