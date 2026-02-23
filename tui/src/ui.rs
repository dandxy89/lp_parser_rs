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
use ratatui::widgets::{Block, Borders};

use crate::app::{App, Focus, Section};
use crate::widgets::{detail, focus_border_style, help, search_popup, sidebar, solve, status_bar, summary};

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

    let filter_count = app.name_list_len();
    let report_summary = app.report.summary();
    let total_changes = report_summary.total_changes();

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
    app.layout.section_selector = sidebar_chunks[0];
    app.layout.name_list = sidebar_chunks[1];
    app.layout.detail = detail_area;
    app.layout.name_list_height = sidebar_chunks[1].height;
    app.layout.detail_height = detail_area.height;

    // Section Selector
    sidebar::draw_section_selector(frame, sidebar_chunks[0], app);

    // Name List
    sidebar::draw_name_list(frame, sidebar_chunks[1], app);

    // Detail Panel
    draw_detail_panel(frame, detail_area, app, &report_summary);

    // Status bar (drawn after detail so detail_content_lines is populated).
    let detail_pos = if app.focus == Focus::Detail && app.layout.detail_content_lines > 0 {
        Some(status_bar::DetailPosition { scroll: app.detail_scroll, content_lines: app.layout.detail_content_lines })
    } else {
        None
    };
    let yank_flash = if app.yank.flash.is_some() { Some(status_bar::YankFlash { message: &app.yank.message }) } else { None };
    // Compute section-specific diff counts for the status bar using the
    // already-derived report_summary to avoid re-accessing report fields.
    let section_counts = match app.active_section {
        Section::Summary => report_summary.aggregate_counts(),
        Section::Variables => report_summary.variables,
        Section::Constraints => report_summary.constraints,
        Section::Objectives => report_summary.objectives,
    };
    status_bar::draw_status_bar(
        frame,
        outer[1],
        &status_bar::StatusBarParams {
            total_changes,
            section_counts: &section_counts,
            filter_label: app.filter.label(),
            filter_count,
            detail_position: detail_pos.as_ref(),
            yank_flash: yank_flash.as_ref(),
        },
    );

    // Search pop-up overlay — rendered on top of main content.
    if app.search_popup.visible {
        search_popup::draw_search_popup(frame, frame.area(), app);
    }

    // Solve overlay — rendered on top of main content.
    if !matches!(app.solver.state, crate::state::SolveState::Idle) {
        solve::draw_solve_overlay(frame, frame.area(), app);
    }

    // Help overlay — rendered last so it draws on top of everything.
    if app.show_help {
        help::draw_help(frame, frame.area());
    }
}

/// Draw the detail panel on the right side.
fn draw_detail_panel(frame: &mut Frame, area: ratatui::layout::Rect, app: &mut App, _report_summary: &crate::diff_model::DiffSummary) {
    let border_style = focus_border_style(app.focus, Focus::Detail);

    let content_lines = if app.active_section == Section::Summary {
        let block = Block::default().borders(Borders::ALL).border_style(border_style).title(" Summary ");
        let inner = block.inner(area);
        frame.render_widget(block, area);
        summary::draw_summary(frame, inner, &app.summary_lines, app.detail_scroll)
    } else if let Some(entry_index) = app.selected_entry_index() {
        app.ensure_coeff_row_cache();
        let scroll = app.detail_scroll;
        let cached_rows = app.cached_coeff_rows();
        match app.active_section {
            Section::Variables => {
                debug_assert!(entry_index < app.report.variables.entries.len(), "variable entry_index {entry_index} out of bounds");
                detail::render_variable_detail(frame, area, &app.report.variables.entries[entry_index], border_style, scroll)
            }
            Section::Constraints => {
                debug_assert!(entry_index < app.report.constraints.entries.len(), "constraint entry_index {entry_index} out of bounds");
                detail::render_constraint_detail(
                    frame,
                    area,
                    &app.report.constraints.entries[entry_index],
                    border_style,
                    scroll,
                    cached_rows,
                    &app.report.interner,
                )
            }
            Section::Objectives => {
                debug_assert!(entry_index < app.report.objectives.entries.len(), "objective entry_index {entry_index} out of bounds");
                detail::render_objective_detail(
                    frame,
                    area,
                    &app.report.objectives.entries[entry_index],
                    border_style,
                    scroll,
                    cached_rows,
                    &app.report.interner,
                )
            }
            Section::Summary => unreachable!("handled above"),
        }
    } else {
        let label = match app.active_section {
            Section::Variables => "Select a variable from the list",
            Section::Constraints => "Select a constraint from the list",
            Section::Objectives => "Select an objective from the list",
            Section::Summary => unreachable!("handled above"),
        };
        sidebar::draw_empty_detail(frame, area, label, border_style);
        0
    };

    app.layout.detail_content_lines = content_lines;
}
