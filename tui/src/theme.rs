//! Centralised colour theme for the TUI.
//!
//! All widget code should use `theme()` to obtain semantic colours rather than
//! hard-coding `Color::*` constants.  This ensures the palette stays consistent
//! and can adapt to light or dark terminal backgrounds.
//!
//! The active palette is installed with [`set_theme`] and read through
//! [`theme()`], which returns the palette by value. It can be swapped while the
//! app runs; anything that caches styled text must be rebuilt afterwards (see
//! `App::restyle`).

use std::sync::RwLock;

use ratatui::style::Color;

/// Semantic colour palette for the TUI.
///
/// Severity reuses the diff colours: errors are drawn in `removed`, warnings in
/// `modified`. Roles that the built-in colour palettes happen to colour alike
/// (`key`, `heading`, `info`, `renamed`, `focus`) have their own field so a
/// theme can pull them apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Theme {
    /// Colour for added / new entries.
    pub added: Color,
    /// Colour for removed / deleted entries, and for error indicators.
    pub removed: Color,
    /// Colour for modified / changed entries, and for warning indicators.
    pub modified: Color,
    /// Colour for renamed entries.
    pub renamed: Color,
    /// Subdued text for labels, hints, unchanged values.
    pub muted: Color,
    /// Fainter than `muted`: separators, placeholders, secondary hints.
    pub faint: Color,
    /// Primary accent (prompts, active borders, numeric highlights).
    pub accent: Color,
    /// Key names in hint rows.
    pub key: Color,
    /// Section headings and overlay titles.
    pub heading: Color,
    /// Informational counts and neutral status feedback.
    pub info: Color,
    /// Default body text.
    pub text: Color,
    /// Background for the selected row in the focused pane.
    pub selection_bg: Color,
    /// Foreground for the selected row: the row's own colours are overridden so
    /// the text stays legible on the bar.
    pub selection_fg: Color,
    /// Background for the selected row in an unfocused pane — neutral, so the
    /// live pane is the one carrying the tinted selection.
    pub selection_bg_dim: Color,
    /// Border and title colour for the focused panel.
    pub border_focus: Color,
    /// Draw the focused panel's border with heavy lines. Themes with no hue to
    /// mark focus with use line weight instead.
    pub focus_thick: bool,
    /// Draw panels with square corners rather than rounded ones.
    pub square: bool,
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
pub const DARK_THEME: Theme = Theme {
    added: Color::Green,
    removed: Color::Red,
    modified: Color::Yellow,
    renamed: Color::Cyan,
    muted: Color::Indexed(245), // mid grey: ~5:1 on a dark ground, unlike DarkGray
    faint: Color::Indexed(238),
    accent: Color::Cyan,
    key: Color::Cyan,
    heading: Color::Cyan,
    info: Color::Cyan,
    text: Color::Indexed(252), // off-white; pure white glares against the chrome
    selection_bg: Color::Indexed(24),
    selection_fg: Color::Indexed(252),
    selection_bg_dim: Color::Indexed(238),
    // The accent, not a grey: a focused panel should be findable at a glance,
    // and a lighter grey border read as barely different from an unfocused one.
    border_focus: Color::Cyan,
    focus_thick: false,
    square: false,
    secondary_accent: Color::Magenta,
    border: Color::Indexed(238),
    zebra_bg: Color::Indexed(236),
};

/// Light palette — darker foregrounds that stay readable on light backgrounds.
/// Indexed colours are used where the ANSI base colour (e.g. yellow) would be
/// near-invisible on white.
pub const LIGHT_THEME: Theme = Theme {
    added: Color::Indexed(28),     // dark green
    removed: Color::Indexed(124),  // dark red
    modified: Color::Indexed(130), // dark orange
    renamed: Color::Indexed(30),
    muted: Color::Indexed(241), // mid grey: 245 fell under 4.5:1 on white
    faint: Color::Indexed(250),
    accent: Color::Indexed(30), // teal
    key: Color::Indexed(30),
    heading: Color::Indexed(30),
    info: Color::Indexed(30),
    text: Color::Indexed(235),         // near-black; pure black is harsh on paper
    selection_bg: Color::Indexed(153), // pale blue
    selection_fg: Color::Indexed(235),
    selection_bg_dim: Color::Indexed(252),
    border_focus: Color::Indexed(30), // the accent teal
    focus_thick: false,
    square: false,
    secondary_accent: Color::Indexed(90), // purple
    border: Color::Indexed(250),
    zebra_bg: Color::Indexed(253),
};

