use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState};

use crate::app::{App, Focus, Section};
use crate::theme::theme;
use crate::widgets::focus_border_style;

/// Draw the section selector as a bordered list in the top-left.
pub fn draw_section_selector(frame: &mut Frame, area: Rect, app: &mut App) {
    debug_assert!(area.width > 0 && area.height > 0, "section selector area must be non-zero");
    let t = theme();
    let border_style = focus_border_style(app.focus, Focus::SectionSelector);

    let items: Vec<ListItem> = Section::ALL
        .iter()
        .map(|section| {
            let marker = if *section == app.active_section { "\u{25b6} " } else { "  " };
            let style = if *section == app.active_section {
                Style::default().fg(t.text).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(t.muted)
            };
            ListItem::new(Line::from(Span::styled(format!("{marker}{}", section.label()), style)))
        })
        .collect();

    let block = Block::default().borders(Borders::ALL).border_style(border_style).title(" Sections ");

    let list = List::new(items)
        .block(block)
        .highlight_style(Style::default().bg(t.highlight_bg).add_modifier(Modifier::BOLD))
        .highlight_symbol("");

    frame.render_stateful_widget(list, area, &mut app.section_selector_state);
}

/// Draw the name list below the section selector.
pub fn draw_name_list(frame: &mut Frame, area: Rect, app: &mut App) {
    debug_assert!(area.width > 0 && area.height > 0, "name list area must be non-zero");
    let t = theme();
    let border_style = focus_border_style(app.focus, Focus::NameList);

    match app.active_section {
        Section::Summary => {
            // When Summary is active, show a quick-nav list of sections with counts.
            let counts = app.report.summary();
            let items: Vec<ListItem> = vec![
                ListItem::new(Line::from(Span::styled(
                    format!("  Variables    ({})", counts.variables.changed()),
                    Style::default().fg(t.muted),
                ))),
                ListItem::new(Line::from(Span::styled(
                    format!("  Constraints  ({})", counts.constraints.changed()),
                    Style::default().fg(t.muted),
                ))),
                ListItem::new(Line::from(Span::styled(
                    format!("  Objectives   ({})", counts.objectives.changed()),
                    Style::default().fg(t.muted),
                ))),
            ];

            let block = Block::default().borders(Borders::ALL).border_style(Style::default().fg(Color::Reset)).title(" Overview ");

            let list = List::new(items).block(block);
            frame.render_widget(list, area);
        }
        Section::Variables => {
            let idx = Section::Variables.list_index().expect("Variables has a list_index");
            let (filtered, cached_lines, state) = app.section_states[idx].indices_lines_and_state_mut();
            let total = app.report.variables.counts.total();
            draw_entry_name_list(
                frame,
                area,
                &NameListParams { filtered_indices: filtered, cached_lines, section_label: "variables", total_count: total, border_style },
                state,
            );
        }
        Section::Constraints => {
            let idx = Section::Constraints.list_index().expect("Constraints has a list_index");
            let (filtered, cached_lines, state) = app.section_states[idx].indices_lines_and_state_mut();
            let total = app.report.constraints.counts.total();
            draw_entry_name_list(
                frame,
                area,
                &NameListParams {
                    filtered_indices: filtered,
                    cached_lines,
                    section_label: "constraints",
                    total_count: total,
                    border_style,
                },
                state,
            );
        }
        Section::Objectives => {
            let idx = Section::Objectives.list_index().expect("Objectives has a list_index");
            let (filtered, cached_lines, state) = app.section_states[idx].indices_lines_and_state_mut();
            let total = app.report.objectives.counts.total();
            draw_entry_name_list(
                frame,
                area,
                &NameListParams { filtered_indices: filtered, cached_lines, section_label: "objectives", total_count: total, border_style },
                state,
            );
        }
    }
}

/// Parameters for rendering a section's name list in the sidebar.
pub struct NameListParams<'a> {
    pub filtered_indices: &'a [usize],
    /// Pre-built lines (one per filtered entry), cached in `SectionViewState`.
    pub cached_lines: &'a [Line<'static>],
    pub section_label: &'a str,
    pub total_count: usize,
    pub border_style: Style,
}

/// Draw a compact name list for a section's entries in the sidebar.
fn draw_entry_name_list(frame: &mut Frame, area: Rect, params: &NameListParams<'_>, state: &mut ratatui::widgets::ListState) {
    debug_assert_eq!(
        params.filtered_indices.len(),
        params.cached_lines.len(),
        "filtered_indices and cached_lines must be the same length for section '{}'",
        params.section_label,
    );

    let t = theme();

    // Convert cached `Line<'static>` to `ListItem` — cheap wrapping, no allocation.
    let items: Vec<ListItem> = params.cached_lines.iter().map(|line| ListItem::new(line.clone())).collect();

    let selected_position = state.selected().map_or(0, |selection| selection + 1);
    let filtered_len = params.filtered_indices.len();
    let title = format!(" {selected_position}/{filtered_len} {} ({} total) ", params.section_label, params.total_count);

    let block = Block::default().borders(Borders::ALL).border_style(params.border_style).title(title);

    let list = List::new(items)
        .block(block)
        .highlight_style(Style::default().bg(t.highlight_bg).add_modifier(Modifier::BOLD))
        .highlight_symbol("\u{25b6} ");

    frame.render_stateful_widget(list, area, state);

    // Scrollbar for long lists.
    if params.filtered_indices.len() > area.height.saturating_sub(2) as usize {
        let mut scrollbar_state = ScrollbarState::new(params.filtered_indices.len()).position(state.selected().unwrap_or(0));
        frame.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight).begin_symbol(None).end_symbol(None),
            area,
            &mut scrollbar_state,
        );
    }
}

/// Render an empty detail panel with a hint message.
pub fn draw_empty_detail(frame: &mut Frame, area: Rect, message: &str, border_style: Style) {
    debug_assert!(area.width > 0 && area.height > 0, "empty detail area must be non-zero");
    let t = theme();
    let block = Block::default().borders(Borders::ALL).border_style(border_style).title(" Detail ");
    let paragraph = Paragraph::new(Line::from(Span::styled(format!("  {message}"), Style::default().fg(t.muted)))).block(block);
    frame.render_widget(paragraph, area);
}
