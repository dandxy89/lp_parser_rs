//! Formatter tests: snapshots, idempotence and semantic equivalence over the
//! whole LP corpus, range and on-type formatting, and broken documents.

use std::path::{Path, PathBuf};

use lp_lsp::config::{FormatSettings, KeywordCase};
use lp_lsp::features::format::{format_document, format_on_type, format_range, format_text};
use lp_lsp::{Document, Encoding};
use lp_parser_rs::diff::DiffOptions;
use lp_parser_rs::problem::LpProblem;
use tower_lsp_server::ls_types::{Position, Range, TextEdit};

const EVERYTHING: &str = r"\* Problem: everything *\
\ leading comment

MAXIMIZE multi-objectives
Cost:Priority=2 Weight=-1   3x+2 y \ trailing
- [ x^2+4 x*y ]/2
Time: 2x



Subject To:
c1: x+y=<10 \ cap
c2 :  -x \* inline *\ + 2y =>-4
\ own-line comment


r1: -3 <= x - y <= 8
ind: b = 1 -> x + y <= 3
q1: x + [ x ^ 2 + y ^ 2 ] <= 4
10 >= x
Lazy Constraints
 l1: x+y<=20
User Cuts
 u1: x-y>=-20
General Constraints
 gm: r = MAX ( x , y , -3 )
BOUNDS
x<=inf
-INF<=y<=5
z free
Generals
 g1 g2
Binaries b
semi-continuous
 s1
SOS
s1: S1:: x:1 y:2
s2: s2 :: z:1.5
END
\ after end
";

fn settings() -> FormatSettings {
    FormatSettings::default()
}

fn fmt(text: &str, settings: &FormatSettings) -> String {
    format_text(text, settings).expect("input must format")
}

fn doc(text: &str) -> Document {
    Document::new("file:///test.lp".parse().unwrap(), text.to_owned(), 1, Encoding::Utf16)
}

fn apply(text: &str, doc: &Document, edits: &[TextEdit]) -> String {
    let mut out = text.to_owned();
    let mut ranges: Vec<_> = edits.iter().map(|e| (doc.byte_range(e.range), e.new_text.clone())).collect();
    ranges.sort_by_key(|(r, _)| std::cmp::Reverse(r.start));
    for (range, new_text) in ranges {
        out.replace_range(range, &new_text);
    }
    out
}

#[test]
fn snapshot_default() {
    let out = fmt(EVERYTHING, &settings());
    assert_semantics(EVERYTHING, &out);
    assert_eq!(fmt(&out, &settings()), out);
    insta::assert_snapshot!(out);
}

#[test]
fn snapshot_indent_four_upper() {
    let settings = FormatSettings { indent: 4, keyword_case: KeywordCase::Upper, ..settings() };
    insta::assert_snapshot!(fmt(EVERYTHING, &settings));
}

#[test]
fn snapshot_lower_and_title() {
    let lower = fmt(EVERYTHING, &FormatSettings { keyword_case: KeywordCase::Lower, ..settings() });
    let title = fmt(EVERYTHING, &FormatSettings { keyword_case: KeywordCase::Title, ..settings() });
    insta::assert_snapshot!(format!("{lower}\n----- title -----\n{title}"));
}

#[test]
fn snapshot_aligned() {
    insta::assert_snapshot!(fmt(EVERYTHING, &FormatSettings { align_operators: true, ..settings() }));
}

#[test]
fn snapshot_wrapped() {
    let text = "min\n obj: 12 alpha + 13 beta + 14 gamma - 15 delta + 16 epsilon + 17 zeta + [ x ^ 2 + y ^ 2 + z * x ] / 2\n\
                st\n long: 1 a1 + 2 a2 + 3 a3 + 4 a4 + 5 a5 + 6 a6 + 7 a7 + 8 a8 \\ mid comment\n + 9 a9 >= 1\nend\n";
    insta::assert_snapshot!(fmt(text, &FormatSettings { line_width: 30, ..settings() }));
}

