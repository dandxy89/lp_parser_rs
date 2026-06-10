//! Custom widgets for the TUI application.

use std::borrow::Cow;
use std::time::Duration;

use ratatui::layout::{Constraint, Flex, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::{Block, BorderType, Borders, Scrollbar, ScrollbarOrientation};

use crate::diff_model::DiffKind;
use crate::state::Focus;
use crate::theme::theme;

/// Subdued text for labels, hints, and unchanged values.
/// Theme-aware: uses `DarkGray` on capable terminals, `Gray` on basic 16-colour.
pub fn muted() -> Style {
    Style::new().fg(theme().muted)
}

/// Default body text.
pub fn text() -> Style {
    Style::new().fg(theme().text)
}

/// Bold body text for emphasis.
pub fn bold_text() -> Style {
    text().add_modifier(Modifier::BOLD)
}

/// Arrow separator used between old → new values.
pub const ARROW: &str = "  \u{2192}  ";

/// Return the border [`Style`] for a panel, highlighted when `current == target`.
pub fn focus_border_style(current: Focus, target: Focus) -> Style {
    if current == target {
        Style::default().fg(theme().border_focus).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme().border)
    }
}

/// Standard rounded panel block used by every bordered widget.
pub fn panel_block(border_style: Style) -> Block<'static> {
    Block::default().borders(Borders::ALL).border_type(BorderType::Rounded).border_style(border_style)
}

/// Standard vertical scrollbar with end caps.
pub fn panel_scrollbar() -> Scrollbar<'static> {
    Scrollbar::new(ScrollbarOrientation::VerticalRight).begin_symbol(Some("\u{25b2}")).end_symbol(Some("\u{25bc}"))
}

/// Background style for alternating rows, keyed on the absolute item index so
/// stripes stay stable while scrolling.
pub fn zebra_style(absolute_index: usize) -> Style {
    if absolute_index % 2 == 1 { Style::default().bg(theme().zebra_bg) } else { Style::default() }
}

/// Braille spinner frames, advanced every 100ms of elapsed time.
const SPINNER_FRAMES: [&str; 10] =
    ["\u{280b}", "\u{2819}", "\u{2839}", "\u{2838}", "\u{283c}", "\u{2834}", "\u{2826}", "\u{2827}", "\u{2807}", "\u{280f}"];

/// Pick the spinner frame for an elapsed duration. Deriving the frame from
/// elapsed time means callers need no extra animation state — the 50ms event
/// tick redraws often enough to animate.
pub fn spinner_frame(elapsed: Duration) -> &'static str {
    SPINNER_FRAMES[(elapsed.as_millis() / 100) as usize % SPINNER_FRAMES.len()]
}

/// Truncate `name` to at most `max_width` display columns, appending `…` when
/// it does not fit. Names in LP files are ASCII in practice, but truncation is
/// performed on a char boundary so multibyte input cannot panic.
pub fn truncate_with_ellipsis(name: &str, max_width: usize) -> Cow<'_, str> {
    debug_assert!(max_width >= 2, "truncate_with_ellipsis needs room for at least one char plus ellipsis");
    if name.chars().count() <= max_width || max_width < 2 {
        return Cow::Borrowed(name);
    }
    let mut truncated: String = name.chars().take(max_width - 1).collect();
    truncated.push('\u{2026}');
    Cow::Owned(truncated)
}

/// A long horizontal rule sliced by [`rule_str`]; 120 `─` characters.
const RULE: &str =
    "────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────";

/// Return a `─` rule of the given display width (clamped to 120 columns)
/// without allocating. `─` is 3 bytes in UTF-8, so the slice is exact.
pub fn rule_str(width: usize) -> &'static str {
    let width = width.min(RULE.len() / 3);
    &RULE[..width * 3]
}

/// Build an inline gauge bar like `▐███░░░░░▌` for a fraction in `[0, 1]`.
///
/// `cells` is the number of fill cells between the end caps.
pub fn gauge_bar(fraction: f64, cells: usize) -> String {
    debug_assert!(cells > 0, "gauge_bar needs at least one cell");
    let clamped = fraction.clamp(0.0, 1.0);
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // clamped to [0, cells]
    let filled = ((clamped * cells as f64).round() as usize).min(cells);
    let mut bar = String::with_capacity(2 + cells * 3);
    bar.push('\u{2590}');
    for _ in 0..filled {
        bar.push('\u{2588}');
    }
    for _ in filled..cells {
        bar.push('\u{2591}');
    }
    bar.push('\u{258c}');
    bar
}

