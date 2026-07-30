//! Model conditioning diagnostics: why a solve is taking so many iterations,
//! and **which** rows and variables are responsible.
//!
//! The Numerics section (`5`) reports the model's *global* coefficient ranges,
//! and the solver log reports what `HiGHS` did. Neither connects the two: a log
//! saying "200k iterations" and a range saying "1e-4 to 1e6" leave you no way
//! to find the offending constraint. This module joins them, keeping one record
//! per constraint and per variable that carries both its structure (density,
//! coefficient spread, bound width) and its behaviour in the last solve
//! (activity, shadow price, whether it sat at a bound with a zero dual).
//!
//! Three independent signals, because they have different cures:
//!
//! - **Conditioning**: a row spanning many orders of magnitude makes the ratio
//!   test pick badly. Cured by scaling or by re-expressing the row's units.
//! - **Density**: a dense row or column destroys the sparsity of the basis
//!   factorisation. It makes each iteration *cost* more; it does not by itself
//!   make the solver take more of them.
//! - **Degeneracy**: many constraints active at the same vertex, so the simplex
//!   shuffles between bases without improving the objective. This is the usual
//!   cause of an iteration count far above the row count, and scaling will not
//!   fix it.
//!
//! The degeneracy figures are proxies computed from the final solution, not a
//! basis inspection — `HiGHS` does not hand back the basis here. They are
//! labelled as proxies wherever they are shown.

use std::collections::HashMap;

use lp_parser_rs::interner::NameId;
use lp_parser_rs::model::Constraint;
use lp_parser_rs::problem::LpProblem;

use crate::solver::{SolveResult, variable_bounds};
// Coefficient ratio above which a model counts as ill-conditioned. Shared with
// the Numerics pane so the two lenses agree.
use crate::widgets::numerics::RATIO_WARN_THRESHOLD as WIDE_RATIO;

/// How many entries to list in each ranked table.
const TOP_N: usize = 8;

/// Values below this magnitude are treated as zero when ranging coefficients.
const ZERO: f64 = 1e-12;

/// Tolerance for calling a value "at its bound" or a dual "zero" in the
/// degeneracy proxies. Deliberately loose: the point is the overall share, and
/// a solver's idea of "at a bound" carries its own feasibility tolerance.
const DEGENERACY_TOL: f64 = 1e-7;

/// Iterations-per-row ratio at which a solve stops looking healthy. A dual
/// simplex run normally settles within a small multiple of the row count.
const ITERATIONS_PER_ROW_ELEVATED: f64 = 5.0;

/// Share of binding rows carrying a zero dual above which degeneracy is the
/// prime suspect.
const DEGENERACY_SHARE_WARN: f64 = 0.30;

/// Magnitude range of a set of values, as `[min, max]` over non-zeros.
#[derive(Debug, Clone, Copy, Default)]
pub struct Range {
    pub min: f64,
    pub max: f64,
    pub count: usize,
}

impl Range {
    fn add(&mut self, value: f64) {
        let magnitude = value.abs();
        if magnitude <= ZERO {
            return;
        }
        if self.count == 0 {
            self.min = magnitude;
            self.max = magnitude;
        } else {
            self.min = self.min.min(magnitude);
            self.max = self.max.max(magnitude);
        }
        self.count += 1;
    }

    /// Max-to-min ratio, or `None` when there is nothing to compare.
    #[must_use]
    pub fn ratio(&self) -> Option<f64> {
        if self.count == 0 || self.min <= ZERO { None } else { Some(self.max / self.min) }
    }
}

/// One constraint, with its structure and its behaviour in the last solve.
#[derive(Debug, Clone)]
pub struct RowStat {
    pub name: String,
    /// Number of non-zero coefficients in the row.
    pub nnz: usize,
    /// Coefficient magnitude range within this row alone.
    pub range: Range,
    pub rhs: f64,
    /// Row activity at the optimum, when a solve has run.
    pub activity: Option<f64>,
    /// Shadow price at the optimum, when a solve has run.
    pub shadow_price: Option<f64>,
    /// Whether the row is active at the optimum.
    pub binding: bool,
    /// Binding but with a zero shadow price — the row holds the solution in
    /// place yet the objective is indifferent to it. A degenerate vertex is
    /// made of these, and they are what the simplex pivots around fruitlessly.
    pub degenerate: bool,
}

