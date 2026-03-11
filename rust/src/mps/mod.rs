//! MPS file format parser.
//!
//! Parses MPS (Mathematical Programming System) files into the same
//! [`ParseResult`] used by the LP grammar, enabling seamless integration
//! with `LpProblem::from_parse_result`.
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

/// Strip `$` inline comments from a field list.
///
/// Per the CPLEX MPS spec, if Field 3 or Field 5 starts with `$`, the
/// remainder of the line is a comment. We check all fields from index 0
/// onward for simplicity -- a `$`-prefixed field truncates everything after.
pub(super) fn strip_dollar_comments<'a>(fields: &[&'a str]) -> Vec<&'a str> {
    debug_assert!(!fields.is_empty(), "strip_dollar_comments called with empty fields");

    let mut result = Vec::with_capacity(fields.len());
    for &field in fields {
        if field.starts_with('$') {
            break;
        }
        result.push(field);
    }

    debug_assert!(result.len() <= fields.len(), "result cannot exceed input length");
    result
}