#[test]
fn keyword_named_entries_stay_off_line_start() {
    // `end` and `bin` are names here; at a line start they would be keywords.
    let text = "min\n obj: x + end\nst\n c1: x + end >= 1\nbounds\n x <= 4 end <= 3\ngenerals x end bin\nend\n";
    let out = fmt(text, &settings());
    assert!(out.contains("x <= 4 end <= 3"), "{out}");
    assert!(out.contains("generals x end bin") || out.contains("  x end bin"), "{out}");
    assert_semantics(text, &out);
}

#[test]
fn split_ranged_constraint_stays_on_one_line() {
    let text = "min\n obj: x\nst\n r1: -3 <= x - y <= 8\n c1: x >= 1\n -x + y >= 0\nend\n";
    let out = fmt(text, &settings());
    assert_eq!(out, "min\n  obj: x\nst\n  r1: -3 <= x - y <= 8\n  c1: x >= 1\n  -x + y >= 0\nend\n");
    assert_semantics(text, &out);
}

#[test]
fn syntax_errors_are_never_formatted() {
    let broken = "min\n obj: x +\nst\n c1: x >= \nend\n";
    assert_eq!(format_text(broken, &settings()), None);
    assert_eq!(format_document(&doc(broken), &settings()), None);
    let range = Range::new(Position::new(0, 0), Position::new(1, 0));
    assert_eq!(format_range(&doc(broken), range, &settings()), None);
    assert_eq!(format_on_type(&doc(broken), Position::new(2, 0), "\n", &settings()), None);
}

#[test]
fn already_formatted_document_has_no_edits() {
    let formatted = fmt(EVERYTHING, &settings());
    assert_eq!(format_document(&doc(&formatted), &settings()), Some(vec![]));
}

#[test]
fn document_edit_reproduces_format_text() {
    let d = doc(EVERYTHING);
    let edits = format_document(&d, &settings()).unwrap();
    assert_eq!(apply(EVERYTHING, &d, &edits), fmt(EVERYTHING, &settings()));
}

#[test]
fn crlf_line_endings_are_kept() {
    let text = "min\r\n obj:  x\r\nst\r\n c1: x>=1 \\ note \r\nend\r\n";
    assert_eq!(fmt(text, &settings()), "min\r\n  obj: x\r\nst\r\n  c1: x >= 1 \\ note\r\nend\r\n");
}

#[test]
fn range_formats_only_overlapping_entries() {
    let text = "min\n obj:  x\nst\n c1:x+y>=1\n c2:x+y<=4\n c3:   y>=0\nend\n";
    let d = doc(text);
    // Range inside `c2` only.
    let range = Range::new(Position::new(4, 3), Position::new(4, 5));
    let edits = format_range(&d, range, &settings()).unwrap();
    assert_eq!(edits.len(), 1);
    assert_eq!(edits[0].range, Range::new(Position::new(4, 0), Position::new(4, 10)));
    assert_eq!(apply(text, &d, &edits), "min\n obj:  x\nst\n c1:x+y>=1\n  c2: x + y <= 4\n c3:   y>=0\nend\n");
}

#[test]
fn range_snaps_to_entries_sharing_a_line() {
    let text = "min\n obj: x\nst\n c1: x>=1 c2: y>=2\nend\n";
    let d = doc(text);
    let range = Range::new(Position::new(3, 12), Position::new(3, 13));
    let edits = format_range(&d, range, &settings()).unwrap();
    assert_eq!(apply(text, &d, &edits), "min\n obj: x\nst\n  c1: x >= 1\n  c2: y >= 2\nend\n");
}

#[test]
fn range_already_formatted_is_empty() {
    let formatted = fmt(EVERYTHING, &settings());
    let d = doc(&formatted);
    let range = Range::new(Position::new(0, 0), Position::new(40, 0));
    assert_eq!(format_range(&d, range, &settings()), Some(vec![]));
}

