#![allow(clippy::multiple_crate_versions)]
#![warn(missing_docs)]

//! LP Parser - A Linear Programming File Parser
//!
//! This crate provides robust parsing capabilities for Linear Programming (LP)
//! files using LALRPOP parser generator. It supports multiple industry-standard
//! LP file formats and offers comprehensive features for optimisation problems.
//!
//! # Features
//!
//! - Owned, interned problem model (no lifetimes) built from a zero-copy grammar
//! - Support for multiple LP file format specifications
//! - Comprehensive parsing of all standard LP file components
//! - Writers for both LP ([`writer`]) and MPS ([`mps::writer`]) output
//! - Optional serialisation (`serde`) and a structural/numeric diff engine
//!   ([`diff`], behind the `diff` feature)
//!
//! # Quick Start
//!
//! ```rust
//! use lp_parser_rs::LpProblem;
//!
//! let input = "\
//! Minimize
//!  obj: 2 x + 3 y
//! Subject To
//!  c1: x + y >= 10
//! Bounds
//!  0 <= x <= 40
//! End";
//!
//! let problem = LpProblem::parse(input)?;
//! assert_eq!(problem.variable_count(), 2);
//! assert_eq!(problem.constraint_count(), 1);
//! # Ok::<(), lp_parser_rs::LpParseError>(())
//! ```
//!
//! To read from disk, use [`parser::parse_file`] to load the file contents
//! (memory-mapped with the `mmap` feature) and pass them to
//! [`LpProblem::parse`] or [`LpProblem::parse_mps`].

pub mod analysis;
/// Assembly of flat LP section bodies into raw objectives/constraints.
pub mod assemble;
/// Compatibility adapters for external solver crates.
pub mod compat;
#[cfg(feature = "csv")]
pub mod csv;
/// Structural and numeric diff engine for two [`LpProblem`]s (behind the `diff` feature).
#[cfg(feature = "diff")]
pub mod diff;
/// Error types returned by the parsers ([`LpParseError`], [`LpResult`]).
pub mod error;
pub mod interner;
pub mod lexer;
pub mod model;
pub mod mps;
/// File reading helpers (plain or memory-mapped with the `mmap` feature).
pub mod parser;
/// The [`LpProblem`] model: parse entry points and mutation API.
pub mod problem;
pub mod writer;

// Crate-root re-exports of the primary public API, so downstream users do not
// need deep module paths for the most common types and entry points.
#[cfg(feature = "diff")]
pub use diff::{DiffOptions, DiffTol, LpDiff, Normaliser, compare as compare_diff};
pub use error::{LpParseError, LpResult};
pub use interner::{NameId, NameInterner};
// LALRPOP generated grammar module
use lalrpop_util::lalrpop_mod;
pub use lexer::ParseResult;
pub use mps::{extract_mps_name, parse_mps};
pub use problem::LpProblem;

#[allow(
    clippy::cast_sign_loss,
    clippy::cloned_instead_of_copied,
    clippy::cognitive_complexity,
    clippy::elidable_lifetime_names,
    clippy::ignored_unit_patterns,
    clippy::match_same_arms,
    clippy::missing_const_for_fn,
    clippy::missing_errors_doc,
    clippy::must_use_candidate,
    clippy::no_effect_underscore_binding,
    clippy::option_if_let_else,
    clippy::redundant_field_names,
    clippy::redundant_pub_crate,
    clippy::too_many_lines,
    clippy::trivially_copy_pass_by_ref,
    clippy::type_complexity,
    clippy::unnecessary_wraps,
    clippy::use_self,
    missing_docs
)]
mod lp_grammar {
    use super::lalrpop_mod;
    lalrpop_mod!(pub lp);
}
pub use lp_grammar::lp;

/// Tolerance for floating-point comparisons in coefficient handling.
/// Used for checking if values are effectively zero or one.
pub(crate) const NUMERIC_EPSILON: f64 = 1e-10;
