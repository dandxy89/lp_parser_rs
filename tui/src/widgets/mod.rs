//! Custom widgets for the TUI application.

use std::borrow::Cow;
use std::time::Duration;

use lp_parser_rs::analysis::IssueSeverity;
use ratatui::layout::{Constraint, Flex, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Scrollbar, ScrollbarOrientation};

use crate::diff_model::DiffKind;
use crate::state::Focus;
use crate::theme::theme;

/// Subdued text for labels, hints, and unchanged values.
pub fn muted() -> Style {
    Style::new().fg(theme().muted)
}

/// Default body text.
pub fn text() -> Style {
    Style::new().fg(theme().text)
}

/// Bold body text for emphasis.
pub fn bold_text() -> Style {
    text().add_modifier(Modifier::BOLD)
}

/// Style for the sidebar delta column shown under the delta sorts.
pub fn delta_column() -> Style {
    Style::new().fg(theme().accent)
}

/// Arrow separator used between old → new values.
pub const ARROW: &str = "  \u{2192}  ";

/// Flatten rendered lines to plain text: concatenate each line's span contents
/// and join with newlines. This is how the clipboard yank gets its text — the
/// widgets are the single source of truth for layout, and stripping the styles
/// is the whole of the "plain" rendering.
pub fn plain(lines: &[ratatui::text::Line<'_>]) -> String {
    let mut out = String::new();
    for line in lines {
        for span in &line.spans {
            out.push_str(&span.content);
        }
        out.push('\n');
    }
    out
}

/// Return the border [`Style`] for a panel, highlighted when `current == target`.
pub fn focus_border_style(current: Focus, target: Focus) -> Style {
    if current == target {
        Style::default().fg(theme().border_focus).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme().border)
    }
}

/// Standard rounded panel block used by every bordered widget.
///
/// A bold border style marks the panel holding focus (see
/// [`focus_border_style`]); on a palette with no colour to spare
/// ([`Theme::focus_thick`](crate::theme::Theme::focus_thick)) that border is
/// drawn with heavy lines instead.
pub fn panel_block(border_style: Style) -> Block<'static> {
    let focused = border_style.add_modifier.contains(Modifier::BOLD);
    let block = Block::default().borders(Borders::ALL).border_type(border_type(focused, theme().focus_thick)).border_style(border_style);
    // A focused panel's title takes its border's colour too; spans that carry
    // their own colour (a diff badge) keep it.
    if focused { block.title_style(border_style) } else { block }
}

/// Line style of a panel border: heavy for the focused panel when the palette
/// marks focus by weight rather than colour, rounded otherwise.
const fn border_type(focused: bool, focus_thick: bool) -> BorderType {
    if focused && focus_thick { BorderType::Thick } else { BorderType::Rounded }
}

/// Standard vertical scrollbar: a hairline track and a hairline thumb, no end
/// caps. The position is the information; the arrows were chrome, the
/// double-line track competed with the panel border beside it, and a full
/// block thumb read as a hole punched through that border.
pub fn panel_scrollbar() -> Scrollbar<'static> {
    let t = theme();
    Scrollbar::new(ScrollbarOrientation::VerticalRight)
        .track_symbol(Some("\u{2502}"))
        .track_style(Style::new().fg(t.border))
        .thumb_symbol("\u{2503}")
        .thumb_style(Style::new().fg(t.muted))
        .begin_symbol(None)
        .end_symbol(None)
}

/// Render [`panel_scrollbar`] down the right edge of a bordered panel.
///
/// `area` is the panel's outer rect, borders included; the track is inset by a
/// row top and bottom so it runs beside the border instead of eating its
/// corners.
pub fn render_panel_scrollbar(frame: &mut ratatui::Frame, area: Rect, state: &mut ratatui::widgets::ScrollbarState) {
    if area.height <= 2 {
        return;
    }
    let track = Rect { y: area.y + 1, height: area.height - 2, ..area };
    frame.render_stateful_widget(panel_scrollbar(), track, state);
}

