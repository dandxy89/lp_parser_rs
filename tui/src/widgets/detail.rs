//! Detail panel widget.
//!
//! Renders the full before/after breakdown for a single selected diff entry
//! (variables, constraints, objectives) in the detail pane.

use lp_parser_rs::model::VariableType;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::detail_model::build_coeff_rows;
use crate::diff_model::{CoefficientChange, ConstraintDiffDetail, ConstraintDiffEntry, DiffKind, ObjectiveDiffEntry, VariableDiffEntry};
use crate::widgets::{ARROW, BOLD_TEXT, MUTED, kind_colour};

/// Horizontal rule used as a visual separator below the entry header.
fn rule<'a>() -> Line<'a> {
    Line::from(Span::styled("──────────────────────────────────────", MUTED))
}

/// Build the common header lines for a detail panel: entity label, name, kind badge, and rule.
fn detail_header(entity_label: &str, name: &str, kind: DiffKind) -> Vec<Line<'static>> {
    vec![
        Line::from(vec![
            Span::styled(format!("{entity_label}: "), MUTED),
            Span::styled(name.to_owned(), BOLD_TEXT),
            Span::styled(format!(" [{kind}]"), Style::default().fg(kind_colour(kind))),
        ]),
        rule(),
    ]
}

/// Extract (lower, upper) bounds from a `VariableType`, returning `None` for
/// bounds that don't apply to that type.
pub const fn variable_bounds(vt: &VariableType) -> (Option<f64>, Option<f64>) {
    match *vt {
        VariableType::LowerBound(lb) => (Some(lb), None),
        VariableType::UpperBound(ub) => (None, Some(ub)),
        VariableType::DoubleBound(lb, ub) => (Some(lb), Some(ub)),
        _ => (None, None),
    }
}

/// Format an optional bound value as a string for display.
pub fn fmt_bound(val: Option<f64>) -> String {
    val.map_or_else(|| "\u{2014}".to_owned(), |v| format!("{v}"))
}

/// Render type/bounds lines for an added or removed variable (single-side view).
fn render_variable_type_info(lines: &mut Vec<Line<'static>>, vt: &VariableType, colour: Color) {
    let label = std::str::from_utf8(vt.as_ref()).unwrap_or("?");
    lines.push(Line::from(vec![Span::styled("  Type:   ", MUTED), Span::styled(label.to_owned(), Style::default().fg(colour))]));

    let (lb, ub) = variable_bounds(vt);
    if let Some(l) = lb {
        lines.push(Line::from(vec![Span::styled("  Lower:  ", MUTED), Span::styled(format!("{l}"), Style::default().fg(colour))]));
    }
    if let Some(u) = ub {
        lines.push(Line::from(vec![Span::styled("  Upper:  ", MUTED), Span::styled(format!("{u}"), Style::default().fg(colour))]));
    }
    if let (Some(l), Some(u)) = (lb, ub) {
        lines.push(Line::from(vec![Span::styled("  Range:  ", MUTED), Span::styled(format!("{}", u - l), Style::default().fg(colour))]));
    }
}

