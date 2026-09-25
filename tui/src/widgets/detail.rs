//! Detail panel widget.
//!
//! Renders the full breakdown of the selected variable, constraint or
//! objective: before and after in diff mode, the entry as written in inspect
//! mode.

use lp_parser_rs::interner::NameInterner;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::detail_model::build_coeff_rows;
use crate::diff_model::{
    CoefficientChange, ConstraintDiffDetail, ConstraintDiffEntry, DiffKind, ObjectiveDiffEntry, ResolvedCoefficient, ResolvedConstraint,
    VarSpec, VariableDiffEntry,
};
use crate::theme::theme;
use crate::widgets::{ARROW, bold_text, fit_number, kind_colour, muted, panel_block, text, truncate_middle};

/// Build the panel title: entity label, entry name (truncated and bold), the
/// diff-kind badge in its kind colour, and the raw-view toggle hint where the
/// raw side-by-side view applies (diff-mode constraints and objectives).
///
/// The border title is the panel's whole header. The builders below emit a
/// matching header *line* for the plain-text yank, which has no border to carry
/// it; on screen that line would only say the same thing twice, so the render
/// paths leave it out.
///
/// The name is sized to the panel's `width` rather than a fixed cap: it takes
/// whatever the label, badge and a stub of border rule leave, and the raw-view
/// hint is the first thing dropped when that is not enough.
fn detail_title(entity_label: &str, name: &str, kind: Option<DiffKind>, raw_hint: bool, width: u16) -> Line<'static> {
    /// Corners plus at least one column of rule after the title.
    const BORDER: usize = 3;
    /// Below this the name is unreadable, so the hint gives way first.
    const MIN_NAME: usize = 12;
    const RAW_HINT: &str = " \u{b7} r:raw";

    let label = format!(" {entity_label}: ");
    let badge = kind.map(|kind| format!(" [{kind}]")).unwrap_or_default();
    let fixed = BORDER + label.chars().count() + badge.chars().count() + 1;
    let room = (width as usize).saturating_sub(fixed);
    let hint_width = RAW_HINT.chars().count();
    let raw_hint = raw_hint && room >= hint_width + MIN_NAME.min(name.chars().count());
    let room = if raw_hint { room - hint_width } else { room };

    let name = truncate_middle(name, room.max(2));
    let mut spans = vec![Span::styled(label, muted()), Span::styled(name.into_owned(), bold_text())];
    if let Some(kind) = kind {
        spans.push(Span::styled(badge, Style::default().fg(kind_colour(kind))));
    }
    if raw_hint {
        spans.push(Span::styled(RAW_HINT, muted()));
    }
    spans.push(Span::raw(" "));
    Line::from(spans)
}

/// Width of a coefficient table's name column: the longest name, capped to
/// what `pane_width` leaves after the indent, a one-column gap and the
/// `reserved` value columns. `None` (the plain-text yank) keeps names whole.
fn name_column_width(longest: usize, pane_width: Option<u16>, reserved: usize) -> usize {
    /// Borders, the four-column indent, and the gap before the value.
    const CHROME: usize = 2 + 4 + 1;
    /// Narrower than this the name column is useless; the row overflows instead.
    const MIN_NAME: usize = 8;
    match pane_width {
        None => longest,
        Some(width) => longest.min((width as usize).saturating_sub(CHROME + reserved)).max(MIN_NAME.min(longest)),
    }
}

/// A `    name ` cell padded to `width`, keeping at least one space before the
/// value that follows.
fn name_cell(name: &str, width: usize) -> String {
    let name = truncate_middle(name, width.max(2));
    format!("    {name:<width$} ")
}

/// Longest name among `coefficients`, for [`name_column_width`].
fn longest_coefficient_name(coefficients: &[ResolvedCoefficient], interner: &NameInterner) -> usize {
    coefficients.iter().map(|coeff| interner.resolve(coeff.name).chars().count()).max().unwrap_or(0)
}

/// Build the header line for a yanked detail panel: entity label, name, kind
/// badge, then a hairline rule running out to the same column the section
/// headings in every other pane use.
///
/// Only the plain-text yank uses this — on screen [`detail_title`] carries it.
pub fn detail_header(entity_label: &str, name: &str, kind: DiffKind) -> Vec<Line<'static>> {
    let badge = format!(" [{kind}] ");
    let used = 2 + entity_label.chars().count() + 2 + name.chars().count() + badge.chars().count();
    vec![Line::from(vec![
        Span::styled(format!("  {entity_label}: "), muted()),
        Span::styled(name.to_owned(), bold_text()),
        Span::styled(badge, Style::default().fg(kind_colour(kind))),
        Span::styled(crate::widgets::rule_str(HEADER_RULE_END.saturating_sub(used)), muted()),
    ])]
}

/// Room the single value after a coefficient name is given when sizing the
/// name column.
const VALUE_COLUMN: usize = 12;

/// Column at which the detail header's trailing rule stops.
const HEADER_RULE_END: usize = 70;

/// Format an optional bound value as a string for display.
pub fn fmt_bound(val: Option<f64>) -> String {
    val.map_or_else(|| "\u{2014}".to_owned(), |v| format!("{v}"))
}

/// Render type/bounds lines for an added or removed variable (single-side view).
fn render_variable_type_info(lines: &mut Vec<Line<'static>>, spec: &VarSpec, style: Style) {
    lines.push(Line::from(vec![Span::styled("  Type:   ", muted()), Span::styled(spec.kind.to_string(), style)]));

    let (lower_bound, upper_bound) = (spec.bounds.lower, spec.bounds.upper);
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

/// Build the content lines of a variable detail panel.
#[allow(clippy::too_many_lines)]
#[allow(clippy::similar_names)] // lower_bound/upper_bound share prefixes
pub fn build_variable_detail(entry: &VariableDiffEntry) -> Vec<Line<'static>> {
    let mut lines: Vec<Line<'static>> = Vec::new();

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
            // Kind only — the bounds get their own rows immediately below.
            let old_label = old.kind.to_string();
            let new_label = new.kind.to_string();

            if old_label == new_label {
                lines.push(Line::from(vec![
                    Span::styled("  Type:   ", muted()),
                    Span::styled(old_label.clone(), Style::default().fg(t.text)),
                    Span::styled(" (unchanged)", muted()),
                ]));
            } else {
                lines.push(Line::from(vec![
                    Span::styled("  Type:   ", muted()),
                    Span::styled(old_label.clone(), Style::default().fg(t.removed)),
                    Span::styled(ARROW, muted()),
                    Span::styled(new_label.clone(), Style::default().fg(t.added)),
                ]));
            }

            // Bounds comparison.
            let (old_lower, old_upper) = (old.bounds.lower, old.bounds.upper);
            let (new_lower, new_upper) = (new.bounds.lower, new.bounds.upper);

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
        DiffKind::Renamed => {
            // Rename detection applies to constraints only; variables never carry Renamed.
            debug_assert!(false, "variable entry cannot be Renamed");
        }
    }

    lines
}

