//! Snapshot tests for the LSP diagnostics of fixture models: code, source,
//! severity, tags, range, related information and `data`, under the default
//! settings and under reshaped ones.

use std::fmt::Write;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

use lp_lsp::features::diagnostics::{SOURCE, compute};
use lp_lsp::{Config, Document, Encoding, semantic};
use ls_types::{Diagnostic, DiagnosticSeverity, NumberOrString};

fn fixtures() -> Vec<(String, String)> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/diagnostics");
    let mut found: Vec<(String, String)> = fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("read {}: {e}", dir.display()))
        .map(|entry| {
            let path = entry.expect("directory entry").path();
            let name = path.file_stem().expect("file stem").to_string_lossy().into_owned();
            let text = fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
            (name, text)
        })
        .collect();
    found.sort();
    assert!(found.len() >= 6, "fixtures found: {}", found.len());
    found
}

fn document(text: &str, config: &Config) -> Document {
    let mut doc = Document::new("file:///fixture.lp".parse().expect("valid uri"), text.to_owned(), 1, Encoding::Utf16);
    doc.semantic_result = Some(Arc::new(semantic::run_with(text, 1, &config.analysis.diagnostics_config())));
    doc
}

fn severity(d: &Diagnostic) -> &'static str {
    match d.severity {
        Some(DiagnosticSeverity::ERROR) => "error",
        Some(DiagnosticSeverity::WARNING) => "warning",
        Some(DiagnosticSeverity::INFORMATION) => "information",
        Some(DiagnosticSeverity::HINT) => "hint",
        other => panic!("unexpected severity {other:?}"),
    }
}

/// One block per diagnostic: position, severity, code, then the details.
fn render(doc: &Document, diagnostics: &[Diagnostic]) -> String {
    let mut out = String::new();
    for d in diagnostics {
        let Some(NumberOrString::String(code)) = &d.code else { panic!("string code expected: {d:?}") };
        assert_eq!(d.source.as_deref(), Some(SOURCE));
        let (s, e) = (d.range.start, d.range.end);
        let tags = d.tags.as_ref().map_or_else(String::new, |t| format!(" tags={}", t.len()));
        writeln!(
            out,
            "{}:{}-{}:{} {} {code}{tags} `{}`",
            s.line + 1,
            s.character,
            e.line + 1,
            e.character,
            severity(d),
            doc.slice(doc.byte_range(d.range))
        )
        .expect("write to string");
        for line in d.message.lines() {
            writeln!(out, "  | {line}").expect("write to string");
        }
        for related in d.related_information.iter().flatten() {
            let r = related.location.range;
            writeln!(out, "  = {}:{} `{}`: {}", r.start.line + 1, r.start.character, doc.slice(doc.byte_range(r)), related.message)
                .expect("write to string");
        }
        if let Some(data) = &d.data {
            writeln!(out, "  data {data}").expect("write to string");
        }
    }
    out
}

fn configured(value: &serde_json::Value) -> Config {
    Config::from_value(serde_json::json!({ "analysis": value })).expect("valid configuration")
}

#[test]
fn fixture_diagnostics_snapshots() {
    let config = Config::default();
    for (name, text) in fixtures() {
        let doc = document(&text, &config);
        let rendered = render(&doc, &compute(&doc, &config));
        insta::assert_snapshot!(name, rendered);
    }
}

#[test]
fn clean_fixture_has_no_diagnostics() {
    let config = Config::default();
    let doc = document(include_str!("fixtures/diagnostics/clean.lp"), &config);
    assert_eq!(compute(&doc, &config), []);
}

#[test]
fn settings_variants_snapshots() {
    let text = include_str!("fixtures/diagnostics/model_checks.lp");
    let variants = [
        ("no_numerical_no_heuristics", configured(&serde_json::json!({ "categories": { "numerical": false }, "heuristics": false }))),
        (
            "severity_overrides",
            configured(&serde_json::json!({
                "severityOverrides": { "proportional-row": "hint", "duplicate-row": "off", "row-infeasible-by-bounds": "warning" }
            })),
        ),
        ("analysis_disabled", configured(&serde_json::json!({ "enabled": false }))),
    ];
    for (name, config) in variants {
        let doc = document(text, &config);
        insta::assert_snapshot!(format!("model_checks_{name}"), render(&doc, &compute(&doc, &config)));
    }
}

#[test]
fn diagnostics_are_deterministic_across_parses() {
    let config = Config::default();
    for (name, text) in fixtures() {
        let first = compute(&document(&text, &config), &config);
        let second = compute(&document(&text, &config), &config);
        assert_eq!(first, second, "{name}");
    }
}
