//! Bottom status bar widget.
//!
//! Displays total change count, per-section diff statistics, active filter, and key hints.
//! Layout is responsive: the left segment flows to fit its content and the
//! key hints stay right-aligned, so long filter/tolerance labels no longer
//! overflow a fixed-width column silently.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::diff_model::DiffCounts;
use crate::theme::theme;

/// Optional detail scroll position for the status bar.
pub struct DetailPosition {
    pub scroll: u16,
    pub content_lines: usize,
}

/// Optional yank flash state for the status bar.
pub struct YankFlash<'a> {
    pub message: &'a str,
}

/// Inspect-mode left segment: the single filename and current section counts.
///
/// When present, it replaces the diff-oriented change/filter segment entirely so
/// no "N changes" / "+N -N ~N" / "filter" text appears in inspect mode.
pub struct InspectInfo<'a> {
    pub file: &'a str,
    pub section_label: &'a str,
    pub entry_count: usize,
}

/// Parameters for rendering the status bar.
pub struct StatusBarParams<'a> {
    pub total_changes: usize,
    pub section_counts: &'a DiffCounts,
    pub filter_label: &'a str,
    pub filter_count: usize,
    pub detail_position: Option<&'a DetailPosition>,
    pub yank_flash: Option<&'a YankFlash<'a>>,
    pub ignore_order: bool,
    /// Sort mode indicator (e.g. "sort:|Δ|"). `None` for the default name sort.
    pub sort_label: Option<&'a str>,
    /// Active non-zero tolerances (e.g. "abs:1e-6 rel:1e-9"). `None` when both are zero.
    pub tolerance_label: Option<&'a str>,
    /// Watch mode: `None` when not watching, `Some(reloading)` when active.
    pub watch_reloading: Option<bool>,
    /// Inspect-mode left segment. `None` in diff mode (the default segment shows).
    pub inspect: Option<InspectInfo<'a>>,
    /// Context-sensitive key hints shown on the right (chosen by the caller
    /// from focus/section/mode so the most relevant actions are advertised).
    pub hints: &'a str,
}

/// Muted separator between status bar segments.
const SEPARATOR: &str = "  \u{2502}  ";

/// Draw the status bar across the given area.
pub fn draw_status_bar(frame: &mut Frame, area: Rect, params: &StatusBarParams<'_>) {
    // A zero-sized area is an environmental condition (shrunken terminal), not a
    // programming error: drawing into it is a no-op.
    if area.width == 0 || area.height == 0 {
        return;
    }

    let t = theme();
    let separator = || Span::styled(SEPARATOR, Style::default().fg(t.border));

    // Left: flowing segments. Inspect mode shows the filename and section count;
    // diff mode shows total/per-kind change counts and the active filter.
    let mut spans = if let Some(inspect) = &params.inspect {
        let entries = if inspect.entry_count == 1 { "entry" } else { "entries" };
        vec![
            Span::styled(format!(" {}", inspect.file), Style::default().fg(t.accent).add_modifier(Modifier::BOLD)),
            separator(),
            Span::styled(format!("{} {} {}", inspect.entry_count, inspect.section_label, entries), Style::default().fg(t.text)),
        ]
    } else {
        vec![
            Span::styled(format!(" {} changes", params.total_changes), Style::default().fg(t.added).add_modifier(Modifier::BOLD)),
            separator(),
            Span::styled(format!("+{}", params.section_counts.added), Style::default().fg(t.added)),
            Span::raw(" "),
            Span::styled(format!("-{}", params.section_counts.removed), Style::default().fg(t.removed)),
            Span::raw(" "),
            Span::styled(format!("~{}", params.section_counts.modified), Style::default().fg(t.modified)),
            Span::raw(" "),
            Span::styled(format!(">{}", params.section_counts.renamed), Style::default().fg(t.info)),
            separator(),
            Span::styled("filter:", Style::default().fg(t.muted)),
            Span::styled(format!("{} ({})", params.filter_label, params.filter_count), Style::default().fg(t.modified)),
        ]
    };
    if params.ignore_order {
        spans.push(Span::styled(" [ignoring order]", Style::default().fg(t.warning)));
    }
    if let Some(sort_label) = params.sort_label {
        spans.push(separator());
        spans.push(Span::styled(sort_label.to_owned(), Style::default().fg(t.info)));
    }
    if let Some(tolerance_label) = params.tolerance_label {
        spans.push(separator());
        spans.push(Span::styled(tolerance_label.to_owned(), Style::default().fg(t.accent)));
    }
    if let Some(reloading) = params.watch_reloading {
        spans.push(separator());
        if reloading {
            spans.push(Span::styled("\u{25cf} reloading\u{2026}", Style::default().fg(t.warning).add_modifier(Modifier::BOLD)));
        } else {
            spans.push(Span::styled("\u{25cf} watch", Style::default().fg(t.secondary_accent)));
        }
    }
    if let Some(position) = params.detail_position
        && position.content_lines > 0
    {
        let top_line = (position.scroll as usize).min(position.content_lines) + 1;
        spans.push(separator());
        spans.push(Span::styled(format!("L{top_line}/{}", position.content_lines), Style::default().fg(t.accent)));
    }

    // Right: yank flash or key hints, right-aligned in a fixed-width chunk so
    // the left segment can flow (and clip) independently.
    // A leading space keeps a visible gap between a clipped left segment and
    // the right-aligned hints.
    let (right_line, right_width) = params.yank_flash.map_or_else(
        || (Line::from(vec![Span::raw(" "), Span::styled(params.hints, Style::default().fg(t.muted))]), params.hints.len()),
        |flash| {
            (
                Line::from(vec![Span::raw(" "), Span::styled(flash.message, Style::default().fg(t.added).add_modifier(Modifier::BOLD))]),
                flash.message.len(),
            )
        },
    );

    #[allow(clippy::cast_possible_truncation)] // hint strings are far below u16::MAX
    let right_len = (right_width as u16).saturating_add(2).min(area.width);
    let chunks = Layout::horizontal([Constraint::Min(0), Constraint::Length(right_len)]).split(area);

    frame.render_widget(Paragraph::new(Line::from(spans)), chunks[0]);
    frame.render_widget(Paragraph::new(right_line), chunks[1]);
}
