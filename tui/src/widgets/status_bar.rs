//! Bottom status bar widget.
//!
//! Displays total change count, per-section diff statistics, active filter, and key hints.
//! The left segment is sized to its content and the key hints stay
//! right-aligned, so a long filter or tolerance label pushes the hints over
//! rather than being cut off by a fixed-width column.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::diff_model::DiffCounts;
use crate::theme::theme;

/// Optional detail scroll position for the status bar.
pub struct DetailPosition {
    pub scroll: usize,
    pub content_lines: usize,
}

/// Optional flash message for the status bar.
pub struct YankFlash<'a> {
    pub message: &'a str,
    /// Severity, which picks the colour.
    pub level: crate::app::FlashLevel,
}

/// Colour of a flash at `level`: errors in the removal red, warnings in the
/// modification amber, successes in the addition green, and neutral feedback
/// in the accent — so a failure never reads as a green success.
fn flash_style(level: crate::app::FlashLevel) -> Style {
    use crate::app::FlashLevel;
    let t = theme();
    let colour = match level {
        FlashLevel::Info => t.accent,
        FlashLevel::Ok => t.added,
        FlashLevel::Warn => t.modified,
        FlashLevel::Err => t.removed,
    };
    Style::default().fg(colour).add_modifier(Modifier::BOLD)
}

/// Inspect-mode left segment: the single filename and current section counts.
///
/// When present, it replaces the diff-oriented change/filter segment entirely so
/// no "N changes" / "+N -N ~N" / "filter" text appears in inspect mode.
pub struct InspectInfo<'a> {
    pub file: &'a str,
    /// The section's noun, already pluralised for `entry_count`.
    pub section_label: &'a str,
    pub entry_count: usize,
}

/// Parameters for rendering the status bar.
pub struct StatusBarParams<'a> {
    pub total_changes: usize,
    pub section_counts: &'a DiffCounts,
    pub filter_label: &'a str,
    pub filter_count: usize,
    pub detail_position: Option<&'a DetailPosition>,
    pub yank_flash: Option<&'a YankFlash<'a>>,
    pub ignore_order: bool,
    /// Sort mode indicator (e.g. "sort:|Δ|"). `None` for the default name sort.
    pub sort_label: Option<&'a str>,
    /// Active non-zero tolerances (e.g. "abs:1e-6 rel:1e-9"). `None` when both are zero.
    pub tolerance_label: Option<&'a str>,
    /// Watch mode: `None` when not watching, `Some(reloading)` when active.
    pub watch_reloading: Option<bool>,
    /// Inspect-mode left segment. `None` in diff mode (the default segment shows).
    pub inspect: Option<InspectInfo<'a>>,
    /// Context-sensitive key hints shown on the right (chosen by the caller
    /// from focus/section/mode so the most relevant actions are advertised).
    pub hints: &'a str,
}

/// Prefix of the renamed count, here and in the tab bar.
pub const RENAMED: &str = "\u{21c4}";

/// Muted separator between status bar segments.
const SEPARATOR: &str = "  \u{2502}  ";

use crate::widgets::HINT_SEPARATOR;

/// Split the packed hint string (`"S:solve  w:csv"`) into styled spans, and
/// return their total display width.
///
/// The key is what the eye is hunting for, so it carries the weight and the
/// action recedes; a middot between pairs separates them more quietly than the
/// run of spaces it replaces.
///
/// Pairs that do not fit within `max_width` are dropped rather than allowed to
/// squeeze the counts on the left off the bar. The first pairs are the
/// section-specific ones, so they are kept; the last pair is always kept too,
/// because it is `?:help` — the way back to everything that was dropped.
fn hint_spans(hints: &str, max_width: usize) -> (Vec<Span<'_>>, usize) {
    let separator_width = HINT_SEPARATOR.chars().count();
    let pairs: Vec<&str> = hints.split("  ").filter(|pair| !pair.is_empty()).collect();
    let Some((last, leading)) = pairs.split_last() else {
        return (Vec::new(), 0);
    };
    // Room the final pair needs, so the greedy fill below never spends it.
    let reserved = last.chars().count() + separator_width;

    let mut kept: Vec<&str> = Vec::with_capacity(pairs.len());
    let mut width = 0;
    for pair in leading {
        let separator = if kept.is_empty() { 0 } else { separator_width };
        let next = width + separator + pair.chars().count();
        if next + reserved > max_width {
            break;
        }
        kept.push(pair);
        width = next;
    }
    if !kept.is_empty() {
        width += separator_width;
    }
    width += last.chars().count();
    kept.push(last);

    (crate::widgets::key_hint_spans(&kept), width)
}

