//! Parse throughput on the bundled large models and a generated one with many
//! short rows (the common shape of real LP files).

use std::fmt::Write as _;
use std::hint::black_box;
use std::path::Path;

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use lp_parser_rs::LpProblem;

const GENERATED_ROWS: usize = 100_000;
const GENERATED_VARIABLES: usize = 20_000;

fn resource(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("resources").join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()))
}

/// Deterministic LP model with `GENERATED_ROWS` rows of three to five terms,
/// a quarter of them unnamed, plus bounds and generals.
fn generate() -> String {
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
        write!(text, "{}.5 x{a} + {} x{b} - x{c}", i % 9 + 1, i % 5 + 2).expect("writing to a String cannot fail");
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

fn parse_lp(c: &mut Criterion) {
    let mut group = c.benchmark_group("parse_lp");
    let inputs = [("generated_100k_rows", generate()), ("fit2d", resource("fit2d.lp")), ("sudoku", resource("sudoku.lp"))];
    for (name, input) in &inputs {
        group.throughput(Throughput::Bytes(input.len() as u64));
        group.bench_function(*name, |b| b.iter(|| LpProblem::parse(black_box(input)).expect("benchmark input must parse")));
    }
    group.finish();
}

fn parse_mps(c: &mut Criterion) {
    let mut group = c.benchmark_group("parse_mps");
    let input = resource("mps/fit2d.mps");
    group.throughput(Throughput::Bytes(input.len() as u64));
    group.bench_function("fit2d", |b| b.iter(|| LpProblem::parse_mps(black_box(&input)).expect("benchmark input must parse")));
    group.finish();
}

criterion_group!(benches, parse_lp, parse_mps);
criterion_main!(benches);
