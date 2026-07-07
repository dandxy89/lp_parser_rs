//! Top-level draw dispatcher.
//!
//! Renders a unified single-window layout:
//!
//! ```text
//!  Summary │ Numerics │ Variables │ Constraints │ Objectives
//! ╭──────────────────╮╭───────────────────────────────────╮
//! │ Name List        ││                                   │
//! │ (filtered)       ││         Detail Panel              │
//! │                  ││                                   │
//! │                  ││                                   │
//! ╰──────────────────╯╰───────────────────────────────────╯
//!   status bar
//! ```

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, ScrollbarState};

use crate::app::{App, AppMode, Focus, Section};
use crate::state::DetailView;
use crate::theme::theme;
use crate::widgets::{
    centred_rect, detail, focus_border_style, help, palette, panel_block, panel_scrollbar, raw_diff, search_popup, sidebar, solve,
    status_bar, summary, what_if,
};

/// Minimum width for the sidebar panel in columns.
const SIDEBAR_MIN_WIDTH: u16 = 20;

/// Fraction of the main area width allocated to the sidebar (1/N).
const SIDEBAR_WIDTH_DIVISOR: u16 = 5;

/// Minimum terminal size below which the normal layout is unusable; a hint is
/// shown instead of a cramped, broken UI.
const MIN_WIDTH: u16 = 60;
const MIN_HEIGHT: u16 = 15;

/// Render the entire TUI for the current application state.
pub fn draw(frame: &mut Frame, app: &mut App) {
    // Below the minimum size the layout cannot render legibly — show a hint
    // rather than a broken UI.
    let full = frame.area();
    if full.width < MIN_WIDTH || full.height < MIN_HEIGHT {
        draw_too_small(frame, full);
        return;
    }

    // Ensure the active section's filter cache is fresh before reading it.
    app.ensure_active_section_cache();

    let filter_count = app.name_list_len();
    let report_summary = app.cached_summary;
    let total_changes = report_summary.total_changes();

    let outer = Layout::vertical([
        Constraint::Length(1), // tab bar
        Constraint::Min(0),    // main area
        Constraint::Length(1), // status bar
    ])
    .split(frame.area());

    // Main area: horizontal split into sidebar + detail.
    let tab_bar_area = outer[0];
    let main_area = outer[1];

    let sidebar_width = (main_area.width / SIDEBAR_WIDTH_DIVISOR).max(SIDEBAR_MIN_WIDTH).min(main_area.width);
    let h_chunks = Layout::horizontal([Constraint::Length(sidebar_width), Constraint::Min(0)]).split(main_area);

    let sidebar_area = h_chunks[0];
    let detail_area = h_chunks[1];

    // Store layout rects and heights on app for mouse hit-testing and page scrolling.
    app.layout.section_selector = tab_bar_area;
    app.layout.name_list = sidebar_area;
    app.layout.detail = detail_area;
    app.layout.name_list_height = sidebar_area.height;
    app.layout.detail_height = detail_area.height;

    // Tab bar across the full width.
    sidebar::draw_tab_bar(frame, tab_bar_area, app);

    // Name List (full sidebar height).
    sidebar::draw_name_list(frame, sidebar_area, app);

    // Detail Panel
    draw_detail_panel(frame, detail_area, app);

    // Detail scrollbar — the sidebar has one; the detail panel deserves the
    // same position feedback without needing focus.
    let detail_inner_height = detail_area.height.saturating_sub(2) as usize;
    if app.layout.detail_content_lines > detail_inner_height {
        let mut scrollbar_state = ScrollbarState::new(app.layout.detail_content_lines).position(app.detail_scroll as usize);
        frame.render_stateful_widget(panel_scrollbar(), detail_area, &mut scrollbar_state);
    }

    // Status bar (drawn after detail so detail_content_lines is populated).
    draw_status(frame, outer[2], app, &report_summary, total_changes, filter_count);

    // Search pop-up overlay — rendered on top of main content.
    if app.search_popup.visible {
        search_popup::draw_search_popup(frame, frame.area(), app);
    }

    // Command palette overlay.
    if app.palette.visible {
        palette::draw_palette(frame, frame.area(), app);
    }

    // Solve overlay — rendered on top of main content.
    if !matches!(app.solver.state, crate::state::SolveState::Idle) {
        solve::draw_solve_overlay(frame, frame.area(), app);
    }

    // What-if prompt overlay (edit constraint RHS & re-solve).
    if let Some(prompt) = &app.what_if {
        what_if::draw_what_if(frame, frame.area(), prompt);
    }

    // Help overlay — rendered last so it draws on top of everything.
    if app.show_help {
        help::draw_help(frame, frame.area(), app);
    }
}