/// Render a variable detail panel. Returns the total content line count.
#[allow(clippy::too_many_lines)]
#[allow(clippy::similar_names)] // lb/ub are standard abbreviations for lower/upper bound
pub fn render_variable_detail(frame: &mut Frame, area: Rect, entry: &VariableDiffEntry, border_style: Style, scroll: u16) -> usize {
    let mut lines = detail_header("Variable", &entry.name, entry.kind);

    match entry.kind {
        DiffKind::Added => {
            let vt = entry.new_type.as_ref().expect("invariant: Added entry must have new_type");
            render_variable_type_info(&mut lines, vt, Color::Green);
        }
        DiffKind::Removed => {
            let vt = entry.old_type.as_ref().expect("invariant: Removed entry must have old_type");
            render_variable_type_info(&mut lines, vt, Color::Red);
        }
        DiffKind::Modified => {
            let old = entry.old_type.as_ref().expect("invariant: Modified entry must have old_type");
            let new = entry.new_type.as_ref().expect("invariant: Modified entry must have new_type");
            let old_label = std::str::from_utf8(old.as_ref()).unwrap_or("?");
            let new_label = std::str::from_utf8(new.as_ref()).unwrap_or("?");

            if old_label == new_label {
                lines.push(Line::from(vec![
                    Span::styled("  Type:   ", MUTED),
                    Span::styled(old_label.to_owned(), Style::default().fg(Color::White)),
                    Span::styled(" (unchanged)", MUTED),
                ]));
            } else {
                lines.push(Line::from(vec![
                    Span::styled("  Type:   ", MUTED),
                    Span::styled(old_label.to_owned(), Style::default().fg(Color::Red)),
                    Span::styled(ARROW, MUTED),
                    Span::styled(new_label.to_owned(), Style::default().fg(Color::Green)),
                ]));
            }

            // Bounds comparison.
            let (old_lb, old_ub) = variable_bounds(old);
            let (new_lb, new_ub) = variable_bounds(new);

            if old_lb.is_some() || new_lb.is_some() {
                lines.push(Line::from(vec![
                    Span::styled("  Lower:  ", MUTED),
                    Span::styled(fmt_bound(old_lb), Style::default().fg(Color::Red)),
                    Span::styled(ARROW, MUTED),
                    Span::styled(fmt_bound(new_lb), Style::default().fg(Color::Green)),
                ]));
            }

            if old_ub.is_some() || new_ub.is_some() {
                lines.push(Line::from(vec![
                    Span::styled("  Upper:  ", MUTED),
                    Span::styled(fmt_bound(old_ub), Style::default().fg(Color::Red)),
                    Span::styled(ARROW, MUTED),
                    Span::styled(fmt_bound(new_ub), Style::default().fg(Color::Green)),
                ]));
            }

            let old_range = old_lb.zip(old_ub).map(|(l, u)| u - l);
            let new_range = new_lb.zip(new_ub).map(|(l, u)| u - l);
            if old_range.is_some() || new_range.is_some() {
                lines.push(Line::from(vec![
                    Span::styled("  Range:  ", MUTED),
                    Span::styled(fmt_bound(old_range), Style::default().fg(Color::Red)),
                    Span::styled(ARROW, MUTED),
                    Span::styled(fmt_bound(new_range), Style::default().fg(Color::Green)),
                ]));
            }
        }
    }

    render_panel(frame, area, " Variable Detail ", lines, border_style, scroll)
}

