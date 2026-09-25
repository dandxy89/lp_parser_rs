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
//!
//! Quadratic terms are written in a bracketed block, `[ x ^ 2 + 4 x * y ]`.
//! In an objective the block must be followed by `/ 2` (CPLEX, Gurobi) and its
//! coefficients are halved on the way in; a constraint's block is not divided.

use std::borrow::Cow;
use std::collections::HashSet;

use crate::lexer::{LexerError, RawCoefficient, RawConstraint, RawObjective, RawQuadraticTerm, SosEntryKind};
use crate::model::{ComparisonOp, GeneralFunction, ObjectiveAttributes, SOSType};

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
    /// `->`, the implication arrow of an indicator constraint.
    Implies,
    /// `[`, opening a quadratic block.
    LBracket,
    /// `]`, closing a quadratic block.
    RBracket,
    /// `^`, the exponent of a squared term.
    Caret,
    /// `*`, the product of two variables.
    Star,
    /// `/`, the division of an objective's quadratic block.
    Slash,
}

/// Where a quadratic block appears, which decides whether it is divided.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum QuadraticContext {
    /// An objective: the block must be written `[ ... ] / 2`.
    Objective,
    /// A constraint: the block is not divided.
    Constraint,
}

fn err(position: usize, message: impl Into<String>) -> LexerError {
    LexerError { position, message: Some(message.into()) }
}

/// Position to report for an error at element index `i` (end of input falls
/// back to the last element's position).
fn pos_at(elems: &[SpannedElem<'_>], i: usize) -> usize {
    elems.get(i).or_else(|| elems.last()).map_or(0, |&(loc, _)| loc)
}

/// A parsed run of terms: variable coefficients, quadratic terms, and a folded
/// constant.
#[derive(Debug, Default)]
struct Segment<'input> {
    coefficients: Vec<RawCoefficient<'input>>,
    quadratic: Vec<RawQuadraticTerm<'input>>,
    constant: f64,
}

impl Segment<'_> {
    /// Whether the segment mentions no variable at all (only constants).
    const fn is_numeric_only(&self) -> bool {
        self.coefficients.is_empty() && self.quadratic.is_empty()
    }
}

/// Parse a term sequence starting at `i`: `[+|-] term ([+|-] term)*` where a
/// term is `Num Var` (coefficient), `Var` (unit coefficient), `Num`
/// (constant), or a quadratic block `[ ... ]` (see [`parse_quadratic_block`]).
/// Stops without error at an `Op`, a `Name`, end of input, or an unsigned
/// term following a complete one (the start of the next entry). Returns the
/// segment and the index of the first unconsumed element.
fn parse_segment<'input>(
    elems: &[SpannedElem<'input>],
    mut i: usize,
    context: QuadraticContext,
) -> Result<(Segment<'input>, usize), LexerError> {
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
            Some((_, Elem::Var(_) | Elem::Num(_) | Elem::LBracket)) if first => 1.0,
            // Op / Name / end of input / unsigned term after a complete one:
            // the segment is finished.
            _ => return Ok((segment, i)),
        };

        match elems.get(i) {
            Some(&(_, Elem::Var(name))) => {
                segment.coefficients.push(RawCoefficient { name, value: sign });
                i += 1;
            }
            Some(&(loc, Elem::Num(value))) => match elems.get(i + 1) {
                Some(&(_, Elem::Var(name))) => {
                    if !value.is_finite() {
                        return Err(err(loc, format!("infinite coefficient for variable '{name}'")));
                    }
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
            Some((_, Elem::LBracket)) => {
                let next = parse_quadratic_block(elems, i, sign, context, &mut segment.quadratic)?;
                debug_assert!(next > i, "a quadratic block consumes at least its brackets");
                i = next;
            }
            Some((loc, Elem::Plus | Elem::Minus)) => return Err(err(*loc, "consecutive signs; expected a number or variable")),
            _ => return Err(err(pos_at(elems, i), "dangling sign; expected a number or variable")),
        }

        first = false;
    }
}