/// One variable, with its structure and its behaviour in the last solve.
#[derive(Debug, Clone)]
pub struct ColStat {
    pub name: String,
    /// Number of rows this variable appears in.
    pub nnz: usize,
    /// Coefficient magnitude range down this column.
    pub range: Range,
    /// Width of the variable's box, or `None` when a side is unbounded.
    pub bound_width: Option<f64>,
    /// Value at the optimum, when a solve has run.
    pub value: Option<f64>,
    /// Reduced cost at the optimum, when a solve has run.
    pub reduced_cost: Option<f64>,
    /// Whether the variable sits on one of its bounds.
    pub at_bound: bool,
    /// At a bound with zero reduced cost — an alternative optimum, and the
    /// plateau the simplex crawls along.
    pub degenerate: bool,
}

/// What the solver reported about the run, parsed from its log.
#[derive(Debug, Clone, Default)]
pub struct Telemetry {
    pub iterations: Option<u64>,
    /// Rows, columns and non-zeros left after `HiGHS`'s own presolve.
    pub presolved: Option<(u64, u64, u64)>,
    /// Primal-dual objective error: how far apart the two bounds finished.
    pub objective_error: Option<f64>,
    pub run_time: Option<f64>,
}

/// Degeneracy totals, aggregated from the per-entity records.
#[derive(Debug, Clone, Copy, Default)]
pub struct Degeneracy {
    pub rows_binding: usize,
    pub rows_degenerate: usize,
    pub vars_at_bound: usize,
    pub vars_degenerate: usize,
}

impl Degeneracy {
    /// Share of binding rows carrying a zero dual.
    #[must_use]
    #[allow(clippy::cast_precision_loss)] // counts are model sizes, far below 2^52
    pub fn row_share(&self) -> f64 {
        if self.rows_binding == 0 { 0.0 } else { self.rows_degenerate as f64 / self.rows_binding as f64 }
    }

    /// Share of at-bound variables carrying a zero reduced cost.
    #[must_use]
    #[allow(clippy::cast_precision_loss)] // counts are model sizes, far below 2^52
    pub fn col_share(&self) -> f64 {
        if self.vars_at_bound == 0 { 0.0 } else { self.vars_degenerate as f64 / self.vars_at_bound as f64 }
    }
}

/// The overall reading, so the pane can lead with a conclusion, not a table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// No solve yet — nothing to judge.
    Unknown,
    Healthy,
    /// Iteration count is high and degeneracy looks like the reason.
    Degenerate,
    /// Iteration count is high and the coefficient ranges are wide.
    IllConditioned,
    /// Iteration count is high without an obvious structural cause.
    Elevated,
}

impl Verdict {
    /// Headline shown at the top of the pane.
    pub const fn summary(self) -> &'static str {
        match self {
            Self::Unknown => "No solve yet \u{2014} press S to solve, then reopen for a reading",
            Self::Healthy => "Iteration count is in the normal range for this model size",
            Self::Degenerate => "Degeneracy is the prime suspect: many active rows carry a zero dual",
            Self::IllConditioned => "Wide coefficient ranges: the model is ill-conditioned",
            Self::Elevated => "Iteration count is high with no single structural cause",
        }
    }

    /// What to do about it.
    pub const fn advice(self) -> &'static str {
        match self {
            Self::Unknown | Self::Healthy => "",
            Self::Degenerate => {
                "Scaling will not help. Look at the degenerate rows below \u{2014} redundant or near-parallel constraints are the usual source."
            }
            Self::IllConditioned => "Rescale the worst rows/columns below, or re-express their units so coefficients sit nearer 1.",
            Self::Elevated => "Check the worst-conditioned and densest entries below; compare against a presolved run (P).",
        }
    }
}

