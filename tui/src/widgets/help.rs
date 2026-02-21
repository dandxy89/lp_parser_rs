//! Help pop-up overlay widget.
//!
//! Renders a centred pop-up listing all keybindings grouped by category.

use ratatui::Frame;
use ratatui::layout::Rect;
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
    "  n / N   Down/Up",
    "  Ctrl-d  Half page ↓                     Clipboard",
    "  Ctrl-u  Half page ↑                     ─────────",
    "  Ctrl-f  Full page ↓                     y   Yank name",
    "  Ctrl-b  Full page ↑                     Y   Yank detail",
    "  Ctrl-o  Jump back",
    "  Ctrl-i  Jump forward",
    "  S       Solve LP file",
    "  Tab     Next panel",
    "  ⇧Tab    Prev panel",
    "  Enter   Go to detail",
    "  h / l   Sidebar / Detail",
    "  1–4     Jump to section",
    "  Esc     Back",
    "",
    "  Search (Telescope-style pop-up)",
    "  ──────────────────────────────",
    "  /         Open search pop-up",
    "  j / ↓     Next result (in pop-up)",
    "  k / ↑     Prev result (in pop-up)",
    "  Tab       Complete with selected name",
    "  Enter     Jump to selected entry",
    "  Esc       Cancel search",
    "",
    "  Search Modes (type prefix in pop-up)",
    "  ───────────────────────────────────",
    "  query       Fuzzy (default)",
    "  r:pattern   Regex",
    "  s:text      Substring",
    "",
    "  Mouse: scroll wheel navigates, click selects",
    "",
];

const POPUP_WIDTH: u16 = 60;
const POPUP_HEIGHT: u16 = 43;

/// Draw a centred help pop-up overlay on top of the current frame.
pub fn draw_help(frame: &mut Frame, area: Rect) {
    debug_assert!(area.width > 0 && area.height > 0, "help overlay area must be non-zero");
    let popup = super::centred_rect(area, POPUP_WIDTH, POPUP_HEIGHT);

    let lines: Vec<Line<'_>> = HELP_TEXT.iter().map(|&s| Line::from(Span::styled(s, Style::default().fg(Color::White)))).collect();

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Green).add_modifier(Modifier::BOLD))
        .title(Span::styled(" Keybindings ", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)));

    let paragraph = Paragraph::new(lines).block(block);

    frame.render_widget(Clear, popup);
    frame.render_widget(paragraph, popup);
}