/// Draw the bottom status bar, choosing the diff or inspect left segment.
fn draw_status(
    frame: &mut Frame,
    area: Rect,
    app: &App,
    report_summary: &crate::diff_model::DiffSummary,
    total_changes: usize,
    filter_count: usize,
) {
    let detail_pos = if app.focus == Focus::Detail && app.layout.detail_content_lines > 0 {
        Some(status_bar::DetailPosition { scroll: app.detail_scroll, content_lines: app.layout.detail_content_lines })
    } else {
        None
    };
    let yank_flash = if app.yank.flash.is_some() { Some(status_bar::YankFlash { message: &app.yank.message }) } else { None };
    // Tolerance indicator — only shown when at least one tolerance is active.
    let tolerance_label = {
        let options = &app.diff_options;
        if options.abs_tol > 0.0 || options.rel_tol > 0.0 {
            let mut label = String::with_capacity(24);
            if options.abs_tol > 0.0 {
                label.push_str("abs:");
                label.push_str(&crate::app::format_tolerance(options.abs_tol));
            }
            if options.rel_tol > 0.0 {
                if !label.is_empty() {
                    label.push(' ');
                }
                label.push_str("rel:");
                label.push_str(&crate::app::format_tolerance(options.rel_tol));
            }
            Some(label)
        } else {
            None
        }
    };
    // Compute section-specific diff counts for the status bar using the
    // already-derived report_summary to avoid re-accessing report fields.
    let section_counts = match app.active_section {
        Section::Summary | Section::Numerics => report_summary.aggregate_counts(),
        Section::Variables => report_summary.variables,
        Section::Constraints => report_summary.constraints,
        Section::Objectives => report_summary.objectives,
    };
    // Watch-mode indicator, shown alongside the sort/tolerance indicators.
    let watch_reloading = if app.watch.enabled { Some(app.watch.is_reloading()) } else { None };
    // Inspect mode shows the filename and section entry count in place of the
    // diff change/filter segment. The filename String is only built (and the
    // segment only shown) in inspect mode; it lives here so InspectInfo can
    // borrow it across the draw call below.
    let inspect_file = (app.mode == AppMode::Inspect).then(|| crate::widgets::short_filename(&app.report.file1));
    let inspect = inspect_file.as_deref().map(|file| {
        let (label, count) = match app.active_section {
            Section::Variables => ("variables", app.report.variables.entries.len()),
            Section::Constraints => ("constraints", app.report.constraints.entries.len()),
            Section::Objectives => ("objectives", app.report.objectives.entries.len()),
            Section::Summary | Section::Numerics => {
                ("total", app.report.variables.entries.len() + app.report.constraints.entries.len() + app.report.objectives.entries.len())
            }
        };
        status_bar::InspectInfo { file, section_label: label, entry_count: count }
    });
    status_bar::draw_status_bar(
        frame,
        area,
        &status_bar::StatusBarParams {
            total_changes,
            section_counts: &section_counts,
            filter_label: app.filter.label(),
            filter_count,
            detail_position: detail_pos.as_ref(),
            yank_flash: yank_flash.as_ref(),
            ignore_order: app.ignore_order,
            sort_label: app.sort_mode.label(),
            tolerance_label: tolerance_label.as_deref(),
            watch_reloading,
            inspect,
        },
    );
}

/// Render a centred "terminal too small" hint for sub-minimum window sizes.
fn draw_too_small(frame: &mut Frame, area: Rect) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let t = theme();
    let lines = vec![
        Line::from(Span::styled("Terminal too small", Style::default().fg(t.error).add_modifier(Modifier::BOLD))),
        Line::from(Span::styled(format!("Need at least {MIN_WIDTH}×{MIN_HEIGHT}"), Style::default().fg(t.muted))),
    ];
    #[allow(clippy::cast_possible_truncation)] // tiny fixed message
    let popup = centred_rect(area, 24.min(area.width), (lines.len() as u16).min(area.height));
    frame.render_widget(Paragraph::new(lines).centered(), popup);
}