/// Separator between `key:action` hint pairs.
pub const HINT_SEPARATOR: &str = " \u{b7} ";

/// Render `key:action` hint pairs in the one style every hint in the app
/// uses: the key — what the eye is hunting for — in the accent, the action
/// receding in the muted colour, and a quiet middot between pairs. A pair
/// without a colon (the `y →` chord prefix) is drawn muted as it stands.
pub fn key_hint_spans<'a>(pairs: &[&'a str]) -> Vec<Span<'a>> {
    let t = theme();
    let key_style = Style::default().fg(t.accent);
    let label_style = Style::default().fg(t.muted);
    let mut spans = Vec::with_capacity(pairs.len() * 3);
    for (i, pair) in pairs.iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled(HINT_SEPARATOR, Style::default().fg(t.border)));
        }
        match pair.split_once(':') {
            Some((key, action)) => {
                spans.push(Span::styled(key, key_style));
                spans.push(Span::styled(":", label_style));
                spans.push(Span::styled(action, label_style));
            }
            None => spans.push(Span::styled(*pair, label_style)),
        }
    }
    spans
}

/// Split packed hints (`"j/k:scroll  Esc:close"`, pairs two spaces apart)
/// into pairs for [`key_hint_spans`].
pub fn hint_pairs(packed: &str) -> Vec<&str> {
    packed.split("  ").filter(|pair| !pair.is_empty()).collect()
}

/// Display width of packed hints once rendered.
pub fn key_hint_width(packed: &str) -> usize {
    let pairs = hint_pairs(packed);
    let separators = pairs.len().saturating_sub(1) * HINT_SEPARATOR.chars().count();
    pairs.iter().map(|pair| pair.chars().count()).sum::<usize>() + separators
}

/// A panel title — the name, then its key hints in parentheses — in the
/// shared hint style.
pub fn title_with_hints(name: &str, packed: &'static str, style: Style) -> Line<'static> {
    let mut spans = vec![Span::styled(format!(" {name}  ("), style)];
    spans.extend(key_hint_spans(&hint_pairs(packed)));
    spans.push(Span::styled(") ", style));
    Line::from(spans)
}

/// Draw packed key hints onto a panel's bottom border, indented past the
/// corner.
///
/// The hint is chrome for the pane it belongs to; sitting on the border keeps
/// it out of the body, which is what the reader is actually there for. Silently
/// skipped when the panel is too narrow to hold it.
pub fn draw_footer_hint(frame: &mut ratatui::Frame, panel: Rect, packed: &str) {
    // A space either side keeps the hint clear of the border rule.
    let width = u16::try_from(key_hint_width(packed) + 2).unwrap_or(u16::MAX);
    if panel.width <= width.saturating_add(4) || panel.height == 0 {
        return;
    }
    let area = Rect { x: panel.x + 2, y: panel.bottom() - 1, width, height: 1 };
    let mut spans = vec![Span::raw(" ")];
    spans.extend(key_hint_spans(&hint_pairs(packed)));
    spans.push(Span::raw(" "));
    frame.render_widget(ratatui::widgets::Paragraph::new(Line::from(spans)), area);
}

/// Dim everything already drawn in `area` so a modal overlay reads as the layer
/// in focus.
///
/// Foregrounds drop to the border colour and backgrounds reset, which flattens
/// the zebra stripes and selection tints underneath: the content stays as
/// texture without competing with the overlay for attention. `DIM` carries the
/// same intent on the monochrome palette, where every colour is the terminal
/// default.
pub fn draw_scrim(frame: &mut ratatui::Frame, area: Rect) {
    let colour = theme().border;
    let buffer = frame.buffer_mut();
    for y in area.top()..area.bottom() {
        for x in area.left()..area.right() {
            if let Some(cell) = buffer.cell_mut((x, y)) {
                cell.set_fg(colour).set_bg(Color::Reset);
                // Bold and underline behind the modal would survive the colour
                // flattening and keep drawing the eye, so they go too.
                cell.modifier = Modifier::DIM;
            }
        }
    }
}

/// Overwrite one cell with a box-drawing junction, so two rules that meet read
/// as joined rather than as one crossing the other. A position outside the
/// buffer is ignored — junction stitching is cosmetic, never load-bearing.
pub fn draw_junction(frame: &mut ratatui::Frame, position: (u16, u16), symbol: &'static str, colour: Color) {
    if let Some(cell) = frame.buffer_mut().cell_mut(position) {
        cell.set_symbol(symbol).set_fg(colour);
    }
}

/// A full-width hairline rule, drawn as a block's top border. Used to separate
/// stacked regions inside one panel, where a second panel would be a nested box.
pub fn separator_rule(border_style: Style) -> Block<'static> {
    Block::default().borders(Borders::TOP).border_style(border_style)
}

