//! Centralised colour theme for the TUI.
//!
//! All widget code should use `theme()` to obtain semantic colours rather than
//! hard-coding `Color::*` constants.  This ensures the palette stays consistent
//! and can later adapt to the terminal's colour capability tier.

use ratatui::style::Color;

/// Semantic colour palette for the TUI.
#[derive(Debug, Clone, Copy)]
pub struct Theme {
    /// Colour for added / new entries.
    pub added: Color,
    /// Colour for removed / deleted entries.
    pub removed: Color,
    /// Colour for modified / changed entries.
    pub modified: Color,
    /// Subdued text for labels, hints, unchanged values.
    pub muted: Color,
    /// Primary accent (headings, prompts, active borders).
    pub accent: Color,
    /// Default body text.
    pub text: Color,
    /// Informational highlights (counts, stats).
    pub info: Color,
    /// Error / failure indicators.
    pub error: Color,
    /// Warning indicators.
    pub warning: Color,
    /// Background for highlighted / selected rows.
    pub highlight_bg: Color,
    /// Border colour for the focused panel.
    pub border_focus: Color,
    /// Secondary accent (e.g. magenta highlights in diff views).
    pub secondary_accent: Color,
}

/// The default theme — a plain const eliminates the atomic load that
/// `LazyLock` would add on every `theme()` call.
static DEFAULT_THEME: Theme = Theme {
    added: Color::Green,
    removed: Color::Red,
    modified: Color::Yellow,
    muted: Color::DarkGray,
    accent: Color::Cyan,
    text: Color::White,
    info: Color::Cyan,
    error: Color::Red,
    warning: Color::Yellow,
    highlight_bg: Color::Blue,
    border_focus: Color::Cyan,
    secondary_accent: Color::Magenta,
};

/// Return the active theme.
pub fn theme() -> &'static Theme {
    &DEFAULT_THEME
}
