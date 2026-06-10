//! Centralised colour theme for the TUI.
//!
//! All widget code should use `theme()` to obtain semantic colours rather than
//! hard-coding `Color::*` constants.  This ensures the palette stays consistent
//! and can adapt to light or dark terminal backgrounds.
//!
//! The active palette is selected once at startup via [`init_theme`]; widgets
//! then read it through [`theme()`].

use std::sync::OnceLock;

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
    /// Border colour for unfocused panels (dim, so focus stands out).
    pub border: Color,
    /// Subtle background for alternating (zebra-striped) rows.
    pub zebra_bg: Color,
}

/// Theme mode selected via `--theme` or detected from the environment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeMode {
    Dark,
    Light,
}

/// Dark palette — the default, tuned for dark terminal backgrounds.
static DARK_THEME: Theme = Theme {
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
    border: Color::Indexed(240),
    zebra_bg: Color::Indexed(236),
};

/// Light palette — darker foregrounds that stay readable on light backgrounds.
/// Indexed colours are used where the ANSI base colour (e.g. yellow) would be
/// near-invisible on white.
static LIGHT_THEME: Theme = Theme {
    added: Color::Indexed(28),     // dark green
    removed: Color::Indexed(124),  // dark red
    modified: Color::Indexed(130), // dark orange
    muted: Color::Indexed(245),    // mid grey
    accent: Color::Indexed(30),    // teal
    text: Color::Black,
    info: Color::Indexed(30),
    error: Color::Indexed(124),
    warning: Color::Indexed(130),
    highlight_bg: Color::Indexed(153), // pale blue
    border_focus: Color::Indexed(30),
    secondary_accent: Color::Indexed(90), // purple
    border: Color::Indexed(250),
    zebra_bg: Color::Indexed(253),
};

/// The palette chosen at startup. Falls back to dark when never initialised
/// (e.g. in unit tests that render widgets without going through `main`).
static ACTIVE_THEME: OnceLock<&'static Theme> = OnceLock::new();

/// Select the active palette. Call once at startup, before the first draw.
///
/// Subsequent calls are ignored — the palette is fixed for the process
/// lifetime so cached lines built against it never go stale.
pub fn init_theme(mode: ThemeMode) {
    let palette = match mode {
        ThemeMode::Dark => &DARK_THEME,
        ThemeMode::Light => &LIGHT_THEME,
    };
    // A second call (only possible from tests) keeps the first palette.
    let _ = ACTIVE_THEME.set(palette);
}

/// Return the active theme.
pub fn theme() -> &'static Theme {
    ACTIVE_THEME.get().copied().unwrap_or(&DARK_THEME)
}

/// Detect the theme mode from a `COLORFGBG` value (format `"<fg>;<bg>"` or
/// `"<fg>;<default>;<bg>"`). Background 7 or 15 means a light terminal.
///
/// Returns `None` when the value is missing or malformed — the variable is
/// advisory only, so callers fall back to dark.
pub fn detect_mode_from_colorfgbg(value: &str) -> Option<ThemeMode> {
    let background: u8 = value.rsplit(';').next()?.trim().parse().ok()?;
    match background {
        7 | 15 => Some(ThemeMode::Light),
        _ => Some(ThemeMode::Dark),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_colorfgbg_light_backgrounds() {
        assert_eq!(detect_mode_from_colorfgbg("0;15"), Some(ThemeMode::Light));
        assert_eq!(detect_mode_from_colorfgbg("0;7"), Some(ThemeMode::Light));
        assert_eq!(detect_mode_from_colorfgbg("0;default;15"), Some(ThemeMode::Light));
    }

    #[test]
    fn test_colorfgbg_dark_backgrounds() {
        assert_eq!(detect_mode_from_colorfgbg("15;0"), Some(ThemeMode::Dark));
        assert_eq!(detect_mode_from_colorfgbg("7;8"), Some(ThemeMode::Dark));
    }

    #[test]
    fn test_colorfgbg_malformed() {
        assert_eq!(detect_mode_from_colorfgbg(""), None);
        assert_eq!(detect_mode_from_colorfgbg("garbage"), None);
        assert_eq!(detect_mode_from_colorfgbg("15;"), None);
        assert_eq!(detect_mode_from_colorfgbg("15;256"), None);
    }
}
