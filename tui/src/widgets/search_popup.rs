//! Telescope-style search pop-up overlay.
//!
//! Renders a centred floating pop-up with:
//! - Top bar: search input with mode indicator and match count
//! - Left pane: ranked results list with match highlighting and diff badges
//! - Right pane: detail preview of the currently highlighted result
//! - Bottom bar: hint line showing mode prefixes and Esc to cancel

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState};

use crate::app::App;
use crate::search;
use crate::state::{SearchResult, Section};
use crate::theme::theme;
use crate::widgets::{detail, kind_prefix, kind_style, sidebar, summary};

/// Section tag prefix for display in results list.
const fn section_tag(section: Section) -> &'static str {
    match section {
        Section::Summary => "[sum]",
        Section::Variables => "[var]",
        Section::Constraints => "[con]",
        Section::Objectives => "[obj]",
    }
}

/// Draw the search pop-up overlay on top of the current frame.
pub fn draw_search_popup(frame: &mut Frame, area: Rect, app: &App) {
    debug_assert!(area.width > 0 && area.height > 0, "search popup area must be non-zero");
    let popup = centred_rect(area);

    // Clear the background behind the pop-up.
    frame.render_widget(Clear, popup);

    let (mode, _) = search::parse_query(&app.search_popup.query);
    let mode_label = mode.label();
    let match_count = app.search_popup.results.len();

    // Vertical layout: top input (3 rows with border), main content, bottom hints (3 rows with border).
    let v_chunks = Layout::vertical([
        Constraint::Length(3), // search input bar
        Constraint::Min(1),    // main content area
        Constraint::Length(3), // hint bar
    ])
    .split(popup);

    // Top bar: search input
    draw_search_input(frame, v_chunks[0], &app.search_popup.query, mode_label, match_count);

    // Main area: horizontal split into results list + detail preview
    let h_chunks = Layout::horizontal([
        Constraint::Percentage(40), // results list
        Constraint::Percentage(60), // detail preview
    ])
    .split(v_chunks[1]);

    draw_results_list(frame, h_chunks[0], &app.search_popup.results, &app.search_name_buffer, app.search_popup.selected);
    draw_detail_preview(frame, h_chunks[1], app);

    // Bottom bar: hints
    draw_hints(frame, v_chunks[2]);
}

/// Draw the search input bar at the top of the pop-up.
fn draw_search_input(frame: &mut Frame, area: Rect, query: &str, mode_label: &str, match_count: usize) {
    let t = theme();
    let input_spans = vec![
        Span::styled(" > ", Style::default().fg(t.accent).add_modifier(Modifier::BOLD)),
        Span::styled(query, Style::default().fg(t.text)),
        // Blinking cursor block
        Span::styled("\u{2588}", Style::default().fg(t.accent)),
    ];

    let right_text = format!("[{mode_label}] {match_count} matches");
    // We can't easily right-align within a single Line, so we pad.
    let used = 3 + query.len() + 1; // " > " + query + cursor
    let available = area.width.saturating_sub(2) as usize; // subtract borders
    let padding = available.saturating_sub(used + right_text.len());

    let mut spans = input_spans;
    spans.push(Span::raw(" ".repeat(padding)));
    spans.push(Span::styled(right_text, Style::default().fg(t.muted)));

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(t.accent))
        .title(Span::styled(" Search ", Style::default().fg(t.accent).add_modifier(Modifier::BOLD)));

    let paragraph = Paragraph::new(Line::from(spans)).block(block);
    frame.render_widget(paragraph, area);
}

/// Draw the ranked results list on the left side.
fn draw_results_list(frame: &mut Frame, area: Rect, results: &[SearchResult], names: &[String], selected: usize) {
    let t = theme();
    let items: Vec<ListItem> = results
        .iter()
        .map(|r| {
            let tag = section_tag(r.section);
            let prefix = kind_prefix(r.kind);
            let style = kind_style(r.kind);

            // Resolve name from the name buffer.
            debug_assert!(r.haystack_index < names.len(), "haystack_index {} out of bounds (len {})", r.haystack_index, names.len());
            let name = &names[r.haystack_index];

            // Build name with match highlighting.
            let name_spans = build_highlighted_name(name, &r.match_indices, style);

            let mut spans = vec![Span::styled(format!("{tag} "), Style::default().fg(t.muted)), Span::styled(format!("{prefix} "), style)];
            spans.extend(name_spans);

            // Show fuzzy score when available.
            if r.score > 0 {
                spans.push(Span::styled(format!(" ({})", r.score), Style::default().fg(t.muted)));
            }

            ListItem::new(Line::from(spans))
        })
        .collect();

    let block = Block::default().borders(Borders::ALL).border_style(Style::default().fg(t.muted)).title(" Results ");

    let mut state = ListState::default();
    if !results.is_empty() {
        state.select(Some(selected));
    }

    let list = List::new(items)
        .block(block)
        .highlight_style(Style::default().bg(t.highlight_bg).add_modifier(Modifier::BOLD))
        .highlight_symbol("\u{25b6} ");

    frame.render_stateful_widget(list, area, &mut state);

    // Scrollbar for long result lists.
    if results.len() > area.height.saturating_sub(2) as usize {
        let mut scrollbar_state = ScrollbarState::new(results.len()).position(selected);
        frame.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight).begin_symbol(None).end_symbol(None),
            area,
            &mut scrollbar_state,
        );
    }
}