/// Parse a quadratic block starting at the `[` at index `i`, pushing its terms
/// (scaled by `sign`) onto `out` and returning the index after it.
///
/// A term is `[+|-] [Num] Var ^ 2` or `[+|-] [Num] Var * Var`. In an objective
/// the block must be followed by `/ 2`, which halves every coefficient; in a
/// constraint it must not be divided.
///
/// # Errors
///
/// Returns an error for an empty or unterminated block, a malformed term, an
/// exponent other than 2, a non-finite coefficient, or a missing (objective)
/// or unexpected (constraint) `/ 2`.
fn parse_quadratic_block<'input>(
    elems: &[SpannedElem<'input>],
    i: usize,
    sign: f64,
    context: QuadraticContext,
    out: &mut Vec<RawQuadraticTerm<'input>>,
) -> Result<usize, LexerError> {
    debug_assert!(matches!(elems.get(i), Some((_, Elem::LBracket))), "a quadratic block starts at '['");
    let open_loc = elems[i].0;
    let mut j = i + 1;
    let mut terms: Vec<RawQuadraticTerm<'input>> = Vec::new();

    loop {
        let term_sign = match elems.get(j) {
            Some((_, Elem::RBracket)) if !terms.is_empty() => {
                j += 1;
                break;
            }
            Some((loc, Elem::RBracket)) => return Err(err(*loc, "empty quadratic block")),
            Some((_, Elem::Plus)) => {
                j += 1;
                1.0
            }
            Some((_, Elem::Minus)) => {
                j += 1;
                -1.0
            }
            Some((_, Elem::Num(_) | Elem::Var(_))) if terms.is_empty() => 1.0,
            None => return Err(err(open_loc, "unterminated quadratic block; expected ']'")),
            Some((loc, _)) => return Err(err(*loc, "expected '+', '-' or ']' in a quadratic block")),
        };

        let (coefficient, coefficient_loc) = match elems.get(j) {
            Some(&(loc, Elem::Num(value))) => {
                j += 1;
                (value, loc)
            }
            _ => (1.0, pos_at(elems, j)),
        };
        if !coefficient.is_finite() {
            return Err(err(coefficient_loc, "quadratic coefficient must be finite"));
        }

        let Some(&(_, Elem::Var(var1))) = elems.get(j) else {
            return Err(err(pos_at(elems, j), "expected a variable in a quadratic term"));
        };
        j += 1;
        let var2 = match (elems.get(j), elems.get(j + 1)) {
            (Some((_, Elem::Caret)), Some(&(loc, Elem::Num(exponent)))) => {
                // The exponent is a literal and only a square is quadratic.
                #[allow(clippy::float_cmp)]
                if exponent != 2.0 {
                    return Err(err(loc, format!("only squares ('{var1} ^ 2') are quadratic, not '{var1} ^ {exponent}'")));
                }
                var1
            }
            (Some((_, Elem::Star)), Some(&(_, Elem::Var(var2)))) => var2,
            _ => return Err(err(pos_at(elems, j), format!("expected '^ 2' or '* variable' after '{var1}' in a quadratic term"))),
        };
        j += 2;

        terms.push(RawQuadraticTerm { var1, var2, coefficient: sign * term_sign * coefficient });
    }

    match (context, elems.get(j), elems.get(j + 1)) {
        (QuadraticContext::Objective, Some((_, Elem::Slash)), Some(&(_, Elem::Num(divisor)))) => {
            // CPLEX and Gurobi only ever write `/ 2`.
            #[allow(clippy::float_cmp)]
            if divisor != 2.0 {
                return Err(err(pos_at(elems, j + 1), format!("an objective's quadratic block must be divided by 2, not {divisor}")));
            }
            for term in &mut terms {
                term.coefficient /= 2.0;
            }
            j += 2;
        }
        (QuadraticContext::Objective, ..) => {
            return Err(err(pos_at(elems, j), "an objective's quadratic block must be followed by '/ 2'"));
        }
        (QuadraticContext::Constraint, Some((loc, Elem::Slash)), _) => {
            return Err(err(*loc, "a constraint's quadratic block is not divided; remove the '/'"));
        }
        (QuadraticContext::Constraint, ..) => {}
    }

    out.extend(terms);
    Ok(j)
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
///
/// With `multi_objective` (Gurobi's `Minimize multi-objectives`), a name may be
/// followed by attributes, `OBJ0: Priority=2 Weight=1 AbsTol=0 RelTol=0`,
/// before its expression.
pub fn assemble_objectives<'input>(elems: &[SpannedElem<'input>], multi_objective: bool) -> Result<Vec<RawObjective<'input>>, LexerError> {
    let mut objectives: Vec<RawObjective<'input>> = Vec::new();
    let mut current: Option<RawObjective<'input>> = None;
    let mut i = 0;

    while i < elems.len() {
        let (loc, ref elem) = elems[i];
        if let Elem::Name(name) = elem {
            if let Some(obj) = current.take() {
                objectives.push(obj);
            }
            i += 1;
            let (attributes, next) = parse_objective_attributes(elems, i, multi_objective)?;
            i = next;
            current = Some(RawObjective {
                name: Cow::Borrowed(name),
                coefficients: Vec::new(),
                quadratic: Vec::new(),
                attributes,
                constant: 0.0,
                byte_offset: Some(loc),
            });
        } else {
            let obj = current.get_or_insert_with(|| RawObjective {
                name: Cow::Borrowed("__obj__"),
                coefficients: Vec::new(),
                quadratic: Vec::new(),
                attributes: ObjectiveAttributes::default(),
                constant: 0.0,
                byte_offset: Some(loc),
            });
            let (segment, next) = parse_segment(elems, i, QuadraticContext::Objective)?;
            if next == i {
                // Nothing parsed: an element that cannot start a term.
                return Err(err(loc, "expected a term in the objective section"));
            }
            obj.coefficients.extend(segment.coefficients);
            obj.quadratic.extend(segment.quadratic);
            obj.constant += segment.constant;
            if !obj.constant.is_finite() {
                return Err(err(loc, "objective constant must be finite"));
            }
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

/// Parse Gurobi multi-objective attributes (`Priority=2 Weight=1 AbsTol=0
/// RelTol=0`) starting at `i`, stopping at the first element that does not
/// start one. Returns the attributes and the index after them.
///
/// # Errors
///
/// Returns an error for an attribute outside a `multi-objectives` section, an
/// unknown or repeated attribute, a non-numeric or non-finite value, a
/// non-integral priority, or a negative tolerance.
fn parse_objective_attributes(
    elems: &[SpannedElem<'_>],
    mut i: usize,
    multi_objective: bool,
) -> Result<(ObjectiveAttributes, usize), LexerError> {
    let mut attributes = ObjectiveAttributes::default();
    while let (Some(&(loc, Elem::Var(attribute))), Some((_, Elem::Op(ComparisonOp::EQ)))) = (elems.get(i), elems.get(i + 1)) {
        if !multi_objective {
            return Err(err(loc, format!("objective attribute '{attribute}' requires 'multi-objectives' after the sense")));
        }
        let (value, next) = parse_signed_number(elems, i + 2, "an objective attribute")?;
        if !value.is_finite() {
            return Err(err(loc, format!("objective attribute '{attribute}' must be finite")));
        }
        let repeated = |set: bool| if set { Err(err(loc, format!("objective attribute '{attribute}' is given twice"))) } else { Ok(()) };
        let tolerance =
            |value: f64| if value < 0.0 { Err(err(loc, format!("tolerance '{attribute}' must not be negative"))) } else { Ok(value) };
        match attribute.to_ascii_lowercase().as_str() {
            "priority" => {
                repeated(attributes.priority.is_some())?;
                // Priorities are integers; 2^53 bounds the exactly representable ones.
                #[allow(clippy::cast_possible_truncation)]
                let priority = (value.fract() == 0.0 && value.abs() < 9_007_199_254_740_992.0).then_some(value as i64);
                attributes.priority = Some(priority.ok_or_else(|| err(loc, format!("priority must be an integer, got {value}")))?);
            }
            "weight" => {
                repeated(attributes.weight.is_some())?;
                attributes.weight = Some(value);
            }
            "abstol" => {
                repeated(attributes.abs_tol.is_some())?;
                attributes.abs_tol = Some(tolerance(value)?);
            }
            "reltol" => {
                repeated(attributes.rel_tol.is_some())?;
                attributes.rel_tol = Some(tolerance(value)?);
            }
            _ => return Err(err(loc, format!("unknown objective attribute '{attribute}' (expected Priority, Weight, AbsTol or RelTol)"))),
        }
        i = next;
    }
    Ok((attributes, i))
}

/// Name the generated upper half of a ranged constraint, avoiding any name the
/// user has written explicitly (`c1_rng`, else `c1_rng2`, `c1_rng3`, ...).
///
/// Without this a user constraint genuinely called `c1_rng` and the generated
/// half of `c1: 2 <= x <= 10` collide, and one of the two is lost.
pub(crate) fn range_upper_name<'input>(base: &'input str, taken: &HashSet<&'input str>) -> Cow<'input, str> {
    let mut candidate = format!("{base}_rng");
    let mut suffix: u32 = 1;
    while taken.contains(candidate.as_str()) {
        suffix += 1;
        candidate = format!("{base}_rng{suffix}");
    }
    Cow::Owned(candidate)
}

/// Reject a right-hand side that folding constants turned into NaN
/// (`inf - inf`); an infinite RHS on its own is a valid (if vacuous) bound.
fn checked_rhs(rhs: f64, position: usize) -> Result<f64, LexerError> {
    if rhs.is_nan() {
        return Err(err(position, "constraint right-hand side is undefined (infinite constants on both sides)"));
    }
    Ok(rhs)
}

/// Check that `op1` and `op2` form a range, `lo op1 expr op2 hi`: both must
/// point the same way (`<`/`<=` or `>`/`>=`). An `=` or a mix of directions
/// (`2 <= x >= 10`) states no range.
fn check_range_operators(op1: ComparisonOp, op2: ComparisonOp, position: usize) -> Result<(), LexerError> {
    let same_direction = matches!(
        (op1, op2),
        (ComparisonOp::LT | ComparisonOp::LTE, ComparisonOp::LT | ComparisonOp::LTE)
            | (ComparisonOp::GT | ComparisonOp::GTE, ComparisonOp::GT | ComparisonOp::GTE)
    );
    if same_direction {
        return Ok(());
    }
    Err(err(position, "a ranged constraint needs both operators in the same direction ('lo <= expr <= hi' or 'hi >= expr >= lo')"))
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
    let mut sections = assemble_constraint_sections(&[elems])?;
    debug_assert_eq!(sections.len(), 1, "one body in, one constraint list out");
    Ok(sections.pop().unwrap_or_default())
}

/// Assemble several constraint section bodies (`Subject To`, `Lazy
/// Constraints`, `User Cuts`), returning one constraint list per body.
///
/// The bodies share one name space: the generated upper half of a ranged
/// constraint avoids an explicit name written in *any* of them.
///
/// # Errors
///
/// See [`assemble_constraints`].
pub fn assemble_constraint_sections<'input>(bodies: &[&[SpannedElem<'input>]]) -> Result<Vec<Vec<RawConstraint<'input>>>, LexerError> {
    let explicit_names: HashSet<&'input str> = bodies
        .iter()
        .flat_map(|elems| elems.iter())
        .filter_map(|(_, elem)| if let Elem::Name(n) = *elem { Some(n) } else { None })
        .collect();
    bodies.iter().map(|elems| assemble_body(elems, &explicit_names)).collect()
}

