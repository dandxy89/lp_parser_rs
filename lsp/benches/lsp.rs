//! Large-file benchmarks: incremental reparse, symbol index rebuild and full
//! semantic tokens on a generated ~50 MB LP file, plus request handlers on
//! generated models that stress one dimension each (a very long line, very
//! many constraints).
//!
//! Every model is generated lazily inside the bench closure, so filtering to
//! one bench only pays for the models it uses.

use std::fmt::{self, Write as _};
use std::hint::black_box;
use std::sync::{Arc, Once, OnceLock};
use std::time::Duration;

use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use lp_lsp::config::{Config, FormatSettings, InlayHintSettings};
use lp_lsp::features::{code_action, code_lens, completion, diagnostics, folding, format, inlay, semantic_tokens, symbols};
use lp_lsp::{Document, Encoding, SymbolIndex, semantic, syntax};
use lp_parser_rs::analysis::AnalysisConfig;
use tower_lsp_server::ls_types::{CodeActionContext, TextDocumentContentChangeEvent, Uri};
use tree_sitter::InputEdit;

const TARGET_BYTES: usize = 50 * 1024 * 1024;
const VARIABLES: usize = 200_000;

fn put(text: &mut String, args: fmt::Arguments<'_>) {
    text.write_fmt(args).expect("writing to a String cannot fail");
}

fn uri() -> Uri {
    "file:///bench.lp".parse().expect("valid URI")
}

fn document(text: String) -> Document {
    let doc = Document::new(uri(), text, 1, Encoding::Utf16);
    assert!(!doc.has_syntax_errors(), "generated file must parse cleanly");
    doc
}

/// Deterministic LP model of roughly `TARGET_BYTES`: constraints fill most of
/// it, then bounds, generals and a few SOS sets.
fn generate() -> String {
    let mut text = String::with_capacity(TARGET_BYTES + 1024 * 1024);
    text.push_str("Minimize\n obj: ");
    for v in 0..1000 {
        put(&mut text, format_args!("{} x{v} + ", v % 7 + 1));
    }
    text.push_str("x0\nSubject To\n");
    let body = TARGET_BYTES - TARGET_BYTES / 10;
    let mut i = 0usize;
    while text.len() < body {
        let (a, b, c) = (i % VARIABLES, (i * 7 + 3) % VARIABLES, (i * 13 + 11) % VARIABLES);
        put(&mut text, format_args!(" c{i}: {}.5 x{a} + {} x{b} - x{c} >= {}\n", i % 9 + 1, i % 5 + 2, i % 100));
        i += 1;
    }
    text.push_str("Bounds\n");
    for v in 0..VARIABLES {
        put(&mut text, format_args!(" 0 <= x{v} <= {}\n", v % 1000 + 1));
    }
    text.push_str("Generals\n");
    for v in (0..VARIABLES).step_by(2) {
        put(&mut text, format_args!(" x{v}\n"));
    }
    text.push_str("SOS\n");
    for s in 0..10 {
        put(&mut text, format_args!(" s{s}: S{} ::\n", s % 2 + 1));
        for k in 0..5 {
            put(&mut text, format_args!("  x{}: {}\n", s * 5 + k, k + 1));
        }
    }
    text.push_str("End\n");
    text
}

/// The ~50 MB document, parsed once.
fn large() -> &'static Document {
    static DOC: OnceLock<Document> = OnceLock::new();
    DOC.get_or_init(|| {
        let text = generate();
        eprintln!("generated LP file: {} bytes (~{} MiB)", text.len(), text.len() / (1024 * 1024));
        document(text)
    })
}

/// Offset just after the `: ` of a constraint near the middle of `doc`.
fn middle_constraint(doc: &Document) -> usize {
    let middle = doc.text.len() / 2;
    doc.text[middle..].find(": ").expect("a constraint after the middle") + middle + 2
}

fn large_file(c: &mut Criterion) {
    let mut group = c.benchmark_group("lsp_50mb");
    group.sample_size(10).warm_up_time(Duration::from_secs(1)).measurement_time(Duration::from_secs(10));

    group.bench_function("apply_changes_single_char", |b| {
        // Insert a digit in front of a constraint coefficient near the middle.
        let doc = large();
        let at = middle_constraint(doc);
        let change = TextDocumentContentChangeEvent { range: Some(doc.range(at..at)), range_length: None, text: "1".to_owned() };
        b.iter_batched(
            || doc.clone(),
            |mut doc| {
                doc.apply_changes(std::slice::from_ref(&change), 2);
                doc
            },
            BatchSize::PerIteration,
        );
    });

    group.bench_function("incremental_reparse_only", |b| {
        let doc = large();
        let at = middle_constraint(doc);
        let mut edited = doc.text.clone();
        edited.insert(at, '1');
        let point = doc.lines.point(at);
        let edit = InputEdit {
            start_byte: at,
            old_end_byte: at,
            new_end_byte: at + 1,
            start_position: point,
            old_end_position: point,
            new_end_position: tree_sitter::Point { row: point.row, column: point.column + 1 },
        };
        b.iter_batched(
            || {
                let mut tree = doc.tree.clone();
                tree.edit(&edit);
                tree
            },
            |tree| syntax::parse(&edited, Some(&tree)),
            BatchSize::PerIteration,
        );
    });

    group.bench_function("symbol_index_build", |b| {
        let doc = large();
        b.iter(|| SymbolIndex::build(black_box(&doc.tree), black_box(&doc.text)));
    });

    group.bench_function("semantic_tokens_full", |b| {
        let doc = large();
        b.iter(|| semantic_tokens::tokens(black_box(doc), None));
    });

    group.finish();
}

