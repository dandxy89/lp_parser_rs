//! Workspace scan: find and index every `*.lp` file under the workspace roots.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

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
            let file_type = match entry.file_type() {
                Ok(file_type) => file_type,
                Err(e) => {
                    errors.push(format!("cannot stat {}: {e}", path.display()));
                    continue;
                }
            };
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

/// Canonical spelling of a document URI, for comparing URIs from different
/// sources: a `file:` URI is rebuilt from its path, so `file:///c%3A/x.lp`
/// and `file:///C:/x.lp` (Windows) or `%61` and `a` compare equal. Other
/// schemes are returned unchanged.
#[must_use]
pub fn key(uri: &Uri) -> Uri {
    if !uri.scheme().as_str().eq_ignore_ascii_case("file") {
        return uri.clone();
    }
    uri.to_file_path().and_then(Uri::from_file_path).unwrap_or_else(|| uri.clone())
}

/// Whether `path`, below `root`, lies in one of the directories never
/// indexed (`target`, `.git`, ...).
#[must_use]
pub fn in_skipped_dir(root: &Path, path: &Path) -> bool {
    path.strip_prefix(root).is_ok_and(|rel| rel.components().any(|c| SKIP_DIRS.iter().any(|skip| c.as_os_str() == *skip)))
}

/// A file read from disk and indexed.
#[derive(Debug)]
pub struct Loaded {
    /// The parsed and indexed document.
    pub doc: Document,
    /// Modification time when it was read, if the platform reports one.
    pub modified: Option<SystemTime>,
}

/// Read and index one file from disk, unless it is larger than `max_bytes`.
///
/// # Errors
/// When the file is too large or cannot be read, or its path is not
/// representable as a URI.
pub fn load(path: &Path, encoding: Encoding, max_bytes: usize) -> Result<Loaded, String> {
    let metadata = std::fs::metadata(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let size = usize::try_from(metadata.len()).unwrap_or(usize::MAX);
    if size > max_bytes {
        return Err(format!("not indexing {} ({size} bytes): the workspace index is full", path.display()));
    }
    // Read the time first: a write during the read then yields an older time,
    // so the next load of the file wins.
    let modified = metadata.modified().ok();
    let text = std::fs::read_to_string(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let uri = Uri::from_file_path(path).ok_or_else(|| format!("cannot build a URI for {}", path.display()))?;
    let doc = Document::new(uri, text, 0, encoding);
    // Workspace files are loaded in the background; build the index now so
    // workspace symbols and cross-file rename never wait for it.
    doc.build_index();
    Ok(Loaded { doc, modified })
}

/// Files indexed from disk, keyed by [`key`], with their total size.
#[derive(Debug, Default)]
pub struct Index {
    docs: HashMap<Uri, Arc<Document>>,
    modified: HashMap<Uri, SystemTime>,
    /// Per file, bumped whenever its indexed copy changes.
    revisions: HashMap<Uri, u64>,
    next_revision: u64,
    bytes: usize,
}

impl Index {
    /// The indexed copy of the file at canonical URI `key`.
    #[must_use]
    pub fn get(&self, key: &Uri) -> Option<&Arc<Document>> {
        self.docs.get(key)
    }

    /// Every indexed file.
    pub fn iter(&self) -> impl Iterator<Item = (&Uri, &Arc<Document>)> {
        self.docs.iter()
    }

    /// Revision of the indexed copy at `key`; changes whenever it is replaced.
    #[must_use]
    pub fn revision(&self, key: &Uri) -> Option<u64> {
        self.revisions.get(key).copied()
    }

    /// Total text size of the indexed files.
    #[must_use]
    pub const fn bytes(&self) -> usize {
        self.bytes
    }

    /// Store `loaded`, unless a copy read later from disk is already there
    /// (loads can finish out of order).
    pub fn insert(&mut self, loaded: Loaded) {
        let key = key(&loaded.doc.uri);
        if let (Some(new), Some(old)) = (loaded.modified, self.modified.get(&key))
            && new < *old
        {
            return;
        }
        self.remove(&key);
        self.bytes += loaded.doc.text.len();
        if let Some(modified) = loaded.modified {
            self.modified.insert(key.clone(), modified);
        }
        self.next_revision += 1;
        self.revisions.insert(key.clone(), self.next_revision);
        self.docs.insert(key, Arc::new(loaded.doc));
    }

    /// Drop the file at canonical URI `key`.
    pub fn remove(&mut self, key: &Uri) {
        self.modified.remove(key);
        self.revisions.remove(key);
        if let Some(doc) = self.docs.remove(key) {
            debug_assert!(self.bytes >= doc.text.len(), "index size underflow");
            self.bytes -= doc.text.len();
        }
    }

    /// Drop every file for which `drop` holds.
    pub fn remove_where(&mut self, drop: impl Fn(&Uri) -> bool) {
        let doomed: Vec<Uri> = self.docs.keys().filter(|uri| drop(uri)).cloned().collect();
        for uri in &doomed {
            self.remove(uri);
        }
    }
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
    fn keys_ignore_uri_spelling() {
        let plain: Uri = "file:///tmp/a.lp".parse().unwrap();
        let encoded: Uri = "file:///tmp/%61.lp".parse().unwrap();
        assert_eq!(key(&encoded), key(&plain));
        assert_eq!(key(&key(&plain)), key(&plain));
        let untitled: Uri = "untitled:Untitled-1".parse().unwrap();
        assert_eq!(key(&untitled), untitled);
    }

    #[test]
    fn index_keeps_the_newest_copy_and_its_size() {
        let loaded = |text: &str, secs: u64| Loaded {
            doc: Document::new("file:///tmp/a.lp".parse().unwrap(), text.to_owned(), 0, Encoding::Utf16),
            modified: Some(SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(secs)),
        };
        let mut index = Index::default();
        index.insert(loaded("newer", 2));
        index.insert(loaded("old", 1));
        let key: Uri = "file:///tmp/a.lp".parse().unwrap();
        assert_eq!(index.get(&key).unwrap().text, "newer");
        let revision = index.revision(&key).unwrap();
        index.insert(loaded("newest", 3));
        assert_ne!(index.revision(&key), Some(revision));
        assert_eq!(index.bytes(), 6);
        index.remove(&key);
        assert_eq!(index.bytes(), 0);
        assert!(index.get(&key).is_none());
    }

    #[test]
    fn skipped_dirs_are_relative_to_the_root() {
        let root = Path::new("/work/.venv/project");
        assert!(!in_skipped_dir(root, &root.join("model.lp")));
        assert!(in_skipped_dir(root, &root.join("target/out.lp")));
    }

    #[test]
    fn missing_root_is_reported_not_fatal() {
        let (files, errors) = find_lp_files(&[PathBuf::from("/definitely/not/here")]);
        assert_eq!(files, Vec::<PathBuf>::new());
        assert_eq!(errors.len(), 1);
    }
}
