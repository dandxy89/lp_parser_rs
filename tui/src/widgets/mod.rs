//! Custom widgets for the TUI application.

use ratatui::layout::{Constraint, Flex, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};

use crate::diff_model::DiffKind;
use crate::state::Focus;

/// Subdued text for labels, hints, and unchanged values.
pub const MUTED: Style = Style::new().fg(Color::DarkGray);

/// Default body text.
pub const TEXT: Style = Style::new().fg(Color::White);

/// Bold body text for emphasis.
pub const BOLD_TEXT: Style = TEXT.add_modifier(Modifier::BOLD);

/// Arrow separator used between old → new values.
pub const ARROW: &str = "  \u{2192}  ";

/// Return the border [`Style`] for a panel, highlighted when `current == target`.
pub fn focus_border_style(current: Focus, target: Focus) -> Style {
    if current == target { Style::default().fg(Color::Green).add_modifier(Modifier::BOLD) } else { Style::default().fg(Color::Reset) }
}

pub mod detail;
pub mod help;
pub mod search_popup;
pub mod sidebar;
pub mod solve;
pub mod status_bar;
pub mod summary;

/// Map a [`DiffKind`] to its display colour.
pub const fn kind_colour(kind: DiffKind) -> Color {
    match kind {
        DiffKind::Added => Color::Green,
        DiffKind::Removed => Color::Red,
        DiffKind::Modified => Color::Yellow,
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
