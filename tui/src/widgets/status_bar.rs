//! Bottom status bar widget.
//!
//! Displays total change count, per-section diff statistics, active filter, and key hints.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::diff_model::DiffCounts;

/// Optional detail scroll position for the status bar.
pub struct DetailPosition {
    pub scroll: u16,
    pub content_lines: usize,
}

/// Optional yank flash state for the status bar.
pub struct YankFlash<'a> {
    pub message: &'a str,
}

/// Draw the status bar across the given area.
#[allow(clippy::too_many_arguments)]
pub fn draw_status_bar(
    frame: &mut Frame,
    area: Rect,
    total_changes: usize,
    section_counts: &DiffCounts,
    filter_label: &str,
    filter_count: usize,
    detail_pos: Option<&DetailPosition>,
    yank_flash: Option<&YankFlash<'_>>,
) {
    let chunks =
        Layout::horizontal([Constraint::Length(20), Constraint::Length(20), Constraint::Length(30), Constraint::Min(0)]).split(area);

    // Left: total number of changes across all sections.
    let changes_widget = Paragraph::new(Line::from(vec![Span::styled(
        format!(" {total_changes} changes"),
        Style::default().fg(Color::Green).add_modifier(Modifier::BOLD),
    )]));
    frame.render_widget(changes_widget, chunks[0]);

    // Section diff counts: +N -N ~N
    let counts_widget = Paragraph::new(Line::from(vec![
        Span::styled(format!("+{}", section_counts.added), Style::default().fg(Color::Green)),
        Span::raw(" "),
        Span::styled(format!("-{}", section_counts.removed), Style::default().fg(Color::Red)),
        Span::raw(" "),
        Span::styled(format!("~{}", section_counts.modified), Style::default().fg(Color::Yellow)),
    ]));
    frame.render_widget(counts_widget, chunks[1]);

    // Centre: active filter name + optional scroll position.
    let mut centre_spans = vec![
        Span::styled("Filter: ", Style::default().fg(Color::DarkGray)),
        Span::styled(format!("{filter_label} ({filter_count})"), Style::default().fg(Color::Yellow)),
    ];
    if let Some(pos) = detail_pos
        && pos.content_lines > 0
    {
        let top_line = (pos.scroll as usize).min(pos.content_lines) + 1;
        centre_spans.push(Span::styled(format!("  L{top_line}/{}", pos.content_lines), Style::default().fg(Color::Cyan)));
    }
    let filter_widget = Paragraph::new(Line::from(centre_spans));
    frame.render_widget(filter_widget, chunks[2]);

    // Right: yank flash or key hints.
    let hints_widget = if let Some(flash) = yank_flash {
        Paragraph::new(Line::from(vec![Span::styled(flash.message, Style::default().fg(Color::Green).add_modifier(Modifier::BOLD))]))
    } else {
        Paragraph::new(Line::from(vec![Span::styled(
            "Tab:panel  Enter:detail  j/k:nav  y:yank  /:search  ?:help  q:quit",
            Style::default().fg(Color::DarkGray),
        )]))
    };
    frame.render_widget(hints_widget, chunks[3]);
}
