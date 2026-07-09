//! Assembly of flat objective/constraint element lists into raw model types.
//!
//! The LALRPOP grammar cannot decide with one token of lookahead whether
//! `... + 10 obj2: y` ends an objective with the constant `10` or scales a
//! variable named `obj2`, so the grammar collects each section body as a flat
//! list of [`Elem`](crate::assemble::Elem)s and the functions here assemble entries with unbounded
//! lookahead. This is also what enables spec features the old grammar
//! rejected: constant terms (`obj: x + 10`, `c1: x + 2 <= 10`), empty
//! objectives, flipped constraints (`10 >= x`), and ranged constraints
//! (`2 <= x + y <= 10`, expanded into two constraints like MPS RANGES).

use std::borrow::Cow;

use crate::lexer::{LexerError, RawCoefficient, RawConstraint, RawObjective};
use crate::model::ComparisonOp;

/// One element of an objective or constraint body, with its byte offset.
pub type SpannedElem<'input> = (usize, Elem<'input>);

/// One element of an objective or constraint body.
#[derive(Debug, Clone, PartialEq)]
pub enum Elem<'input> {
    /// `name:` / `name::` — starts a named entry.
    Name(&'input str),
    /// A bare identifier (variable reference).
    Var(&'input str),
    /// A numeric literal (number or infinity).
    Num(f64),
    /// `+`
    Plus,
    /// `-`
    Minus,
    /// A comparison operator (constraint bodies only).
    Op(ComparisonOp),
}

fn err(position: usize, message: impl Into<String>) -> LexerError {
    LexerError { position, message: Some(message.into()) }
}

/// Position to report for an error at element index `i` (end of input falls
/// back to the last element's position).
fn pos_at(elems: &[SpannedElem<'_>], i: usize) -> usize {
    elems.get(i).or_else(|| elems.last()).map_or(0, |&(loc, _)| loc)
}

/// A parsed run of terms: variable coefficients plus a folded constant.
#[derive(Debug, Default)]
struct Segment<'input> {
    coefficients: Vec<RawCoefficient<'input>>,
    constant: f64,
}

/// Parse a term sequence starting at `i`: `[+|-] term ([+|-] term)*` where a
/// term is `Num Var` (coefficient), `Var` (unit coefficient), or `Num`
/// (constant). Stops without error at an `Op`, a `Name`, end of input, or an
/// unsigned term following a complete one (the start of the next entry).
/// Returns the segment and the index of the first unconsumed element.
fn parse_segment<'input>(elems: &[SpannedElem<'input>], mut i: usize) -> Result<(Segment<'input>, usize), LexerError> {
    let mut segment = Segment::default();
    let mut first = true;

    loop {
        let sign = match elems.get(i) {
            Some((_, Elem::Plus)) => {
                i += 1;
                1.0
            }
            Some((_, Elem::Minus)) => {
                i += 1;
                -1.0
            }
            Some((_, Elem::Var(_) | Elem::Num(_))) if first => 1.0,
            // Op / Name / end of input / unsigned term after a complete one:
            // the segment is finished.
            _ => return Ok((segment, i)),
        };

        match elems.get(i) {
            Some(&(_, Elem::Var(name))) => {
                segment.coefficients.push(RawCoefficient { name, value: sign });
                i += 1;
            }
            Some(&(_, Elem::Num(value))) => match elems.get(i + 1) {
                Some(&(_, Elem::Var(name))) => {
                    segment.coefficients.push(RawCoefficient { name, value: sign * value });
                    i += 2;
                }
                Some((loc, Elem::Num(_))) => {
                    return Err(err(*loc, "adjacent numeric literals; expected '+', '-', or a variable name"));
                }
                _ => {
                    segment.constant += sign * value;
                    i += 1;
                }
            },
            Some((loc, Elem::Plus | Elem::Minus)) => return Err(err(*loc, "consecutive signs; expected a number or variable")),
            _ => return Err(err(pos_at(elems, i), "dangling sign; expected a number or variable")),
        }

        first = false;
    }
}

/// Parse a single signed numeric value (`[+|-] Num`) at `i`.
fn parse_signed_number(elems: &[SpannedElem<'_>], mut i: usize, context: &str) -> Result<(f64, usize), LexerError> {
    let sign = match elems.get(i) {
        Some((_, Elem::Plus)) => {
            i += 1;
            1.0
        }
        Some((_, Elem::Minus)) => {
            i += 1;
            -1.0
        }
        _ => 1.0,
    };
    match elems.get(i) {
        Some(&(_, Elem::Num(value))) => Ok((sign * value, i + 1)),
        _ => Err(err(pos_at(elems, i), format!("{context} must be a numeric value"))),
    }
}

/// Assemble the objective section body into raw objectives.
///
/// An empty body yields no objectives (CPLEX permits an empty objective
/// function). A `Name` element starts a new objective; terms before the first
/// name form an unnamed objective.
///
/// # Errors
///
/// Returns an error for malformed term sequences (consecutive signs, adjacent
/// numeric literals, a dangling sign, or unsigned adjacent terms).
pub fn assemble_objectives<'input>(elems: &[SpannedElem<'input>]) -> Result<Vec<RawObjective<'input>>, LexerError> {
    let mut objectives: Vec<RawObjective<'input>> = Vec::new();
    let mut current: Option<RawObjective<'input>> = None;
    let mut i = 0;

    while i < elems.len() {
        let (loc, ref elem) = elems[i];
        if let Elem::Name(name) = elem {
            if let Some(obj) = current.take() {
                objectives.push(obj);
            }
            current = Some(RawObjective { name: Cow::Borrowed(name), coefficients: Vec::new(), constant: 0.0, byte_offset: Some(loc) });
            i += 1;
        } else {
            let obj = current.get_or_insert_with(|| RawObjective {
                name: Cow::Borrowed("__obj__"),
                coefficients: Vec::new(),
                constant: 0.0,
                byte_offset: Some(loc),
            });
            let (segment, next) = parse_segment(elems, i)?;
            debug_assert!(next > i, "parse_segment must consume at least one element here");
            obj.coefficients.extend(segment.coefficients);
            obj.constant += segment.constant;
            if next < elems.len() && !matches!(elems[next].1, Elem::Name(_)) {
                return Err(err(pos_at(elems, next), "expected '+', '-', or a new objective in the objective section"));
            }
            i = next;
        }
    }

    if let Some(obj) = current.take() {
        objectives.push(obj);
    }
    Ok(objectives)
}