/// The row cursor: a solid bar in the gutter rather than an arrowhead, so the
/// selected row reads as a marked edge instead of a pointer aimed at the text.
/// Two columns wide, matching the `"  "` gutter of unselected rows.
pub const SELECTION_CURSOR: &str = "\u{258d} ";

/// Style for the selected row of a list. The focused pane carries the tinted
/// selection; an unfocused pane keeps its selection visible but neutral, so
/// only one list at a time looks live.
pub fn selection_style(focused: bool) -> Style {
    let t = theme();
    Style::new().bg(if focused { t.selection_bg } else { t.selection_bg_dim }).add_modifier(Modifier::BOLD)
}

/// Background style for alternating rows, keyed on the absolute item index so
/// stripes stay stable while scrolling.
pub fn zebra_style(absolute_index: usize) -> Style {
    if absolute_index % 2 == 1 { Style::default().bg(theme().zebra_bg) } else { Style::default() }
}

/// Braille spinner frames, advanced every 100ms of elapsed time.
const SPINNER_FRAMES: [&str; 10] =
    ["\u{280b}", "\u{2819}", "\u{2839}", "\u{2838}", "\u{283c}", "\u{2834}", "\u{2826}", "\u{2827}", "\u{2807}", "\u{280f}"];

/// Pick the spinner frame for an elapsed duration. Deriving the frame from
/// elapsed time means callers need no extra animation state — the 50ms event
/// tick redraws often enough to animate.
pub const fn spinner_frame(elapsed: Duration) -> &'static str {
    SPINNER_FRAMES[(elapsed.as_millis() / 100) as usize % SPINNER_FRAMES.len()]
}

/// Truncate `name` to at most `max_width` display columns, appending `…` when
/// it does not fit. Names in LP files are ASCII in practice, but truncation is
/// performed on a char boundary so multibyte input cannot panic.
pub fn truncate_with_ellipsis(name: &str, max_width: usize) -> Cow<'_, str> {
    debug_assert!(max_width >= 2, "truncate_with_ellipsis needs room for at least one char plus ellipsis");
    if name.chars().count() <= max_width || max_width < 2 {
        return Cow::Borrowed(name);
    }
    let mut truncated: String = name.chars().take(max_width - 1).collect();
    truncated.push('\u{2026}');
    Cow::Owned(truncated)
}

