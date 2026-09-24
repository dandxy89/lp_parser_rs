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
///
/// Severity reuses the diff colours: errors are drawn in `removed`, warnings in
/// `modified`, and informational highlights in `accent`. Every palette below
/// gave those pairs the same value, so they were one concept with two names.
/// Split them back out if a palette ever needs them to differ.
#[derive(Debug, Clone, Copy)]
pub struct Theme {
    /// Colour for added / new entries.
    pub added: Color,
    /// Colour for removed / deleted entries, and for error indicators.
    pub removed: Color,
    /// Colour for modified / changed entries, and for warning indicators.
    pub modified: Color,
    /// Subdued text for labels, hints, unchanged values.
    pub muted: Color,
    /// Primary accent (headings, prompts, active borders, informational counts).
    pub accent: Color,
    /// Default body text.
    pub text: Color,
    /// Background for the selected row in the focused pane.
    pub selection_bg: Color,
    /// Background for the selected row in an unfocused pane — neutral, so the
    /// live pane is the one carrying the tinted selection.
    pub selection_bg_dim: Color,
    /// Border and title colour for the focused panel.
    pub border_focus: Color,
    /// Draw the focused panel's border with heavy lines. The monochrome
    /// palette has no colour to mark focus with, so the line weight does it.
    pub focus_thick: bool,
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
    /// Monochrome — every colour resolves to the terminal default. Selected when
    /// `NO_COLOR` is set (see <https://no-color.org>); structure is then carried
    /// by borders, bold/underline modifiers, and the `[+] [-] [~] [>]` prefixes.
    Mono,
}

/// Dark palette — the default, tuned for dark terminal backgrounds.
///
/// Diff and severity hues stay on the ANSI base colours so they inherit the
/// user's own terminal palette. The neutrals are pinned to the 256-colour ramp
/// instead: pure white body text, a near-black `DarkGray` for muted labels, and
/// a full-saturation blue selection bar all read as chrome shouting over the
/// content.
static DARK_THEME: Theme = Theme {
    added: Color::Green,
    removed: Color::Red,
    modified: Color::Yellow,
    muted: Color::Indexed(245), // mid grey: ~5:1 on a dark ground, unlike DarkGray
    accent: Color::Cyan,
    text: Color::Indexed(252), // off-white; pure white glares against the chrome
    selection_bg: Color::Indexed(24),
    selection_bg_dim: Color::Indexed(238),
    // The accent, not a grey: a focused panel should be findable at a glance,
    // and a lighter grey border read as barely different from an unfocused one.
    border_focus: Color::Cyan,
    focus_thick: false,
    secondary_accent: Color::Magenta,
    border: Color::Indexed(238),
    zebra_bg: Color::Indexed(236),
};

/// Light palette — darker foregrounds that stay readable on light backgrounds.
/// Indexed colours are used where the ANSI base colour (e.g. yellow) would be
/// near-invisible on white.
static LIGHT_THEME: Theme = Theme {
    added: Color::Indexed(28),         // dark green
    removed: Color::Indexed(124),      // dark red
    modified: Color::Indexed(130),     // dark orange
    muted: Color::Indexed(241),        // mid grey: 245 fell under 4.5:1 on white
    accent: Color::Indexed(30),        // teal
    text: Color::Indexed(235),         // near-black; pure black is harsh on paper
    selection_bg: Color::Indexed(153), // pale blue
    selection_bg_dim: Color::Indexed(252),
    border_focus: Color::Indexed(30), // the accent teal
    focus_thick: false,
    secondary_accent: Color::Indexed(90), // purple
    border: Color::Indexed(250),
    zebra_bg: Color::Indexed(253),
};

/// Monochrome palette — every colour is the terminal default. Honours
/// `NO_COLOR`: nothing but the default fg/bg is emitted, so selection and focus
/// rely on bold/underline modifiers, the `▍` cursor bar, and the kind prefixes.
static MONO_THEME: Theme = Theme {
    added: Color::Reset,
    removed: Color::Reset,
    modified: Color::Reset,
    muted: Color::Reset,
    accent: Color::Reset,
    text: Color::Reset,
    selection_bg: Color::Reset,
    selection_bg_dim: Color::Reset,
    border_focus: Color::Reset,
    focus_thick: true,
    secondary_accent: Color::Reset,
    border: Color::Reset,
    zebra_bg: Color::Reset,
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
        ThemeMode::Mono => &MONO_THEME,
    };
    // First call wins; later calls (only possible from tests) keep the first
    // palette so cached lines built against it never go stale.
    ACTIVE_THEME.get_or_init(|| palette);
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

/// Detect the theme mode from a terminal's reply to the OSC 11 background
/// colour query, `ESC ] 11 ; rgb:RRRR/GGGG/BBBB` ended by BEL or ST.
///
/// Each channel may carry one to four hex digits; it is scaled to `[0, 1]`
/// by its own width. A relative luminance above one half means a light
/// background. `None` when the reply holds no such colour (the terminal did
/// not answer, or answered in a form this does not read).
pub fn detect_mode_from_osc11(reply: &[u8]) -> Option<ThemeMode> {
    const PREFIX: &[u8] = b"]11;rgb:";
    let start = reply.windows(PREFIX.len()).position(|window| window == PREFIX)? + PREFIX.len();
    let rest = &reply[start..];
    let end = rest.iter().position(|&byte| byte == 0x07 || byte == 0x1b).unwrap_or(rest.len());
    let body = std::str::from_utf8(&rest[..end]).ok()?;
    let mut channels = body.split('/').map(|hex| {
        if hex.is_empty() || hex.len() > 4 {
            return None;
        }
        let value = u32::from_str_radix(hex, 16).ok()?;
        let max = (1_u32 << (4 * hex.len())) - 1;
        Some(f64::from(value) / f64::from(max))
    });
    let (red, green, blue) = (channels.next()??, channels.next()??, channels.next()??);
    if channels.next().is_some() {
        return None;
    }
    let luminance = 0.2126 * red + 0.7152 * green + 0.0722 * blue;
    Some(if luminance > 0.5 { ThemeMode::Light } else { ThemeMode::Dark })
}

