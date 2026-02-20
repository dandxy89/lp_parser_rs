//! Top-level draw dispatcher.
//!
//! Renders a unified single-window layout:
//!
//! ```text
//! ┌──────────────────┬───────────────────────────────────┐
//! │ Section Selector │                                   │
//! │ (4 items)        │         Detail Panel              │
//! ├──────────────────┤                                   │
//! │ Name List        │                                   │
//! │ (filtered)       │                                   │
//! └──────────────────┴───────────────────────────────────┘
//! │  status bar                                          │
//! └──────────────────────────────────────────────────────┘
//! ```

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState};

use crate::app::{App, Focus, Section};
use crate::diff_model::DiffEntry;
use crate::search;
use crate::widgets::status_bar::SearchState;
use crate::widgets::{detail, help, kind_prefix, kind_style, search as search_widget, status_bar, summary};

/// Minimum width for the sidebar panel in columns.
const SIDEBAR_MIN_WIDTH: u16 = 20;

/// Fraction of the main area width allocated to the sidebar (1/N).
const SIDEBAR_WIDTH_DIVISOR: u16 = 5;

/// Height of the section selector: 4 items + top/bottom border.
const SECTION_SELECTOR_HEIGHT: u16 = 6;

/// Render the entire TUI for the current application state.
pub fn draw(frame: &mut Frame, app: &mut App) {
    // Ensure the active section's filter cache is fresh before reading it.
    app.ensure_active_section_cache();

    let filter_count = app.current_filter_count();
    let has_regex_error = app.has_search_regex_error();
    let report_summary = app.report.summary();
    let total_changes = report_summary.total_changes();

    let (search_mode, _) = search::parse_query(&app.search_query);
    let search_mode_label = search_mode.label();

    let outer = Layout::vertical([
        Constraint::Min(0),    // main area
        Constraint::Length(1), // status bar
    ])
    .split(frame.area());

    // Status bar.
    let search_state = SearchState { active: app.search_active, query: &app.search_query, mode_label: search_mode_label, has_regex_error };
    status_bar::draw_status_bar(frame, outer[1], total_changes, app.filter.label(), filter_count, &search_state);

    // Main area: horizontal split into sidebar + detail.
    let main_area = outer[0];

    let sidebar_width = (main_area.width / SIDEBAR_WIDTH_DIVISOR).max(SIDEBAR_MIN_WIDTH).min(main_area.width);
    let h_chunks = Layout::horizontal([Constraint::Length(sidebar_width), Constraint::Min(0)]).split(main_area);

    let sidebar_area = h_chunks[0];
    let detail_area = h_chunks[1];

    // Sidebar: vertical split into section selector + name list.
    let sidebar_chunks = Layout::vertical([Constraint::Length(SECTION_SELECTOR_HEIGHT), Constraint::Min(0)]).split(sidebar_area);

    // -- Section Selector --
    draw_section_selector(frame, sidebar_chunks[0], app);

    // -- Name List --
    draw_name_list(frame, sidebar_chunks[1], app);

    // -- Detail Panel --
    draw_detail_panel(frame, detail_area, app, &report_summary);

    // Help overlay — rendered last so it draws on top of everything.
    if app.show_help {
        help::draw_help(frame, frame.area());
    }
}

/// Draw the section selector as a bordered list in the top-left.
fn draw_section_selector(frame: &mut Frame, area: Rect, app: &mut App) {
    let border_style = if app.focus == Focus::SectionSelector {
        Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Reset)
    };

    let items: Vec<ListItem> = Section::ALL
        .iter()
        .map(|section| {
            let marker = if *section == app.active_section { "\u{25b6} " } else { "  " };
            let style = if *section == app.active_section {
                Style::default().fg(Color::White).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::DarkGray)
            };
            ListItem::new(Line::from(Span::styled(format!("{marker}{}", section.label()), style)))
        })
        .collect();

    let block = Block::default().borders(Borders::ALL).border_style(border_style).title(" Sections ");

    let list =
        List::new(items).block(block).highlight_style(Style::default().bg(Color::Blue).add_modifier(Modifier::BOLD)).highlight_symbol("");

    frame.render_stateful_widget(list, area, &mut app.section_selector_state);
}

/// Draw the name list below the section selector.
fn draw_name_list(frame: &mut Frame, area: Rect, app: &mut App) {
    let border_style = if app.focus == Focus::NameList {
        Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Reset)
    };

    match app.active_section {
        Section::Summary => {
            // When Summary is active, show a quick-nav list of sections with counts.
            let counts = app.report.summary();
            let items: Vec<ListItem> = vec![
                ListItem::new(Line::from(Span::styled(
                    format!("  Variables    ({})", counts.variables.changed()),
                    Style::default().fg(Color::DarkGray),
                ))),
                ListItem::new(Line::from(Span::styled(
                    format!("  Constraints  ({})", counts.constraints.changed()),
                    Style::default().fg(Color::DarkGray),
                ))),
                ListItem::new(Line::from(Span::styled(
                    format!("  Objectives   ({})", counts.objectives.changed()),
                    Style::default().fg(Color::DarkGray),
                ))),
            ];

            let block = Block::default().borders(Borders::ALL).border_style(Style::default().fg(Color::Reset)).title(" Overview ");

            let list = List::new(items).block(block);
            frame.render_widget(list, area);
        }
        Section::Variables => {
            let (filtered, state) = app.section_states[0].indices_and_state_mut();
            let total = app.report.variables.counts.total();
            draw_entry_name_list(frame, area, &app.report.variables.entries, filtered, state, "variables", total, border_style);
        }
        Section::Constraints => {
            let (filtered, state) = app.section_states[1].indices_and_state_mut();
            let total = app.report.constraints.counts.total();
            draw_entry_name_list(frame, area, &app.report.constraints.entries, filtered, state, "constraints", total, border_style);
        }
        Section::Objectives => {
            let (filtered, state) = app.section_states[2].indices_and_state_mut();
            let total = app.report.objectives.counts.total();
            draw_entry_name_list(frame, area, &app.report.objectives.entries, filtered, state, "objectives", total, border_style);
        }
    }
}