/// Render a variable detail panel. Returns the total content line count.
pub fn render_variable_detail(frame: &mut Frame, area: Rect, entry: &VariableDiffEntry, border_style: Style, scroll: usize) -> usize {
    // A zero-sized area is an environmental condition (shrunken terminal), not a
    // programming error: drawing into it is a no-op.
    if area.width == 0 || area.height == 0 {
        return 0;
    }
    let content = PanelLines::whole(build_variable_detail(entry));
    render_windowed_panel(
        frame,
        area,
        detail_title("Variable", &entry.name, Some(entry.kind), false, area.width),
        content,
        border_style,
        scroll,
    )
}

/// How a constraint detail continues after its header block.
enum ConstraintBody<'a> {
    /// Coefficient (or SOS weight) rows to append. `side_by_side` requests the
    /// two-column old/new comparison, which only the styled renderer can honour.
    Rows { changes: &'a [CoefficientChange], old: &'a [ResolvedCoefficient], new: &'a [ResolvedCoefficient], side_by_side: bool },
    /// The header block is the entire panel.
    Complete,
}

/// Build the header block of a constraint detail panel and say how it continues.
///
/// Shared by the styled renderer (which windows the coefficient rows to the
/// viewport, or splits them into two columns) and by the plain-text builder
/// (which appends all of them unified).
#[allow(clippy::too_many_lines)]
fn constraint_detail_parts<'a>(
    entry: &'a ConstraintDiffEntry,
    interner: &NameInterner,
    pane_width: Option<u16>,
) -> (Vec<Line<'static>>, ConstraintBody<'a>) {
    let mut lines: Vec<Line<'static>> = Vec::new();

    let t = theme();

    // Renamed entries: show the name the constraint carried in the first file.
    if let Some(old_name) = &entry.renamed_from {
        lines.push(Line::from(vec![
            Span::styled("  Renamed from: ", muted()),
            Span::styled(old_name.clone(), Style::default().fg(t.accent).add_modifier(Modifier::BOLD)),
        ]));
    }

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

    // Order-only badge.
    if entry.order_only {
        lines.push(Line::from(Span::styled(
            "  \u{26a0} Order Changed \u{2014} coefficients are identical but appear in a different order",
            Style::default().fg(t.modified),
        )));
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
            order_changed,
            ..
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

            // Note about order change when there are also value changes.
            if *order_changed && !entry.order_only {
                lines.push(Line::from(Span::styled("  Note: coefficient order also differs", Style::default().fg(t.modified))));
            }

            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled("  Coefficients:", muted().add_modifier(Modifier::BOLD))));

            return (
                lines,
                ConstraintBody::Rows {
                    changes: coeff_changes,
                    old: old_coefficients,
                    new: new_coefficients,
                    side_by_side: entry.kind == DiffKind::Modified,
                },
            );
        }

        ConstraintDiffDetail::Sos { old_weights, new_weights, weight_changes, type_change, order_changed, .. } => {
            if let Some((old_type, new_type)) = type_change {
                lines.push(Line::from(vec![
                    Span::styled("  SOS Type: ", muted()),
                    Span::styled(format!("{old_type}"), Style::default().fg(t.removed)),
                    Span::styled(ARROW, muted()),
                    Span::styled(format!("{new_type}"), Style::default().fg(t.added)),
                ]));
            }

            if *order_changed && !entry.order_only {
                lines.push(Line::from(Span::styled("  Note: weight order also differs", Style::default().fg(t.modified))));
            }

            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled("  Weights:", muted().add_modifier(Modifier::BOLD))));

            return (lines, ConstraintBody::Rows { changes: weight_changes, old: old_weights, new: new_weights, side_by_side: false });
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
                    let name_w = name_column_width(longest_coefficient_name(coefficients, interner), pane_width, VALUE_COLUMN);
                    for coeff in coefficients {
                        let name = interner.resolve(coeff.name);
                        lines.push(Line::from(vec![
                            Span::styled(name_cell(name, name_w), Style::default().fg(entry_colour)),
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
                    let name_w = name_column_width(longest_coefficient_name(weights, interner), pane_width, VALUE_COLUMN);
                    for w in weights {
                        let name = interner.resolve(w.name);
                        lines.push(Line::from(vec![
                            Span::styled(name_cell(name, name_w), Style::default().fg(entry_colour)),
                            Span::styled(format!("{}", w.value), Style::default().fg(entry_colour)),
                        ]));
                    }
                }
            }
        }
    }

    (lines, ConstraintBody::Complete)
}

/// Build the content lines of a constraint detail panel, with every coefficient
/// row present and unified into a single column.
pub fn build_constraint_detail(
    entry: &ConstraintDiffEntry,
    cached_rows: Option<&[crate::detail_model::CoefficientRow]>,
    interner: &NameInterner,
) -> Vec<Line<'static>> {
    let (mut lines, body) = constraint_detail_parts(entry, interner, None);
    if let ConstraintBody::Rows { changes, old, new, .. } = body {
        let (skipped, _) = render_coeff_changes(&mut lines, changes, old, new, cached_rows, None, None, interner);
        debug_assert_eq!(skipped, 0, "an unwindowed render leaves nothing out");
    }
    lines
}

/// Render a constraint detail panel. Returns the total content line count.
///
/// When `cached_rows` is `Some`, pre-built coefficient rows are reused instead of
/// rebuilding them each frame.
pub fn render_constraint_detail(
    frame: &mut Frame,
    area: Rect,
    entry: &ConstraintDiffEntry,
    border_style: Style,
    scroll: usize,
    cached_rows: Option<&[crate::detail_model::CoefficientRow]>,
    interner: &NameInterner,
) -> usize {
    // A zero-sized area is an environmental condition (shrunken terminal), not a
    // programming error: drawing into it is a no-op.
    if area.width == 0 || area.height == 0 {
        return 0;
    }
    let (mut lines, body) = constraint_detail_parts(entry, interner, Some(area.width));
    if let ConstraintBody::Rows { changes, old, new, side_by_side } = body {
        if side_by_side {
            return render_constraint_side_by_side(
                frame,
                area,
                detail_title("Constraint", &entry.name, Some(entry.kind), true, area.width),
                lines,
                changes,
                old,
                new,
                border_style,
                scroll,
                cached_rows,
                interner,
            );
        }
        let visible = coeff_visible_range(scroll, area, lines.len());
        let header = lines.len();
        let (skipped, rows) = render_coeff_changes(&mut lines, changes, old, new, cached_rows, Some(visible), Some(area.width), interner);
        let content = PanelLines { lines, skipped, total: header + rows };
        return render_windowed_panel(
            frame,
            area,
            detail_title("Constraint", &entry.name, Some(entry.kind), true, area.width),
            content,
            border_style,
            scroll,
        );
    }
    let content = PanelLines::whole(lines);
    render_windowed_panel(
        frame,
        area,
        detail_title("Constraint", &entry.name, Some(entry.kind), true, area.width),
        content,
        border_style,
        scroll,
    )
}

