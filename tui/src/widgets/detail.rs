//! Detail panel widget.
//!
//! Renders the full before/after breakdown for a single selected diff entry
//! (variables, constraints, objectives) in the detail pane.

use std::collections::BTreeMap;

use lp_parser_rs::model::VariableType;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::diff_model::{CoefficientChange, ConstraintDiffDetail, ConstraintDiffEntry, DiffKind, ObjectiveDiffEntry, VariableDiffEntry};
use crate::widgets::kind_colour;

/// Horizontal rule used as a visual separator below the entry header.
fn rule<'a>() -> Line<'a> {
    Line::from(Span::styled("──────────────────────────────────────", Style::default().fg(Color::DarkGray)))
}

/// Build the common header lines for a detail panel: entity label, name, kind badge, and rule.
fn detail_header(entity_label: &str, name: &str, kind: DiffKind) -> Vec<Line<'static>> {
    vec![
        Line::from(vec![
            Span::styled(format!("{entity_label}: "), Style::default().fg(Color::DarkGray)),
            Span::styled(name.to_owned(), Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
            Span::styled(format!(" [{kind}]"), Style::default().fg(kind_colour(kind))),
        ]),
        rule(),
    ]
}

/// Extract (lower, upper) bounds from a `VariableType`, returning `None` for
/// bounds that don't apply to that type.
pub(crate) const fn variable_bounds(vt: &VariableType) -> (Option<f64>, Option<f64>) {
    match *vt {
        VariableType::LowerBound(lb) => (Some(lb), None),
        VariableType::UpperBound(ub) => (None, Some(ub)),
        VariableType::DoubleBound(lb, ub) => (Some(lb), Some(ub)),
        _ => (None, None),
    }
}

/// Format an optional bound value as a string for display.
fn fmt_bound(val: Option<f64>) -> String {
    match val {
        Some(v) => format!("{v}"),
        None => "\u{2014}".to_owned(), // em-dash
    }
}

