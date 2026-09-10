//! Solve profile pane (`B`): the same model under several `HiGHS`
//! configurations, with the times side by side.
//!
//! One row per preset. The columns that matter are wall-clock and iterations —
//! but the objective column is what makes the table trustworthy, because a
//! configuration that returns a *different* optimum is not a faster solve, and
//! that row is marked rather than celebrated.

use std::time::Duration;

use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use crate::profile::{Profile, Run};
use crate::theme::theme;

/// Width of the preset-label column, sized to the longest label in `PRESETS`.
const LABEL_WIDTH: usize = 16;

/// Width of the status column, sized to `UnboundedOrInfeasible`.
const STATUS_WIDTH: usize = 22;

/// Width of the numeric columns.
const NUMBER_WIDTH: usize = 11;

/// Format a duration the way the solve overlay does, so times read the same
/// wherever they appear.
fn seconds(duration: Duration) -> String {
    format!("{:.3}s", duration.as_secs_f64())
}

/// Format an objective value, or a dash when the preset produced none.
fn objective(value: Option<f64>) -> String {
    value.map_or_else(|| "\u{2014}".to_owned(), |v| format!("{v:.6}"))
}

/// Build the full set of display lines for the pane.
pub fn build_lines(profile: &Profile) -> Vec<Line<'static>> {
    let t = theme();
    let mut lines = Vec::with_capacity(profile.runs.len() + 16);

    lines.push(crate::widgets::note_line("presets run one after another: concurrent solves would inflate each other's times"));
    lines.push(Line::from(""));

    lines.push(Line::from(Span::styled(
        format!(
            "  {:<LABEL_WIDTH$}  {:<STATUS_WIDTH$}  {:>NUMBER_WIDTH$}  {:>NUMBER_WIDTH$}  {:>NUMBER_WIDTH$}",
            "preset", "status", "total", "solve", "iterations"
        ),
        Style::default().fg(t.muted).add_modifier(Modifier::BOLD),
    )));

    for (index, run) in profile.runs.iter().enumerate() {
        lines.push(run_line(run, profile.fastest == Some(index)));
        // The objective sits under its row rather than in a sixth column: it is
        // only interesting when it disagrees, and a full-width line has room to
        // say so in words.
        if let Ok(measurement) = &run.outcome {
            let style = if measurement.differs {
                Style::default().fg(t.removed).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(t.muted)
            };
            let note = if measurement.differs { "  \u{2190} differs from the baseline" } else { "" };
            lines.push(Line::from(Span::styled(
                format!("  {:<LABEL_WIDTH$}  objective {}{note}", "", objective(measurement.objective)),
                style,
            )));
        }
    }

    lines.push(Line::from(""));
    footer(&mut lines, profile);
    lines
}

/// One preset's row.
fn run_line(run: &Run, fastest: bool) -> Line<'static> {
    let t = theme();
    let marker = if fastest { "\u{25b8} " } else { "  " };

    match &run.outcome {
        Ok(measurement) => {
            let label_style = if fastest { Style::default().fg(t.added).add_modifier(Modifier::BOLD) } else { Style::default().fg(t.text) };
            let status_style = if measurement.status == "Optimal" { Style::default().fg(t.added) } else { Style::default().fg(t.modified) };
            Line::from(vec![
                Span::styled(format!("{marker}{:<LABEL_WIDTH$}  ", run.label), label_style),
                Span::styled(format!("{:<STATUS_WIDTH$}  ", measurement.status), status_style),
                Span::styled(format!("{:>NUMBER_WIDTH$}  ", seconds(measurement.total_time)), Style::default().fg(t.accent)),
                Span::styled(format!("{:>NUMBER_WIDTH$}  ", seconds(measurement.solve_time)), Style::default().fg(t.muted)),
                Span::styled(
                    format!("{:>NUMBER_WIDTH$}", measurement.iterations.map_or_else(|| "\u{2014}".to_owned(), |i| i.to_string())),
                    Style::default().fg(t.muted),
                ),
            ])
        }
        Err(error) => Line::from(vec![
            Span::styled(format!("  {:<LABEL_WIDTH$}  ", run.label), Style::default().fg(t.muted)),
            Span::styled(format!("failed: {error}"), Style::default().fg(t.removed)),
        ]),
    }
}

/// The closing summary: what won, and what the rewrite did.
fn footer(lines: &mut Vec<Line<'static>>, profile: &Profile) {
    let t = theme();

    match profile.speedup() {
        Some((label, factor)) => lines.push(Line::from(Span::styled(
            format!("  fastest: {label} \u{2014} {factor:.2}x over default"),
            Style::default().fg(t.added).add_modifier(Modifier::BOLD),
        ))),
        None => lines.push(Line::from(Span::styled("  no configuration beat the default".to_owned(), Style::default().fg(t.muted)))),
    }

    if let Some(rewrite) = &profile.rewrite {
        lines.push(Line::from(Span::styled(format!("  local presolve: {}", rewrite.headline()), Style::default().fg(t.muted))));
    }
    // Name the value every `differs` flag above was measured against.
    lines.push(Line::from(Span::styled(format!("  baseline objective {}", objective(profile.baseline)), Style::default().fg(t.muted))));
    lines.push(Line::from(Span::styled(
        format!("  budget {} per preset after the baseline \u{b7} sweep took {}", seconds(profile.budget), seconds(profile.duration)),
        Style::default().fg(t.muted),
    )));
}

/// Plain-text form of the table, for `w`.
///
/// Built from the same values as the pane rather than by stripping styles off
/// the lines, so the export keeps full precision where the pane pads columns.
pub fn export_text(profile: &Profile) -> String {
    use std::fmt::Write as _;

    let mut out = String::with_capacity(profile.runs.len() * 96);
    out.push_str("Solve profile\n\n");
    let _ = writeln!(out, "{:<18}{:<24}{:>12}{:>12}{:>12}  objective", "preset", "status", "total", "solve", "iterations");

    for run in &profile.runs {
        match &run.outcome {
            Ok(m) => {
                let _ = writeln!(
                    out,
                    "{:<18}{:<24}{:>12}{:>12}{:>12}  {}{}",
                    run.label,
                    m.status,
                    seconds(m.total_time),
                    seconds(m.solve_time),
                    m.iterations.map_or_else(|| "-".to_owned(), |i| i.to_string()),
                    objective(m.objective),
                    if m.differs { "  (differs from baseline)" } else { "" },
                );
            }
            Err(error) => {
                let _ = writeln!(out, "{:<18}failed: {error}", run.label);
            }
        }
    }

    out.push('\n');
    match profile.speedup() {
        Some((label, factor)) => {
            let _ = writeln!(out, "fastest: {label} - {factor:.2}x over default");
        }
        None => out.push_str("no configuration beat the default\n"),
    }
    if let Some(rewrite) = &profile.rewrite {
        let _ = writeln!(out, "local presolve: {}", rewrite.headline());
    }
    let _ = writeln!(out, "baseline objective {}", objective(profile.baseline));
    let _ = writeln!(out, "budget {} per preset after the baseline; sweep took {}", seconds(profile.budget), seconds(profile.duration));
    out
}
