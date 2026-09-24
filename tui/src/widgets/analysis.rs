//! The generic overlay for a slow, read-only analysis ([`AnalysisState`]).
//!
//! Every such analysis renders the same way — a spinner while it runs, a
//! scrollable report when it finishes, an error if it does not — so the pane
//! itself is written once and the individual analyses only supply lines.

use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph};

use crate::state::AnalysisState;
use crate::theme::theme;
use crate::widgets::{centred_rect, panel_block, spinner_frame};

/// Draw the analysis overlay over the current frame.
///
/// Takes `&mut App` so the scroll offset can be clamped to the real content
/// height once the visible window is known, matching the diagnostics pane.
pub fn draw_analysis(frame: &mut ratatui::Frame, area: Rect, app: &mut crate::app::App) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let t = theme();
    let border_style = Style::default().fg(t.accent).add_modifier(Modifier::BOLD);

    // Running and failed states are a couple of lines; a finished report wants
    // the whole screen, since the value is in comparing rows against each other.
    let (popup, lines, title) = match &mut app.analysis {
        AnalysisState::Idle => return,
        AnalysisState::Running { label, started } => {
            let elapsed = started.elapsed();
            let lines = vec![
                Line::from(""),
                Line::from(Span::styled(
                    format!("  {} {label} running\u{2026} {}", spinner_frame(elapsed), crate::format::fmt_duration(elapsed)),
                    Style::default().fg(t.accent),
                )),
                indented_hints("any key:cancel"),
            ];
            (centred_rect(area, 56.min(area.width), 5.min(area.height)), lines, Line::styled(format!(" {label} "), border_style))
        }
        AnalysisState::Failed { label, error } => {
            let lines = vec![
                Line::from(""),
                Line::from(Span::styled(format!("  {error}"), Style::default().fg(t.removed))),
                indented_hints("any key:close"),
            ];
            (centred_rect(area, 76.min(area.width), 5.min(area.height)), lines, Line::styled(format!(" {label} "), border_style))
        }
        AnalysisState::Done { label, pane } => {
            let popup = crate::widgets::report_rect(area);
            let inner_height = popup.height.saturating_sub(2) as usize;
            let max_scroll = u16::try_from(pane.lines.len().saturating_sub(inner_height)).unwrap_or(u16::MAX);
            pane.scroll = pane.scroll.min(max_scroll);
            let hints = if max_scroll > 0 { "j/k:scroll  w:write  Esc:close" } else { "w:write  Esc:close" };
            (popup, pane.lines.clone(), crate::widgets::title_with_hints(label, hints, border_style))
        }
    };

    let scroll = app.analysis.pane().map_or(0, |pane| pane.scroll);
    let block = panel_block(border_style).title(title);
    frame.render_widget(Clear, popup);
    frame.render_widget(Paragraph::new(lines).block(block).scroll((scroll, 0)), popup);
}

/// Packed key hints as a body line, indented to the pane's text column.
fn indented_hints(packed: &'static str) -> Line<'static> {
    let mut spans = vec![Span::raw("  ")];
    spans.extend(crate::widgets::key_hint_spans(&crate::widgets::hint_pairs(packed)));
    Line::from(spans)
}
