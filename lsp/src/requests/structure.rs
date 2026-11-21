//! Row-by-column structure of a model in compressed sparse row form, shared by
//! `lp/graph` and `lp/matrix`.

use lp_parser_rs::model::{Coefficient, Constraint, QuadraticTerm};
use lp_parser_rs::{LpProblem, NameId};

use super::{Cancel, RequestError};

/// Which parts of the model become entries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// Linear coefficients only (the matrix): objectives and the linear parts
    /// of linear, indicator and quadratic constraints. Zero and non-finite
    /// coefficients are dropped.
    Linear,
    /// Every reference (the graph): also quadratic variables, indicator
    /// variables, SOS members and general-constraint arguments, so a variable
    /// is connected to every row that mentions it.
    All,
}

/// An objective or constraint row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowKind {
    /// Position `index` in `LpProblem::objectives`.
    Objective,
    /// Position `index` in `LpProblem::constraints`.
    Constraint,
}

/// One row of the structure.
#[derive(Debug, Clone, Copy)]
pub struct Row {
    /// Objective or constraint.
    pub kind: RowKind,
    /// Position in the owning map.
    pub index: usize,
    /// Model name.
    pub name: NameId,
}

/// The transpose of a [`Structure`]: rows per column.
#[derive(Debug)]
pub struct Columns {
    ptr: Vec<usize>,
    rows: Vec<usize>,
}

impl Columns {
    /// Rows referencing column `col`, ascending.
    #[must_use]
    pub fn rows(&self, col: usize) -> &[usize] {
        &self.rows[self.ptr[col]..self.ptr[col + 1]]
    }
}

/// Rows with their entries.
#[derive(Debug, Default)]
pub struct Structure {
    /// Objectives (when included) then constraints, in model order.
    pub rows: Vec<Row>,
    row_ptr: Vec<usize>,
    cols: Vec<u32>,
    /// `NAN` marks an entry without a coefficient (a bare reference).
    vals: Vec<f64>,
    /// Number of variables (columns).
    pub n_cols: usize,
}

impl Structure {
    /// Build the structure of `problem`.
    ///
    /// # Errors
    /// [`RequestError::Cancelled`] when `cancel` fires.
    pub fn build(problem: &LpProblem, scope: Scope, include_objectives: bool, cancel: &Cancel) -> Result<Self, RequestError> {
        let n_cols = problem.variables.len();
        let mut builder = Builder::new(problem, scope, n_cols);
        if include_objectives {
            for (index, objective) in problem.objectives.values().enumerate() {
                cancel.tick(builder.structure.rows.len())?;
                builder.begin(Row { kind: RowKind::Objective, index, name: objective.name });
                builder.linear(&objective.coefficients);
                if scope == Scope::All {
                    builder.quadratic(&objective.quadratic);
                }
                builder.end();
            }
        }
        for (index, constraint) in problem.constraints.values().enumerate() {
            cancel.tick(builder.structure.rows.len())?;
            let row = Row { kind: RowKind::Constraint, index, name: constraint.name() };
            match constraint {
                Constraint::Standard { coefficients, .. } => {
                    builder.begin(row);
                    builder.linear(coefficients);
                }
                Constraint::Indicator { variable, coefficients, .. } => {
                    builder.begin(row);
                    if scope == Scope::All {
                        builder.reference(*variable);
                    }
                    builder.linear(coefficients);
                }
                Constraint::Quadratic { coefficients, quadratic, .. } => {
                    builder.begin(row);
                    builder.linear(coefficients);
                    if scope == Scope::All {
                        builder.quadratic(quadratic);
                    }
                }
                Constraint::SOS { weights, .. } if scope == Scope::All => {
                    builder.begin(row);
                    builder.linear(weights);
                }
                Constraint::General { resultant, function, .. } if scope == Scope::All => {
                    builder.begin(row);
                    builder.reference(*resultant);
                    for variable in function.variables() {
                        builder.reference(*variable);
                    }
                }
                Constraint::SOS { .. } | Constraint::General { .. } => continue,
            }
            builder.end();
        }
        let structure = builder.finish();
        debug_assert_eq!(structure.row_ptr.len(), structure.rows.len() + 1);
        debug_assert_eq!(structure.cols.len(), structure.vals.len());
        Ok(structure)
    }

