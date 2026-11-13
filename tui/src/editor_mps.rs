//! A line-oriented index of an MPS file's row and column names, for the
//! editor's go-to-definition, references, hover and rename.
//!
//! The LP grammar does not cover MPS, so this reads the file the way MPS is
//! laid out: section headers start in column one, data lines start with a
//! blank, and a line's fields are whitespace separated. It indexes names in
//! ROWS, COLUMNS, RHS, RANGES and BOUNDS; it does not validate the file (the
//! real parser reports errors, see [`parse_error`]).

use std::collections::HashMap;
use std::ops::Range;

use lp_parser_rs::LpProblem;
use lp_parser_rs::error::LpParseError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    /// A constraint (or the objective) row, defined in ROWS.
    Row,
    /// A column (variable), defined by its first COLUMNS line.
    Column,
}

impl Kind {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Row => "row",
            Self::Column => "column",
        }
    }
}

#[derive(Debug, Default)]
pub struct MpsIndex {
    /// Every occurrence of every name, in file order, with the section it is in.
    sites: Vec<Site>,
    by_name: HashMap<(Kind, String), Vec<usize>>,
}

#[derive(Debug, Clone)]
pub struct Site {
    pub range: Range<usize>,
    pub kind: Kind,
    pub name: String,
    pub section: &'static str,
}

/// Fields of `line` with their byte offsets in the whole text.
fn fields(line: &str, line_start: usize) -> Vec<(Range<usize>, &str)> {
    let mut out = Vec::new();
    let mut start = None;
    for (i, ch) in line.char_indices() {
        match (ch.is_whitespace(), start) {
            (false, None) => start = Some(i),
            (true, Some(s)) => {
                out.push((line_start + s..line_start + i, &line[s..i]));
                start = None;
            }
            _ => {}
        }
    }
    if let Some(s) = start {
        out.push((line_start + s..line_start + line.len(), &line[s..]));
    }
    out
}

fn is_number(text: &str) -> bool {
    text.parse::<f64>().is_ok()
}

impl MpsIndex {
    pub fn build(text: &str) -> Self {
        let mut index = Self::default();
        let mut section = "";
        let mut offset = 0;
        for raw in text.split_inclusive('\n') {
            let line_start = offset;
            offset += raw.len();
            let line = raw.trim_end_matches(['\n', '\r']);
            if line.is_empty() || line.starts_with('*') {
                continue;
            }
            if !line.starts_with([' ', '\t']) {
                section = match line.split_whitespace().next().unwrap_or("") {
                    "ROWS" => "ROWS",
                    "COLUMNS" => "COLUMNS",
                    "RHS" => "RHS",
                    "RANGES" => "RANGES",
                    "BOUNDS" => "BOUNDS",
                    _ => "",
                };
                continue;
            }
            let fields = fields(line, line_start);
            match section {
                "ROWS" => {
                    if let [_, (range, name), ..] = fields.as_slice() {
                        index.push(range.clone(), Kind::Row, name, section);
                    }
                }
                "COLUMNS" => {
                    // Integer markers: `MARKER 'MARKER' 'INTORG'`.
                    if fields.iter().any(|(_, f)| f.contains("'MARKER'")) {
                        continue;
                    }
                    if let Some((range, name)) = fields.first() {
                        index.push(range.clone(), Kind::Column, name, section);
                    }
                    for pair in fields.get(1..).unwrap_or_default().chunks(2) {
                        if let [(range, name), (_, value)] = pair
                            && is_number(value)
                        {
                            index.push(range.clone(), Kind::Row, name, section);
                        }
                    }
                }
                "RHS" | "RANGES" => {
                    // `set row value [row value]`; the set name may be omitted.
                    let skip = usize::from(fields.len() % 2 == 1);
                    for pair in fields.get(skip..).unwrap_or_default().chunks(2) {
                        if let [(range, name), (_, value)] = pair
                            && is_number(value)
                        {
                            index.push(range.clone(), Kind::Row, name, section);
                        }
                    }
                }
                "BOUNDS" => {
                    // `type set column [value]`; the set may be omitted for the valueless types.
                    let Some((_, kind)) = fields.first() else { continue };
                    let valueless = matches!(*kind, "FR" | "MI" | "PL" | "BV");
                    let column = match (valueless, fields.len()) {
                        (true, 3) | (false, 4) => 2,
                        (true, 2) | (false, 3) => 1,
                        _ => continue,
                    };
                    if let Some((range, name)) = fields.get(column) {
                        index.push(range.clone(), Kind::Column, name, section);
                    }
                }
                _ => {}
            }
        }
        index
    }

    fn push(&mut self, range: Range<usize>, kind: Kind, name: &str, section: &'static str) {
        self.by_name.entry((kind, name.to_owned())).or_default().push(self.sites.len());
        self.sites.push(Site { range, kind, name: name.to_owned(), section });
    }

