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
    "  E       What-if: edit constraint RHS & re-solve",
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
    "  ↓ / Ctrl-n  Next result",
    "  ↑ / Ctrl-p  Prev result",
    "  Tab       Complete with selected name",
    "  Enter     Jump to selected entry",
    "  Esc       Cancel search",
    "  ←/→ Ctrl-w/u  Edit query (readline-style)",
    "",
    "  Search Modes (type prefix in pop-up)",
    "  ───────────────────────────────────",
    "  query       Fuzzy (default)",
    "  r:pattern   Regex",
    "  s:text      Substring",
    "  c:text      Content (variables, coefficients, RHS)",
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

/// Inspect-mode keybindings: diff-only actions (filters, tolerance, raw view,
/// delta sorts, side yanks) are omitted since they do not apply to a single file.
const INSPECT_HELP_TEXT: &[&str] = &[
    "",
    "  Single-file inspect mode",
    "  ───────────────────────",
    "",
    "  Navigation                              Other",
    "  ─────────                               ─────",
    "  j / ↓   Down                            /   Search",
    "  k / ↑   Up                              Ctrl-p  Command palette",
    "  g / Home Top                             ?   This help",
    "  G / End  Bottom                          q   Quit",
    "  [ / ]   Prev/next section                Ctrl-C  Force quit",
    "  n / N   Down / Up                       E   What-if: edit RHS & re-solve",
    "  Ctrl-d / Ctrl-u  Half page ↓ / ↑        Clipboard",
    "  Ctrl-f / Ctrl-b  Full page ↓ / ↑        ─────────",
    "  Ctrl-o / Ctrl-i  Jump back / forward    yy  Yank name",
    "  Tab / ⇧Tab  Next / prev panel           Y   Yank detail",
    "  Enter   Go to detail                    w   Export CSV (objectives,",
    "  h / l   Sidebar / Detail                    constraints, variables)",
    "  1–5     Jump to section (5: Numerics)    S   Solve this model",
    "  Esc     Back",
    "",
    "  Search (Telescope-style pop-up)",
    "  ──────────────────────────────",
    "  /         Open search pop-up",
    "  Tab       Complete with selected name",
    "  Enter     Jump to selected entry",
    "  Esc       Cancel search",
    "  query / r:pattern / s:text / c:text   Fuzzy / regex / substring / content",
    "",
    "  Filters, tolerance, raw diff and delta sorts are diff-only",
    "  and are unavailable when inspecting a single file.",
    "",
    "  Mouse: scroll wheel navigates, click selects",
    "",
];

const POPUP_WIDTH: u16 = 60;
/// Desired popup height for a given help text: all lines plus top/bottom borders.
/// Clamped to the terminal at draw time, so on short terminals the content
/// scrolls instead of being silently truncated.
const fn desired_height(text: &[&str]) -> u16 {
    #[allow(clippy::cast_possible_truncation)] // help text is a few dozen lines
    let lines = text.len() as u16;
    lines.saturating_add(2)
}

/// Pre-built diff help text lines, cached to avoid per-frame allocation.
static HELP_LINES: LazyLock<Vec<Line<'static>>> = LazyLock::new(|| {
    let t = theme();
    let text_style = Style::default().fg(t.text);
    HELP_TEXT.iter().map(|&s| Line::from(Span::styled(s, text_style))).collect()
});

/// Pre-built inspect help text lines, cached to avoid per-frame allocation.
static INSPECT_HELP_LINES: LazyLock<Vec<Line<'static>>> = LazyLock::new(|| {
    let t = theme();
    let text_style = Style::default().fg(t.text);
    INSPECT_HELP_TEXT.iter().map(|&s| Line::from(Span::styled(s, text_style))).collect()
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
    let inspect = app.mode == crate::state::AppMode::Inspect;
    let (help_text, help_lines): (&[&str], &LazyLock<Vec<Line<'static>>>) =
        if inspect { (INSPECT_HELP_TEXT, &INSPECT_HELP_LINES) } else { (HELP_TEXT, &HELP_LINES) };

    let popup = super::centred_rect(area, POPUP_WIDTH, desired_height(help_text));

    let inner_height = popup.height.saturating_sub(2) as usize;
    #[allow(clippy::cast_possible_truncation)] // help text is a few dozen lines
    let max_scroll = help_text.len().saturating_sub(inner_height) as u16;
    app.help_scroll = app.help_scroll.min(max_scroll);

    let t = theme();
    let border_style = Style::default().fg(t.added).add_modifier(Modifier::BOLD);
    let title = if max_scroll > 0 { " Keybindings  (j/k scroll · Esc close) " } else { " Keybindings " };
    let block = panel_block(border_style).title(Span::styled(title, border_style));

    let paragraph = Paragraph::new((**help_lines).clone()).block(block).scroll((app.help_scroll, 0));

    frame.render_widget(Clear, popup);
    frame.render_widget(paragraph, popup);
}
