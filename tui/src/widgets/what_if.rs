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

    // Columns the input may use: the popup less its borders and the label,
    // less one so the cursor has a cell after the last character.
    let field_width = (popup.width.saturating_sub(2) as usize).saturating_sub(INPUT_LABEL.len() + 1);
    // Scrolled so the cursor stays in view however long the input grows.
    let scroll = prompt.input.visual_scroll(field_width);
    let visible_input = skip_columns(prompt.input.value(), scroll);

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
            Span::styled(visible_input.to_owned(), Style::default().fg(t.accent).add_modifier(Modifier::BOLD)),
        ]),
    ];
    if let Some(error) = &prompt.error {
        lines.push(Line::from(Span::styled(format!(" {error}"), Style::default().fg(t.removed))));
    } else {
        let mut hint = vec![Span::raw(" ")];
        hint.extend(crate::widgets::key_hint_spans(&["Enter:solve baseline vs what-if", "Esc:cancel"]));
        lines.push(Line::from(hint));
    }

    let block = panel_block(Style::default().fg(t.accent))
        .title(Span::styled(" What-if: edit RHS & re-solve ", Style::default().fg(t.accent).add_modifier(Modifier::BOLD)));
    frame.render_widget(Paragraph::new(lines).block(block), popup);

    // Place the real terminal cursor at the edit position on the input line
    // (row 3 inside the border; column after the label).
    let visual_cursor = prompt.input.visual_cursor().saturating_sub(scroll);
    debug_assert!(visual_cursor <= field_width, "the scroll keeps the cursor inside the field");
    #[allow(clippy::cast_possible_truncation)] // bounded by the popup width, itself a u16
    let cursor_x = popup.x + 1 + INPUT_LABEL.len() as u16 + visual_cursor as u16;
    let cursor_y = popup.y + 3;
    if cursor_x < popup.right().saturating_sub(1) && cursor_y < popup.bottom().saturating_sub(1) {
        frame.set_cursor_position((cursor_x, cursor_y));
    }
}

/// `text` with its first `columns` display columns dropped (as counted by
/// `tui_input`'s own scroll, so the two stay in step).
fn skip_columns(text: &str, columns: usize) -> &str {
    let mut skipped = 0;
    for (index, c) in text.char_indices() {
        if skipped >= columns {
            return &text[index..];
        }
        skipped += unicode_width::UnicodeWidthChar::width(c).unwrap_or(0);
    }
    ""
}

#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    use super::*;

    fn prompt(value: &str) -> WhatIfPrompt {
        WhatIfPrompt { constraint_name: "c1".to_owned(), current_rhs: 2.0, input: tui_input::Input::new(value.to_owned()), error: None }
    }

    /// Regression: the input never scrolled, so once it outgrew the field the
    /// tail (where the cursor is) was clipped and the cursor vanished.
    #[test]
    fn a_long_input_scrolls_to_keep_the_cursor_in_view() {
        let value = format!("{}END", "1".repeat(120));
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).expect("test terminal must build");
        terminal.draw(|frame| draw_what_if(frame, frame.area(), &prompt(&value))).expect("draw must succeed");

        let text: String = terminal.backend().buffer().content().iter().map(ratatui::buffer::Cell::symbol).collect();
        assert!(text.contains("END"), "the tail being edited must be visible");
        let cursor = terminal.get_cursor_position().expect("cursor position is readable");
        let popup = centred_rect(Rect::new(0, 0, 80, 24), POPUP_WIDTH, POPUP_HEIGHT);
        assert!(cursor.x > popup.x && cursor.x < popup.right() - 1, "the cursor sits inside the popup, got {cursor:?}");
        assert_eq!(cursor.y, popup.y + 3, "on the input row");
    }

    #[test]
    fn skip_columns_counts_display_width() {
        assert_eq!(skip_columns("abcdef", 0), "abcdef");
        assert_eq!(skip_columns("abcdef", 2), "cdef");
        assert_eq!(skip_columns("ab", 5), "");
    }
}
