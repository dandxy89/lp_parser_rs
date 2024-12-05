//! LP Parser for optimization problems.
//!
//! This module provides a fast, memory-efficient parser for Linear Programming (LP) files using nom.
//! Supports standard LP file format including:
//! - Problem name and sense (minimize/maximize)
//! - Objectives and constraints
//! - Bounds, integers, and binary variables
//! - Scientific notation
//! - Comments and whitespace handling
//!
//! Example usage:
//! ```rust
//! use lp_parser::lp_problem;
//!
//! let input = r#"
//!     Minimize
//!     obj: x1 + 2 x2
//!     Subject To
//!     c1: x1 + x2 <= 5
//!     Bounds
//!     0 <= x1 <= 1
//! "#;
//!
//! let (_, problem) = lp_problem(input).unwrap();
//! ```

use std::sync::atomic::{AtomicU64, Ordering};

use nom::{
    branch::alt,
    bytes::complete::{tag, tag_no_case, take_until, take_while1},
    character::complete::{char, digit1, multispace0, not_line_ending, one_of, space0},
    combinator::{map, opt, recognize, value},
    multi::{many0, many1},
    sequence::{delimited, pair, preceded, terminated, tuple},
    IResult,
};

// Pre-compiled static constant sets
const VALID_LP_CHARS: [char; 18] = ['!', '#', '$', '%', '&', '(', ')', '_', ',', '.', ';', '?', '@', '\\', '{', '}', '~', '\''];

const SECTION_HEADERS: [&str; 12] =
    ["integers", "integer", "general", "generals", "gen", "binaries", "binary", "bin", "bounds", "bound", "sos", "end"];

#[derive(Debug)]
pub struct IdGenerator {
    counter: AtomicU64,
    prefix: &'static str,
}

impl IdGenerator {
    pub fn new(prefix: &'static str) -> Self {
        Self { counter: AtomicU64::new(0), prefix }
    }

    pub fn next_id(&self) -> String {
        let id = self.counter.fetch_add(1, Ordering::Relaxed);
        format!("{}_{}", self.prefix, id)
    }
}

