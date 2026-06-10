use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{List, ListItem, Paragraph, ScrollbarState};

use crate::app::{App, Focus, Section};
use crate::theme::theme;
use crate::widgets::{focus_border_style, panel_block, panel_scrollbar, zebra_style};

/// Draw the section tab bar across the top of the frame.
///
/// Renders ` Summary │ Numerics │ Variables (n) │ … ` and records each tab's
/// column range in `app.layout.tab_bounds` for mouse hit-testing.
pub fn draw_tab_bar(frame: &mut Frame, area: Rect, app: &mut App) {
    // A zero-sized area is an environmental condition (shrunken terminal), not a
    // programming error: drawing into it is a no-op.
    if area.width == 0 || area.height == 0 {
        return;
    }
    let t = theme();
    let focused = app.focus == Focus::SectionSelector;

    let mut spans: Vec<Span<'_>> = Vec::with_capacity(11);
    let mut bounds = [(0_u16, 0_u16); 5];
    spans.push(Span::raw(" "));
    let mut x = area.x.saturating_add(1);

    for (i, label) in app.section_labels.iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled(" \u{2502} ", Style::default().fg(t.border)));
            x = x.saturating_add(3);
        }
        let active = Section::from_index(i) == app.active_section;
        let style = if active {
            let base = Style::default().fg(t.accent).add_modifier(Modifier::BOLD | Modifier::UNDERLINED);
            if focused { base.bg(t.highlight_bg) } else { base }
        } else if focused {
            Style::default().fg(t.text)
        } else {
            Style::default().fg(t.muted)
        };
        spans.push(Span::styled(label.as_ref().to_owned(), style));
        #[allow(clippy::cast_possible_truncation)] // labels are short, far below u16::MAX
        let width = label.chars().count() as u16;
        bounds[i] = (x, x.saturating_add(width));
        x = x.saturating_add(width);
    }

    app.layout.tab_bounds = bounds;
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

/// Draw the name list filling the sidebar.
pub fn draw_name_list(frame: &mut Frame, area: Rect, app: &mut App) {
    // A zero-sized area is an environmental condition (shrunken terminal), not a
    // programming error: drawing into it is a no-op.
    if area.width == 0 || area.height == 0 {
        return;
    }
    let t = theme();
    let border_style = focus_border_style(app.focus, Focus::NameList);

    match app.active_section {
        Section::Summary | Section::Numerics => {
            // Static sections (Summary, Numerics) have no entry list — show a
            // quick-nav overview of the list sections with their change counts.
            let counts = app.cached_summary;
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

            let block = panel_block(Style::default().fg(t.border)).title(" Overview ");

            let list = List::new(items).block(block);
            frame.render_widget(list, area);
        }
        section @ (Section::Variables | Section::Constraints | Section::Objectives) => {
            draw_section_entry_list(frame, area, app, section, border_style);
        }
    }
}