/// Monochrome palette — every colour is the terminal default. Honours
/// `NO_COLOR`: nothing but the default fg/bg is emitted, so selection and focus
/// rely on bold/underline modifiers, the `▍` cursor bar, and the kind prefixes.
pub const MONO_THEME: Theme = Theme {
    added: Color::Reset,
    removed: Color::Reset,
    modified: Color::Reset,
    renamed: Color::Reset,
    muted: Color::Reset,
    faint: Color::Reset,
    accent: Color::Reset,
    key: Color::Reset,
    heading: Color::Reset,
    info: Color::Reset,
    text: Color::Reset,
    selection_bg: Color::Reset,
    selection_fg: Color::Reset,
    selection_bg_dim: Color::Reset,
    border_focus: Color::Reset,
    focus_thick: true,
    square: false,
    secondary_accent: Color::Reset,
    border: Color::Reset,
    zebra_bg: Color::Reset,
};

#[expect(clippy::cast_possible_truncation)] // each shift leaves one byte
const fn rgb(hex: u32) -> Color {
    Color::Rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}

/// Flat, ruled palette for light backgrounds: greyscale chrome, with colour
/// kept for the four diff meanings. Focus is carried by weight, not hue.
pub const FLAT_LIGHT_THEME: Theme = Theme {
    added: rgb(0x0d_7a_3c),
    removed: rgb(0xc2_3c_06),
    modified: rgb(0x8a_5a_12),
    renamed: rgb(0x3a_66_8f),
    muted: rgb(0x6b_6b_6b),
    faint: rgb(0x76_76_76),
    accent: rgb(0x2b_2b_2b),
    key: rgb(0x2b_2b_2b),
    heading: rgb(0x2b_2b_2b),
    info: rgb(0x6b_6b_6b),
    text: rgb(0x2b_2b_2b),
    selection_bg: rgb(0xec_ec_e7),
    selection_fg: rgb(0x2b_2b_2b),
    selection_bg_dim: rgb(0xf6_f6_f6),
    border_focus: rgb(0x2b_2b_2b),
    focus_thick: true,
    square: true,
    secondary_accent: rgb(0x3a_66_8f),
    border: rgb(0xc9_c9_c9),
    zebra_bg: rgb(0xf6_f6_f6),
};

/// Flat, ruled palette for dark backgrounds.
pub const FLAT_DARK_THEME: Theme = Theme {
    added: rgb(0x4e_c9_7a),
    removed: rgb(0xf2_70_4f),
    modified: rgb(0xe3_b3_41),
    renamed: rgb(0x7f_b2_e5),
    muted: rgb(0x8c_8c_88),
    faint: rgb(0x88_88_84),
    accent: rgb(0xec_ec_e7),
    key: rgb(0xec_ec_e7),
    heading: rgb(0xec_ec_e7),
    info: rgb(0x8c_8c_88),
    text: rgb(0xec_ec_e7),
    selection_bg: rgb(0x2e_2e_2b),
    selection_fg: rgb(0xec_ec_e7),
    selection_bg_dim: rgb(0x26_26_24),
    border_focus: rgb(0xec_ec_e7),
    focus_thick: true,
    square: true,
    secondary_accent: rgb(0x7f_b2_e5),
    border: rgb(0x3a_3a_38),
    zebra_bg: rgb(0x26_26_24),
};

