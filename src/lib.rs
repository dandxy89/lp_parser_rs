//! LP Parser
//!
//! This is a parser for the [LP](https://en.wikipedia.org/wiki/Linear_programming) file format leveraging the [pest](https://pest.rs) crate for parsing.
//!
//! The PEST file has been derived from the following resources:
//! - [IBM v22.1.1 Specification](https://www.ibm.com/docs/en/icos/22.1.1?topic=cplex-lp-file-format-algebraic-representation)
//! - [fico](https://www.fico.com/fico-xpress-optimization/docs/dms2020-03/solver/optimizer/HTML/chapter10_sec_section102.html)
//! - [Gurobi](https://www.gurobi.com/documentation/current/refman/lp_format.html)
//!
//! It supports the following LP file features:
//! - Problem Name
//! - Problem Sense
//! - Objectives
//!   - Single-Objective Case
//!   - Multi-Objective Case
//! - Constraints
//! - Bounds
//! - Variable Types: Integer, Generals, Lower Bounded, Upper Bounded, Free & Upper and Lower Bounded
//! - Semi-continuous
//! - Special Order Sets (SOS)
//!

#![deny(
    rust_2018_idioms,
    unused_must_use,
    clippy::nursery,
    clippy::pedantic,
    clippy::perf,
    clippy::correctness,
    clippy::dbg_macro,
    clippy::else_if_without_else,
    clippy::empty_drop,
    clippy::empty_structs_with_brackets,
    clippy::expect_used,
    clippy::if_then_some_else_none,
    clippy::integer_division,
    clippy::multiple_inherent_impl,
    clippy::panic,
    clippy::print_stderr,
    clippy::print_stdout,
    clippy::same_name_method,
    clippy::string_to_string,
    clippy::todo,
    clippy::try_err,
    clippy::unimplemented,
    clippy::unnecessary_self_imports,
    clippy::unreachable,
    clippy::unwrap_used,
    clippy::wildcard_enum_match_arm
)]
#![allow(clippy::module_name_repetitions)]

use pest_derive::Parser;

pub mod common;
pub mod model;
pub mod lp_parts;
pub mod parse;

#[derive(Parser)]
#[grammar = "lp_file_format.pest"]
pub struct LParser;
