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
pub mod compat;
pub mod context;
#[cfg(feature = "csv")]
pub mod csv;
pub mod error;
pub mod lexer;
pub mod model;
pub mod parser;
pub mod problem;
pub mod writer;

// LALRPOP generated grammar module
use lalrpop_util::lalrpop_mod;

#[allow(
    clippy::redundant_field_names,
    clippy::type_complexity,
    clippy::missing_const_for_fn,
    clippy::too_many_lines,
    clippy::cast_sign_loss,
    clippy::match_same_arms,
    clippy::missing_errors_doc,
    clippy::no_effect_underscore_binding,
    clippy::trivially_copy_pass_by_ref,
    clippy::unnecessary_wraps
)]
mod lp_grammar {
    use super::lalrpop_mod;
    lalrpop_mod!(pub lp);
}
pub use lp_grammar::lp;
