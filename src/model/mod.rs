use std::iter::Peekable;

use pest::iterators::Pairs;
use unique_id::{sequence::SequenceGenerator, Generator as _};

use crate::{model::prefix::Prefix as _, Rule};

pub mod coefficient;
pub mod constraint;
pub mod lp_problem;
pub mod objective;
pub mod parse_model;
pub mod prefix;
pub mod sense;
pub mod sos;
pub mod variable;

fn get_name(parts: &mut Peekable<Pairs<'_, Rule>>, gen: &SequenceGenerator, rule: Rule) -> String {
    if parts.peek().unwrap().as_rule() == rule {
        parts.next().unwrap().as_str().to_owned()
    } else {
        format!("{}{}", rule.prefix(), gen.next_id())
    }
}