    /// The occurrence under `offset`.
    pub fn site_at(&self, offset: usize) -> Option<&Site> {
        self.sites.iter().find(|site| site.range.start <= offset && offset < site.range.end)
    }

    /// Every occurrence of `site`'s name, in file order.
    pub fn occurrences(&self, site: &Site) -> Vec<&Site> {
        self.by_name.get(&(site.kind, site.name.clone())).map(|ids| ids.iter().map(|&i| &self.sites[i]).collect()).unwrap_or_default()
    }

    /// Where `site`'s name is defined: its ROWS line, or its first COLUMNS line.
    pub fn definition(&self, site: &Site) -> Option<&Site> {
        self.occurrences(site).into_iter().find(|s| matches!(s.section, "ROWS" | "COLUMNS"))
    }

    /// Every occurrence of the `kind` named `name`.
    pub fn occurrences_of(&self, kind: Kind, name: &str) -> Vec<&Site> {
        self.by_name.get(&(kind, name.to_owned())).map(|ids| ids.iter().map(|&i| &self.sites[i]).collect()).unwrap_or_default()
    }

    /// Whether `name` is already in use as a `kind`.
    pub fn contains(&self, kind: Kind, name: &str) -> bool {
        self.by_name.contains_key(&(kind, name.to_owned()))
    }
}

/// The real parser's verdict on `text`: where it failed and why, or `None` if it parses.
///
/// Only the parse errors that carry a position are located; any other failure
/// is reported at the start of the file.
pub fn parse_error(text: &str) -> Option<(usize, String)> {
    match LpProblem::parse_mps(text) {
        Ok(_) => None,
        Err(error) => {
            let offset = match &error {
                LpParseError::ParseError { position, .. } | LpParseError::InvalidNumber { position, .. } => (*position).min(text.len()),
                _ => 0,
            };
            Some((offset, error.to_string().lines().next().unwrap_or_default().to_owned()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MPS: &str = "NAME          DEMO\nROWS\n N  COST\n L  LIM1\n G  LIM2\nCOLUMNS\n    X         COST         1.0   LIM1         1.0\n    X         LIM2         1.0\n    MARKER                 'MARKER'                 'INTORG'\n    Y         COST         2.0   LIM1         1.0\nRHS\n    RHS       LIM1         4.0   LIM2         1.0\nBOUNDS\n UP BND       X            4.0\n FR BND       Y\nENDATA\n";

    fn site_of(index: &MpsIndex, text: &str, needle: &str, nth: usize) -> Site {
        let at = text.match_indices(needle).nth(nth).expect("needle present").0;
        index.site_at(at).expect("an indexed name").clone()
    }

    #[test]
    fn rows_and_columns_are_indexed_across_sections() {
        let index = MpsIndex::build(MPS);
        let lim1 = site_of(&index, MPS, "LIM1", 0);
        assert_eq!((lim1.kind, lim1.section), (Kind::Row, "ROWS"));
        let uses = index.occurrences(&lim1);
        let sections: Vec<&str> = uses.iter().map(|s| s.section).collect();
        assert_eq!(sections, ["ROWS", "COLUMNS", "COLUMNS", "RHS"], "defined once, used in COLUMNS (twice) and RHS");
        let x = site_of(&index, MPS, "X ", 0);
        assert_eq!(x.kind, Kind::Column);
        let sections: Vec<&str> = index.occurrences(&x).iter().map(|s| s.section).collect();
        assert_eq!(sections, ["COLUMNS", "COLUMNS", "BOUNDS"], "a column's first COLUMNS line defines it");
    }

    #[test]
    fn markers_set_names_and_valueless_bounds_are_handled() {
        let index = MpsIndex::build(MPS);
        assert!(!index.contains(Kind::Column, "MARKER") && !index.contains(Kind::Column, "RHS"));
        assert!(index.contains(Kind::Column, "Y"));
        let y_bound = site_of(&index, MPS, "Y\n", 0);
        assert_eq!(y_bound.section, "BOUNDS", "`FR BND Y` has no value but still names Y");
    }

    #[test]
    fn definition_jumps_to_rows_and_first_column_lines() {
        let index = MpsIndex::build(MPS);
        let rhs_use = site_of(&index, MPS, "LIM2", 2);
        assert_eq!(rhs_use.section, "RHS");
        let definition = index.definition(&rhs_use).expect("defined");
        assert_eq!(definition.section, "ROWS");
        assert!(MPS[..definition.range.start].ends_with(" G  "));
    }

    #[test]
    fn the_real_parser_locates_a_bad_number() {
        assert!(parse_error(MPS).is_none(), "the fixture parses");
        let broken = MPS.replace("4.0   LIM2", "4.x   LIM2");
        let (offset, message) = parse_error(&broken).expect("a bad number is an error");
        assert_ne!(message, "");
        assert!(offset <= broken.len());
    }
}
