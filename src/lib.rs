//! LP Parser
//!
//! A Rust LP file parser leveraging [PEST](https://docs.rs/pest/latest/pest/) and adhering to the following specifications:
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

#![allow(clippy::module_name_repetitions)]

pub mod common;
pub mod model;
pub mod parse;

#[cfg(feature = "nom")]
pub mod nom;

use pest_derive::Parser;

#[derive(Parser)]
#[grammar = "lp_file_format.pest"]
pub struct LParser;
