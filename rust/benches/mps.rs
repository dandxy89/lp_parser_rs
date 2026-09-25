//! MPS reader benchmarks on generated models whose `INDICATORS` and
//! `QCMATRIX` sections name many rows.

use std::fmt::{self, Write as _};
use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};
use lp_parser_rs::parse_mps;

const ROWS: usize = 10_000;

fn put(text: &mut String, args: fmt::Arguments<'_>) {
    text.write_fmt(args).expect("writing to a String cannot fail");
}

/// Deterministic MPS model with `ROWS` constraint rows. Every even row gets an
/// indicator and every odd row a `QCMATRIX` section.
fn generate() -> String {
    let mut text = String::from("NAME bench\nROWS\n N obj\n");
    for r in 0..ROWS {
        put(&mut text, format_args!(" L c{r}\n"));
    }
    text.push_str("COLUMNS\n");
    for r in 0..ROWS {
        put(&mut text, format_args!("    x{r} obj 1 c{r} {}\n", r % 7 + 1));
    }
    put(&mut text, format_args!("    b obj 1\n"));
    text.push_str("RHS\n");
    for r in 0..ROWS {
        put(&mut text, format_args!("    RHS c{r} {}\n", r % 100 + 1));
    }
    text.push_str("BOUNDS\n BV BND b\n");
    for r in (1..ROWS).step_by(2) {
        put(&mut text, format_args!("QCMATRIX c{r}\n    x{r} x{r} 1\n"));
    }
    text.push_str("INDICATORS\n");
    for r in (0..ROWS).step_by(2) {
        put(&mut text, format_args!(" IF c{r} b 1\n"));
    }
    text.push_str("ENDATA\n");
    text
}

fn bench_mps(c: &mut Criterion) {
    let text = generate();
    // Sanity check once, outside the timed loop.
    let parsed = parse_mps(&text).expect("generated model must parse");
    assert_eq!(parsed.constraints.len(), ROWS);

    c.bench_function("parse_mps_indicators_qcmatrix", |b| b.iter(|| parse_mps(black_box(&text)).expect("generated model must parse")));
}

criterion_group! {
    name = benches;
    config = Criterion::default().sample_size(20);
    targets = bench_mps
}
criterion_main!(benches);
