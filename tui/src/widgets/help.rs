//! Help popup overlay widget.
//!
//! Renders a centred popup listing all keybindings grouped by category.

use ratatui::Frame;
use ratatui::layout::{Constraint, Flex, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

const HELP_TEXT: &[&str] = &[
    "",
    "  Navigation          Filters             Other",
    "  ─────────           ───────             ─────",
    "  j / ↓   Down        a   All             /   Search",
    "  k / ↑   Up          +   Added           ?   This help",
    "  g / Home Top         -   Removed         q   Quit",
    "  G / End  Bottom      m   Modified        Ctrl-C  Force quit",
    "  Tab     Next panel",
    "  ⇧Tab    Prev panel",
    "  Enter   Go to detail",
    "  h / l   Sidebar / Detail",
    "  1–4     Jump to section",
    "  Esc     Back / Clear search",
    "",
    "  Search Modes",
    "  ────────────",
    "  /query      Fuzzy (default)",
    "  /r:pattern  Regex",
    "  /s:text     Substring",
    "",
];

const POPUP_WIDTH: u16 = 60;
const POPUP_HEIGHT: u16 = 24;

/// Draw a centred help popup overlay on top of the current frame.
pub fn draw_help(frame: &mut Frame, area: Rect) {
    let popup = centred_rect(area);

    let lines: Vec<Line<'_>> = HELP_TEXT.iter().map(|&s| Line::from(Span::styled(s, Style::default().fg(Color::White)))).collect();

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Green).add_modifier(Modifier::BOLD))
        .title(Span::styled(" Keybindings ", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)));

    let paragraph = Paragraph::new(lines).block(block);

    frame.render_widget(Clear, popup);
    frame.render_widget(paragraph, popup);
}

/// Compute a centred rectangle clamped to the terminal size.
fn centred_rect(area: Rect) -> Rect {
    let width = POPUP_WIDTH.min(area.width);
    let height = POPUP_HEIGHT.min(area.height);

    let vertical = Layout::vertical([Constraint::Length(height)]).flex(Flex::Center).split(area);
    let horizontal = Layout::horizontal([Constraint::Length(width)]).flex(Flex::Center).split(vertical[0]);

    horizontal[0]
}
