//! LP Parser
//!
//! A Rust LP file parser leveraging [NOM](https://docs.rs/nom/latest/nom/) and adhering to the following specifications:
//!
//! - [IBM v22.1.1 Specification](https://www.ibm.com/docs/en/icos/22.1.1?topic=cplex-lp-file-format-algebraic-representation)
//! - [fico](https://www.fico.com/fico-xpress-optimization/docs/dms2020-03/solver/optimizer/HTML/chapter10_sec_section102.html)
//! - [Gurobi](https://www.gurobi.com/documentation/current/refman/lp_format.html)
//!
//! It supports the following LP file features:
//! - Problem Name
//! - Problem Senses
//! - Objectives
//!   - Single-Objective Case
//!   - Multi-Objective Case
//! - Constraints
//! - Bounds
//! - Variable Types: Integer, Generals, Lower Bounded, Upper Bounded, Free & Upper and Lower Bounded
//! - Semi-continuous
//! - Special Order Sets (SOS)
//!

pub mod decoder;
pub mod lp_problem;
pub mod model;
pub mod parser;

use nom::{branch::alt, bytes::complete::tag_no_case, error::ErrorKind, IResult};

pub const CONSTRAINT_HEADERS: [&str; 5] = ["subject to", "such that", "s.t.", "st:", "st"];

pub const ALL_BOUND_HEADERS: [&str; 14] = [
    "bounds",
    "bound",
    "generals",
    "general",
    "gen",
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
pub const BINARY_HEADERS: [&str; 4] = ["binaries", "binary", "bin", "end"];
pub const END_HEADER: [&str; 1] = ["end"];
pub const GENERAL_HEADERS: [&str; 4] = ["generals", "general", "gen", "end"];
pub const INTEGER_HEADERS: [&str; 3] = ["integers", "integer", "end"];
pub const SEMI_HEADERS: [&str; 4] = ["semi-continuous", "semis", "semi", "end"];
pub const SOS_HEADERS: [&str; 2] = ["sos", "end"];

pub const VALID_LP_CHARS: [char; 18] = ['!', '#', '$', '%', '&', '(', ')', '_', ',', '.', ';', '?', '@', '\\', '{', '}', '~', '\''];

pub fn log_remaining(prefix: &str, remaining: &str) {
    if !remaining.trim().is_empty() {
        log::debug!("{prefix}: {remaining}");
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

#[allow(clippy::manual_try_fold)]
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

#[inline]
pub fn is_binary_section(input: &str) -> IResult<&str, &str> {
    alt((tag_no_case("binaries"), tag_no_case("binary"), tag_no_case("bin")))(input)
}

#[inline]
pub fn is_bounds_section(input: &str) -> IResult<&str, &str> {
    alt((tag_no_case("bounds"), tag_no_case("bound")))(input)
}

#[inline]
pub fn is_generals_section(input: &str) -> IResult<&str, &str> {
    alt((tag_no_case("generals"), tag_no_case("general"), tag_no_case("gen")))(input)
}

#[inline]
pub fn is_integers_section(input: &str) -> IResult<&str, &str> {
    alt((tag_no_case("integers"), tag_no_case("integer")))(input)
}

#[inline]
pub fn is_semi_section(input: &str) -> IResult<&str, &str> {
    alt((tag_no_case("semis"), tag_no_case("semi")))(input)
}

#[inline]
pub fn is_sos_section(input: &str) -> IResult<&str, &str> {
    tag_no_case("sos")(input)
}