/// High-contrast palette: pure black and white neutrals and saturated hues,
/// with heavy focus borders.
pub const HIGH_CONTRAST_THEME: Theme = Theme {
    added: Color::LightGreen,
    removed: Color::LightRed,
    modified: Color::LightYellow,
    renamed: Color::LightCyan,
    muted: Color::White,
    faint: Color::Gray,
    accent: Color::LightCyan,
    key: Color::LightYellow,
    heading: Color::White,
    info: Color::LightCyan,
    text: Color::White,
    selection_bg: Color::White,
    selection_fg: Color::Black,
    selection_bg_dim: Color::Gray,
    border_focus: Color::White,
    focus_thick: true,
    square: true,
    secondary_accent: Color::LightMagenta,
    border: Color::Gray,
    zebra_bg: Color::Reset,
};

/// Colour-blind-safe palette: blue and orange carry added / removed instead of
/// green and red, and modified is a distinct purple.
pub const COLOUR_BLIND_THEME: Theme = Theme {
    added: Color::Indexed(33),     // blue
    removed: Color::Indexed(208),  // orange
    modified: Color::Indexed(141), // light purple
    renamed: Color::Indexed(250),
    ..DARK_THEME
};

/// Built-in palettes by name, as accepted by `--theme`.
pub const BUILTIN_THEMES: [(&str, Theme); 8] = [
    ("dark", DARK_THEME),
    ("light", LIGHT_THEME),
    ("mono", MONO_THEME),
    ("flat-dark", FLAT_DARK_THEME),
    ("flat-light", FLAT_LIGHT_THEME),
    ("high-contrast", HIGH_CONTRAST_THEME),
    ("colour-blind", COLOUR_BLIND_THEME),
    ("color-blind", COLOUR_BLIND_THEME),
];

/// Look up a built-in palette by name.
pub fn builtin(name: &str) -> Option<Theme> {
    BUILTIN_THEMES.iter().find(|(candidate, _)| *candidate == name).map(|(_, theme)| *theme)
}

impl Theme {
    /// The built-in palette for a detected mode.
    pub const fn for_mode(mode: ThemeMode) -> Self {
        match mode {
            ThemeMode::Dark => DARK_THEME,
            ThemeMode::Light => LIGHT_THEME,
            ThemeMode::Mono => MONO_THEME,
        }
    }
}

/// The palette in use. Falls back to dark when never set (e.g. in unit tests
/// that render widgets without going through `main`).
static ACTIVE_THEME: RwLock<Theme> = RwLock::new(DARK_THEME);

/// Install `theme` as the active palette. Callers that cache styled text must
/// rebuild it afterwards (see `App::restyle`).
pub fn set_theme(theme: Theme) {
    // A poisoned lock only means another thread panicked mid-read; the palette is plain data.
    *ACTIVE_THEME.write().unwrap_or_else(std::sync::PoisonError::into_inner) = theme;
}

/// Return the active theme.
pub fn theme() -> Theme {
    *ACTIVE_THEME.read().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Name of the built-in palette equal to `theme`, if it is one.
pub fn builtin_name(theme: &Theme) -> Option<&'static str> {
    BUILTIN_THEMES.iter().find(|(_, candidate)| candidate == theme).map(|(name, _)| *name)
}

/// How many colours the terminal can show.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Depth {
    TrueColor,
    Ansi256,
    Ansi16,
}

/// Colour depth from `COLORTERM` and `TERM`: `truecolor`/`24bit` means 24-bit,
/// a `*256color` terminal means the 256 ramp, anything else the 16 ANSI colours.
pub fn detect_depth(colorterm: Option<&str>, term: Option<&str>) -> Depth {
    if colorterm.is_some_and(|value| matches!(value, "truecolor" | "24bit")) {
        Depth::TrueColor
    } else if term.is_some_and(|value| value.contains("256color")) {
        Depth::Ansi256
    } else {
        // 16 is the only depth every terminal is guaranteed to draw.
        Depth::Ansi16
    }
}

