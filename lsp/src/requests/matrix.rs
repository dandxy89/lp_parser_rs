//! `lp/matrix`: the sparse coefficient matrix, as exact entries or a density
//! raster.

use lp_parser_rs::LpProblem;
use lp_parser_rs::model::Constraint;
use serde::{Deserialize, Serialize};

use super::model::{constraint_kind, relation};
use super::structure::{Row, RowKind, Scope, Structure};
use super::{Cancel, DocumentRef, RequestError, Source, Target, check_limit, constraint_class, finite, variable_kind};
use crate::document::Document;

/// Default rows or columns per entries window.
pub const DEFAULT_WINDOW: usize = 200;
/// Largest rows or columns per entries window.
pub const MAX_WINDOW: usize = 5000;
/// Entries one response carries.
pub const MAX_ENTRIES: usize = 250_000;
/// Default raster size.
pub const DEFAULT_BINS: usize = 128;
/// Largest raster size per axis.
pub const MAX_BINS: usize = 512;

/// What to return.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Mode {
    /// Exact entries of a window.
    Entries,
    /// A raster of counts and magnitudes.
    Density,
}

/// Row and column ordering.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Order {
    /// As written.
    #[default]
    Model,
    /// By name (objectives still come first).
    Name,
}

/// A slice of rows or columns.
#[derive(Debug, Clone, Copy, Deserialize)]
pub struct Window {
    /// First position.
    #[serde(default)]
    pub offset: usize,
    /// How many.
    #[serde(default = "default_window")]
    pub limit: usize,
}

const fn default_window() -> usize {
    DEFAULT_WINDOW
}

impl Default for Window {
    fn default() -> Self {
        Self { offset: 0, limit: DEFAULT_WINDOW }
    }
}

/// Raster size.
#[derive(Debug, Clone, Copy, Deserialize)]
pub struct Bins {
    /// Bins down.
    pub rows: usize,
    /// Bins across.
    pub cols: usize,
}

/// Params of `lp/matrix`.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Params {
    /// Document and progress token.
    #[serde(flatten)]
    pub target: Target,
    /// Entries or density.
    pub mode: Mode,
    /// Row window (entries mode).
    #[serde(default)]
    pub rows: Option<Window>,
    /// Column window (entries mode).
    #[serde(default)]
    pub cols: Option<Window>,
    /// Ordering of rows and columns.
    #[serde(default)]
    pub order: Order,
    /// Objectives are the first rows (default true).
    #[serde(default = "yes")]
    pub include_objectives: bool,
    /// Raster size (density mode).
    #[serde(default)]
    pub bins: Option<Bins>,
}

const fn yes() -> bool {
    true
}

/// Result of `lp/matrix`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(untagged)]
pub enum Result {
    /// Entries mode.
    Entries(Entries),
    /// Density mode.
    Density(Density),
}

/// Size of the whole matrix.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Shape {
    pub rows: usize,
    pub cols: usize,
    pub nonzeros: usize,
}

/// Entries mode result.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Entries {
    #[serde(flatten)]
    pub document: DocumentRef,
    pub mode: &'static str,
    pub shape: Shape,
    pub row_offset: usize,
    pub col_offset: usize,
    pub rows: Vec<RowInfo>,
    pub cols: Vec<ColInfo>,
    pub entries: EntryArrays,
    pub scale: Scale,
    pub truncated: bool,
}

/// A row of the window.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RowInfo {
    pub name: String,
    pub kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub class: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operator: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rhs: Option<Option<f64>>,
}

/// A column of the window.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ColInfo {
    pub name: String,
    pub kind: &'static str,
}

/// Entries as parallel arrays, indices relative to the window.
#[derive(Debug, Clone, PartialEq, Default, Serialize)]
pub struct EntryArrays {
    pub row: Vec<usize>,
    pub col: Vec<usize>,
    pub value: Vec<f64>,
}

/// Magnitudes over the whole matrix, so colours are stable across windows.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Scale {
    pub min_abs: Option<f64>,
    pub max_abs: Option<f64>,
}

/// Density mode result.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Density {
    #[serde(flatten)]
    pub document: DocumentRef,
    pub mode: &'static str,
    pub shape: Shape,
    /// The raster size used (never more bins than rows or columns).
    pub bins: BinsOut,
    /// First row position of each bin.
    pub row_edges: Vec<usize>,
    /// First column position of each bin.
    pub col_edges: Vec<usize>,
    /// Entries per bin, row-major.
    pub count: Vec<u32>,
    /// Largest magnitude per bin.
    pub max_abs: Vec<f64>,
    /// `1` all positive, `-1` all negative, `0` mixed or empty.
    pub sign: Vec<i8>,
}