/// Shorten `name` to at most `max_width` display columns by cutting out its
/// middle and marking the cut with `…`.
///
/// Model names tend to share long prefixes and differ at the end
/// (`Steel_Flow_Conservation_in_Node_Chicago`, `…_Node_Gary`), so cutting the
/// end off leaves a column of identical rows. Keeping both ends — the tail
/// given the odd column — keeps them apart. Width is measured in terminal
/// columns and cuts fall on char boundaries, so wide or multibyte names
/// neither overflow nor panic.
pub fn truncate_middle(name: &str, max_width: usize) -> Cow<'_, str> {
    use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};
    if name.width() <= max_width {
        return Cow::Borrowed(name);
    }
    if max_width <= 1 {
        return Cow::Owned("\u{2026}".repeat(max_width));
    }
    let budget = max_width - 1;
    let (head_budget, tail_budget) = (budget / 2, budget - budget / 2);
    let take = |chars: &mut dyn Iterator<Item = char>, budget: usize| {
        let mut used = 0;
        let mut taken = Vec::new();
        for c in chars {
            let width = c.width().unwrap_or(0);
            if used + width > budget {
                break;
            }
            used += width;
            taken.push(c);
        }
        taken
    };
    let head = take(&mut name.chars(), head_budget);
    let mut tail = take(&mut name.chars().rev(), tail_budget);
    tail.reverse();
    let mut out: String = head.into_iter().collect();
    out.push('\u{2026}');
    out.extend(tail);
    Cow::Owned(out)
}

/// Format `value` in at most `width` columns, rounding rather than clipping.
///
/// The shortest exact representation is used when it fits. Otherwise the
/// value is rounded to fewer decimal places, and when even the integer part
/// (or a tiny magnitude) will not fit in fixed notation, to scientific
/// notation with fewer mantissa digits. A number cut off mid-digit reads as a
/// different number, so when nothing fits the shortest scientific form is
/// returned wider than asked for — the caller's column then overflows rather
/// than lies.
pub fn fit_number(value: f64, width: usize) -> String {
    let exact = format!("{value}");
    if exact.chars().count() <= width || !value.is_finite() {
        return exact;
    }
    // Fixed notation keeps the integer digits, so it is only worth trying for
    // magnitudes where it still shows a significant digit.
    let magnitude = value.abs();
    if (1e-4..1e15).contains(&magnitude) {
        for decimals in (0..=15).rev() {
            let fixed = format!("{value:.decimals$}");
            let fixed = if fixed.contains('.') { fixed.trim_end_matches('0').trim_end_matches('.').to_owned() } else { fixed };
            if fixed.chars().count() <= width && fixed.trim_start_matches('-') != "0" {
                return fixed;
            }
        }
    }
    let mut shortest = exact;
    for digits in (0..=15).rev() {
        let scientific = format!("{value:.digits$e}");
        if scientific.chars().count() <= width {
            return scientific;
        }
        shortest = scientific;
    }
    shortest
}

/// Return a `─` rule of the given display width (clamped to 120 columns).
pub fn rule_str(width: usize) -> String {
    "\u{2500}".repeat(width.min(120))
}

/// Column at which a section heading's trailing rule stops.
///
/// Sized so the rule ends inside the detail panel of an 80-column terminal,
/// with a gutter before the border and the scrollbar, instead of running under
/// them. Wider panes then read it as a measure rather than a full-bleed
/// divider; narrower ones clip it, which is the graceful direction to fail.
const HEADING_WIDTH: usize = 54;

/// A section heading: the title in the accent colour, run out to a common
/// column with a hairline rule, so every heading in every pane shares one
/// spine. This replaces the older two-line treatment (title, then an underline
/// matched to the title's width) — same structure, half the vertical cost, and
/// headings no longer step in and out with the length of their own text.
pub fn heading_line(title: &str) -> Line<'static> {
    let t = theme();
    let used = title.chars().count() + 3;
    Line::from(vec![
        Span::styled(format!("  {title} "), Style::default().fg(t.accent).add_modifier(Modifier::BOLD)),
        Span::styled(rule_str(HEADING_WIDTH.saturating_sub(used)), Style::default().fg(t.border)),
    ])
}

/// A muted note line, indented to the pane's text column.
///
/// Used where a pane opens with a one-line explanation: the border title
/// already names the pane, so a heading there would only say it twice.
pub fn note_line(note: &str) -> Line<'static> {
    Line::from(Span::styled(format!("  {note}"), Style::default().fg(theme().muted)))
}

