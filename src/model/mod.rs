use std::iter::Peekable;

use pest::iterators::Pairs;
use unique_id::{sequence::SequenceGenerator, Generator as _};

use crate::{
    model::{lp_error::LPParserError, prefix::Prefix as _},
    Rule,
};

pub mod coefficient;
pub mod constraint;
pub mod lp_error;
pub mod lp_problem;
pub mod objective;
pub mod parse_model;
pub mod prefix;
pub mod sense;
pub mod sos;
pub mod variable;

pub type ParseResult<T> = Result<Vec<T>, LPParserError>;

fn get_name(parts: &mut Peekable<Pairs<'_, Rule>>, id_gen: &SequenceGenerator, rule: Rule) -> String {
    let Some(part) = parts.peek() else { return String::new() };
    if part.as_rule() == rule {
        parts.next().unwrap().as_str().to_owned()
    } else {
        format!("{}{}", rule.prefix(), id_gen.next_id())
    }
}