/// Raster size in a result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct BinsOut {
    pub rows: usize,
    pub cols: usize,
}

/// Handle `lp/matrix`.
///
/// # Errors
/// `parseError`, `-32602` for bad windows or bins, or cancellation.
pub fn handle(doc: &Document, version: Option<i32>, params: &Params, cancel: &Cancel) -> std::result::Result<Result, RequestError> {
    let (rows_window, cols_window) = (params.rows.unwrap_or_default(), params.cols.unwrap_or_default());
    let bins = params.bins.unwrap_or(Bins { rows: DEFAULT_BINS, cols: DEFAULT_BINS });
    match params.mode {
        Mode::Entries => {
            check_limit("rows.limit", rows_window.limit, MAX_WINDOW)?;
            check_limit("cols.limit", cols_window.limit, MAX_WINDOW)?;
        }
        Mode::Density => {
            check_limit("bins.rows", bins.rows, MAX_BINS)?;
            check_limit("bins.cols", bins.cols, MAX_BINS)?;
        }
    }
    let source = Source::acquire(doc)?;
    let problem = source.problem();
    cancel.set_total(problem.objectives.len() + problem.constraints.len());
    let structure = Structure::build(problem, Scope::Linear, params.include_objectives, cancel)?;
    let row_order = ordered_rows(problem, &structure, params.order);
    let col_order = ordered_cols(problem, structure.n_cols, params.order);
    let mut col_pos = vec![0usize; structure.n_cols];
    for (pos, &col) in col_order.iter().enumerate() {
        col_pos[col] = pos;
    }
    let shape = Shape { rows: structure.rows.len(), cols: structure.n_cols, nonzeros: structure.nnz() };
    let document = DocumentRef::new(&params.target.text_document.uri, version);
    match params.mode {
        Mode::Entries => {
            entries(problem, &structure, &row_order, &col_order, &col_pos, shape, document, (rows_window, cols_window), cancel)
                .map(Result::Entries)
        }
        Mode::Density => density(&structure, &row_order, &col_pos, shape, document, (bins.rows, bins.cols), cancel).map(Result::Density),
    }
}

/// Row indices in display order: objectives first, then constraints.
fn ordered_rows(problem: &LpProblem, structure: &Structure, order: Order) -> Vec<usize> {
    let mut rows: Vec<usize> = (0..structure.rows.len()).collect();
    if order == Order::Name {
        // Objectives stay first (they already are); sort each group by name.
        rows.sort_by(|&a, &b| {
            let (ra, rb) = (structure.rows[a], structure.rows[b]);
            let group = |r: Row| r.kind == RowKind::Constraint;
            group(ra).cmp(&group(rb)).then_with(|| problem.resolve(ra.name).cmp(problem.resolve(rb.name)))
        });
    }
    rows
}

/// Column indices in display order.
fn ordered_cols(problem: &LpProblem, n_cols: usize, order: Order) -> Vec<usize> {
    let mut cols: Vec<usize> = (0..n_cols).collect();
    if order == Order::Name {
        let name = |col: usize| problem.variables.get_index(col).map_or("", |(id, _)| problem.resolve(*id));
        cols.sort_by(|&a, &b| name(a).cmp(name(b)));
    }
    cols
}

fn scale(structure: &Structure) -> Scale {
    let mut range: Option<(f64, f64)> = None;
    for row in 0..structure.rows.len() {
        for (_, value) in structure.entries(row) {
            let magnitude = value.abs();
            range = Some(range.map_or((magnitude, magnitude), |(lo, hi)| (lo.min(magnitude), hi.max(magnitude))));
        }
    }
    Scale { min_abs: range.map(|r| r.0).and_then(finite), max_abs: range.map(|r| r.1).and_then(finite) }
}

