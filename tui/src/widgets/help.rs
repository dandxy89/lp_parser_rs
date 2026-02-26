//! Help pop-up overlay widget.
//!
//! Renders a centred pop-up listing all keybindings grouped by category.

use std::sync::LazyLock;

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

use crate::theme::theme;

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
    "  Ctrl-f  Full page ↓                     yy  Yank name",
    "  Ctrl-b  Full page ↑                     yo  Yank old (file 1)",
    "                                          yn  Yank new (file 2)",
    "                                          Y   Yank detail",
    "                                          w   Export CSV",
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
    "  Solver Results (after S → solve)",
    "  ────────────────────────────────",
    "  y         Yank results to clipboard",
    "  w         Write diff to CSV (both mode)",
    "  d         Toggle diff-only (both mode)",
    "  t / T     Cycle delta threshold (both mode)",
    "  1–4       Switch tab",
    "  Esc       Close solver overlay",
    "",
    "  Mouse: scroll wheel navigates, click selects",
    "",
];

const POPUP_WIDTH: u16 = 60;
const POPUP_HEIGHT: u16 = 53;

/// Pre-built help text lines, cached to avoid per-frame allocation.
static HELP_LINES: LazyLock<Vec<Line<'static>>> = LazyLock::new(|| {
    let t = theme();
    let text_style = Style::default().fg(t.text);
    HELP_TEXT.iter().map(|&s| Line::from(Span::styled(s, text_style))).collect()
});

/// Draw a centred help pop-up overlay on top of the current frame.
pub fn draw_help(frame: &mut Frame, area: Rect) {
    debug_assert!(area.width > 0 && area.height > 0, "help overlay area must be non-zero");
    let popup = super::centred_rect(area, POPUP_WIDTH, POPUP_HEIGHT);

    let t = theme();
    let border_style = Style::default().fg(t.added).add_modifier(Modifier::BOLD);
    let block = Block::default().borders(Borders::ALL).border_style(border_style).title(Span::styled(" Keybindings ", border_style));

    let paragraph = Paragraph::new(HELP_LINES.clone()).block(block);

    frame.render_widget(Clear, popup);
    frame.render_widget(paragraph, popup);
}
