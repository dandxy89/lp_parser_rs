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
            Span::styled(" new rhs     ", Style::default().fg(t.muted)),
            Span::styled(prompt.input.clone(), Style::default().fg(t.accent).add_modifier(Modifier::BOLD)),
            Span::styled("\u{2588}", Style::default().fg(t.accent)),
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
}
