//! Diagnostics pane (`D`): why the solve is slow, and which rows and variables
//! are responsible.
//!
//! A scrollable overlay built once when the pane is opened. It leads with a
//! verdict, then the solver's own telemetry, then the model's ranges, and
//! finally the ranked tables that name individual constraints and variables.
//!
//! The tables carry both structure and solve behaviour on the same line — a
//! row's coefficient spread next to its shadow price — because that pairing is
//! the whole point: a global range tells you the model is badly scaled, but
//! only the join tells you *which* constraint to go and fix.

use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use crate::diagnostics::{ColStat, Diagnostics, Range, RowStat, Verdict};
use crate::theme::theme;
use crate::widgets::numerics::{RATIO_ERROR_THRESHOLD, format_ratio, ratio_colour};
use crate::widgets::{rule_str, truncate_with_ellipsis};

/// Width of the name column in the ranked tables.
const NAME_WIDTH: usize = 26;

/// Label column width in the key/value blocks.
const LABEL_WIDTH: usize = 20;

/// Box width above which a variable's bounds are effectively no constraint at
/// all, and the ratio test has little to work with.
const WIDE_BOX: f64 = 1e9;

/// Build the full set of display lines for the pane.
///
/// Called once when the pane opens and cached on `App`, matching how the
/// summary and numerics panels avoid rebuilding text every frame.
pub fn build_lines(diagnostics: &Diagnostics) -> Vec<Line<'static>> {
    let mut lines = Vec::new();

    verdict_block(&mut lines, diagnostics);
    solve_block(&mut lines, diagnostics);
    model_block(&mut lines, diagnostics);

    row_table(&mut lines, "Worst-conditioned constraints", &diagnostics.worst_rows, "coefficient spread within the row");
    col_table(&mut lines, "Worst-conditioned variables", &diagnostics.worst_cols, "coefficient spread down the column");

    if !diagnostics.degenerate_rows.is_empty() {
        row_table(
            &mut lines,
            "Degenerate constraints",
            &diagnostics.degenerate_rows,
            "active at the optimum but with a zero dual \u{2014} the simplex pivots around these",
        );
    }
    if !diagnostics.degenerate_cols.is_empty() {
        col_table(
            &mut lines,
            "Degenerate variables",
            &diagnostics.degenerate_cols,
            "at a bound with zero reduced cost \u{2014} alternative optima",
        );
    }

    row_table(&mut lines, "Densest constraints", &diagnostics.densest_rows, "cost per iteration, not iteration count");
    col_table(&mut lines, "Densest variables", &diagnostics.densest_cols, "cost per iteration, not iteration count");

    lines
}

/// Section heading with an underline, matching the numerics panel's style.
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

/// A `label   value` line in one of the key/value blocks.
fn field(lines: &mut Vec<Line<'static>>, label: &str, value: String, style: Style) {
    let t = theme();
    lines.push(Line::from(vec![
        // Explicit trailing gap, so a label that reaches the column width
        // still separates from its value.
        Span::styled(format!("  {label:LABEL_WIDTH$}  "), Style::default().fg(t.muted)),
        Span::styled(value, style),
    ]));
}

/// The headline reading and what to do about it.
fn verdict_block(lines: &mut Vec<Line<'static>>, diagnostics: &Diagnostics) {
    let t = theme();
    let colour = match diagnostics.verdict {
        Verdict::Unknown => t.muted,
        Verdict::Healthy => t.added,
        Verdict::Degenerate | Verdict::IllConditioned => t.removed,
        Verdict::Elevated => t.modified,
    };
    heading(lines, "Verdict", "");
    lines.push(Line::from(Span::styled(
        format!("  {}", diagnostics.verdict.summary()),
        Style::default().fg(colour).add_modifier(Modifier::BOLD),
    )));
    let advice = diagnostics.verdict.advice();
    if !advice.is_empty() {
        lines.push(Line::from(Span::styled(format!("  {advice}"), Style::default().fg(t.text))));
    }
}