/// Assemble one constraint body; see [`assemble_constraints`].
fn assemble_body<'input>(
    elems: &[SpannedElem<'input>],
    explicit_names: &HashSet<&'input str>,
) -> Result<Vec<RawConstraint<'input>>, LexerError> {
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

        let indicator = parse_indicator_head(elems, i)?;
        if let Some((_, _, next)) = indicator {
            i = next;
        }
        let first_new = constraints.len();
        // Quadratic terms of the entry, attached once its linear row is built.
        let quadratic: Vec<RawQuadraticTerm<'input>>;

        let (lhs, next) = parse_segment(elems, i, QuadraticContext::Constraint)?;
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

        if lhs.is_numeric_only() {
            // Numeric-only LHS: flipped (`10 >= x + y`) or ranged
            // (`2 <= x + y <= 10`) constraint.
            let (mid, next) = parse_segment(elems, i, QuadraticContext::Constraint)?;
            if next == i {
                return Err(err(pos_at(elems, i), "expected an expression after the comparison operator"));
            }
            i = next;

            if let Some(&(op2_loc, Elem::Op(op2))) = elems.get(i) {
                // Ranged: lhs.constant op1 mid op2 rhs
                check_range_operators(op1, op2, op2_loc)?;
                i += 1;
                let (rhs, next) = parse_signed_number(elems, i, "range bound")?;
                i = next;
                if !mid.quadratic.is_empty() {
                    return Err(err(entry_loc, "a ranged constraint cannot have quadratic terms"));
                }
                quadratic = Vec::new();

                let lower_name: Cow<'input, str> = name.map_or(Cow::Borrowed("__c__"), Cow::Borrowed);
                let upper_name: Cow<'input, str> = name.map_or(Cow::Borrowed("__c__"), |n| range_upper_name(n, explicit_names));
                constraints.push(RawConstraint::Standard {
                    name: lower_name,
                    coefficients: mid.coefficients.clone(),
                    operator: flip(op1),
                    rhs: checked_rhs(lhs.constant - mid.constant, entry_loc)?,
                    byte_offset: Some(entry_loc),
                });
                constraints.push(RawConstraint::Standard {
                    name: upper_name,
                    coefficients: mid.coefficients,
                    operator: op2,
                    rhs: checked_rhs(rhs - mid.constant, entry_loc)?,
                    byte_offset: Some(entry_loc),
                });
            } else {
                // Flipped: normalise so the variables sit on the left.
                quadratic = mid.quadratic;
                constraints.push(RawConstraint::Standard {
                    name: name.map_or(Cow::Borrowed("__c__"), Cow::Borrowed),
                    coefficients: mid.coefficients,
                    operator: flip(op1),
                    rhs: checked_rhs(lhs.constant - mid.constant, entry_loc)?,
                    byte_offset: Some(entry_loc),
                });
            }
        } else {
            // Standard: RHS is a single signed number; LHS constants fold in.
            let (rhs, next) = parse_signed_number(elems, i, "constraint right-hand side")?;
            i = next;
            quadratic = lhs.quadratic;
            constraints.push(RawConstraint::Standard {
                name: name.map_or(Cow::Borrowed("__c__"), Cow::Borrowed),
                coefficients: lhs.coefficients,
                operator: op1,
                rhs: checked_rhs(rhs - lhs.constant, entry_loc)?,
                byte_offset: Some(entry_loc),
            });
        }

        if !quadratic.is_empty() {
            if indicator.is_some() {
                return Err(err(entry_loc, "the constraint of an indicator must be linear"));
            }
            let Some(RawConstraint::Standard { name, coefficients, operator, rhs, byte_offset }) = constraints.pop() else {
                unreachable!("a non-ranged entry pushes exactly one standard constraint");
            };
            debug_assert_eq!(constraints.len(), first_new, "the quadratic entry must be the only constraint pushed");
            constraints.push(RawConstraint::Quadratic { name, coefficients, quadratic, operator, rhs, byte_offset });
        }

        if let Some((variable, active_value, _)) = indicator {
            let linear = constraints.split_off(first_new);
            let Ok([RawConstraint::Standard { name, coefficients, operator, rhs, byte_offset }]) =
                <[RawConstraint<'input>; 1]>::try_from(linear)
            else {
                return Err(err(entry_loc, "the constraint of an indicator must be a single linear constraint, not a range"));
            };
            constraints.push(RawConstraint::Indicator { name, variable, active_value, coefficients, operator, rhs, byte_offset });
        }
    }

    Ok(constraints)
}