/// Push a section heading into `lines`, preceded by a blank spacer unless it
/// opens the pane, and followed by `note` when one is given.
pub fn push_heading(lines: &mut Vec<Line<'static>>, title: &str, note: &str) {
    if !lines.is_empty() {
        lines.push(Line::from(""));
    }
    lines.push(heading_line(title));
    if !note.is_empty() {
        lines.push(Line::from(Span::styled(format!("  {note}"), Style::default().fg(theme().muted))));
    }
}

/// The rectangle for a report overlay holding `content_lines` lines: the
/// whole width, and only as tall as the content (plus borders) needs, centred
/// in the screen less a row top and bottom — so the tab bar and the status
/// bar, which carries the pane's keys, stay readable behind it. A short report
/// no longer sits in a screenful of empty box. Insetting the sides as well
/// left a two-column sliver of the panel underneath showing through, which
/// read as a second, broken border.
pub fn report_rect(area: Rect, content_lines: usize) -> Rect {
    if area.height <= 2 {
        return area;
    }
    let available = area.height - 2;
    let wanted = u16::try_from(content_lines.saturating_add(2)).unwrap_or(u16::MAX);
    let height = wanted.min(available);
    Rect { x: area.x, y: area.y + 1 + (available - height) / 2, width: area.width, height }
}

/// Draw a scrollable report pane: `lines` inside `block` at `popup`, from
/// `scroll`, with the shared scrollbar down the right edge when the content
/// is taller than the pane.
pub fn draw_scroll_pane(frame: &mut ratatui::Frame, popup: Rect, lines: &[Line<'static>], scroll: u16, block: Block<'static>) {
    frame.render_widget(ratatui::widgets::Clear, popup);
    let inner_height = popup.height.saturating_sub(2) as usize;
    // Only the rows in view are cloned: the lines never wrap, so row `scroll`
    // of the content is `lines[scroll]`.
    let first = usize::from(scroll).min(lines.len());
    let window = &lines[first..(first + inner_height).min(lines.len())];
    frame.render_widget(ratatui::widgets::Paragraph::new(window.to_vec()).block(block), popup);
    if lines.len() > inner_height {
        let max_scroll = lines.len() - inner_height;
        let mut state = ratatui::widgets::ScrollbarState::new(max_scroll + 1).position(scroll as usize);
        render_panel_scrollbar(frame, popup, &mut state);
    }
}

/// Build an inline gauge bar like `▐███░░░░░▌` for a fraction in `[0, 1]`.
///
/// `cells` is the number of fill cells between the end caps.
pub fn gauge_bar(fraction: f64, cells: usize) -> String {
    debug_assert!(cells > 0, "gauge_bar needs at least one cell");
    let clamped = fraction.clamp(0.0, 1.0);
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss, clippy::cast_precision_loss)] // clamped to [0, cells]
    let filled = ((clamped * cells as f64).round() as usize).min(cells);
    let mut bar = String::with_capacity(2 + cells * 3);
    bar.push('\u{2590}');
    for _ in 0..filled {
        bar.push('\u{2588}');
    }
    for _ in filled..cells {
        bar.push('\u{2591}');
    }
    bar.push('\u{258c}');
    bar
}

pub mod analysis;
pub mod detail;
pub mod diagnostics;
pub mod help;
pub mod highs_query;
pub mod numerics;
pub mod palette;
pub mod presolve;
pub mod profile;
pub mod raw_diff;
pub mod search_popup;
pub mod sidebar;
pub mod solve;
pub mod status_bar;
pub mod summary;
pub mod what_if;

/// Map a [`DiffKind`] to its theme-aware display colour.
pub fn kind_colour(kind: DiffKind) -> Color {
    let t = theme();
    match kind {
        DiffKind::Added => t.added,
        DiffKind::Removed => t.removed,
        DiffKind::Modified => t.modified,
        DiffKind::Renamed => t.accent,
    }
}

