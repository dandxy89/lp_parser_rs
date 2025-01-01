use std::{
    error::Error,
    fs::File,
    io::{BufReader, Read as _},
    path::Path,
};

#[inline]
/// Parses the contents of a file at the given path into a string.
///
/// # Arguments
///
/// * `path` - A reference to a `Path` that represents the file path to be parsed.
///
/// # Returns
///
/// A `Result` containing the file contents as a `String` if successful, or an error
/// if the file cannot be opened or read.
///
/// # Errors
///
/// Returns an error if the `read_to_string` or `open` fails.
pub fn parse_file(path: &Path) -> Result<String, Box<dyn Error>> {
    let file = File::open(path)?;
    let mut buf_reader = BufReader::new(file);

    let mut contents = String::new();
    buf_reader.read_to_string(&mut contents)?;

    Ok(contents)
}