/// Build the content lines of an objective detail panel, with every
/// coefficient row present (the plain-text yank).
pub fn build_objective_detail(
    entry: &ObjectiveDiffEntry,
    cached_rows: Option<&[crate::detail_model::CoefficientRow]>,
    interner: &NameInterner,
) -> Vec<Line<'static>> {
    objective_detail_lines(entry, cached_rows, interner, None).lines
}

/// The content of an objective detail panel.
///
/// `viewport` is the `(scroll, area)` the panel will be drawn into; when given,
/// `Line`s are only built for the coefficient rows actually visible. `None`
/// builds them all.
fn objective_detail_lines(
    entry: &ObjectiveDiffEntry,
    cached_rows: Option<&[crate::detail_model::CoefficientRow]>,
    interner: &NameInterner,
    viewport: Option<(usize, Rect)>,
) -> PanelLines {
    let t = theme();
    let mut lines: Vec<Line<'static>> = Vec::new();

    if entry.order_only {
        lines.push(Line::from(Span::styled(
            "  \u{26a0} Order Changed \u{2014} coefficients are identical but appear in a different order",
            Style::default().fg(t.modified),
        )));
    } else if entry.order_changed {
        lines.push(Line::from(Span::styled("  Note: coefficient order also differs", Style::default().fg(t.modified))));
    }

    push_objective_extras(&mut lines, entry);

    lines.push(Line::from(Span::styled("  Coefficients:", muted().add_modifier(Modifier::BOLD))));

    let window = viewport.map(|(scroll, area)| coeff_visible_range(scroll, area, lines.len()));
    if entry.kind == DiffKind::Modified {
        let header = lines.len();
        let (skipped, rows) = render_coeff_changes(
            &mut lines,
            &entry.coeff_changes,
            &entry.old_coefficients,
            &entry.new_coefficients,
            cached_rows,
            window,
            viewport.map(|(_, area)| area.width),
            interner,
        );
        PanelLines { lines, skipped, total: header + rows }
    } else {
        let coeffs = if entry.kind == DiffKind::Added { &entry.new_coefficients } else { &entry.old_coefficients };
        let style = Style::default().fg(kind_colour(entry.kind));
        let total = lines.len() + coeffs.len();
        let skipped = push_coefficient_rows(&mut lines, coeffs, interner, viewport.map(|(_, area)| area.width), style, window);
        PanelLines { lines, skipped, total }
    }
}

/// Push the objective's constant and quadratic part, when they matter: the
/// change for a modified objective, the one side's value for an added or
/// removed one. Nothing is pushed for a plain linear objective with no constant.
fn push_objective_extras(lines: &mut Vec<Line<'static>>, entry: &ObjectiveDiffEntry) {
    let t = theme();
    let extras = &entry.extras;
    let term_count = |terms: &[(String, String, f64)]| crate::format::plural(terms.len(), "term", "terms");
    if entry.kind == DiffKind::Modified {
        if extras.constant_changed {
            lines.push(Line::from(vec![
                Span::styled("  Constant: ", muted()),
                Span::styled(format!("{}", extras.old_constant), Style::default().fg(t.removed)),
                Span::styled(ARROW, muted()),
                Span::styled(format!("{}", extras.new_constant), Style::default().fg(t.added)),
            ]));
        }
        if extras.quadratic_changed {
            lines.push(Line::from(vec![
                Span::styled("  Quadratic: ", muted()),
                Span::styled(format!("changed, {}", term_count(&extras.old_quadratic)), Style::default().fg(t.removed)),
                Span::styled(ARROW, muted()),
                Span::styled(term_count(&extras.new_quadratic), Style::default().fg(t.added)),
            ]));
        }
        return;
    }
    let (constant, quadratic) = if entry.kind == DiffKind::Added {
        (extras.new_constant, &extras.new_quadratic)
    } else {
        (extras.old_constant, &extras.old_quadratic)
    };
    let colour = kind_colour(entry.kind);
    if constant != 0.0 {
        lines.push(Line::from(vec![
            Span::styled("  Constant: ", muted()),
            Span::styled(format!("{constant}"), Style::default().fg(colour)),
        ]));
    }
    if !quadratic.is_empty() {
        lines.push(Line::from(vec![
            Span::styled("  Quadratic: ", muted()),
            Span::styled(term_count(quadratic), Style::default().fg(colour)),
        ]));
    }
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
    scroll: usize,
    cached_rows: Option<&[crate::detail_model::CoefficientRow]>,
    interner: &NameInterner,
) -> usize {
    // A zero-sized area is an environmental condition (shrunken terminal), not a
    // programming error: drawing into it is a no-op.
    if area.width == 0 || area.height == 0 {
        return 0;
    }
    let content = objective_detail_lines(entry, cached_rows, interner, Some((scroll, area)));
    render_windowed_panel(
        frame,
        area,
        detail_title("Objective", &entry.name, Some(entry.kind), true, area.width),
        content,
        border_style,
        scroll,
    )
}

/// Build the neutral header line for a yanked inspect detail panel: entity
/// label, name (bold), and the trailing rule. No diff badge — inspect shows a
/// single model. Only the plain-text yank uses this; on screen the panel title
/// carries it.
pub fn inspect_header(entity_label: &str, name: &str) -> Vec<Line<'static>> {
    let used = 2 + entity_label.chars().count() + 3 + name.chars().count();
    vec![Line::from(vec![
        Span::styled(format!("  {entity_label}: "), muted()),
        Span::styled(name.to_owned(), bold_text()),
        Span::styled(format!(" {}", crate::widgets::rule_str(HEADER_RULE_END.saturating_sub(used))), muted()),
    ])]
}

/// Build the content lines of an inspect (single-file) variable detail panel:
/// type and bounds, all in the neutral text colour.
pub fn build_inspect_variable(entry: &VariableDiffEntry) -> Vec<Line<'static>> {
    let mut lines: Vec<Line<'static>> = Vec::new();
    // Inspect entries always carry the single-file value on the `new` side.
    if let Some(variable_type) = entry.new_type.as_ref() {
        render_variable_type_info(&mut lines, variable_type, text());
    }
    lines
}

/// Render an inspect (single-file) variable detail panel. Returns the total content line count.
pub fn render_inspect_variable(frame: &mut Frame, area: Rect, entry: &VariableDiffEntry, border_style: Style, scroll: usize) -> usize {
    if area.width == 0 || area.height == 0 {
        return 0;
    }
    let content = PanelLines::whole(build_inspect_variable(entry));
    render_windowed_panel(frame, area, detail_title("Variable", &entry.name, None, false, area.width), content, border_style, scroll)
}