/// Assemble a Gurobi `General Constraints` section body. Each entry is
/// `[name:] resultant = FUNCTION ( arg , arg , ... )` with `FUNCTION` one of
/// `MAX`, `MIN` (variables and at most one constant), `ABS` (one variable),
/// `AND`, `OR` (variables). Parentheses and commas must be separated from
/// names by whitespace, as Gurobi writes them: `x1,` would read as one name.
///
/// # Errors
///
/// Returns an error for an entry that does not have this shape.
pub fn assemble_general_constraints<'input>(elems: &[SpannedElem<'input>]) -> Result<Vec<RawConstraint<'input>>, LexerError> {
    let mut constraints = Vec::new();
    let mut i = 0;
    while i < elems.len() {
        let entry_loc = elems[i].0;
        let name = if let Elem::Name(n) = elems[i].1 {
            i += 1;
            Cow::Borrowed(n)
        } else {
            Cow::Borrowed("__c__")
        };
        let (
            Some(&(_, Elem::Var(resultant))),
            Some((_, Elem::Op(ComparisonOp::EQ))),
            Some(&(func_loc, Elem::Var(keyword))),
            Some((_, Elem::Var("("))),
        ) = (elems.get(i), elems.get(i + 1), elems.get(i + 2), elems.get(i + 3))
        else {
            return Err(err(pos_at(elems, i), "expected 'resultant = FUNCTION ( ... )' in the general constraints section"));
        };
        i += 4;

        // Arguments: variables and signed numbers separated by ',' up to ')'.
        let mut variables: Vec<&'input str> = Vec::new();
        let mut constants: Vec<f64> = Vec::new();
        loop {
            match elems.get(i) {
                Some(&(_, Elem::Var(variable))) if variable != ")" && variable != "," && variable != "(" => {
                    variables.push(variable);
                    i += 1;
                }
                Some((_, Elem::Plus | Elem::Minus | Elem::Num(_))) => {
                    let (value, next) = parse_signed_number(elems, i, "a general constraint argument")?;
                    if !value.is_finite() {
                        return Err(err(pos_at(elems, i), "a general constraint constant must be finite"));
                    }
                    constants.push(value);
                    i = next;
                }
                _ => return Err(err(pos_at(elems, i), "expected a variable or number as a general constraint argument")),
            }
            match elems.get(i) {
                Some((_, Elem::Var(","))) => i += 1,
                Some((_, Elem::Var(")"))) => {
                    i += 1;
                    break;
                }
                _ => return Err(err(pos_at(elems, i), "expected ',' or ')' after a general constraint argument")),
            }
        }

        let function = general_function(keyword, variables, &constants).map_err(|message| err(func_loc, message))?;
        constraints.push(RawConstraint::General { name, resultant, function, byte_offset: Some(entry_loc) });
    }
    Ok(constraints)
}

