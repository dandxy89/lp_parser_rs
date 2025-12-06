//! LP Parser - A Linear Programming File Parser
//!
//! This crate provides robust parsing capabilities for Linear Programming (LP)
//! files using LALRPOP parser generator. It supports multiple industry-standard
//! LP file formats and offers comprehensive features for optimisation problems.
//!
//! # Features
//!
//! - Zero-copy parsing with lifetime management
//! - Support for multiple LP file format specifications
//! - Comprehensive parsing of all standard LP file components
//! - Optional serialisation and diff tracking
//!
//! # Quick Start
//!
//! ```rust
//! use std::path::Path;
//!
//! use lp_parser::{parser::parse_file, LpProblem};
//!
//!
//! fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let content = parse_file(Path::new("problem.lp"))?;
//!     let problem = LpProblem::parse(&content)?;
//!     println!("Problem name: {:?}", problem.name());
//!     Ok(())
//! }
//! ```

pub mod builder;
pub mod context;
#[cfg(feature = "csv")]
pub mod csv;
pub mod error;
pub mod lexer;
pub mod model;
pub mod parser;
pub mod perf;
pub mod problem;
pub mod writer;

// LALRPOP generated grammar module
use lalrpop_util::lalrpop_mod;
#[allow(clippy::redundant_field_names, clippy::type_complexity, clippy::missing_const_for_fn)]
mod lp_grammar {
    use super::*;
    lalrpop_mod!(pub lp);
}
pub use lp_grammar::lp;

/// Headers that indicate the beginning of a constraint section in an LP file.
pub const CONSTRAINT_HEADERS: [&str; 5] = ["subject to", "such that", "s.t.", "st:", "st"];

/// All possible section headers that can appear in an LP file's bounds section.
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

/// Headers that indicate the beginning of a binary variable section.
pub const BINARY_HEADERS: [&str; 4] = ["binaries", "binary", "bin", "end"];

/// Header marking the end of an LP file or section.
pub const END_HEADER: [&str; 1] = ["end"];

/// Headers that indicate the beginning of a general integer variable section.
pub const GENERAL_HEADERS: [&str; 4] = ["generals", "general", "gen", "end"];

/// Headers that indicate the beginning of an integer variable section.
pub const INTEGER_HEADERS: [&str; 3] = ["integers", "integer", "end"];

/// Headers that indicate the beginning of a semi-continuous variable section.
pub const SEMI_HEADERS: [&str; 4] = ["semi-continuous", "semis", "semi", "end"];

/// Headers that indicate the beginning of a Special Ordered Set (SOS) constraint section.
pub const SOS_HEADERS: [&str; 2] = ["sos", "end"];

/// Valid characters that can appear in LP file elements.
///
/// These characters are allowed in addition to alphanumeric
/// characters in names and other elements of LP files.
pub const VALID_LP_FILE_CHARS: [char; 18] = ['!', '#', '$', '%', '&', '(', ')', '_', ',', '.', ';', '?', '@', '\\', '{', '}', '~', '\''];