/// Build the content lines of an inspect (single-file) constraint detail panel:
/// operator, RHS, and coefficients (or SOS type and weights), neutrally coloured.
pub fn build_inspect_constraint(entry: &ConstraintDiffEntry, interner: &NameInterner, pane_width: Option<u16>) -> Vec<Line<'static>> {
    inspect_constraint_lines(entry, interner, pane_width, None).lines
}

/// The content of an inspect constraint panel, windowed to `viewport` when
/// given (see [`objective_detail_lines`]).
fn inspect_constraint_lines(
    entry: &ConstraintDiffEntry,
    interner: &NameInterner,
    pane_width: Option<u16>,
    viewport: Option<(usize, Rect)>,
) -> PanelLines {
    let t = theme();
    let mut lines: Vec<Line<'static>> = Vec::new();
    let mut skipped = 0;
    // Set by the branches with rows: the height with every row present.
    let mut total = None;

    // Source location (inspect builds the model on the `new`/file-2 side).
    if let Some(line) = entry.line_file2.or(entry.line_file1) {
        lines
            .push(Line::from(vec![Span::styled("  Location: ", muted()), Span::styled(format!("L{line}"), Style::default().fg(t.accent))]));
    }

    match &entry.detail {
        ConstraintDiffDetail::AddedOrRemoved(ResolvedConstraint::Standard { coefficients, operator, rhs }) => {
            lines.push(Line::from(vec![Span::styled("  Operator: ", muted()), Span::styled(format!("{operator}"), text())]));
            lines.push(Line::from(vec![Span::styled("  RHS:      ", muted()), Span::styled(format!("{rhs}"), text())]));
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled("  Coefficients:", muted().add_modifier(Modifier::BOLD))));
            let window = viewport.map(|(scroll, area)| coeff_visible_range(scroll, area, lines.len()));
            total = Some(lines.len() + coefficients.len());
            skipped = push_coefficient_rows(&mut lines, coefficients, interner, pane_width, text(), window);
        }
        ConstraintDiffDetail::AddedOrRemoved(ResolvedConstraint::Sos { sos_type, weights }) => {
            lines.push(Line::from(vec![Span::styled("  SOS Type: ", muted()), Span::styled(format!("{sos_type}"), text())]));
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled("  Weights:", muted().add_modifier(Modifier::BOLD))));
            let window = viewport.map(|(scroll, area)| coeff_visible_range(scroll, area, lines.len()));
            total = Some(lines.len() + weights.len());
            skipped = push_coefficient_rows(&mut lines, weights, interner, pane_width, text(), window);
        }
        // Inspect always diffs against an empty base, so every constraint is an
        // AddedOrRemoved single-side entry; other variants cannot occur.
        _ => {
            debug_assert!(false, "inspect constraint detail must be AddedOrRemoved");
            lines.push(Line::from(Span::styled("  (unavailable)", muted())));
        }
    }

    let total = total.unwrap_or(lines.len());
    PanelLines { lines, skipped, total }
}

/// Render an inspect (single-file) constraint detail panel. Returns the total content line count.
pub fn render_inspect_constraint(
    frame: &mut Frame,
    area: Rect,
    entry: &ConstraintDiffEntry,
    border_style: Style,
    scroll: usize,
    interner: &NameInterner,
) -> usize {
    if area.width == 0 || area.height == 0 {
        return 0;
    }
    let content = inspect_constraint_lines(entry, interner, Some(area.width), Some((scroll, area)));
    render_windowed_panel(frame, area, detail_title("Constraint", &entry.name, None, false, area.width), content, border_style, scroll)
}

/// Build the content lines of an inspect (single-file) objective detail panel:
/// coefficients, neutral.
pub fn build_inspect_objective(entry: &ObjectiveDiffEntry, interner: &NameInterner, pane_width: Option<u16>) -> Vec<Line<'static>> {
    inspect_objective_lines(entry, interner, pane_width, None).lines
}

/// The content of an inspect objective panel, windowed to `viewport` when
/// given (see [`objective_detail_lines`]).
fn inspect_objective_lines(
    entry: &ObjectiveDiffEntry,
    interner: &NameInterner,
    pane_width: Option<u16>,
    viewport: Option<(usize, Rect)>,
) -> PanelLines {
    let mut lines: Vec<Line<'static>> = Vec::new();
    lines.push(Line::from(Span::styled("  Coefficients:", muted().add_modifier(Modifier::BOLD))));
    let window = viewport.map(|(scroll, area)| coeff_visible_range(scroll, area, lines.len()));
    let total = lines.len() + entry.new_coefficients.len();
    let skipped = push_coefficient_rows(&mut lines, &entry.new_coefficients, interner, pane_width, text(), window);
    PanelLines { lines, skipped, total }
}

/// Render an inspect (single-file) objective detail panel. Returns the total content line count.
pub fn render_inspect_objective(
    frame: &mut Frame,
    area: Rect,
    entry: &ObjectiveDiffEntry,
    border_style: Style,
    scroll: usize,
    interner: &NameInterner,
) -> usize {
    if area.width == 0 || area.height == 0 {
        return 0;
    }
    let content = inspect_objective_lines(entry, interner, Some(area.width), Some((scroll, area)));
    render_windowed_panel(frame, area, detail_title("Objective", &entry.name, None, false, area.width), content, border_style, scroll)
}

/// Append `name  value` rows in `style` for a resolved coefficient/weight
/// list: all of them, or only those in `window` (`(first, count)`, as from
/// [`coeff_visible_range`]). Returns how many rows before the window were left
/// out.
fn push_coefficient_rows(
    lines: &mut Vec<Line<'static>>,
    coefficients: &[ResolvedCoefficient],
    interner: &NameInterner,
    pane_width: Option<u16>,
    style: Style,
    window: Option<(usize, usize)>,
) -> usize {
    let name_w = name_column_width(longest_coefficient_name(coefficients, interner), pane_width, VALUE_COLUMN);
    let (first, count) = window.unwrap_or((0, coefficients.len()));
    let skipped = first.min(coefficients.len());
    for coeff in coefficients.iter().skip(skipped).take(count) {
        let name = interner.resolve(coeff.name);
        lines.push(Line::from(vec![Span::styled(name_cell(name, name_w), style), Span::styled(format!("{}", coeff.value), style)]));
    }
    skipped
}

/// Detail-panel content windowed to the viewport: the header lines and the
/// coefficient rows in view, with `skipped` rows left out between the two.
struct PanelLines {
    lines: Vec<Line<'static>>,
    /// Rows left out ahead of the first one in `lines` after the header.
    skipped: usize,
    /// Content height as if every row were present, for the scroll bound.
    total: usize,
}

impl PanelLines {
    /// Content with nothing left out.
    fn whole(lines: Vec<Line<'static>>) -> Self {
        let total = lines.len();
        Self { lines, skipped: 0, total }
    }
}

