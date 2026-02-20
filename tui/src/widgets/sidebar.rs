use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState};

use crate::app::{App, Focus, Section};
use crate::diff_model::DiffEntry;
use crate::widgets::{kind_prefix, kind_style};

/// Draw the section selector as a bordered list in the top-left.
pub fn draw_section_selector(frame: &mut Frame, area: Rect, app: &mut App) {
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
pub fn draw_name_list(frame: &mut Frame, area: Rect, app: &mut App) {
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

    let selected_pos = state.selected().map_or(0, |s| s + 1);
    let filtered_len = filtered_indices.len();
    let title = format!(" {selected_pos}/{filtered_len} {section_label} ({total_count} total) ");

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

/// Render an empty detail panel with a hint message.
pub fn draw_empty_detail(frame: &mut Frame, area: Rect, message: &str, border_style: Style) {
    let block = Block::default().borders(Borders::ALL).border_style(border_style).title(" Detail ");
    let paragraph = Paragraph::new(Line::from(Span::styled(format!("  {message}"), Style::default().fg(Color::DarkGray)))).block(block);
    frame.render_widget(paragraph, area);
}
