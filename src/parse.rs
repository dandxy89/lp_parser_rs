use std::{
    fs::File,
    io::{BufReader, ErrorKind, Read as _},
    path::Path,
};

use pest::Parser as _;
use unique_id::{sequence::SequenceGenerator, GeneratorFromSeed as _};

use crate::{
    model::{lp_error::LPParserError, lp_problem::LPProblem, parse_model::compose},
    LParser, Rule,
};

#[inline]
/// # Errors
/// Returns an error if the `read_to_string` or `open` fails
pub fn parse_file(path: &Path) -> Result<String, LPParserError> {
    let file = File::open(path).map_err(LPParserError::IOError)?;
    let mut buf_reader = BufReader::new(file);

    let mut contents = String::new();
    buf_reader.read_to_string(&mut contents)?;

    Ok(contents)
}

#[inline]
/// # Errors
/// Returns an error if the parse fails
pub fn parse_lp_file(contents: &str) -> Result<LPProblem, LPParserError> {
    let mut parsed = LParser::parse(Rule::LP_FILE, contents).map_err(|err| LPParserError::FileParseError(err.to_string()))?;

    let Some(pair) = parsed.next() else {
        log::warn!("Unexpected EOF in parse_lp_file");
        return Err(LPParserError::IOError(std::io::Error::from(ErrorKind::UnexpectedEof)));
    };

    let mut parsed_contents = LPProblem::default();
    let mut code_generator = SequenceGenerator::new(2024);

    for pair in pair.clone().into_inner() {
        parsed_contents = compose(pair, parsed_contents, &mut code_generator)?;
    }

    Ok(parsed_contents)
}