/// Group the entries of an `SOS` section into sets: each header owns the
/// weights that follow it. `section_start` is the position of the `SOS`
/// keyword.
///
/// # Errors
///
/// Returns an error for a weight before any header, or a header with no
/// weights: either would otherwise vanish from the model without a trace.
pub(crate) fn assemble_sos<'input>(
    entries: Vec<(usize, SosEntryKind<'input>)>,
    section_start: usize,
) -> Result<Vec<RawConstraint<'input>>, LexerError> {
    let mut sets: Vec<RawConstraint<'input>> = Vec::new();
    for (position, entry) in entries {
        match entry {
            SosEntryKind::Header(name, sos_type, offset) => {
                reject_empty_sos_set(sets.last(), section_start)?;
                sets.push(RawConstraint::SOS { name: Cow::Borrowed(name), sos_type, weights: Vec::new(), byte_offset: Some(offset) });
            }
            SosEntryKind::Weight(coefficient) => {
                let Some(RawConstraint::SOS { weights, .. }) = sets.last_mut() else {
                    return Err(err(
                        position,
                        format!("SOS weight '{}' does not follow a set header ('name: S1::' or 'S1::')", coefficient.name),
                    ));
                };
                weights.push(coefficient);
            }
        }
    }
    reject_empty_sos_set(sets.last(), section_start)?;
    debug_assert!(sets.iter().all(|set| matches!(set, RawConstraint::SOS { weights, .. } if !weights.is_empty())));
    Ok(sets)
}