/// xterm RGB for colours that have a fixed value: `Rgb` and the 16..=255 part of the ramp.
/// `None` for named colours and `Reset`, which each terminal defines itself.
pub fn rgb_of(color: Color) -> Option<[u8; 3]> {
    match color {
        Color::Rgb(r, g, b) => Some([r, g, b]),
        Color::Indexed(n @ 16..=231) => {
            const LEVELS: [u8; 6] = [0, 95, 135, 175, 215, 255];
            let n = usize::from(n - 16);
            Some([LEVELS[n / 36], LEVELS[n / 6 % 6], LEVELS[n % 6]])
        }
        Color::Indexed(n @ 232..=255) => Some([8 + 10 * (n - 232); 3]),
        _ => None,
    }
}

const ANSI16: [(Color, [u8; 3]); 16] = [
    (Color::Black, [0, 0, 0]),
    (Color::Red, [205, 0, 0]),
    (Color::Green, [0, 205, 0]),
    (Color::Yellow, [205, 205, 0]),
    (Color::Blue, [0, 0, 238]),
    (Color::Magenta, [205, 0, 205]),
    (Color::Cyan, [0, 205, 205]),
    (Color::Gray, [229, 229, 229]),
    (Color::DarkGray, [127, 127, 127]),
    (Color::LightRed, [255, 0, 0]),
    (Color::LightGreen, [0, 255, 0]),
    (Color::LightYellow, [255, 255, 0]),
    (Color::LightBlue, [92, 92, 255]),
    (Color::LightMagenta, [255, 0, 255]),
    (Color::LightCyan, [0, 255, 255]),
    (Color::White, [255, 255, 255]),
];

fn distance(a: [u8; 3], b: [u8; 3]) -> i32 {
    a.iter().zip(b).map(|(&x, y)| (i32::from(x) - i32::from(y)).pow(2)).sum()
}

/// The nearest colour a terminal of `depth` can show. Named colours and `Reset` pass through.
pub fn degrade(color: Color, depth: Depth) -> Color {
    let Some(target) = rgb_of(color) else {
        return color;
    };
    match depth {
        Depth::TrueColor => color,
        Depth::Ansi256 => match color {
            Color::Rgb(..) => (16..=255_u8)
                .min_by_key(|&n| distance(target, rgb_of(Color::Indexed(n)).expect("16..=255 have fixed values")))
                .map_or(color, Color::Indexed),
            _ => color,
        },
        Depth::Ansi16 => ANSI16.iter().min_by_key(|(_, rgb)| distance(target, *rgb)).map_or(color, |(named, _)| *named),
    }
}

impl Theme {
    /// This palette with every colour reduced to what `depth` can show.
    pub fn degraded(mut self, depth: Depth) -> Self {
        for name in COLOUR_TOKENS {
            if let Some(slot) = self.colour_mut(name) {
                *slot = degrade(*slot, depth);
            }
        }
        self
    }

    fn colour_mut(&mut self, name: &str) -> Option<&mut Color> {
        Some(match name {
            "added" => &mut self.added,
            "removed" => &mut self.removed,
            "modified" => &mut self.modified,
            "renamed" => &mut self.renamed,
            "muted" => &mut self.muted,
            "faint" => &mut self.faint,
            "accent" => &mut self.accent,
            "key" => &mut self.key,
            "heading" => &mut self.heading,
            "info" => &mut self.info,
            "text" => &mut self.text,
            "selection_bg" => &mut self.selection_bg,
            "selection_fg" => &mut self.selection_fg,
            "selection_bg_dim" => &mut self.selection_bg_dim,
            "focus" => &mut self.border_focus,
            "secondary_accent" => &mut self.secondary_accent,
            "border" => &mut self.border,
            "zebra_bg" => &mut self.zebra_bg,
            _ => return None,
        })
    }
}