/// A model whose objective is a single line of `terms` terms.
fn long_objective(terms: usize) -> Document {
    debug_assert!(terms >= 2, "the constraint uses x0 and x1");
    let mut text = String::with_capacity(terms * 12 + 64);
    text.push_str("Minimize\n obj: x0");
    for v in 1..terms {
        put(&mut text, format_args!(" + {} x{v}", v % 7 + 1));
    }
    text.push_str("\nSubject To\n c0: x0 + x1 >= 1\nEnd\n");
    document(text)
}

fn long_line(c: &mut Criterion) {
    let mut group = c.benchmark_group("long_line");
    group.sample_size(10).warm_up_time(Duration::from_secs(1)).measurement_time(Duration::from_secs(10));

    group.bench_function("code_lens_40k_term_objective", |b| {
        let doc = long_objective(40_000);
        doc.build_index();
        b.iter(|| code_lens::lenses(black_box(&doc)));
    });

    group.finish();
}

/// A model of `n` constraints over `n` variables (about 50 bytes each), each
/// variable bounded.
fn many_constraints(n: usize) -> Document {
    debug_assert!(n >= 1);
    let mut text = String::with_capacity(n * 64 + 64);
    text.push_str("Minimize\n obj: x0 + x1\nSubject To\n");
    for i in 0..n {
        let (b, c) = ((i * 7 + 3) % n, (i * 13 + 11) % n);
        put(&mut text, format_args!(" c{i}: {}.5 x{i} + {} x{b} - x{c} >= {}\n", i % 9 + 1, i % 5 + 2, i % 100));
    }
    text.push_str("Bounds\n");
    for v in 0..n {
        put(&mut text, format_args!(" x{v} <= {}\n", v % 1000 + 1));
    }
    text.push_str("End\n");
    document(text)
}

/// [`many_constraints`] with 200k constraints (~10 MB), parsed once.
fn constraints_200k() -> &'static Document {
    static DOC: OnceLock<Document> = OnceLock::new();
    DOC.get_or_init(|| many_constraints(200_000))
}

