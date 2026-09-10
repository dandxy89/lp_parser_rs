//! Bottom status bar widget.
//!
//! Displays total change count, per-section diff statistics, active filter, and key hints.
//! Layout is responsive: the left segment flows to fit its content and the
//! key hints stay right-aligned, so long filter/tolerance labels no longer
//! overflow a fixed-width column silently.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::diff_model::DiffCounts;
use crate::theme::theme;

/// Optional detail scroll position for the status bar.
pub struct DetailPosition {
    pub scroll: u16,
    pub content_lines: usize,
}

/// Optional yank flash state for the status bar.
pub struct YankFlash<'a> {
    pub message: &'a str,
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

/// Muted separator between status bar segments.
const SEPARATOR: &str = "  \u{2502}  ";

/// Separator between key hints in the right-hand segment.
const HINT_SEPARATOR: &str = " \u{b7} ";

/// Fraction of the status bar the key hints may occupy before pairs are
/// dropped: two thirds, leaving a third for the counts on the left.
const HINT_WIDTH_NUMERATOR: u16 = 2;
const HINT_WIDTH_DENOMINATOR: u16 = 3;

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

    let t = theme();
    let key_style = Style::default().fg(t.muted).add_modifier(Modifier::BOLD);
    let label_style = Style::default().fg(t.muted);
    let mut spans = Vec::with_capacity(kept.len() * 3);
    for (i, pair) in kept.iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled(HINT_SEPARATOR, Style::default().fg(t.border)));
        }
        match pair.split_once(':') {
            Some((key, action)) => {
                spans.push(Span::styled(key, key_style));
                spans.push(Span::styled(":", label_style));
                spans.push(Span::styled(action, label_style));
            }
            // No colon: a plain fragment such as the `y →` chord prefix.
            None => spans.push(Span::styled(*pair, label_style)),
        }
    }
    (spans, width)
}

/// Assemble segments into one line, separated by [`SEPARATOR`], keeping only
/// those that fit in `available` columns.
///
/// Segments are dropped whole from the end rather than clipped: half a label
/// (`filter:` with its value cut off) reads as a bug, where a missing segment
/// just reads as a narrow terminal.
fn fit_segments<'a>(segments: Vec<Vec<Span<'a>>>, separator: &Span<'a>, available: usize) -> Vec<Span<'a>> {
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
    spans
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

    // Right: yank flash or key hints, right-aligned in its own chunk so the
    // left segments can flow (and drop) independently.
    let (mut right_spans, right_width) = params.yank_flash.map_or_else(
        || hint_spans(params.hints, (area.width * HINT_WIDTH_NUMERATOR / HINT_WIDTH_DENOMINATOR) as usize),
        |flash| {
            (vec![Span::styled(flash.message, Style::default().fg(t.added).add_modifier(Modifier::BOLD))], flash.message.chars().count())
        },
    );
    // A leading space keeps a visible gap between the two halves.
    right_spans.insert(0, Span::raw(" "));

    #[allow(clippy::cast_possible_truncation)] // hint strings are far below u16::MAX
    let right_len = (right_width as u16).saturating_add(2).min(area.width);
    let chunks = Layout::horizontal([Constraint::Min(0), Constraint::Length(right_len)]).split(area);

    // Left: one group per fact. Inspect mode shows the filename and section
    // count; diff mode shows total/per-kind change counts and the active filter.
    let mut segments: Vec<Vec<Span<'_>>> = Vec::with_capacity(8);
    if let Some(inspect) = &params.inspect {
        segments.push(vec![Span::styled(format!(" {}", inspect.file), Style::default().fg(t.accent).add_modifier(Modifier::BOLD))]);
        segments.push(vec![Span::styled(format!("{} {}", inspect.entry_count, inspect.section_label), Style::default().fg(t.text))]);
    } else {
        segments.push(vec![Span::styled(
            format!(" {} changes", params.total_changes),
            Style::default().fg(t.added).add_modifier(Modifier::BOLD),
        )]);
        segments.push(vec![
            Span::styled(format!("+{}", params.section_counts.added), Style::default().fg(t.added)),
            Span::raw(" "),
            Span::styled(format!("-{}", params.section_counts.removed), Style::default().fg(t.removed)),
            Span::raw(" "),
            Span::styled(format!("~{}", params.section_counts.modified), Style::default().fg(t.modified)),
            Span::raw(" "),
            Span::styled(format!(">{}", params.section_counts.renamed), Style::default().fg(t.accent)),
        ]);
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
        let top_line = (position.scroll as usize).min(position.content_lines) + 1;
        segments.push(vec![Span::styled(format!("L{top_line}/{}", position.content_lines), Style::default().fg(t.accent))]);
    }

    let left_spans = fit_segments(segments, &separator, chunks[0].width as usize);
    frame.render_widget(Paragraph::new(Line::from(left_spans)), chunks[0]);
    frame.render_widget(Paragraph::new(Line::from(right_spans)), chunks[1]);
}