/// Render a variable detail panel.
pub(crate) fn render_variable_detail(frame: &mut Frame, area: Rect, entry: &VariableDiffEntry, border_style: Style, scroll: u16) {
    let mut lines = detail_header("Variable", &entry.name, entry.kind);

    match entry.kind {
        DiffKind::Added => {
            let vt = entry.new_type.as_ref().expect("invariant: Added entry must have new_type");
            let label = std::str::from_utf8(vt.as_ref()).unwrap_or("?");
            lines.push(Line::from(vec![
                Span::styled("  Type:   ", Style::default().fg(Color::DarkGray)),
                Span::styled(label.to_owned(), Style::default().fg(Color::Green)),
            ]));

            let (lb, ub) = variable_bounds(vt);
            if let Some(l) = lb {
                lines.push(Line::from(vec![
                    Span::styled("  Lower:  ", Style::default().fg(Color::DarkGray)),
                    Span::styled(format!("{l}"), Style::default().fg(Color::Green)),
                ]));
            }
            if let Some(u) = ub {
                lines.push(Line::from(vec![
                    Span::styled("  Upper:  ", Style::default().fg(Color::DarkGray)),
                    Span::styled(format!("{u}"), Style::default().fg(Color::Green)),
                ]));
            }
            if let (Some(l), Some(u)) = (lb, ub) {
                lines.push(Line::from(vec![
                    Span::styled("  Range:  ", Style::default().fg(Color::DarkGray)),
                    Span::styled(format!("{}", u - l), Style::default().fg(Color::Green)),
                ]));
            }
        }
        DiffKind::Removed => {
            let vt = entry.old_type.as_ref().expect("invariant: Removed entry must have old_type");
            let label = std::str::from_utf8(vt.as_ref()).unwrap_or("?");
            lines.push(Line::from(vec![
                Span::styled("  Type:   ", Style::default().fg(Color::DarkGray)),
                Span::styled(label.to_owned(), Style::default().fg(Color::Red)),
            ]));

            let (lb, ub) = variable_bounds(vt);
            if let Some(l) = lb {
                lines.push(Line::from(vec![
                    Span::styled("  Lower:  ", Style::default().fg(Color::DarkGray)),
                    Span::styled(format!("{l}"), Style::default().fg(Color::Red)),
                ]));
            }
            if let Some(u) = ub {
                lines.push(Line::from(vec![
                    Span::styled("  Upper:  ", Style::default().fg(Color::DarkGray)),
                    Span::styled(format!("{u}"), Style::default().fg(Color::Red)),
                ]));
            }
            if let (Some(l), Some(u)) = (lb, ub) {
                lines.push(Line::from(vec![
                    Span::styled("  Range:  ", Style::default().fg(Color::DarkGray)),
                    Span::styled(format!("{}", u - l), Style::default().fg(Color::Red)),
                ]));
            }
        }
        DiffKind::Modified => {
            let old = entry.old_type.as_ref().expect("invariant: Modified entry must have old_type");
            let new = entry.new_type.as_ref().expect("invariant: Modified entry must have new_type");
            let old_label = std::str::from_utf8(old.as_ref()).unwrap_or("?");
            let new_label = std::str::from_utf8(new.as_ref()).unwrap_or("?");

            if old_label != new_label {
                lines.push(Line::from(vec![
                    Span::styled("  Type:   ", Style::default().fg(Color::DarkGray)),
                    Span::styled(old_label.to_owned(), Style::default().fg(Color::Red)),
                    Span::styled("  \u{2192}  ", Style::default().fg(Color::DarkGray)),
                    Span::styled(new_label.to_owned(), Style::default().fg(Color::Green)),
                ]));
            } else {
                lines.push(Line::from(vec![
                    Span::styled("  Type:   ", Style::default().fg(Color::DarkGray)),
                    Span::styled(old_label.to_owned(), Style::default().fg(Color::White)),
                    Span::styled(" (unchanged)", Style::default().fg(Color::DarkGray)),
                ]));
            }

            // Bounds comparison.
            let (old_lb, old_ub) = variable_bounds(old);
            let (new_lb, new_ub) = variable_bounds(new);

            if old_lb.is_some() || new_lb.is_some() {
                lines.push(Line::from(vec![
                    Span::styled("  Lower:  ", Style::default().fg(Color::DarkGray)),
                    Span::styled(fmt_bound(old_lb), Style::default().fg(Color::Red)),
                    Span::styled("  \u{2192}  ", Style::default().fg(Color::DarkGray)),
                    Span::styled(fmt_bound(new_lb), Style::default().fg(Color::Green)),
                ]));
            }

            if old_ub.is_some() || new_ub.is_some() {
                lines.push(Line::from(vec![
                    Span::styled("  Upper:  ", Style::default().fg(Color::DarkGray)),
                    Span::styled(fmt_bound(old_ub), Style::default().fg(Color::Red)),
                    Span::styled("  \u{2192}  ", Style::default().fg(Color::DarkGray)),
                    Span::styled(fmt_bound(new_ub), Style::default().fg(Color::Green)),
                ]));
            }

            let old_range = old_lb.zip(old_ub).map(|(l, u)| u - l);
            let new_range = new_lb.zip(new_ub).map(|(l, u)| u - l);
            if old_range.is_some() || new_range.is_some() {
                lines.push(Line::from(vec![
                    Span::styled("  Range:  ", Style::default().fg(Color::DarkGray)),
                    Span::styled(fmt_bound(old_range), Style::default().fg(Color::Red)),
                    Span::styled("  \u{2192}  ", Style::default().fg(Color::DarkGray)),
                    Span::styled(fmt_bound(new_range), Style::default().fg(Color::Green)),
                ]));
            }
        }
    }

    render_panel(frame, area, " Variable Detail ", lines, border_style, scroll);
}

