//! Row checks: emptiness, bounds-based feasibility, clustering of duplicate,
//! proportional, dominated and contradictory rows, and numerical scaling.

// Exact comparisons are intended where noted: zero tests, infinities and
// values copied from the same source.
#![allow(clippy::float_cmp)]

use std::collections::hash_map::Entry;

use rustc_hash::{FxHashMap, FxHashSet};

use super::diagnostics::{Ctx, DiagnosticCode, DiagnosticSubject, RelatedSubject, Sink, SubjectSite, fmt_num};
use super::domains::activity_domain;
use crate::interner::NameId;
use crate::model::{Coefficient, ComparisonOp, Constraint, ConstraintClass};

/// Variables listed in one diagnostic at most.
const LIST_LIMIT: usize = 10;

/// Merge repeated variables by summing, drop exact zeros and sort by name id.
/// `None` when a coefficient is not finite.
pub(super) fn merge_terms(coefficients: &[Coefficient]) -> Option<Vec<(NameId, f64)>> {
    let mut terms: Vec<(NameId, f64)> = Vec::with_capacity(coefficients.len());
    for coefficient in coefficients {
        if !coefficient.value.is_finite() {
            return None;
        }
        terms.push((coefficient.name, coefficient.value));
    }
    terms.sort_by_key(|(id, _)| *id);
    let mut merged: Vec<(NameId, f64)> = Vec::with_capacity(terms.len());
    for (id, value) in terms {
        match merged.last_mut() {
            Some((last, sum)) if *last == id => *sum += value,
            _ => merged.push((id, value)),
        }
    }
    merged.retain(|(_, value)| *value != 0.0);
    Some(merged)
}

/// A linear form divided by its first coefficient.
#[derive(Debug, Clone)]
pub(super) struct NormVec {
    pub vars: Vec<NameId>,
    pub coeffs: Vec<f64>,
    /// The first coefficient before normalisation (never zero).
    pub pivot: f64,
}

/// Normalise merged terms; `None` when there are none.
pub(super) fn normalise(terms: &[(NameId, f64)]) -> Option<NormVec> {
    let pivot = terms.first()?.1;
    debug_assert!(pivot != 0.0, "merged terms hold no zeros");
    Some(NormVec { vars: terms.iter().map(|(id, _)| *id).collect(), coeffs: terms.iter().map(|(_, c)| c / pivot).collect(), pivot })
}

fn near(a: f64, b: f64, tolerance: f64) -> bool {
    a == b || (a.is_finite() && b.is_finite() && (a - b).abs() <= tolerance * a.abs().max(b.abs()).max(1.0))
}

fn at_most(a: f64, b: f64, tolerance: f64) -> bool {
    a <= b || near(a, b, tolerance)
}

fn vectors_match(a: &[f64], b: &[f64], tolerance: f64) -> bool {
    a.iter().zip(b).all(|(x, y)| near(*x, *y, tolerance))
}

type Context = Option<(NameId, bool)>;

/// Group vectors that match within `tolerance`. Each group has at least two
/// members, ascending by item index; groups are ordered by their first member.
///
/// Items are bucketed by `(context, variables)`, sorted by coefficients and
/// grouped against the first member of each run, so a reported match is always
/// one the comparison proves; a match can be missed but never invented.
pub(super) fn cluster<C: std::hash::Hash + Eq + Copy>(items: &[(C, &NormVec)], tolerance: f64) -> Vec<Vec<usize>> {
    let mut slots: FxHashMap<(C, &[NameId]), usize> = FxHashMap::default();
    let mut buckets: Vec<Vec<usize>> = Vec::new();
    for (index, (context, vector)) in items.iter().enumerate() {
        match slots.entry((*context, vector.vars.as_slice())) {
            Entry::Occupied(slot) => buckets[*slot.get()].push(index),
            Entry::Vacant(slot) => {
                slot.insert(buckets.len());
                buckets.push(vec![index]);
            }
        }
    }
    let mut groups = Vec::new();
    for mut bucket in buckets {
        if bucket.len() < 2 {
            continue;
        }
        bucket.sort_by(|&a, &b| {
            let (x, y) = (&items[a].1.coeffs, &items[b].1.coeffs);
            x.iter().zip(y).map(|(p, q)| p.total_cmp(q)).find(|o| o.is_ne()).unwrap_or(std::cmp::Ordering::Equal)
        });
        let mut start = 0;
        while start < bucket.len() {
            let anchor = &items[bucket[start]].1.coeffs;
            let mut end = start + 1;
            while end < bucket.len() && vectors_match(anchor, &items[bucket[end]].1.coeffs, tolerance) {
                end += 1;
            }
            if end - start >= 2 {
                let mut group = bucket[start..end].to_vec();
                group.sort_unstable();
                groups.push(group);
            }
            start = end;
        }
    }
    groups.sort_by_key(|g| g[0]);
    groups
}