/// Ask the terminal for its background colour (OSC 11) and read the theme
/// mode from the reply.
///
/// The query is followed by a primary device attributes request (`ESC [ c`),
/// which virtually every terminal answers, and in order: its reply arriving
/// means the background reply either came first or is never coming, so the
/// read can stop at once instead of waiting out the timeout — and nothing is
/// left in the input for the TUI to misread as keystrokes. Terminals that
/// answer neither are given `timeout` in total.
///
/// Reads the controlling terminal directly with `poll(2)`, in raw mode so the
/// reply is neither echoed nor line-buffered. Returns `None` — the caller falls
/// back — when stdin or stderr is not a terminal, or on any I/O failure.
#[cfg(unix)]
pub fn query_background_mode(timeout: std::time::Duration) -> Option<ThemeMode> {
    use std::io::Write as _;
    use std::time::Instant;

    /// Restores cooked mode however the query ends.
    struct RawGuard;
    impl Drop for RawGuard {
        fn drop(&mut self) {
            // A failure here leaves the terminal raw; the TUI enables raw mode
            // next anyway, and there is no better place to report it.
            if let Err(error) = crossterm::terminal::disable_raw_mode() {
                debug_assert!(false, "could not leave raw mode after the background query: {error}");
            }
        }
    }

    // SAFETY: `isatty` only inspects the descriptor; 0 and 2 are always valid
    // arguments, open or not.
    let terminals = unsafe { libc::isatty(libc::STDIN_FILENO) == 1 && libc::isatty(libc::STDERR_FILENO) == 1 };
    if !terminals {
        return None;
    }
    crossterm::terminal::enable_raw_mode().ok()?;
    let _raw = RawGuard;

    let mut stderr = std::io::stderr();
    stderr.write_all(b"\x1b]11;?\x07\x1b[c").ok()?;
    stderr.flush().ok()?;

    let deadline = Instant::now() + timeout;
    let mut reply: Vec<u8> = Vec::with_capacity(64);
    let mut buffer = [0_u8; 64];
    loop {
        // The device attributes reply, `ESC [ ? … c`, ends the exchange.
        if let Some(start) = reply.windows(3).position(|window| window == b"\x1b[?")
            && reply[start..].contains(&b'c')
        {
            break;
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            break;
        }
        let mut poll_fd = libc::pollfd { fd: libc::STDIN_FILENO, events: libc::POLLIN, revents: 0 };
        let millis = libc::c_int::try_from(remaining.as_millis().max(1)).unwrap_or(libc::c_int::MAX);
        // SAFETY: `poll_fd` is one valid, initialised `pollfd`, and the count
        // passed is 1 to match.
        let ready = unsafe { libc::poll(&raw mut poll_fd, 1, millis) };
        if ready < 0 {
            if std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            return None;
        }
        if ready == 0 {
            break;
        }
        // SAFETY: `buffer` is valid for writes of its full length, which is
        // the count passed.
        let read = unsafe { libc::read(libc::STDIN_FILENO, buffer.as_mut_ptr().cast(), buffer.len()) };
        let Ok(read) = usize::try_from(read) else {
            return None;
        };
        if read == 0 {
            break;
        }
        reply.extend_from_slice(&buffer[..read]);
    }
    detect_mode_from_osc11(&reply)
}

/// OSC 11 needs `poll(2)`; elsewhere the background stays unknown.
#[cfg(not(unix))]
pub fn query_background_mode(_timeout: std::time::Duration) -> Option<ThemeMode> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn osc11_replies_are_read_by_luminance() {
        assert_eq!(detect_mode_from_osc11(b"\x1b]11;rgb:ffff/ffff/ffff\x07"), Some(ThemeMode::Light));
        assert_eq!(detect_mode_from_osc11(b"\x1b]11;rgb:0000/0000/0000\x1b\\"), Some(ThemeMode::Dark));
        assert_eq!(detect_mode_from_osc11(b"\x1b]11;rgb:28/2c/34\x07\x1b[?62;22c"), Some(ThemeMode::Dark), "two-digit channels");
        assert_eq!(detect_mode_from_osc11(b"\x1b]11;rgb:fdf6/e3e3/c7c7\x07"), Some(ThemeMode::Light), "solarised light");
    }

    #[test]
    fn osc11_without_a_colour_is_unknown() {
        assert_eq!(detect_mode_from_osc11(b""), None, "no reply");
        assert_eq!(detect_mode_from_osc11(b"\x1b[?62;22c"), None, "only the device attributes came back");
        assert_eq!(detect_mode_from_osc11(b"\x1b]11;rgb:zz/00/00\x07"), None, "not hex");
        assert_eq!(detect_mode_from_osc11(b"\x1b]11;rgb:ffff/ffff\x07"), None, "a channel short");
    }

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