/// Assemble segments into one line, separated by [`SEPARATOR`], keeping only
/// those that fit in `available` columns.
///
/// Segments are dropped whole from the end rather than clipped: half a label
/// (`filter:` with its value cut off) reads as a bug, where a missing segment
/// just reads as a narrow terminal.
fn fit_segments<'a>(segments: Vec<Vec<Span<'a>>>, separator: &Span<'a>, available: usize) -> (Vec<Span<'a>>, usize) {
    let separator_width = separator.content.chars().count();
    let mut spans: Vec<Span<'a>> = Vec::with_capacity(segments.len() * 3);
    let mut width = 0;
    for segment in segments {
        let segment_width: usize = segment.iter().map(|span| span.content.chars().count()).sum();
        let gap = if spans.is_empty() { 0 } else { separator_width };
        if width + gap + segment_width > available {
            break;
        }
        if gap > 0 {
            spans.push(separator.clone());
        }
        spans.extend(segment);
        width += gap + segment_width;
    }
    (spans, width)
}

/// Draw the status bar across the given area.
pub fn draw_status_bar(frame: &mut Frame, area: Rect, params: &StatusBarParams<'_>) {
    // A zero-sized area is an environmental condition (shrunken terminal), not a
    // programming error: drawing into it is a no-op.
    if area.width == 0 || area.height == 0 {
        return;
    }

    let t = theme();
    let separator = Span::styled(SEPARATOR, Style::default().fg(t.border));

    // Left: one group per fact. Inspect mode shows the filename and section
    // count; diff mode shows total/per-kind change counts and the active filter.
    let mut segments: Vec<Vec<Span<'_>>> = Vec::with_capacity(8);
    if let Some(inspect) = &params.inspect {
        segments.push(vec![Span::styled(format!(" {}", inspect.file), Style::default().fg(t.accent).add_modifier(Modifier::BOLD))]);
        segments.push(vec![Span::styled(format!("{} {}", inspect.entry_count, inspect.section_label), Style::default().fg(t.text))]);
    } else {
        // No changes is a quiet fact, not a headline.
        let changes_style = if params.total_changes == 0 {
            Style::default().fg(t.muted)
        } else {
            Style::default().fg(t.added).add_modifier(Modifier::BOLD)
        };
        segments.push(vec![Span::styled(format!(" {}", crate::format::plural(params.total_changes, "change", "changes")), changes_style)]);
        let counts = params.section_counts;
        let mut kinds = vec![
            Span::styled(format!("+{}", counts.added), Style::default().fg(t.added)),
            Span::raw(" "),
            Span::styled(format!("-{}", counts.removed), Style::default().fg(t.removed)),
            Span::raw(" "),
            Span::styled(format!("~{}", counts.modified), Style::default().fg(t.modified)),
        ];
        // Renames only happen under rename rules or detection; a permanent
        // `>0` was noise, and `>` read as a comparison.
        if counts.renamed > 0 {
            kinds.push(Span::raw(" "));
            kinds.push(Span::styled(format!("{RENAMED}{}", counts.renamed), Style::default().fg(t.accent)));
        }
        segments.push(kinds);
        segments.push(vec![
            Span::styled("filter:", Style::default().fg(t.muted)),
            Span::styled(format!("{} ({})", params.filter_label, params.filter_count), Style::default().fg(t.modified)),
        ]);
    }
    if params.ignore_order {
        segments.push(vec![Span::styled("ignoring order", Style::default().fg(t.modified))]);
    }
    if let Some(sort_label) = params.sort_label {
        segments.push(vec![Span::styled(sort_label.to_owned(), Style::default().fg(t.accent))]);
    }
    if let Some(tolerance_label) = params.tolerance_label {
        segments.push(vec![Span::styled(tolerance_label.to_owned(), Style::default().fg(t.accent))]);
    }
    if let Some(reloading) = params.watch_reloading {
        segments.push(vec![if reloading {
            Span::styled("\u{25cf} reloading\u{2026}", Style::default().fg(t.modified).add_modifier(Modifier::BOLD))
        } else {
            Span::styled("\u{25cf} watch", Style::default().fg(t.secondary_accent))
        }]);
    }
    if let Some(position) = params.detail_position
        && position.content_lines > 0
    {
        let top_line = position.scroll.min(position.content_lines) + 1;
        segments.push(vec![Span::styled(format!("L{top_line}/{}", position.content_lines), Style::default().fg(t.accent))]);
    }

    // The state on the left outranks the hints on the right: the left
    // segments take what they need, less the room for the one hint that is
    // never dropped (`?:help`), and the hints fill whatever is left over.
    // A flash is transient and is the thing the user is waiting to read, so
    // it keeps its full width instead.
    let available = area.width as usize;
    let right_floor = params.yank_flash.map_or_else(|| last_hint_width(params.hints), |flash| flash.message.chars().count()) + 2;
    let (left_spans, left_width) = fit_segments(segments, &separator, available.saturating_sub(right_floor));

    let (mut right_spans, right_width) = params.yank_flash.map_or_else(
        || hint_spans(params.hints, available.saturating_sub(left_width + 2)),
        |flash| (vec![Span::styled(flash.message, flash_style(flash.level))], flash.message.chars().count()),
    );
    // A leading space keeps a visible gap between the two halves.
    right_spans.insert(0, Span::raw(" "));

    #[allow(clippy::cast_possible_truncation)] // hint strings are far below u16::MAX
    let right_len = (right_width as u16).saturating_add(2).min(area.width);
    let chunks = Layout::horizontal([Constraint::Min(0), Constraint::Length(right_len)]).split(area);
    frame.render_widget(Paragraph::new(Line::from(left_spans)), chunks[0]);
    frame.render_widget(Paragraph::new(Line::from(right_spans)), chunks[1]);
}

