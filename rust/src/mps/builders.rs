use std::borrow::Cow;
use std::collections::{HashMap, HashSet};

use super::{BoundAccumulator, RowType};
use crate::lexer::{RawCoefficient, RawConstraint, RawObjective};
use crate::model::{ComparisonOp, VariableType};

/// Build objective(s) from the parsed MPS data.
///
/// Produces one `RawObjective` per N-row, supporting multi-objective MPS files.
pub(super) fn build_objectives<'input>(
    objective_rows: &[&'input str],
    coefficients: &HashMap<(&'input str, &'input str), f64>,
    column_order: &[&'input str],
) -> Vec<RawObjective<'input>> {
    debug_assert!(objective_rows.iter().all(|r| !r.is_empty()), "objective_rows must not contain empty row names");

    if objective_rows.is_empty() {
        return vec![RawObjective { name: Cow::Borrowed("__obj__"), coefficients: Vec::new(), byte_offset: None }];
    }

    let mut objectives = Vec::with_capacity(objective_rows.len());
    for &obj_row in objective_rows {
        let mut objective_coefficients = Vec::new();
        for &var_name in column_order {
            if let Some(&value) = coefficients.get(&(var_name, obj_row)) {
                objective_coefficients.push(RawCoefficient { name: var_name, value });
            }
        }
        objectives.push(RawObjective { name: Cow::Borrowed(obj_row), coefficients: objective_coefficients, byte_offset: None });
    }

    debug_assert!(objectives.len() == objective_rows.len(), "should produce one objective per N-row");
    objectives
}

/// Build constraints from the parsed MPS data, including RANGES expansion.
///
/// For rows with a RANGES entry, the single constraint is expanded into two
/// constraints to represent both bounds:
/// - **G row**: original `>= rhs`, plus `<= rhs + |range|`
/// - **L row**: original `<= rhs`, plus `>= rhs - |range|`
/// - **E row, positive range**: `>= rhs` and `<= rhs + range`
/// - **E row, negative range**: `<= rhs` and `>= rhs + range`
pub(super) fn build_constraints<'input>(
    row_types: &HashMap<&'input str, RowType>,
    row_order: &[&'input str],
    coefficients: &HashMap<(&'input str, &'input str), f64>,
    column_order: &[&'input str],
    rhs_values: &HashMap<&'input str, f64>,
    range_values: &HashMap<&'input str, f64>,
) -> Vec<RawConstraint<'input>> {
    debug_assert!(row_order.iter().all(|r| row_types.contains_key(r)), "every row in row_order must have a type in row_types");

    let mut constraints = Vec::with_capacity(row_order.len());

    for &row_name in row_order {
        let row_type = row_types.get(row_name).copied().expect("row_order entries must exist in row_types (validated by debug_assert)");
        debug_assert!(row_type != RowType::N, "N-type rows should not appear in row_order");

        let operator = match row_type {
            RowType::L => ComparisonOp::LTE,
            RowType::G => ComparisonOp::GTE,
            RowType::E => ComparisonOp::EQ,
            RowType::N => unreachable!("N-type rows filtered above"),
        };

        let mut row_coeffs = Vec::new();
        for &var_name in column_order {
            if let Some(&value) = coefficients.get(&(var_name, row_name)) {
                row_coeffs.push(RawCoefficient { name: var_name, value });
            }
        }

        let rhs = rhs_values.get(row_name).copied().unwrap_or(0.0);

        // Check for RANGES entry on this row
        if let Some(&range_val) = range_values.get(row_name) {
            // Expand into two constraints based on row type and range value
            let (lower_rhs, upper_rhs) = match row_type {
                RowType::G => (rhs, rhs + range_val.abs()),
                RowType::L => (rhs - range_val.abs(), rhs),
                RowType::E => {
                    if range_val >= 0.0 {
                        (rhs, rhs + range_val)
                    } else {
                        (rhs + range_val, rhs)
                    }
                }
                RowType::N => unreachable!("N-type rows filtered above"),
            };

            // Emit the lower-bound constraint (GTE)
            constraints.push(RawConstraint::Standard {
                name: Cow::Borrowed(row_name),
                coefficients: row_coeffs.clone(),
                operator: ComparisonOp::GTE,
                rhs: lower_rhs,
                byte_offset: None,
            });

            // Emit the upper-bound constraint (LTE)
            constraints.push(RawConstraint::Standard {
                name: Cow::Owned(format!("{row_name}_rng")),
                coefficients: row_coeffs,
                operator: ComparisonOp::LTE,
                rhs: upper_rhs,
                byte_offset: None,
            });
        } else {
            constraints.push(RawConstraint::Standard {
                name: Cow::Borrowed(row_name),
                coefficients: row_coeffs,
                operator,
                rhs,
                byte_offset: None,
            });
        }
    }

    debug_assert!(constraints.len() >= row_order.len(), "constraints cannot be fewer than rows (ranges add extra)");
    constraints
}

/// Build bounds from accumulated bound data.
///
/// Applies MPS default bounds: variables without explicit BOUNDS entries get
/// `[0, +inf]`. Integer variables (INTORG/INTEND) without explicit bounds get
/// `[0, 1]`. When an UP bound is negative with no explicit LO, the lower
/// bound is set to `-inf` per CPLEX spec.
pub(super) fn build_bounds<'input>(
    bound_accumulators: &HashMap<&'input str, BoundAccumulator>,
    bound_order: &[&'input str],
    column_order: &[&'input str],
    integer_vars: &HashSet<&'input str>,
) -> Vec<(&'input str, VariableType)> {
    debug_assert!(bound_order.iter().all(|v| bound_accumulators.contains_key(v)), "every variable in bound_order must have an accumulator");

    let mut bounds = Vec::with_capacity(bound_order.len() + column_order.len());

    // First, emit bounds for variables with explicit BOUNDS entries
    let mut has_explicit_bounds: HashSet<&str> = HashSet::with_capacity(bound_order.len());

    for &var_name in bound_order {
        has_explicit_bounds.insert(var_name);

        let Some(accumulator) = bound_accumulators.get(var_name) else {
            continue;
        };

        let var_type = if accumulator.binary {
            VariableType::Binary
        } else if accumulator.free {
            VariableType::Free
        } else if let Some(fixed) = accumulator.fixed {
            VariableType::DoubleBound(fixed, fixed)
        } else {
            match (accumulator.lower, accumulator.upper) {
                (Some(lo), Some(hi)) => VariableType::DoubleBound(lo, hi),
                (Some(lo), None) => VariableType::LowerBound(lo),
                (None, Some(hi)) => {
                    // CPLEX spec: UP < 0 with no LO implies lower = -inf
                    if hi < 0.0 { VariableType::DoubleBound(f64::NEG_INFINITY, hi) } else { VariableType::UpperBound(hi) }
                }
                (None, None) => continue, // No bounds to emit
            }
        };

        bounds.push((var_name, var_type));
    }

    // Apply MPS default bounds for variables without explicit BOUNDS entries
    for &var_name in column_order {
        if has_explicit_bounds.contains(var_name) {
            continue;
        }

        if integer_vars.contains(var_name) {
            // Integer variables default to [0, 1]
            bounds.push((var_name, VariableType::DoubleBound(0.0, 1.0)));
        } else {
            // Continuous variables default to [0, +inf]
            bounds.push((var_name, VariableType::LowerBound(0.0)));
        }
    }

    debug_assert!(
        !bounds.is_empty() || (bound_order.is_empty() && column_order.is_empty()),
        "bounds should be non-empty when there are variables"
    );
    bounds
}