/// Render windowed content in a bordered panel, scrolled to `scroll`. The rows
/// left out above the window stand in for that much of the scroll, and the
/// rest is dropped from the front here, so the offset is never narrowed to the
/// `u16` a `Paragraph` scrolls by. Returns the total content line count.
fn render_windowed_panel(
    frame: &mut Frame,
    area: Rect,
    title: Line<'static>,
    mut content: PanelLines,
    border_style: Style,
    scroll: usize,
) -> usize {
    debug_assert!(content.skipped <= scroll, "only rows scrolled past are left out");
    debug_assert!(content.lines.len() + content.skipped <= content.total, "the window cannot hold more than the content");
    let local_scroll = (scroll - content.skipped).min(content.lines.len());
    content.lines.drain(..local_scroll);
    render_panel(frame, area, title, content.lines, border_style, 0);
    content.total
}

/// Render a side-by-side old/new coefficient comparison for modified
/// standard constraints. Returns the total content line count.
///
/// One row per variable: the name, the old and new values, and — where both
/// sides exist and differ — the change `Δ` and relative change `%Δ`. Values
/// are aligned on their decimal points, and a side the variable is missing
/// from shows a dimmed `—`. The optional `%Δ` then `Δ` columns give way on a
/// narrow pane before the name is cut below [`SideBySideColumns::MIN_NAME`].
///
/// Uses windowed rendering: only builds `Line` objects for coefficient rows
/// visible in the viewport, avoiding `O(total_rows)` allocations per frame.
/// Column widths come from the visible rows too.
#[allow(clippy::too_many_arguments)]
fn render_constraint_side_by_side(
    frame: &mut Frame,
    area: Rect,
    title: Line<'static>,
    header_lines: Vec<Line<'static>>,
    coeff_changes: &[CoefficientChange],
    old_coefficients: &[ResolvedCoefficient],
    new_coefficients: &[ResolvedCoefficient],
    border_style: Style,
    scroll: usize,
    cached_rows: Option<&[crate::detail_model::CoefficientRow]>,
    interner: &NameInterner,
) -> usize {
    let t = theme();
    let header_line_count = header_lines.len();
    let block = panel_block(border_style).title(title);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    #[allow(clippy::cast_possible_truncation)]
    let header_height = header_line_count as u16;
    // The header shrinks as it scrolls away, handing its rows to the
    // coefficients: a fixed-height header chunk would hide the last
    // `header_height` rows at maximum scroll.
    // Below the header height, so it fits the header's own `u16`.
    let header_scroll = u16::try_from(scroll.min(header_line_count)).unwrap_or(header_height);
    let header_visible = header_height - header_scroll;
    let v_chunks = Layout::vertical([Constraint::Length(header_visible), Constraint::Min(0)]).split(inner);

    let header_paragraph = Paragraph::new(header_lines).scroll((header_scroll, 0));
    frame.render_widget(header_paragraph, v_chunks[0]);

    let owned_rows;
    let rows = if let Some(cached) = cached_rows {
        cached
    } else {
        owned_rows = build_coeff_rows(coeff_changes, old_coefficients, new_coefficients, interner);
        &owned_rows
    };

    let coefficient_scroll = scroll.saturating_sub(header_line_count);
    let visible_height = v_chunks[1].height as usize;

    // Windowed rendering: only build Lines for visible coefficient rows.
    // The column header occupies the first line of the coefficient area.
    let column_header_lines: usize = 1;
    let data_skip = coefficient_scroll.saturating_sub(column_header_lines);
    let data_take = if coefficient_scroll == 0 { visible_height.saturating_sub(column_header_lines) } else { visible_height };
    let window = &rows[data_skip.min(rows.len())..(data_skip + data_take).min(rows.len())];

    // Cell text for the visible rows, then each column aligned on the point.
    let cells: Vec<[Option<String>; 4]> = window.iter().map(side_by_side_cells).collect();
    let column = |index: usize| decimal_align(&cells.iter().map(|row| row[index].as_deref()).collect::<Vec<_>>());
    let (old, new, delta, percent) = (column(0), column(1), column(2), column(3));
    let widths = [&old, &new, &delta, &percent].map(|cells| cells.iter().map(|cell| cell.chars().count()).max().unwrap_or(0));
    let longest_name = window.iter().map(|row| row.variable.chars().count()).max().unwrap_or(0);
    let columns = SideBySideColumns::fit(v_chunks[1].width as usize, longest_name, widths);
    let [old_w, new_w, delta_w, percent_w] = columns.values;
    let name_w = columns.name;

    let heading = Style::default().fg(t.muted).add_modifier(Modifier::BOLD);
    let mut header = vec![
        Span::styled(format!("  {:<name_w$} ", "Variable"), heading),
        Span::styled(format!("{:>old_w$}", "Old"), Style::default().fg(t.removed).add_modifier(Modifier::BOLD)),
        Span::styled(format!("{GAP}{:>new_w$}", "New"), Style::default().fg(t.added).add_modifier(Modifier::BOLD)),
    ];
    if columns.delta {
        header.push(Span::styled(format!("{GAP}{:>delta_w$}", "\u{394}"), heading));
    }
    if columns.percent {
        header.push(Span::styled(format!("{GAP}{:>percent_w$}", "%\u{394}"), heading));
    }
    // The column header scrolls away with the first data row.
    let mut lines: Vec<Line<'_>> = if coefficient_scroll == 0 { vec![Line::from(header)] } else { Vec::new() };

    let dim = Style::default().fg(t.border);
    let change = Style::default().fg(t.accent);
    for (index, row) in window.iter().enumerate() {
        let (name_style, old_style, new_style, badge) = match row.change_kind {
            Some(DiffKind::Added) => (Style::default().fg(t.added), dim, Style::default().fg(t.added), " [+]"),
            Some(DiffKind::Removed) => (Style::default().fg(t.removed), Style::default().fg(t.removed), dim, " [-]"),
            // Renamed never occurs on coefficient rows (asserted in build_coeff_rows);
            // folded with Modified to keep the match exhaustive.
            Some(DiffKind::Modified | DiffKind::Renamed) => {
                (Style::default().fg(t.modified), Style::default().fg(t.removed), Style::default().fg(t.added), " [~]")
            }
            None => (Style::default().fg(t.muted), Style::default().fg(t.muted), Style::default().fg(t.muted), ""),
        };
        // The missing side's `—` is dimmed whatever the row's colour.
        let side_style = |value: Option<f64>, style: Style| if value.is_some() { style } else { dim };
        let name = truncate_middle(&row.variable, name_w.max(2));
        let mut spans = vec![
            Span::styled(format!("  {name:<name_w$} "), name_style),
            Span::styled(format!("{:>old_w$}", old[index]), side_style(row.old_value, old_style)),
            Span::styled(format!("{GAP}{:>new_w$}", new[index]), side_style(row.new_value, new_style)),
        ];
        if columns.delta {
            spans.push(Span::styled(format!("{GAP}{:>delta_w$}", delta[index]), change));
        }
        if columns.percent {
            spans.push(Span::styled(format!("{GAP}{:>percent_w$}", percent[index]), change));
        }
        spans.push(Span::styled(badge, name_style));
        lines.push(Line::from(spans));
    }

    frame.render_widget(Paragraph::new(lines), v_chunks[1]);

    header_line_count + 1 + rows.len()
}