/// What the solver reported, plus the degeneracy proxies.
fn solve_block(lines: &mut Vec<Line<'static>>, diagnostics: &Diagnostics) {
    let t = theme();
    let Some(telemetry) = &diagnostics.telemetry else {
        heading(lines, "Solve", "no solve recorded yet");
        return;
    };
    heading(lines, "Solve", "");

    match (telemetry.iterations, diagnostics.iterations_per_row()) {
        (Some(iterations), Some(per_row)) => {
            // The ratio, not the raw count, is what says whether this is normal:
            // 200k iterations is fine for 100k rows and alarming for 500.
            let style = if per_row >= 20.0 {
                Style::default().fg(t.removed).add_modifier(Modifier::BOLD)
            } else if per_row >= 5.0 {
                Style::default().fg(t.modified)
            } else {
                Style::default().fg(t.text)
            };
            field(lines, "simplex iterations", format!("{iterations}  ({per_row:.1} per row)"), style);
        }
        (Some(iterations), None) => field(lines, "simplex iterations", iterations.to_string(), Style::default().fg(t.text)),
        _ => field(lines, "simplex iterations", "none \u{2014} solved in presolve".to_owned(), Style::default().fg(t.added)),
    }

    if let Some((rows, cols, nnz)) = telemetry.presolved {
        field(lines, "after HiGHS presolve", format!("{rows} rows, {cols} cols, {nnz} nonzeros"), Style::default().fg(t.text));
    }
    if let Some(run_time) = telemetry.run_time {
        field(lines, "solver run time", format!("{run_time:.2}s"), Style::default().fg(t.text));
    }
    if let Some(error) = telemetry.objective_error {
        // A large primal-dual gap means the answer itself is shaky, not just slow.
        let style = if error > 1e-6 { Style::default().fg(t.removed) } else { Style::default().fg(t.text) };
        field(lines, "P-D objective error", format!("{error:.1e}"), style);
    }

    let Some(degeneracy) = diagnostics.degeneracy else {
        return;
    };
    let share_style = |share: f64| {
        if share >= 0.30 { Style::default().fg(t.removed).add_modifier(Modifier::BOLD) } else { Style::default().fg(t.text) }
    };
    field(
        lines,
        "degenerate rows",
        format!("{} of {} active ({:.0}%)", degeneracy.rows_degenerate, degeneracy.rows_binding, degeneracy.row_share() * 100.0),
        share_style(degeneracy.row_share()),
    );
    field(
        lines,
        "degenerate columns",
        format!("{} of {} at a bound ({:.0}%)", degeneracy.vars_degenerate, degeneracy.vars_at_bound, degeneracy.col_share() * 100.0),
        share_style(degeneracy.col_share()),
    );
    lines.push(Line::from(Span::styled("  proxies from the final solution, not a basis inspection", Style::default().fg(t.muted))));
}

/// Size and the four global magnitude ranges.
fn model_block(lines: &mut Vec<Line<'static>>, diagnostics: &Diagnostics) {
    let t = theme();
    heading(lines, "Model", "");

    let cells = diagnostics.rows.saturating_mul(diagnostics.cols);
    #[allow(clippy::cast_precision_loss)] // model dimensions are far below 2^52
    let density = if cells == 0 { 0.0 } else { diagnostics.nnz as f64 / cells as f64 * 100.0 };
    field(
        lines,
        "size",
        format!("{} rows \u{00d7} {} cols, {} nonzeros ({density:.2}% dense)", diagnostics.rows, diagnostics.cols, diagnostics.nnz),
        Style::default().fg(t.text),
    );

    for (label, range) in [
        ("matrix range", diagnostics.matrix),
        ("cost range", diagnostics.cost),
        ("rhs range", diagnostics.rhs),
        ("bound range", diagnostics.bound),
    ] {
        field(lines, label, format_range(range), ratio_style(range.ratio()));
    }
}

/// `[min, max]  ratio R`, or a placeholder when the set is empty.
fn format_range(range: Range) -> String {
    if range.count == 0 {
        return "\u{2014}".to_owned();
    }
    match range.ratio() {
        Some(ratio) => format!("[{:.0e}, {:.0e}]  ratio {ratio:.0e}", range.min, range.max),
        None => format!("[{:.0e}, {:.0e}]", range.min, range.max),
    }
}

/// Colour a ratio by how far it has drifted from a well-scaled model, bolding
/// the error tier. Thresholds come from `numerics` so the two panes agree.
fn ratio_style(ratio: Option<f64>) -> Style {
    let style = Style::default().fg(ratio_colour(ratio));
    if ratio.is_some_and(|value| value > RATIO_ERROR_THRESHOLD) { style.add_modifier(Modifier::BOLD) } else { style }
}

