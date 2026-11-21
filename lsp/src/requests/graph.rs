//! `lp/graph`: the bipartite row/variable dependency graph.

use std::cmp::Reverse;

use lp_parser_rs::LpProblem;
use serde::{Deserialize, Serialize};

use super::structure::{RowKind, Scope, Structure};
use super::{Cancel, DocumentRef, RequestError, Source, Target, check_limit, finite};
use crate::document::Document;

/// Default node cap.
pub const DEFAULT_NODES: usize = 2000;
/// Largest node cap a client may ask for.
pub const MAX_NODES: usize = 20_000;
/// Default edge cap.
pub const DEFAULT_EDGES: usize = 20_000;
/// Largest edge cap a client may ask for.
pub const MAX_EDGES: usize = 200_000;
/// Isolated variables listed.
pub const MAX_ISOLATED: usize = 1000;
/// Hubs listed.
pub const HUBS: usize = 20;
/// Component sizes listed.
pub const COMPONENT_SIZES: usize = 50;

/// Kind of the node a graph is centred on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FocusKind {
    /// A constraint row.
    Constraint,
    /// An objective row.
    Objective,
    /// A variable.
    Variable,
}

/// Centre the graph on one node.
#[derive(Debug, Clone, Deserialize)]
pub struct Focus {
    /// What `name` names.
    pub kind: FocusKind,
    /// The node's name.
    pub name: String,
    /// Hops to follow (1 to 3, default 1).
    #[serde(default)]
    pub depth: Option<usize>,
}

/// Params of `lp/graph`.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Params {
    /// Document and progress token.
    #[serde(flatten)]
    pub target: Target,
    /// Optional centre.
    #[serde(default)]
    pub focus: Option<Focus>,
    /// Include objective rows (default true).
    #[serde(default = "yes")]
    pub include_objectives: bool,
    /// Node cap.
    #[serde(default = "default_nodes")]
    pub max_nodes: usize,
    /// Edge cap.
    #[serde(default = "default_edges")]
    pub max_edges: usize,
}

const fn yes() -> bool {
    true
}
const fn default_nodes() -> usize {
    DEFAULT_NODES
}
const fn default_edges() -> usize {
    DEFAULT_EDGES
}

/// Result of `lp/graph`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Result {
    #[serde(flatten)]
    pub document: DocumentRef,
    pub nodes: Vec<Node>,
    pub edges: Edges,
    pub components: Components,
    /// Variables no row references (capped).
    pub isolated: Vec<String>,
    /// Variables in the most rows.
    pub hubs: Vec<Hub>,
    /// Whether a cap left nodes or edges out.
    pub truncated: bool,
}

/// A row or variable.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Node {
    pub id: usize,
    pub kind: &'static str,
    pub name: String,
    /// Degree in the whole model.
    pub degree: usize,
    /// Rank of the node's component (0 is the largest).
    pub component: usize,
}

/// Edges as parallel arrays, row to variable.
#[derive(Debug, Clone, PartialEq, Default, Serialize)]
pub struct Edges {
    pub source: Vec<usize>,
    pub target: Vec<usize>,
    /// `null` where the row references the variable without a coefficient
    /// (indicator variables, quadratic-only variables, general constraints).
    pub coefficient: Vec<Option<f64>>,
}

/// Connected components of the whole model.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Components {
    pub count: usize,
    /// Sizes in nodes, descending, at most [`COMPONENT_SIZES`].
    pub sizes: Vec<usize>,
}

/// A high-degree variable.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Hub {
    pub name: String,
    pub degree: usize,
}

