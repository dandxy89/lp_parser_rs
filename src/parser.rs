use std::{
    error::Error,
    fs::File,
    io::{BufReader, Read as _},
    path::Path,
};

#[inline]
/// # Errors
/// Returns an error if the `read_to_string` or `open` fails
pub fn parse_file(path: &Path) -> Result<String, Box<dyn Error>> {
    let file = File::open(path)?;
    let mut buf_reader = BufReader::new(file);

    let mut contents = String::new();
    buf_reader.read_to_string(&mut contents)?;

    Ok(contents)
}
