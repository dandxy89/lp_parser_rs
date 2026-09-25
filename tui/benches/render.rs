//! Render-path benchmarks: one frame of the detail pane and friends against a
//! `TestBackend`, on models large enough that per-frame work proportional to
//! the model (rather than to the viewport) shows up.
//!
//! `lp_diff` is a binary crate, so there is no library to link against: the
//! crate's modules are compiled into this benchmark directly, with the one
//! crate-root item they use (`disconnected`) restated below.

#![allow(dead_code, unused_imports)]

#[path = "../src/app.rs"]
mod app;
#[path = "../src/cli_output.rs"]
mod cli_output;
#[path = "../src/clipboard.rs"]
mod clipboard;
#[path = "../src/detail_model.rs"]
mod detail_model;
#[path = "../src/detail_text.rs"]
mod detail_text;
#[path = "../src/diagnostics.rs"]
mod diagnostics;
#[path = "../src/diff_model.rs"]
mod diff_model;
#[path = "../src/event.rs"]
mod event;
#[path = "../src/export.rs"]
mod export;
#[path = "../src/format.rs"]
mod format;
#[path = "../src/highs_presolve.rs"]
mod highs_presolve;
#[path = "../src/highs_query.rs"]
mod highs_query;
#[path = "../src/input.rs"]
mod input;
#[path = "../src/inspect_model.rs"]
mod inspect_model;
#[path = "../src/parse.rs"]
mod parse;
#[path = "../src/presolve.rs"]
mod presolve;
#[path = "../src/profile.rs"]
mod profile;
#[path = "../src/search.rs"]
mod search;
// `clippy --all-targets` checks this target with `cfg(test)`, where the
// modules' own tests need their shared fixtures.
#[cfg(test)]
#[path = "../src/snapshot_tests.rs"]
mod snapshot_tests;
#[path = "../src/solver.rs"]
mod solver;
#[path = "../src/state.rs"]
mod state;
#[path = "../src/theme.rs"]
mod theme;
#[path = "../src/ui.rs"]
mod ui;
#[path = "../src/watch.rs"]
mod watch;
#[path = "../src/widgets/mod.rs"]
mod widgets;

use std::fmt::Write as _;
use std::hint::black_box;
use std::path::PathBuf;
use std::sync::Arc;

use criterion::{Criterion, criterion_group, criterion_main};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::layout::Rect;
use ratatui::style::Style;

use crate::app::App;
use crate::diff_model::{DiffInput, DiffOptions, build_diff_report};
use crate::state::Section;

/// The crate root's `disconnected`, which the modules above call. The worker
/// panic hook lives in `main`, so there is never a recorded panic to name.
pub(crate) fn disconnected(what: &str) -> String {
    format!("{what} thread disconnected")
}

/// Terms in the large objective and constraint.
const TERMS: u32 = 200_000;
const WIDTH: u16 = 120;
const HEIGHT: u16 = 40;

/// An LP whose objective and single constraint each carry `TERMS` terms, with
/// every coefficient scaled by `scale` so two models differ everywhere.
fn big_lp(objective: &str, scale: f64) -> String {
    let mut text = format!("Minimize\n {objective}: ");
    for i in 0..TERMS {
        let sep = if i == 0 { "" } else { " + " };
        write!(text, "{sep}{} x{i:06}", f64::from(i % 97) * scale + 1.0).expect("writing to a String cannot fail");
    }
    text.push_str("\nSubject To\n c1: ");
    for i in 0..TERMS {
        let sep = if i == 0 { "" } else { " + " };
        write!(text, "{sep}{} x{i:06}", f64::from(i % 89) * scale + 1.0).expect("writing to a String cannot fail");
    }
    text.push_str(" >= 1\nEnd\n");
    text
}

fn inspect_app(source: &str) -> App {
    let (problem, analysis, line_map, raw_text) = parse::parse_text(source, false, "big.lp").expect("bench LP must parse");
    let report = inspect_model::build_inspect_report("big.lp", &problem, &line_map, analysis);
    let problem = Arc::new(problem);
    App::new_inspect(report, PathBuf::from("big.lp"), Arc::clone(&problem), raw_text.into(), line_map)
}

fn diff_app(base: &str, changed: &str) -> App {
    let (problem1, analysis1, line_map1, raw_text1) = parse::parse_text(base, false, "a.lp").expect("base LP must parse");
    let (problem2, analysis2, line_map2, raw_text2) = parse::parse_text(changed, false, "b.lp").expect("changed LP must parse");
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

/// Draw one full frame of `app`.
fn draw(terminal: &mut Terminal<TestBackend>, app: &mut App) {
    terminal.draw(|frame| ui::draw(frame, app)).expect("draw must succeed");
}

fn bench_detail(c: &mut Criterion) {
    let mut group = c.benchmark_group("detail");
    let mut terminal = Terminal::new(TestBackend::new(WIDTH, HEIGHT)).expect("test terminal must build");

    // Inspect mode: every entry is a single-model view.
    let mut app = inspect_app(&big_lp("obj", 1.0));
    for (section, label) in [(Section::Objectives, "inspect_objective_200k"), (Section::Constraints, "inspect_constraint_200k")] {
        app.set_section(section);
        draw(&mut terminal, &mut app);
        group.bench_function(label, |b| b.iter(|| draw(black_box(&mut terminal), black_box(&mut app))));
        app.detail_scroll = 30_000;
        group.bench_function(format!("{label}_scrolled"), |b| b.iter(|| draw(black_box(&mut terminal), black_box(&mut app))));
    }

    // Diff mode, objective only on one side: an added and a removed entry.
    let mut app = diff_app(&big_lp("old_obj", 1.0), &big_lp("new_obj", 1.0));
    app.set_section(Section::Objectives);
    draw(&mut terminal, &mut app);
    group.bench_function("diff_objective_removed_200k", |b| b.iter(|| draw(black_box(&mut terminal), black_box(&mut app))));

    // Diff mode, modified everywhere: the windowed unified and side-by-side views.
    let mut app = diff_app(&big_lp("obj", 1.0), &big_lp("obj", 2.0));
    for (section, label) in [(Section::Objectives, "diff_objective_modified_200k"), (Section::Constraints, "diff_constraint_modified_200k")] {
        app.set_section(section);
        draw(&mut terminal, &mut app);
        group.bench_function(label, |b| b.iter(|| draw(black_box(&mut terminal), black_box(&mut app))));
    }
    group.finish();
}

criterion_group!(benches, bench_detail);
criterion_main!(benches);