// Core data structures
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Coefficient<'a> {
    /// Variable name (borrowed from input)
    pub var_name: &'a str,
    /// Coefficient value
    pub coefficient: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Objective<'a> {
    /// Optional objective name
    pub name: Option<String>,
    /// Vector of coefficients
    pub coefficients: Vec<Coefficient<'a>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sense {
    Minimize,
    Maximize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComparisonOp {
    LessThan,
    LessOrEqual,
    Equal,
    GreaterThan,
    GreaterOrEqual,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SOSType {
    /// Type 1 SOS
    S1,
    /// Type 2 SOS
    S2,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Constraint<'a> {
    Standard { name: Option<String>, coefficients: Vec<Coefficient<'a>>, operator: ComparisonOp, rhs: f64 },
    SOS { name: Option<String>, sos_type: SOSType, weights: Vec<Coefficient<'a>> },
}

#[derive(Debug, Clone, PartialEq)]
pub enum Bound<'a> {
    /// Free variable (unbounded)
    Free(&'a str),
    /// Lower bounded variable: x ≥ lb
    LowerBound(&'a str, f64),
    /// Upper bounded variable: x ≤ ub
    UpperBound(&'a str, f64),
    /// Double bounded variable: lb ≤ x ≤ ub
    DoubleBound(&'a str, f64, f64),
}

#[derive(Debug, Clone, PartialEq)]
pub enum VariableType {
    Free,
    Binary,
    Integer,
    General { lower_bound: Option<f64>, upper_bound: Option<f64> },
    SemiContinuous { lower_bound: Option<f64>, upper_bound: Option<f64> },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Variable<'a> {
    pub name: &'a str,
    pub var_type: VariableType,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LPProblem<'a> {
    /// Optional problem name
    pub name: Option<&'a str>,
    /// Optimization sense (min/max)
    pub sense: Sense,
    /// Objective function
    pub objectives: Vec<Objective<'a>>,
    /// Vector of constraints
    pub constraints: Vec<Constraint<'a>>,
    /// Vector of variable bounds
    pub variables: Vec<Variable<'a>>,
}

impl<'a> LPProblem<'a> {
    /// Returns the problem name if it exists
    #[inline]
    pub fn name(&self) -> Option<&str> {
        self.name
    }

    #[inline]
    /// Returns true if the problem is a minimization
    pub fn is_minimization(&self) -> bool {
        matches!(self.sense, Sense::Minimize)
    }

    #[inline]
    /// Returns the number of constraints
    pub fn constraint_count(&self) -> usize {
        self.constraints.len()
    }

    #[inline]
    /// Returns the number of objectives
    pub fn objective_count(&self) -> usize {
        self.objectives.len()
    }
}

#[inline(always)]
pub fn valid_lp_char(c: char) -> bool {
    c.is_alphanumeric() || VALID_LP_CHARS.contains(&c)
}

#[inline]
fn scientific_notation(input: &str) -> IResult<&str, &str> {
    recognize(tuple((
        // Optional sign
        opt(one_of("+-")),
        digit1,
        opt(pair(char('.'), digit1)),
        alt((char('e'), char('E'))),
        opt(one_of("+-")),
        digit1,
    )))(input)
}

#[inline]
fn decimal_number(input: &str) -> IResult<&str, &str> {
    recognize(tuple((opt(one_of("+-")), digit1, opt(pair(char('.'), digit1)))))(input)
}

#[inline]
fn number(input: &str) -> IResult<&str, &str> {
    alt((scientific_notation, decimal_number))(input)
}

#[inline]
fn infinity(input: &str) -> IResult<&str, f64> {
    map(tuple((opt(one_of("+-")), alt((tag_no_case("infinity"), tag_no_case("inf"))))), |(sign, _)| match sign {
        Some('-') => f64::NEG_INFINITY,
        _ => f64::INFINITY,
    })(input)
}

#[inline]
pub fn number_value(input: &str) -> IResult<&str, f64> {
    preceded(multispace0, alt((infinity, map(number, |n: &str| n.parse::<f64>().unwrap_or(0.0)))))(input)
}

#[inline]
pub fn problem_name(input: &str) -> IResult<&str, &str> {
    recognize(take_while1(valid_lp_char))(input)
}

#[inline]
pub fn problem_name_line(input: &str) -> IResult<&str, &str> {
    delimited(tuple((multispace0, tag("\\"), multispace0, take_until(":"), tag(":"), multispace0)), not_line_ending, multispace0)(input)
}

#[inline]
pub fn problem_sense(input: &str) -> IResult<&str, Sense> {
    delimited(
        multispace0,
        alt((
            value(Sense::Minimize, alt((tag_no_case("minimize"), tag_no_case("minimum"), tag_no_case("min")))),
            value(Sense::Maximize, alt((tag_no_case("maximize"), tag_no_case("maximum"), tag_no_case("max")))),
        )),
        multispace0,
    )(input)
}

#[inline]
pub fn variable(input: &str) -> IResult<&str, &str> {
    take_while1(valid_lp_char)(input)
}

#[inline]
pub fn coefficient(input: &str) -> IResult<&str, Coefficient> {
    map(
        tuple((opt(preceded(space0, alt((char('+'), char('-'))))), opt(preceded(space0, number_value)), preceded(space0, variable))),
        |(sign, coef, var_name)| Coefficient {
            var_name,
            coefficient: {
                let base_coef = coef.unwrap_or(1.0);
                if sign == Some('-') {
                    -base_coef
                } else {
                    base_coef
                }
            },
        },
    )(input)
}

#[inline]
pub fn objective(input: &str) -> IResult<&str, Objective> {
    map(
        tuple((
            opt(terminated(preceded(multispace0, variable), delimited(multispace0, char(':'), multispace0))),
            many1(preceded(space0, coefficient)),
        )),
        |(name, coefficients)| Objective { name: name.map(|s| s.to_string()), coefficients },
    )(input)
}

#[inline]
pub fn objectives_section(input: &str) -> IResult<&str, Vec<Objective>> {
    many1(objective)(input)
}

#[inline]
pub fn comparison_operator(input: &str) -> IResult<&str, ComparisonOp> {
    preceded(
        multispace0,
        alt((
            value(ComparisonOp::LessOrEqual, tag("<=")),
            value(ComparisonOp::GreaterOrEqual, tag(">=")),
            value(ComparisonOp::Equal, tag("=")),
            value(ComparisonOp::LessThan, tag("<")),
            value(ComparisonOp::GreaterThan, tag(">")),
        )),
    )(input)
}

#[inline]
pub fn constraint_section_header(input: &str) -> IResult<&str, ()> {
    value(
        (),
        tuple((
            multispace0,
            alt((tag_no_case("subject to"), tag_no_case("such that"), tag_no_case("s.t."), tag_no_case("st"))),
            opt(char(':')),
            multispace0,
        )),
    )(input)
}

#[inline]
pub fn constraint(input: &str) -> IResult<&str, Constraint> {
    map(
        tuple((
            // Name part with optional whitespace and newlines
            opt(terminated(preceded(multispace0, variable), delimited(multispace0, opt(char(':')), multispace0))),
            // Coefficients with flexible whitespace and newlines
            many1(preceded(
                multispace0, // This will handle spaces, tabs, and newlines
                coefficient,
            )),
            // Operator and RHS with flexible whitespace
            preceded(multispace0, comparison_operator),
            preceded(multispace0, number_value),
        )),
        |(name, coefficients, operator, rhs)| Constraint::Standard { name: name.map(|s| s.to_string()), coefficients, operator, rhs },
    )(input)
}

#[inline]
pub fn constraints_section(input: &str) -> IResult<&str, Vec<Constraint>> {
    preceded(constraint_section_header, many0(terminated(constraint, multispace0)))(input)
}

#[inline]
pub fn bound(input: &str) -> IResult<&str, Bound> {
    preceded(
        multispace0,
        alt((
            // Free variable: x1 free
            map(tuple((variable, preceded(space0, tag_no_case("free")))), |(var_name, _)| Bound::Free(var_name)),
            // Double bound: 0 <= x1 <= 5
            map(
                tuple((
                    number_value,
                    preceded(space0, tag("<=")),
                    preceded(space0, variable),
                    preceded(space0, tag("<=")),
                    preceded(space0, number_value),
                )),
                |(lower, _, var_name, _, upper)| Bound::DoubleBound(var_name, lower, upper),
            ),
            // Lower bound: x1 >= 5 or 5 <= x1
            alt((
                map(tuple((variable, preceded(space0, tag(">=")), preceded(space0, number_value))), |(var_name, _, bound)| {
                    Bound::LowerBound(var_name, bound)
                }),
                map(tuple((number_value, preceded(space0, tag("<=")), preceded(space0, variable))), |(bound, _, var_name)| {
                    Bound::LowerBound(var_name, bound)
                }),
            )),
            // Upper bound: x1 <= 5 or 5 >= x1
            alt((
                map(tuple((variable, preceded(space0, tag("<=")), preceded(space0, number_value))), |(var_name, _, bound)| {
                    Bound::UpperBound(var_name, bound)
                }),
                map(tuple((number_value, preceded(space0, tag(">=")), preceded(space0, variable))), |(bound, _, var_name)| {
                    Bound::UpperBound(var_name, bound)
                }),
            )),
        )),
    )(input)
}

#[inline]
pub fn bounds_section(input: &str) -> IResult<&str, Vec<Bound>> {
    preceded(tuple((multispace0, tag_no_case("bounds"), opt(preceded(space0, char(':'))), multispace0)), many0(bound))(input)
}

#[inline]
fn is_section_header(input: &str) -> bool {
    let lower_input = input.trim().to_lowercase();
    SECTION_HEADERS.iter().any(|&header| lower_input.starts_with(header))
}

#[inline]
fn variable_not_header(input: &str) -> IResult<&str, &str> {
    let (input, _) = multispace0(input)?;
    if is_section_header(input) {
        return Err(nom::Err::Error(nom::error::Error::new(input, nom::error::ErrorKind::Not)));
    }
    variable(input)
}

#[inline]
pub fn variable_list(input: &str) -> IResult<&str, Vec<&str>> {
    many0(variable_not_header)(input)
}

#[inline]
pub fn integer_section(input: &str) -> IResult<&str, Vec<&str>> {
    preceded(
        tuple((
            multispace0,
            alt((tag_no_case("generals"), tag_no_case("general"), tag_no_case("integers"), tag_no_case("integer"), tag_no_case("gen"))),
            opt(preceded(space0, char(':'))),
            multispace0,
        )),
        variable_list,
    )(input)
}

#[inline]
pub fn binary_section(input: &str) -> IResult<&str, Vec<&str>> {
    preceded(
        tuple((
            multispace0,
            alt((tag_no_case("binaries"), tag_no_case("binary"), tag_no_case("bin"))),
            opt(preceded(space0, char(':'))),
            multispace0,
        )),
        variable_list,
    )(input)
}

#[inline]
pub fn end_section(input: &str) -> IResult<&str, ()> {
    value((), tuple((multispace0, tag_no_case("end"), multispace0)))(input)
}

#[inline]
pub fn semi_continuous_header(input: &str) -> IResult<&str, ()> {
    value(
        (),
        tuple((multispace0, alt((tag_no_case("semi-continuous"), tag_no_case("semi"), tag_no_case("semis"))), opt(char(':')), multispace0)),
    )(input)
}

// Parse a semi-continuous variable declaration
#[inline]
pub fn semi_continuous_variable(input: &str) -> IResult<&str, Variable> {
    map(
        tuple((
            multispace0,
            variable, // Reuse existing variable name parser
            opt(preceded(
                space0,
                delimited(
                    char('('),
                    tuple((
                        number_value, // Lower bound
                        preceded(space0, char(',')),
                        preceded(space0, number_value), // Upper bound
                    )),
                    char(')'),
                ),
            )),
        )),
        |(_, name, bounds)| {
            let (lower_bound, upper_bound) = if let Some((lb, _, ub)) = bounds {
                (Some(lb), Some(ub))
            } else {
                (Some(0.0), None) // Default lower bound of 0 if not specified
            };

            Variable { name, var_type: VariableType::SemiContinuous { lower_bound, upper_bound } }
        },
    )(input)
}

// Parse the semi-continuous section
#[inline]
pub fn semi_continuous_section(input: &str) -> IResult<&str, Vec<Variable>> {
    preceded(semi_continuous_header, many0(terminated(semi_continuous_variable, multispace0)))(input)
}

#[inline]
pub fn sos_header(input: &str) -> IResult<&str, ()> {
    value((), tuple((multispace0, tag_no_case("sos"), opt(char(':')), multispace0)))(input)
}

#[inline]
pub fn sos_type(input: &str) -> IResult<&str, SOSType> {
    preceded(
        multispace0,
        alt((map(preceded(tag_no_case("s1"), tag("::")), |_| SOSType::S1), map(preceded(tag_no_case("s2"), tag("::")), |_| SOSType::S2))),
    )(input)
}

#[inline]
pub fn sos_weight(input: &str) -> IResult<&str, Coefficient> {
    map(tuple((preceded(multispace0, variable), delimited(multispace0, char(':'), multispace0), number_value)), |(var_name, _, weight)| {
        Coefficient { var_name, coefficient: weight }
    })(input)
}

#[inline]
pub fn sos_constraint(input: &str) -> IResult<&str, Constraint> {
    map(
        tuple((opt(delimited(multispace0, variable, delimited(multispace0, char(':'), multispace0))), sos_type, many1(sos_weight))),
        |(name, sos_type, weights)| Constraint::SOS { name: name.map(|s| s.to_string()), sos_type, weights },
    )(input)
}

#[inline]
pub fn sos_section(input: &str) -> IResult<&str, Vec<Constraint>> {
    preceded(sos_header, many0(terminated(sos_constraint, multispace0)))(input)
}

#[inline]
pub fn lp_problem(input: &str) -> IResult<&str, LPProblem> {
    let (input, (name, sense, objectives, mut constraints, bounds, integer_vars, binary_vars, semi_continuous_vars, sos_constraints)) =
        tuple((
            opt(terminated(problem_name_line, multispace0)),
            terminated(problem_sense, multispace0),
            terminated(objectives_section, multispace0),
            terminated(constraints_section, multispace0),
            opt(terminated(bounds_section, multispace0)),
            opt(terminated(integer_section, multispace0)),
            opt(terminated(binary_section, multispace0)),
            opt(terminated(semi_continuous_section, multispace0)),
            opt(terminated(sos_section, multispace0)),
        ))(input)?;

    if let Some(sos) = sos_constraints {
        constraints.extend(sos);
    }

    let mut variables = Vec::new();
    if let Some(bounds) = bounds {
        for bound in bounds {
            let var = match bound {
                Bound::Free(name) => Variable { name, var_type: VariableType::Free },
                Bound::LowerBound(name, lb) => {
                    Variable { name, var_type: VariableType::General { lower_bound: Some(lb), upper_bound: None } }
                }
                Bound::UpperBound(name, ub) => {
                    Variable { name, var_type: VariableType::General { lower_bound: None, upper_bound: Some(ub) } }
                }
                Bound::DoubleBound(name, lb, ub) => {
                    if (lb, ub) == (0.0, 1.0) {
                        Variable { name, var_type: VariableType::Binary }
                    } else {
                        Variable { name, var_type: VariableType::General { lower_bound: Some(lb), upper_bound: Some(ub) } }
                    }
                }
            };
            variables.push(var);
        }
    }

    if let Some(int_vars) = integer_vars {
        for name in int_vars {
            if let Some(existing) = variables.iter_mut().find(|v| v.name == name) {
                existing.var_type = VariableType::Integer;
            } else {
                variables.push(Variable { name, var_type: VariableType::Integer });
            }
        }
    }

    if let Some(bin_vars) = binary_vars {
        for name in bin_vars {
            if let Some(existing) = variables.iter_mut().find(|v| v.name == name) {
                existing.var_type = VariableType::Binary;
            } else {
                variables.push(Variable { name, var_type: VariableType::Binary });
            }
        }
    }

    if let Some(semi_vars) = semi_continuous_vars {
        for semi_var in semi_vars {
            if let Some(existing) = variables.iter_mut().find(|v| v.name == semi_var.name) {
                existing.var_type = semi_var.var_type.clone();
            } else {
                variables.push(semi_var);
            }
        }
    }

    let problem = LPProblem { name, sense, objectives, constraints, variables };

    match end_section(input) {
        Ok((remaining, _)) => Ok((remaining, problem)),
        Err(_) if input.trim().is_empty() => Ok((input, problem)),
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use float_eq::assert_float_eq;

    use crate::{model::lp_error::LPParserError, parse::parse_file};

    use super::*;

    #[test]
    fn test_problem_name() {
        assert_eq!(problem_name("MyProblem123"), Ok(("", "MyProblem123")));
        assert_eq!(problem_name("Test_Problem!#$"), Ok(("", "Test_Problem!#$")));
        assert_eq!(problem_name("Problem1 rest"), Ok((" rest", "Problem1")));
    }

    #[test]
    fn test_problem_name_line() {
        assert_eq!(problem_name_line("\\ Problem name: TestProblem"), Ok(("", "TestProblem")));
        assert_eq!(problem_name_line("\\ Problem   name:    SpacedProblem"), Ok(("", "SpacedProblem")));
        assert_eq!(problem_name_line("\\ Problem name: Test_Problem!#$"), Ok(("", "Test_Problem!#$")));
        assert_eq!(problem_name_line("   \\ Problem name: LeadingSpace"), Ok(("", "LeadingSpace")));
    }

    #[test]
    fn test_problem_sense() {
        assert_eq!(problem_sense("   minimize"), Ok(("", Sense::Minimize)));
        assert_eq!(problem_sense("\nmaximize"), Ok(("", Sense::Maximize)));
        assert_eq!(problem_sense("minimize obj"), Ok(("obj", Sense::Minimize)));
        assert_eq!(problem_sense("maximize obj"), Ok(("obj", Sense::Maximize)));
        assert_eq!(problem_sense("MINIMIZE"), Ok(("", Sense::Minimize)));
        assert_eq!(problem_sense("MAXIMIZE"), Ok(("", Sense::Maximize)));
    }

    #[test]
    fn test_scientific_number() {
        assert_float_eq!(number_value("1e-03").unwrap().1, 0.001, abs <= 1e-10);
        assert_float_eq!(number_value("2.5e+02").unwrap().1, 250.0, abs <= 1e-10);
        assert_float_eq!(number_value("3.14").unwrap().1, 3.14, abs <= 1e-10);
    }

    #[test]
    fn test_coefficient() {
        let (_, coef1) = coefficient("x1").unwrap();
        assert_eq!(coef1.var_name, "x1");
        assert_float_eq!(coef1.coefficient, 1.0, abs <= 1e-10);

        let (_, coef2) = coefficient("-2.5x2").unwrap();
        assert_eq!(coef2.var_name, "x2");
        assert_float_eq!(coef2.coefficient, -2.5, abs <= 1e-10);

        let (_, coef3) = coefficient("1e-03x3").unwrap();
        assert_eq!(coef3.var_name, "x3");
        assert_float_eq!(coef3.coefficient, 0.001, abs <= 1e-10);

        let (_, coef4) = coefficient("  +3.5 x4").unwrap();
        assert_eq!(coef4.var_name, "x4");
        assert_float_eq!(coef4.coefficient, 3.5, abs <= 1e-10);

        let (_, coef5) = coefficient(" -x5").unwrap();
        assert_eq!(coef5.var_name, "x5");
        assert_float_eq!(coef5.coefficient, -1.0, abs <= 1e-10);

        let (_, coef6) = coefficient(" +2.5e+02 x6").unwrap();
        assert_eq!(coef6.var_name, "x6");
        assert_float_eq!(coef6.coefficient, 250.0, abs <= 1e-10);
    }

    #[test]
    fn test_objective() {
        let (rest, obj1) = objective("obj1: x1 + 2 x2").unwrap();
        assert_eq!(obj1.name, Some("obj1".to_string()));
        assert_eq!(obj1.coefficients.len(), 2);
        assert_eq!(obj1.coefficients[0].var_name, "x1");
        assert_float_eq!(obj1.coefficients[0].coefficient, 1.0, abs <= 1e-10);
        assert_eq!(obj1.coefficients[1].var_name, "x2");
        assert_float_eq!(obj1.coefficients[1].coefficient, 2.0, abs <= 1e-10);
        assert_eq!(rest, "");

        let (_, obj2) = objective("obj: 1e-03x1 + 2.5e+02x2 - 3.14x3").unwrap();
        assert_eq!(obj2.name, Some("obj".to_string()));
        assert_eq!(obj2.coefficients.len(), 3);
        assert_float_eq!(obj2.coefficients[0].coefficient, 0.001, abs <= 1e-10);
        assert_float_eq!(obj2.coefficients[1].coefficient, 250.0, abs <= 1e-10);
        assert_float_eq!(obj2.coefficients[2].coefficient, -3.14, abs <= 1e-10);

        let (_, obj3) = objective("obj: x1 - 2 x2 + 1.5 x3").unwrap();
        assert_eq!(obj3.name, Some("obj".to_string()));
        assert_eq!(obj3.coefficients.len(), 3);
        assert_float_eq!(obj3.coefficients[0].coefficient, 1.0, abs <= 1e-10);
        assert_float_eq!(obj3.coefficients[1].coefficient, -2.0, abs <= 1e-10);
        assert_float_eq!(obj3.coefficients[2].coefficient, 1.5, abs <= 1e-10);
    }

    #[test]
    fn test_comparison_operator() {
        assert_eq!(comparison_operator("<="), Ok(("", ComparisonOp::LessOrEqual)));
        assert_eq!(comparison_operator(">="), Ok(("", ComparisonOp::GreaterOrEqual)));
        assert_eq!(comparison_operator("="), Ok(("", ComparisonOp::Equal)));
        assert_eq!(comparison_operator("<"), Ok(("", ComparisonOp::LessThan)));
        assert_eq!(comparison_operator(">"), Ok(("", ComparisonOp::GreaterThan)));
    }

    #[test]
    fn test_constraint_section_header() {
        let test_cases = vec![
            "subject to:",
            "SUBJECT TO",
            "Subject To:",
            "subject to ",
            " subject to",
            "s.t.:",
            "s.t.",
            "st:",
            "ST ",
            "such that:",
            "SUCH THAT",
        ];

        for case in test_cases {
            let result = constraint_section_header(case);
            assert!(result.is_ok(), "Failed to parse: {case}",);

            let (remaining, _) = result.unwrap();
            assert!(remaining.trim().is_empty(), "Unexpected remaining input for {case}: '{remaining}'");
        }
    }

    #[test]
    fn test_infinity() {
        assert_eq!(infinity("infinity").unwrap().1, f64::INFINITY);
        assert_eq!(infinity("INFINITY").unwrap().1, f64::INFINITY);
        assert_eq!(infinity("inf").unwrap().1, f64::INFINITY);
        assert_eq!(infinity("INF").unwrap().1, f64::INFINITY);
        assert_eq!(infinity("+infinity").unwrap().1, f64::INFINITY);
        assert_eq!(infinity("-infinity").unwrap().1, f64::NEG_INFINITY);
        assert_eq!(infinity("-inf").unwrap().1, f64::NEG_INFINITY);
    }

    #[test]
    fn test_bounds_section() {
        let input = "\
            Bounds
                x1 free
                0 <= x2 <= 10
                x3 >= 5
                x4 <= 20
                -infinity <= x5 <= infinity
        ";
        let (rest, bounds) = bounds_section(input).unwrap();
        assert!(rest.trim().is_empty());
        assert_eq!(bounds.len(), 5);

        assert_eq!(bounds[0], Bound::Free("x1"));
        assert_eq!(bounds[1], Bound::DoubleBound("x2", 0.0, 10.0));
        assert_eq!(bounds[2], Bound::LowerBound("x3", 5.0));
        assert_eq!(bounds[3], Bound::UpperBound("x4", 20.0));
        assert_eq!(bounds[4], Bound::DoubleBound("x5", f64::NEG_INFINITY, f64::INFINITY));
    }

    #[test]
    fn test_integer_section() {
        let test_cases = vec![
            ("integers\n  x1  x2  x3\n", vec!["x1", "x2", "x3"]),
            ("Integers: x1 x2 x3", vec!["x1", "x2", "x3"]),
            ("GENERALS\nx1\nx2\nx3", vec!["x1", "x2", "x3"]),
            ("General:\n  x1  \n  x2  \n  x3", vec!["x1", "x2", "x3"]),
        ];

        for (input, expected) in test_cases {
            let (rest, vars) = integer_section(input).unwrap_or_else(|_| panic!("Failed to parse integer section: {}", input));
            assert!(rest.trim().is_empty(), "Remaining input for: {}", input);
            assert_eq!(vars, expected.into_iter().map(String::from).collect::<Vec<_>>(), "Mismatch for input: {}", input);
        }
    }

    #[test]
    fn test_binary_section() {
        let test_cases = vec![
            ("binaries\n  y1  y2  y3\n", vec!["y1", "y2", "y3"]),
            ("Binaries: y1 y2 y3", vec!["y1", "y2", "y3"]),
            ("BINARY\ny1\ny2\ny3", vec!["y1", "y2", "y3"]),
            ("Bin:\n  y1  \n  y2  \n  y3", vec!["y1", "y2", "y3"]),
        ];

        for (input, expected) in test_cases {
            let (rest, vars) = binary_section(input).unwrap_or_else(|_| panic!("Failed to parse binary section: {}", input));
            assert!(rest.trim().is_empty(), "Remaining input for: {}", input);
            assert_eq!(vars, expected.into_iter().map(String::from).collect::<Vec<_>>(), "Mismatch for input: {}", input);
        }
    }

    #[test]
    fn test_combined_sections() {
        let input = "\
            Integers\n\
            x1 x2 x3\n\
            Binaries\n\
            y1 y2 y3\n\
        ";

        let (rest, int_vars) = integer_section(input).unwrap();
        assert_eq!(int_vars, vec!["x1", "x2", "x3"].into_iter().map(String::from).collect::<Vec<_>>());

        let (final_rest, bin_vars) = binary_section(rest).unwrap();
        assert_eq!(bin_vars, vec!["y1", "y2", "y3"].into_iter().map(String::from).collect::<Vec<_>>());
        assert!(final_rest.trim().is_empty());
    }

    #[test]
    fn test_end_section() {
        let test_cases = vec!["end", "END", "End", "  end  ", "END\n", "end\r\n", "\nend\n", "  END  \n  "];

        for case in test_cases {
            let result = end_section(case);
            assert!(result.is_ok(), "Failed to parse END section: '{}'", case);
            let (remaining, _) = result.unwrap();
            assert!(remaining.trim().is_empty(), "Unexpected remaining input for '{}': '{}'", case, remaining);
        }
    }

    #[test]
    fn test_lp_problem_with_end() {
        let test_cases = vec![
            // Basic END
            "\
            Minimize
            x1 + x2
            Subject to
            x1 + x2 <= 1
            End
            ",
            // END with extra whitespace
            "\
            Minimize
            x1 + x2
            Subject to
            x1 + x2 <= 1
            END
            ",
            // END with newlines
            "\
            Minimize
            x1 + x2
            Subject to
            x1 + x2 <= 1

            END

            ",
            // END with mixed case
            "\
            Minimize
            x1 + x2
            Subject to
            x1 + x2 <= 1
            eNd
            ",
        ];

        for case in test_cases {
            let result = lp_problem(case);
            assert!(result.is_ok(), "Failed to parse LP problem with END: '{}'", case);
            let (remaining, _) = result.unwrap();
            assert!(remaining.trim().is_empty(), "Unexpected remaining input: '{}'", remaining);
        }
    }

    #[test]
    fn test_lp_problem_without_end() {
        let input = "\
        Minimize
        x1 + x2
        Subject to
        x1 + x2 <= 1
        ";

        let result = lp_problem(input);
        assert!(result.is_ok(), "Failed to parse LP problem without END");
        let (remaining, _) = result.unwrap();
        assert!(remaining.trim().is_empty(), "Unexpected remaining input: '{}'", remaining);
    }

    #[test]
    fn test_sos_type() {
        assert_eq!(sos_type("S1::"), Ok(("", SOSType::S1)));
        assert_eq!(sos_type("s1::"), Ok(("", SOSType::S1)));
        assert_eq!(sos_type("S2::"), Ok(("", SOSType::S2)));
        assert_eq!(sos_type("s2::"), Ok(("", SOSType::S2)));
        assert_eq!(sos_type(" S1::"), Ok(("", SOSType::S1)));
        assert_eq!(sos_type(" S2::"), Ok(("", SOSType::S2)));
    }

    #[test]
    fn test_sos_weight() {
        let (rest, weight) = sos_weight("x1: 1.0").unwrap();
        assert_eq!(rest, "");
        assert_eq!(weight.var_name, "x1");
        assert_float_eq!(weight.coefficient, 1.0, abs <= 1e-10);

        let (rest, weight) = sos_weight("x2:2.5").unwrap();
        assert_eq!(rest, "");
        assert_eq!(weight.var_name, "x2");
        assert_float_eq!(weight.coefficient, 2.5, abs <= 1e-10);

        let (rest, weight) = sos_weight("x3 : 3.0").unwrap();
        assert_eq!(rest, "");
        assert_eq!(weight.var_name, "x3");
        assert_float_eq!(weight.coefficient, 3.0, abs <= 1e-10);
    }

    #[test]
    fn test_sos_constraint() {
        let input = "csos1: S1:: V1:1.0 V3:2.2 V5:3.1";
        let (rest, constraint) = sos_constraint(input).unwrap();
        assert_eq!(rest, "");

        match constraint {
            Constraint::SOS { name, sos_type, weights } => {
                assert_eq!(name, Some("csos1".to_owned()));
                assert_eq!(sos_type, SOSType::S1);
                assert_eq!(weights.len(), 3);
                assert_eq!(weights[0].var_name, "V1");
                assert_float_eq!(weights[0].coefficient, 1.0, abs <= 1e-10);
                assert_eq!(weights[1].var_name, "V3");
                assert_float_eq!(weights[1].coefficient, 2.2, abs <= 1e-10);
                assert_eq!(weights[2].var_name, "V5");
                assert_float_eq!(weights[2].coefficient, 3.1, abs <= 1e-10);
            }
            _ => panic!("Expected SOS constraint"),
        }
    }

    #[test]
    fn test_lp_problem_with_all_sections() {
        let input = "\
        \\ Problem name: CompleteExample
        Minimize
        x1 + 2 x2
        Subject to
        c1: x1 + x2 <= 5
        Bounds
        0 <= x1 <= 1
        x2 <= 10
        Integers
        x1
        Binary
        x2
        SOS
        csos1: S1:: V1:1 V3:2 V5:3
        End
        ";

        let result = lp_problem(input).unwrap();
        insta::assert_debug_snapshot!(result);
    }

    #[test]
    fn test_lol() {
        let input = r#"
        maximize
        obj: 3 x1 + x2 + 5 x3 + x4
        subject to
        c1:  3 x1 + x2 + 2 x3 = 30
        c2:  2 x1 + x2 + 3 x3 + x4 >= 15
        c3:  2 x2 + 3 x4 <= 25
        bounds
         0 <= x1 <= +infinity
         0 <= x2 <= 10
         0 <= x3 <= +infinity
         0 <= x4 <= +infinity
        end
        "#;

        let result = lp_problem(input).unwrap();
        insta::assert_debug_snapshot!(result);
    }

    #[test]
    fn test_objectives_section() {
        let input = "OBJ1: x1 + 2 x2\n OBJ2: 3 x1 - x2\nOBJ3: 5 x1 + 4 x2";
        let (_, objectives) = objectives_section(input).unwrap();
        assert_eq!(objectives.len(), 3);
    }

    #[test]
    fn test_afiro() {
        let input = read_file_from_resources("afiro.lp").unwrap();
        let result = lp_problem(&input).unwrap();
        insta::assert_debug_snapshot!(result);
    }

    fn read_file_from_resources(file_name: &str) -> Result<String, LPParserError> {
        let mut file_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        file_path.push(format!("resources/{file_name}"));
        parse_file(&file_path)
    }

    #[test]
    fn test_x() {
        let input = read_file_from_resources("sos.lp").unwrap();
        let result = lp_problem(&input).unwrap();
        insta::assert_debug_snapshot!(result);

        // let input = read_file_from_resources("boeing1.lp").unwrap();
        // let result = lp_problem(&input).unwrap();
        // insta::assert_debug_snapshot!(result);
    }
}
