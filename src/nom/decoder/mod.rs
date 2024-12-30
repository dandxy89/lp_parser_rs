pub mod coefficient;
pub mod constraint;
pub mod number;
pub mod objective;
pub mod problem_name;
pub mod sense;
pub mod variable;

pub const VALID_LP_CHARS: [char; 18] = ['!', '#', '$', '%', '&', '(', ')', '_', ',', '.', ';', '?', '@', '\\', '{', '}', '~', '\''];

#[inline]
pub fn is_valid_lp_char(c: char) -> bool {
    c.is_alphanumeric() || VALID_LP_CHARS.contains(&c)
}
