//! MPS (Mathematical Programming System) reading and writing.
//!
//! [`parse_mps`] produces the same [`ParseResult`](crate::lexer::ParseResult)
//! as the LP grammar, so [`LpProblem::parse_mps`](crate::LpProblem::parse_mps)
//! builds a problem through the same code path as LP input, and an MPS file
//! can be diffed against, or converted to, an LP file.
//!
//! Lines are split on whitespace (free format), so names may not contain
//! spaces; fixed-format files whose names have no spaces read the same way.
//! Supported sections are `NAME`, `OBJSENSE`, `ROWS`, `COLUMNS` (with
//! `INTORG`/`INTEND` markers), `RHS`, `RANGES`, `BOUNDS`, `SOS` and `ENDATA`,
//! plus the CPLEX/Gurobi extensions `LAZYCONS`, `USERCUTS`, `INDICATORS`,
//! `QUADOBJ`, `QMATRIX` and `QCMATRIX`. `PWLOBJ`, `GENCONS` and `SCENARIOS`
//! are skipped with a warning on stderr; any other section is an error.
//!

mod builders;
mod sections;
mod state;
#[cfg(test)]
mod tests;
pub mod writer;

pub use state::{extract_mps_name, parse_mps};

use crate::lexer::RawCoefficient;
use crate::model::SOSType;

/// MPS section currently being parsed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum MpsSection {
    Name,
    ObjSense,
    Rows,
    /// CPLEX `LAZYCONS`: rows (in `ROWS` format) that are lazy constraints.
    LazyCons,
    /// CPLEX `USERCUTS`: rows (in `ROWS` format) that are user cuts.
    UserCuts,
    Columns,
    Rhs,
    Ranges,
    Bounds,
    Sos,
    /// CPLEX `INDICATORS`: `IF row column value` lines.
    Indicators,
    /// `QUADOBJ`: upper triangle of the objective's `Q` (`c'x + 1/2 x'Qx`).
    QuadObj,
    /// `QMATRIX`: the full objective `Q` (`c'x + 1/2 x'Qx`).
    QMatrix,
    /// `QCMATRIX row`: the full `Q` of a quadratic constraint (`a'x + x'Qx`).
    QcMatrix,
    Unsupported,
}

/// Row type from the ROWS section.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RowType {
    /// Free row (objective function).
    N,
    /// Less-than-or-equal constraint.
    L,
    /// Greater-than-or-equal constraint.
    G,
    /// Equality constraint.
    E,
}

/// Accumulated bound state for a single variable.
#[derive(Debug, Default)]
pub(super) struct BoundAccumulator {
    pub(super) lower: Option<f64>,
    pub(super) upper: Option<f64>,
    pub(super) fixed: Option<f64>,
    /// `lower` was set by an `FR` record, so a later bound record on that side
    /// overrides it instead of counting as a duplicate.
    pub(super) lower_from_free: bool,
    /// `upper` was set by an `FR` record (see `lower_from_free`).
    pub(super) upper_from_free: bool,
    pub(super) binary: bool,
}

impl BoundAccumulator {
    /// Whether a lower bound has been declared by a record other than `FR`.
    pub(super) const fn has_explicit_lower(&self) -> bool {
        self.lower.is_some() && !self.lower_from_free
    }

    /// Whether an upper bound has been declared by a record other than `FR`.
    pub(super) const fn has_explicit_upper(&self) -> bool {
        self.upper.is_some() && !self.upper_from_free
    }

    /// Record an explicit lower bound, replacing any `FR`-derived one.
    pub(super) const fn set_lower(&mut self, value: f64) {
        self.lower = Some(value);
        self.lower_from_free = false;
    }

    /// Record an explicit upper bound, replacing any `FR`-derived one.
    pub(super) const fn set_upper(&mut self, value: f64) {
        self.upper = Some(value);
        self.upper_from_free = false;
    }
}

/// Maximum number of whitespace-separated fields on an MPS data line.
/// The MPS format defines at most six fields per line.
pub(super) const MAX_FIELDS: usize = 6;

/// Split an MPS data line into whitespace-separated fields without heap
/// allocation, honouring `$` inline comments.
///
/// Per the CPLEX MPS spec, if Field 3 or Field 5 starts with `$`, the
/// remainder of the line is a comment. For simplicity every field is checked:
/// a `$`-prefixed field truncates everything after it.
///
/// Returns the field buffer and the number of fields written. Fields beyond
/// [`MAX_FIELDS`] are ignored, as they exceed what the MPS format defines.
pub(super) fn split_fields(line: &str) -> ([&str; MAX_FIELDS], usize) {
    debug_assert!(!line.is_empty(), "split_fields called with empty line");

    let mut buf = [""; MAX_FIELDS];
    let mut len = 0;
    for field in line.split_whitespace() {
        if field.starts_with('$') || len == MAX_FIELDS {
            break;
        }
        buf[len] = field;
        len += 1;
    }

    debug_assert!(len <= MAX_FIELDS, "field count cannot exceed buffer length");
    (buf, len)
}