/// The gap between the side-by-side view's value columns.
const GAP: &str = "  ";

/// The cells of one side-by-side row: old, new, `Δ` and `%Δ`. A side the
/// variable is missing from is `—`; the change columns are only filled for a
/// modified coefficient, which has both sides.
fn side_by_side_cells(row: &crate::detail_model::CoefficientRow) -> [Option<String>; 4] {
    const VALUE: usize = SideBySideColumns::VALUE;
    let side = |value: Option<f64>| Some(value.map_or_else(|| "\u{2014}".to_owned(), |v| fit_number(v, VALUE)));
    let modified = matches!(row.change_kind, Some(DiffKind::Modified));
    let (delta, percent) = match (row.old_value, row.new_value) {
        (Some(old), Some(new)) if modified => {
            let delta = new - old;
            let sign = if delta > 0.0 { "+" } else { "" };
            let percent = (old.abs() > 0.0).then(|| format!("{:+.1}%", delta / old.abs() * 100.0));
            (Some(format!("{sign}{}", fit_number(delta, VALUE - sign.len()))), percent)
        }
        _ => (None, None),
    };
    [side(row.old_value), side(row.new_value), delta, percent]
}

/// Align a column of numbers on their decimal points: integer parts
/// right-aligned, fractions left-aligned. Cells without a point (integers,
/// scientific notation, `—`) align as though the point followed them; `None`
/// cells come back blank.
fn decimal_align(cells: &[Option<&str>]) -> Vec<String> {
    let split = |cell: &str| -> (usize, usize) {
        let integer = if cell.contains('e') { cell.len() } else { cell.find('.').unwrap_or(cell.len()) };
        let integer_width = cell[..integer].chars().count();
        (integer_width, cell.chars().count() - integer_width)
    };
    let (integer_width, fraction_width) =
        cells.iter().flatten().map(|cell| split(cell)).fold((0, 0), |(integer, fraction), (i, f)| (integer.max(i), fraction.max(f)));
    cells
        .iter()
        .map(|cell| match cell {
            Some(cell) => {
                let (integer, _) = split(cell);
                let pad = integer_width - integer;
                let text = format!("{:pad$}{cell}", "");
                format!("{text:<width$}", width = integer_width + fraction_width)
            }
            None => " ".repeat(integer_width + fraction_width),
        })
        .collect()
}

/// Column layout of the side-by-side coefficient view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SideBySideColumns {
    name: usize,
    /// Widths of the old, new, `Δ` and `%Δ` columns.
    values: [usize; 4],
    /// Whether the `Δ` column is drawn.
    delta: bool,
    /// Whether the `%Δ` column is drawn.
    percent: bool,
}

impl SideBySideColumns {
    /// Indent before the name, and the gap after it.
    const CHROME: usize = 2 + 1;
    /// The ` [~]` change badge, plus a spare column at the right edge.
    const BADGE: usize = 4 + 1;
    /// Widest a value is formatted; longer ones are rounded to fit.
    const VALUE: usize = 10;
    /// The narrowest name column worth keeping the change columns for.
    const MIN_NAME: usize = 8;
    /// Header labels, which set each value column's minimum width.
    const HEADERS: [&str; 4] = ["Old", "New", "\u{394}", "%\u{394}"];

    /// Lay the columns out in `width`, given the longest visible name and the
    /// visible cells' `widths` (zero for a column with no cells). `%Δ` goes
    /// first when room is short, then `Δ`; only then does the name shrink
    /// below [`Self::MIN_NAME`].
    fn fit(width: usize, longest_name: usize, widths: [usize; 4]) -> Self {
        let mut values = widths;
        for (value, header) in values.iter_mut().zip(Self::HEADERS) {
            *value = (*value).max(header.chars().count());
        }
        let gap = GAP.len();
        let base = Self::CHROME + values[0] + gap + values[1] + Self::BADGE;
        let with_delta = base + gap + values[2];
        let with_percent = with_delta + gap + values[3];
        let min_name = Self::MIN_NAME.min(longest_name.max("Variable".len()));
        // A change column with nothing in it (no modified row in view) is not drawn.
        let (has_delta, has_percent) = (widths[2] > 0, widths[3] > 0);
        let (used, delta, percent) = if has_delta && has_percent && width >= with_percent + min_name {
            (with_percent, true, true)
        } else if has_delta && width >= with_delta + min_name {
            (with_delta, true, false)
        } else {
            (base, false, false)
        };
        let name = width.saturating_sub(used).clamp(3, longest_name.max("Variable".len()));
        Self { name, values, delta, percent }
    }
}

/// How the unified coefficient rows spell their change badges.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BadgeStyle {
    /// ` [modified]`, ` (unchanged)`.
    Words,
    /// ` [~]`, and nothing for an unchanged row — for panes too narrow for words.
    Glyphs,
}

impl BadgeStyle {
    const fn label(self, kind: DiffKind) -> &'static str {
        match (self, kind) {
            (Self::Words, DiffKind::Added) => " [added]",
            (Self::Words, DiffKind::Removed) => " [removed]",
            (Self::Words, DiffKind::Modified | DiffKind::Renamed) => " [modified]",
            (Self::Glyphs, DiffKind::Added) => " [+]",
            (Self::Glyphs, DiffKind::Removed) => " [-]",
            (Self::Glyphs, DiffKind::Modified | DiffKind::Renamed) => " [~]",
        }
    }

    const fn unchanged(self) -> &'static str {
        match self {
            Self::Words => " (unchanged)",
            Self::Glyphs => "",
        }
    }

    const fn width(self) -> usize {
        match self {
            Self::Words => " [modified]".len(),
            Self::Glyphs => " [~]".len(),
        }
    }
}

/// Value columns of the unified `name  old → new  [badge]` coefficient rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct UnifiedColumns {
    value: usize,
    badges: BadgeStyle,
}

impl UnifiedColumns {
    /// The value columns' width when there is room — wide enough for typical
    /// LP coefficients.
    const VALUE: usize = 12;
    /// The narrowest a value column gets; `fit_number` rounds into it.
    const MIN_VALUE: usize = 5;
    /// Borders, indent, gap, and the narrowest useful name column.
    const NAME_CHROME: usize = 2 + 4 + 1 + 8;

