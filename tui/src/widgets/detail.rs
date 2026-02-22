//! Detail panel widget.
//!
//! Renders the full before/after breakdown for a single selected diff entry
//! (variables, constraints, objectives) in the detail pane.

use lp_parser_rs::model::VariableType;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::detail_model::build_coeff_rows;
use crate::diff_model::{
    CoefficientChange, ConstraintDiffDetail, ConstraintDiffEntry, DiffKind, ObjectiveDiffEntry, ResolvedCoefficient, ResolvedConstraint,
    VariableDiffEntry,
};
use crate::theme::theme;
use crate::widgets::{ARROW, bold_text, kind_colour, muted};

/// Horizontal rule used as a visual separator below the entry header.
fn rule<'a>() -> Line<'a> {
    Line::from(Span::styled("──────────────────────────────────────", muted()))
}

/// Build the common header lines for a detail panel: entity label, name, kind badge, and rule.
fn detail_header(entity_label: &str, name: &str, kind: DiffKind) -> Vec<Line<'static>> {
    vec![
        Line::from(vec![
            Span::styled(format!("{entity_label}: "), muted()),
            Span::styled(name.to_owned(), bold_text()),
            Span::styled(format!(" [{kind}]"), Style::default().fg(kind_colour(kind))),
        ]),
        rule(),
    ]
}

/// Extract (lower, upper) bounds from a `VariableType`, returning `None` for
/// bounds that don't apply to that type.
pub const fn variable_bounds(variable_type: &VariableType) -> (Option<f64>, Option<f64>) {
    match *variable_type {
        VariableType::LowerBound(lower) => (Some(lower), None),
        VariableType::UpperBound(upper) => (None, Some(upper)),
        VariableType::DoubleBound(lower, upper) => (Some(lower), Some(upper)),
        _ => (None, None),
    }
}

/// Format an optional bound value as a string for display.
pub fn fmt_bound(val: Option<f64>) -> String {
    val.map_or_else(|| "\u{2014}".to_owned(), |v| format!("{v}"))
}

/// Render type/bounds lines for an added or removed variable (single-side view).
fn render_variable_type_info(lines: &mut Vec<Line<'static>>, variable_type: &VariableType, style: Style) {
    let label = std::str::from_utf8(variable_type.as_ref()).unwrap_or("?");
    lines.push(Line::from(vec![Span::styled("  Type:   ", muted()), Span::styled(label.to_owned(), style)]));

    let (lower_bound, upper_bound) = variable_bounds(variable_type);
    if let Some(lower) = lower_bound {
        lines.push(Line::from(vec![Span::styled("  Lower:  ", muted()), Span::styled(format!("{lower}"), style)]));
    }
    if let Some(upper) = upper_bound {
        lines.push(Line::from(vec![Span::styled("  Upper:  ", muted()), Span::styled(format!("{upper}"), style)]));
    }
    if let (Some(lower), Some(upper)) = (lower_bound, upper_bound) {
        lines.push(Line::from(vec![Span::styled("  Range:  ", muted()), Span::styled(format!("{}", upper - lower), style)]));
    }
}