/// Every colour token a theme file may set.
const COLOUR_TOKENS: [&str; 18] = [
    "added",
    "removed",
    "modified",
    "renamed",
    "muted",
    "faint",
    "accent",
    "key",
    "heading",
    "info",
    "text",
    "selection_bg",
    "selection_fg",
    "selection_bg_dim",
    "focus",
    "secondary_accent",
    "border",
    "zebra_bg",
];

/// Parse one colour: `#rrggbb`, `ansi:<name>`, `256:<n>` or `default`.
pub fn parse_colour(value: &str) -> Result<Color, String> {
    if value == "default" {
        return Ok(Color::Reset);
    }
    if let Some(hex) = value.strip_prefix('#') {
        if hex.len() == 6
            && let Ok(number) = u32::from_str_radix(hex, 16)
        {
            return Ok(rgb(number));
        }
        return Err(format!("`{value}` is not a #rrggbb colour"));
    }
    if let Some(index) = value.strip_prefix("256:") {
        return index.parse::<u8>().map(Color::Indexed).map_err(|_| format!("`{value}`: the 256-colour index must be 0-255"));
    }
    if let Some(name) = value.strip_prefix("ansi:") {
        return name.parse::<Color>().map_err(|_| format!("`{value}` is not an ANSI colour name"));
    }
    Err(format!("`{value}` is not a colour: use #rrggbb, ansi:<name>, 256:<n> or default"))
}

/// Parse a theme file onto `base`.
///
/// One `token = "value"` per line, `#` comments, blank lines ignored. `base`
/// names a built-in palette to start from (it must come before any colour so a
/// later override is not discarded); `focus_thick` and `square` take
/// `true`/`false`. Unknown tokens are errors, so a typo does not silently
/// leave a colour unchanged.
pub fn parse_theme_file(text: &str, default_base: Theme) -> Result<Theme, String> {
    let mut theme = default_base;
    for (number, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let at = |message: String| format!("line {}: {message}", number + 1);
        let (key, value) = line.split_once('=').ok_or_else(|| at("expected `token = \"value\"`".to_owned()))?;
        let (key, value) = (key.trim(), value.trim().trim_matches('"'));
        match key {
            "base" => theme = builtin(value).ok_or_else(|| at(format!("unknown base theme `{value}`")))?,
            "focus_thick" | "square" => {
                let flag: bool = value.parse().map_err(|_| at(format!("`{key}` takes true or false")))?;
                if key == "square" {
                    theme.square = flag;
                } else {
                    theme.focus_thick = flag;
                }
            }
            _ => {
                let colour = parse_colour(value).map_err(at)?;
                *theme.colour_mut(key).ok_or_else(|| at(format!("unknown token `{key}`")))? = colour;
            }
        }
    }
    Ok(theme)
}

