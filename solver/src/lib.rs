//! `HiGHS` solver integration for LP and MPS models.
//!
//! * [`protocol`] (always built): the JSON helper protocol spoken by the
//!   `lp-highs` binary.
//! * [`solve`] (feature `highs`): build a model from an `LpProblem`, solve it
//!   with optional cancellation and `highs.opt` options, and diagnose
//!   infeasibility by elastic relaxation.
//! * [`query`] (feature `highs`): unbounded rays, irreducible infeasible
//!   subsystems and ranging.
//! * [`run_request`] (feature `highs`): one protocol request, end to end.
//!
//! Without the `highs` feature nothing here needs a C++ toolchain.

pub mod protocol;

#[cfg(feature = "highs")]
pub mod query;
#[cfg(feature = "highs")]
mod run;
#[cfg(feature = "highs")]
pub mod solve;

#[cfg(feature = "highs")]
pub use run::{highs_version, run_request};
