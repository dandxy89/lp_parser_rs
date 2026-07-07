//! `Ctrl+P` command palette overlay.
//!
//! A compact, fuzzy-filterable list of every action that also has a direct
//! keybinding. Renders a centred floating pop-up with a query input on top, the
//! filtered command list (label left, key hint right) below, and a hint bar.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, List, ListItem, ListState, Paragraph, ScrollbarState};

use crate::app::App;
use crate::state::PaletteCommand;
use crate::theme::theme;
use crate::widgets::{panel_block, panel_scrollbar, zebra_style};

/// Draw the command palette overlay on top of the current frame.
pub fn draw_palette(frame: &mut Frame, area: Rect, app: &App) {
    // A zero-sized area is an environmental condition (shrunken terminal), not a
    // programming error: drawing into it is a no-op.
    if area.width == 0 || area.height == 0 {
        return;
    }
    let popup = centred_rect(area);
    frame.render_widget(Clear, popup);

    let v_chunks = Layout::vertical([
        Constraint::Length(3), // query input
        Constraint::Min(1),    // command list
        Constraint::Length(3), // hint bar
    ])
    .split(popup);

    draw_input(frame, v_chunks[0], &app.palette.query, app.palette.filtered.len());
    draw_command_list(frame, v_chunks[1], app);
    draw_hints(frame, v_chunks[2]);
}

/// Draw the query input bar at the top of the palette: an editable query on
/// the left (with the real terminal cursor) and the command count on the right.
fn draw_input(frame: &mut Frame, area: Rect, query: &tui_input::Input, match_count: usize) {
    let t = theme();
    let block = panel_block(Style::default().fg(t.accent))
        .title(Span::styled(" Command Palette ", Style::default().fg(t.accent).add_modifier(Modifier::BOLD)));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let right_text = format!("{match_count} commands");
    #[allow(clippy::cast_possible_truncation)] // label is a few dozen columns
    let right_len = (right_text.len() as u16).min(inner.width);
    let chunks = Layout::horizontal([Constraint::Min(0), Constraint::Length(right_len)]).split(inner);

    crate::widgets::draw_prompt_input(frame, chunks[0], query);
    frame.render_widget(Paragraph::new(Span::styled(right_text, Style::default().fg(t.muted))), chunks[1]);
}

/// Draw the filtered command list with the key hint right-aligned per row.
fn draw_command_list(frame: &mut Frame, area: Rect, app: &App) {
    let t = theme();
    let inner_width = area.width.saturating_sub(2) as usize;
    // Account for the highlight symbol gutter ("▶ ") so the hint never clips.
    let label_width = inner_width.saturating_sub(2);

    let items: Vec<ListItem> = app
        .palette
        .filtered
        .iter()
        .enumerate()
        .map(|(row, &command_index)| {
            let command = PaletteCommand::ALL[command_index];
            let label = command.label();
            let hint = command.hint();
            let pad = label_width.saturating_sub(label.len() + hint.len()).max(1);
            let line = Line::from(vec![
                Span::styled(label.to_owned(), Style::default().fg(t.text)),
                Span::raw(" ".repeat(pad)),
                Span::styled(hint.to_owned(), Style::default().fg(t.accent)),
            ]);
            ListItem::new(line).style(zebra_style(row))
        })
        .collect();

    let block = panel_block(Style::default().fg(t.border)).title(" Commands ");

    let mut state = ListState::default();
    if !app.palette.filtered.is_empty() {
        state.select(Some(app.palette.selected.min(app.palette.filtered.len() - 1)));
    }

    let list = List::new(items)
        .block(block)
        .highlight_style(Style::default().bg(t.highlight_bg).add_modifier(Modifier::BOLD))
        .highlight_symbol("\u{25b6} ");
    frame.render_stateful_widget(list, area, &mut state);

    if app.palette.filtered.len() > area.height.saturating_sub(2) as usize {
        let mut scrollbar_state = ScrollbarState::new(app.palette.filtered.len()).position(app.palette.selected);
        frame.render_stateful_widget(panel_scrollbar(), area, &mut scrollbar_state);
    }
}

/// Draw the hint bar at the bottom of the palette.
fn draw_hints(frame: &mut Frame, area: Rect) {
    let t = theme();
    let hints = Line::from(vec![
        Span::styled("  type", Style::default().fg(t.muted)),
        Span::styled(" to filter  ", Style::default().fg(t.muted)),
        Span::styled("\u{2191}/\u{2193}", Style::default().fg(t.accent)),
        Span::styled(" move  ", Style::default().fg(t.muted)),
        Span::styled("Enter", Style::default().fg(t.accent)),
        Span::styled(" run  ", Style::default().fg(t.muted)),
        Span::styled("Esc", Style::default().fg(t.accent)),
        Span::styled(" cancel", Style::default().fg(t.muted)),
    ]);
    frame.render_widget(Paragraph::new(hints).block(panel_block(Style::default().fg(t.muted))), area);
}

/// Compute a centred rectangle sized for the palette, clamped to the terminal.
fn centred_rect(area: Rect) -> Rect {
    let width = ((area.width * 3) / 5).clamp(40, area.width);
    let height = ((area.height * 4) / 5).clamp(12, area.height);
    super::centred_rect(area, width, height)
}