/// Everything the diagnostics pane displays.
#[derive(Debug, Clone)]
pub struct Diagnostics {
    pub rows: usize,
    pub cols: usize,
    pub nnz: usize,
    /// Constraint matrix coefficients.
    pub matrix: Range,
    /// Objective coefficients.
    pub cost: Range,
    /// Right-hand sides.
    pub rhs: Range,
    /// Finite variable bounds.
    pub bound: Range,
    /// Rows ranked by intra-row coefficient ratio, worst first.
    pub worst_rows: Vec<RowStat>,
    /// Columns ranked by intra-column coefficient ratio, worst first.
    pub worst_cols: Vec<ColStat>,
    /// Rows ranked by non-zero count, densest first.
    pub densest_rows: Vec<RowStat>,
    /// Columns ranked by non-zero count, densest first.
    pub densest_cols: Vec<ColStat>,
    /// Degenerate rows, densest first — the ones the simplex pivots around.
    pub degenerate_rows: Vec<RowStat>,
    /// Degenerate columns, densest first.
    pub degenerate_cols: Vec<ColStat>,
    pub telemetry: Option<Telemetry>,
    pub degeneracy: Option<Degeneracy>,
    pub verdict: Verdict,
}

impl Diagnostics {
    /// Iterations per row, the headline health ratio.
    #[must_use]
    #[allow(clippy::cast_precision_loss)] // iteration and row counts are far below 2^52
    pub fn iterations_per_row(&self) -> Option<f64> {
        let iterations = self.telemetry.as_ref()?.iterations?;
        if self.rows == 0 { None } else { Some(iterations as f64 / self.rows as f64) }
    }
}

/// Analyse `problem`, folding in the last solve's results when there is one.
#[must_use]
pub fn analyse(problem: &LpProblem, solve: Option<&SolveResult>) -> Diagnostics {
    let MatrixScan { matrix, rhs, rows: mut row_stats, columns } = scan_matrix(problem);
    let cost = scan_cost(problem);
    let bound = scan_bounds(problem);
    let mut col_stats = build_col_stats(problem, &columns);

    let nnz = row_stats.iter().map(|row| row.nnz).sum();

    // Join the solve outcome onto the structural records, so every ranking
    // below can mix the two without a second lookup.
    if let Some(result) = solve {
        attach_row_results(&mut row_stats, result);
        attach_col_results(problem, &mut col_stats, result);
    }
    let degeneracy = solve.map(|_| aggregate_degeneracy(&row_stats, &col_stats));
    let telemetry = solve.map(|result| parse_telemetry(&result.solver_log));

    // Three rankings over the same records: each answers a different question,
    // and they rarely have the same winner.
    let densest_rows = top(&mut row_stats, |a, b| b.nnz.cmp(&a.nnz));
    let densest_cols = top(&mut col_stats, |a, b| b.nnz.cmp(&a.nnz));
    let worst_rows = top(&mut row_stats, |a, b| compare_ratio(a.range.ratio(), b.range.ratio()));
    let worst_cols = top(&mut col_stats, |a, b| compare_ratio(a.range.ratio(), b.range.ratio()));
    let degenerate_rows = top_filtered(&row_stats, |row| row.degenerate, |a, b| b.nnz.cmp(&a.nnz));
    let degenerate_cols = top_filtered(&col_stats, |col| col.degenerate, |a, b| b.nnz.cmp(&a.nnz));

    let mut diagnostics = Diagnostics {
        rows: problem.constraint_count(),
        cols: problem.variable_count(),
        nnz,
        matrix,
        cost,
        rhs,
        bound,
        worst_rows,
        worst_cols,
        densest_rows,
        densest_cols,
        degenerate_rows,
        degenerate_cols,
        telemetry,
        degeneracy,
        verdict: Verdict::Unknown,
    };
    diagnostics.verdict = judge(&diagnostics);
    diagnostics
}