    /// Size the value columns to `pane_width` so the badge stays in view: the
    /// badges drop to glyphs first, then the values narrow (rounded, never
    /// clipped). `None` (the plain-text yank) is the full layout.
    fn fit(pane_width: Option<u16>) -> Self {
        let full = Self { value: Self::VALUE, badges: BadgeStyle::Words };
        let Some(width) = pane_width else {
            return full;
        };
        let room = (width as usize).saturating_sub(Self::NAME_CHROME);
        if room >= full.reserved() {
            return full;
        }
        let spare = room.saturating_sub(ARROW.chars().count() + BadgeStyle::Glyphs.width());
        Self { value: (spare / 2).clamp(Self::MIN_VALUE, Self::VALUE), badges: BadgeStyle::Glyphs }
    }

    /// Columns taken after the name: both values, the arrow and the badge.
    fn reserved(self) -> usize {
        2 * self.value + ARROW.chars().count() + self.badges.width()
    }
}

/// Compute the visible range of coefficient rows for windowed rendering.
///
/// Returns `(first_visible_row, max_visible_rows)`. When the scroll position
/// is within the header area, `first_visible_row` is 0 and `max_visible_rows`
/// accounts for header lines still occupying the viewport.
const fn coeff_visible_range(scroll: usize, area: Rect, header_line_count: usize) -> (usize, usize) {
    let inner_height = area.height.saturating_sub(2) as usize; // subtract borders
    let scroll_usize = scroll;
    let first_visible = scroll_usize.saturating_sub(header_line_count);
    let visible_space = inner_height.saturating_sub(header_line_count.saturating_sub(scroll_usize));
    // +1 for partially visible lines at the bottom edge.
    (first_visible, visible_space + 1)
}

/// Render a combined view of old and new coefficient lists, annotating each
/// variable with its change status.
///
/// When `visible_range` is `Some((first, count))`, only builds `Line` objects
/// for the visible window, avoiding `O(total_rows)` work per frame. Returns
/// `(skipped, rows)`: how many rows ahead of the window were left out, and how
/// many rows there are in all.
#[allow(clippy::too_many_arguments)] // the row source, window and pane width are all independent
fn render_coeff_changes(
    lines: &mut Vec<Line<'static>>,
    changes: &[CoefficientChange],
    old_coefficients: &[ResolvedCoefficient],
    new_coefficients: &[ResolvedCoefficient],
    cached_rows: Option<&[crate::detail_model::CoefficientRow]>,
    visible_range: Option<(usize, usize)>,
    pane_width: Option<u16>,
    interner: &NameInterner,
) -> (usize, usize) {
    let t = theme();

    let owned_rows;
    let rows = if let Some(cached) = cached_rows {
        cached
    } else {
        owned_rows = build_coeff_rows(changes, old_coefficients, new_coefficients, interner);
        &owned_rows
    };

    let (skip, take) = visible_range.unwrap_or((0, rows.len()));
    let layout = UnifiedColumns::fit(pane_width);
    let (val_w, badges) = (layout.value, layout.badges);
    let name_w = name_column_width(rows.iter().map(|row| row.variable.chars().count()).max().unwrap_or(0), pane_width, layout.reserved());

    let skipped = skip.min(rows.len());

    // Build styled Lines only for the visible window.
    // Reuse string buffers across rows to avoid per-row heap allocations.
    let mut name_buf = String::with_capacity(24);
    let mut old_buf = String::with_capacity(16);
    let mut new_buf = String::with_capacity(16);
    for row in rows.iter().skip(skip).take(take) {
        old_buf.clear();
        if let Some(v) = row.old_value {
            old_buf.push_str(&fit_number(v, val_w));
        }
        new_buf.clear();
        if let Some(v) = row.new_value {
            new_buf.push_str(&fit_number(v, val_w));
        }
        name_buf.clear();
        name_buf.push_str(&name_cell(&row.variable, name_w));

        match row.change_kind {
            Some(DiffKind::Added) => {
                lines.push(Line::from(vec![
                    Span::styled(name_buf.clone(), Style::default().fg(t.added)),
                    Span::styled(format!("{:>val_w$}", ""), Style::default()),
                    Span::styled(ARROW, muted()),
                    Span::styled(format!("{new_buf:<val_w$}"), Style::default().fg(t.added)),
                    Span::styled(badges.label(DiffKind::Added), Style::default().fg(t.added)),
                ]));
            }
            Some(DiffKind::Removed) => {
                lines.push(Line::from(vec![
                    Span::styled(name_buf.clone(), Style::default().fg(t.removed)),
                    Span::styled(format!("{old_buf:>val_w$}"), Style::default().fg(t.removed)),
                    Span::styled(ARROW, muted()),
                    Span::styled(format!("{:val_w$}", ""), Style::default()),
                    Span::styled(badges.label(DiffKind::Removed), Style::default().fg(t.removed)),
                ]));
            }
            // Renamed never occurs on coefficient rows (asserted in build_coeff_rows);
            // folded with Modified to keep the match exhaustive.
            Some(DiffKind::Modified | DiffKind::Renamed) => {
                lines.push(Line::from(vec![
                    Span::styled(name_buf.clone(), Style::default().fg(t.modified)),
                    Span::styled(format!("{old_buf:>val_w$}"), Style::default().fg(t.removed)),
                    Span::styled(ARROW, muted()),
                    Span::styled(format!("{new_buf:<val_w$}"), Style::default().fg(t.added)),
                    Span::styled(badges.label(DiffKind::Modified), Style::default().fg(t.modified)),
                ]));
            }
            None => {
                lines.push(Line::from(vec![
                    Span::styled(name_buf.clone(), Style::default().fg(t.muted)),
                    Span::styled(format!("{old_buf:>val_w$}"), Style::default().fg(t.muted)),
                    Span::styled(badges.unchanged(), Style::default().fg(t.muted)),
                ]));
            }
        }
    }

    (skipped, rows.len())
}

/// Wrap `lines` in a bordered block with the given `title` and render it,
/// applying vertical scroll. Returns the total content line count.
fn render_panel(frame: &mut Frame, area: Rect, title: Line<'static>, lines: Vec<Line<'_>>, border_style: Style, scroll: u16) -> usize {
    let line_count = lines.len();
    let block = panel_block(border_style).title(title);
    let paragraph = Paragraph::new(lines).block(block).scroll((scroll, 0));
    frame.render_widget(paragraph, area);
    line_count
}

