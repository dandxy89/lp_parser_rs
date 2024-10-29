use std::{
    fs::File,
    io::{BufReader, Read as _},
    path::Path,
};

use pest::Parser as _;
use unique_id::{sequence::SequenceGenerator, GeneratorFromSeed as _};

use crate::{
    model::{lp_problem::LPProblem, parse_model::compose},
    LParser, Rule,
};

#[inline]
/// # Errors
/// Returns an error if the `read_to_string` or `open` fails
pub fn parse_file(path: &Path) -> anyhow::Result<String> {
    let Ok(file) = File::open(path) else {
        anyhow::bail!("Could not open file at {path:?}");
    };
    let mut buf_reader = BufReader::new(file);
    let mut contents = String::new();
    buf_reader.read_to_string(&mut contents)?;

    Ok(contents)
}

#[inline]
/// # Errors
/// Returns an error if the parse fails
pub fn parse_lp_file(contents: &str) -> anyhow::Result<LPProblem> {
    let mut parsed = LParser::parse(Rule::LP_FILE, contents)?;
    let Some(pair) = parsed.next() else {
        anyhow::bail!("Invalid LP file");
    };
    let mut parsed_contents = LPProblem::default();
    let mut code_generator = SequenceGenerator::new(2024);
    for pair in pair.clone().into_inner() {
        parsed_contents = compose(pair, parsed_contents, &mut code_generator)?;
    }
    Ok(parsed_contents)
}
