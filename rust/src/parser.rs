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
///
/// `Send`/`Sync` are auto-derived: `memmap2::Mmap` is already `Send + Sync` and
/// this struct holds no other state.
#[cfg(feature = "mmap")]
pub struct MappedFile {
    mmap: memmap2::Mmap,
}

#[cfg(feature = "mmap")]
impl MappedFile {
    /// Memory-map a file and validate its contents as UTF-8.
    ///
    /// # Safety
    ///
    /// Uses `unsafe` for `Mmap::map`: behaviour is undefined if the file is
    /// modified externally while mapped. This is the standard trade-off for
    /// memory-mapped I/O and matches the documented contract of `memmap2::Mmap`.
    ///
    /// # Errors
    ///
    /// Returns an `LpParseError::IoError` if the file cannot be opened, mapped,
    /// or is not valid UTF-8.
    pub fn open(path: &Path) -> LpResult<Self> {
        let file = File::open(path).map_err(|e| LpParseError::io_error(format!("Failed to open file '{}': {}", path.display(), e)))?;

        // SAFETY: The file is opened read-only. Concurrent external modification
        // would be undefined behaviour, but this is the standard trade-off for
        // memory-mapped I/O and matches the documented contract of memmap2::Mmap.
        let mmap = unsafe {
            memmap2::Mmap::map(&file).map_err(|e| LpParseError::io_error(format!("Failed to mmap file '{}': {}", path.display(), e)))?
        };

        // Validate UTF-8 once here so `as_str` can rely on the invariant.
        std::str::from_utf8(&mmap).map_err(|e| LpParseError::io_error(format!("File '{}' is not valid UTF-8: {}", path.display(), e)))?;

        Ok(Self { mmap })
    }

    /// Borrow the file contents as a string slice.
    #[inline]
    #[must_use]
    pub fn as_str(&self) -> &str {
        // SAFETY: the bytes were validated as UTF-8 in `open` and the mapping is
        // immutable for the lifetime of `&self`.
        unsafe { std::str::from_utf8_unchecked(&self.mmap) }
    }
}
