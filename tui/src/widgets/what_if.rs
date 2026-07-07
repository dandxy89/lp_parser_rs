//! What-if prompt overlay: edit a constraint's RHS and re-solve.
//!
//! A small centred input box opened with `E` on a selected constraint. On
//! confirm the app clones the baseline problem, applies the new RHS, and
//! launches a baseline-vs-modified comparison solve (the standard `DoneBoth`
//! comparison view).

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph};

use crate::state::WhatIfPrompt;
use crate::theme::theme;
use crate::widgets::{centred_rect, panel_block, truncate_with_ellipsis};

/// Overlay dimensions: wide enough for a constraint name plus a number.
const POPUP_WIDTH: u16 = 62;
const POPUP_HEIGHT: u16 = 7;

/// Draw the what-if prompt overlay on top of the current frame.
pub fn draw_what_if(frame: &mut Frame, area: Rect, prompt: &WhatIfPrompt) {
    const INPUT_LABEL: &str = " new rhs     ";
    if area.width == 0 || area.height == 0 {
        return;
    }
    let t = theme();
    let popup = centred_rect(area, POPUP_WIDTH.min(area.width), POPUP_HEIGHT.min(area.height));
    frame.render_widget(Clear, popup);

    let inner_width = popup.width.saturating_sub(4) as usize;
    let name = truncate_with_ellipsis(&prompt.constraint_name, inner_width.saturating_sub(12));

    let mut lines = vec![
        Line::from(vec![
            Span::styled(" constraint ", Style::default().fg(t.muted)),
            Span::styled(name.into_owned(), Style::default().fg(t.text).add_modifier(Modifier::BOLD)),
        ]),
        Line::from(vec![
            Span::styled(" current rhs ", Style::default().fg(t.muted)),
            Span::styled(format!("{}", prompt.current_rhs), Style::default().fg(t.text)),
        ]),
        Line::from(vec![
            Span::styled(INPUT_LABEL, Style::default().fg(t.muted)),
            Span::styled(prompt.input.value().to_owned(), Style::default().fg(t.accent).add_modifier(Modifier::BOLD)),
        ]),
    ];
    if let Some(error) = &prompt.error {
        lines.push(Line::from(Span::styled(format!(" {error}"), Style::default().fg(t.error))));
    } else {
        lines.push(Line::from(Span::styled(" Enter solve baseline vs what-if \u{2022} Esc cancel", Style::default().fg(t.muted))));
    }

    let block = panel_block(Style::default().fg(t.accent))
        .title(Span::styled(" What-if: edit RHS & re-solve ", Style::default().fg(t.accent).add_modifier(Modifier::BOLD)));
    frame.render_widget(Paragraph::new(lines).block(block), popup);

    // Place the real terminal cursor at the edit position on the input line
    // (row 3 inside the border; column after the label).
    #[allow(clippy::cast_possible_truncation)] // label and input are far narrower than u16::MAX
    let cursor_x = popup.x + 1 + INPUT_LABEL.len() as u16 + prompt.input.visual_cursor() as u16;
    let cursor_y = popup.y + 3;
    if cursor_x < popup.right().saturating_sub(1) && cursor_y < popup.bottom().saturating_sub(1) {
        frame.set_cursor_position((cursor_x, cursor_y));
    }
}