pub(crate) fn render_constraint_detail(frame: &mut Frame, area: Rect, entry: &ConstraintDiffEntry, border_style: Style, scroll: u16) {
    let mut lines = detail_header("Constraint", &entry.name, entry.kind);

    // Render line number location if available.
    if entry.line_file1.is_some() || entry.line_file2.is_some() {
        let mut spans = vec![Span::styled("  Location: ", Style::default().fg(Color::DarkGray))];
        match (entry.line_file1, entry.line_file2) {
            (Some(l1), Some(l2)) => {
                // Present in both files (modified or unchanged).
                spans.push(Span::styled(format!("L{l1}"), Style::default().fg(Color::Cyan)));
                spans.push(Span::styled(" / ", Style::default().fg(Color::DarkGray)));
                spans.push(Span::styled(format!("L{l2}"), Style::default().fg(Color::Cyan)));
            }
            (Some(l1), None) => {
                // Removed: only in file 1.
                spans.push(Span::styled(format!("L{l1}"), Style::default().fg(Color::Red)));
                spans.push(Span::styled(" (removed)", Style::default().fg(Color::DarkGray)));
            }
            (None, Some(l2)) => {
                // Added: only in file 2.
                spans.push(Span::styled(format!("L{l2}"), Style::default().fg(Color::Green)));
                spans.push(Span::styled(" (added)", Style::default().fg(Color::DarkGray)));
            }
            (None, None) => unreachable!("guarded by outer if"),
        }
        lines.push(Line::from(spans));
    }

    match &entry.detail {
        ConstraintDiffDetail::Standard {
            old_coefficients,
            new_coefficients,
            coeff_changes,
            operator_change,
            rhs_change,
            old_rhs,
            new_rhs,
        } => {
            // Operator change.
            if let Some((old_op, new_op)) = operator_change {
                lines.push(Line::from(vec![
                    Span::styled("  Operator: ", Style::default().fg(Color::DarkGray)),
                    Span::styled(format!("{old_op}"), Style::default().fg(Color::Red)),
                    Span::styled("  \u{2192}  ", Style::default().fg(Color::DarkGray)),
                    Span::styled(format!("{new_op}"), Style::default().fg(Color::Green)),
                ]));
            }

            // RHS change.
            if rhs_change.is_some() {
                lines.push(Line::from(vec![
                    Span::styled("  RHS:      ", Style::default().fg(Color::DarkGray)),
                    Span::styled(format!("{old_rhs}"), Style::default().fg(Color::Red)),
                    Span::styled("  \u{2192}  ", Style::default().fg(Color::DarkGray)),
                    Span::styled(format!("{new_rhs}"), Style::default().fg(Color::Green)),
                ]));
            } else {
                lines.push(Line::from(vec![
                    Span::styled("  RHS:      ", Style::default().fg(Color::DarkGray)),
                    Span::styled(format!("{old_rhs} (unchanged)"), Style::default().fg(Color::DarkGray)),
                ]));
            }

            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled("  Coefficients:", Style::default().fg(Color::DarkGray).add_modifier(Modifier::BOLD))));

            render_coeff_changes(&mut lines, coeff_changes, old_coefficients, new_coefficients);
        }

        ConstraintDiffDetail::Sos { old_weights, new_weights, weight_changes, type_change } => {
            if let Some((old_type, new_type)) = type_change {
                lines.push(Line::from(vec![
                    Span::styled("  SOS Type: ", Style::default().fg(Color::DarkGray)),
                    Span::styled(format!("{old_type}"), Style::default().fg(Color::Red)),
                    Span::styled("  \u{2192}  ", Style::default().fg(Color::DarkGray)),
                    Span::styled(format!("{new_type}"), Style::default().fg(Color::Green)),
                ]));
            }

            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled("  Weights:", Style::default().fg(Color::DarkGray).add_modifier(Modifier::BOLD))));

            render_coeff_changes(&mut lines, weight_changes, old_weights, new_weights);
        }

        ConstraintDiffDetail::TypeChanged { old_summary, new_summary } => {
            lines.push(Line::from(vec![
                Span::styled("  Was:  ", Style::default().fg(Color::DarkGray)),
                Span::styled(old_summary.clone(), Style::default().fg(Color::Red)),
            ]));
            lines.push(Line::from(vec![
                Span::styled("  Now:  ", Style::default().fg(Color::DarkGray)),
                Span::styled(new_summary.clone(), Style::default().fg(Color::Green)),
            ]));
        }

        ConstraintDiffDetail::AddedOrRemoved(constraint) => {
            use lp_parser_rs::model::ConstraintOwned;
            let entry_colour = kind_colour(entry.kind);

            match constraint {
                ConstraintOwned::Standard { coefficients, operator, rhs, .. } => {
                    lines.push(Line::from(vec![
                        Span::styled("  Operator: ", Style::default().fg(Color::DarkGray)),
                        Span::styled(format!("{operator}"), Style::default().fg(Color::White)),
                    ]));
                    lines.push(Line::from(vec![
                        Span::styled("  RHS:      ", Style::default().fg(Color::DarkGray)),
                        Span::styled(format!("{rhs}"), Style::default().fg(Color::White)),
                    ]));
                    lines.push(Line::from(""));
                    lines.push(Line::from(Span::styled(
                        "  Coefficients:",
                        Style::default().fg(Color::DarkGray).add_modifier(Modifier::BOLD),
                    )));
                    for coeff in coefficients {
                        lines.push(Line::from(vec![
                            Span::styled(format!("    {:<20}", coeff.name), Style::default().fg(entry_colour)),
                            Span::styled(format!("{}", coeff.value), Style::default().fg(entry_colour)),
                        ]));
                    }
                }
                ConstraintOwned::SOS { sos_type, weights, .. } => {
                    lines.push(Line::from(vec![
                        Span::styled("  SOS Type: ", Style::default().fg(Color::DarkGray)),
                        Span::styled(format!("{sos_type}"), Style::default().fg(Color::White)),
                    ]));
                    lines.push(Line::from(""));
                    lines.push(Line::from(Span::styled("  Weights:", Style::default().fg(Color::DarkGray).add_modifier(Modifier::BOLD))));
                    for w in weights {
                        lines.push(Line::from(vec![
                            Span::styled(format!("    {:<20}", w.name), Style::default().fg(entry_colour)),
                            Span::styled(format!("{}", w.value), Style::default().fg(entry_colour)),
                        ]));
                    }
                }
            }
        }
    }

    render_panel(frame, area, " Constraint Detail ", lines, border_style, scroll);
}

