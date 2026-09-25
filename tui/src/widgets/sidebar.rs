//! The section tab bar, the name list, and the placeholders the detail panel
//! shows when nothing is selected.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{HighlightSpacing, List, ListItem, Paragraph, ScrollbarState};

use crate::app::{App, Focus, Section};
use crate::theme::theme;
use crate::widgets::{SELECTION_CURSOR, focus_border_style, panel_block, selection_style, truncate_middle, zebra_style};

/// Draw the section tab bar across the top of the frame.
///
/// Renders ` 1 Summary │ 2 Variables (n) │ … │ 5 Numerics ` and records each tab's
/// column range in `app.layout.tab_bounds` for mouse hit-testing.
pub fn draw_tab_bar(frame: &mut Frame, area: Rect, app: &mut App) {
    // A zero-sized area is an environmental condition (shrunken terminal), not a
    // programming error: drawing into it is a no-op.
    if area.width == 0 || area.height == 0 {
        return;
    }
    let t = theme();
    let focused = app.focus == Focus::SectionSelector;
    let active_index = app.active_section.index();

    // Five short labels: cheap enough to build per draw, and a draw only
    // happens on input, resize, or an animation tick.
    let labels = crate::app::build_section_labels(&app.cached_summary, app.mode, app.filter, app.numerics_badge);

    // Each tab's spans at the chosen density. The coloured per-kind change
    // counts keep their own colours regardless of tab state — they are
    // information, not chrome.
    let tab_spans = |index: usize, density: TabDensity| -> Vec<Span<'_>> {
        let label = &labels[index];
        let active = index == active_index;
        let style = if active {
            let base = Style::default().fg(t.accent).add_modifier(Modifier::BOLD | Modifier::UNDERLINED);
            if focused { base.bg(t.selection_bg) } else { base }
        } else if focused {
            Style::default().fg(t.text)
        } else {
            Style::default().fg(t.muted)
        };
        let name = if density == TabDensity::Full { label.name.as_ref() } else { label.short.as_ref() };
        // The digit that jumps to the tab, so `1`–`5` need no memorising.
        let digit = Span::styled(TAB_DIGITS[index], if active { style } else { Style::default().fg(t.muted) });
        let mut spans = vec![digit, Span::styled(name, style)];
        let show_counts = density != TabDensity::ActiveCounts || active;
        if show_counts && !label.counts.is_empty() {
            spans.push(Span::raw(" "));
            // Borrow the cached span's content rather than cloning its String.
            spans.extend(label.counts.iter().map(|count| Span::styled(count.content.as_ref(), count.style)));
        }
        spans
    };

    let available = area.width as usize;
    let mut chosen: Option<Vec<Vec<Span<'_>>>> = None;
    for density in [TabDensity::Full, TabDensity::Short, TabDensity::ActiveCounts] {
        let tabs: Vec<Vec<Span<'_>>> = (0..labels.len()).map(|index| tab_spans(index, density)).collect();
        if tab_bar_width(tabs.iter().map(|tab| spans_width(tab))) <= available {
            chosen = Some(tabs);
            break;
        }
    }
    // Still too wide: show a run of tabs around the active one, marking the
    // hidden ends with an ellipsis, so the active tab is never the one lost.
    let (tabs, visible) = if let Some(tabs) = chosen {
        let count = tabs.len();
        (tabs, 0..count)
    } else {
        let tabs: Vec<Vec<Span<'_>>> = (0..labels.len()).map(|index| tab_spans(index, TabDensity::ActiveCounts)).collect();
        let widths: Vec<usize> = tabs.iter().map(|tab| spans_width(tab)).collect();
        let visible = visible_tab_window(&widths, active_index, available);
        (tabs, visible)
    };

    let mut spans: Vec<Span<'_>> = Vec::with_capacity(16);
    let mut bounds = [(0_u16, 0_u16); 5];
    spans.push(Span::raw(if visible.start > 0 { "\u{2026}" } else { " " }));
    let mut x = area.x.saturating_add(1);
    let last = visible.end;
    for (index, tab) in tabs.into_iter().enumerate() {
        if !visible.contains(&index) {
            continue;
        }
        if x > area.x.saturating_add(1) {
            spans.push(Span::styled(" \u{2502} ", Style::default().fg(t.border)));
            x = x.saturating_add(3);
        }
        #[allow(clippy::cast_possible_truncation)] // labels are short, far below u16::MAX
        let width = spans_width(&tab) as u16;
        bounds[index] = (x, x.saturating_add(width));
        x = x.saturating_add(width);
        spans.extend(tab);
    }
    if last < labels.len() {
        spans.push(Span::styled(" \u{2026}", Style::default().fg(t.muted)));
    }

    app.layout.tab_bounds = bounds;
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