fn many(c: &mut Criterion) {
    let mut group = c.benchmark_group("constraints_200k");
    group.sample_size(10).warm_up_time(Duration::from_secs(1)).measurement_time(Duration::from_secs(10));

    group.bench_function("apply_changes_1000_edits", |b| {
        // One `didChange` carrying 1000 single-character insertions, each a
        // line further down from the middle (ranges refer to the text after
        // the previous change).
        let doc = constraints_200k();
        let first = doc.lines.line_of(doc.text.len() / 2);
        let changes: Vec<TextDocumentContentChangeEvent> = (0..1000u32)
            .map(|k| {
                let position = tower_lsp_server::ls_types::Position::new(u32::try_from(first).expect("line fits") + k, 1);
                let range = tower_lsp_server::ls_types::Range::new(position, position);
                TextDocumentContentChangeEvent { range: Some(range), range_length: None, text: "d".to_owned() }
            })
            .collect();
        b.iter_batched(
            || doc.clone(),
            |mut doc| {
                doc.apply_changes(&changes, 2);
                doc
            },
            BatchSize::PerIteration,
        );
    });

    group.bench_function("code_lens_serialised", |b| {
        // What `textDocument/codeLens` costs the server: compute and encode.
        static SIZE: Once = Once::new();
        let doc = constraints_200k();
        doc.build_index();
        SIZE.call_once(|| {
            eprintln!("codeLens response: {} bytes", serde_json::to_vec(&code_lens::lenses(doc)).expect("lenses serialise").len());
        });
        b.iter(|| serde_json::to_vec(&code_lens::lenses(black_box(doc))).expect("lenses serialise"));
    });

    group.bench_function("completion_after_label_serialised", |b| {
        // Completion right after a constraint label (`c100000: |`), where
        // every variable is a candidate: what a trigger character costs.
        static SIZE: Once = Once::new();
        let doc = constraints_200k();
        doc.build_index();
        let at = doc.text.find(" c100000: ").expect("constraint c100000") + " c100000: ".len();
        let position = doc.position(at);
        SIZE.call_once(|| {
            let bytes = serde_json::to_vec(&completion::complete(doc, position)).expect("items serialise").len();
            eprintln!("completion response: {bytes} bytes");
        });
        b.iter(|| serde_json::to_vec(&completion::complete(black_box(doc), position)).expect("items serialise"));
    });

    group.bench_function("format_on_type_enter", |b| {
        // Enter pressed at the end of a constraint near the middle.
        let base = constraints_200k();
        let line = base.lines.line_of(base.text.len() / 2);
        let end = base.lines.line_range(&base.text, line).end;
        let mut text = base.text.clone();
        text.insert(end, '\n');
        let doc = document(text);
        let position = tower_lsp_server::ls_types::Position::new(u32::try_from(line + 1).expect("line fits"), 0);
        let settings = FormatSettings::default();
        b.iter(|| format::format_on_type(black_box(&doc), position, "\n", &settings));
    });

    group.bench_function("code_actions_at_cursor", |b| {
        // What the client asks for on every cursor move: actions for an
        // empty range inside a constraint near the middle.
        let doc = constraints_200k();
        doc.build_index();
        let at = doc.position(middle_constraint(doc));
        let range = tower_lsp_server::ls_types::Range::new(at, at);
        let context = CodeActionContext::default();
        b.iter(|| code_action::actions(black_box(doc), range, &context, true));
    });

    group.bench_function("diagnostics", |b| {
        // Diagnostics for a clean model before the semantic pass: what a
        // pull-mode client asks for after every edit.
        let doc = constraints_200k();
        doc.build_index();
        let config = Config::default();
        b.iter(|| diagnostics::compute(black_box(doc), &config));
    });

    group.bench_function("document_symbols_serialised", |b| {
        // What `textDocument/documentSymbol` costs the server: compute and encode.
        let doc = constraints_200k();
        doc.build_index();
        b.iter(|| serde_json::to_vec(&symbols::document_symbols(black_box(doc))).expect("symbols serialise"));
    });

    group.bench_function("folding_ranges", |b| {
        // What `textDocument/foldingRange` costs after every edit.
        let doc = constraints_200k();
        b.iter(|| folding::ranges(black_box(doc)));
    });

    group.bench_function("inlay_hints_viewport", |b| {
        // Hints for a 60-line viewport near the middle, after the semantic pass.
        let mut doc = constraints_200k().clone();
        doc.semantic_result = Some(Arc::new(semantic::run(&doc.text, doc.version, &AnalysisConfig::default())));
        assert!(doc.semantic().and_then(|s| s.model()).is_some(), "generated model parses upstream");
        doc.build_index();
        let first = u32::try_from(doc.lines.line_of(doc.text.len() / 2)).expect("line fits");
        let range = tower_lsp_server::ls_types::Range::new(
            tower_lsp_server::ls_types::Position::new(first, 0),
            tower_lsp_server::ls_types::Position::new(first + 60, 0),
        );
        let settings = InlayHintSettings::default();
        b.iter(|| inlay::hints(black_box(&doc), range, &settings));
    });

    group.finish();
}

fn medium(c: &mut Criterion) {
    let mut group = c.benchmark_group("constraints_50k");
    group.sample_size(10).warm_up_time(Duration::from_secs(1)).measurement_time(Duration::from_secs(10));

    group.bench_function("symbol_index_build", |b| {
        // Below the parallel threshold (~2.5 MB): built on one thread after every edit.
        let doc = many_constraints(50_000);
        assert_eq!(lp_lsp::index::workers(doc.text.len()), 1, "built on one thread");
        b.iter(|| SymbolIndex::build(black_box(&doc.tree), black_box(&doc.text)));
    });

    group.finish();
}

fn small(c: &mut Criterion) {
    let mut group = c.benchmark_group("constraints_15k");
    group.sample_size(10).warm_up_time(Duration::from_secs(1)).measurement_time(Duration::from_secs(10));

    group.bench_function("format_on_type_enter", |b| {
        // Enter pressed at the end of a constraint near the middle of a model
        // small enough (under 1 MiB) to be laid out and re-parsed.
        let base = many_constraints(15_000);
        let line = base.lines.line_of(base.text.len() / 2);
        let end = base.lines.line_range(&base.text, line).end;
        let mut text = base.text.clone();
        text.insert(end, '\n');
        let doc = document(text);
        assert!(doc.text.len() <= format::ON_TYPE_REFORMAT_MAX_BYTES, "laid out on Enter");
        let position = tower_lsp_server::ls_types::Position::new(u32::try_from(line + 1).expect("line fits"), 0);
        let settings = FormatSettings::default();
        b.iter(|| format::format_on_type(black_box(&doc), position, "\n", &settings));
    });

    group.finish();
}

criterion_group!(lsp, large_file, long_line, many, medium, small);
criterion_main!(lsp);
