#![allow(clippy::multiple_crate_versions)]
#![warn(missing_docs)]

//! Parser, writer and analysis tools for Linear Programming (LP) and MPS files.
//!
//! The LP reader accepts the CPLEX, Gurobi, FICO Xpress and Mosek dialects of
//! the format, including quadratic objectives and constraints, indicator,
//! lazy and general constraints, SOS sets, semi-continuous and semi-integer
//! variables. Parsing goes through three stages:
//!
//! 1. [`lexer::Lexer`] (Logos) tokenises the input. It is separate from the
//!    grammar because LP keywords are context-sensitive: `min`, `bin` or
//!    `free` are valid variable names in most positions. The lexer decides
//!    from the surrounding tokens, so the grammar only sees a keyword where
//!    one can appear.
//! 2. The LALRPOP grammar ([`lp`]) builds a [`ParseResult`] that borrows names
//!    from the input. Objective and constraint bodies are collected as flat
//!    element lists and split into entries by [`assemble`], since finding
//!    where one entry ends needs more than one token of lookahead.
//! 3. [`LpProblem`] interns every name into a [`NameInterner`] and stores
//!    [`NameId`]s. The resulting model owns its data, has no lifetime tied to
//!    the input, and compares or hashes names as `u32`s.
//!
//! MPS input ([`mps`]) produces the same [`ParseResult`] and so the same
//! model. [`writer`] and [`mps::writer`] write either format back out,
//! [`analysis`] reports statistics and numerical issues, and the `diff`
//! feature adds a structural and numeric diff engine (`diff`).
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
//! To read from disk, load the file with [`parser::parse_file`] (or map it
//! with `parser::MappedFile` under the `mmap` feature) and pass the text to
//! [`LpProblem::parse`] or [`LpProblem::parse_mps`].
//!
//! # Feature flags
//!
//! - `serde`: `Serialize`/`Deserialize` for [`LpProblem`] and the model types.
//! - `diff`: the `diff` module and `LpProblem::diff` (enables `serde`).
//! - `csv`: `LpProblem::to_csv`, which writes one CSV file per section.
//! - `mmap`: `parser::MappedFile` for memory-mapped input.
//! - `lp-solvers`: an adapter to the `lp-solvers` crate (`compat::lp_solvers`).
//! - `cli`: builds the `lp_parser` binary.

pub mod analysis;
/// Assembly of flat LP section bodies into raw objectives/constraints.
pub mod assemble;
/// Ergonomic programmatic construction of an [`LpProblem`] ([`ProblemBuilder`]).
pub mod builder;
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
/// Byte-offset to line/column mapping for diagnostics.
pub mod line_index;
pub mod model;
pub mod mps;
/// File reading helpers (plain or memory-mapped with the `mmap` feature).
pub mod parser;
/// The [`LpProblem`] model: parse entry points and mutation API.
pub mod problem;
pub mod writer;

// Crate-root re-exports of the primary public API, so downstream users do not
// need deep module paths for the most common types and entry points.
pub use builder::ProblemBuilder;
#[cfg(feature = "diff")]
pub use diff::{DiffOptions, DiffTol, LpDiff, Normaliser};
pub use error::{EntityKind, LpParseError, LpResult, ParseContext};
pub use interner::{NameId, NameInterner};
// LALRPOP generated grammar module
use lalrpop_util::lalrpop_mod;
pub use lexer::ParseResult;
pub use line_index::{LineIndex, SourceLocation};
pub use model::{ConstraintClass, GeneralFunction, ObjectiveAttributes, QuadraticTerm, VariableBounds, VariableKind};
pub use mps::{extract_mps_name, parse_mps};
pub use problem::LpProblem;

#[allow(
    clippy::cast_sign_loss,
    clippy::cloned_instead_of_copied,
    clippy::cognitive_complexity,
    clippy::elidable_lifetime_names,
    clippy::ignored_unit_patterns,
    clippy::large_enum_variant,
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
    clippy::unused_self,
    clippy::use_self,
    missing_docs
)]
mod lp_grammar {
    use super::lalrpop_mod;
    lalrpop_mod!(pub lp);
}

/// The LALRPOP-generated LP parser.
///
/// The grammar's start symbol yields a boxed [`ParseResult`]: LALRPOP keeps
/// every value on its parse stack in one enum as large as the largest
/// value, so an unboxed `ParseResult` would make each token move several
/// hundred bytes. [`LpProblemParser`](lp::LpProblemParser) unboxes the result,
/// keeping the interface the generated parser has always had.
pub mod lp {
    use lalrpop_util::ParseError;

    use crate::lexer::{LexerError, ParseResult, Token};
    pub use crate::lp_grammar::lp::__ToTriple;

    /// Parser for a whole LP file, driven by a [`Lexer`](crate::lexer::Lexer).
    pub struct LpProblemParser {
        inner: crate::lp_grammar::lp::BoxedLpProblemParser,
    }

    impl Default for LpProblemParser {
        fn default() -> Self {
            Self::new()
        }
    }

    impl LpProblemParser {
        /// Create a parser.
        #[must_use]
        pub fn new() -> Self {
            Self { inner: crate::lp_grammar::lp::BoxedLpProblemParser::new() }
        }

        /// Parse a token stream into a [`ParseResult`].
        ///
        /// # Errors
        ///
        /// Returns the parse error for a token stream that is not a valid LP
        /// file, or the lexer's error for input it cannot tokenise.
        pub fn parse<'input, T, I>(&self, tokens: I) -> Result<ParseResult<'input>, ParseError<usize, Token<'input>, LexerError>>
        where
            T: __ToTriple<'input>,
            I: IntoIterator<Item = T>,
        {
            self.inner.parse(tokens).map(|parsed| *parsed)
        }
    }
}

/// Tolerance for floating-point comparisons in coefficient handling.
/// Used for checking if values are effectively zero or one.
pub(crate) const NUMERIC_EPSILON: f64 = 1e-10;

/// Magnitude at or beyond which a parsed bound is treated as infinite, per
/// the CPLEX convention shared by LP and MPS readers (`1e30` means "no bound").
pub(crate) const INFINITE_BOUND_THRESHOLD: f64 = 1e30;