/// Reject an SOS set that ended without any weights.
fn reject_empty_sos_set(last: Option<&RawConstraint<'_>>, fallback_position: usize) -> Result<(), LexerError> {
    match last {
        Some(RawConstraint::SOS { name, weights, byte_offset, .. }) if weights.is_empty() => {
            let shown = if name == "__c__" { "(unnamed)" } else { name.as_ref() };
            Err(err(byte_offset.unwrap_or(fallback_position), format!("SOS set '{shown}' has no weights")))
        }
        _ => Ok(()),
    }
}

/// The header of an unnamed SOS set, `S1::` or `S2::` (CPLEX). The set is
/// named `SOS<n>` when the problem is built.
///
/// # Errors
///
/// Returns an error when `keyword` is not an SOS type.
pub(crate) fn unnamed_sos_header(keyword: &str, position: usize) -> Result<SosEntryKind<'static>, LexerError> {
    let sos_type = if keyword.eq_ignore_ascii_case("S1") {
        SOSType::S1
    } else if keyword.eq_ignore_ascii_case("S2") {
        SOSType::S2
    } else {
        return Err(err(position, format!("expected an SOS type 'S1' or 'S2' before '::', found '{keyword}'")));
    };
    Ok(SosEntryKind::Header("__c__", sos_type, position))
}

