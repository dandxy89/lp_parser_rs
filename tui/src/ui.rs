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
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::{Block, Borders};

use crate::app::{App, Focus, Section};
use crate::search;
use crate::widgets::status_bar::SearchState;
use crate::widgets::{detail, help, search as search_widget, search_popup, sidebar, status_bar, summary};

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

    // Main area: horizontal split into sidebar + detail.
    let main_area = outer[0];

    let sidebar_width = (main_area.width / SIDEBAR_WIDTH_DIVISOR).max(SIDEBAR_MIN_WIDTH).min(main_area.width);
    let h_chunks = Layout::horizontal([Constraint::Length(sidebar_width), Constraint::Min(0)]).split(main_area);

    let sidebar_area = h_chunks[0];
    let detail_area = h_chunks[1];

    // Sidebar: vertical split into section selector + name list.
    let sidebar_chunks = Layout::vertical([Constraint::Length(SECTION_SELECTOR_HEIGHT), Constraint::Min(0)]).split(sidebar_area);

    // Store layout rects and heights on app for mouse hit-testing and page scrolling.
    app.section_selector_rect = sidebar_chunks[0];
    app.name_list_rect = sidebar_chunks[1];
    app.detail_rect = detail_area;
    app.name_list_height = sidebar_chunks[1].height;
    app.detail_height = detail_area.height;

    // Section Selector
    sidebar::draw_section_selector(frame, sidebar_chunks[0], app);

    // Name List
    sidebar::draw_name_list(frame, sidebar_chunks[1], app);

    // Detail Panel
    draw_detail_panel(frame, detail_area, app, &report_summary);

    // Status bar (drawn after detail so detail_content_lines is populated).
    let search_state = SearchState { active: app.search_active, query: &app.search_query, mode_label: search_mode_label, has_regex_error };
    let detail_pos = if app.focus == Focus::Detail && app.detail_content_lines > 0 {
        Some(status_bar::DetailPosition { scroll: app.detail_scroll, content_lines: app.detail_content_lines })
    } else {
        None
    };
    let yank_flash = if app.yank_flash.is_some() { Some(status_bar::YankFlash { message: &app.yank_message }) } else { None };
    status_bar::draw_status_bar(
        frame,
        outer[1],
        total_changes,
        app.filter.label(),
        filter_count,
        &search_state,
        detail_pos.as_ref(),
        yank_flash.as_ref(),
    );

    // Search pop-up overlay — rendered on top of main content.
    if app.show_search_popup {
        search_popup::draw_search_popup(frame, frame.area(), app);
    }

    // Help overlay — rendered last so it draws on top of everything.
    if app.show_help {
        help::draw_help(frame, frame.area());
    }
}

/// Draw the detail panel on the right side.
fn draw_detail_panel(frame: &mut Frame, area: ratatui::layout::Rect, app: &mut App, report_summary: &crate::diff_model::DiffSummary) {
    let border_style = if app.focus == Focus::Detail {
        Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Reset)
    };

    // If there's a committed search query, show an indicator at the top.
    let content_area = if !app.search_query.is_empty() && !app.search_active {
        let chunks = Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).split(area);
        let filter_count = app.current_filter_count();
        let selected_index = app.selected_name_index();
        let (mode, _) = search::parse_query(&app.search_query);
        search_widget::draw_search_indicator(frame, chunks[0], &app.search_query, filter_count, mode.label(), selected_index);
        chunks[1]
    } else {
        area
    };

    let content_lines = match app.active_section {
        Section::Summary => {
            let block = Block::default().borders(Borders::ALL).border_style(border_style).title(" Summary ");
            let inner = block.inner(content_area);
            frame.render_widget(block, content_area);
            summary::draw_summary(
                frame,
                inner,
                &app.report,
                report_summary,
                &app.report.analysis1,
                &app.report.analysis2,
                app.detail_scroll,
            )
        }
        Section::Variables => {
            let (filtered, state) = app.section_states[0].indices_and_state_mut();
            let selected_entry_idx = state.selected().and_then(|sel| filtered.get(sel).copied());

            if let Some(idx) = selected_entry_idx {
                debug_assert!(idx < app.report.variables.entries.len(), "variable entry_idx {idx} out of bounds");
                detail::render_variable_detail(frame, content_area, &app.report.variables.entries[idx], border_style, app.detail_scroll)
            } else {
                sidebar::draw_empty_detail(frame, content_area, "Select a variable from the list", border_style);
                0
            }
        }
        Section::Constraints => {
            let (filtered, state) = app.section_states[1].indices_and_state_mut();
            let selected_entry_idx = state.selected().and_then(|sel| filtered.get(sel).copied());

            if let Some(idx) = selected_entry_idx {
                debug_assert!(idx < app.report.constraints.entries.len(), "constraint entry_idx {idx} out of bounds");
                detail::render_constraint_detail(frame, content_area, &app.report.constraints.entries[idx], border_style, app.detail_scroll)
            } else {
                sidebar::draw_empty_detail(frame, content_area, "Select a constraint from the list", border_style);
                0
            }
        }
        Section::Objectives => {
            let (filtered, state) = app.section_states[2].indices_and_state_mut();
            let selected_entry_idx = state.selected().and_then(|sel| filtered.get(sel).copied());

            if let Some(idx) = selected_entry_idx {
                debug_assert!(idx < app.report.objectives.entries.len(), "objective entry_idx {idx} out of bounds");
                detail::render_objective_detail(frame, content_area, &app.report.objectives.entries[idx], border_style, app.detail_scroll)
            } else {
                sidebar::draw_empty_detail(frame, content_area, "Select an objective from the list", border_style);
                0
            }
        }
    };

    app.detail_content_lines = content_lines;
}