/// Rank `items` in place and clone out the best [`TOP_N`]. Cloning keeps the
/// source list available for the next ranking, which is cheap at this size.
fn top<T: Clone>(items: &mut [T], order: impl FnMut(&T, &T) -> std::cmp::Ordering) -> Vec<T> {
    items.sort_by(order);
    items.iter().take(TOP_N).cloned().collect()
}

/// Rank the subset matching `keep`, without disturbing `items`.
fn top_filtered<T: Clone>(items: &[T], keep: impl Fn(&T) -> bool, order: impl FnMut(&T, &T) -> std::cmp::Ordering) -> Vec<T> {
    let mut kept: Vec<T> = items.iter().filter(|item| keep(item)).cloned().collect();
    kept.sort_by(order);
    kept.truncate(TOP_N);
    kept
}

/// Order two optional ratios, worst (largest) first, with `None` last.
fn compare_ratio(a: Option<f64>, b: Option<f64>) -> std::cmp::Ordering {
    match (a, b) {
        (Some(x), Some(y)) => y.partial_cmp(&x).unwrap_or(std::cmp::Ordering::Equal),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => std::cmp::Ordering::Equal,
    }
}

/// Everything one pass over the constraint matrix yields.
struct MatrixScan {
    /// Global matrix coefficient range.
    matrix: Range,
    /// Global right-hand-side range.
    rhs: Range,
    /// One record per standard constraint.
    rows: Vec<RowStat>,
    /// Coefficient range and non-zero count per variable.
    columns: HashMap<NameId, (Range, usize)>,
}

/// Single pass over the constraint matrix, collecting the global ranges, the
/// per-row records and the per-column ranges together.
fn scan_matrix(problem: &LpProblem) -> MatrixScan {
    let mut matrix = Range::default();
    let mut rhs_range = Range::default();
    let mut rows = Vec::with_capacity(problem.constraint_count());
    let mut columns: HashMap<NameId, (Range, usize)> = HashMap::with_capacity(problem.variable_count());

    for constraint in problem.constraints.values() {
        // SOS sets carry weights, not matrix coefficients, and the solve skips them.
        let Constraint::Standard { name, coefficients, rhs, .. } = constraint else {
            continue;
        };
        let mut row = Range::default();
        for coefficient in coefficients {
            if coefficient.value.abs() <= ZERO {
                continue;
            }
            row.add(coefficient.value);
            matrix.add(coefficient.value);
            let entry = columns.entry(coefficient.name).or_insert((Range::default(), 0));
            entry.0.add(coefficient.value);
            entry.1 += 1;
        }
        rhs_range.add(*rhs);
        rows.push(RowStat {
            name: problem.resolve(*name).to_owned(),
            nnz: row.count,
            range: row,
            rhs: *rhs,
            activity: None,
            shadow_price: None,
            binding: false,
            degenerate: false,
        });
    }

    MatrixScan { matrix, rhs: rhs_range, rows, columns }
}

/// Objective coefficient magnitudes across every objective.
fn scan_cost(problem: &LpProblem) -> Range {
    let mut cost = Range::default();
    for objective in problem.objectives.values() {
        for coefficient in &objective.coefficients {
            cost.add(coefficient.value);
        }
    }
    cost
}

/// Magnitudes of the finite variable bounds.
fn scan_bounds(problem: &LpProblem) -> Range {
    let mut bound = Range::default();
    for variable in problem.variables.values() {
        for value in [variable.bounds.lower, variable.bounds.upper].into_iter().flatten() {
            if value.is_finite() {
                bound.add(value);
            }
        }
    }
    bound
}

/// Turn the per-column ranges gathered during the matrix scan into records.
fn build_col_stats(problem: &LpProblem, columns: &HashMap<NameId, (Range, usize)>) -> Vec<ColStat> {
    problem
        .variables
        .keys()
        .map(|var_id| {
            let (range, nnz) = columns.get(var_id).copied().unwrap_or_default();
            let (_, lower, upper) = variable_bounds(problem.variables.get(var_id));
            let bound_width = (lower.is_finite() && upper.is_finite()).then_some(upper - lower);
            ColStat {
                name: problem.resolve(*var_id).to_owned(),
                nnz,
                range,
                bound_width,
                value: None,
                reduced_cost: None,
                at_bound: false,
                degenerate: false,
            }
        })
        .collect()
}