pub mod detail;
pub mod help;
pub mod numerics;
pub mod raw_diff;
pub mod search_popup;
pub mod sidebar;
pub mod solve;
pub mod status_bar;
pub mod summary;

/// Map a [`DiffKind`] to its theme-aware display colour.
pub fn kind_colour(kind: DiffKind) -> Color {
    let t = theme();
    match kind {
        DiffKind::Added => t.added,
        DiffKind::Removed => t.removed,
        DiffKind::Modified => t.modified,
        DiffKind::Renamed => t.info,
    }
}

/// Return a [`Style`] with the foreground set to the colour for `kind`.
pub fn kind_style(kind: DiffKind) -> Style {
    Style::default().fg(kind_colour(kind))
}

/// Return a fixed-width prefix glyph for the given [`DiffKind`].
pub const fn kind_prefix(kind: DiffKind) -> &'static str {
    match kind {
        DiffKind::Added => "[+]",
        DiffKind::Removed => "[-]",
        DiffKind::Modified => "[~]",
        DiffKind::Renamed => "[>]",
    }
}

/// Compute a centred rectangle of the given dimensions, clamped to the terminal area.
pub fn centred_rect(area: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(area.width);
    let height = height.min(area.height);

    let vertical = Layout::vertical([Constraint::Length(height)]).flex(Flex::Center).split(area);
    let horizontal = Layout::horizontal([Constraint::Length(width)]).flex(Flex::Center).split(vertical[0]);

    horizontal[0]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_truncate_fits_unchanged() {
        assert_eq!(truncate_with_ellipsis("short", 10), "short");
        assert_eq!(truncate_with_ellipsis("exact", 5), "exact");
    }

    #[test]
    fn test_truncate_overflow_gets_ellipsis() {
        assert_eq!(truncate_with_ellipsis("overflowing", 6), "overf\u{2026}");
        assert_eq!(truncate_with_ellipsis("ab", 2), "ab");
        assert_eq!(truncate_with_ellipsis("abc", 2), "a\u{2026}");
    }

    #[test]
    fn test_truncate_multibyte_safe() {
        // 4 chars, max 3 → 2 chars + ellipsis, no panic on char boundaries.
        assert_eq!(truncate_with_ellipsis("\u{0394}\u{0394}\u{0394}\u{0394}", 3), "\u{0394}\u{0394}\u{2026}");
    }

    #[test]
    fn test_gauge_bar_boundaries() {
        assert_eq!(gauge_bar(0.0, 4), "\u{2590}\u{2591}\u{2591}\u{2591}\u{2591}\u{258c}");
        assert_eq!(gauge_bar(1.0, 4), "\u{2590}\u{2588}\u{2588}\u{2588}\u{2588}\u{258c}");
        assert_eq!(gauge_bar(0.5, 4), "\u{2590}\u{2588}\u{2588}\u{2591}\u{2591}\u{258c}");
        // Out-of-range fractions are clamped, never panic.
        assert_eq!(gauge_bar(-1.0, 4), gauge_bar(0.0, 4));
        assert_eq!(gauge_bar(2.0, 4), gauge_bar(1.0, 4));
    }

    #[test]
    fn test_rule_str_widths() {
        assert_eq!(rule_str(0), "");
        assert_eq!(rule_str(3), "───");
        assert_eq!(rule_str(3).chars().count(), 3);
        // Clamped to the backing constant's width.
        assert_eq!(rule_str(999).chars().count(), 120);
    }

    #[test]
    fn test_spinner_frame_cycles() {
        assert_eq!(spinner_frame(Duration::from_millis(0)), SPINNER_FRAMES[0]);
        assert_eq!(spinner_frame(Duration::from_millis(150)), SPINNER_FRAMES[1]);
        assert_eq!(spinner_frame(Duration::from_millis(1000)), SPINNER_FRAMES[0]);
    }
}
