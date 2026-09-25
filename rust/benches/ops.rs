//! Throughput of the operations on an already-parsed problem: the LP and MPS
//! writers, the diff engine and the analysis pass.

use std::fmt::Write as _;
use std::hint::black_box;
use std::path::Path;

use criterion::{Criterion, criterion_group, criterion_main};
use lp_parser_rs::LpProblem;
use lp_parser_rs::diff::DiffOptions;
use lp_parser_rs::mps::writer::write_mps_string;
use lp_parser_rs::writer::write_lp_string;

const GENERATED_ROWS: usize = 100_000;
const GENERATED_VARIABLES: usize = 20_000;

fn resource(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("resources").join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()))
}

/// Deterministic LP model with `GENERATED_ROWS` rows of three to five terms,
/// a quarter of them unnamed, plus bounds and generals. `scale` perturbs the
/// fractional coefficients so two models built with different scales differ.
fn generate(scale: &str) -> String {
    let mut text = String::with_capacity(GENERATED_ROWS * 48);
    text.push_str("Minimize\n obj: ");
    for v in 0..100 {
        write!(text, "{} x{v} + ", v % 7 + 1).expect("writing to a String cannot fail");
    }
    text.push_str("x0\nSubject To\n");
    for i in 0..GENERATED_ROWS {
        let (a, b, c) = (i % GENERATED_VARIABLES, (i * 7 + 3) % GENERATED_VARIABLES, (i * 13 + 11) % GENERATED_VARIABLES);
        if i % 4 != 0 {
            write!(text, " c{i}: ").expect("writing to a String cannot fail");
        }
        write!(text, "{}.{scale} x{a} + {} x{b} - x{c}", i % 9 + 1, i % 5 + 2).expect("writing to a String cannot fail");
        if i % 3 == 0 {
            write!(text, " + x{} - 2 x{}", (i * 17 + 5) % GENERATED_VARIABLES, (i * 19 + 1) % GENERATED_VARIABLES)
                .expect("writing to a String cannot fail");
        }
        writeln!(text, " >= {}", i % 100).expect("writing to a String cannot fail");
    }
    text.push_str("Bounds\n");
    for v in (0..GENERATED_VARIABLES).step_by(2) {
        writeln!(text, " 0 <= x{v} <= {}", v % 1000 + 1).expect("writing to a String cannot fail");
    }
    text.push_str("Generals\n");
    for v in (1..GENERATED_VARIABLES).step_by(5) {
        writeln!(text, " x{v}").expect("writing to a String cannot fail");
    }
    text.push_str("End\n");
    text
}

fn models() -> [(&'static str, LpProblem); 2] {
    let generated = LpProblem::parse(&generate("5")).expect("generated model must parse");
    let fit2d = LpProblem::parse(&resource("fit2d.lp")).expect("fit2d must parse");
    [("generated_100k_rows", generated), ("fit2d", fit2d)]
}

fn bench_writers(c: &mut Criterion) {
    let models = models();
    let mut group = c.benchmark_group("write_lp");
    for (name, problem) in &models {
        group.bench_function(*name, |b| b.iter(|| write_lp_string(black_box(problem)).expect("benchmark model must write")));
    }
    group.finish();

    let mut group = c.benchmark_group("write_mps");
    for (name, problem) in &models {
        group.bench_function(*name, |b| b.iter(|| write_mps_string(black_box(problem)).expect("benchmark model must write")));
    }
    group.finish();
}

fn bench_diff(c: &mut Criterion) {
    let old = LpProblem::parse(&generate("5")).expect("generated model must parse");
    let new = LpProblem::parse(&generate("75")).expect("generated model must parse");
    let options = DiffOptions::default();
    let mut group = c.benchmark_group("diff");
    group.bench_function("generated_100k_rows", |b| b.iter(|| black_box(&old).diff(black_box(&new), &options)));
    let normalise = |name: &str| name.to_ascii_lowercase();
    let options = DiffOptions { normalise: Some(&normalise), ..DiffOptions::default() };
    group.bench_function("generated_100k_rows_normalised", |b| b.iter(|| black_box(&old).diff(black_box(&new), &options)));
    group.finish();
}

fn bench_analyze(c: &mut Criterion) {
    let models = models();
    let mut group = c.benchmark_group("analyze");
    for (name, problem) in &models {
        group.bench_function(*name, |b| b.iter(|| black_box(problem).analyze()));
    }
    group.finish();
}

criterion_group! {
    name = benches;
    config = Criterion::default().sample_size(30);
    targets = bench_writers, bench_diff, bench_analyze
}
criterion_main!(benches);
