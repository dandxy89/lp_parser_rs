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
use crate::presolve::{PresolveStats, Rule};
use crate::theme::theme;
use crate::widgets::{centred_rect, panel_block};

/// Overlay dimensions: wide enough for a rule label plus its one-line detail.
const POPUP_WIDTH: u16 = 78;
/// Border, the rule rows, the detail line, the last-run block, and the key hint.
const POPUP_HEIGHT: u16 = 15;

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
    let hint = " j/k move \u{2022} space toggle \u{2022} a all/none \u{2022} Enter rewrite & solve \u{2022} Esc cancel ";
    let hint_width = u16::try_from(hint.chars().count()).unwrap_or(u16::MAX);
    if popup.width > hint_width && popup.height > 0 {
        let hint_area = Rect { x: popup.x + 2, y: popup.bottom() - 1, width: hint_width, height: 1 };
        frame.render_widget(Paragraph::new(Line::from(Span::styled(hint, Style::default().fg(t.muted)))), hint_area);
    }
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
                "    pass {}: -{} rows, {} cols fixed, {} bounds",
                index + 1,
                pass.rows_removed,
                pass.cols_fixed,
                pass.bounds_tightened
            ),
            Style::default().fg(muted),
        )));
    }
    lines
}
