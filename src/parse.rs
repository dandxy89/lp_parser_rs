use std::{
    fs::File,
    io::{BufReader, Read},
    path::Path,
};

use pest::Parser;
use tiny_id::ShortCodeGenerator;

use crate::{lp_parts::compose, model::lp_problem::LPProblem, LParser, Rule};

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

/// # Errors
/// Returns an error if the parse fails
pub fn parse_lp_file(contents: &str) -> anyhow::Result<LPProblem> {
    let mut parsed = LParser::parse(Rule::LP_FILE, contents)?;
    let Some(pair) = parsed.next() else {
        anyhow::bail!("Invalid LP file");
    };
    let mut parsed_contents = LPProblem::default();
    let mut code_generator = ShortCodeGenerator::new_lowercase_alphanumeric(6);
    for pair in pair.clone().into_inner() {
        parsed_contents = compose(pair, parsed_contents, &mut code_generator)?;
    }
    Ok(parsed_contents)
}
