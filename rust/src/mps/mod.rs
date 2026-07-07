//! MPS file format parser.
//!
//! Parses MPS (Mathematical Programming System) files into the same
//! [`ParseResult`](crate::lexer::ParseResult) used by the LP grammar,
//! enabling seamless integration with `LpProblem::parse_mps`.
//!

mod builders;
mod sections;
mod state;
#[cfg(test)]
mod tests;
/// MPS file writing ([`write_mps_string`](writer::write_mps_string) /
/// [`write_mps_string_with_options`](writer::write_mps_string_with_options)),
/// mirroring [`crate::writer`] for the LP format.
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
    Columns,
    Rhs,
    Ranges,
    Bounds,
    Sos,
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
    pub(super) free: bool,
    pub(super) binary: bool,
}

/// Maximum number of whitespace-separated fields on an MPS data line.
/// The MPS format defines at most six fields per line.
pub(super) const MAX_FIELDS: usize = 6;

/// Split an MPS data line into whitespace-separated fields without heap
/// allocation, honouring `$` inline comments.
///
/// Per the CPLEX MPS spec, if Field 3 or Field 5 starts with `$`, the
/// remainder of the line is a comment. We check all fields from index 0
/// onward for simplicity -- a `$`-prefixed field truncates everything after.
///
/// Returns the field buffer and the number of fields written. Fields beyond
/// [`MAX_FIELDS`] are ignored -- they exceed what the MPS format defines.
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
