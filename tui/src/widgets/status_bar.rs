//! Bottom status bar widget.
//!
//! Displays total change count, active filter, and key hints. When the search
//! bar is active, the hints area is replaced with the live search input.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

/// Bundled search state passed to the status bar renderer.
pub struct SearchState<'a> {
    pub active: bool,
    pub query: &'a str,
    pub mode_label: &'a str,
    pub has_regex_error: bool,
}

/// Draw the status bar across the given area.
pub fn draw_status_bar(
    frame: &mut Frame,
    area: Rect,
    total_changes: usize,
    filter_label: &str,
    filter_count: usize,
    search: &SearchState<'_>,
) {
    let chunks = Layout::horizontal([Constraint::Length(20), Constraint::Length(25), Constraint::Min(0)]).split(area);

    // Left: total number of changes across all sections.
    let changes_widget = Paragraph::new(Line::from(vec![Span::styled(
        format!(" {total_changes} changes"),
        Style::default().fg(Color::Green).add_modifier(Modifier::BOLD),
    )]));
    frame.render_widget(changes_widget, chunks[0]);

    // Centre: active filter name and count of matching entries.
    let filter_widget = Paragraph::new(Line::from(vec![
        Span::styled("Filter: ", Style::default().fg(Color::DarkGray)),
        Span::styled(format!("{filter_label} ({filter_count})"), Style::default().fg(Color::Yellow)),
    ]));
    frame.render_widget(filter_widget, chunks[1]);

    // Right: key hints, or search input when the search bar is active.
    let hints_widget = if search.active {
        let query_colour = if search.has_regex_error { Color::Red } else { Color::White };
        Paragraph::new(Line::from(vec![
            Span::styled(format!("Search [{}]: ", search.mode_label), Style::default().fg(Color::Cyan)),
            Span::styled(search.query, Style::default().fg(query_colour)),
            Span::styled("\u{2588}", Style::default().fg(Color::White)),
            Span::styled("  Esc:cancel  Enter:apply", Style::default().fg(Color::DarkGray)),
        ]))
    } else {
        Paragraph::new(Line::from(vec![Span::styled(
            "Tab:panel  Enter:detail  j/k:nav  1-4:section  /:search  ?:help  q:quit",
            Style::default().fg(Color::DarkGray),
        )]))
    };
    frame.render_widget(hints_widget, chunks[2]);
}
