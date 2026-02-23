use std::fs::File;
use std::io::{BufReader, Read as _};
use std::path::Path;

use crate::error::{LpParseError, LpResult};

#[inline]
/// Parses the contents of a file at the given path into a string.
///
/// # Arguments
///
/// * `path` - A reference to a `Path` that represents the file path to be parsed.
///
/// # Returns
///
/// A `Result` containing the file contents as a `String` if successful, or an `LpParseError`
/// if the file cannot be opened or read.
///
/// # Errors
///
/// Returns an `LpParseError::IoError` if the file cannot be opened or read.
pub fn parse_file(path: &Path) -> LpResult<String> {
    let file = File::open(path).map_err(|e| LpParseError::io_error(format!("Failed to open file '{}': {}", path.display(), e)))?;

    let mut buf_reader = BufReader::new(file);
    let mut contents = String::new();

    buf_reader
        .read_to_string(&mut contents)
        .map_err(|e| LpParseError::io_error(format!("Failed to read file '{}': {}", path.display(), e)))?;

    Ok(contents)
}

/// A memory-mapped file that can be borrowed as a `&str`.
///
/// Avoids copying file contents into a heap-allocated `String` by letting the OS
/// manage paging via `mmap`. The file must be valid UTF-8.
#[cfg(feature = "mmap")]
pub struct MappedFile {
    _mmap: memmap2::Mmap,
    /// SAFETY: Points into `_mmap`'s mapping, which lives as long as this struct.
    content: *const str,
}

// SAFETY: The underlying Mmap is Send + Sync and the raw pointer only references
// memory owned by _mmap, which cannot be mutated or freed while MappedFile exists.
#[cfg(feature = "mmap")]
unsafe impl Send for MappedFile {}
// SAFETY: See Send impl — no interior mutability, content is immutable.
#[cfg(feature = "mmap")]
unsafe impl Sync for MappedFile {}

#[cfg(feature = "mmap")]
impl MappedFile {
    /// Memory-map a file and validate its contents as UTF-8.
    ///
    /// # Safety
    ///
    /// Uses `unsafe` for `Mmap::map` (undefined behaviour if the file is modified
    /// externally while mapped) and for the internal raw pointer to the mapped
    /// region. The pointer is valid for the lifetime of the returned `MappedFile`
    /// because the `Mmap` is owned by the struct and dropped only when the struct
    /// is dropped.
    ///
    /// # Errors
    ///
    /// Returns an `LpParseError::IoError` if the file cannot be opened, mapped,
    /// or is not valid UTF-8.
    pub fn open(path: &Path) -> LpResult<Self> {
        let file =
            File::open(path).map_err(|e| LpParseError::io_error(format!("Failed to open file '{}': {}", path.display(), e)))?;

        // SAFETY: The file is opened read-only. Concurrent external modification
        // would be undefined behaviour, but this is the standard trade-off for
        // memory-mapped I/O and matches the documented contract of memmap2::Mmap.
        let mmap = unsafe {
            memmap2::Mmap::map(&file)
                .map_err(|e| LpParseError::io_error(format!("Failed to mmap file '{}': {}", path.display(), e)))?
        };

        let content_str = std::str::from_utf8(&mmap)
            .map_err(|e| LpParseError::io_error(format!("File '{}' is not valid UTF-8: {}", path.display(), e)))?;

        // Store a raw pointer so MappedFile is self-referential without lifetimes.
        let content: *const str = content_str;

        Ok(Self { _mmap: mmap, content })
    }

    /// Borrow the file contents as a string slice.
    #[inline]
    #[must_use]
    pub const fn as_str(&self) -> &str {
        // SAFETY: `content` points into `_mmap` which is alive for `&self`'s lifetime.
        unsafe { &*self.content }
    }
}