/// Render a variable detail panel. Returns the total content line count.
#[allow(clippy::too_many_lines)]
#[allow(clippy::similar_names)] // lower_bound/upper_bound share prefixes
pub fn render_variable_detail(frame: &mut Frame, area: Rect, entry: &VariableDiffEntry, border_style: Style, scroll: u16) -> usize {
    debug_assert!(area.width > 0 && area.height > 0, "variable detail area must be non-zero");
    let mut lines = detail_header("Variable", &entry.name, entry.kind);

    let t = theme();
    match entry.kind {
        DiffKind::Added => {
            let variable_type = entry.new_type.as_ref().expect("invariant: Added entry must have new_type");
            render_variable_type_info(&mut lines, variable_type, Style::default().fg(t.added));
        }
        DiffKind::Removed => {
            let variable_type = entry.old_type.as_ref().expect("invariant: Removed entry must have old_type");
            render_variable_type_info(&mut lines, variable_type, Style::default().fg(t.removed));
        }
        DiffKind::Modified => {
            let old = entry.old_type.as_ref().expect("invariant: Modified entry must have old_type");
            let new = entry.new_type.as_ref().expect("invariant: Modified entry must have new_type");
            let old_label = std::str::from_utf8(old.as_ref()).unwrap_or("?");
            let new_label = std::str::from_utf8(new.as_ref()).unwrap_or("?");

            if old_label == new_label {
                lines.push(Line::from(vec![
                    Span::styled("  Type:   ", muted()),
                    Span::styled(old_label.to_owned(), Style::default().fg(t.text)),
                    Span::styled(" (unchanged)", muted()),
                ]));
            } else {
                lines.push(Line::from(vec![
                    Span::styled("  Type:   ", muted()),
                    Span::styled(old_label.to_owned(), Style::default().fg(t.removed)),
                    Span::styled(ARROW, muted()),
                    Span::styled(new_label.to_owned(), Style::default().fg(t.added)),
                ]));
            }

            // Bounds comparison.
            let (old_lower, old_upper) = variable_bounds(old);
            let (new_lower, new_upper) = variable_bounds(new);

            if old_lower.is_some() || new_lower.is_some() {
                lines.push(Line::from(vec![
                    Span::styled("  Lower:  ", muted()),
                    Span::styled(fmt_bound(old_lower), Style::default().fg(t.removed)),
                    Span::styled(ARROW, muted()),
                    Span::styled(fmt_bound(new_lower), Style::default().fg(t.added)),
                ]));
            }

            if old_upper.is_some() || new_upper.is_some() {
                lines.push(Line::from(vec![
                    Span::styled("  Upper:  ", muted()),
                    Span::styled(fmt_bound(old_upper), Style::default().fg(t.removed)),
                    Span::styled(ARROW, muted()),
                    Span::styled(fmt_bound(new_upper), Style::default().fg(t.added)),
                ]));
            }

            let old_range = old_lower.zip(old_upper).map(|(lower, upper)| upper - lower);
            let new_range = new_lower.zip(new_upper).map(|(lower, upper)| upper - lower);
            if old_range.is_some() || new_range.is_some() {
                lines.push(Line::from(vec![
                    Span::styled("  Range:  ", muted()),
                    Span::styled(fmt_bound(old_range), Style::default().fg(t.removed)),
                    Span::styled(ARROW, muted()),
                    Span::styled(fmt_bound(new_range), Style::default().fg(t.added)),
                ]));
            }
        }
    }

    render_panel(frame, area, " Variable Detail ", lines, border_style, scroll)
}