/// Return a [`Style`] with the foreground set to the colour for `kind`.
pub fn kind_style(kind: DiffKind) -> Style {
    Style::default().fg(kind_colour(kind))
}

/// Return a fixed-width prefix glyph for the given [`DiffKind`].
pub const fn kind_prefix(kind: DiffKind) -> &'static str {
    match kind {
        DiffKind::Added => "[+]",
        DiffKind::Removed => "[-]",
        DiffKind::Modified => "[~]",
        DiffKind::Renamed => "[>]",
    }
}

/// Map an [`IssueSeverity`] to its theme-aware colour.
pub fn severity_colour(severity: IssueSeverity) -> Color {
    let t = theme();
    match severity {
        IssueSeverity::Error => t.removed,
        IssueSeverity::Warning => t.modified,
        IssueSeverity::Info => t.accent,
    }
}

/// Extract the filename from a path string for compact display.
pub fn short_filename(path: &str) -> String {
    std::path::Path::new(path).file_name().map_or_else(|| path.to_owned(), |name| name.to_string_lossy().into_owned())
}

/// Render a `" > "`-prompted single-line text input into `area`, placing the
/// real terminal cursor at the edit position and scrolling horizontally so the
/// cursor stays visible on long queries.
pub fn draw_prompt_input(frame: &mut ratatui::Frame, area: Rect, input: &tui_input::Input) {
    const PROMPT: &str = " > ";
    if area.width == 0 || area.height == 0 {
        return;
    }
    let t = theme();
    #[allow(clippy::cast_possible_truncation)] // PROMPT is 3 columns
    let prompt_width = (PROMPT.len() as u16).min(area.width);
    let prompt_area = Rect { width: prompt_width, ..area };
    frame.render_widget(
        ratatui::widgets::Paragraph::new(Span::styled(PROMPT, Style::default().fg(t.accent).add_modifier(Modifier::BOLD))),
        prompt_area,
    );

    let text_area = Rect { x: area.x + prompt_width, width: area.width - prompt_width, ..area };
    if text_area.width == 0 {
        return;
    }
    // Keep one column free so the cursor can sit past the last character.
    let scroll = input.visual_scroll(text_area.width.saturating_sub(1) as usize);
    #[allow(clippy::cast_possible_truncation)] // scroll is bounded by the query width
    let paragraph = ratatui::widgets::Paragraph::new(Span::styled(input.value(), Style::default().fg(t.text))).scroll((0, scroll as u16));
    frame.render_widget(paragraph, text_area);

    #[allow(clippy::cast_possible_truncation)] // cursor offset is bounded by text_area.width
    let cursor_x = text_area.x + (input.visual_cursor().saturating_sub(scroll)) as u16;
    frame.set_cursor_position((cursor_x.min(text_area.right().saturating_sub(1)), text_area.y));
}

