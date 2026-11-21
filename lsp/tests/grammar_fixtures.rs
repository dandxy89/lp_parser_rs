//! Runs every `tree-sitter-lp` fixture (`test/fixtures/**/*.lp`) through the
//! server's document index and holds the index against the grammar's own
//! `TAGS_QUERY`: no `ERROR` nodes, and the same row and variable names.
//!
//! Needs a checkout of `tree-sitter-lp` for the fixtures: `$TREE_SITTER_LP_DIR`,
//! else `../../tree-sitter-lp` beside this repository. Without one the tests
//! print a notice and skip, unless `$CI` is set, where a missing checkout fails.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use lp_lsp::index::EntityKind;
use lp_lsp::syntax::{self, TagRole};
use lp_lsp::{Document, Encoding};

/// The checkout, or `None` (after printing a notice) when there is none locally.
fn checkout() -> Option<PathBuf> {
    let dir = std::env::var_os("TREE_SITTER_LP_DIR")
        .map_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tree-sitter-lp"), PathBuf::from);
    if dir.join("test/fixtures").is_dir() {
        return Some(dir);
    }
    assert!(std::env::var_os("CI").is_none(), "no tree-sitter-lp fixtures at {}: CI must check out tree-sitter-lp", dir.display());
    eprintln!("SKIPPED: no tree-sitter-lp fixtures at {}: set TREE_SITTER_LP_DIR to a checkout", dir.display());
    None
}

fn lp_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let mut entries: Vec<PathBuf> = std::fs::read_dir(dir).expect("readable fixture directory").map(|e| e.expect("entry").path()).collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            lp_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "lp") {
            out.push(path);
        }
    }
}

fn fixtures() -> Vec<PathBuf> {
    let Some(dir) = checkout() else { return Vec::new() };
    let mut files = Vec::new();
    lp_files(&dir.join("test/fixtures"), &mut files);
    assert!(files.len() >= 10, "expected the shared fixture set, found {}", files.len());
    files
}

fn document(path: &Path) -> Document {
    let text = std::fs::read_to_string(path).expect("readable fixture");
    let uri = format!("file:///fixtures/{}", path.file_name().expect("file name").to_string_lossy());
    Document::new(uri.parse().expect("valid uri"), text, 1, Encoding::Utf16)
}

#[test]
fn every_fixture_parses_without_error_nodes_and_indexes() {
    for path in fixtures() {
        let doc = document(&path);
        assert!(!doc.has_syntax_errors(), "{}: ERROR or MISSING node in {}", path.display(), doc.tree.root_node().to_sexp());
        assert!(!doc.index().entities.is_empty(), "{}: nothing indexed", path.display());
    }
}

#[test]
fn the_index_and_the_tags_query_agree_on_every_fixture() {
    for path in fixtures() {
        let doc = document(&path);
        let tags = syntax::tags(&doc.tree, &doc.text);
        assert!(!tags.is_empty(), "{}: no tags", path.display());

        let rows_by_tags: BTreeSet<&str> = tags.iter().filter(|t| t.role == TagRole::DefinesRow).map(|t| t.name.as_str()).collect();
        let rows_by_index: BTreeSet<&str> = doc
            .index()
            .entities
            .iter()
            .filter(|e| matches!(e.kind, EntityKind::Objective | EntityKind::Constraint | EntityKind::GeneralConstraint | EntityKind::Sos))
            .filter_map(|e| e.name.as_deref())
            .collect();
        assert_eq!(rows_by_tags, rows_by_index, "{}: row names", path.display());

        let variables_by_tags: BTreeSet<&str> = tags.iter().filter(|t| t.role != TagRole::DefinesRow).map(|t| t.name.as_str()).collect();
        let variables_by_index: BTreeSet<&str> = doc.index().variables.iter().map(|v| v.name.as_str()).collect();
        assert_eq!(variables_by_tags, variables_by_index, "{}: variable names", path.display());
    }
}