    /// Number of entries.
    #[must_use]
    pub const fn nnz(&self) -> usize {
        self.cols.len()
    }

    /// Entries of row `row`: `(column, coefficient)`, the coefficient `NAN`
    /// for a bare reference.
    pub fn entries(&self, row: usize) -> impl ExactSizeIterator<Item = (usize, f64)> + '_ {
        let range = self.row_ptr[row]..self.row_ptr[row + 1];
        self.cols[range.clone()].iter().zip(&self.vals[range]).map(|(&col, &value)| (col as usize, value))
    }

    /// Number of entries in row `row`.
    #[must_use]
    pub fn row_len(&self, row: usize) -> usize {
        self.row_ptr[row + 1] - self.row_ptr[row]
    }

    /// For each column, the rows that reference it, in row order.
    ///
    /// # Errors
    /// [`RequestError::Cancelled`] when `cancel` fires.
    pub fn columns(&self, cancel: &Cancel) -> Result<Columns, RequestError> {
        let degrees = self.column_degrees();
        let mut ptr = vec![0usize; self.n_cols + 1];
        for (col, &degree) in degrees.iter().enumerate() {
            ptr[col + 1] = ptr[col] + degree;
        }
        let mut fill = ptr.clone();
        let mut rows = vec![0usize; self.nnz()];
        for row in 0..self.rows.len() {
            cancel.tick(row)?;
            for (col, _) in self.entries(row) {
                rows[fill[col]] = row;
                fill[col] += 1;
            }
        }
        Ok(Columns { ptr, rows })
    }

    /// Entries per column (the number of rows referencing each variable).
    #[must_use]
    pub fn column_degrees(&self) -> Vec<usize> {
        let mut degrees = vec![0; self.n_cols];
        for &col in &self.cols {
            degrees[col as usize] += 1;
        }
        degrees
    }
}

struct Builder<'p> {
    problem: &'p LpProblem,
    scope: Scope,
    structure: Structure,
    /// Per column: `row stamp` of the last row that touched it, and the entry's
    /// position, to merge repeated variables within a row.
    stamp: Vec<usize>,
    slot: Vec<usize>,
    start: usize,
}

impl<'p> Builder<'p> {
    fn new(problem: &'p LpProblem, scope: Scope, n_cols: usize) -> Self {
        let structure = Structure { rows: Vec::new(), row_ptr: vec![0], cols: Vec::new(), vals: Vec::new(), n_cols };
        Self { problem, scope, structure, stamp: vec![usize::MAX; n_cols], slot: vec![0; n_cols], start: 0 }
    }

    fn begin(&mut self, row: Row) {
        self.start = self.structure.cols.len();
        self.structure.rows.push(row);
    }

    fn push(&mut self, name: NameId, value: f64) {
        let Some(col) = self.problem.variables.get_index_of(&name) else {
            debug_assert!(false, "every referenced variable is declared in the model");
            return;
        };
        let row = self.structure.rows.len() - 1;
        if self.stamp[col] == row {
            let at = self.slot[col];
            let existing = self.structure.vals[at];
            self.structure.vals[at] = if existing.is_nan() {
                value
            } else if value.is_nan() {
                existing
            } else {
                existing + value
            };
        } else {
            self.stamp[col] = row;
            self.slot[col] = self.structure.cols.len();
            self.structure.cols.push(u32::try_from(col).expect("fewer than 2^32 variables"));
            self.structure.vals.push(value);
        }
    }

    fn linear(&mut self, coefficients: &[Coefficient]) {
        for c in coefficients {
            // A non-finite coefficient has no number to show.
            self.push(c.name, if c.value.is_finite() { c.value } else { f64::NAN });
        }
    }

    fn reference(&mut self, name: NameId) {
        self.push(name, f64::NAN);
    }

    fn quadratic(&mut self, terms: &[QuadraticTerm]) {
        for term in terms {
            self.reference(term.var1);
            self.reference(term.var2);
        }
    }