/// Build a [`GeneralFunction`] from its keyword and parsed arguments.
fn general_function<'input>(keyword: &str, variables: Vec<&'input str>, constants: &[f64]) -> Result<GeneralFunction<&'input str>, String> {
    let upper = keyword.to_ascii_uppercase();
    let no_constants = |name: &str| {
        if constants.is_empty() { Ok(()) } else { Err(format!("{name} takes variables only, not constants")) }
    };
    match upper.as_str() {
        "MAX" | "MIN" => {
            if variables.is_empty() {
                return Err(format!("{upper} needs at least one variable argument"));
            }
            // Several constants are equivalent to their largest (MAX) or smallest (MIN).
            let fold = if upper == "MAX" { f64::max } else { f64::min };
            let constant = constants.iter().copied().reduce(fold);
            Ok(if upper == "MAX" { GeneralFunction::Max { variables, constant } } else { GeneralFunction::Min { variables, constant } })
        }
        "ABS" => {
            no_constants("ABS")?;
            let [variable] = variables.as_slice() else {
                return Err(format!("ABS takes exactly one variable, got {}", variables.len()));
            };
            Ok(GeneralFunction::Abs { variable })
        }
        "AND" | "OR" => {
            no_constants(&upper)?;
            if variables.is_empty() {
                return Err(format!("{upper} needs at least one variable argument"));
            }
            Ok(if upper == "AND" { GeneralFunction::And { variables } } else { GeneralFunction::Or { variables } })
        }
        _ => Err(format!("unsupported general constraint function '{keyword}' (expected MAX, MIN, ABS, AND or OR)")),
    }
}

/// Recognise the head of an indicator constraint, `var = 0 ->` or
/// `var = 1 ->`, at `i`. Returns the variable, whether the constraint is
/// active when the variable is 1, and the index after the arrow.
///
/// # Errors
///
/// Returns an error when the head is followed by `->` but the value is not
/// 0 or 1.
fn parse_indicator_head<'input>(elems: &[SpannedElem<'input>], i: usize) -> Result<Option<(&'input str, bool, usize)>, LexerError> {
    let (
        Some(&(_, Elem::Var(variable))),
        Some((_, Elem::Op(ComparisonOp::EQ))),
        Some(&(value_loc, Elem::Num(value))),
        Some((_, Elem::Implies)),
    ) = (elems.get(i), elems.get(i + 1), elems.get(i + 2), elems.get(i + 3))
    else {
        return Ok(None);
    };
    // The literals are compared exactly: only `0` and `1` are indicator values.
    #[allow(clippy::float_cmp)]
    let active_value = if value == 1.0 {
        true
    } else if value == 0.0 {
        false
    } else {
        return Err(err(value_loc, format!("indicator variable '{variable}' must be compared with 0 or 1, not {value}")));
    };
    Ok(Some((variable, active_value, i + 4)))
}