/// Flip a comparison operator for moving it to the other side of a relation.
const fn flip(op: ComparisonOp) -> ComparisonOp {
    match op {
        ComparisonOp::LT => ComparisonOp::GT,
        ComparisonOp::LTE => ComparisonOp::GTE,
        ComparisonOp::GT => ComparisonOp::LT,
        ComparisonOp::GTE => ComparisonOp::LTE,
        ComparisonOp::EQ => ComparisonOp::EQ,
    }
}

/// Assemble the constraint section body into raw constraints.
///
/// Supported entry shapes (each optionally preceded by `name:`):
/// - `expr op number` — standard; constants in `expr` fold into the RHS
/// - `number op expr` — flipped; normalised by reversing the operator
/// - `number op expr op number` — ranged; expanded into two constraints
///   (`name` and `name_rng`), matching the MPS RANGES expansion
///
/// # Errors
///
/// Returns an error for malformed term sequences, a missing comparison
/// operator, or a non-numeric right-hand side / range bound.
pub fn assemble_constraints<'input>(elems: &[SpannedElem<'input>]) -> Result<Vec<RawConstraint<'input>>, LexerError> {
    let mut constraints = Vec::new();
    let mut i = 0;

    while i < elems.len() {
        let entry_loc = elems[i].0;

        let name: Option<&'input str> = if let Elem::Name(n) = elems[i].1 {
            i += 1;
            Some(n)
        } else {
            None
        };

        let (lhs, next) = parse_segment(elems, i)?;
        if next == i {
            // parse_segment consumed nothing: the entry starts with something
            // that cannot begin an expression (e.g. a stray operator).
            return Err(err(pos_at(elems, i), "expected an expression before the comparison operator"));
        }
        i = next;

        let Some(&(_, Elem::Op(op1))) = elems.get(i) else {
            return Err(err(pos_at(elems, i), "expected a comparison operator in constraint"));
        };
        i += 1;

        if lhs.coefficients.is_empty() {
            // Numeric-only LHS: flipped (`10 >= x + y`) or ranged
            // (`2 <= x + y <= 10`) constraint.
            let (mid, next) = parse_segment(elems, i)?;
            if next == i {
                return Err(err(pos_at(elems, i), "expected an expression after the comparison operator"));
            }
            i = next;

            if let Some(&(_, Elem::Op(op2))) = elems.get(i) {
                // Ranged: lhs.constant op1 mid op2 rhs
                i += 1;
                let (rhs, next) = parse_signed_number(elems, i, "range bound")?;
                i = next;

                let lower_name: Cow<'input, str> = name.map_or(Cow::Borrowed("__c__"), Cow::Borrowed);
                let upper_name: Cow<'input, str> = name.map_or(Cow::Borrowed("__c__"), |n| Cow::Owned(format!("{n}_rng")));
                constraints.push(RawConstraint::Standard {
                    name: lower_name,
                    coefficients: mid.coefficients.clone(),
                    operator: flip(op1),
                    rhs: lhs.constant - mid.constant,
                    byte_offset: Some(entry_loc),
                });
                constraints.push(RawConstraint::Standard {
                    name: upper_name,
                    coefficients: mid.coefficients,
                    operator: op2,
                    rhs: rhs - mid.constant,
                    byte_offset: Some(entry_loc),
                });
            } else {
                // Flipped: normalise so the variables sit on the left.
                constraints.push(RawConstraint::Standard {
                    name: name.map_or(Cow::Borrowed("__c__"), Cow::Borrowed),
                    coefficients: mid.coefficients,
                    operator: flip(op1),
                    rhs: lhs.constant - mid.constant,
                    byte_offset: Some(entry_loc),
                });
            }
        } else {
            // Standard: RHS is a single signed number; LHS constants fold in.
            let (rhs, next) = parse_signed_number(elems, i, "constraint right-hand side")?;
            i = next;
            constraints.push(RawConstraint::Standard {
                name: name.map_or(Cow::Borrowed("__c__"), Cow::Borrowed),
                coefficients: lhs.coefficients,
                operator: op1,
                rhs: rhs - lhs.constant,
                byte_offset: Some(entry_loc),
            });
        }
    }

    Ok(constraints)
}