/// Emit `explicit-zero-coefficient` for one row or objective.
pub(super) fn explicit_zero(sink: &mut Sink, subject: DiagnosticSubject, name: &str, variables: &[&str]) {
    let mut listed: Vec<&str> = Vec::new();
    for variable in variables {
        if !listed.contains(variable) {
            listed.push(variable);
        }
    }
    let total = listed.len();
    listed.truncate(LIST_LIMIT);
    let more = if total > LIST_LIMIT { format!(" and {} more", total - LIST_LIMIT) } else { String::new() };
    sink.push(
        DiagnosticCode::ExplicitZeroCoefficient,
        format!("'{name}' has explicit zero coefficients for {}{more}", listed.join(", ")),
        Some("A zero term has no effect. It may be left over from generated code; remove it or check the intended coefficient."),
        Some(subject),
        Vec::new(),
    );
}

/// A normalised, comparable row.
struct Row {
    id: NameId,
    context: Context,
    norm: NormVec,
    lower: f64,
    upper: f64,
    offset: Option<usize>,
}

impl Row {
    /// 0 for a lower-bounding row, 1 for an upper-bounding row, 2 for an equality.
    fn direction(&self) -> usize {
        if self.lower == self.upper { 2 } else { usize::from(self.upper != f64::INFINITY) }
    }

    /// Whether two rows come from one statement (a ranged constraint expands
    /// to `c1` and `c1_rng`) and so must never be compared with each other.
    fn same_source(&self, other: &Self, ctx: &Ctx<'_>) -> bool {
        if self.offset.is_some() && self.offset == other.offset {
            return true;
        }
        let (a, b) = (ctx.name(self.id), ctx.name(other.id));
        a.strip_prefix(b).is_some_and(|rest| rest.starts_with("_rng")) || b.strip_prefix(a).is_some_and(|rest| rest.starts_with("_rng"))
    }
}

const fn op_symbol(operator: ComparisonOp) -> &'static str {
    match operator {
        ComparisonOp::LTE => "<=",
        ComparisonOp::LT => "<",
        ComparisonOp::GTE => ">=",
        ComparisonOp::GT => ">",
        ComparisonOp::EQ => "=",
    }
}

/// Activity bounds of a row over the variable domains.
struct Activity {
    lower: f64,
    upper: f64,
    /// Row tolerance.
    tau: f64,
}

/// `None` when a variable's domain is empty or unknown.
fn activity(ctx: &Ctx<'_>, domains: &[Option<(f64, f64)>], terms: &[(NameId, f64)], rhs: f64) -> Option<Activity> {
    let (mut lower, mut upper) = (0.0_f64, 0.0_f64);
    let (mut lower_infinite, mut upper_infinite) = (false, false);
    let mut scale = 0.0_f64;
    for &(id, a) in terms {
        let (l, u) = (*domains.get(id.index())?).filter(|(l, u)| l <= u)?;
        let (low, high) = if a > 0.0 { (a * l, a * u) } else { (a * u, a * l) };
        if low == f64::NEG_INFINITY {
            lower_infinite = true;
        } else if low.is_finite() {
            lower += low;
        } else {
            return None;
        }
        if high == f64::INFINITY {
            upper_infinite = true;
        } else if high.is_finite() {
            upper += high;
        } else {
            return None;
        }
        let extent = l.abs().max(u.abs());
        if extent.is_finite() {
            scale += a.abs() * extent;
        }
    }
    Some(Activity {
        lower: if lower_infinite { f64::NEG_INFINITY } else { lower },
        upper: if upper_infinite { f64::INFINITY } else { upper },
        tau: ctx.config.feasibility_tolerance * 1.0_f64.max(rhs.abs()).max(scale),
    })
}

struct Scale {
    min: f64,
    max: f64,
    count: usize,
}

impl Scale {
    fn new() -> Self {
        Self { min: f64::INFINITY, max: 0.0, count: 0 }
    }

    fn update(&mut self, value: f64) {
        let magnitude = value.abs();
        if magnitude > 0.0 && magnitude.is_finite() {
            self.min = self.min.min(magnitude);
            self.max = self.max.max(magnitude);
            self.count += 1;
        }
    }

