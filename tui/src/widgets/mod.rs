//! Custom widgets for the TUI application.

use ratatui::layout::{Constraint, Flex, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};

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
        Style::default().fg(Color::Reset)
    }
}

pub mod detail;
pub mod help;
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