#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    use super::*;

    /// The windowed renderers must draw exactly what rendering every line and
    /// scrolling the paragraph would, at every scroll offset.
    #[test]
    fn windowed_panels_match_the_full_render_at_every_scroll() {
        let terms: Vec<String> = (0..300).map(|i| format!("{} x{i:03}", i + 1)).collect();
        let source = format!("min\nobj: {}\nst\nc1: {} >= 2\nend\n", terms.join(" + "), terms.join(" + "));
        let app = crate::snapshot_tests::inspect_app_from(&source);
        let interner = &app.report.interner;
        let objective = &app.report.objectives.entries[0];
        let constraint = &app.report.constraints.entries[0];
        let area = Rect::new(0, 0, 60, 20);
        let draw = |f: &dyn Fn(&mut Frame)| {
            let mut terminal = Terminal::new(TestBackend::new(area.width, area.height)).expect("test terminal must build");
            terminal.draw(|frame| f(frame)).expect("draw must succeed");
            terminal.backend().buffer().clone()
        };
        let style = Style::default();

        for scroll in [0_u16, 1, 2, 3, 4, 5, 17, 150, 290, 300] {
            let windowed = draw(&|frame| {
                render_inspect_objective(frame, area, objective, style, usize::from(scroll), interner);
            });
            let full = draw(&|frame| {
                let title = detail_title("Objective", &objective.name, None, false, area.width);
                render_panel(frame, area, title, build_inspect_objective(objective, interner, Some(area.width)), style, scroll);
            });
            assert_eq!(windowed, full, "inspect objective differs at scroll {scroll}");

            let windowed = draw(&|frame| {
                render_inspect_constraint(frame, area, constraint, style, usize::from(scroll), interner);
            });
            let full = draw(&|frame| {
                let title = detail_title("Constraint", &constraint.name, None, false, area.width);
                render_panel(frame, area, title, build_inspect_constraint(constraint, interner, Some(area.width)), style, scroll);
            });
            assert_eq!(windowed, full, "inspect constraint differs at scroll {scroll}");
        }

        // A diff-mode objective present on one side only.
        let diff = crate::snapshot_tests::diff_app_from(&source, &source.replace("obj:", "renamed_obj:"));
        let interner = &diff.report.interner;
        let removed = diff.report.objectives.entries.iter().find(|e| e.kind == DiffKind::Removed).expect("obj is removed");
        for scroll in [0_u16, 1, 2, 150, 300] {
            let windowed = draw(&|frame| {
                render_objective_detail(frame, area, removed, style, usize::from(scroll), None, interner);
            });
            let full = draw(&|frame| {
                // A viewport tall enough to hold every row: the same pane width, no window.
                let everything = Rect::new(0, 0, area.width, u16::MAX);
                let content = objective_detail_lines(removed, None, interner, Some((0, everything)));
                assert_eq!(content.lines.len(), content.total, "every row is built");
                let title = detail_title("Objective", &removed.name, Some(removed.kind), true, area.width);
                render_panel(frame, area, title, content.lines, style, scroll);
            });
            assert_eq!(windowed, full, "removed objective differs at scroll {scroll}");
        }

        // A modified objective, through the unified change rows.
        let diff = crate::snapshot_tests::diff_app_from(&source, &source.replace("obj: 1 x000", "obj: 5 x000 + 7 extra"));
        let interner = &diff.report.interner;
        let modified = diff.report.objectives.entries.iter().find(|e| e.kind == DiffKind::Modified).expect("obj is modified");
        for scroll in [0_usize, 1, 2, 3, 150, 301] {
            let windowed = draw(&|frame| {
                render_objective_detail(frame, area, modified, style, scroll, None, interner);
            });
            let full = draw(&|frame| {
                let everything = Rect::new(0, 0, area.width, u16::MAX);
                let content = objective_detail_lines(modified, None, interner, Some((0, everything)));
                assert_eq!(content.lines.len(), content.total, "every row is built");
                let title = detail_title("Objective", &modified.name, Some(modified.kind), true, area.width);
                render_panel(frame, area, title, content.lines, style, u16::try_from(scroll).expect("small scroll"));
            });
            assert_eq!(windowed, full, "modified objective differs at scroll {scroll}");
        }
    }

    #[test]
    fn unified_columns_keep_the_badge_in_narrow_panes() {
        assert_eq!(UnifiedColumns::fit(None), UnifiedColumns { value: 12, badges: BadgeStyle::Words });
        assert_eq!(UnifiedColumns::fit(Some(80)), UnifiedColumns { value: 12, badges: BadgeStyle::Words });
        let narrow = UnifiedColumns::fit(Some(45));
        assert_eq!(narrow.badges, BadgeStyle::Glyphs, "a narrow pane drops to glyph badges");
        assert!(UnifiedColumns::NAME_CHROME + narrow.reserved() <= 45, "the row fits: {narrow:?}");
    }

    #[test]
    fn side_by_side_drops_the_change_columns_before_the_name() {
        let widths = [7, 7, 6, 6];
        let wide = SideBySideColumns::fit(80, 20, widths);
        assert!(wide.delta && wide.percent, "a wide pane shows both change columns");
        let unchanged = SideBySideColumns::fit(80, 20, [7, 7, 0, 0]);
        assert!(!unchanged.delta && !unchanged.percent, "empty change columns are not drawn");
        assert_eq!(wide.name, 20, "the name column is no wider than the longest name");
        let medium = SideBySideColumns::fit(45, 20, widths);
        assert!(medium.delta && !medium.percent, "%\u{394} goes first");
        let narrow = SideBySideColumns::fit(35, 20, widths);
        assert!(!narrow.delta && !narrow.percent, "then \u{394}");
        assert!(narrow.name >= SideBySideColumns::MIN_NAME, "the name keeps its minimum: {narrow:?}");
    }

    #[test]
    fn decimal_align_lines_up_the_points() {
        let aligned = decimal_align(&[Some("1086.9"), Some("41.19926"), Some("2"), Some("\u{2014}"), None]);
        assert_eq!(aligned, ["1086.9    ", "  41.19926", "   2      ", "   \u{2014}      ", "          "]);
        let dots: Vec<usize> = aligned[..2].iter().map(|cell| cell.find('.').expect("has a point")).collect();
        assert_eq!(dots[0], dots[1], "the points share a column");
    }

    #[test]
    fn side_by_side_cells_fill_the_change_only_where_both_sides_differ() {
        let row = |old, new, kind| crate::detail_model::CoefficientRow {
            variable: "x".to_owned(),
            old_value: old,
            new_value: new,
            change_kind: kind,
        };
        let modified = side_by_side_cells(&row(Some(2.0), Some(3.0), Some(DiffKind::Modified)));
        assert_eq!(modified, [Some("2".to_owned()), Some("3".to_owned()), Some("+1".to_owned()), Some("+50.0%".to_owned())]);
        let added = side_by_side_cells(&row(None, Some(3.0), Some(DiffKind::Added)));
        assert_eq!(added, [Some("\u{2014}".to_owned()), Some("3".to_owned()), None, None], "the missing side is a dash");
        let from_zero = side_by_side_cells(&row(Some(0.0), Some(3.0), Some(DiffKind::Modified)));
        assert_eq!(from_zero[3], None, "no relative change from zero");
    }
}