/// Draw a compact name list for a section's entries in the sidebar.
#[allow(clippy::too_many_arguments)]
fn draw_entry_name_list<T: DiffEntry>(
    frame: &mut Frame,
    area: Rect,
    entries: &[T],
    filtered_indices: &[usize],
    state: &mut ratatui::widgets::ListState,
    section_label: &str,
    total_count: usize,
    border_style: Style,
) {
    debug_assert!(filtered_indices.iter().all(|&i| i < entries.len()), "filtered index out of bounds for section '{section_label}'");

    let items: Vec<ListItem> = filtered_indices
        .iter()
        .map(|&idx| {
            let entry = &entries[idx];
            let kind = entry.kind();
            let line = Line::from(vec![
                Span::styled(kind_prefix(kind), kind_style(kind)),
                Span::styled(" ", Style::default()),
                Span::styled(entry.name().to_owned(), kind_style(kind)),
            ]);
            ListItem::new(line)
        })
        .collect();

    let changed = filtered_indices.len();
    let title = format!(" {changed}/{total_count} {section_label} ");

    let block = Block::default().borders(Borders::ALL).border_style(border_style).title(title);

    let list = List::new(items)
        .block(block)
        .highlight_style(Style::default().bg(Color::Blue).add_modifier(Modifier::BOLD))
        .highlight_symbol("\u{25b6} ");

    frame.render_stateful_widget(list, area, state);

    // Scrollbar for long lists.
    if filtered_indices.len() > area.height.saturating_sub(2) as usize {
        let mut scrollbar_state = ScrollbarState::new(filtered_indices.len()).position(state.selected().unwrap_or(0));
        frame.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight).begin_symbol(None).end_symbol(None),
            area,
            &mut scrollbar_state,
        );
    }
}

/// Draw the detail panel on the right side.
fn draw_detail_panel(frame: &mut Frame, area: Rect, app: &mut App, report_summary: &crate::diff_model::DiffSummary) {
    let border_style = if app.focus == Focus::Detail {
        Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Reset)
    };

    // If there's a committed search query, show an indicator at the top.
    let content_area = if !app.search_query.is_empty() && !app.search_active {
        let chunks = Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).split(area);
        let filter_count = app.current_filter_count();
        let (mode, _) = search::parse_query(&app.search_query);
        search_widget::draw_search_indicator(frame, chunks[0], &app.search_query, filter_count, mode.label());
        chunks[1]
    } else {
        area
    };

    match app.active_section {
        Section::Summary => {
            let block = Block::default().borders(Borders::ALL).border_style(border_style).title(" Summary ");
            let inner = block.inner(content_area);
            frame.render_widget(block, content_area);
            summary::draw_summary(frame, inner, &app.report, report_summary);
        }
        Section::Variables => {
            let (filtered, state) = app.section_states[0].indices_and_state_mut();
            let selected_entry_idx = state.selected().and_then(|sel| filtered.get(sel).copied());

            if let Some(idx) = selected_entry_idx {
                debug_assert!(idx < app.report.variables.entries.len(), "variable entry_idx {idx} out of bounds");
                detail::render_variable_detail(frame, content_area, &app.report.variables.entries[idx], border_style, app.detail_scroll);
            } else {
                draw_empty_detail(frame, content_area, "Select a variable from the list", border_style);
            }
        }
        Section::Constraints => {
            let (filtered, state) = app.section_states[1].indices_and_state_mut();
            let selected_entry_idx = state.selected().and_then(|sel| filtered.get(sel).copied());

            if let Some(idx) = selected_entry_idx {
                debug_assert!(idx < app.report.constraints.entries.len(), "constraint entry_idx {idx} out of bounds");
                detail::render_constraint_detail(
                    frame,
                    content_area,
                    &app.report.constraints.entries[idx],
                    border_style,
                    app.detail_scroll,
                );
            } else {
                draw_empty_detail(frame, content_area, "Select a constraint from the list", border_style);
            }
        }
        Section::Objectives => {
            let (filtered, state) = app.section_states[2].indices_and_state_mut();
            let selected_entry_idx = state.selected().and_then(|sel| filtered.get(sel).copied());

            if let Some(idx) = selected_entry_idx {
                debug_assert!(idx < app.report.objectives.entries.len(), "objective entry_idx {idx} out of bounds");
                detail::render_objective_detail(frame, content_area, &app.report.objectives.entries[idx], border_style, app.detail_scroll);
            } else {
                draw_empty_detail(frame, content_area, "Select an objective from the list", border_style);
            }
        }
    }
}

/// Render an empty detail panel with a hint message.
fn draw_empty_detail(frame: &mut Frame, area: Rect, message: &str, border_style: Style) {
    let block = Block::default().borders(Borders::ALL).border_style(border_style).title(" Detail ");
    let paragraph = Paragraph::new(Line::from(Span::styled(format!("  {message}"), Style::default().fg(Color::DarkGray)))).block(block);
    frame.render_widget(paragraph, area);
}