fn row_info(problem: &LpProblem, row: Row) -> RowInfo {
    let name = problem.resolve(row.name).to_owned();
    match row.kind {
        RowKind::Objective => RowInfo { name, kind: "objective", class: None, operator: None, rhs: None },
        RowKind::Constraint => {
            let constraint: &Constraint = problem.constraints.get_index(row.index).map(|(_, c)| c).expect("rows index the constraint map");
            let relation = relation(constraint);
            RowInfo {
                name,
                kind: constraint_kind(constraint),
                class: Some(constraint_class(problem.constraint_class(row.name))),
                operator: relation.map(|(op, _)| op),
                rhs: relation.map(|(_, rhs)| rhs),
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn entries(
    problem: &LpProblem,
    structure: &Structure,
    row_order: &[usize],
    col_order: &[usize],
    col_pos: &[usize],
    shape: Shape,
    document: DocumentRef,
    (rows_window, cols_window): (Window, Window),
    cancel: &Cancel,
) -> std::result::Result<Entries, RequestError> {
    let window = |w: Window, len: usize| {
        let start = w.offset.min(len);
        start..start.saturating_add(w.limit).min(len)
    };
    let row_range = window(rows_window, row_order.len());
    let col_range = window(cols_window, col_order.len());
    let mut out = EntryArrays::default();
    let mut truncated = false;
    let mut scratch: Vec<(usize, f64)> = Vec::new();
    'rows: for (rel_row, &row) in row_order[row_range.clone()].iter().enumerate() {
        cancel.tick(row_range.start + rel_row)?;
        scratch.clear();
        scratch.extend(structure.entries(row).filter_map(|(col, value)| {
            let pos = col_pos[col];
            col_range.contains(&pos).then(|| (pos - col_range.start, value))
        }));
        scratch.sort_by_key(|&(pos, _)| pos);
        for &(pos, value) in &scratch {
            if out.row.len() >= MAX_ENTRIES {
                truncated = true;
                break 'rows;
            }
            out.row.push(rel_row);
            out.col.push(pos);
            out.value.push(value);
        }
    }
    cancel.check()?;
    Ok(Entries {
        document,
        mode: "entries",
        shape,
        row_offset: row_range.start,
        col_offset: col_range.start,
        rows: row_order[row_range].iter().map(|&r| row_info(problem, structure.rows[r])).collect(),
        cols: col_order[col_range]
            .iter()
            .filter_map(|&c| problem.variables.get_index(c))
            .map(|(id, v)| ColInfo { name: problem.resolve(*id).to_owned(), kind: variable_kind(v.kind) })
            .collect(),
        entries: out,
        scale: scale(structure),
        truncated,
    })
}

/// First position of bin `i` when `n` positions are split into `bins` bins:
/// the smallest `p` with `p * bins / n >= i`.
const fn bin_edge(i: usize, n: usize, bins: usize) -> usize {
    (i * n).div_ceil(bins)
}

fn density(
    structure: &Structure,
    row_order: &[usize],
    col_pos: &[usize],
    shape: Shape,
    document: DocumentRef,
    (bins_rows, bins_cols): (usize, usize),
    cancel: &Cancel,
) -> std::result::Result<Density, RequestError> {
    let (n_rows, n_cols) = (shape.rows, shape.cols);
    // Never more bins than positions: empty bins would only waste the payload.
    let (br, bc) = (bins_rows.min(n_rows.max(1)), bins_cols.min(n_cols.max(1)));
    let cells = br * bc;
    let mut count = vec![0u32; cells];
    let mut max_abs = vec![0.0f64; cells];
    // Bit 0: a positive entry, bit 1: a negative one.
    let mut signs = vec![0u8; cells];
    for (pos, &row) in row_order.iter().enumerate() {
        cancel.tick(pos)?;
        let bin_row = pos * br / n_rows;
        for (col, value) in structure.entries(row) {
            let cell = bin_row * bc + col_pos[col] * bc / n_cols;
            count[cell] += 1;
            max_abs[cell] = max_abs[cell].max(value.abs());
            signs[cell] |= if value > 0.0 { 1 } else { 2 };
        }
    }
    cancel.check()?;
    let sign = signs
        .iter()
        .map(|&bits| match bits {
            1 => 1,
            2 => -1,
            _ => 0,
        })
        .collect();
    Ok(Density {
        document,
        mode: "density",
        shape,
        bins: BinsOut { rows: br, cols: bc },
        row_edges: (0..br).map(|i| bin_edge(i, n_rows, br)).collect(),
        col_edges: (0..bc).map(|i| bin_edge(i, n_cols, bc)).collect(),
        count,
        max_abs,
        sign,
    })
}

#[cfg(test)]
mod tests {
    use std::fmt::Write as _;

    use serde_json::{Value, json};

    use super::super::testing::{MODEL, doc, json, target};
    use super::*;

    fn params(mode: Mode) -> Params {
        Params { target: target(), mode, rows: None, cols: None, order: Order::Model, include_objectives: true, bins: None }
    }

    fn run(text: &str, params: &Params) -> Value {
        json(&handle(&doc(text), Some(1), params, &Cancel::new()).expect("matrix"))
    }

    fn triples(result: &Value) -> Vec<(u64, u64, f64)> {
        let e = &result["entries"];
        (0..e["row"].as_array().expect("entries").len())
            .map(|i| (e["row"][i].as_u64().unwrap_or(99), e["col"][i].as_u64().unwrap_or(99), e["value"][i].as_f64().unwrap_or(f64::NAN)))
            .collect()
    }

    #[test]
    fn entries_cover_objectives_and_linear_rows() {
        let result = run(MODEL, &params(Mode::Entries));
        assert_eq!(result["mode"], "entries");
        assert_eq!(result["shape"], json!({ "rows": 4, "cols": 3, "nonzeros": 10 }));
        assert_eq!(result["scale"], json!({ "minAbs": 1.0, "maxAbs": 4.0 }));
        assert_eq!(result["truncated"], false);
        let rows = result["rows"].as_array().expect("rows");
        assert_eq!(rows[0], json!({ "name": "obj", "kind": "objective" }));
        assert_eq!(rows[2], json!({ "name": "c2", "kind": "linear", "class": "normal", "operator": "<=", "rhs": 100.0 }));
        assert_eq!(
            result["cols"],
            json!([{ "name": "x", "kind": "continuous" }, { "name": "y", "kind": "continuous" }, { "name": "z", "kind": "general" }])
        );
        let entries = triples(&result);
        assert_eq!(entries.len(), 10);
        assert_eq!(entries[0], (0, 0, 3.0));
        assert!(entries.contains(&(2, 1, 4.0)), "c2 has 4 y: {entries:?}");
        assert!(entries.windows(2).all(|w| (w[0].0, w[0].1) < (w[1].0, w[1].1)), "sorted by row then column");
    }

    #[test]
    fn windows_select_rows_and_columns_with_relative_indices() {
        let mut p = params(Mode::Entries);
        p.rows = Some(Window { offset: 2, limit: 2 });
        p.cols = Some(Window { offset: 1, limit: 5 });
        let result = run(MODEL, &p);
        assert_eq!((result["rowOffset"].clone(), result["colOffset"].clone()), (json!(2), json!(1)));
        assert_eq!(result["rows"].as_array().map(Vec::len), Some(2));
        assert_eq!(result["cols"].as_array().map(Vec::len), Some(2));
        assert_eq!(triples(&result), [(0, 0, 4.0), (1, 0, 1.0), (1, 1, 1.0)]);
        assert_eq!(result["shape"]["rows"], 4, "shape is the whole matrix");
        assert_eq!(result["scale"]["maxAbs"], 4.0, "scale is the whole matrix");
    }

    #[test]
    fn name_order_sorts_rows_after_objectives_and_columns() {
        let text = "Minimize\n obj: b + a\nSubject To\n z: a + b >= 1\n m: a - b <= 2\nEnd\n";
        let mut p = params(Mode::Entries);
        p.order = Order::Name;
        let result = run(text, &p);
        let names: Vec<_> = result["rows"].as_array().expect("rows").iter().map(|r| r["name"].as_str().unwrap_or("").to_owned()).collect();
        assert_eq!(names, ["obj", "m", "z"]);
        let cols: Vec<_> = result["cols"].as_array().expect("cols").iter().map(|c| c["name"].as_str().unwrap_or("").to_owned()).collect();
        assert_eq!(cols, ["a", "b"]);
        assert_eq!(triples(&result)[0], (0, 0, 1.0), "obj's a coefficient comes first once columns are sorted");
    }

    #[test]
    fn objectives_can_be_left_out_and_special_rows_are_not_rows() {
        let text = "Minimize\n obj: x + y\nSubject To\n c1: x + y >= 1\nSOS\n s: S1:: x:1 y:2\nEnd\n";
        let mut p = params(Mode::Entries);
        p.include_objectives = false;
        let result = run(text, &p);
        assert_eq!(result["shape"], json!({ "rows": 1, "cols": 2, "nonzeros": 2 }));
    }

    #[test]
    fn repeated_variables_merge_and_cancelling_pairs_vanish() {
        let result = run("Minimize\n obj: x + y\nSubject To\n c1: x + y + x >= 1\n c2: x - x + y >= 1\nEnd\n", &params(Mode::Entries));
        assert_eq!(triples(&result), [(0, 0, 1.0), (0, 1, 1.0), (1, 0, 2.0), (1, 1, 1.0), (2, 1, 1.0)]);
    }

    #[test]
    fn density_bins_counts_magnitudes_and_signs() {
        let text = "Minimize\n obj: x - y\nSubject To\n c1: 5 x >= 1\n c2: y - 2 x >= 0\n c3: -4 y <= 1\nEnd\n";
        let mut p = params(Mode::Density);
        p.bins = Some(Bins { rows: 2, cols: 2 });
        let result = run(text, &p);
        assert_eq!(result["mode"], "density");
        assert_eq!(result["bins"], json!({ "rows": 2, "cols": 2 }));
        assert_eq!(result["rowEdges"], json!([0, 2]));
        assert_eq!(result["colEdges"], json!([0, 1]));
        // Rows 0-1 (obj, c1) and 2-3 (c2, c3); columns x and y.
        assert_eq!(result["count"], json!([2, 1, 1, 2]));
        assert_eq!(result["maxAbs"], json!([5.0, 1.0, 2.0, 4.0]));
        assert_eq!(result["sign"], json!([1, -1, -1, 0]), "bin (1,1) holds c2's +1 y and c3's -4 y");
        let total: u64 = result["count"].as_array().expect("count").iter().filter_map(Value::as_u64).sum();
        assert_eq!(total, result["shape"]["nonzeros"].as_u64().expect("nonzeros"));
    }

    #[test]
    fn density_never_uses_more_bins_than_positions() {
        let result = run(MODEL, &params(Mode::Density));
        assert_eq!(result["bins"], json!({ "rows": 4, "cols": 3 }));
        assert_eq!(result["rowEdges"], json!([0, 1, 2, 3]));
        let empty = run("Minimize\n obj: 0 x\nSubject To\n c1: 0 x >= 0\nEnd\n", &params(Mode::Density));
        assert_eq!(empty["shape"]["nonzeros"], 0);
        assert!(empty["sign"].as_array().expect("sign").iter().all(|s| *s == 0));
    }

    #[test]
    fn bin_edges_partition_positions() {
        for n in 1..40 {
            for bins in 1..=n {
                let edges: Vec<usize> = (0..bins).map(|i| bin_edge(i, n, bins)).collect();
                assert_eq!(edges[0], 0);
                for pos in 0..n {
                    let bin = pos * bins / n;
                    assert!(edges[bin] <= pos && (bin + 1 == bins || pos < edges[bin + 1]), "n={n} bins={bins} pos={pos} edges={edges:?}");
                }
            }
        }
    }

    #[test]
    fn rejects_bad_windows_and_bins() {
        let mut p = params(Mode::Entries);
        p.rows = Some(Window { offset: 0, limit: MAX_WINDOW + 1 });
        assert!(matches!(handle(&doc(MODEL), None, &p, &Cancel::new()), Err(RequestError::InvalidParams(_))));
        p.rows = Some(Window { offset: 0, limit: 0 });
        assert!(matches!(handle(&doc(MODEL), None, &p, &Cancel::new()), Err(RequestError::InvalidParams(_))));
        let mut p = params(Mode::Density);
        p.bins = Some(Bins { rows: MAX_BINS + 1, cols: 4 });
        assert!(matches!(handle(&doc(MODEL), None, &p, &Cancel::new()), Err(RequestError::InvalidParams(_))));
        p.bins = Some(Bins { rows: 4, cols: 0 });
        assert!(matches!(handle(&doc(MODEL), None, &p, &Cancel::new()), Err(RequestError::InvalidParams(_))));
        let mut p = params(Mode::Density);
        p.rows = Some(Window { offset: 0, limit: 0 });
        assert!(handle(&doc(MODEL), None, &p, &Cancel::new()).is_ok(), "windows do not apply to the raster");
    }

    #[test]
    fn parse_errors_and_cancellation() {
        assert!(matches!(
            handle(&doc("min\n obj: x +\nst\n c: >= \nend\n"), None, &params(Mode::Entries), &Cancel::new()),
            Err(RequestError::Failed(_))
        ));
        let cancel = Cancel::new();
        cancel.cancel();
        assert_eq!(handle(&doc(MODEL), None, &params(Mode::Density), &cancel).err(), Some(RequestError::Cancelled));
    }

    #[test]
    fn large_models_are_capped_at_the_entry_limit() {
        // 600 rows x 500 columns of dense coefficients is 300 000 entries.
        let mut text = String::from("Minimize\n obj: x0\nSubject To\n");
        for r in 0..600 {
            write!(text, " r{r}: ").unwrap();
            text.push_str(&(0..500).map(|c| format!("x{c}")).collect::<Vec<_>>().join(" + "));
            text.push_str(" >= 1\n");
        }
        text.push_str("End\n");
        let mut p = params(Mode::Entries);
        p.rows = Some(Window { offset: 0, limit: 600 });
        p.cols = Some(Window { offset: 0, limit: 500 });
        let result = run(&text, &p);
        assert_eq!(result["truncated"], true);
        assert_eq!(result["entries"]["row"].as_array().map(Vec::len), Some(MAX_ENTRIES));
        assert_eq!(result["shape"]["nonzeros"], 300_001);
    }
}