/// Handle `lp/graph`.
///
/// # Errors
/// `parseError`, `-32602` for bad caps, depth or focus, or cancellation.
pub fn handle(doc: &Document, version: Option<i32>, params: &Params, cancel: &Cancel) -> std::result::Result<Result, RequestError> {
    check_limit("maxNodes", params.max_nodes, MAX_NODES)?;
    check_limit("maxEdges", params.max_edges, MAX_EDGES)?;
    let depth = params.focus.as_ref().map_or(1, |f| f.depth.unwrap_or(1));
    if !(1..=3).contains(&depth) {
        return Err(RequestError::invalid(format!("depth must be 1, 2 or 3, got {depth}")));
    }
    let source = Source::acquire(doc)?;
    let problem = source.problem();
    cancel.set_total(problem.objectives.len() + problem.constraints.len());
    let structure = Structure::build(problem, Scope::All, params.include_objectives, cancel)?;
    let rows = structure.rows.len();
    let cols = structure.n_cols;

    let degrees = structure.column_degrees();
    let components = components(&structure, cancel)?;
    let focus = params.focus.as_ref().map(|f| resolve_focus(problem, &structure, f)).transpose()?;

    let (selected_rows, selected_cols, mut truncated) = match focus {
        Some(node) => neighbourhood(&structure, node, depth, params.max_nodes, cancel)?,
        None => busiest_rows(&structure, params.max_nodes, params.max_edges, cancel)?,
    };

    // Node ids: selected rows in model order, then selected variables.
    let mut row_id = vec![usize::MAX; rows];
    let mut col_id = vec![usize::MAX; cols];
    let mut nodes = Vec::new();
    for (row, _) in selected_rows.iter().enumerate().filter(|&(_, &s)| s) {
        let id = nodes.len();
        row_id[row] = id;
        let r = structure.rows[row];
        nodes.push(Node {
            id,
            kind: if r.kind == RowKind::Objective { "objective" } else { "constraint" },
            name: problem.resolve(r.name).to_owned(),
            degree: structure.row_len(row),
            component: components.rank[row],
        });
    }
    for (col, _) in selected_cols.iter().enumerate().filter(|&(_, &s)| s) {
        let id = nodes.len();
        col_id[col] = id;
        nodes.push(Node {
            id,
            kind: "variable",
            name: variable_name(problem, col),
            degree: degrees[col],
            component: components.rank[rows + col],
        });
    }
    let mut edges = Edges::default();
    'rows: for (row, _) in selected_rows.iter().enumerate().filter(|&(_, &s)| s) {
        cancel.tick(row)?;
        for (col, value) in structure.entries(row) {
            if col_id[col] == usize::MAX {
                continue;
            }
            if edges.source.len() >= params.max_edges {
                truncated = true;
                break 'rows;
            }
            edges.source.push(row_id[row]);
            edges.target.push(col_id[col]);
            edges.coefficient.push(finite(value));
        }
    }

    let isolated = (0..cols).filter(|&c| degrees[c] == 0).take(MAX_ISOLATED).map(|c| variable_name(problem, c)).collect();
    let mut by_degree: Vec<usize> = (0..cols).filter(|&c| degrees[c] > 0).collect();
    by_degree.sort_by_key(|&c| (Reverse(degrees[c]), c));
    let hubs = by_degree.into_iter().take(HUBS).map(|c| Hub { name: variable_name(problem, c), degree: degrees[c] }).collect();

    cancel.check()?;
    Ok(Result {
        document: DocumentRef::new(&params.target.text_document.uri, version),
        nodes,
        edges,
        components: Components { count: components.count, sizes: components.sizes },
        isolated,
        hubs,
        truncated,
    })
}

fn variable_name(problem: &LpProblem, col: usize) -> String {
    problem.variables.get_index(col).map_or_else(String::new, |(id, _)| problem.resolve(*id).to_owned())
}

/// A node as `(is_row, index)`.
type Node2 = (bool, usize);

fn resolve_focus(problem: &LpProblem, structure: &Structure, focus: &Focus) -> std::result::Result<Node2, RequestError> {
    let label = match focus.kind {
        FocusKind::Constraint => "constraint",
        FocusKind::Objective => "objective",
        FocusKind::Variable => "variable",
    };
    let unknown = || RequestError::invalid(format!("unknown {label} '{}'", focus.name));
    let id = problem.name_id(&focus.name).ok_or_else(unknown)?;
    match focus.kind {
        FocusKind::Variable => problem.variables.get_index_of(&id).map(|c| (false, c)).ok_or_else(unknown),
        FocusKind::Constraint | FocusKind::Objective => {
            let kind = if focus.kind == FocusKind::Objective { RowKind::Objective } else { RowKind::Constraint };
            structure.rows.iter().position(|r| r.kind == kind && r.name == id).map(|r| (true, r)).ok_or_else(|| {
                if kind == RowKind::Objective && problem.objectives.contains_key(&id) {
                    RequestError::invalid("the objective is excluded by includeObjectives: false")
                } else {
                    unknown()
                }
            })
        }
    }
}