#[test]
fn on_type_newline_formats_previous_entry_and_indents() {
    // The user typed `c1:x+y>=1` and pressed Enter.
    let text = "min\n obj: x\nst\nc1:x+y>=1\n\nend\n";
    let d = doc(text);
    let edits = format_on_type(&d, Position::new(4, 0), "\n", &settings()).unwrap();
    assert_eq!(apply(text, &d, &edits), "min\n obj: x\nst\n  c1: x + y >= 1\n  \nend\n");
}

#[test]
fn on_type_newline_after_header_indents_body() {
    let text = "min\n obj: x\nst\n c1: x >= 1\nBounds\n\nend\n";
    let d = doc(text);
    let edits = format_on_type(&d, Position::new(5, 0), "\n", &settings()).unwrap();
    assert_eq!(apply(text, &d, &edits), "min\n obj: x\nst\n c1: x >= 1\nBounds\n  \nend\n");
}

#[test]
fn on_type_newline_inside_entry_only_indents_continuation() {
    let text = "min\n obj: x\nst\n  c1: x + y\n+ z >= 1\nend\n";
    let d = doc(text);
    let edits = format_on_type(&d, Position::new(4, 0), "\n", &settings()).unwrap();
    assert_eq!(apply(text, &d, &edits), "min\n obj: x\nst\n  c1: x + y\n    + z >= 1\nend\n");
}

#[test]
fn on_type_ignores_other_characters() {
    assert_eq!(format_on_type(&doc(EVERYTHING), Position::new(3, 0), ";", &settings()), None);
}

/// Parse both texts upstream and assert the models are equal.
fn assert_semantics(before: &str, after: &str) {
    let original = LpProblem::parse(before).expect("original parses");
    let formatted = LpProblem::parse(after).unwrap_or_else(|e| panic!("formatted text must parse: {e}\n{after}"));
    assert_eq!(original.name(), formatted.name(), "problem name changed");
    let diff = original.diff(&formatted, &DiffOptions::default());
    assert!(diff.is_empty(), "formatting changed the model: {diff:?}\n{after}");
    assert_eq!(original.constraints.len(), formatted.constraints.len());
    assert_eq!(original.objectives.len(), formatted.objectives.len());
    assert_eq!(original.variables.len(), formatted.variables.len());
}

fn lp_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            lp_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "lp") {
            out.push(path);
        }
    }
}

#[test]
fn corpus_is_idempotent_and_preserves_semantics() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../rust");
    let mut files = Vec::new();
    lp_files(&root.join("resources"), &mut files);
    lp_files(&root.join("tests"), &mut files);
    files.sort();
    assert!(files.len() > 40, "corpus not found");

    let variants = [
        settings(),
        FormatSettings { indent: 4, line_width: 40, align_operators: true, keyword_case: KeywordCase::Upper },
        FormatSettings { indent: 1, line_width: 20, align_operators: false, keyword_case: KeywordCase::Title },
    ];
    let (mut formatted, mut skipped) = (0, Vec::new());
    for path in &files {
        let Ok(text) = std::fs::read_to_string(path) else {
            skipped.push(format!("{}: not UTF-8", path.display()));
            continue;
        };
        if let Err(e) = LpProblem::parse(&text) {
            skipped.push(format!("{}: upstream parse error ({e})", path.display()));
            continue;
        }
        if lp_lsp::syntax::parse(&text, None).root_node().has_error() {
            assert_eq!(format_text(&text, &settings()), None, "{}", path.display());
            skipped.push(format!("{}: tree-sitter syntax error", path.display()));
            continue;
        }
        for settings in &variants {
            let once = format_text(&text, settings).unwrap_or_else(|| panic!("{} did not format with {settings:?}", path.display()));
            let twice = fmt(&once, settings);
            assert_eq!(once, twice, "{} is not idempotent with {settings:?}", path.display());
            assert_semantics(&text, &once);
        }
        formatted += 1;
    }
    eprintln!("formatted {formatted} files, skipped {}:", skipped.len());
    for reason in &skipped {
        eprintln!("  {reason}");
    }
    assert!(formatted >= 40, "too few corpus files formatted: {formatted}");
}