/// Render a constraint detail panel. Returns the total content line count.
#[allow(clippy::too_many_lines)]
pub fn render_constraint_detail(frame: &mut Frame, area: Rect, entry: &ConstraintDiffEntry, border_style: Style, scroll: u16) -> usize {
    let mut lines = detail_header("Constraint", &entry.name, entry.kind);

    // Render line number location if available.
    if entry.line_file1.is_some() || entry.line_file2.is_some() {
        let mut spans = vec![Span::styled("  Location: ", MUTED)];
        match (entry.line_file1, entry.line_file2) {
            (Some(l1), Some(l2)) => {
                // Present in both files (modified or unchanged).
                spans.push(Span::styled(format!("L{l1}"), Style::default().fg(Color::Cyan)));
                spans.push(Span::styled(" / ", MUTED));
                spans.push(Span::styled(format!("L{l2}"), Style::default().fg(Color::Cyan)));
            }
            (Some(l1), None) => {
                // Removed: only in file 1.
                spans.push(Span::styled(format!("L{l1}"), Style::default().fg(Color::Red)));
                spans.push(Span::styled(" (removed)", MUTED));
            }
            (None, Some(l2)) => {
                // Added: only in file 2.
                spans.push(Span::styled(format!("L{l2}"), Style::default().fg(Color::Green)));
                spans.push(Span::styled(" (added)", MUTED));
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
                    Span::styled("  Operator: ", MUTED),
                    Span::styled(format!("{old_op}"), Style::default().fg(Color::Red)),
                    Span::styled(ARROW, MUTED),
                    Span::styled(format!("{new_op}"), Style::default().fg(Color::Green)),
                ]));
            }

            // RHS change.
            if rhs_change.is_some() {
                lines.push(Line::from(vec![
                    Span::styled("  RHS:      ", MUTED),
                    Span::styled(format!("{old_rhs}"), Style::default().fg(Color::Red)),
                    Span::styled(ARROW, MUTED),
                    Span::styled(format!("{new_rhs}"), Style::default().fg(Color::Green)),
                ]));
            } else {
                lines.push(Line::from(vec![
                    Span::styled("  RHS:      ", MUTED),
                    Span::styled(format!("{old_rhs} (unchanged)"), Style::default().fg(Color::DarkGray)),
                ]));
            }

            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled("  Coefficients:", MUTED.add_modifier(Modifier::BOLD))));

            if entry.kind == DiffKind::Modified {
                // Side-by-side rendering for modified standard constraints.
                // Render header + operator/RHS in a single Paragraph, then split
                // the remaining area into two columns for old/new coefficients.
                let header_line_count = lines.len();
                let block = Block::default().borders(Borders::ALL).border_style(border_style).title(" Constraint Detail ");
                let inner = block.inner(area);
                frame.render_widget(block, area);

                // Header paragraph (above the two columns).
                #[allow(clippy::cast_possible_truncation)]
                let header_height = header_line_count as u16;
                let v_chunks = Layout::vertical([Constraint::Length(header_height), Constraint::Min(0)]).split(inner);

                let header_para = Paragraph::new(lines).scroll((scroll, 0));
                frame.render_widget(header_para, v_chunks[0]);

                // Build the unified coefficient rows.
                let rows = build_coeff_rows(coeff_changes, old_coefficients, new_coefficients);

                // Build left (old) and right (new) line lists.
                let mut left_lines: Vec<Line<'_>> = vec![Line::from(Span::styled(
                    " Old",
                    Style::default().fg(Color::Red).add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
                ))];
                let mut right_lines: Vec<Line<'_>> = vec![Line::from(Span::styled(
                    " New",
                    Style::default().fg(Color::Green).add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
                ))];

                for row in &rows {
                    let (left_style, right_style, badge) = match row.change_kind {
                        Some(DiffKind::Added) => (Style::default().fg(Color::DarkGray), Style::default().fg(Color::Green), " [+]"),
                        Some(DiffKind::Removed) => (Style::default().fg(Color::Red), Style::default().fg(Color::DarkGray), " [-]"),
                        Some(DiffKind::Modified) => (Style::default().fg(Color::Red), Style::default().fg(Color::Green), " [~]"),
                        None => (Style::default().fg(Color::DarkGray), Style::default().fg(Color::DarkGray), ""),
                    };

                    let old_str = row.old_value.map_or_else(String::new, |v| format!("{v}"));
                    let new_str = row.new_value.map_or_else(String::new, |v| format!("{v}"));

                    left_lines.push(Line::from(vec![
                        Span::styled(format!(" {:<18}", row.variable), left_style),
                        Span::styled(format!("{old_str:>10}"), left_style),
                        Span::styled(badge, left_style),
                    ]));
                    right_lines.push(Line::from(vec![
                        Span::styled(format!(" {:<18}", row.variable), right_style),
                        Span::styled(format!("{new_str:>10}"), right_style),
                        Span::styled(badge, right_style),
                    ]));
                }

                // Scroll the coefficient area if the header is scrolled past.
                let coeff_scroll = scroll.saturating_sub(header_height);

                let h_chunks = Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)]).split(v_chunks[1]);

                let left_para = Paragraph::new(left_lines).scroll((coeff_scroll, 0));
                let right_para = Paragraph::new(right_lines).scroll((coeff_scroll, 0));
                frame.render_widget(left_para, h_chunks[0]);
                frame.render_widget(right_para, h_chunks[1]);

                // Return total content lines (header + 1 header line + coeff rows).
                return header_line_count + 1 + rows.len();
            }

            render_coeff_changes(&mut lines, coeff_changes, old_coefficients, new_coefficients);
        }

        ConstraintDiffDetail::Sos { old_weights, new_weights, weight_changes, type_change } => {
            if let Some((old_type, new_type)) = type_change {
                lines.push(Line::from(vec![
                    Span::styled("  SOS Type: ", MUTED),
                    Span::styled(format!("{old_type}"), Style::default().fg(Color::Red)),
                    Span::styled(ARROW, MUTED),
                    Span::styled(format!("{new_type}"), Style::default().fg(Color::Green)),
                ]));
            }

            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled("  Weights:", MUTED.add_modifier(Modifier::BOLD))));

            render_coeff_changes(&mut lines, weight_changes, old_weights, new_weights);
        }

        ConstraintDiffDetail::TypeChanged { old_summary, new_summary } => {
            lines.push(Line::from(vec![
                Span::styled("  Was:  ", MUTED),
                Span::styled(old_summary.clone(), Style::default().fg(Color::Red)),
            ]));
            lines.push(Line::from(vec![
                Span::styled("  Now:  ", MUTED),
                Span::styled(new_summary.clone(), Style::default().fg(Color::Green)),
            ]));
        }

        ConstraintDiffDetail::AddedOrRemoved(constraint) => {
            use lp_parser_rs::model::ConstraintOwned;
            let entry_colour = kind_colour(entry.kind);

            match constraint {
                ConstraintOwned::Standard { coefficients, operator, rhs, .. } => {
                    lines.push(Line::from(vec![
                        Span::styled("  Operator: ", MUTED),
                        Span::styled(format!("{operator}"), Style::default().fg(Color::White)),
                    ]));
                    lines.push(Line::from(vec![
                        Span::styled("  RHS:      ", MUTED),
                        Span::styled(format!("{rhs}"), Style::default().fg(Color::White)),
                    ]));
                    lines.push(Line::from(""));
                    lines.push(Line::from(Span::styled("  Coefficients:", MUTED.add_modifier(Modifier::BOLD))));
                    for coeff in coefficients {
                        lines.push(Line::from(vec![
                            Span::styled(format!("    {:<20}", coeff.name), Style::default().fg(entry_colour)),
                            Span::styled(format!("{}", coeff.value), Style::default().fg(entry_colour)),
                        ]));
                    }
                }
                ConstraintOwned::SOS { sos_type, weights, .. } => {
                    lines.push(Line::from(vec![
                        Span::styled("  SOS Type: ", MUTED),
                        Span::styled(format!("{sos_type}"), Style::default().fg(Color::White)),
                    ]));
                    lines.push(Line::from(""));
                    lines.push(Line::from(Span::styled("  Weights:", MUTED.add_modifier(Modifier::BOLD))));
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

    render_panel(frame, area, " Constraint Detail ", lines, border_style, scroll)
}

/// Render an objective detail panel. Returns the total content line count.
pub fn render_objective_detail(frame: &mut Frame, area: Rect, entry: &ObjectiveDiffEntry, border_style: Style, scroll: u16) -> usize {
    let mut lines = detail_header("Objective", &entry.name, entry.kind);

    lines.push(Line::from(Span::styled("  Coefficients:", MUTED.add_modifier(Modifier::BOLD))));

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

    render_panel(frame, area, " Objective Detail ", lines, border_style, scroll)
}

/// Render a combined view of old and new coefficient lists, annotating each
/// variable with its change status.
fn render_coeff_changes(
    lines: &mut Vec<Line<'_>>,
    changes: &[CoefficientChange],
    old_coefficients: &[lp_parser_rs::model::CoefficientOwned],
    new_coefficients: &[lp_parser_rs::model::CoefficientOwned],
) {
    // Column width for value formatting — wide enough for typical LP coefficients.
    const VAL_WIDTH: usize = 12;

    let rows = build_coeff_rows(changes, old_coefficients, new_coefficients);

    for row in &rows {
        let old_str = row.old_value.map_or_else(String::new, |v| format!("{v}"));
        let new_str = row.new_value.map_or_else(String::new, |v| format!("{v}"));

        match row.change_kind {
            Some(DiffKind::Added) => {
                lines.push(Line::from(vec![
                    Span::styled(format!("    {:<20}", row.variable), Style::default().fg(Color::Green)),
                    Span::styled(format!("{:>VAL_WIDTH$}", ""), Style::default()),
                    Span::styled(ARROW, MUTED),
                    Span::styled(format!("{new_str:<VAL_WIDTH$}"), Style::default().fg(Color::Green)),
                    Span::styled(" [added]", Style::default().fg(Color::Green)),
                ]));
            }
            Some(DiffKind::Removed) => {
                lines.push(Line::from(vec![
                    Span::styled(format!("    {:<20}", row.variable), Style::default().fg(Color::Red)),
                    Span::styled(format!("{old_str:>VAL_WIDTH$}"), Style::default().fg(Color::Red)),
                    Span::styled(ARROW, MUTED),
                    Span::styled(format!("{:VAL_WIDTH$}", ""), Style::default()),
                    Span::styled(" [removed]", Style::default().fg(Color::Red)),
                ]));
            }
            Some(DiffKind::Modified) => {
                lines.push(Line::from(vec![
                    Span::styled(format!("    {:<20}", row.variable), Style::default().fg(Color::Yellow)),
                    Span::styled(format!("{old_str:>VAL_WIDTH$}"), Style::default().fg(Color::Red)),
                    Span::styled(ARROW, MUTED),
                    Span::styled(format!("{new_str:<VAL_WIDTH$}"), Style::default().fg(Color::Green)),
                    Span::styled(" [modified]", Style::default().fg(Color::Yellow)),
                ]));
            }
            None => {
                lines.push(Line::from(vec![
                    Span::styled(format!("    {:<20}", row.variable), Style::default().fg(Color::DarkGray)),
                    Span::styled(format!("{old_str:>VAL_WIDTH$}"), Style::default().fg(Color::DarkGray)),
                    Span::styled(" (unchanged)", Style::default().fg(Color::DarkGray)),
                ]));
            }
        }
    }
}

/// Wrap `lines` in a bordered block with the given `title` and render it,
/// applying vertical scroll. Returns the total content line count.
fn render_panel(frame: &mut Frame, area: Rect, title: &'static str, lines: Vec<Line<'_>>, border_style: Style, scroll: u16) -> usize {
    let line_count = lines.len();
    let block = Block::default().borders(Borders::ALL).border_style(border_style).title(title);
    let paragraph = Paragraph::new(lines).block(block).scroll((scroll, 0));
    frame.render_widget(paragraph, area);
    line_count
}