/// Rows and columns within `depth` hops of `focus`, nearest first, up to `max_nodes`.
fn neighbourhood(
    structure: &Structure,
    focus: Node2,
    depth: usize,
    max_nodes: usize,
    cancel: &Cancel,
) -> std::result::Result<(Vec<bool>, Vec<bool>, bool), RequestError> {
    let (rows, cols) = (structure.rows.len(), structure.n_cols);
    let columns = structure.columns(cancel)?;

    let mut seen_rows = vec![false; rows];
    let mut seen_cols = vec![false; cols];
    let mut taken = 0usize;
    let mut truncated = false;
    let mut frontier = vec![focus];
    let visit = |node: Node2, seen_rows: &mut [bool], seen_cols: &mut [bool], taken: &mut usize| -> bool {
        let seen = if node.0 { &mut seen_rows[node.1] } else { &mut seen_cols[node.1] };
        if *seen {
            return false;
        }
        if *taken >= max_nodes {
            return false;
        }
        *seen = true;
        *taken += 1;
        true
    };
    visit(focus, &mut seen_rows, &mut seen_cols, &mut taken);
    for hop in 0..depth {
        let mut next = Vec::new();
        for (i, &(is_row, index)) in frontier.iter().enumerate() {
            cancel.tick(hop * rows + i)?;
            if is_row {
                for (col, _) in structure.entries(index) {
                    if !seen_cols[col] {
                        if visit((false, col), &mut seen_rows, &mut seen_cols, &mut taken) {
                            next.push((false, col));
                        } else {
                            truncated = true;
                        }
                    }
                }
            } else {
                for &row in columns.rows(index) {
                    if !seen_rows[row] {
                        if visit((true, row), &mut seen_rows, &mut seen_cols, &mut taken) {
                            next.push((true, row));
                        } else {
                            truncated = true;
                        }
                    }
                }
            }
        }
        frontier = next;
    }
    // Neighbours beyond the last hop were not visited, but that is the depth
    // limit, not truncation.
    Ok((seen_rows, seen_cols, truncated))
}

/// Without a focus: rows from the busiest down, each with all its variables,
/// while the node and edge caps allow.
fn busiest_rows(
    structure: &Structure,
    max_nodes: usize,
    max_edges: usize,
    cancel: &Cancel,
) -> std::result::Result<(Vec<bool>, Vec<bool>, bool), RequestError> {
    let rows = structure.rows.len();
    let mut order: Vec<usize> = (0..rows).collect();
    order.sort_by_key(|&r| (Reverse(structure.row_len(r)), r));
    let mut selected_rows = vec![false; rows];
    let mut selected_cols = vec![false; structure.n_cols];
    let (mut nodes, mut edges, mut truncated) = (0usize, 0usize, false);
    for (i, &row) in order.iter().enumerate() {
        cancel.tick(i)?;
        let new_cols = structure.entries(row).filter(|&(col, _)| !selected_cols[col]).count();
        let len = structure.row_len(row);
        if nodes + 1 + new_cols > max_nodes || edges + len > max_edges {
            truncated = true;
            continue;
        }
        selected_rows[row] = true;
        for (col, _) in structure.entries(row) {
            selected_cols[col] = true;
        }
        nodes += 1 + new_cols;
        edges += len;
    }
    Ok((selected_rows, selected_cols, truncated))
}

/// Root of `x` in a union-find forest, halving paths on the way.
fn find(parent: &mut [usize], mut x: usize) -> usize {
    while parent[x] != x {
        parent[x] = parent[parent[x]];
        x = parent[x];
    }
    x
}

struct ComponentIndex {
    count: usize,
    sizes: Vec<usize>,
    /// Component rank of every node: rows first, then variables.
    rank: Vec<usize>,
}

