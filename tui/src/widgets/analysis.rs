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
                    format!("  {} {label} running\u{2026} {:.1}s", spinner_frame(elapsed), elapsed.as_secs_f64()),
                    Style::default().fg(t.accent),
                )),
                Line::from(Span::styled("  any key to cancel".to_owned(), Style::default().fg(t.muted))),
            ];
            (centred_rect(area, 56.min(area.width), 5.min(area.height)), lines, format!(" {label} "))
        }
        AnalysisState::Failed { label, error } => {
            let lines = vec![
                Line::from(""),
                Line::from(Span::styled(format!("  {error}"), Style::default().fg(t.removed))),
                Line::from(Span::styled("  any key to close".to_owned(), Style::default().fg(t.muted))),
            ];
            (centred_rect(area, 76.min(area.width), 5.min(area.height)), lines, format!(" {label} "))
        }
        AnalysisState::Done { label, pane } => {
            let popup = centred_rect(area, area.width.saturating_sub(4).max(1), area.height.saturating_sub(2).max(1));
            let inner_height = popup.height.saturating_sub(2) as usize;
            let max_scroll = u16::try_from(pane.lines.len().saturating_sub(inner_height)).unwrap_or(u16::MAX);
            pane.scroll = pane.scroll.min(max_scroll);
            let hint = if max_scroll > 0 { "j/k scroll \u{2022} w write \u{2022} Esc close" } else { "w write \u{2022} Esc close" };
            (popup, pane.lines.clone(), format!(" {label}  ({hint}) "))
        }
    };

    let scroll = app.analysis.pane().map_or(0, |pane| pane.scroll);
    let block = panel_block(border_style).title(Span::styled(title, border_style));
    frame.render_widget(Clear, popup);
    frame.render_widget(Paragraph::new(lines).block(block).scroll((scroll, 0)), popup);
}