/// Compute a centred rectangle of the given dimensions, clamped to the terminal area.
pub fn centred_rect(area: Rect, width: u16, height: u16) -> Rect {
    // Snap to the full extent when the margin would be a sliver on each side
    // (two columns, or a row): the panel border underneath showing past the
    // overlay's border reads as a second, broken border rather than as
    // breathing room.
    let width = if width + 6 > area.width { area.width } else { width };
    let height = if height + 4 > area.height { area.height } else { height };

    let vertical = Layout::vertical([Constraint::Length(height)]).flex(Flex::Center).split(area);
    let horizontal = Layout::horizontal([Constraint::Length(width)]).flex(Flex::Center).split(vertical[0]);

    horizontal[0]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The windowed scroll pane must draw exactly what scrolling a paragraph
    /// of every line would.
    #[test]
    fn the_windowed_scroll_pane_matches_the_full_paragraph() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let lines: Vec<Line<'static>> = (0..100).map(|i| Line::from(format!("line {i}"))).collect();
        let popup = Rect::new(0, 0, 30, 12);
        let draw = |f: &dyn Fn(&mut ratatui::Frame)| {
            let mut terminal = Terminal::new(TestBackend::new(popup.width, popup.height)).expect("test terminal must build");
            terminal.draw(|frame| f(frame)).expect("draw must succeed");
            terminal.backend().buffer().clone()
        };
        for scroll in [0_u16, 1, 50, 90, 99, 100, 150] {
            let windowed = draw(&|frame| draw_scroll_pane(frame, popup, &lines, scroll, Block::bordered()));
            let full = draw(&|frame| {
                frame.render_widget(ratatui::widgets::Paragraph::new(lines.clone()).block(Block::bordered()).scroll((scroll, 0)), popup);
                let mut state = ratatui::widgets::ScrollbarState::new(lines.len() - 10 + 1).position(usize::from(scroll));
                render_panel_scrollbar(frame, popup, &mut state);
            });
            assert_eq!(windowed, full, "scroll pane differs at scroll {scroll}");
        }
    }

    #[test]
    fn test_truncate_fits_unchanged() {
        assert_eq!(truncate_with_ellipsis("short", 10), "short");
        assert_eq!(truncate_with_ellipsis("exact", 5), "exact");
    }

    #[test]
    fn test_truncate_overflow_gets_ellipsis() {
        assert_eq!(truncate_with_ellipsis("overflowing", 6), "overf\u{2026}");
        assert_eq!(truncate_with_ellipsis("ab", 2), "ab");
        assert_eq!(truncate_with_ellipsis("abc", 2), "a\u{2026}");
    }

    #[test]
    fn test_truncate_multibyte_safe() {
        // 4 chars, max 3 → 2 chars + ellipsis, no panic on char boundaries.
        assert_eq!(truncate_with_ellipsis("\u{0394}\u{0394}\u{0394}\u{0394}", 3), "\u{0394}\u{0394}\u{2026}");
    }

    #[test]
    fn focus_is_marked_by_colour_or_by_line_weight() {
        assert_eq!(border_type(true, true), BorderType::Thick, "monochrome marks focus with heavy lines");
        assert_eq!(border_type(false, true), BorderType::Rounded);
        assert_eq!(border_type(true, false), BorderType::Rounded, "colour palettes keep the rounded border");
        let focused = focus_border_style(Focus::Detail, Focus::Detail);
        assert_eq!(focused.fg, Some(theme().accent), "a focused border takes the accent");
    }

    #[test]
    fn key_hints_put_keys_in_the_accent() {
        let spans = key_hint_spans(&hint_pairs("j/k:scroll  Esc:close"));
        let text: String = spans.iter().map(|span| span.content.as_ref()).collect();
        assert_eq!(text, "j/k:scroll \u{b7} Esc:close");
        assert_eq!(key_hint_width("j/k:scroll  Esc:close"), text.chars().count());
        let keys: Vec<&str> = spans.iter().filter(|span| span.style.fg == Some(theme().accent)).map(|span| span.content.as_ref()).collect();
        assert_eq!(keys, ["j/k", "Esc"], "only the keys take the accent");
    }

    #[test]
    fn truncate_middle_keeps_both_ends() {
        assert_eq!(truncate_middle("short", 10), "short");
        assert_eq!(truncate_middle("exact", 5), "exact");
        let chicago = truncate_middle("Steel_Flow_Conservation_in_Node_Chicago", 16);
        let gary = truncate_middle("Steel_Flow_Conservation_in_Node_Gary", 16);
        assert_eq!(chicago, "Steel_F\u{2026}_Chicago");
        assert_ne!(chicago, gary, "names differing at the end must stay distinguishable");
        assert_eq!(truncate_middle("abcdef", 1), "\u{2026}");
        assert_eq!(truncate_middle("abcdef", 0), "");
        assert_eq!(truncate_middle("abcdef", 2), "\u{2026}f");
    }

    #[test]
    fn truncate_middle_measures_columns_not_bytes() {
        // Four two-byte chars, one column each.
        assert_eq!(truncate_middle("\u{0394}\u{0394}\u{0394}\u{0394}", 3), "\u{0394}\u{2026}\u{0394}");
        // Wide CJK chars take two columns each: 5 columns fit "漢…字" (2 + 1 + 2).
        let wide = truncate_middle("\u{6f22}\u{5b57}\u{6f22}\u{5b57}", 5);
        assert_eq!(wide, "\u{6f22}\u{2026}\u{5b57}");
        assert!(unicode_width::UnicodeWidthStr::width(wide.as_ref()) <= 5);
    }

    #[test]
    fn report_panes_are_as_tall_as_their_content() {
        let area = Rect::new(0, 0, 120, 40);
        let short = report_rect(area, 10);
        assert_eq!(short.height, 12, "ten lines and two borders");
        assert_eq!(short.width, 120, "always the full width");
        assert_eq!(short.y, 1 + (38 - 12) / 2, "centred between the tab and status bars");
        assert_eq!(report_rect(area, 500), Rect::new(0, 1, 120, 38), "a long report fills the screen and scrolls");
    }

    #[test]
    fn centred_rect_never_leaves_a_sliver_beside_the_popup() {
        let area = Rect::new(0, 0, 64, 24);
        assert_eq!(centred_rect(area, 60, 10).width, 64, "a two-column margin snaps to the full width");
        assert_eq!(centred_rect(Rect::new(0, 0, 80, 24), 60, 10).width, 60, "a real margin is kept");
    }

    #[test]
    fn fit_number_rounds_instead_of_clipping() {
        assert_eq!(fit_number(41.19926, 10), "41.19926", "fits as is");
        assert_eq!(fit_number(41.19926, 5), "41.2", "rounded, never clipped to 41.19");
        assert_eq!(fit_number(41.19926, 2), "41");
        assert_eq!(fit_number(-0.123_456_789, 6), "-0.123");
        assert_eq!(fit_number(123_456_789.0, 6), "1.23e8", "integer digits that do not fit go scientific");
        assert_eq!(fit_number(0.000_012_345, 7), "1.23e-5", "tiny magnitudes never round to 0");
        // Nothing fits: the shortest honest form, wider than asked.
        assert_eq!(fit_number(123_456_789.0, 2), "1e8");
    }

    #[test]
    fn test_gauge_bar_boundaries() {
        assert_eq!(gauge_bar(0.0, 4), "\u{2590}\u{2591}\u{2591}\u{2591}\u{2591}\u{258c}");
        assert_eq!(gauge_bar(1.0, 4), "\u{2590}\u{2588}\u{2588}\u{2588}\u{2588}\u{258c}");
        assert_eq!(gauge_bar(0.5, 4), "\u{2590}\u{2588}\u{2588}\u{2591}\u{2591}\u{258c}");
        // Out-of-range fractions are clamped, never panic.
        assert_eq!(gauge_bar(-1.0, 4), gauge_bar(0.0, 4));
        assert_eq!(gauge_bar(2.0, 4), gauge_bar(1.0, 4));
    }

    #[test]
    fn test_rule_str_widths() {
        assert_eq!(rule_str(0), "");
        assert_eq!(rule_str(3), "───");
        assert_eq!(rule_str(3).chars().count(), 3);
        // Clamped to the backing constant's width.
        assert_eq!(rule_str(999).chars().count(), 120);
    }

    #[test]
    fn test_spinner_frame_cycles() {
        assert_eq!(spinner_frame(Duration::from_millis(0)), SPINNER_FRAMES[0]);
        assert_eq!(spinner_frame(Duration::from_millis(150)), SPINNER_FRAMES[1]);
        assert_eq!(spinner_frame(Duration::from_secs(1)), SPINNER_FRAMES[0]);
    }
}