/// Ranked table of constraints: structure on the left, solve behaviour on the right.
fn row_table(lines: &mut Vec<Line<'static>>, title: &str, rows: &[RowStat], note: &str) {
    let t = theme();
    heading(lines, title, note);
    if rows.is_empty() {
        lines.push(Line::from(Span::styled("  (none)", Style::default().fg(t.muted))));
        return;
    }

    lines.push(Line::from(Span::styled(
        format!("  {:NAME_WIDTH$} {:>5} {:>10} {:>12} {:>12}  {}", "constraint", "nnz", "ratio", "rhs", "dual", "state"),
        Style::default().fg(t.muted).add_modifier(Modifier::BOLD),
    )));

    for row in rows {
        let name = truncate_with_ellipsis(&row.name, NAME_WIDTH).into_owned();
        let dual = row.shadow_price.map_or_else(|| "\u{2014}".to_owned(), format_dual);
        let state = match (row.activity.is_some(), row.degenerate, row.binding) {
            (false, _, _) => "",
            (true, true, _) => "degenerate",
            (true, false, true) => "binding",
            (true, false, false) => "slack",
        };
        let state_style = match state {
            "degenerate" => Style::default().fg(t.removed).add_modifier(Modifier::BOLD),
            "binding" => Style::default().fg(t.modified),
            _ => Style::default().fg(t.muted),
        };
        lines.push(Line::from(vec![
            Span::styled(format!("  {name:NAME_WIDTH$} "), Style::default().fg(t.text)),
            Span::styled(format!("{:>5} ", row.nnz), Style::default().fg(t.text)),
            Span::styled(format!("{:>10} ", format_ratio(row.range.ratio())), ratio_style(row.range.ratio())),
            Span::styled(format!("{:>12} ", format_value(row.rhs)), Style::default().fg(t.text)),
            Span::styled(format!("{dual:>12}  "), Style::default().fg(t.text)),
            Span::styled(state.to_owned(), state_style),
        ]));
    }
}

/// Ranked table of variables, same shape as [`row_table`].
fn col_table(lines: &mut Vec<Line<'static>>, title: &str, cols: &[ColStat], note: &str) {
    let t = theme();
    heading(lines, title, note);
    if cols.is_empty() {
        lines.push(Line::from(Span::styled("  (none)", Style::default().fg(t.muted))));
        return;
    }

    lines.push(Line::from(Span::styled(
        format!("  {:NAME_WIDTH$} {:>5} {:>10} {:>12} {:>12}  {}", "variable", "rows", "ratio", "value", "red. cost", "state"),
        Style::default().fg(t.muted).add_modifier(Modifier::BOLD),
    )));

    for col in cols {
        let name = truncate_with_ellipsis(&col.name, NAME_WIDTH).into_owned();
        let value = col.value.map_or_else(|| "\u{2014}".to_owned(), format_value);
        let cost = col.reduced_cost.map_or_else(|| "\u{2014}".to_owned(), format_dual);
        let solve_state = match (col.value.is_some(), col.degenerate, col.at_bound) {
            (false, _, _) => "",
            (true, true, _) => "degenerate",
            (true, false, true) => "at bound",
            (true, false, false) => "interior",
        };
        let state_style = match solve_state {
            "degenerate" => Style::default().fg(t.removed).add_modifier(Modifier::BOLD),
            "at bound" => Style::default().fg(t.modified),
            _ => Style::default().fg(t.muted),
        };
        // A variable with no finite box (or an enormous one) gives the ratio
        // test nothing to bite on, which shows up as long, wandering steps.
        let box_tag = match col.bound_width {
            None => " unbounded",
            Some(width) if width > WIDE_BOX => " wide box",
            Some(_) => "",
        };
        let state = format!("{solve_state}{box_tag}");
        lines.push(Line::from(vec![
            Span::styled(format!("  {name:NAME_WIDTH$} "), Style::default().fg(t.text)),
            Span::styled(format!("{:>5} ", col.nnz), Style::default().fg(t.text)),
            Span::styled(format!("{:>10} ", format_ratio(col.range.ratio())), ratio_style(col.range.ratio())),
            Span::styled(format!("{value:>12} "), Style::default().fg(t.text)),
            Span::styled(format!("{cost:>12}  "), Style::default().fg(t.text)),
            Span::styled(state, state_style),
        ]));
    }
}