/// Build spans for a name with fuzzy match positions highlighted.
fn build_highlighted_name<'a>(name: &'a str, match_indices: &[usize], base_style: Style) -> Vec<Span<'a>> {
    debug_assert!(name.is_ascii(), "fuzzy match highlighting assumes ASCII names");
    if match_indices.is_empty() {
        return vec![Span::styled(name, base_style)];
    }

    let highlight_style = base_style.add_modifier(Modifier::BOLD | Modifier::UNDERLINED);
    let mut spans = Vec::new();
    let mut last_end = 0;

    for &idx in match_indices {
        if idx > name.len() {
            continue;
        }
        // Add non-highlighted segment before this match.
        if idx > last_end {
            spans.push(Span::styled(&name[last_end..idx], base_style));
        }
        // Add highlighted character.
        let end = (idx + 1).min(name.len());
        spans.push(Span::styled(&name[idx..end], highlight_style));
        last_end = end;
    }

    // Remaining non-highlighted tail.
    if last_end < name.len() {
        spans.push(Span::styled(&name[last_end..], base_style));
    }

    spans
}

/// Draw the detail preview on the right side of the pop-up.
fn draw_detail_preview(frame: &mut Frame, area: Rect, app: &App) {
    let t = theme();
    let border_style = Style::default().fg(t.muted);

    if app.search_popup.results.is_empty() {
        sidebar::draw_empty_detail(frame, area, "No results", border_style);
        return;
    }

    let selected = app.search_popup.selected.min(app.search_popup.results.len().saturating_sub(1));
    let result = &app.search_popup.results[selected];
    let scroll = app.search_popup.scroll;

    match result.section {
        Section::Variables => {
            if let Some(entry) = app.report.variables.entries.get(result.entry_index) {
                detail::render_variable_detail(frame, area, entry, border_style, scroll);
            } else {
                sidebar::draw_empty_detail(frame, area, "Entry not found", border_style);
            }
        }
        Section::Constraints => {
            if let Some(entry) = app.report.constraints.entries.get(result.entry_index) {
                detail::render_constraint_detail(frame, area, entry, border_style, scroll, None, &app.report.interner);
            } else {
                sidebar::draw_empty_detail(frame, area, "Entry not found", border_style);
            }
        }
        Section::Objectives => {
            if let Some(entry) = app.report.objectives.entries.get(result.entry_index) {
                detail::render_objective_detail(frame, area, entry, border_style, scroll, None, &app.report.interner);
            } else {
                sidebar::draw_empty_detail(frame, area, "Entry not found", border_style);
            }
        }
        Section::Summary => {
            // Summary entries don't appear in search results, but handle gracefully.
            let block = Block::default().borders(Borders::ALL).border_style(border_style).title(" Summary ");
            let inner = block.inner(area);
            frame.render_widget(block, area);
            summary::draw_summary(frame, inner, &app.summary_lines, scroll);
        }
    }
}

/// Draw the hint bar at the bottom of the pop-up.
fn draw_hints(frame: &mut Frame, area: Rect) {
    let t = theme();
    let hints = Line::from(vec![
        Span::styled("  r:", Style::default().fg(t.accent).add_modifier(Modifier::BOLD)),
        Span::styled("regex  ", Style::default().fg(t.muted)),
        Span::styled("s:", Style::default().fg(t.accent).add_modifier(Modifier::BOLD)),
        Span::styled("substring  ", Style::default().fg(t.muted)),
        Span::styled("(default: fuzzy)  ", Style::default().fg(t.muted)),
        Span::styled("j/k", Style::default().fg(t.accent)),
        Span::styled(" navigate  ", Style::default().fg(t.muted)),
        Span::styled("Enter", Style::default().fg(t.accent)),
        Span::styled(" select  ", Style::default().fg(t.muted)),
        Span::styled("Esc", Style::default().fg(t.accent)),
        Span::styled(" cancel", Style::default().fg(t.muted)),
    ]);

    let block = Block::default().borders(Borders::ALL).border_style(Style::default().fg(t.muted));

    let paragraph = Paragraph::new(hints).block(block);
    frame.render_widget(paragraph, area);
}

/// Compute a centred rectangle at ~80% of terminal size, clamped.
fn centred_rect(area: Rect) -> Rect {
    let width = ((area.width * 4) / 5).max(40).min(area.width);
    let height = ((area.height * 4) / 5).max(15).min(area.height);

    super::centred_rect(area, width, height)
}
