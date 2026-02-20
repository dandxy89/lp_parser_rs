//! Search indicator widget.
//!
//! The actual search input is rendered by the status bar; this module provides
//! a one-line indicator shown at the top of the content area when a committed
//! search query is filtering the visible results.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

/// Draw a one-line search indicator above the content area.
///
/// Does nothing when `query` is empty, so callers may invoke this
/// unconditionally without a prior emptiness check.
///
/// `selected_index` is the 0-based index of the currently selected match
/// within the filtered list (if any).
pub fn draw_search_indicator(
    frame: &mut Frame,
    area: Rect,
    query: &str,
    result_count: usize,
    mode_label: &str,
    selected_index: Option<usize>,
) {
    if query.is_empty() {
        return;
    }

    let position_text = if let Some(sel) = selected_index {
        format!("  \u{2014}  {}/{} match(es)", sel + 1, result_count)
    } else {
        format!("  \u{2014}  {result_count} match(es)")
    };

    let line = Line::from(vec![
        Span::styled(format!(" Search [{mode_label}]: "), Style::default().fg(Color::Cyan)),
        Span::styled(format!("\"{query}\""), Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
        Span::styled(position_text, Style::default().fg(Color::DarkGray)),
    ]);

    frame.render_widget(Paragraph::new(line), area);
}