    /// Ratio of largest to smallest, for two or more entries.
    fn ratio(&self) -> Option<f64> {
        (self.count >= 2).then_some(self.max / self.min)
    }
}

fn orders_of_magnitude(ratio: f64) -> String {
    format!("{:.0}", ratio.log10().round())
}

#[expect(clippy::too_many_lines, reason = "one pass over the rows runs every per-row check")]
pub(super) fn run(ctx: &Ctx<'_>, sink: &mut Sink) {
    let problem = ctx.problem;
    let config = ctx.config;
    let thresholds = &config.thresholds;

    let mut domains: Vec<Option<(f64, f64)>> = vec![None; problem.interner.len()];
    for (id, variable) in &problem.variables {
        domains[id.index()] = Some(activity_domain(ctx, variable));
    }

    // Statements that expand to several rows (a ranged constraint is `c1` and
    // `c1_rng`) share a byte offset; their halves must be judged together.
    let mut statement_rows: FxHashMap<usize, (usize, NameId)> = FxHashMap::default();
    for (id, constraint) in &problem.constraints {
        if matches!(constraint, Constraint::Standard { .. })
            && let Some(offset) = constraint.byte_offset()
        {
            statement_rows.entry(offset).or_insert((0, *id)).0 += 1;
        }
    }
    // (row, byte offset, operator, rhs) of every row found redundant by bounds.
    let mut redundant_rows: Vec<(NameId, Option<usize>, ComparisonOp, f64)> = Vec::new();

    let mut rows: Vec<Row> = Vec::new();
    let mut singletons: Vec<(NameId, NameId, f64, ComparisonOp, f64)> = Vec::new();
    let mut suppressed_singletons: FxHashSet<NameId> = FxHashSet::default();
    let mut columns: Vec<Scale> = (0..problem.variables.len()).map(|_| Scale::new()).collect();
    let mut all_scale = Scale::new();
    let mut equality_rows = 0_usize;
    let mut equality_vars: FxHashSet<NameId> = FxHashSet::default();

    for (id, constraint) in &problem.constraints {
        let name = ctx.name(*id);
        let class = problem.constraint_class(*id);
        let (coefficients, operator, rhs, indicator) = match constraint {
            Constraint::Standard { coefficients, operator, rhs, .. } | Constraint::Quadratic { coefficients, operator, rhs, .. } => {
                (coefficients, *operator, *rhs, None)
            }
            Constraint::Indicator { coefficients, operator, rhs, variable, active_value, .. } => {
                (coefficients, *operator, *rhs, Some((*variable, *active_value)))
            }
            Constraint::SOS { .. } | Constraint::General { .. } => continue,
        };
        let is_standard = matches!(constraint, Constraint::Standard { .. });

        // 20. explicit-zero-coefficient
        let zeros: Vec<&str> = coefficients.iter().filter(|t| t.value == 0.0).map(|t| ctx.name(t.name)).collect();
        if !zeros.is_empty() {
            explicit_zero(sink, DiagnosticSubject::constraint(name), name, &zeros);
        }

        // 21. repeated-variable-in-row
        let mut seen: FxHashSet<NameId> = FxHashSet::default();
        let mut reported: FxHashSet<NameId> = FxHashSet::default();
        for term in coefficients {
            if !seen.insert(term.name) && reported.insert(term.name) {
                let variable = ctx.name(term.name);
                sink.push(
                    DiagnosticCode::RepeatedVariableInRow,
                    format!("Constraint '{name}' lists variable '{variable}' more than once"),
                    Some("Solvers add the coefficients together. Merge the terms to make the row easier to read."),
                    Some(DiagnosticSubject::constraint(name).with_variable(variable)),
                    Vec::new(),
                );
            }
        }

        // Numerical scaling of the row.
        let mut row_scale = Scale::new();
        for term in coefficients {
            let magnitude = term.value.abs();
            row_scale.update(term.value);
            all_scale.update(term.value);
            // Quadratic rows count towards the model's scale but not towards columns.
            if (is_standard || indicator.is_some())
                && let Some(slot) = problem.variables.get_index_of(&term.name)
            {
                columns[slot].update(term.value);
            }
            if magnitude == 0.0 || !magnitude.is_finite() {
                continue;
            }
            let (code, label) = if magnitude > thresholds.large_coefficient_threshold {
                (DiagnosticCode::LargeCoefficient, "large")
            } else if magnitude < thresholds.small_coefficient_threshold {
                (DiagnosticCode::SmallCoefficient, "small")
            } else {
                continue;
            };
            let variable = ctx.name(term.name);
            sink.push(
                code,
                format!("Coefficient {} of '{variable}' in constraint '{name}' is {label}", fmt_num(term.value)),
                Some("Very large or tiny coefficients lose precision. Rescale the variable or the row."),
                Some(DiagnosticSubject::constraint(name).with_variable(variable)),
                Vec::new(),
            );
        }
        if let Some(ratio) = row_scale.ratio().filter(|r| *r > thresholds.coefficient_ratio_threshold) {
            sink.push(
                DiagnosticCode::RowCoefficientRange,
                format!("Constraint '{name}' has coefficients spanning about {} orders of magnitude", orders_of_magnitude(ratio)),
                Some("Rescale the variables or the row so its coefficients are of similar size."),
                Some(DiagnosticSubject::constraint(name)),
                Vec::new(),
            );
        }
        if rhs.is_finite() && rhs.abs() > thresholds.large_rhs_threshold {
            sink.push(
                DiagnosticCode::LargeRhs,
                format!("Constraint '{name}' has a large right-hand side ({})", fmt_num(rhs)),
                Some("A huge right-hand side loses precision. Rescale the row or change units."),
                Some(DiagnosticSubject::constraint(name)),
                Vec::new(),
            );
        }
        if rhs.is_finite() && row_scale.max > 0.0 && rhs.abs() / row_scale.max > config.rhs_coefficient_ratio_threshold {
            sink.push(
                DiagnosticCode::RhsCoefficientRatio,
                format!("Constraint '{name}' has a right-hand side {} times its largest coefficient", fmt_num(rhs.abs() / row_scale.max)),
                Some("A right-hand side far larger than the coefficients suggests a unit mismatch."),
                Some(DiagnosticSubject::constraint(name)),
                Vec::new(),
            );
        }

        if !is_standard && indicator.is_none() {
            continue;
        }
        if class == ConstraintClass::UserCut {
            continue;
        }
        let Some(terms) = merge_terms(coefficients) else {
            continue;
        };

        // 18/19. empty rows
        if terms.is_empty() && is_standard {
            if rhs.is_finite() {
                let tolerance = config.feasibility_tolerance * rhs.abs().max(1.0);
                let holds = match operator {
                    ComparisonOp::LTE | ComparisonOp::LT => rhs >= -tolerance,
                    ComparisonOp::GTE | ComparisonOp::GT => rhs <= tolerance,
                    ComparisonOp::EQ => rhs.abs() <= tolerance,
                };
                if holds {
                    sink.push(
                        DiagnosticCode::EmptyRow,
                        format!("Constraint '{name}' has no terms and always holds"),
                        Some("It restricts nothing, which usually means the generator dropped its terms."),
                        Some(DiagnosticSubject::constraint(name)),
                        Vec::new(),
                    );
                } else {
                    sink.push(
                        DiagnosticCode::EmptyRowInfeasible,
                        format!(
                            "Constraint '{name}' has no terms, so it reads 0 {} {} and can never hold",
                            op_symbol(operator),
                            fmt_num(rhs)
                        ),
                        Some("The model is infeasible. The terms of this row were probably lost."),
                        Some(DiagnosticSubject::constraint(name)),
                        Vec::new(),
                    );
                }
            }
            continue;
        }
        if terms.is_empty() || !rhs.is_finite() {
            continue;
        }

        if is_standard && class == ConstraintClass::Normal && operator == ComparisonOp::EQ {
            equality_rows += 1;
            equality_vars.extend(terms.iter().map(|(v, _)| *v));
        }

        // 23/24. feasibility against the variable bounds
        let mut redundant = false;
        if is_standard && let Some(act) = activity(ctx, &domains, &terms, rhs) {
            let too_high = act.lower > rhs + act.tau;
            let too_low = act.upper < rhs - act.tau;
            let infeasible = match operator {
                ComparisonOp::LTE | ComparisonOp::LT => too_high,
                ComparisonOp::GTE | ComparisonOp::GT => too_low,
                ComparisonOp::EQ => too_high || too_low,
            };
            if infeasible {
                let related: Vec<RelatedSubject> = terms
                    .iter()
                    .take(LIST_LIMIT)
                    .map(|(var, _)| {
                        let (l, u) = domains[var.index()].unwrap_or((f64::NEG_INFINITY, f64::INFINITY));
                        RelatedSubject::new(
                            DiagnosticSubject::variable(ctx.name(*var), SubjectSite::Bound),
                            format!("ranges over [{}, {}]", fmt_num(l), fmt_num(u)),
                        )
                    })
                    .collect();
                sink.push(
                    DiagnosticCode::RowInfeasibleByBounds,
                    format!(
                        "Constraint '{name}' cannot hold: its terms range over [{}, {}] given the variable bounds, not {} {}",
                        fmt_num(act.lower),
                        fmt_num(act.upper),
                        op_symbol(operator),
                        fmt_num(rhs)
                    ),
                    Some("The model is infeasible. Relax the bounds of the listed variables or the constraint."),
                    Some(DiagnosticSubject::constraint(name)),
                    related,
                );
            } else if class == ConstraintClass::Normal {
                let slack = config.equality_tolerance * rhs.abs().max(1.0);
                redundant = match operator {
                    ComparisonOp::LTE | ComparisonOp::LT => act.upper <= rhs + slack,
                    ComparisonOp::GTE | ComparisonOp::GT => act.lower >= rhs - slack,
                    ComparisonOp::EQ => (act.lower - rhs).abs() <= act.tau && (act.upper - rhs).abs() <= act.tau,
                };
                if redundant {
                    suppressed_singletons.insert(*id);
                    redundant_rows.push((*id, constraint.byte_offset(), operator, rhs));
                }
            }
        }

        if class != ConstraintClass::Normal {
            continue;
        }

        // 22. singleton-row (reported after clustering, see below)
        if is_standard && terms.len() == 1 && !redundant {
            singletons.push((*id, terms[0].0, terms[0].1, operator, rhs));
        }

        // Normalised row for clustering.
        let byte_offset = constraint.byte_offset();
        let Some(norm) = normalise(&terms) else {
            continue;
        };
        let pivot = norm.pivot;
        let flipped = pivot < 0.0;
        let bound = rhs / pivot;
        let direction = match (operator, flipped) {
            (ComparisonOp::EQ, _) => ComparisonOp::EQ,
            (ComparisonOp::LTE | ComparisonOp::LT, false) | (ComparisonOp::GTE | ComparisonOp::GT, true) => ComparisonOp::LTE,
            (ComparisonOp::GTE | ComparisonOp::GT, false) | (ComparisonOp::LTE | ComparisonOp::LT, true) => ComparisonOp::GTE,
        };
        let (lower, upper) = match direction {
            ComparisonOp::LTE => (f64::NEG_INFINITY, bound),
            ComparisonOp::GTE => (bound, f64::INFINITY),
            _ => (bound, bound),
        };
        rows.push(Row { id: *id, context: indicator, norm, lower, upper, offset: byte_offset });
    }

    // 24. row-redundant-by-bounds. A ranged constraint is reported as a whole
    // only when both halves are redundant; one redundant half still leaves the
    // statement binding on the other side.
    let mut redundant_per_statement: FxHashMap<usize, usize> = FxHashMap::default();
    for (_, offset, _, _) in &redundant_rows {
        if let Some(offset) = offset {
            *redundant_per_statement.entry(*offset).or_insert(0) += 1;
        }
    }
    let mut whole_reported: FxHashSet<usize> = FxHashSet::default();
    for (id, offset, operator, rhs) in &redundant_rows {
        let statement = offset.and_then(|o| statement_rows.get(&o).map(|(count, first)| (o, *count, *first)));
        match statement {
            Some((offset, count, first)) if count > 1 => {
                let statement_name = ctx.name(first);
                if redundant_per_statement.get(&offset) == Some(&count) {
                    if whole_reported.insert(offset) {
                        sink.push(
                            DiagnosticCode::RowRedundantByBounds,
                            format!("Constraint '{statement_name}' always holds given the variable bounds"),
                            Some("The bounds on its variables already guarantee both sides; it can be removed."),
                            Some(DiagnosticSubject::constraint(statement_name)),
                            Vec::new(),
                        );
                    }
                } else {
                    let side = match operator {
                        ComparisonOp::LTE | ComparisonOp::LT => "upper",
                        ComparisonOp::GTE | ComparisonOp::GT => "lower",
                        ComparisonOp::EQ => "equality",
                    };
                    sink.push(
                        DiagnosticCode::RowRedundantByBounds,
                        format!(
                            "The {side} side ({} {}) of ranged constraint '{statement_name}' always holds given the variable bounds",
                            op_symbol(*operator),
                            fmt_num(*rhs)
                        ),
                        Some(
                            "Only this side is implied by the bounds. The constraint as written still binds on its other side, so keep it.",
                        ),
                        Some(DiagnosticSubject::constraint(statement_name)),
                        Vec::new(),
                    );
                }
            }
            _ => {
                let name = ctx.name(*id);
                sink.push(
                    DiagnosticCode::RowRedundantByBounds,
                    format!("Constraint '{name}' always holds given the variable bounds"),
                    Some("The bounds on its variables already guarantee it; it can be removed."),
                    Some(DiagnosticSubject::constraint(name)),
                    Vec::new(),
                );
            }
        }
    }

    // Clustering: 25-29.
    let tolerance = config.equality_tolerance;
    let items: Vec<(Context, &NormVec)> = rows.iter().map(|r| (r.context, &r.norm)).collect();
    let mut duplicated: FxHashSet<NameId> = FxHashSet::default();
    // Rows from a ranged statement never take part in rows-form-range.
    let mut ranged: FxHashSet<NameId> = FxHashSet::default();
    {
        let by_name: FxHashMap<&str, NameId> = rows.iter().map(|r| (ctx.name(r.id), r.id)).collect();
        for row in &rows {
            if let Some(offset) = row.offset
                && statement_rows.get(&offset).is_some_and(|(count, _)| *count > 1)
            {
                ranged.insert(row.id);
            }
            let name = ctx.name(row.id);
            if let Some(position) = name.rfind("_rng")
                && let Some(base) = by_name.get(&name[..position])
            {
                ranged.insert(row.id);
                ranged.insert(*base);
            }
        }
    }
    // Row-pair claims are proven, so rows cluster only on bit-identical
    // normalised coefficients: near-equal vectors can still be separated by a
    // feasible point, and a tolerance would make the intersection unsound.
    // A half of a ranged statement says nothing about the whole statement, so
    // whole-row redundancy findings are never reported against it.
    let is_half = |row: &Row| ranged.contains(&row.id);
    for group in cluster(&items, 0.0) {
        let representative = &rows[group[0]];

        // Per direction (lower, upper, equality) keep the tightest row so far
        // and classify each newcomer against it.
        let mut tight: [Option<usize>; 3] = [None; 3];
        for &member in &group {
            let row = &rows[member];
            let direction = row.direction();
            let name = ctx.name(row.id);
            let mut dominated = false;
            if let Some(held) = tight[direction] {
                let held_row = &rows[held];
                let held_name = ctx.name(held_row.id);
                let related = |note: &str| vec![RelatedSubject::new(DiagnosticSubject::constraint(held_name), note)];
                if row.same_source(held_row, ctx) {
                    // Halves of one statement are never compared.
                } else if direction == 2 && !(near(row.lower, held_row.lower, tolerance) && near(row.upper, held_row.upper, tolerance)) {
                    // Equalities at different values contradict each other; that
                    // is reported below, not as dominance.
                } else if near(row.lower, held_row.lower, tolerance) && near(row.upper, held_row.upper, tolerance) {
                    let factor = row.norm.pivot / held_row.norm.pivot;
                    duplicated.insert(row.id);
                    dominated = true;
                    if is_half(row) && !is_half(held_row) {
                        // Only one side of a ranged statement matches a plain row.
                    } else if near(factor, 1.0, tolerance) {
                        sink.push(
                            DiagnosticCode::DuplicateRow,
                            format!("Constraint '{name}' duplicates constraint '{held_name}'"),
                            Some("The two rows are identical; remove one."),
                            Some(DiagnosticSubject::constraint(name)),
                            related("the row it duplicates"),
                        );
                    } else {
                        sink.push(
                            DiagnosticCode::ProportionalRow,
                            format!("Constraint '{name}' is '{held_name}' scaled by {}", fmt_num(factor)),
                            Some("The two rows describe the same restriction; remove one."),
                            Some(DiagnosticSubject::constraint(name)),
                            related("the row it is proportional to"),
                        );
                    }
                } else if at_most(row.lower, held_row.lower, tolerance) && at_most(held_row.upper, row.upper, tolerance) {
                    dominated = true;
                    if !is_half(row) {
                        sink.push(
                            DiagnosticCode::DominatedRow,
                            format!("Constraint '{name}' is implied by constraint '{held_name}' and is redundant"),
                            Some("The other row is at least as tight on the same terms; remove this one."),
                            Some(DiagnosticSubject::constraint(name)),
                            related("the tighter row"),
                        );
                    }
                } else {
                    if !is_half(held_row) {
                        sink.push(
                            DiagnosticCode::DominatedRow,
                            format!("Constraint '{held_name}' is implied by constraint '{name}' and is redundant"),
                            Some("The other row is at least as tight on the same terms; remove this one."),
                            Some(DiagnosticSubject::constraint(held_name)),
                            vec![RelatedSubject::new(DiagnosticSubject::constraint(name), "the tighter row")],
                        );
                    }
                    tight[direction] = Some(member);
                }
            } else {
                tight[direction] = Some(member);
            }

            // An equality implies, or is implied by, a one-sided row.
            if dominated {
                continue;
            }
            for other in 0..3 {
                let Some(held) = tight[other].filter(|held| other != direction && *held != member) else {
                    continue;
                };
                let held_row = &rows[held];
                if row.same_source(held_row, ctx) {
                    continue;
                }
                let held_name = ctx.name(held_row.id);
                if at_most(row.lower, held_row.lower, tolerance) && at_most(held_row.upper, row.upper, tolerance) {
                    if !is_half(row) {
                        sink.push(
                            DiagnosticCode::DominatedRow,
                            format!("Constraint '{name}' is implied by constraint '{held_name}' and is redundant"),
                            Some("The other row is at least as tight on the same terms; remove this one."),
                            Some(DiagnosticSubject::constraint(name)),
                            vec![RelatedSubject::new(DiagnosticSubject::constraint(held_name), "the tighter row")],
                        );
                    }
                    if tight[direction] == Some(member) {
                        tight[direction] = None;
                    }
                    break;
                } else if at_most(held_row.lower, row.lower, tolerance) && at_most(row.upper, held_row.upper, tolerance) {
                    if !is_half(held_row) {
                        sink.push(
                            DiagnosticCode::DominatedRow,
                            format!("Constraint '{held_name}' is implied by constraint '{name}' and is redundant"),
                            Some("The other row is at least as tight on the same terms; remove this one."),
                            Some(DiagnosticSubject::constraint(held_name)),
                            vec![RelatedSubject::new(DiagnosticSubject::constraint(name), "the tighter row")],
                        );
                    }
                    tight[other] = None;
                }
            }
        }

        // 27. rows-form-range: at most one per group, from the tightest pair of
        // user-written rows.
        if representative.context.is_none()
            && let (Some(lo), Some(hi)) = (tight[0], tight[1])
        {
            let (lo_row, hi_row) = (&rows[lo], &rows[hi]);
            if !ranged.contains(&lo_row.id) && !ranged.contains(&hi_row.id) && at_most(lo_row.lower, hi_row.upper, tolerance) {
                let (first, second) = if lo < hi { (lo_row, hi_row) } else { (hi_row, lo_row) };
                let (first_name, second_name) = (ctx.name(first.id), ctx.name(second.id));
                sink.push(
                    DiagnosticCode::RowsFormRange,
                    format!(
                        "Constraints '{first_name}' and '{second_name}' together bound the same terms between {} and {}",
                        fmt_num(lo_row.lower),
                        fmt_num(hi_row.upper)
                    ),
                    Some("Write them as one ranged constraint, `lo <= expression <= hi`."),
                    Some(DiagnosticSubject::constraint(second_name)),
                    vec![RelatedSubject::new(DiagnosticSubject::constraint(first_name), "the other half of the range")],
                );
            }
        }

        // 28. contradictory-rows
        if representative.context.is_some() {
            continue;
        }
        let tau = config.feasibility_tolerance;
        let (mut low, mut high) = (f64::NEG_INFINITY, f64::INFINITY);
        let (mut low_source, mut high_source): (Option<&Row>, Option<&Row>) = (None, None);
        for &member in &group {
            let row = &rows[member];
            let mut culprit: Option<&Row> = None;
            if row.lower > low {
                low = row.lower;
                low_source = Some(row);
                if low - high > tau * low.abs().max(high.abs()).max(1.0) {
                    culprit = high_source;
                }
            }
            if row.upper < high {
                high = row.upper;
                high_source = Some(row);
                if culprit.is_none() && low - high > tau * low.abs().max(high.abs()).max(1.0) {
                    culprit = low_source;
                }
            }
            // Two halves of one statement conflicting is the statement's own
            // emptiness, which other checks report.
            if let Some(other) = culprit.filter(|other| !other.same_source(row, ctx)) {
                let name = ctx.name(row.id);
                let other_name = ctx.name(other.id);
                sink.push(
                    DiagnosticCode::ContradictoryRows,
                    format!(
                        "Constraints '{name}' and '{other_name}' restrict the same terms to disjoint ranges, so no point satisfies both"
                    ),
                    Some("The model is infeasible. Correct the right-hand side or operator of one of the rows."),
                    Some(DiagnosticSubject::constraint(name)),
                    vec![RelatedSubject::new(DiagnosticSubject::constraint(other_name), "supplies the conflicting bound")],
                );
                break;
            }
        }
    }

    // 22. singleton-row
    for (id, variable, coefficient, operator, rhs) in singletons {
        if suppressed_singletons.contains(&id) || duplicated.contains(&id) {
            continue;
        }
        let operator = if coefficient < 0.0 {
            match operator {
                ComparisonOp::LTE => ComparisonOp::GTE,
                ComparisonOp::LT => ComparisonOp::GT,
                ComparisonOp::GTE => ComparisonOp::LTE,
                ComparisonOp::GT => ComparisonOp::LT,
                ComparisonOp::EQ => ComparisonOp::EQ,
            }
        } else {
            operator
        };
        let (name, variable_name) = (ctx.name(id), ctx.name(variable));
        sink.push(
            DiagnosticCode::SingletonRow,
            format!(
                "Constraint '{name}' has one term and is really the bound {variable_name} {} {}",
                op_symbol(operator),
                fmt_num(rhs / coefficient)
            ),
            Some("Declare it in the Bounds section so solvers treat it as a bound rather than a row."),
            Some(DiagnosticSubject::constraint(name).with_variable(variable_name)),
            Vec::new(),
        );
    }

    // 30. equalities-exceed-variables
    if equality_rows > equality_vars.len() {
        sink.push(
            DiagnosticCode::EqualitiesExceedVariables,
            format!("{equality_rows} equality constraints reference only {} variables", equality_vars.len()),
            Some("Unless the equalities are dependent the model is infeasible. Check for redundant or conflicting equalities."),
            None,
            Vec::new(),
        );
    }

    // Objectives: coefficient-level scaling and range.
    for (id, objective) in &problem.objectives {
        let name = ctx.name(*id);
        let mut scale = Scale::new();
        for term in &objective.coefficients {
            scale.update(term.value);
            all_scale.update(term.value);
            let magnitude = term.value.abs();
            if magnitude == 0.0 || !magnitude.is_finite() {
                continue;
            }
            let (code, label) = if magnitude > thresholds.large_coefficient_threshold {
                (DiagnosticCode::LargeCoefficient, "large")
            } else if magnitude < thresholds.small_coefficient_threshold {
                (DiagnosticCode::SmallCoefficient, "small")
            } else {
                continue;
            };
            let variable = ctx.name(term.name);
            sink.push(
                code,
                format!("Coefficient {} of '{variable}' in objective '{name}' is {label}", fmt_num(term.value)),
                Some("Very large or tiny coefficients lose precision. Rescale the variable or the objective."),
                Some(DiagnosticSubject::objective(name).with_variable(variable)),
                Vec::new(),
            );
        }
        if let Some(ratio) = scale.ratio().filter(|r| *r > thresholds.coefficient_ratio_threshold) {
            sink.push(
                DiagnosticCode::ObjectiveCoefficientRange,
                format!("Objective '{name}' has coefficients spanning about {} orders of magnitude", orders_of_magnitude(ratio)),
                Some("Rescale the variables or the objective so its coefficients are of similar size."),
                Some(DiagnosticSubject::objective(name)),
                Vec::new(),
            );
        }
    }

    // 35. column-coefficient-range
    for ((id, _), column) in problem.variables.iter().zip(&columns) {
        if let Some(ratio) = column.ratio().filter(|r| *r > thresholds.coefficient_ratio_threshold) {
            let name = ctx.name(*id);
            sink.push(
                DiagnosticCode::ColumnCoefficientRange,
                format!(
                    "Variable '{name}' has coefficients spanning about {} orders of magnitude across the constraints",
                    orders_of_magnitude(ratio)
                ),
                Some("Rescale the variable so its coefficients are of similar size."),
                Some(DiagnosticSubject::variable(name, SubjectSite::Definition)),
                Vec::new(),
            );
        }
    }

    // 37. coefficient-range
    if let Some(ratio) = all_scale.ratio().filter(|r| *r > thresholds.coefficient_ratio_threshold) {
        sink.push(
            DiagnosticCode::CoefficientRange,
            format!("Coefficients across the model span about {} orders of magnitude", orders_of_magnitude(ratio)),
            Some("Rescale the model; the per-row, per-column and per-objective diagnostics show where."),
            None,
            Vec::new(),
        );
    }
}
