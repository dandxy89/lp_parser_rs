//! Solver overlay widgets — file picker, progress, results, and error display.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

use crate::app::App;
use crate::state::SolveState;

/// Draw the solver overlay on top of the current frame, based on the current solve state.
pub fn draw_solve_overlay(frame: &mut Frame, area: Rect, app: &App) {
    match &app.solve_state {
        SolveState::Idle => {}
        SolveState::Picking => draw_picker(frame, area, app),
        SolveState::Running { file } => draw_running(frame, area, file),
        SolveState::Done(result) => draw_done(frame, area, result, app.solve_view.scroll),
        SolveState::Failed(err) => draw_failed(frame, area, err),
    }
}

fn draw_picker(frame: &mut Frame, area: Rect, app: &App) {
    let popup = super::centred_rect(area, 60, 8);
    let lines = vec![
        Line::from(""),
        Line::from(Span::styled("  Choose a file to solve:", Style::default().fg(Color::White).add_modifier(Modifier::BOLD))),
        Line::from(""),
        Line::from(vec![
            Span::styled("  [1] ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            Span::styled(app.file1_path.display().to_string(), Style::default().fg(Color::White)),
        ]),
        Line::from(vec![
            Span::styled("  [2] ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            Span::styled(app.file2_path.display().to_string(), Style::default().fg(Color::White)),
        ]),
        Line::from(""),
    ];

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
        .title(Span::styled(" Solve LP ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)));

    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(Clear, popup);
    frame.render_widget(paragraph, popup);
}

fn draw_running(frame: &mut Frame, area: Rect, file: &str) {
    let popup = super::centred_rect(area, 50, 5);
    let lines = vec![
        Line::from(""),
        Line::from(vec![
            Span::styled("  Solving ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            Span::styled(file.to_owned(), Style::default().fg(Color::White)),
            Span::styled("...", Style::default().fg(Color::Yellow)),
        ]),
        Line::from(""),
    ];

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))
        .title(Span::styled(" Solver ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)));

    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(Clear, popup);
    frame.render_widget(paragraph, popup);
}

fn draw_done(frame: &mut Frame, area: Rect, result: &crate::solver::SolveResult, scroll: u16) {
    let popup_width = (area.width * 4 / 5).max(60).min(area.width);
    let popup_height = (area.height * 4 / 5).max(20).min(area.height);
    let popup = super::centred_rect(area, popup_width, popup_height);

    let mut lines = vec![
        Line::from(""),
        Line::from(vec![
            Span::styled("  Status:    ", Style::default().fg(Color::DarkGray)),
            Span::styled(&result.status, status_style(&result.status)),
        ]),
    ];

    if let Some(obj) = result.objective_value {
        lines.push(Line::from(vec![
            Span::styled("  Objective: ", Style::default().fg(Color::DarkGray)),
            Span::styled(format!("{obj}"), Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
        ]));
    }

    lines.push(Line::from(vec![
        Span::styled("  Time:      ", Style::default().fg(Color::DarkGray)),
        Span::styled(format!("{:.3}s", result.solve_time.as_secs_f64()), Style::default().fg(Color::Cyan)),
    ]));

    if !result.variables.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            format!("  Variables ({}):", result.variables.len()),
            Style::default().fg(Color::DarkGray).add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(Span::styled("  ────────────────────────────────────────", Style::default().fg(Color::DarkGray))));

        for (name, val) in &result.variables {
            let val_style = if val.abs() < 1e-10 { Style::default().fg(Color::DarkGray) } else { Style::default().fg(Color::White) };
            lines.push(Line::from(vec![
                Span::styled(format!("  {name:<30}"), Style::default().fg(Color::White)),
                Span::styled(format!("{val:>12.6}"), val_style),
            ]));
        }
    }

    if !result.solver_log.is_empty() {
        const MAX_LOG_LINES: usize = 200;

        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled("  Solver Log:", Style::default().fg(Color::DarkGray).add_modifier(Modifier::BOLD))));
        lines.push(Line::from(Span::styled("  ────────────────────────────────────────", Style::default().fg(Color::DarkGray))));

        let all_log_lines: Vec<&str> = result.solver_log.lines().collect();
        let total = all_log_lines.len();

        if total > MAX_LOG_LINES {
            lines.push(Line::from(Span::styled(
                format!("  ... ({} lines truncated)", total - MAX_LOG_LINES),
                Style::default().fg(Color::Yellow),
            )));
        }

        for log_line in all_log_lines.iter().rev().take(MAX_LOG_LINES).rev() {
            lines.push(Line::from(Span::styled(format!("  {log_line}"), Style::default().fg(Color::DarkGray))));
        }
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled("  j/k: scroll  y: yank  Esc: close", Style::default().fg(Color::DarkGray))));

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Green).add_modifier(Modifier::BOLD))
        .title(Span::styled(" Solve Results ", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)));

    let paragraph = Paragraph::new(lines).block(block).scroll((scroll, 0));
    frame.render_widget(Clear, popup);
    frame.render_widget(paragraph, popup);
}

fn draw_failed(frame: &mut Frame, area: Rect, err: &str) {
    let popup = super::centred_rect(area, 60, 8);
    let lines = vec![
        Line::from(""),
        Line::from(Span::styled("  Solve failed:", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD))),
        Line::from(""),
        Line::from(Span::styled(format!("  {err}"), Style::default().fg(Color::Red))),
        Line::from(""),
        Line::from(Span::styled("  Press Esc to close", Style::default().fg(Color::DarkGray))),
    ];

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Red).add_modifier(Modifier::BOLD))
        .title(Span::styled(" Solver Error ", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)));

    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(Clear, popup);
    frame.render_widget(paragraph, popup);
}

/// Pick a style colour based on the status string.
fn status_style(status: &str) -> Style {
    if status.contains("Optimal") {
        Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)
    } else if status.contains("Infeasible") || status.contains("Unbounded") {
        Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
    }
}