/// Resolve the per-section label and total count, then render the entry name list.
///
/// Collapses the near-identical match arms in `draw_name_list` into one place.
/// Must not be called for static sections (Summary, Numerics), which have no entry list.
fn draw_section_entry_list(frame: &mut Frame, area: Rect, app: &mut App, section: Section, border_style: Style) {
    debug_assert!(section.list_index().is_some(), "draw_section_entry_list called for static section {section:?}");
    let (section_label, total_count) = match section {
        Section::Variables => ("variables", app.report.variables.counts.total()),
        Section::Constraints => ("constraints", app.report.constraints.counts.total()),
        Section::Objectives => ("objectives", app.report.objectives.counts.total()),
        Section::Summary | Section::Numerics => return,
    };
    let idx = section.list_index().expect("list section has a list_index");
    let (filtered, cached_lines, state) = app.section_states[idx].indices_lines_and_state_mut();
    draw_entry_name_list(
        frame,
        area,
        &NameListParams { filtered_indices: filtered, cached_lines, section_label, total_count, border_style },
        state,
    );
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
///
/// Uses virtualised rendering: only `ListItem`s for the visible window are
/// allocated, keeping the per-frame cost at O(visible_height) instead of
/// O(total_items).  This is critical when the list contains 1M+ entries.
fn draw_entry_name_list(frame: &mut Frame, area: Rect, params: &NameListParams<'_>, state: &mut ratatui::widgets::ListState) {
    debug_assert_eq!(
        params.filtered_indices.len(),
        params.cached_lines.len(),
        "filtered_indices and cached_lines must be the same length for section '{}'",
        params.section_label,
    );

    let t = theme();
    let total_items = params.cached_lines.len();
    // Inner height excludes the top and bottom border rows.
    let inner_height = area.height.saturating_sub(2) as usize;

    let selected_position = state.selected().map_or(0, |s| s + 1);
    let title = format!(" {selected_position}/{total_items} {} ({} total) ", params.section_label, params.total_count);
    let block = panel_block(params.border_style).title(title);

    if total_items == 0 || inner_height == 0 {
        frame.render_widget(block, area);
        return;
    }

    // Clamp selected within bounds (mirrors what List does internally).
    if state.selected().is_some_and(|sel| sel >= total_items) {
        state.select(Some(total_items.saturating_sub(1)));
    }

    // --- Compute the visible window, replicating List's scroll-to-selection. ---
    let selected = state.selected().unwrap_or(0);
    let mut offset = state.offset();

    if selected < offset {
        offset = selected;
    } else if selected >= offset + inner_height {
        offset = selected - inner_height + 1;
    }
    // Clamp so the window never extends past the end of the list.
    offset = offset.min(total_items.saturating_sub(inner_height));

    // Persist the computed offset back into the real state so that
    // subsequent frames / input handlers see a consistent value.
    *state.offset_mut() = offset;

    // Build ListItems for only the visible slice, zebra-striped on the
    // absolute index so stripes stay stable while scrolling.
    let window_end = (offset + inner_height).min(total_items);
    let visible_lines = &params.cached_lines[offset..window_end];
    let items: Vec<ListItem> =
        visible_lines.iter().enumerate().map(|(i, line)| ListItem::new(line.clone()).style(zebra_style(offset + i))).collect();

    // Temporary state mapped to the slice coordinate space.
    let mut slice_state = ratatui::widgets::ListState::default().with_offset(0).with_selected(state.selected().map(|s| s - offset));

    let list = List::new(items)
        .block(block)
        .highlight_style(Style::default().bg(t.highlight_bg).add_modifier(Modifier::BOLD))
        .highlight_symbol("\u{25b6} ");

    frame.render_stateful_widget(list, area, &mut slice_state);

    // Mark rows whose name is wider than the pane with a trailing ellipsis —
    // ratatui clips them silently otherwise. The highlight symbol ("▶ ")
    // shifts every row's content right by 2 columns.
    let inner_width = area.width.saturating_sub(2) as usize; // borders
    let usable = inner_width.saturating_sub(2); // highlight symbol gutter
    if usable > 1 {
        let buf = frame.buffer_mut();
        for (i, line) in visible_lines.iter().enumerate() {
            if line.width() > usable {
                #[allow(clippy::cast_possible_truncation)] // bounded by area width
                let x = area.x + 1 + inner_width as u16 - 1;
                let y = area.y + 1 + i as u16;
                if let Some(cell) = buf.cell_mut((x, y)) {
                    cell.set_symbol("\u{2026}").set_fg(t.muted);
                }
            }
        }
    }

    // Scrollbar — uses real position within the full list.
    if total_items > inner_height {
        let mut scrollbar_state = ScrollbarState::new(total_items).position(selected);
        frame.render_stateful_widget(panel_scrollbar(), area, &mut scrollbar_state);
    }
}

/// Render an empty detail panel with a hint message.
pub fn draw_empty_detail(frame: &mut Frame, area: Rect, message: &str, border_style: Style) {
    // A zero-sized area is an environmental condition (shrunken terminal), not a
    // programming error: drawing into it is a no-op.
    if area.width == 0 || area.height == 0 {
        return;
    }
    let t = theme();
    let block = panel_block(border_style).title(" Detail ");
    let paragraph = Paragraph::new(Line::from(Span::styled(format!("  {message}"), Style::default().fg(t.muted)))).block(block);
    frame.render_widget(paragraph, area);
}
