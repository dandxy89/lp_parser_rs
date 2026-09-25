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
/// remainder of the line is a comment. We check all fields from index 0
/// onward for simplicity -- a `$`-prefixed field truncates everything after.
///
/// Returns the field buffer and the number of fields written. Fields beyond
/// [`MAX_FIELDS`] are ignored -- they exceed what the MPS format defines.
pub(super) fn split_fields(line: &str) -> ([&str; MAX_FIELDS], usize) {
    debug_assert!(!line.is_empty(), "split_fields called with empty line");

    let mut buf = [""; MAX_FIELDS];
    let mut len = 0;
    for field in whitespace_fields(line) {
        if field.starts_with('$') || len == MAX_FIELDS {
            break;
        }
        buf[len] = field;
        len += 1;
    }

    debug_assert!(len <= MAX_FIELDS, "field count cannot exceed buffer length");
    (buf, len)
}

/// Split `line` on whitespace exactly as [`str::split_whitespace`] does, with
/// a byte-level fast path for ASCII text.
///
/// MPS data is almost always ASCII, where decoding `char`s and running the
/// generic pattern searcher dominates parse time. The fast path splits on the
/// ASCII bytes that [`char::is_whitespace`] accepts (which, unlike
/// [`u8::is_ascii_whitespace`], include the vertical tab). On meeting a
/// non-ASCII byte it hands the rest of the line, from the start of the
/// current field, to [`str::split_whitespace`]: splitting is local, so the
/// fields are identical to splitting the whole line that way.
pub(super) const fn whitespace_fields(line: &str) -> WhitespaceFields<'_> {
    WhitespaceFields::Ascii(line)
}

/// Iterator returned by [`whitespace_fields`].
pub(super) enum WhitespaceFields<'a> {
    /// The unsplit remainder, so far all ASCII.
    Ascii(&'a str),
    /// Fallback once a non-ASCII byte has been seen.
    Unicode(std::str::SplitWhitespace<'a>),
}

/// Whether an ASCII byte is whitespace according to [`char::is_whitespace`].
const fn is_ascii_char_whitespace(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\n' | 0x0B | 0x0C | b'\r')
}

impl<'a> Iterator for WhitespaceFields<'a> {
    type Item = &'a str;

    fn next(&mut self) -> Option<&'a str> {
        let rest = match self {
            Self::Ascii(rest) => *rest,
            Self::Unicode(fields) => return fields.next(),
        };
        let bytes = rest.as_bytes();
        let mut start = 0;
        while start < bytes.len() && is_ascii_char_whitespace(bytes[start]) {
            start += 1;
        }
        let mut end = start;
        while end < bytes.len() && !is_ascii_char_whitespace(bytes[end]) {
            if !bytes[end].is_ascii() {
                // Every byte before `end` is ASCII, so `start` is a char boundary.
                *self = Self::Unicode(rest[start..].split_whitespace());
                return self.next();
            }
            end += 1;
        }
        debug_assert!(rest.is_char_boundary(start) && rest.is_char_boundary(end), "field bounds must be char boundaries");
        *self = Self::Ascii(&rest[end..]);
        (end > start).then(|| &rest[start..end])
    }
}

#[cfg(test)]
mod whitespace_fields_tests {
    use super::whitespace_fields;

    #[test]
    fn matches_split_whitespace() {
        let cases = [
            "",
            "   ",
            "a",
            "  a  b\tc\r\n",
            "a\x0Bb\x0Cc",
            "x\u{a0}y z",
            "  \u{3000}lead  trail\u{2003}",
            "é b",
            "a é\u{85}b  c",
            "caf\u{e9}  \u{a0} d",
        ];
        for line in cases {
            let fast: Vec<&str> = whitespace_fields(line).collect();
            let reference: Vec<&str> = line.split_whitespace().collect();
            assert_eq!(fast, reference, "{line:?}");
        }
    }
}