/// Each tab's shortcut digit, drawn before its name.
const TAB_DIGITS: [&str; 5] = ["1 ", "2 ", "3 ", "4 ", "5 "];

/// How much of each tab label the tab bar draws, from most to least.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TabDensity {
    /// Full section names and every tab's change counts.
    Full,
    /// Abbreviated names, every tab's change counts.
    Short,
    /// Abbreviated names, change counts on the active tab only.
    ActiveCounts,
}

/// Display width of a run of spans.
fn spans_width(spans: &[Span<'_>]) -> usize {
    spans.iter().map(|span| span.content.chars().count()).sum()
}

/// Width of the whole tab bar for tabs of the given widths: a leading space,
/// then the tabs separated by ` │ `.
fn tab_bar_width(widths: impl Iterator<Item = usize>) -> usize {
    let (count, sum) = widths.fold((0_usize, 0_usize), |(count, sum), width| (count + 1, sum + width));
    1 + sum + 3 * count.saturating_sub(1)
}

/// The run of tabs to draw when not all of them fit in `available` columns:
/// the active tab, grown one neighbour at a time (right first) while the run
/// still fits with room for the ellipsis markers at either end.
fn visible_tab_window(widths: &[usize], active: usize, available: usize) -> std::ops::Range<usize> {
    /// The ` …` marker after the run; the one before it replaces the leading space.
    const MARKER: usize = 2;
    debug_assert!(active < widths.len(), "active tab {active} out of range");
    let fits = |range: &std::ops::Range<usize>| tab_bar_width(widths[range.clone()].iter().copied()) + MARKER <= available;
    let mut range = active..active + 1;
    loop {
        let right = range.start..range.end + 1;
        let left = range.start.saturating_sub(1)..range.end;
        if range.end < widths.len() && fits(&right) {
            range = right;
        } else if range.start > 0 && fits(&left) {
            range = left;
        } else {
            return range;
        }
    }
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
            let inner_width = area.width.saturating_sub(2) as usize;
            // Each row is a way into its section: it shows the digit that
            // jumps there, and a click on it does the same.
            let items: Vec<ListItem> = OVERVIEW_SECTIONS
                .into_iter()
                .map(|section| {
                    let count = match section {
                        Section::Variables => counts.variables.changed(),
                        Section::Constraints => counts.constraints.changed(),
                        _ => counts.objectives.changed(),
                    };
                    ListItem::new(overview_row(section, count, inner_width))
                })
                .collect();

            let block = panel_block(Style::default().fg(t.border)).title(" Overview ");

            let list = List::new(items).block(block);
            frame.render_widget(list, area);
        }
        section @ (Section::Variables | Section::Constraints | Section::Objectives) => {
            draw_section_entry_list(frame, area, app, section, border_style);
        }
    }
}

/// The list sections the Overview rows lead to, top to bottom. A click on
/// row `i` of the Overview opens `OVERVIEW_SECTIONS[i]`.
pub const OVERVIEW_SECTIONS: [Section; 3] = [Section::Variables, Section::Constraints, Section::Objectives];

/// One Overview row: the section's shortcut digit and label on the left and
/// its change count right-aligned to `width`, so a large count is never
/// clipped. The label falls back to its short form when the full one would
/// not leave a gap.
fn overview_row(section: Section, count: usize, width: usize) -> Line<'static> {
    /// Indent, then the digit and a space.
    const PREFIX: usize = 2 + 2;
    let t = theme();
    let count = count.to_string();
    let label = if PREFIX + section.label().chars().count() + 1 + count.len() <= width { section.label() } else { section.short_label() };
    let pad = width.saturating_sub(PREFIX + count.len()).max(label.chars().count() + 1);
    Line::from(vec![
        Span::raw("  "),
        Span::styled(format!("{} ", section.index() + 1), Style::default().fg(t.accent)),
        Span::styled(format!("{label:<pad$}"), Style::default().fg(t.text)),
        Span::styled(count, Style::default().fg(t.muted)),
    ])
}

