//! Bottom status bar widget.
//!
//! Displays total change count, per-section diff statistics, active filter, and key hints.

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

/// Parameters for rendering the status bar.
pub struct StatusBarParams<'a> {
    pub total_changes: usize,
    pub section_counts: &'a DiffCounts,
    pub filter_label: &'a str,
    pub filter_count: usize,
    pub detail_position: Option<&'a DetailPosition>,
    pub yank_flash: Option<&'a YankFlash<'a>>,
}

/// Draw the status bar across the given area.
pub fn draw_status_bar(frame: &mut Frame, area: Rect, params: &StatusBarParams<'_>) {
    debug_assert!(area.width > 0 && area.height > 0, "status bar area must be non-zero");

    let t = theme();

    let chunks =
        Layout::horizontal([Constraint::Length(20), Constraint::Length(20), Constraint::Length(30), Constraint::Min(0)]).split(area);

    // Left: total number of changes across all sections.
    let changes_widget = Paragraph::new(Line::from(vec![Span::styled(
        format!(" {} changes", params.total_changes),
        Style::default().fg(t.added).add_modifier(Modifier::BOLD),
    )]));
    frame.render_widget(changes_widget, chunks[0]);

    // Section diff counts: +N -N ~N
    let counts_widget = Paragraph::new(Line::from(vec![
        Span::styled(format!("+{}", params.section_counts.added), Style::default().fg(t.added)),
        Span::raw(" "),
        Span::styled(format!("-{}", params.section_counts.removed), Style::default().fg(t.removed)),
        Span::raw(" "),
        Span::styled(format!("~{}", params.section_counts.modified), Style::default().fg(t.modified)),
    ]));
    frame.render_widget(counts_widget, chunks[1]);

    // Centre: active filter name + optional scroll position.
    let mut centre_spans = vec![
        Span::styled("Filter: ", Style::default().fg(t.muted)),
        Span::styled(format!("{} ({})", params.filter_label, params.filter_count), Style::default().fg(t.modified)),
    ];
    if let Some(position) = params.detail_position
        && position.content_lines > 0
    {
        let top_line = (position.scroll as usize).min(position.content_lines) + 1;
        centre_spans.push(Span::styled(format!("  L{top_line}/{}", position.content_lines), Style::default().fg(t.accent)));
    }
    let filter_widget = Paragraph::new(Line::from(centre_spans));
    frame.render_widget(filter_widget, chunks[2]);

    // Right: yank flash or key hints.
    let hints_widget = params.yank_flash.map_or_else(
        || {
            Paragraph::new(Line::from(vec![Span::styled(
                "Tab:panel  Enter:detail  j/k:nav  y:yank  /:search  ?:help  q:quit",
                Style::default().fg(t.muted),
            )]))
        },
        |flash| Paragraph::new(Line::from(vec![Span::styled(flash.message, Style::default().fg(t.added).add_modifier(Modifier::BOLD))])),
    );
    frame.render_widget(hints_widget, chunks[3]);
}
