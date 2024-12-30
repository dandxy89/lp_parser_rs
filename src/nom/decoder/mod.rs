pub mod coefficient;
pub mod constraint;
pub mod lp_problem;
pub mod number;
pub mod objective;
pub mod problem_name;
pub mod sense;
pub mod variable;

pub const VALID_LP_CHARS: [char; 18] = ['!', '#', '$', '%', '&', '(', ')', '_', ',', '.', ';', '?', '@', '\\', '{', '}', '~', '\''];
pub const SECTION_HEADERS: [&str; 12] =
    ["integers", "integer", "general", "generals", "gen", "binaries", "binary", "bin", "bounds", "bound", "sos", "end"];

#[inline]
pub fn valid_lp_char(c: char) -> bool {
    c.is_alphanumeric() || VALID_LP_CHARS.contains(&c)
}