/// Where the user's theme file lives: `$XDG_CONFIG_HOME/lp_diff/theme.toml`,
/// else `~/.config/lp_diff/theme.toml`, else `%APPDATA%\lp_diff\theme.toml`.
pub fn config_path() -> Option<std::path::PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|value| !value.is_empty())
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("HOME").filter(|value| !value.is_empty()).map(|home| std::path::Path::new(&home).join(".config")))
        .or_else(|| std::env::var_os("APPDATA").filter(|value| !value.is_empty()).map(std::path::PathBuf::from))?;
    Some(base.join("lp_diff").join("theme.toml"))
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

    fn linear(rgb: [u8; 3]) -> [f64; 3] {
        rgb.map(|c| {
            let c = f64::from(c) / 255.0;
            if c <= 0.03928 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
        })
    }

    fn contrast(a: Color, b: Color) -> Option<f64> {
        let luminance = |[r, g, b]: [f64; 3]| 0.2126 * r + 0.7152 * g + 0.0722 * b;
        let (a, b) = (luminance(linear(rgb_of(a)?)), luminance(linear(rgb_of(b)?)));
        Some((a.max(b) + 0.05) / (a.min(b) + 0.05))
    }

    /// Guard for readability: every built-in with fixed colours keeps its selection
    /// bars and zebra stripes readable under the selection text colour.
    #[test]
    fn selected_rows_meet_wcag_aa() {
        for (name, theme) in BUILTIN_THEMES {
            for (bar, bg) in
                [("selection_bg", theme.selection_bg), ("selection_bg_dim", theme.selection_bg_dim), ("zebra_bg", theme.zebra_bg)]
            {
                // Reset and named colours have no fixed value to measure.
                let Some(ratio) = contrast(theme.selection_fg, bg) else { continue };
                assert!(ratio >= 4.5, "{name}: selection_fg on {bar} is {ratio:.2}:1");
            }
        }
    }

    /// The flat palettes assume a plain page behind them; their hues must clear AA on it.
    #[test]
    fn flat_hues_are_readable_on_their_page() {
        for (name, theme, page) in [("flat-light", FLAT_LIGHT_THEME, rgb(0xff_ff_ff)), ("flat-dark", FLAT_DARK_THEME, rgb(0x1c_1c_1c))] {
            for (token, colour) in [
                ("text", theme.text),
                ("muted", theme.muted),
                ("faint", theme.faint),
                ("added", theme.added),
                ("removed", theme.removed),
                ("modified", theme.modified),
                ("renamed", theme.renamed),
            ] {
                let ratio = contrast(colour, page).expect("flat palettes use RGB");
                assert!(ratio >= 4.5, "{name}: {token} on the page is {ratio:.2}:1");
            }
        }
    }

    #[test]
    fn theme_files_override_a_base_and_reject_typos() {
        let text = "# comment\nbase = \"flat-dark\"\nadded = \"#112233\"\nmuted = \"256:245\"\nborder = \"ansi:red\"\nsquare = false\nfocus = \"default\"\n";
        let theme = parse_theme_file(text, DARK_THEME).expect("valid file");
        assert_eq!(theme.added, Color::Rgb(0x11, 0x22, 0x33));
        assert_eq!(theme.muted, Color::Indexed(245));
        assert_eq!(theme.border, Color::Red);
        assert_eq!(theme.border_focus, Color::Reset);
        assert!(!theme.square, "an override beats the base");
        assert_eq!(theme.text, FLAT_DARK_THEME.text, "unset tokens come from the base");
        assert!(parse_theme_file("addd = \"#112233\"", DARK_THEME).unwrap_err().contains("unknown token"));
        assert!(parse_theme_file("added = \"#12\"", DARK_THEME).unwrap_err().contains("line 1"));
        assert!(parse_theme_file("base = \"nope\"", DARK_THEME).is_err());
    }

    #[test]
    fn colours_degrade_to_the_terminal_depth() {
        assert_eq!(degrade(Color::Rgb(255, 0, 0), Depth::TrueColor), Color::Rgb(255, 0, 0));
        assert_eq!(degrade(Color::Rgb(255, 0, 0), Depth::Ansi256), Color::Indexed(196));
        assert_eq!(degrade(Color::Rgb(250, 10, 10), Depth::Ansi16), Color::LightRed);
        assert_eq!(degrade(Color::Cyan, Depth::Ansi16), Color::Cyan, "named colours pass through");
        assert_eq!(degrade(Color::Indexed(245), Depth::Ansi16), Color::DarkGray);
        assert_eq!(FLAT_DARK_THEME.degraded(Depth::Ansi16).text, Color::Gray);
        assert_eq!(degrade(Color::Rgb(0, 250, 0), Depth::Ansi16), Color::LightGreen);
    }

    #[test]
    fn depth_comes_from_the_environment() {
        assert_eq!(detect_depth(Some("truecolor"), Some("xterm")), Depth::TrueColor);
        assert_eq!(detect_depth(Some("24bit"), None), Depth::TrueColor);
        assert_eq!(detect_depth(None, Some("xterm-256color")), Depth::Ansi256);
        assert_eq!(detect_depth(None, Some("linux")), Depth::Ansi16);
    }

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