    /// Close the row. In the linear scope drop entries that are zero or have
    /// no finite value (after merging repeats).
    fn end(&mut self) {
        if self.scope == Scope::Linear {
            let s = &mut self.structure;
            let mut keep = self.start;
            for i in self.start..s.cols.len() {
                if s.vals[i].is_finite() && s.vals[i] != 0.0 {
                    s.cols[keep] = s.cols[i];
                    s.vals[keep] = s.vals[i];
                    keep += 1;
                }
            }
            s.cols.truncate(keep);
            s.vals.truncate(keep);
        }
        let end = self.structure.cols.len();
        self.structure.row_ptr.push(end);
    }

    fn finish(self) -> Structure {
        self.structure
    }
}

#[cfg(test)]
mod tests {
    use lp_parser_rs::LpProblem;

    use super::*;

    const TEXT: &str = "Minimize\n obj: x + 2 y + [ y ^ 2 ] / 2\nSubject To\n c1: x + y + x >= 1\n c2: b = 1 -> x - y <= 0\n c3: 0 x + y <= 3\nBounds\n x <= 5\nBinaries\n b\nSOS\n s: S1:: x:1 y:2\nEnd\n";

    fn build(scope: Scope, objectives: bool) -> (LpProblem, Structure) {
        let problem = LpProblem::parse(TEXT).expect("parses");
        let structure = Structure::build(&problem, scope, objectives, &Cancel::new()).expect("builds");
        (problem, structure)
    }

    fn row(problem: &LpProblem, structure: &Structure, name: &str) -> Vec<(String, f64)> {
        let r = structure.rows.iter().position(|r| problem.resolve(r.name) == name).expect("row exists");
        structure.entries(r).map(|(col, v)| (problem.resolve(*problem.variables.get_index(col).expect("column").0).to_owned(), v)).collect()
    }

    #[test]
    fn repeated_variables_are_summed_and_zeros_dropped_in_the_linear_scope() {
        let (problem, s) = build(Scope::Linear, true);
        assert_eq!(row(&problem, &s, "c1"), [("x".to_owned(), 2.0), ("y".to_owned(), 1.0)]);
        assert_eq!(row(&problem, &s, "c3"), [("y".to_owned(), 1.0)], "0 x is not a matrix entry");
        assert!(s.rows.iter().all(|r| problem.resolve(r.name) != "s"), "SOS sets are not matrix rows");
        assert_eq!(row(&problem, &s, "obj"), [("x".to_owned(), 1.0), ("y".to_owned(), 2.0)], "quadratic parts are not linear entries");
    }

    #[test]
    fn the_all_scope_keeps_every_reference() {
        let (problem, s) = build(Scope::All, true);
        let c2 = row(&problem, &s, "c2");
        assert_eq!(c2.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>(), ["b", "x", "y"], "indicator variable first");
        assert!(c2[0].1.is_nan(), "the indicator has no coefficient");
        assert_eq!(row(&problem, &s, "s"), [("x".to_owned(), 1.0), ("y".to_owned(), 2.0)], "SOS weights");
        assert_eq!(row(&problem, &s, "c3").len(), 2, "explicit zeros stay: the reference exists");
    }

    #[test]
    fn objectives_can_be_left_out() {
        let (problem, s) = build(Scope::Linear, false);
        assert!(s.rows.iter().all(|r| r.kind == RowKind::Constraint));
        assert!(s.rows.iter().all(|r| problem.resolve(r.name) != "obj"));
    }

    #[test]
    fn a_cancelled_build_stops() {
        let problem = LpProblem::parse(TEXT).expect("parses");
        let cancel = Cancel::new();
        cancel.cancel();
        assert_eq!(Structure::build(&problem, Scope::All, true, &cancel).err(), Some(RequestError::Cancelled));
    }

    #[test]
    fn column_degrees_count_rows() {
        let (problem, s) = build(Scope::Linear, true);
        let x = problem.variables.get_index_of(&problem.name_id("x").expect("x")).expect("column");
        assert_eq!(s.column_degrees()[x], 3, "obj, c1, c2");
    }
}
