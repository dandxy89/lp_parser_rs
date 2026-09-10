//! `Ctrl+P` command palette overlay.
//!
//! A compact, fuzzy-filterable list of every action that also has a direct
//! keybinding. Renders as one centred floating panel: the query on the top row,
//! a hairline, then the filtered command list (label left, key hint right).
//! Input, list and hints share the one frame — stacked as three bordered boxes
//! they read as a pile of cards rather than a single palette.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, List, ListItem, ListState, ScrollbarState};

use crate::app::App;
use crate::state::PaletteCommand;
use crate::theme::theme;
use crate::widgets::{SELECTION_CURSOR, draw_footer_hint, draw_junction, panel_block, selection_style, separator_rule, zebra_style};

/// Keys the palette itself responds to, shown on its bottom border.
const PALETTE_HINT: &str = " type to filter \u{b7} \u{2191}/\u{2193} move \u{b7} Enter run \u{b7} Esc cancel ";

/// Draw the command palette overlay on top of the current frame.
pub fn draw_palette(frame: &mut Frame, area: Rect, app: &App) {
    // A zero-sized area is an environmental condition (shrunken terminal), not a
    // programming error: drawing into it is a no-op.
    if area.width == 0 || area.height == 0 {
        return;
    }
    let popup = centred_rect(area);
    frame.render_widget(Clear, popup);

    let t = theme();
    let match_count = app.palette.filtered.len();
    let block = panel_block(Style::default().fg(t.accent))
        .title(Span::styled(" Command Palette ", Style::default().fg(t.accent).add_modifier(Modifier::BOLD)))
        .title_top(Line::from(Span::styled(format!(" {match_count} commands "), Style::default().fg(t.muted))).right_aligned());
    let inner = block.inner(popup);
    frame.render_widget(block, popup);
    if inner.width == 0 || inner.height < 3 {
        return;
    }

    let rows = Layout::vertical([
        Constraint::Length(1), // query
        Constraint::Length(1), // hairline
        Constraint::Min(1),    // command list
    ])
    .split(inner);

    crate::widgets::draw_prompt_input(frame, rows[0], &app.palette.query);
    frame.render_widget(separator_rule(Style::default().fg(t.border)), rows[1]);
    // Join the rule to the panel's sides; unstitched it reads as a line laid
    // across the panel rather than as a division of it.
    draw_junction(frame, (popup.x, rows[1].y), "\u{251c}", t.border);
    draw_junction(frame, (popup.right().saturating_sub(1), rows[1].y), "\u{2524}", t.border);
    draw_command_list(frame, rows[2], app);

    // The scrollbar rides the panel border beside the list rows only, so its
    // travel matches what actually scrolls.
    if match_count > rows[2].height as usize {
        let mut scrollbar_state = ScrollbarState::new(match_count).position(app.palette.selected);
        let track = Rect { y: rows[2].y.saturating_sub(1), height: rows[2].height.saturating_add(2), ..popup };
        crate::widgets::render_panel_scrollbar(frame, track, &mut scrollbar_state);
    }

    draw_footer_hint(frame, popup, PALETTE_HINT);
}

/// Draw the filtered command list with the key hint right-aligned per row.
fn draw_command_list(frame: &mut Frame, area: Rect, app: &App) {
    let t = theme();
    // Account for the selection cursor gutter ("▍ ") on the left and a column of
    // air on the right, so the key hint never sits flush against the border.
    let label_width = (area.width as usize).saturating_sub(3);

    let items: Vec<ListItem> = app
        .palette
        .filtered
        .iter()
        .enumerate()
        .map(|(row, &command_index)| {
            let command = PaletteCommand::at(command_index);
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

    let mut state = ListState::default();
    if !app.palette.filtered.is_empty() {
        state.select(Some(app.palette.selected.min(app.palette.filtered.len() - 1)));
    }

    let list = List::new(items).highlight_style(selection_style(true)).highlight_symbol(SELECTION_CURSOR);
    frame.render_stateful_widget(list, area, &mut state);
}

/// Compute a centred rectangle sized for the palette, clamped to the terminal.
fn centred_rect(area: Rect) -> Rect {
    let width = ((area.width * 3) / 5).clamp(40, area.width);
    let height = ((area.height * 4) / 5).clamp(12, area.height);
    super::centred_rect(area, width, height)
}