/// Resolve the per-section label and total count, then render the entry name list.
///
/// Collapses the near-identical match arms in `draw_name_list` into one place.
/// Must not be called for static sections (Summary, Numerics), which have no entry list.
fn draw_section_entry_list(frame: &mut Frame, area: Rect, app: &mut App, section: Section, border_style: Style) {
    debug_assert!(section.list_index().is_some(), "draw_section_entry_list called for static section {section:?}");
    let focused = app.focus == Focus::NameList;
    let (section_label, total_count) = match section {
        Section::Variables => ("variables", app.report.variables.counts.total()),
        Section::Constraints => ("constraints", app.report.constraints.counts.total()),
        Section::Objectives => ("objectives", app.report.objectives.counts.total()),
        Section::Summary | Section::Numerics => return,
    };
    let idx = section.list_index().expect("list section has a list_index");
    let sort_label = app.sort_mode.label();
    let (filtered, cached_lines, state) = app.section_states[idx].indices_lines_and_state_mut();
    draw_entry_name_list(
        frame,
        area,
        &NameListParams { filtered_indices: filtered, cached_lines, section_label, total_count, border_style, sort_label, focused },
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
    /// Active sort indicator (e.g. "sort:|Δ|"). `None` for the default name sort.
    pub sort_label: Option<&'a str>,
    /// Whether the sidebar holds focus — the selected row is tinted when it
    /// does and neutral when it does not, so only the live list looks live.
    pub focused: bool,
}

/// Draw a compact name list for a section's entries in the sidebar.
///
/// Uses virtualised rendering: only `ListItem`s for the visible window are
/// allocated, keeping the per-frame cost at `O(visible_height)` instead of
/// `O(total_items)`.  This is critical when the list contains 1M+ entries.
fn draw_entry_name_list(frame: &mut Frame, area: Rect, params: &NameListParams<'_>, state: &mut ratatui::widgets::ListState) {
    debug_assert_eq!(
        params.filtered_indices.len(),
        params.cached_lines.len(),
        "filtered_indices and cached_lines must be the same length for section '{}'",
        params.section_label,
    );

    let total_items = params.cached_lines.len();
    // Inner height excludes the top and bottom border rows.
    let inner_height = area.height.saturating_sub(2) as usize;

    let selected_position = state.selected().map_or(0, |s| s + 1);
    let sort = params.sort_label.map(|label| format!("\u{b7} {label} ")).unwrap_or_default();
    // The sidebar is a fifth of the width, so the full title rarely fits. Drop
    // whole segments rather than let the block clip mid-word.
    let inner_width = area.width.saturating_sub(2) as usize;
    let title = [
        format!(" {selected_position}/{total_items} {} ({} total) {sort}", params.section_label, params.total_count),
        format!(" {selected_position}/{total_items} {} ", params.section_label),
        format!(" {selected_position}/{total_items} "),
    ]
    .into_iter()
    .find(|candidate| candidate.chars().count() <= inner_width)
    .unwrap_or_default();
    let block = panel_block(params.border_style).title(title);

    if total_items == 0 || inner_height == 0 {
        frame.render_widget(block, area);
        return;
    }

    // Clamp selected within bounds (mirrors what List does internally).
    if state.selected().is_some_and(|sel| sel >= total_items) {
        state.select(Some(total_items.saturating_sub(1)));
    }

    // Compute the visible window, replicating List's scroll-to-selection.
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

    // The scrollbar runs down the last column inside the border, not on it:
    // the right border is the divider the detail panel shares, and a thumb
    // drawn there reads as a break in that divider.
    let inner = block.inner(area);
    let scrollable = total_items > inner_height && inner.width > 1;
    let list_area = if scrollable { Rect { width: inner.width - 1, ..inner } } else { inner };
    // Room for a row's text: the list less the selection cursor's gutter.
    let usable = (list_area.width as usize).saturating_sub(SELECTION_CURSOR.chars().count());

    // Build ListItems for only the visible slice, zebra-striped on the
    // absolute index so stripes stay stable while scrolling. Names too wide
    // for the pane lose their middle, not their end, so rows that share a
    // long prefix stay distinguishable.
    let window_end = (offset + inner_height).min(total_items);
    let visible_lines = &params.cached_lines[offset..window_end];
    let items: Vec<ListItem> =
        visible_lines.iter().enumerate().map(|(i, line)| ListItem::new(fit_row(line, usable)).style(zebra_style(offset + i))).collect();

    // Temporary state mapped to the slice coordinate space.
    let mut slice_state = ratatui::widgets::ListState::default().with_offset(0).with_selected(state.selected().map(|s| s - offset));

    // Always reserve the cursor gutter, so rows do not jump two columns
    // sideways when the selection comes and goes.
    let list = List::new(items)
        .highlight_style(selection_style(params.focused))
        .highlight_symbol(SELECTION_CURSOR)
        .highlight_spacing(HighlightSpacing::Always);

    frame.render_widget(block, area);
    frame.render_stateful_widget(list, list_area, &mut slice_state);

    // Scrollbar — uses real position within the full list.
    if scrollable {
        let mut scrollbar_state = ScrollbarState::new(total_items).position(selected);
        let track = Rect { x: inner.right() - 1, width: 1, ..inner };
        frame.render_stateful_widget(crate::widgets::panel_scrollbar(), track, &mut scrollbar_state);
    }
}

/// Fit a cached sidebar row into `width` columns. The entry name is always the
/// row's last span (after the badge and any delta column); when the row is too
/// wide, only the name is shortened, from the middle.
fn fit_row(line: &Line<'static>, width: usize) -> Line<'static> {
    if line.width() <= width {
        return line.clone();
    }
    let Some((name, prefix)) = line.spans.split_last() else {
        return line.clone();
    };
    let prefix_width: usize = prefix.iter().map(Span::width).sum();
    let name_width = width.saturating_sub(prefix_width).max(1);
    let mut spans = prefix.to_vec();
    spans.push(Span::styled(truncate_middle(&name.content, name_width).into_owned(), name.style));
    Line::from(spans)
}

/// Render an empty detail panel with only a hint message (used by the search
/// pop-up preview, where a cheat sheet would be noise).
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

/// Render an empty detail panel with a hint message and a mini cheat sheet —
/// the blank half-screen is the best place to teach the section's actions.
pub fn draw_empty_detail_cheatsheet(frame: &mut Frame, area: Rect, message: &str, border_style: Style, diff_mode: bool) {
    // A zero-sized area is an environmental condition (shrunken terminal), not a
    // programming error: drawing into it is a no-op.
    if area.width == 0 || area.height == 0 {
        return;
    }
    let t = theme();
    let muted = Style::default().fg(t.muted);
    let key = Style::default().fg(t.accent);
    let hint = |keys: &'static str, action: &'static str| {
        Line::from(vec![Span::styled(format!("  {keys:<12}"), key), Span::styled(action, muted)])
    };

    let mut lines = vec![Line::from(Span::styled(format!("  {message}"), Style::default().fg(t.text))), Line::default()];
    lines.push(hint("j/k Enter", "navigate the list, open detail"));
    lines.push(hint("/", "search all sections"));
    lines.push(hint("E", "what-if: edit constraint RHS & re-solve"));
    if diff_mode {
        lines.push(hint("r", "toggle raw text side-by-side view"));
        lines.push(hint("+/-/m/=/a", "filter by change kind"));
        lines.push(hint("s", "sort by delta magnitude"));
        lines.push(hint("yy/yo/yn", "yank name / old side / new side"));
    } else {
        lines.push(hint("yy", "yank entry name"));
    }
    lines.push(hint("S", "solve with HiGHS"));
    lines.push(hint("w", "export CSV"));
    lines.push(Line::default());
    lines.push(hint("Ctrl-p ?", "command palette, full keybindings"));

    let block = panel_block(border_style).title(" Detail ");
    frame.render_widget(Paragraph::new(lines).block(block), area);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tab_window_always_holds_the_active_tab() {
        let widths = [10, 10, 10, 10, 10];
        // Everything fits: 1 + 50 + 12 separators.
        assert_eq!(visible_tab_window(&widths, 2, 80), 0..5);
        assert_eq!(visible_tab_window(&widths, 4, 30), 3..5, "grows left when the right is exhausted");
        assert_eq!(visible_tab_window(&widths, 0, 30), 0..2);
        assert_eq!(visible_tab_window(&widths, 3, 5), 3..4, "the active tab stays even when nothing else fits");
    }

    #[test]
    fn overview_counts_are_right_aligned_and_never_clipped() {
        let text = |section, count, width| crate::widgets::plain(&[overview_row(section, count, width)]).trim_end_matches('\n').to_owned();
        assert_eq!(text(Section::Variables, 590, 18), "  2 Variables  590");
        assert_eq!(text(Section::Constraints, 12_345, 18), "  3 Cons     12345", "a label that leaves no gap is shortened");
        assert_eq!(text(Section::Objectives, 7, 18).chars().count(), 18);
    }
}
