//! Full-screen source view for the editor (`--features editor`).

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph};
use unicode_width::UnicodeWidthChar;

use crate::app::App;
use crate::editor::{Editor, Mode, Popup};
use crate::theme::theme;

/// The part of `text` between display columns `skip` and `skip + width`.
fn slice_columns(text: &str, skip: usize, width: usize) -> String {
    let mut column = 0;
    let mut out = String::new();
    for ch in text.chars() {
        let w = ch.width().unwrap_or(0);
        if column >= skip && column + w <= skip + width {
            out.push(if ch == '\t' { ' ' } else { ch });
        }
        column += w;
        if column >= skip + width {
            break;
        }
    }
    out
}

pub fn draw_source(frame: &mut Frame, area: Rect, app: &mut App) {
    let Some(editor) = app.editor.as_deref_mut() else { return };
    if area.width < 10 || area.height < 3 {
        return;
    }
    let t = theme();
    frame.render_widget(Clear, area);
    let rows = usize::from(area.height) - 1;
    editor.height = rows;
    editor.scroll_to_cursor(rows);

    let gutter = editor.line_count().to_string().len() + 2;
    let text_width = usize::from(area.width).saturating_sub(gutter + 1).max(1);
    let cursor_column = editor.cursor_column();
    let hscroll = (cursor_column + 1).saturating_sub(text_width);
    let cursor_line = editor.cursor_line();

    let mut lines = Vec::with_capacity(rows);
    for row in 0..rows {
        let line = editor.top + row;
        if line >= editor.line_count() {
            lines.push(Line::from(Span::styled("~", Style::default().fg(t.faint))));
            continue;
        }
        let marker = match editor.line_severity(line) {
            Some(severity) if severity == ls_types::DiagnosticSeverity::ERROR => Span::styled("E", Style::default().fg(t.removed).add_modifier(Modifier::BOLD)),
            Some(severity) if severity == ls_types::DiagnosticSeverity::WARNING => Span::styled("W", Style::default().fg(t.modified).add_modifier(Modifier::BOLD)),
            Some(_) => Span::styled("i", Style::default().fg(t.info)),
            None => Span::raw(" "),
        };
        let number = Span::styled(format!("{:>width$} ", line + 1, width = gutter - 1), Style::default().fg(if line == cursor_line { t.text } else { t.faint }));
        let source = editor.line_text(line);
        let style = if source.trim_start().starts_with('\\') { Style::default().fg(t.muted) } else { Style::default().fg(t.text) };
        let mut text = Span::styled(slice_columns(source, hscroll, text_width), style);
        if line == cursor_line {
            text.style = text.style.bg(t.selection_bg_dim);
        }
        lines.push(Line::from(vec![marker, number, text]));
    }
    frame.render_widget(Paragraph::new(lines), Rect { height: area.height - 1, ..area });

    draw_status(frame, Rect { y: area.y + area.height - 1, height: 1, ..area }, editor);

    if editor.popup.is_some() {
        draw_popup(frame, area, editor);
    } else if matches!(editor.mode, Mode::Normal | Mode::Insert) {
        #[expect(clippy::cast_possible_truncation)] // both are bounded by the terminal size
        let (x, y) = (area.x + (gutter + cursor_column - hscroll) as u16, area.y + (cursor_line - editor.top) as u16);
        frame.set_cursor_position((x, y));
    }
}

fn draw_status(frame: &mut Frame, area: Rect, editor: &Editor) {
    let t = theme();
    let line = match editor.mode {
        Mode::Command => Line::from(format!(":{}", editor.prompt)),
        Mode::Search => Line::from(format!("/{}", editor.prompt)),
        Mode::Normal | Mode::Insert => {
            let mode = if editor.mode == Mode::Insert { " INSERT " } else { " NORMAL " };
            let name = editor.path.file_name().map_or_else(|| editor.path.display().to_string(), |n| n.to_string_lossy().into_owned());
            let mut spans = vec![
                Span::styled(mode, Style::default().fg(t.selection_fg).bg(t.selection_bg).add_modifier(Modifier::BOLD)),
                Span::styled(format!(" {name}{}", if editor.dirty { " [+]" } else { "" }), Style::default().fg(t.text)),
            ];
            let diagnostics = editor.diagnostics.len();
            if diagnostics > 0 {
                spans.push(Span::styled(format!("  {diagnostics} diagnostics"), Style::default().fg(t.modified)));
            }
            if editor.mps {
                spans.push(Span::styled("  MPS: names only", Style::default().fg(t.muted)));
            } else if !editor.lsp {
                spans.push(Span::styled("  plain text", Style::default().fg(t.muted)));
            }
            match &editor.message {
                Some(message) => spans.push(Span::styled(format!("  {message}"), Style::default().fg(t.info))),
                None => spans.push(Span::styled("  gd gr K ]d [d  :w :q :format :rename :actions  Enter:select entry  Esc:close", Style::default().fg(t.faint))),
            }
            let position = format!(" {}:{} ", editor.cursor_line() + 1, editor.cursor_column() + 1);
            let used: usize = spans.iter().map(Span::width).sum();
            let pad = usize::from(area.width).saturating_sub(used + position.len());
            spans.push(Span::raw(" ".repeat(pad)));
            spans.push(Span::styled(position, Style::default().fg(t.muted)));
            Line::from(spans)
        }
    };
    frame.render_widget(Paragraph::new(line), area);
    if matches!(editor.mode, Mode::Command | Mode::Search) {
        #[expect(clippy::cast_possible_truncation)] // a prompt line is far below u16::MAX
        frame.set_cursor_position((area.x + 1 + unicode_width::UnicodeWidthStr::width(editor.prompt.as_str()) as u16, area.y));
    }
}

fn draw_popup(frame: &mut Frame, area: Rect, editor: &Editor) {
    let Some(Popup { title, lines, selected, targets, edits }) = &editor.popup else { return };
    let t = theme();
    let is_list = !targets.is_empty() || !edits.is_empty();
    let width = (lines.iter().map(|l| unicode_width::UnicodeWidthStr::width(l.as_str())).max().unwrap_or(10) + 4).clamp(24, usize::from(area.width));
    let height = (lines.len() + 2).clamp(3, usize::from(area.height).saturating_sub(2).max(3));
    #[expect(clippy::cast_possible_truncation)] // clamped to the terminal size above
    let popup = crate::widgets::centred_rect(area, width as u16, height as u16);
    frame.render_widget(Clear, popup);
    let block = crate::widgets::panel_block(Style::default().fg(t.heading).add_modifier(Modifier::BOLD))
        .title(Span::styled(format!(" {title} "), Style::default().fg(t.heading).add_modifier(Modifier::BOLD)));
    let inner_height = height.saturating_sub(2);
    let first = if is_list { selected.saturating_sub(inner_height.saturating_sub(1)) } else { 0 };
    let body: Vec<Line> = lines
        .iter()
        .enumerate()
        .skip(first)
        .take(inner_height)
        .map(|(index, text)| {
            let style = if is_list && index == *selected { crate::widgets::selection_style(true) } else { Style::default().fg(t.text) };
            Line::from(Span::styled(text.clone(), style))
        })
        .collect();
    frame.render_widget(Paragraph::new(body).block(block), popup);
}
