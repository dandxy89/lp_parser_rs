//! Panes for the `HiGHS` C-API analyses ([`crate::highs_query`]).

use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use crate::highs_query::UnboundedRay;
use crate::theme::theme;
use crate::widgets::{rule_str, truncate_with_ellipsis};

/// Width of the name column in the ray table.
const NAME_WIDTH: usize = 26;

/// Width of the numeric columns.
const NUMBER_WIDTH: usize = 14;

/// Section heading with an underline, matching the diagnostics pane.
fn heading(lines: &mut Vec<Line<'static>>, title: &str, note: &str) {
    let t = theme();
    if !lines.is_empty() {
        lines.push(Line::from(""));
    }
    lines.push(Line::from(Span::styled(format!("  {title}"), Style::default().fg(t.accent).add_modifier(Modifier::BOLD))));
    lines.push(Line::from(Span::styled(format!("  {}", rule_str(title.chars().count())), Style::default().fg(t.muted))));
    if !note.is_empty() {
        lines.push(Line::from(Span::styled(format!("  {note}"), Style::default().fg(t.muted))));
    }
}

/// Render a bound, using the infinity sign where there is none.
fn bound(value: f64) -> String {
    if value.is_infinite() {
        if value.is_sign_negative() { "-\u{221e}".to_owned() } else { "+\u{221e}".to_owned() }
    } else {
        format!("{value:.4}")
    }
}

/// Build the display lines for the unbounded-ray pane.
pub fn ray_lines(report: &UnboundedRay) -> Vec<Line<'static>> {
    let t = theme();
    let mut lines = Vec::new();

    heading(&mut lines, "Unbounded ray", "");

    if !report.is_unbounded() {
        lines.push(Line::from(Span::styled(
            format!("  this model is not unbounded \u{2014} it solved as {}", report.status),
            Style::default().fg(t.added),
        )));
        footer(&mut lines, report);
        return lines;
    }

    lines.push(Line::from(Span::styled(
        format!("  the objective is unbounded ({})", report.status),
        Style::default().fg(t.removed).add_modifier(Modifier::BOLD),
    )));

    if report.directions.is_empty() {
        lines.push(Line::from(Span::styled(
            "  HiGHS returned no ray for this model, so the variables below are".to_owned(),
            Style::default().fg(t.muted),
        )));
        lines.push(Line::from(Span::styled(
            "  candidates found by inspection, not a certificate.".to_owned(),
            Style::default().fg(t.muted),
        )));
        suspect_table(&mut lines, report);
        footer(&mut lines, report);
        return lines;
    }

    lines.push(Line::from(Span::styled(
        format!("  objective improves by {:.6} per unit along the ray", report.objective_rate),
        Style::default().fg(t.text),
    )));

    heading(
        &mut lines,
        "Variables that run to infinity",
        "give any one of these a finite bound in its ray direction to close the model",
    );
    lines.push(Line::from(Span::styled(
        format!(
            "  {:<NAME_WIDTH$}{:>NUMBER_WIDTH$}{:>NUMBER_WIDTH$}{:>NUMBER_WIDTH$}{:>NUMBER_WIDTH$}",
            "variable", "ray", "cost", "lower", "upper"
        ),
        Style::default().fg(t.muted).add_modifier(Modifier::BOLD),
    )));

    for entry in &report.directions {
        lines.push(Line::from(vec![
            Span::styled(format!("  {:<NAME_WIDTH$}", truncate_with_ellipsis(&entry.name, NAME_WIDTH)), Style::default().fg(t.text)),
            Span::styled(format!("{:>NUMBER_WIDTH$.4}", entry.ray), Style::default().fg(t.removed)),
            Span::styled(format!("{:>NUMBER_WIDTH$.4}", entry.cost), Style::default().fg(t.accent)),
            Span::styled(format!("{:>NUMBER_WIDTH$}", bound(entry.lower)), Style::default().fg(t.muted)),
            Span::styled(format!("{:>NUMBER_WIDTH$}", bound(entry.upper)), Style::default().fg(t.muted)),
        ]));
    }

    footer(&mut lines, report);
    lines
}

