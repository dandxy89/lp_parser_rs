//! UI snapshot tests: render the main layouts into a `TestBackend` and compare
//! against committed `insta` snapshots (`cargo insta review` to update).
//!
//! These catch accidental layout regressions — a shifted panel, a broken
//! border, a truncated status bar — from refactors or dependency bumps,
//! without needing a real terminal.

use std::path::PathBuf;
use std::sync::Arc;

use ratatui::Terminal;
use ratatui::backend::TestBackend;

use crate::app::App;
use crate::diff_model::{DiffInput, DiffOptions, build_diff_report};
use crate::parse::parse_text;
use crate::state::Section;
use crate::ui;

const BASE_LP: &str = "min\nobj: 2 x + 3 y\nst\nc1: x + y >= 2\nc2: x - y <= 8\nbounds\n0 <= x <= 10\n0 <= y <= 10\nend\n";

const CHANGED_LP: &str = "min\nobj: 2 x + 4 y\nst\nc1: x + y >= 3\nc3: 2 x + y <= 12\nbounds\n0 <= x <= 10\n0 <= y <= 10\nend\n";

/// A model the presolve rules actually bite on: `c2` is a singleton, `c3` can
/// never bind, and `z` appears in no row.
pub(crate) const REDUCIBLE_LP: &str =
    "min\nobj: 2 x + 3 y + z\nst\nc1: x + y >= 2\nc2: 3 x <= 12\nc3: x + y <= 900\nbounds\n0 <= x <= 10\n0 <= y <= 10\n0 <= z <= 5\nend\n";

/// Build an inspect-mode app from an in-memory LP model.
fn inspect_app() -> App {
    inspect_app_from(BASE_LP)
}

/// Build an inspect-mode app from the given LP source.
pub(crate) fn inspect_app_from(source: &str) -> App {
    let (problem, analysis, line_map, raw_text) = parse_text(source, false, "model.lp").expect("test LP must parse");
    let report = crate::inspect_model::build_inspect_report("model.lp", &problem, &line_map, analysis);
    App::new_inspect(report, PathBuf::from("model.lp"), Arc::new(problem), raw_text.into(), line_map)
}

/// Build a diff-mode app comparing two in-memory LP models.
fn diff_app() -> App {
    let (problem1, analysis1, line_map1, raw_text1) = parse_text(BASE_LP, false, "a.lp").expect("base LP must parse");
    let (problem2, analysis2, line_map2, raw_text2) = parse_text(CHANGED_LP, false, "b.lp").expect("changed LP must parse");
    let options = DiffOptions::default();
    let report = build_diff_report(&DiffInput {
        file1: "a.lp",
        file2: "b.lp",
        p1: &problem1,
        p2: &problem2,
        line_map1: &line_map1,
        line_map2: &line_map2,
        analysis1,
        analysis2,
        options: options.clone(),
    });
    App::new(
        report,
        PathBuf::from("a.lp"),
        PathBuf::from("b.lp"),
        Arc::new(problem1),
        Arc::new(problem2),
        raw_text1.into(),
        raw_text2.into(),
        options,
        line_map1,
        line_map2,
    )
}

/// Render one frame at the given size and return the terminal for snapshotting
/// (`terminal.backend()` displays the rendered cell grid).
fn render(app: &mut App, width: u16, height: u16) -> Terminal<TestBackend> {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("test terminal must build");
    terminal.draw(|frame| ui::draw(frame, app)).expect("draw must succeed");
    terminal
}

#[test]
fn snapshot_inspect_summary_80x24() {
    let mut app = inspect_app();
    insta::assert_snapshot!(render(&mut app, 80, 24).backend());
}

#[test]
fn snapshot_inspect_variables_80x24() {
    let mut app = inspect_app();
    app.set_section(Section::Variables);
    insta::assert_snapshot!(render(&mut app, 80, 24).backend());
}

#[test]
fn snapshot_diff_summary_80x24() {
    let mut app = diff_app();
    insta::assert_snapshot!(render(&mut app, 80, 24).backend());
}

#[test]
fn snapshot_diff_constraints_80x24() {
    let mut app = diff_app();
    app.set_section(Section::Constraints);
    insta::assert_snapshot!(render(&mut app, 80, 24).backend());
}

#[test]
fn snapshot_diff_constraints_delta_sort_80x24() {
    let mut app = diff_app();
    app.set_section(Section::Constraints);
    app.cycle_sort_mode();
    insta::assert_snapshot!(render(&mut app, 80, 24).backend());
}

#[test]
fn snapshot_diff_constraints_filtered_80x24() {
    let mut app = diff_app();
    app.set_section(Section::Constraints);
    app.set_filter(crate::state::DiffFilter::Modified);
    insta::assert_snapshot!(render(&mut app, 80, 24).backend());
}

#[test]
fn snapshot_diff_empty_detail_cheatsheet_80x24() {
    let mut app = diff_app();
    // Variables carry no changes in the test models, so the list is empty and
    // the detail panel shows the cheat sheet.
    app.set_section(Section::Variables);
    insta::assert_snapshot!(render(&mut app, 80, 24).backend());
}

#[test]
fn snapshot_help_overlay_80x24() {
    let mut app = inspect_app();
    app.show_help = true;
    insta::assert_snapshot!(render(&mut app, 80, 24).backend());
}

#[test]
fn snapshot_too_small_terminal_40x10() {
    let mut app = inspect_app();
    insta::assert_snapshot!(render(&mut app, 40, 10).backend());
}

#[test]
fn snapshot_presolve_picker_100x30() {
    let mut app = inspect_app();
    app.presolve_cursor = Some(0);
    insta::assert_snapshot!(render(&mut app, 100, 30).backend());
}

#[test]
fn snapshot_diagnostics_pane_120x40() {
    let mut app = inspect_app();
    // No solve: exercises the "no solve recorded yet" path and the structural
    // tables, which need no solver to populate.
    let diagnostics = crate::diagnostics::analyse(&app.problem1, None);
    let lines = crate::widgets::diagnostics::build_lines(&diagnostics);
    app.diagnostics = Some(crate::state::ScrollPane { lines, scroll: 0 });
    insta::assert_snapshot!(render(&mut app, 120, 40).backend());
}

#[test]
fn snapshot_presolve_log_pane_120x40() {
    let mut app = inspect_app_from(REDUCIBLE_LP);
    let (_, mut stats) = crate::presolve::presolve(&app.problem1, crate::presolve::DEFAULT_RULES);
    // The headline carries the wall-clock time; a snapshot cannot.
    stats.duration = std::time::Duration::ZERO;
    let lines = crate::widgets::presolve::log_lines(&stats);
    app.presolve_log = Some(crate::state::ScrollPane { lines, scroll: 0 });
    insta::assert_snapshot!(render(&mut app, 120, 40).backend());
}

/// The clipboard yank is now derived from the same lines the widgets draw, by
/// stripping their styles. Snapshot the plain text so a change to either the
/// panel layout or the flattening shows up here.
#[test]
fn snapshot_yank_constraint_detail_plain() {
    let mut app = diff_app();
    app.set_section(Section::Constraints);
    let text = crate::detail_text::render_detail_plain(&app).expect("a constraint is selected");
    insta::assert_snapshot!(text);
}

/// Summary yanks the pre-built summary panel lines rather than a parallel
/// text renderer; this pins that they still carry the counts table.
#[test]
fn snapshot_yank_summary_plain() {
    let mut app = diff_app();
    app.set_section(Section::Summary);
    let text = crate::detail_text::render_detail_plain(&app).expect("summary always yields text");
    insta::assert_snapshot!(text);
}