/// Format a dual value, collapsing both signed zeros to a plain `0`.
fn format_dual(value: f64) -> String {
    if value == 0.0 { "0".to_owned() } else { format!("{value:.3e}") }
}

/// Format a solution value compactly, preferring plain decimals in the range
/// where they are shorter and easier to compare by eye.
fn format_value(value: f64) -> String {
    if value == 0.0 {
        return "0".to_owned();
    }
    let magnitude = value.abs();
    if (1e-3..1e7).contains(&magnitude) { format!("{value:.4}") } else { format!("{value:.3e}") }
}

/// Draw the diagnostics pane over the current frame.
///
/// Takes `&mut App` so the scroll offset can be clamped to the real content
/// height once the visible window is known, matching the help overlay.
pub fn draw_diagnostics(frame: &mut ratatui::Frame, area: ratatui::layout::Rect, app: &mut crate::app::App) {
    use ratatui::widgets::{Clear, Paragraph};

    let Some(pane) = &mut app.diagnostics else {
        return;
    };
    if area.width == 0 || area.height == 0 {
        return;
    }
    let t = theme();

    // Near-full-screen: the tables are wide, and the value is in reading
    // several of them against each other.
    let popup = crate::widgets::centred_rect(area, area.width.saturating_sub(4).max(1), area.height.saturating_sub(2).max(1));

    let inner_height = popup.height.saturating_sub(2) as usize;
    let max_scroll = u16::try_from(pane.lines.len().saturating_sub(inner_height)).unwrap_or(u16::MAX);
    pane.scroll = pane.scroll.min(max_scroll);

    let border_style = Style::default().fg(t.accent).add_modifier(Modifier::BOLD);
    let title = if max_scroll > 0 { " Diagnostics  (j/k scroll \u{2022} Esc close) " } else { " Diagnostics  (Esc close) " };
    let block = crate::widgets::panel_block(border_style).title(Span::styled(title, border_style));

    frame.render_widget(Clear, popup);
    frame.render_widget(Paragraph::new(pane.lines.clone()).block(block).scroll((pane.scroll, 0)), popup);
}

#[cfg(test)]
mod tests {
    use lp_parser_rs::problem::LpProblem;

    use super::*;
    use crate::diagnostics::analyse;

    fn rendered(lines: &[Line<'_>]) -> String {
        lines.iter().map(|line| line.spans.iter().map(|span| span.content.as_ref()).collect::<String>()).collect::<Vec<_>>().join("\n")
    }

    #[test]
    fn without_a_solve_the_pane_says_so_and_still_shows_structure() {
        let problem = LpProblem::parse("Minimize\n obj: x + y\nSubject To\n c1: 1000000 x + 0.001 y <= 5\nEnd").expect("parses");
        let text = rendered(&build_lines(&analyse(&problem, None)));

        assert!(text.contains("No solve yet"), "the verdict names the missing input");
        assert!(text.contains("no solve recorded yet"), "the solve block is explicit rather than blank");
        assert!(text.contains("Worst-conditioned constraints"), "structural tables do not need a solve");
        assert!(text.contains("c1"), "the offending constraint is named");
    }

    #[test]
    fn a_solve_attributes_duals_to_named_rows() {
        let problem = LpProblem::parse("Minimize\n obj: x + y\nSubject To\n tight: x + y >= 2\n slack: x + y <= 900\nEnd").expect("parses");
        let solved = crate::solver::solve_problem(&problem).expect("solves");
        let text = rendered(&build_lines(&analyse(&problem, Some(&solved))));

        assert!(text.contains("tight"), "constraints are listed by name");
        assert!(text.contains("binding") || text.contains("degenerate"), "the active row is labelled with its state");
        assert!(text.contains("slack"), "the inactive row is listed too");
    }

    #[test]
    fn empty_ranges_and_missing_duals_render_without_panicking() {
        // A model with no bounds and no solve exercises every `None` path in
        // the formatters at once.
        let problem = LpProblem::parse("Minimize\n obj: x\nSubject To\n c1: x >= 1\nEnd").expect("parses");
        let text = rendered(&build_lines(&analyse(&problem, None)));

        assert!(text.contains("bound range"), "an empty range still gets its row");
        assert!(!text.is_empty());
    }
}