/// The candidate list shown when no certificate was available.
fn suspect_table(lines: &mut Vec<Line<'static>>, report: &UnboundedRay) {
    let t = theme();
    if report.suspects.is_empty() {
        heading(lines, "Candidates", "");
        lines.push(Line::from(Span::styled(
            "  no variable escapes on its own \u{2014} the cause is a combination of rows".to_owned(),
            Style::default().fg(t.muted),
        )));
        return;
    }

    heading(lines, "Candidates", "improving the objective moves these towards a bound they do not have");
    lines.push(Line::from(Span::styled(
        format!("  {:<NAME_WIDTH$}{:>NUMBER_WIDTH$}   {}", "variable", "cost", "runs"),
        Style::default().fg(t.muted).add_modifier(Modifier::BOLD),
    )));
    for suspect in &report.suspects {
        lines.push(Line::from(vec![
            Span::styled(format!("  {:<NAME_WIDTH$}", truncate_with_ellipsis(&suspect.name, NAME_WIDTH)), Style::default().fg(t.text)),
            Span::styled(format!("{:>NUMBER_WIDTH$.4}", suspect.cost), Style::default().fg(t.accent)),
            Span::styled(format!("   {}", suspect.direction), Style::default().fg(t.removed)),
        ]));
    }
}

/// Caveats that apply to the whole report.
fn footer(lines: &mut Vec<Line<'static>>, report: &UnboundedRay) {
    let t = theme();
    lines.push(Line::from(""));
    if report.relaxed_integrality > 0 {
        lines.push(Line::from(Span::styled(
            format!("  {} integer column(s) relaxed: a ray is an LP concept", report.relaxed_integrality),
            Style::default().fg(t.modified),
        )));
    }
    if report.skipped_sos > 0 {
        lines.push(Line::from(Span::styled(
            format!("  {} SOS constraint(s) not modelled", report.skipped_sos),
            Style::default().fg(t.modified),
        )));
    }
    lines.push(Line::from(Span::styled(format!("  took {:.3}s", report.duration.as_secs_f64()), Style::default().fg(t.muted))));
}

/// Plain-text form of the ray report, for `w`.
pub fn ray_export(report: &UnboundedRay) -> String {
    use std::fmt::Write as _;

    let mut out = String::with_capacity(report.directions.len() * 80 + 256);
    let _ = writeln!(out, "Unbounded ray\n\nstatus: {}", report.status);

    if report.is_unbounded() {
        let _ = writeln!(out, "objective improves by {:.6} per unit along the ray\n", report.objective_rate);
        if report.directions.is_empty() {
            out.push_str("HiGHS returned no ray; the following are candidates found by inspection.\n\n");
            let _ = writeln!(out, "{:<30}{:>16}   runs", "variable", "cost");
            for suspect in &report.suspects {
                let _ = writeln!(out, "{:<30}{:>16.4}   {}", suspect.name, suspect.cost, suspect.direction);
            }
        } else {
            let _ = writeln!(out, "{:<30}{:>16}{:>16}{:>16}{:>16}", "variable", "ray", "cost", "lower", "upper");
            for entry in &report.directions {
                let _ = writeln!(
                    out,
                    "{:<30}{:>16.6}{:>16.6}{:>16}{:>16}",
                    entry.name,
                    entry.ray,
                    entry.cost,
                    bound(entry.lower),
                    bound(entry.upper)
                );
            }
        }
    } else {
        out.push_str("this model is not unbounded\n");
    }

    if report.relaxed_integrality > 0 {
        let _ = writeln!(out, "\n{} integer column(s) relaxed", report.relaxed_integrality);
    }
    if report.skipped_sos > 0 {
        let _ = writeln!(out, "{} SOS constraint(s) not modelled", report.skipped_sos);
    }
    let _ = writeln!(out, "took {:.3}s", report.duration.as_secs_f64());
    out
}