/// Render the neutral single-model detail for the selected inspect entry.
/// Returns the total content line count.
fn draw_inspect_detail(frame: &mut Frame, area: Rect, app: &App, entry_index: usize, border_style: Style) -> usize {
    let scroll = app.detail_scroll;
    let interner = &app.report.interner;
    match app.active_section {
        Section::Variables => {
            debug_assert!(entry_index < app.report.variables.entries.len(), "variable entry_index {entry_index} out of bounds");
            detail::render_inspect_variable(frame, area, &app.report.variables.entries[entry_index], border_style, scroll)
        }
        Section::Constraints => {
            debug_assert!(entry_index < app.report.constraints.entries.len(), "constraint entry_index {entry_index} out of bounds");
            detail::render_inspect_constraint(frame, area, &app.report.constraints.entries[entry_index], border_style, scroll, interner)
        }
        Section::Objectives => {
            debug_assert!(entry_index < app.report.objectives.entries.len(), "objective entry_index {entry_index} out of bounds");
            detail::render_inspect_objective(frame, area, &app.report.objectives.entries[entry_index], border_style, scroll, interner)
        }
        Section::Summary | Section::Numerics => unreachable!("static sections are handled by draw_detail_panel"),
    }
}

/// Draw the detail panel on the right side.
fn draw_detail_panel(frame: &mut Frame, area: ratatui::layout::Rect, app: &mut App) {
    let border_style = focus_border_style(app.focus, Focus::Detail);

    let content_lines = if app.active_section == Section::Summary {
        let block = panel_block(border_style).title(" Summary ");
        let inner = block.inner(area);
        frame.render_widget(block, area);
        summary::draw_summary(frame, inner, &app.summary_lines, app.detail_scroll)
    } else if app.active_section == Section::Numerics {
        // Numerics renders exactly like Summary: pre-built cached lines, windowed.
        let block = panel_block(border_style).title(" Numerics ");
        let inner = block.inner(area);
        frame.render_widget(block, area);
        summary::draw_summary(frame, inner, &app.numerics_lines, app.detail_scroll)
    } else if let Some(entry_index) = app.selected_entry_index() {
        // Inspect mode renders each entry as a neutral single-model view.
        if app.mode == AppMode::Inspect {
            draw_inspect_detail(frame, area, app, entry_index, border_style)
        } else {
            // Check for raw view mode on supported sections (Constraints, Objectives).
            let use_raw = app.detail_view == DetailView::Raw && matches!(app.active_section, Section::Constraints | Section::Objectives);

            if use_raw {
                let (old_text, new_text) = app.extract_raw_texts();
                // Variables show a message; Constraints/Objectives show the raw text.
                raw_diff::draw_raw_diff(frame, area, old_text, new_text, app.detail_scroll, border_style)
            } else {
                app.ensure_coeff_row_cache();
                let scroll = app.detail_scroll;
                let cached_rows = app.cached_coeff_rows();
                match app.active_section {
                    Section::Variables => {
                        debug_assert!(entry_index < app.report.variables.entries.len(), "variable entry_index {entry_index} out of bounds");
                        detail::render_variable_detail(frame, area, &app.report.variables.entries[entry_index], border_style, scroll)
                    }
                    Section::Constraints => {
                        debug_assert!(
                            entry_index < app.report.constraints.entries.len(),
                            "constraint entry_index {entry_index} out of bounds"
                        );
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
                        debug_assert!(
                            entry_index < app.report.objectives.entries.len(),
                            "objective entry_index {entry_index} out of bounds"
                        );
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
                    Section::Summary | Section::Numerics => unreachable!("handled above"),
                }
            }
        }
    } else {
        let label = match app.active_section {
            Section::Variables => "Select a variable from the list",
            Section::Constraints => "Select a constraint from the list",
            Section::Objectives => "Select an objective from the list",
            Section::Summary | Section::Numerics => unreachable!("handled above"),
        };
        sidebar::draw_empty_detail(frame, area, label, border_style);
        0
    };

    app.layout.detail_content_lines = content_lines;
}
