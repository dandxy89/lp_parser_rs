//! Workspace scan: find and index every `*.lp` file under the workspace roots.

use std::path::{Path, PathBuf};

use tower_lsp_server::ls_types::Uri;

use crate::document::Document;
use crate::position::Encoding;

/// Directories never descended into.
const SKIP_DIRS: &[&str] = &[".git", "target", "node_modules", ".venv", "venv", "__pycache__"];

/// Every `*.lp` file under `roots`, sorted. Unreadable directories are skipped
/// and reported in the second element.
#[must_use]
pub fn find_lp_files(roots: &[PathBuf]) -> (Vec<PathBuf>, Vec<String>) {
    let mut files = Vec::new();
    let mut errors = Vec::new();
    let mut stack: Vec<PathBuf> = roots.to_vec();
    while let Some(dir) = stack.pop() {
        let entries = match std::fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(e) => {
                errors.push(format!("cannot read {}: {e}", dir.display()));
                continue;
            }
        };
        for entry in entries {
            let entry = match entry {
                Ok(entry) => entry,
                Err(e) => {
                    errors.push(format!("cannot read entry in {}: {e}", dir.display()));
                    continue;
                }
            };
            let path = entry.path();
            // `file_type` does not follow symlinks, so symlink loops are impossible.
            let Ok(file_type) = entry.file_type() else { continue };
            if file_type.is_dir() {
                let name = entry.file_name();
                if !SKIP_DIRS.iter().any(|skip| name == *skip) {
                    stack.push(path);
                }
            } else if file_type.is_file() && is_lp_path(&path) {
                files.push(path);
            }
        }
    }
    files.sort();
    (files, errors)
}

/// Whether `path` has an `.lp` extension (case-insensitive).
#[must_use]
pub fn is_lp_path(path: &Path) -> bool {
    path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("lp"))
}

/// Read and index one file from disk.
///
/// # Errors
/// When the file cannot be read or its path is not representable as a URI.
pub fn load(path: &Path, encoding: Encoding) -> Result<Document, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let uri = Uri::from_file_path(path).ok_or_else(|| format!("cannot build a URI for {}", path.display()))?;
    let doc = Document::new(uri, text, 0, encoding);
    // Workspace files are loaded in the background; build the index now so
    // workspace symbols and cross-file rename never wait for it.
    doc.build_index();
    Ok(doc)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_lp_fixtures_and_skips_others() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../rust/resources");
        let (files, errors) = find_lp_files(&[root]);
        assert_eq!(errors, Vec::<String>::new());
        assert!(files.iter().any(|f| f.ends_with("afiro.lp")));
        assert!(files.iter().all(|f| is_lp_path(f)));
    }

    #[test]
    fn missing_root_is_reported_not_fatal() {
        let (files, errors) = find_lp_files(&[PathBuf::from("/definitely/not/here")]);
        assert_eq!(files, Vec::<PathBuf>::new());
        assert_eq!(errors.len(), 1);
    }
}
