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
    app.diagnostics = Some(crate::state::ScrollPane { lines, scroll: 0, export: None });
    insta::assert_snapshot!(render(&mut app, 120, 40).backend());
}

#[test]
fn snapshot_presolve_log_pane_120x40() {
    let mut app = inspect_app_from(REDUCIBLE_LP);
    let (_, mut stats) = crate::presolve::presolve(&app.problem1, crate::presolve::DEFAULT_RULES);
    // The headline carries the wall-clock time; a snapshot cannot.
    stats.duration = std::time::Duration::ZERO;
    let lines = crate::widgets::presolve::log_lines(&stats);
    app.presolve_log = Some(crate::state::ScrollPane { lines, scroll: 0, export: Some(("presolve_log.txt", stats.log_text())) });
    insta::assert_snapshot!(render(&mut app, 120, 40).backend());
}

#[test]
fn snapshot_highs_presolve_pane_120x40() {
    let mut app = inspect_app_from(REDUCIBLE_LP);
    let mut report = crate::highs_presolve::highs_presolve(&app.problem1).expect("a plain LP presolves");
    // The headline carries the wall-clock time; a snapshot cannot.
    report.duration = std::time::Duration::ZERO;
    let lines = crate::widgets::presolve::highs_log_lines(&report);
    app.presolve_log = Some(crate::state::ScrollPane { lines, scroll: 0, export: Some(("highs_presolve.txt", report.log_text())) });
    insta::assert_snapshot!(render(&mut app, 120, 40).backend());
}

/// The solve-profile pane, built from a fixed table rather than a real sweep:
/// wall-clock times cannot be snapshotted, and the point here is the layout.
#[test]
fn snapshot_solve_profile_pane_120x40() {
    use std::time::Duration;

    use crate::profile::{Measurement, Profile, Run};

    let measurement = |status: &str, total_ms: u64, solve_ms: u64, iterations, objective, differs| Measurement {
        status: status.to_owned(),
        objective,
        solve_time: Duration::from_millis(solve_ms),
        total_time: Duration::from_millis(total_ms),
        iterations,
        differs,
    };

    let mut app = inspect_app_from(REDUCIBLE_LP);
    let (_, mut rewrite) = crate::presolve::presolve(&app.problem1, crate::presolve::DEFAULT_RULES);
    rewrite.duration = Duration::ZERO;

    let profile = Profile {
        runs: vec![
            Run { label: "default", outcome: Ok(measurement("Optimal", 41, 38, Some(312), Some(190.0), false)) },
            Run { label: "presolve off", outcome: Ok(measurement("Optimal", 118, 115, Some(904), Some(190.0), false)) },
            Run { label: "simplex dual", outcome: Ok(measurement("Optimal", 38, 35, Some(298), Some(190.0), false)) },
            // A row that stopped early, and a row that disagreed: both must be
            // visibly excluded from the "fastest" verdict.
            Run { label: "simplex primal", outcome: Ok(measurement("ReachedTimeLimit", 12, 10, None, None, true)) },
            Run { label: "ipm", outcome: Err("HiGHS rejected `solver = ipm`".to_owned()) },
            Run { label: "local presolve", outcome: Ok(measurement("Optimal", 29, 21, Some(201), Some(190.0), false)) },
        ],
        baseline: Some(190.0),
        fastest: Some(5),
        rewrite: Some(rewrite),
        budget: Duration::from_secs(10),
        duration: Duration::from_millis(238),
    };

    app.analysis = crate::state::AnalysisState::Done {
        label: "Solve profile",
        pane: crate::state::ScrollPane {
            lines: crate::widgets::profile::build_lines(&profile),
            scroll: 0,
            export: Some(("solve_profile.txt", crate::widgets::profile::export_text(&profile))),
        },
    };
    insta::assert_snapshot!(render(&mut app, 120, 40).backend());
}

/// An LP with nothing stopping `x` from growing: the pane must name it.
const UNBOUNDED_LP: &str = "max\nobj: 3 x + 2 y\nst\nc1: y <= 4\nend\n";

#[test]
fn snapshot_unbounded_ray_pane_120x40() {
    let mut app = inspect_app_from(UNBOUNDED_LP);
    let mut report = crate::highs_query::unbounded_ray(&app.problem1).expect("an unbounded LP must diagnose");
    // The footer carries the wall-clock time; a snapshot cannot.
    report.duration = std::time::Duration::ZERO;
    app.analysis = crate::state::AnalysisState::Done {
        label: "Unbounded ray",
        pane: crate::state::ScrollPane {
            lines: crate::widgets::highs_query::ray_lines(&report),
            scroll: 0,
            export: Some(("unbounded_ray.txt", crate::widgets::highs_query::ray_export(&report))),
        },
    };
    insta::assert_snapshot!(render(&mut app, 120, 40).backend());
}

/// Two rows that cannot both hold, with a third that can: the IIS must name
/// the first two and leave the third out.
const INFEASIBLE_LP: &str = "min\nobj: x + y\nst\nc1: x >= 5\nc2: x <= 3\nc3: y >= 1\nend\n";

#[test]
fn snapshot_iis_pane_120x40() {
    let mut app = inspect_app_from(INFEASIBLE_LP);
    let mut report = crate::highs_query::iis(&app.problem1).expect("an infeasible LP must yield an IIS");
    // The footer carries the wall-clock time; a snapshot cannot.
    report.duration = std::time::Duration::ZERO;
    app.analysis = crate::state::AnalysisState::Done {
        label: "Irreducible infeasible subsystem",
        pane: crate::state::ScrollPane {
            lines: crate::widgets::highs_query::iis_lines(&report),
            scroll: 0,
            export: Some(("iis.txt", crate::widgets::highs_query::iis_export(&report))),
        },
    };
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
