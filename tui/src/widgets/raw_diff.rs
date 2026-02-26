//! Side-by-side raw text diff widget.
//!
//! Shows the actual LP file text for the currently selected entry,
//! one column per file, with simple line-level colour highlighting.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::theme::theme;

/// Render a side-by-side raw text diff into `area`.
///
/// Returns the maximum line count across both columns (used for scroll clamping).
pub fn draw_raw_diff(
    frame: &mut Frame,
    area: Rect,
    old_text: Option<&str>,
    new_text: Option<&str>,
    scroll: u16,
    border_style: Style,
) -> usize {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(border_style)
        .title(Span::styled(" Raw Text (r to toggle) ", Style::default().fg(theme().accent).add_modifier(Modifier::BOLD)));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.width < 5 || inner.height < 1 {
        return 0;
    }

    // Split into two columns with a 1-char divider.
    let chunks = Layout::horizontal([Constraint::Percentage(50), Constraint::Length(1), Constraint::Percentage(50)]).split(inner);

    let left_lines = build_column_lines(old_text, "File 1");
    let right_lines = build_column_lines(new_text, "File 2");

    let max_lines = left_lines.len().max(right_lines.len());

    // Render left column.
    let left_para = Paragraph::new(left_lines).scroll((scroll, 0));
    frame.render_widget(left_para, chunks[0]);

    // Render divider.
    render_divider(frame, chunks[1], inner.height);

    // Render right column.
    let right_para = Paragraph::new(right_lines).scroll((scroll, 0));
    frame.render_widget(right_para, chunks[2]);

    max_lines
}

/// Build styled lines for a single column.
fn build_column_lines<'a>(text: Option<&str>, label: &str) -> Vec<Line<'a>> {
    let t = theme();

    match text {
        Some(content) if !content.trim().is_empty() => {
            let header_style = Style::default().fg(t.accent).add_modifier(Modifier::BOLD);
            let text_style = Style::default().fg(t.text);

            let mut lines = vec![Line::from(Span::styled(format!(" {label}"), header_style)), Line::default()];
            for line in content.lines() {
                lines.push(Line::from(Span::styled(line.to_owned(), text_style)));
            }
            lines
        }
        _ => {
            let muted_style = Style::default().fg(t.muted);
            vec![
                Line::from(Span::styled(format!(" {label}"), muted_style)),
                Line::default(),
                Line::from(Span::styled(" [Not present in this file]", muted_style)),
            ]
        }
    }
}

/// Render a vertical divider line.
fn render_divider(frame: &mut Frame, area: Rect, height: u16) {
    let muted_style = Style::default().fg(theme().muted);
    let lines: Vec<Line<'_>> = (0..height).map(|_| Line::from(Span::styled("\u{2502}", muted_style))).collect();
    let para = Paragraph::new(lines);
    frame.render_widget(para, area);
}

/// Extract the raw LP text for an entry given its byte_offset in the source text.
///
/// Scans forward from `byte_offset` until a section keyword or the next named
/// entry (line starting with `identifier:`) is found, returning the slice.
pub fn extract_entry_text(raw_text: &str, byte_offset: usize) -> &str {
    debug_assert!(byte_offset <= raw_text.len(), "byte_offset {byte_offset} exceeds raw_text length {}", raw_text.len());

    if byte_offset >= raw_text.len() {
        return "";
    }

    let rest = &raw_text[byte_offset..];
    let end = find_next_entry_boundary(rest);
    rest[..end].trim()
}

/// Find the byte offset of the next entry or section boundary in `text`.
///
/// An entry ends when we encounter:
/// - A section keyword at the start of a line (case-insensitive):
///   `subject to`, `s.t.`, `st`, `bounds`, `binary`, `binaries`, `generals`,
///   `integers`, `semi-continuous`, `sos`, `end`, `minimize`, `maximize`, etc.
/// - A line that looks like a named entry: `identifier:` (but not `::` for SOS).
/// - EOF
fn find_next_entry_boundary(text: &str) -> usize {
    let mut offset = 0;
    let mut first_line = true;

    for line in text.lines() {
        // Skip the first line (it's the start of the current entry).
        if first_line {
            first_line = false;
            offset += line.len() + 1; // +1 for newline
            continue;
        }

        let trimmed = line.trim();
        if trimmed.is_empty() {
            offset += line.len() + 1;
            continue;
        }

        // Check for section keywords (case-insensitive).
        let lower = trimmed.to_ascii_lowercase();
        if is_section_keyword(&lower) {
            return offset;
        }

        // Check for named entry: identifier followed by `:` (but not `::` for SOS).
        if looks_like_named_entry(trimmed) {
            return offset;
        }

        offset += line.len() + 1;
    }

    text.len()
}