/// Union-find over the whole model (rows and variables).
fn components(structure: &Structure, cancel: &Cancel) -> std::result::Result<ComponentIndex, RequestError> {
    let rows = structure.rows.len();
    let nodes = rows + structure.n_cols;
    let mut parent: Vec<usize> = (0..nodes).collect();
    for row in 0..rows {
        cancel.tick(row)?;
        for (col, _) in structure.entries(row) {
            let (a, b) = (find(&mut parent, row), find(&mut parent, rows + col));
            if a != b {
                // The smaller index becomes the root, so the root is the first node.
                parent[a.max(b)] = a.min(b);
            }
        }
    }
    let mut size = vec![0usize; nodes];
    let roots: Vec<usize> = (0..nodes).map(|n| find(&mut parent, n)).collect();
    for &root in &roots {
        size[root] += 1;
    }
    let mut order: Vec<usize> = (0..nodes).filter(|&n| roots[n] == n).collect();
    order.sort_by_key(|&root| (Reverse(size[root]), root));
    let mut rank_of_root = vec![0usize; nodes];
    for (rank, &root) in order.iter().enumerate() {
        rank_of_root[root] = rank;
    }
    Ok(ComponentIndex {
        count: order.len(),
        sizes: order.iter().take(COMPONENT_SIZES).map(|&root| size[root]).collect(),
        rank: roots.iter().map(|&root| rank_of_root[root]).collect(),
    })
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::super::testing::{MODEL, doc, json, target};
    use super::*;

    fn params() -> Params {
        Params { target: target(), focus: None, include_objectives: true, max_nodes: DEFAULT_NODES, max_edges: DEFAULT_EDGES }
    }

    fn run(text: &str, params: &Params) -> Value {
        json(&handle(&doc(text), Some(1), params, &Cancel::new()).expect("graph"))
    }

    fn names(graph: &Value, kind: &str) -> Vec<String> {
        graph["nodes"]
            .as_array()
            .expect("nodes")
            .iter()
            .filter(|n| n["kind"] == kind)
            .map(|n| n["name"].as_str().unwrap_or("").to_owned())
            .collect()
    }

    /// `(source name, target name, coefficient)` of each edge.
    fn edge_list(graph: &Value) -> Vec<(String, String, Value)> {
        let name = |id: &Value| {
            graph["nodes"][usize::try_from(id.as_u64().expect("id")).expect("small id")]["name"].as_str().unwrap_or("").to_owned()
        };
        let e = &graph["edges"];
        (0..e["source"].as_array().expect("edges").len())
            .map(|i| (name(&e["source"][i]), name(&e["target"][i]), e["coefficient"][i].clone()))
            .collect()
    }

    #[test]
    fn whole_graph_has_rows_variables_and_directed_edges() {
        let graph = run(MODEL, &params());
        assert_eq!(names(&graph, "objective"), ["obj"]);
        assert_eq!(names(&graph, "constraint"), ["c1", "c2", "c3"]);
        assert_eq!(names(&graph, "variable"), ["x", "y", "z"]);
        let edges = edge_list(&graph);
        assert_eq!(edges.len(), 3 + 2 + 2 + 3);
        assert!(edges.contains(&("c2".into(), "y".into(), json!(4.0))));
        assert_eq!(graph["components"], json!({ "count": 1, "sizes": [7] }));
        assert_eq!(graph["isolated"], json!([]));
        assert_eq!(graph["truncated"], false);
        let hubs = graph["hubs"].as_array().expect("hubs");
        assert_eq!(
            (hubs[0]["name"].clone(), hubs[0]["degree"].clone()),
            (json!("x"), json!(4)),
            "x is in obj, c1, c2, c3; y ties and comes later"
        );
    }

    #[test]
    fn components_isolated_variables_and_parallel_arrays() {
        let graph = run("Minimize\n obj: a + c\nSubject To\n r1: a + b >= 1\n r2: c + d >= 1\nBounds\n w <= 4\nEnd\n", &params());
        assert_eq!(graph["isolated"], json!(["w"]));
        assert!(names(&graph, "variable").iter().all(|v| v != "w"), "isolated variables are listed, not drawn");
        // obj joins both halves: one component of 7 nodes plus the isolated variable.
        assert_eq!(graph["components"], json!({ "count": 2, "sizes": [7, 1] }));
        let e = &graph["edges"];
        let n = e["source"].as_array().expect("edges").len();
        assert_eq!((e["target"].as_array().map(Vec::len), e["coefficient"].as_array().map(Vec::len)), (Some(n), Some(n)));
        // Components are ranked by size, so the connected component is 0.
        assert!(graph["nodes"].as_array().expect("nodes").iter().all(|n| n["component"] == 0));
    }

    #[test]
    fn bare_references_have_null_coefficients() {
        let graph = run("Minimize\n obj: x + y\nSubject To\n ind: b = 1 -> x + y <= 3\nBinaries\n b\nEnd\n", &params());
        assert!(edge_list(&graph).contains(&("ind".into(), "b".into(), Value::Null)));
    }

    #[test]
    fn objectives_can_be_excluded() {
        let mut p = params();
        p.include_objectives = false;
        let graph = run(MODEL, &p);
        assert_eq!(names(&graph, "objective"), Vec::<String>::new());
        assert_eq!(graph["hubs"][0]["degree"], 3);
    }

    #[test]
    fn focus_follows_depth_hops_from_a_variable() {
        let text = "Minimize\n obj: a\nSubject To\n r1: a + b >= 1\n r2: b + c >= 1\n r3: c + d >= 1\nEnd\n";
        let mut p = params();
        p.include_objectives = false;
        p.focus = Some(Focus { kind: FocusKind::Variable, name: "a".into(), depth: Some(1) });
        let graph = run(text, &p);
        assert_eq!(
            (names(&graph, "constraint"), names(&graph, "variable")),
            (vec!["r1".to_owned()], vec!["a".to_owned()]),
            "depth 1: the rows of a, and only a: {graph}"
        );
        p.focus = Some(Focus { kind: FocusKind::Variable, name: "a".into(), depth: Some(2) });
        let graph = run(text, &p);
        assert_eq!(names(&graph, "variable"), ["a", "b"]);
        p.focus = Some(Focus { kind: FocusKind::Variable, name: "a".into(), depth: Some(3) });
        let graph = run(text, &p);
        assert_eq!(names(&graph, "constraint"), ["r1", "r2"]);
        assert_eq!(graph["truncated"], false, "the depth limit is not truncation");
        p.focus = Some(Focus { kind: FocusKind::Constraint, name: "r2".into(), depth: None });
        let graph = run(text, &p);
        assert_eq!((names(&graph, "constraint"), names(&graph, "variable")), (vec!["r2".to_owned()], vec!["b".to_owned(), "c".to_owned()]));
        // Degrees and components describe the whole model, not the slice.
        assert_eq!(graph["components"], json!({ "count": 1, "sizes": [7] }));
    }

    #[test]
    fn caps_truncate_and_keep_the_busiest_rows() {
        let text = "Minimize\n obj: a\nSubject To\n big: a + b + c + d >= 1\n small: e + f >= 1\n tiny: g >= 1\nEnd\n";
        let mut p = params();
        p.include_objectives = false;
        p.max_nodes = 6;
        let graph = run(text, &p);
        assert_eq!(names(&graph, "constraint"), ["big"], "big (1 + 4 nodes) fits; small would need 3 more");
        assert_eq!(graph["truncated"], true);
        assert!(names(&graph, "constraint").len() + names(&graph, "variable").len() <= 6);
        p.max_nodes = 100;
        p.max_edges = 4;
        let graph = run(text, &p);
        assert_eq!(graph["edges"]["source"].as_array().map(Vec::len), Some(4));
        assert_eq!(graph["truncated"], true);
        p.max_edges = 5;
        assert_eq!(run(text, &p)["truncated"], true, "small does not fit under 5 edges after big");
    }

    #[test]
    fn focus_node_cap_truncates() {
        let mut p = params();
        p.focus = Some(Focus { kind: FocusKind::Variable, name: "x".into(), depth: Some(1) });
        p.max_nodes = 2;
        let graph = run(MODEL, &p);
        assert_eq!(graph["nodes"].as_array().map(Vec::len), Some(2));
        assert_eq!(graph["truncated"], true);
    }

    #[test]
    fn rejects_bad_params() {
        let mut p = params();
        p.max_nodes = MAX_NODES + 1;
        assert!(matches!(handle(&doc(MODEL), None, &p, &Cancel::new()), Err(RequestError::InvalidParams(_))));
        let mut p = params();
        p.max_edges = 0;
        assert!(matches!(handle(&doc(MODEL), None, &p, &Cancel::new()), Err(RequestError::InvalidParams(_))));
        let mut p = params();
        p.focus = Some(Focus { kind: FocusKind::Variable, name: "x".into(), depth: Some(4) });
        assert!(matches!(handle(&doc(MODEL), None, &p, &Cancel::new()), Err(RequestError::InvalidParams(_))));
        p.focus = Some(Focus { kind: FocusKind::Constraint, name: "nope".into(), depth: None });
        assert!(matches!(handle(&doc(MODEL), None, &p, &Cancel::new()), Err(RequestError::InvalidParams(m)) if m.contains("nope")));
        p.focus = Some(Focus { kind: FocusKind::Variable, name: "c1".into(), depth: None });
        assert!(
            matches!(handle(&doc(MODEL), None, &p, &Cancel::new()), Err(RequestError::InvalidParams(_))),
            "c1 is a constraint, not a variable"
        );
        p.include_objectives = false;
        p.focus = Some(Focus { kind: FocusKind::Objective, name: "obj".into(), depth: None });
        assert!(
            matches!(handle(&doc(MODEL), None, &p, &Cancel::new()), Err(RequestError::InvalidParams(m)) if m.contains("includeObjectives"))
        );
        assert!(matches!(
            handle(&doc("min\n obj: x +\nst\n c: >= \nend\n"), None, &params(), &Cancel::new()),
            Err(RequestError::Failed(_))
        ));
    }

    #[test]
    fn cancellation_is_honoured() {
        let cancel = Cancel::new();
        cancel.cancel();
        assert_eq!(handle(&doc(MODEL), None, &params(), &cancel).err(), Some(RequestError::Cancelled));
    }
}