/// Render a constraint detail panel. Returns the total content line count.
///
/// When `cached_rows` is `Some`, pre-built coefficient rows are reused instead of
/// rebuilding them each frame.
#[allow(clippy::too_many_lines)]
pub fn render_constraint_detail(
    frame: &mut Frame,
    area: Rect,
    entry: &ConstraintDiffEntry,
    border_style: Style,
    scroll: u16,
    cached_rows: Option<&[crate::detail_model::CoefficientRow]>,
) -> usize {
    debug_assert!(area.width > 0 && area.height > 0, "constraint detail area must be non-zero");
    let mut lines = detail_header("Constraint", &entry.name, entry.kind);

    let t = theme();

    // Render line number location if available.
    if entry.line_file1.is_some() || entry.line_file2.is_some() {
        let mut spans = vec![Span::styled("  Location: ", muted())];
        match (entry.line_file1, entry.line_file2) {
            (Some(l1), Some(l2)) => {
                // Present in both files (modified or unchanged).
                spans.push(Span::styled(format!("L{l1}"), Style::default().fg(t.accent)));
                spans.push(Span::styled(" / ", muted()));
                spans.push(Span::styled(format!("L{l2}"), Style::default().fg(t.accent)));
            }
            (Some(l1), None) => {
                // Removed: only in file 1.
                spans.push(Span::styled(format!("L{l1}"), Style::default().fg(t.removed)));
                spans.push(Span::styled(" (removed)", muted()));
            }
            (None, Some(l2)) => {
                // Added: only in file 2.
                spans.push(Span::styled(format!("L{l2}"), Style::default().fg(t.added)));
                spans.push(Span::styled(" (added)", muted()));
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
                    Span::styled("  Operator: ", muted()),
                    Span::styled(format!("{old_op}"), Style::default().fg(t.removed)),
                    Span::styled(ARROW, muted()),
                    Span::styled(format!("{new_op}"), Style::default().fg(t.added)),
                ]));
            }

            // RHS change.
            if rhs_change.is_some() {
                lines.push(Line::from(vec![
                    Span::styled("  RHS:      ", muted()),
                    Span::styled(format!("{old_rhs}"), Style::default().fg(t.removed)),
                    Span::styled(ARROW, muted()),
                    Span::styled(format!("{new_rhs}"), Style::default().fg(t.added)),
                ]));
            } else {
                lines.push(Line::from(vec![
                    Span::styled("  RHS:      ", muted()),
                    Span::styled(format!("{old_rhs} (unchanged)"), Style::default().fg(t.muted)),
                ]));
            }

            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled("  Coefficients:", muted().add_modifier(Modifier::BOLD))));

            if entry.kind == DiffKind::Modified {
                return render_constraint_side_by_side(
                    frame,
                    area,
                    lines,
                    coeff_changes,
                    old_coefficients,
                    new_coefficients,
                    border_style,
                    scroll,
                    cached_rows,
                );
            }

            let visible = coeff_visible_range(scroll, area, lines.len());
            render_coeff_changes(&mut lines, coeff_changes, old_coefficients, new_coefficients, cached_rows, Some(visible));
        }

        ConstraintDiffDetail::Sos { old_weights, new_weights, weight_changes, type_change } => {
            if let Some((old_type, new_type)) = type_change {
                lines.push(Line::from(vec![
                    Span::styled("  SOS Type: ", muted()),
                    Span::styled(format!("{old_type}"), Style::default().fg(t.removed)),
                    Span::styled(ARROW, muted()),
                    Span::styled(format!("{new_type}"), Style::default().fg(t.added)),
                ]));
            }

            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled("  Weights:", muted().add_modifier(Modifier::BOLD))));

            let visible = coeff_visible_range(scroll, area, lines.len());
            render_coeff_changes(&mut lines, weight_changes, old_weights, new_weights, cached_rows, Some(visible));
        }

        ConstraintDiffDetail::TypeChanged { old_summary, new_summary } => {
            lines.push(Line::from(vec![
                Span::styled("  Was:  ", muted()),
                Span::styled(old_summary.clone(), Style::default().fg(t.removed)),
            ]));
            lines
                .push(Line::from(vec![Span::styled("  Now:  ", muted()), Span::styled(new_summary.clone(), Style::default().fg(t.added))]));
        }

        ConstraintDiffDetail::AddedOrRemoved(constraint) => {
            let entry_colour = kind_colour(entry.kind);

            match constraint {
                ResolvedConstraint::Standard { coefficients, operator, rhs } => {
                    lines.push(Line::from(vec![
                        Span::styled("  Operator: ", muted()),
                        Span::styled(format!("{operator}"), Style::default().fg(t.text)),
                    ]));
                    lines.push(Line::from(vec![
                        Span::styled("  RHS:      ", muted()),
                        Span::styled(format!("{rhs}"), Style::default().fg(t.text)),
                    ]));
                    lines.push(Line::from(""));
                    lines.push(Line::from(Span::styled("  Coefficients:", muted().add_modifier(Modifier::BOLD))));
                    for coeff in coefficients {
                        lines.push(Line::from(vec![
                            Span::styled(format!("    {:<20}", coeff.name), Style::default().fg(entry_colour)),
                            Span::styled(format!("{}", coeff.value), Style::default().fg(entry_colour)),
                        ]));
                    }
                }
                ResolvedConstraint::Sos { sos_type, weights } => {
                    lines.push(Line::from(vec![
                        Span::styled("  SOS Type: ", muted()),
                        Span::styled(format!("{sos_type}"), Style::default().fg(t.text)),
                    ]));
                    lines.push(Line::from(""));
                    lines.push(Line::from(Span::styled("  Weights:", muted().add_modifier(Modifier::BOLD))));
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
///
/// When `cached_rows` is `Some`, pre-built coefficient rows are reused instead of
/// rebuilding them each frame.
pub fn render_objective_detail(
    frame: &mut Frame,
    area: Rect,
    entry: &ObjectiveDiffEntry,
    border_style: Style,
    scroll: u16,
    cached_rows: Option<&[crate::detail_model::CoefficientRow]>,
) -> usize {
    debug_assert!(area.width > 0 && area.height > 0, "objective detail area must be non-zero");
    let mut lines = detail_header("Objective", &entry.name, entry.kind);

    lines.push(Line::from(Span::styled("  Coefficients:", muted().add_modifier(Modifier::BOLD))));

    if entry.kind == DiffKind::Modified {
        let visible = coeff_visible_range(scroll, area, lines.len());
        render_coeff_changes(
            &mut lines,
            &entry.coeff_changes,
            &entry.old_coefficients,
            &entry.new_coefficients,
            cached_rows,
            Some(visible),
        );
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

/// Render a side-by-side old/new coefficient comparison for modified
/// standard constraints. Returns the total content line count.
///
/// Uses windowed rendering: only builds `Line` objects for coefficient rows
/// visible in the viewport, avoiding O(total_rows) allocations per frame.
#[allow(clippy::too_many_arguments)]
fn render_constraint_side_by_side(
    frame: &mut Frame,
    area: Rect,
    header_lines: Vec<Line<'_>>,
    coeff_changes: &[CoefficientChange],
    old_coefficients: &[ResolvedCoefficient],
    new_coefficients: &[ResolvedCoefficient],
    border_style: Style,
    scroll: u16,
    cached_rows: Option<&[crate::detail_model::CoefficientRow]>,
) -> usize {
    let t = theme();
    let header_line_count = header_lines.len();
    let block = Block::default().borders(Borders::ALL).border_style(border_style).title(" Constraint Detail ");
    let inner = block.inner(area);
    frame.render_widget(block, area);

    #[allow(clippy::cast_possible_truncation)]
    let header_height = header_line_count as u16;
    let v_chunks = Layout::vertical([Constraint::Length(header_height), Constraint::Min(0)]).split(inner);

    let header_paragraph = Paragraph::new(header_lines).scroll((scroll, 0));
    frame.render_widget(header_paragraph, v_chunks[0]);

    let owned_rows;
    let rows = if let Some(cached) = cached_rows {
        cached
    } else {
        owned_rows = build_coeff_rows(coeff_changes, old_coefficients, new_coefficients);
        &owned_rows
    };

    let coefficient_scroll = scroll.saturating_sub(header_height);
    let visible_height = v_chunks[1].height as usize;

    // Windowed rendering: only build Lines for visible coefficient rows.
    // The "Old"/"New" column header occupies the first line of the coefficient area.
    let column_header_lines: usize = 1;
    let data_skip = (coefficient_scroll as usize).saturating_sub(column_header_lines);
    let data_take = if coefficient_scroll == 0 { visible_height.saturating_sub(column_header_lines) } else { visible_height };

    let mut left_lines: Vec<Line<'_>> =
        vec![Line::from(Span::styled(" Old", Style::default().fg(t.removed).add_modifier(Modifier::BOLD | Modifier::UNDERLINED)))];
    let mut right_lines: Vec<Line<'_>> =
        vec![Line::from(Span::styled(" New", Style::default().fg(t.added).add_modifier(Modifier::BOLD | Modifier::UNDERLINED)))];

    // Placeholder lines for data rows scrolled above the viewport.
    for _ in 0..data_skip.min(rows.len()) {
        left_lines.push(Line::default());
        right_lines.push(Line::default());
    }

    // Build styled Lines only for the visible window.
    for row in rows.iter().skip(data_skip).take(data_take) {
        let (left_style, right_style, badge) = match row.change_kind {
            Some(DiffKind::Added) => (Style::default().fg(t.muted), Style::default().fg(t.added), " [+]"),
            Some(DiffKind::Removed) => (Style::default().fg(t.removed), Style::default().fg(t.muted), " [-]"),
            Some(DiffKind::Modified) => (Style::default().fg(t.removed), Style::default().fg(t.added), " [~]"),
            None => (Style::default().fg(t.muted), Style::default().fg(t.muted), ""),
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

    // Placeholder lines for data rows below the viewport.
    let built_data = data_skip.min(rows.len()) + rows.len().saturating_sub(data_skip).min(data_take);
    for _ in built_data..rows.len() {
        left_lines.push(Line::default());
        right_lines.push(Line::default());
    }

    let h_chunks = Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)]).split(v_chunks[1]);

    let left_paragraph = Paragraph::new(left_lines).scroll((coefficient_scroll, 0));
    let right_paragraph = Paragraph::new(right_lines).scroll((coefficient_scroll, 0));
    frame.render_widget(left_paragraph, h_chunks[0]);
    frame.render_widget(right_paragraph, h_chunks[1]);

    header_line_count + 1 + rows.len()
}

/// Compute the visible range of coefficient rows for windowed rendering.
///
/// Returns `(first_visible_row, max_visible_rows)`. When the scroll position
/// is within the header area, `first_visible_row` is 0 and `max_visible_rows`
/// accounts for header lines still occupying the viewport.
const fn coeff_visible_range(scroll: u16, area: Rect, header_line_count: usize) -> (usize, usize) {
    let inner_height = area.height.saturating_sub(2) as usize; // subtract borders
    let scroll_usize = scroll as usize;
    let first_visible = scroll_usize.saturating_sub(header_line_count);
    let visible_space =
        if scroll_usize >= header_line_count { inner_height } else { inner_height.saturating_sub(header_line_count - scroll_usize) };
    // +1 for partially visible lines at the bottom edge.
    (first_visible, visible_space + 1)
}

/// Render a combined view of old and new coefficient lists, annotating each
/// variable with its change status.
///
/// When `visible_range` is `Some((first, count))`, only builds `Line` objects
/// for the visible window, inserting cheap placeholder lines for rows above and
/// below the viewport. This avoids `O(total_rows)` `format!` allocations per frame.
fn render_coeff_changes(
    lines: &mut Vec<Line<'_>>,
    changes: &[CoefficientChange],
    old_coefficients: &[ResolvedCoefficient],
    new_coefficients: &[ResolvedCoefficient],
    cached_rows: Option<&[crate::detail_model::CoefficientRow]>,
    visible_range: Option<(usize, usize)>,
) {
    // Column width for value formatting — wide enough for typical LP coefficients.
    const VAL_WIDTH: usize = 12;

    let t = theme();

    let owned_rows;
    let rows = if let Some(cached) = cached_rows {
        cached
    } else {
        owned_rows = build_coeff_rows(changes, old_coefficients, new_coefficients);
        &owned_rows
    };

    let (skip, take) = visible_range.unwrap_or((0, rows.len()));

    // Placeholder lines for coefficient rows scrolled above the viewport.
    let placeholder_before = skip.min(rows.len());
    for _ in 0..placeholder_before {
        lines.push(Line::default());
    }

    // Build styled Lines only for the visible window.
    let visible_count = rows.len().saturating_sub(skip).min(take);
    for row in rows.iter().skip(skip).take(take) {
        let old_str = row.old_value.map_or_else(String::new, |v| format!("{v}"));
        let new_str = row.new_value.map_or_else(String::new, |v| format!("{v}"));

        match row.change_kind {
            Some(DiffKind::Added) => {
                lines.push(Line::from(vec![
                    Span::styled(format!("    {:<20}", row.variable), Style::default().fg(t.added)),
                    Span::styled(format!("{:>VAL_WIDTH$}", ""), Style::default()),
                    Span::styled(ARROW, muted()),
                    Span::styled(format!("{new_str:<VAL_WIDTH$}"), Style::default().fg(t.added)),
                    Span::styled(" [added]", Style::default().fg(t.added)),
                ]));
            }
            Some(DiffKind::Removed) => {
                lines.push(Line::from(vec![
                    Span::styled(format!("    {:<20}", row.variable), Style::default().fg(t.removed)),
                    Span::styled(format!("{old_str:>VAL_WIDTH$}"), Style::default().fg(t.removed)),
                    Span::styled(ARROW, muted()),
                    Span::styled(format!("{:VAL_WIDTH$}", ""), Style::default()),
                    Span::styled(" [removed]", Style::default().fg(t.removed)),
                ]));
            }
            Some(DiffKind::Modified) => {
                lines.push(Line::from(vec![
                    Span::styled(format!("    {:<20}", row.variable), Style::default().fg(t.modified)),
                    Span::styled(format!("{old_str:>VAL_WIDTH$}"), Style::default().fg(t.removed)),
                    Span::styled(ARROW, muted()),
                    Span::styled(format!("{new_str:<VAL_WIDTH$}"), Style::default().fg(t.added)),
                    Span::styled(" [modified]", Style::default().fg(t.modified)),
                ]));
            }
            None => {
                lines.push(Line::from(vec![
                    Span::styled(format!("    {:<20}", row.variable), Style::default().fg(t.muted)),
                    Span::styled(format!("{old_str:>VAL_WIDTH$}"), Style::default().fg(t.muted)),
                    Span::styled(" (unchanged)", Style::default().fg(t.muted)),
                ]));
            }
        }
    }

    // Placeholder lines for coefficient rows below the viewport.
    let after_count = rows.len().saturating_sub(placeholder_before + visible_count);
    for _ in 0..after_count {
        lines.push(Line::default());
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