/// Check if a lowercased, trimmed line starts with a section keyword.
fn is_section_keyword(lower: &str) -> bool {
    // Split on whitespace and check the first word, or check multi-word patterns.
    let first_word = lower.split_whitespace().next().unwrap_or("");

    matches!(
        first_word,
        "subject"
            | "such"
            | "s.t."
            | "st"
            | "st:"
            | "bounds"
            | "bound"
            | "binary"
            | "binaries"
            | "bin"
            | "generals"
            | "general"
            | "gen"
            | "integers"
            | "integer"
            | "semi-continuous"
            | "semis"
            | "semi"
            | "sos"
            | "end"
            | "minimize"
            | "minimise"
            | "minimum"
            | "min"
            | "maximize"
            | "maximise"
            | "maximum"
            | "max"
    )
}

/// Check if a line looks like a named constraint/objective entry (`name: ...`).
///
/// Heuristic: the line contains `:` (not `::`) and the part before `:` looks
/// like a single identifier (no spaces, no operators).
fn looks_like_named_entry(line: &str) -> bool {
    if let Some(colon_pos) = line.find(':') {
        // Exclude `::` (SOS weight separator).
        if line[colon_pos..].starts_with("::") {
            return false;
        }
        let before = line[..colon_pos].trim();
        // Must be a single token (no spaces) and non-empty.
        !before.is_empty() && !before.contains(' ') && !before.contains('\t')
    } else {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_entry_text_simple() {
        let text = "minimize\nobj: x1 + x2\nsubject to\nc1: x1 <= 10\nend\n";
        // "obj" starts at some offset; let's find it.
        let obj_offset = text.find("obj:").unwrap();
        let extracted = extract_entry_text(text, obj_offset);
        assert_eq!(extracted, "obj: x1 + x2");
    }

    #[test]
    fn test_extract_entry_text_constraint() {
        let text = "subject to\nc1: x1 + x2 <= 10\nc2: x3 >= 5\nend\n";
        let c1_offset = text.find("c1:").unwrap();
        let extracted = extract_entry_text(text, c1_offset);
        assert_eq!(extracted, "c1: x1 + x2 <= 10");
    }

    #[test]
    fn test_extract_entry_text_last_before_section() {
        let text = "subject to\nc1: x1 <= 10\nbounds\n0 <= x1 <= 5\nend\n";
        let c1_offset = text.find("c1:").unwrap();
        let extracted = extract_entry_text(text, c1_offset);
        assert_eq!(extracted, "c1: x1 <= 10");
    }

    #[test]
    fn test_is_section_keyword() {
        assert!(is_section_keyword("bounds"));
        assert!(is_section_keyword("end"));
        assert!(is_section_keyword("subject to"));
        assert!(is_section_keyword("minimize"));
        assert!(!is_section_keyword("c1: x1 <= 10"));
        assert!(!is_section_keyword("x1"));
    }

    #[test]
    fn test_looks_like_named_entry() {
        assert!(looks_like_named_entry("c1: x1 <= 10"));
        assert!(looks_like_named_entry("obj: x1 + x2"));
        assert!(!looks_like_named_entry("x1 + x2 <= 10"));
        // `V1:1` looks like a named entry (single token before colon); this is an
        // acceptable false positive — SOS weights like this appear inside SOS entries
        // rather than at line boundaries the scanner encounters.
        assert!(looks_like_named_entry("V1:1"));
        // Double colon (SOS separator) should not match.
        assert!(!looks_like_named_entry("V1::1"));
    }
}
