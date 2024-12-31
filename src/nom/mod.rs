pub mod decoder;
pub mod generator;
pub mod lp_problem;
pub mod model;

use nom::{error::ErrorKind, IResult};

pub const CONSTRAINT_HEADERS: [&str; 4] = ["subject to", "such that", "s.t.", "st:"];

pub const BINARY_HEADERS: [&str; 4] = ["binaries", "binary", "bin", "end"];
pub const ALL_BOUND_HEADERS: [&str; 13] = [
    "bounds",
    "bound",
    "generals",
    "general",
    "integers",
    "integer",
    "binaries",
    "binary",
    "bin",
    "semi-continuous",
    "semis",
    "semi",
    "end",
];
pub const GENERAL_HEADERS: [&str; 3] = ["generals", "general", "end"];
pub const INTEGER_HEADERS: [&str; 3] = ["integers", "integer", "end"];
pub const SEMI_HEADERS: [&str; 4] = ["semi-continuous", "semis", "semi", "end"];
pub const SOS_HEADERS: [&str; 2] = ["sos", "end"];

pub const VALID_LP_CHARS: [char; 18] = ['!', '#', '$', '%', '&', '(', ')', '_', ',', '.', ';', '?', '@', '\\', '{', '}', '~', '\''];

pub fn log_remaining(prefix: &str, remaining: &str) {
    if !remaining.trim().is_empty() {
        log::debug!("{prefix}: {remaining}");
        println!("{prefix}: {remaining}"); // Remove once branch is complete
    }
}

fn take_until_cased<'a>(tag: &'a str) -> impl Fn(&'a str) -> IResult<&'a str, &'a str> {
    move |input: &str| {
        let mut index = 0;
        let tag_lower = tag.to_lowercase();
        let chars: Vec<char> = input.chars().collect();

        if chars.len() < tag.len() {
            return Err(nom::Err::Error(nom::error::Error::new(input, ErrorKind::TakeUntil)));
        }

        while index <= chars.len() - tag.len() {
            let window: String = chars[index..index + tag.len()].iter().collect();
            if window.to_lowercase() == tag_lower {
                return Ok((&input[index..], &input[..index]));
            }
            index += 1;
        }

        Err(nom::Err::Error(nom::error::Error::new(input, ErrorKind::TakeUntil)))
    }
}

pub fn take_until_parser<'a>(tags: &'a [&'a str]) -> impl Fn(&'a str) -> IResult<&'a str, &'a str> + 'a {
    move |input| {
        tags.iter().map(|&tag| take_until_cased(tag)).fold(
            Err(nom::Err::Error(nom::error::Error::new(input, nom::error::ErrorKind::Alt))),
            |acc, parser| match acc {
                Ok(ok) => Ok(ok),
                Err(_) => parser(input),
            },
        )
    }
}
