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
