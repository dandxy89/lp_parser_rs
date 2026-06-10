//! Help pop-up overlay widget.
//!
//! Renders a centred pop-up listing all keybindings grouped by category.

use std::sync::LazyLock;

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph};

use crate::theme::theme;
use crate::widgets::panel_block;

const HELP_TEXT: &[&str] = &[
    "",
    "  Navigation          Filters             Other",
    "  ─────────           ───────             ─────",
    "  j / ↓   Down        a   All             /   Search",
    "  k / ↑   Up          +   Added           ?   This help",
    "  g / Home Top         -   Removed         q   Quit",
    "  G / End  Bottom      m   Modified        Ctrl-C  Force quit",
    "                       =   Renamed",
    "                       o   Ignore order",
    "  n       Next (down)",
    "  N       Previous (up)",
    "  Ctrl-d  Half page ↓                     Clipboard",
    "  Ctrl-u  Half page ↑                     ─────────",
    "  Ctrl-f  Full page ↓                     yy  Yank name",
    "  Ctrl-b  Full page ↑                     yo  Yank old (file 1)",
    "                                          yn  Yank new (file 2)",
    "                                          Y   Yank detail",
    "                                          w   Export CSV",
    "  Ctrl-o  Jump back",
    "  Ctrl-i  Jump forward",
    "  r       Toggle raw text view",
    "  s       Cycle sort: name \u{2192} |\u{394}| \u{2192} rel\u{394}",
    "  t / T   Cycle rel / abs tolerance (rebuilds diff;",
    "          a non-preset CLI value restarts at off)",
    "  S       Solve problem",
    "  Tab     Next panel",
    "  ⇧Tab    Prev panel",
    "  Enter   Go to detail",
    "  h / l   Sidebar / Detail",
    "  1–5     Jump to section (5: Numerics)",
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
    "  e         Diagnose infeasibility",
    "  1–5       Switch tab",
    "  Esc       Close solver overlay",
    "",
    "  Mouse: scroll wheel navigates, click selects",
    "",
];

const POPUP_WIDTH: u16 = 60;
// Must equal HELP_TEXT.len() + 2 (top/bottom borders); see debug_assert in draw_help.
const POPUP_HEIGHT: u16 = 61;

/// Pre-built help text lines, cached to avoid per-frame allocation.
static HELP_LINES: LazyLock<Vec<Line<'static>>> = LazyLock::new(|| {
    let t = theme();
    let text_style = Style::default().fg(t.text);
    HELP_TEXT.iter().map(|&s| Line::from(Span::styled(s, text_style))).collect()
});

/// Draw a centred help pop-up overlay on top of the current frame.
pub fn draw_help(frame: &mut Frame, area: Rect) {
    // A zero-sized area is an environmental condition (shrunken terminal), not a
    // programming error: drawing into it is a no-op.
    if area.width == 0 || area.height == 0 {
        return;
    }
    debug_assert!(POPUP_HEIGHT as usize == HELP_TEXT.len() + 2, "POPUP_HEIGHT must match help text length plus borders");
    let popup = super::centred_rect(area, POPUP_WIDTH, POPUP_HEIGHT);

    let t = theme();
    let border_style = Style::default().fg(t.added).add_modifier(Modifier::BOLD);
    let block = panel_block(border_style).title(Span::styled(" Keybindings ", border_style));

    let paragraph = Paragraph::new(HELP_LINES.clone()).block(block);

    frame.render_widget(Clear, popup);
    frame.render_widget(paragraph, popup);
}