/// Width of the last hint pair — `?:help`, which is always kept.
fn last_hint_width(hints: &str) -> usize {
    hints.split("  ").filter(|pair| !pair.is_empty()).last().map_or(0, |pair| pair.chars().count())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Render a diff-mode status bar with the given counts; returns its text
    /// and the cell under the first character of the change count.
    fn render_counts(total_changes: usize, renamed: usize) -> (String, ratatui::buffer::Cell) {
        let counts = DiffCounts { renamed, ..DiffCounts::default() };
        let params = StatusBarParams {
            total_changes,
            section_counts: &counts,
            filter_label: "All",
            filter_count: 0,
            detail_position: None,
            yank_flash: None,
            ignore_order: false,
            sort_label: None,
            tolerance_label: None,
            watch_reloading: None,
            inspect: None,
            hints: "?:help",
        };
        let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(80, 1)).expect("test terminal");
        terminal.draw(|frame| draw_status_bar(frame, frame.area(), &params)).expect("draws");
        let buffer = terminal.backend().buffer().clone();
        let text = buffer.content().iter().map(ratatui::buffer::Cell::symbol).collect();
        (text, buffer[(1, 0)].clone())
    }

    #[test]
    fn no_changes_is_quiet_and_renames_show_only_when_present() {
        let (text, cell) = render_counts(0, 0);
        assert!(text.contains("0 changes"), "{text:?}");
        assert_eq!(cell.fg, theme().muted, "no changes is muted");
        assert!(!text.contains(RENAMED), "no renames, no rename count: {text:?}");

        let (text, cell) = render_counts(1, 2);
        assert!(text.contains("1 change "), "singular for one: {text:?}");
        assert_eq!(cell.fg, theme().added);
        assert!(text.contains("\u{21c4}2"), "renames carry the swap arrow: {text:?}");
    }

    #[test]
    fn hints_shrink_to_help_before_the_state_is_dropped() {
        let hints = "E:what-if  r:raw  s:sort  ?:help";
        assert_eq!(last_hint_width(hints), "?:help".len());
        let (_, width) = hint_spans(hints, 6);
        assert_eq!(width, 6, "only ?:help survives a tight budget");
        let (_, width) = hint_spans(hints, 18);
        assert_eq!(width, "E:what-if · ?:help".chars().count(), "the leading pairs come back as room allows");
    }
}