/// Join row activities and shadow prices onto the structural records.
fn attach_row_results(rows: &mut [RowStat], result: &SolveResult) {
    let activities: HashMap<&str, f64> = result.row_values.iter().map(|(name, value)| (name.as_str(), *value)).collect();
    let shadow: HashMap<&str, f64> = result.shadow_prices.iter().map(|(name, value)| (name.as_str(), *value)).collect();

    for row in rows {
        let Some(&activity) = activities.get(row.name.as_str()) else {
            continue;
        };
        let price = shadow.get(row.name.as_str()).copied();
        row.activity = Some(activity);
        row.shadow_price = price;
        row.binding = (activity - row.rhs).abs() <= DEGENERACY_TOL * (1.0 + row.rhs.abs());
        row.degenerate = row.binding && price.is_some_and(|value| value.abs() <= DEGENERACY_TOL);
    }
}

/// Join variable values and reduced costs onto the structural records.
fn attach_col_results(problem: &LpProblem, cols: &mut [ColStat], result: &SolveResult) {
    let values: HashMap<&str, f64> = result.variables.iter().map(|(name, value)| (name.as_str(), *value)).collect();
    let reduced: HashMap<&str, f64> = result.reduced_costs.iter().map(|(name, value)| (name.as_str(), *value)).collect();

    for col in cols {
        let Some(&value) = values.get(col.name.as_str()) else {
            continue;
        };
        let Some(var_id) = problem.name_id(&col.name) else {
            continue;
        };
        let (_, lower, upper) = variable_bounds(problem.variables.get(&var_id));
        let cost = reduced.get(col.name.as_str()).copied();
        col.value = Some(value);
        col.reduced_cost = cost;
        col.at_bound =
            [lower, upper].into_iter().any(|bound| bound.is_finite() && (value - bound).abs() <= DEGENERACY_TOL * (1.0 + bound.abs()));
        col.degenerate = col.at_bound && cost.is_some_and(|value| value.abs() <= DEGENERACY_TOL);
    }
}

/// Totals over the per-entity records, for the headline.
fn aggregate_degeneracy(rows: &[RowStat], cols: &[ColStat]) -> Degeneracy {
    Degeneracy {
        rows_binding: rows.iter().filter(|row| row.binding).count(),
        rows_degenerate: rows.iter().filter(|row| row.degenerate).count(),
        vars_at_bound: cols.iter().filter(|col| col.at_bound).count(),
        vars_degenerate: cols.iter().filter(|col| col.degenerate).count(),
    }
}

/// Pull the figures `HiGHS` prints about its own run out of the captured log.
///
/// Parsing the log rather than recomputing keeps these numbers authoritative:
/// they are what the solver did, including the effect of its internal presolve.
/// Every field is optional — the log format is not an API, so a missing line
/// leaves a gap rather than breaking the pane.
fn parse_telemetry(log: &str) -> Telemetry {
    let mut telemetry = Telemetry::default();

    for line in log.lines() {
        let line = line.trim();
        if let Some(value) = line.strip_prefix("Simplex   iterations:").or_else(|| line.strip_prefix("Simplex iterations:")) {
            telemetry.iterations = value.trim().parse().ok();
        } else if let Some(value) = line.strip_prefix("P-D objective error :") {
            telemetry.objective_error = value.trim().parse().ok();
        } else if let Some(value) = line.strip_prefix("HiGHS run time      :") {
            telemetry.run_time = value.trim().parse().ok();
        } else if line.starts_with("Presolve reductions:") {
            telemetry.presolved = parse_presolve_reductions(line);
        }
    }
    telemetry
}

