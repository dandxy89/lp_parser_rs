//! Help pop-up overlay widget.
//!
//! Renders a centred pop-up listing all keybindings grouped by category.

use std::sync::LazyLock;

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph};

use crate::app::App;
use crate::theme::theme;
use crate::widgets::panel_block;

const HELP_TEXT: &[&str] = &[
    "",
    "  Navigation          Filters             Other",
    "  ─────────           ───────             ─────",
    "  j / ↓   Down        a   All             /   Search",
    "  k / ↑   Up          +   Added           Ctrl-p  Command palette",
    "  g / Home Top         -   Removed         ?   This help",
    "  G / End  Bottom      m   Modified        q   Quit",
    "  [ / ]   Prev/next    =   Renamed         Ctrl-C  Force quit",
    "          section      o   Ignore order",
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
/// Desired popup height: all help lines plus top/bottom borders. Clamped to the
/// terminal at draw time, so on short terminals the content scrolls instead of
/// being silently truncated.
const fn desired_height() -> u16 {
    #[allow(clippy::cast_possible_truncation)] // help text is a few dozen lines
    let lines = HELP_TEXT.len() as u16;
    lines.saturating_add(2)
}

/// Pre-built help text lines, cached to avoid per-frame allocation.
static HELP_LINES: LazyLock<Vec<Line<'static>>> = LazyLock::new(|| {
    let t = theme();
    let text_style = Style::default().fg(t.text);
    HELP_TEXT.iter().map(|&s| Line::from(Span::styled(s, text_style))).collect()
});

/// Draw a centred help pop-up overlay on top of the current frame.
///
/// Takes `&mut App` so the scroll offset can be clamped to the real content
/// height: `G`/End sets `help_scroll` to `u16::MAX`, which is corrected here
/// once the visible window is known.
pub fn draw_help(frame: &mut Frame, area: Rect, app: &mut App) {
    // A zero-sized area is an environmental condition (shrunken terminal), not a
    // programming error: drawing into it is a no-op.
    if area.width == 0 || area.height == 0 {
        return;
    }
    let popup = super::centred_rect(area, POPUP_WIDTH, desired_height());

    let inner_height = popup.height.saturating_sub(2) as usize;
    #[allow(clippy::cast_possible_truncation)] // help text is a few dozen lines
    let max_scroll = HELP_TEXT.len().saturating_sub(inner_height) as u16;
    app.help_scroll = app.help_scroll.min(max_scroll);

    let t = theme();
    let border_style = Style::default().fg(t.added).add_modifier(Modifier::BOLD);
    let title = if max_scroll > 0 { " Keybindings  (j/k scroll · Esc close) " } else { " Keybindings " };
    let block = panel_block(border_style).title(Span::styled(title, border_style));

    let paragraph = Paragraph::new(HELP_LINES.clone()).block(block).scroll((app.help_scroll, 0));

    frame.render_widget(Clear, popup);
    frame.render_widget(paragraph, popup);
}
