//! Custom widgets for the TUI application.

use ratatui::style::{Color, Style};

use crate::diff_model::DiffKind;

pub mod detail;
pub mod help;
pub mod search;
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