/// Parse `Presolve reductions: rows 122(-18); columns 162(-0); nonzeros 813(-402)`.
fn parse_presolve_reductions(line: &str) -> Option<(u64, u64, u64)> {
    let field = |label: &str| -> Option<u64> {
        let rest = line.split(label).nth(1)?.trim_start();
        let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
        digits.parse().ok()
    };
    Some((field("rows ")?, field("columns ")?, field("nonzeros ")?))
}

/// Reduce the measurements to a single reading.
///
/// Degeneracy is checked before conditioning: it is both the more common cause
/// of a runaway iteration count and the one scaling will not fix, so naming it
/// first avoids sending anyone off to rescale a model that does not need it.
fn judge(diagnostics: &Diagnostics) -> Verdict {
    // No telemetry at all means no solve has run; a solve that reports no
    // iterations means HiGHS settled the model in its own presolve, which is
    // the healthiest outcome there is.
    if diagnostics.telemetry.is_none() {
        return Verdict::Unknown;
    }
    let Some(per_row) = diagnostics.iterations_per_row() else {
        return Verdict::Healthy;
    };
    if per_row < ITERATIONS_PER_ROW_ELEVATED {
        return Verdict::Healthy;
    }
    if diagnostics.degeneracy.is_some_and(|d| d.row_share() >= DEGENERACY_SHARE_WARN) {
        return Verdict::Degenerate;
    }
    if diagnostics.matrix.ratio().is_some_and(|ratio| ratio >= WIDE_RATIO) {
        return Verdict::IllConditioned;
    }
    Verdict::Elevated
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(source: &str) -> LpProblem {
        LpProblem::parse(source).expect("test fixture must parse")
    }

    #[test]
    fn ranges_cover_matrix_cost_rhs_and_bounds() {
        let problem = parse("Minimize\n obj: 2 x + 0.5 y\nSubject To\n c1: 100 x + 0.01 y <= 40\nBounds\n x <= 7\nEnd");
        let diagnostics = analyse(&problem, None);

        let close = |actual: f64, expected: f64| (actual - expected).abs() < 1e-9;
        assert!(diagnostics.matrix.ratio().is_some_and(|r| close(r, 10_000.0)), "100 / 0.01 spans four orders of magnitude");
        assert!(diagnostics.cost.ratio().is_some_and(|r| close(r, 4.0)), "2 / 0.5");
        assert!(close(diagnostics.rhs.max, 40.0));
        assert!(close(diagnostics.bound.max, 7.0));
    }

    #[test]
    fn the_worst_conditioned_row_is_ranked_first() {
        let problem = parse(
            "Minimize\n obj: x + y + z\nSubject To\n\
             tame: x + 2 y <= 10\n wild: 1000000 x + 0.001 z <= 5\n mild: 3 y + 6 z <= 8\nEnd",
        );
        let diagnostics = analyse(&problem, None);

        assert_eq!(diagnostics.worst_rows[0].name, "wild", "the row spanning nine orders of magnitude ranks first");
        assert!(diagnostics.worst_rows[0].range.ratio().is_some_and(|ratio| ratio > 1e8));
    }

    #[test]
    fn the_worst_conditioned_column_is_ranked_first() {
        let problem = parse("Minimize\n obj: x + y\nSubject To\n c1: 1000000 x + y <= 10\n c2: 0.001 x + 2 y <= 8\nEnd");
        let diagnostics = analyse(&problem, None);

        assert_eq!(diagnostics.worst_cols[0].name, "x", "x's column spans 1e6 down to 1e-3");
    }

    #[test]
    fn density_ranking_is_separate_from_conditioning() {
        let problem = parse(
            "Minimize\n obj: a + b + c + d\nSubject To\n\
             dense: a + b + c + d <= 10\n skewed: 1000000 a + 0.001 b <= 5\nEnd",
        );
        let diagnostics = analyse(&problem, None);

        assert_eq!(diagnostics.densest_rows[0].name, "dense", "density ranks by non-zero count");
        assert_eq!(diagnostics.worst_rows[0].name, "skewed", "conditioning ranks by coefficient spread");
    }

    #[test]
    fn telemetry_is_read_out_of_a_real_highs_log() {
        let log = "\
LP has 140 rows; 162 cols; 1215 nonzeros
Presolve reductions: rows 122(-18); columns 162(-0); nonzeros 813(-402)
Model status        : Optimal
Simplex   iterations: 131
P-D objective error :  1.8015862075e-16
HiGHS run time      :          0.04
";
        let telemetry = parse_telemetry(log);

        assert_eq!(telemetry.iterations, Some(131));
        assert_eq!(telemetry.presolved, Some((122, 162, 813)));
        assert_eq!(telemetry.run_time, Some(0.04));
        assert!(telemetry.objective_error.is_some_and(|error| error < 1e-15));
    }

    #[test]
    fn a_missing_log_line_leaves_its_field_empty() {
        let telemetry = parse_telemetry("Model status        : Optimal\n");

        assert_eq!(telemetry.iterations, None);
        assert_eq!(telemetry.presolved, None);
    }

    #[test]
    fn solve_results_are_attributed_to_named_rows_and_columns() {
        // The whole point of the pane: after a solve, each record carries its
        // own activity and dual, so an offender can be named rather than
        // inferred from a global statistic.
        let problem = parse("Minimize\n obj: x + y\nSubject To\n tight: x + y >= 2\n slack: x + y <= 900\nEnd");
        let solved = crate::solver::solve_problem(&problem).expect("the model solves");
        let diagnostics = analyse(&problem, Some(&solved));

        let by_name = |name: &str| diagnostics.densest_rows.iter().find(|row| row.name == name).expect("row present").clone();

        let tight = by_name("tight");
        assert!(tight.binding, "the >= 2 row is active at the optimum");
        assert!(tight.activity.is_some_and(|activity| (activity - 2.0).abs() < 1e-6));
        assert!(tight.shadow_price.is_some(), "an active row carries a shadow price");

        let slack = by_name("slack");
        assert!(!slack.binding, "the <= 900 row is nowhere near active");
        assert!(!slack.degenerate, "a non-binding row is not a degeneracy suspect");
    }

    #[test]
    fn degenerate_rows_are_listed_by_name() {
        // Every row is active at the origin and no dual is non-zero: the
        // textbook shape of a degenerate vertex.
        let problem = parse("Minimize\n obj: x + y\nSubject To\n c1: x + y >= 0\n c2: x - y >= 0\nEnd");
        let solved = crate::solver::solve_problem(&problem).expect("the model solves");
        let diagnostics = analyse(&problem, Some(&solved));

        let degeneracy = diagnostics.degeneracy.expect("a solve populates the degeneracy proxies");
        assert!(degeneracy.rows_binding > 0, "both rows are active at the optimum");
        assert!(!diagnostics.degenerate_rows.is_empty(), "the degenerate rows are named, not just counted");
        assert!(
            diagnostics.degenerate_rows.iter().all(|row| row.binding && row.degenerate),
            "the list holds only rows that are both active and dual-zero"
        );
    }

    #[test]
    fn without_a_solve_the_verdict_is_unknown_and_nothing_is_attributed() {
        let problem = parse("Minimize\n obj: x\nSubject To\n c1: x >= 1\n c2: x + 0 y >= 0\nEnd");
        let diagnostics = analyse(&problem, None);

        assert_eq!(diagnostics.verdict, Verdict::Unknown);
        assert_eq!(diagnostics.iterations_per_row(), None);
        assert!(diagnostics.telemetry.is_none());
        assert!(diagnostics.degeneracy.is_none());
        assert!(diagnostics.densest_rows.iter().all(|row| row.activity.is_none()), "no solve means no attribution");
    }

    #[test]
    fn a_short_solve_reads_as_healthy() {
        let problem = parse("Minimize\n obj: x + y\nSubject To\n c1: x + y >= 2\n c2: x - y <= 1\nEnd");
        let solved = crate::solver::solve_problem(&problem).expect("the model solves");
        let diagnostics = analyse(&problem, Some(&solved));

        assert_eq!(diagnostics.verdict, Verdict::Healthy, "a handful of iterations over two rows is healthy");
    }
}