pub(crate) fn render_objective_detail(frame: &mut Frame, area: Rect, entry: &ObjectiveDiffEntry, border_style: Style, scroll: u16) {
    let mut lines = detail_header("Objective", &entry.name, entry.kind);

    lines.push(Line::from(Span::styled("  Coefficients:", Style::default().fg(Color::DarkGray).add_modifier(Modifier::BOLD))));

    if entry.kind == DiffKind::Modified {
        render_coeff_changes(&mut lines, &entry.coeff_changes, &entry.old_coefficients, &entry.new_coefficients);
    } else {
        let coeffs = if entry.kind == DiffKind::Added { &entry.new_coefficients } else { &entry.old_coefficients };
        let colour = kind_colour(entry.kind);
        for c in coeffs {
            lines.push(Line::from(vec![
                Span::styled(format!("    {:<20}", c.name), Style::default().fg(colour)),
                Span::styled(format!("{}", c.value), Style::default().fg(colour)),
            ]));
        }
    }

    render_panel(frame, area, " Objective Detail ", lines, border_style, scroll);
}

/// `(old value, new value, change kind)` entry in the per-variable diff map.
type CoeffEntry = (Option<f64>, Option<f64>, Option<DiffKind>);

/// Render a combined view of old and new coefficient lists, annotating each
/// variable with its change status.
fn render_coeff_changes(
    lines: &mut Vec<Line<'_>>,
    changes: &[CoefficientChange],
    old_coefficients: &[lp_parser_rs::model::CoefficientOwned],
    new_coefficients: &[lp_parser_rs::model::CoefficientOwned],
) {
    let mut all_vars: BTreeMap<String, CoeffEntry> = BTreeMap::new();

    for c in old_coefficients {
        all_vars.entry(c.name.clone()).or_default().0 = Some(c.value);
    }
    for c in new_coefficients {
        all_vars.entry(c.name.clone()).or_default().1 = Some(c.value);
    }
    for change in changes {
        if let Some(entry) = all_vars.get_mut(&change.variable) {
            entry.2 = Some(change.kind);
        }
    }

    // Column width for value formatting — wide enough for typical LP coefficients.
    const VAL_WIDTH: usize = 12;

    for (var_name, (old_val, new_val, change_kind)) in &all_vars {
        let old_str = old_val.map_or_else(String::new, |v| format!("{v}"));
        let new_str = new_val.map_or_else(String::new, |v| format!("{v}"));

        match change_kind {
            Some(DiffKind::Added) => {
                lines.push(Line::from(vec![
                    Span::styled(format!("    {var_name:<20}"), Style::default().fg(Color::Green)),
                    Span::styled(format!("{:>VAL_WIDTH$}", ""), Style::default()),
                    Span::styled("  \u{2192}  ", Style::default().fg(Color::DarkGray)),
                    Span::styled(format!("{new_str:<VAL_WIDTH$}"), Style::default().fg(Color::Green)),
                    Span::styled(" [added]", Style::default().fg(Color::Green)),
                ]));
            }
            Some(DiffKind::Removed) => {
                lines.push(Line::from(vec![
                    Span::styled(format!("    {var_name:<20}"), Style::default().fg(Color::Red)),
                    Span::styled(format!("{old_str:>VAL_WIDTH$}"), Style::default().fg(Color::Red)),
                    Span::styled("  \u{2192}  ", Style::default().fg(Color::DarkGray)),
                    Span::styled(format!("{:VAL_WIDTH$}", ""), Style::default()),
                    Span::styled(" [removed]", Style::default().fg(Color::Red)),
                ]));
            }
            Some(DiffKind::Modified) => {
                lines.push(Line::from(vec![
                    Span::styled(format!("    {var_name:<20}"), Style::default().fg(Color::Yellow)),
                    Span::styled(format!("{old_str:>VAL_WIDTH$}"), Style::default().fg(Color::Red)),
                    Span::styled("  \u{2192}  ", Style::default().fg(Color::DarkGray)),
                    Span::styled(format!("{new_str:<VAL_WIDTH$}"), Style::default().fg(Color::Green)),
                    Span::styled(" [modified]", Style::default().fg(Color::Yellow)),
                ]));
            }
            None => {
                lines.push(Line::from(vec![
                    Span::styled(format!("    {var_name:<20}"), Style::default().fg(Color::DarkGray)),
                    Span::styled(format!("{old_str:>VAL_WIDTH$}"), Style::default().fg(Color::DarkGray)),
                    Span::styled(" (unchanged)", Style::default().fg(Color::DarkGray)),
                ]));
            }
        }
    }
}

/// Wrap `lines` in a bordered block with the given `title` and render it,
/// applying vertical scroll.
fn render_panel(frame: &mut Frame, area: Rect, title: &'static str, lines: Vec<Line<'_>>, border_style: Style, scroll: u16) {
    let block = Block::default().borders(Borders::ALL).border_style(border_style).title(title);
    let paragraph = Paragraph::new(lines).block(block).scroll((scroll, 0));
    frame.render_widget(paragraph, area);
}
