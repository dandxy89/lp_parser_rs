//! Lexer for LP files using Logos.
//!
//! This module provides a token-based lexer for Linear Programming files,
//! handling case-insensitive keywords, numbers, identifiers, and operators.

use std::borrow::Cow;
use std::fmt::{Display, Formatter, Result as FmtResult};

use logos::Logos;

use crate::model::{ComparisonOp, SOSType, Sense, VariableType};

/// Lexer error type
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LexerError;

// === Raw intermediate types ===
//
// These types are produced by the LALRPOP grammar during parsing and use
// `&'input str` for zero-copy name references. They are converted to the
// final interned model types in `problem.rs`.

/// Raw coefficient produced by the grammar (zero-copy).
#[derive(Debug, Clone, PartialEq)]
pub struct RawCoefficient<'input> {
    /// Variable name as a borrowed slice from the input.
    pub name: &'input str,
    /// The coefficient value.
    pub value: f64,
}

/// Raw constraint produced by the grammar (zero-copy).
#[derive(Debug, Clone, PartialEq)]
pub enum RawConstraint<'input> {
    /// A standard linear constraint.
    Standard {
        name: Cow<'input, str>,
        coefficients: Vec<RawCoefficient<'input>>,
        operator: ComparisonOp,
        rhs: f64,
        byte_offset: Option<usize>,
    },
    /// A special ordered set constraint.
    SOS { name: Cow<'input, str>, sos_type: SOSType, weights: Vec<RawCoefficient<'input>>, byte_offset: Option<usize> },
}

/// Raw objective produced by the grammar (zero-copy).
#[derive(Debug, Clone, PartialEq)]
pub struct RawObjective<'input> {
    /// Objective name (may be a sentinel like `"__obj__"` if unnamed).
    pub name: Cow<'input, str>,
    /// Coefficients of the objective function.
    pub coefficients: Vec<RawCoefficient<'input>>,
}

/// Helper enum for parsing SOS entries in the grammar.
#[derive(Debug, Clone, PartialEq)]
pub enum SosEntryKind<'input> {
    /// SOS constraint header: name, type, and byte offset.
    Header(&'input str, SOSType, usize),
    /// SOS weight: variable and weight value.
    Weight(RawCoefficient<'input>),
}

/// Helper enum for constraint continuation parsing.
#[derive(Debug, Clone, PartialEq)]
pub enum ConstraintCont<'input> {
    /// Named constraint: the leading identifier was the constraint name.
    Named(Vec<RawCoefficient<'input>>, ComparisonOp, f64),
    /// Unnamed constraint: the leading identifier was the first variable.
    Unnamed(Vec<(f64, RawCoefficient<'input>)>, ComparisonOp, f64),
}

impl<'input> ConstraintCont<'input> {
    /// Convert to a `RawConstraint`, using the given identifier as either name or first variable.
    ///
    /// `byte_offset` is the position of the constraint in the source text.
    #[must_use]
    pub fn into_constraint(self, id: &'input str, byte_offset: Option<usize>) -> RawConstraint<'input> {
        match self {
            ConstraintCont::Named(coeffs, op, rhs) => {
                RawConstraint::Standard { name: Cow::Borrowed(id), coefficients: coeffs, operator: op, rhs, byte_offset }
            }
            ConstraintCont::Unnamed(rest, op, rhs) => {
                let mut coeffs = vec![RawCoefficient { name: id, value: 1.0 }];
                for (s, c) in rest {
                    coeffs.push(RawCoefficient { name: c.name, value: s * c.value });
                }
                RawConstraint::Standard { name: Cow::Borrowed("__c__"), coefficients: coeffs, operator: op, rhs, byte_offset }
            }
        }
    }
}

/// Helper enum for optional sections that can appear in any order.
#[derive(Debug, Clone, PartialEq)]
pub enum OptionalSection<'input> {
    Bounds(Vec<(&'input str, VariableType)>),
    Generals(Vec<&'input str>),
    Integers(Vec<&'input str>),
    Binaries(Vec<&'input str>),
    SemiContinuous(Vec<&'input str>),
    SOS(Vec<RawConstraint<'input>>),
}

/// Structured result from the LALRPOP parser, replacing the previous 9-tuple.
#[derive(Debug, Clone, PartialEq)]
pub struct ParseResult<'input> {
    /// Optimisation sense (minimise/maximise).
    pub sense: Sense,
    /// Raw objectives from the grammar.
    pub objectives: Vec<RawObjective<'input>>,
    /// Raw constraints from the grammar.
    pub constraints: Vec<RawConstraint<'input>>,
    /// Variable bounds declarations.
    pub bounds: Vec<(&'input str, VariableType)>,
    /// General variable names.
    pub generals: Vec<&'input str>,
    /// Integer variable names.
    pub integers: Vec<&'input str>,
    /// Binary variable names.
    pub binaries: Vec<&'input str>,
    /// Semi-continuous variable names.
    pub semi_continuous: Vec<&'input str>,
    /// Raw SOS constraints.
    pub sos: Vec<RawConstraint<'input>>,
}

impl Display for LexerError {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        write!(f, "lexer error")
    }
}

/// Tokens for LP file parsing
#[derive(Logos, Debug, Clone, PartialEq)]
#[logos(skip r"[ \t\r]+")] // Skip horizontal whitespace (not newlines)
#[logos(error = LexerError)]
pub enum Token<'input> {
    // === Keywords (case-insensitive) ===
    /// Optimization sense: minimize
    #[regex(r"(?i)minimize|minimise|minimum|min", |_| Sense::Minimize, priority = 10)]
    /// Optimization sense: maximize
    #[regex(r"(?i)maximize|maximise|maximum|max", |_| Sense::Maximize, priority = 10)]
    SenseKw(Sense),

    /// Subject to / constraints header
    #[regex(r"(?i)(subject[ \t]+to|such[ \t]+that|s\.t\.|st)[ \t]*:?", priority = 10)]
    SubjectTo,

    /// Bounds section header
    #[regex(r"(?i)bounds?", priority = 10)]
    Bounds,

    /// Generals section header
    #[regex(r"(?i)generals?|gen", priority = 10)]
    Generals,

    /// Integers section header
    #[regex(r"(?i)integers?", priority = 10)]
    Integers,

    /// Binaries section header
    #[regex(r"(?i)binar(y|ies)|bin", priority = 10)]
    Binaries,

    /// Semi-continuous section header
    #[regex(r"(?i)semi-continuous|semis?", priority = 10)]
    SemiContinuous,

    /// SOS section header
    #[regex(r"(?i)sos", priority = 10)]
    Sos,

    /// End marker
    #[regex(r"(?i)end", priority = 10)]
    End,

    /// Free variable keyword
    #[regex(r"(?i)free", priority = 10)]
    Free,

    /// SOS type S1
    #[regex(r"(?i)s1", |_| SOSType::S1, priority = 10)]
    /// SOS type S2
    #[regex(r"(?i)s2", |_| SOSType::S2, priority = 10)]
    SosType(SOSType),

    // === Numbers and Infinity ===
    /// Positive infinity
    #[regex(r"(?i)\+?inf(inity)?", |_| f64::INFINITY, priority = 9)]
    /// Negative infinity
    #[regex(r"(?i)-inf(inity)?", |_| f64::NEG_INFINITY, priority = 9)]
    Infinity(f64),

    /// Numeric value (integer, float, scientific notation)
    /// Matches: 42, 3.14, .5, 1e5, 1.5e-3
    /// Note: Leading +/- are separate tokens to correctly parse expressions like "+4z"
    #[regex(r"([0-9]+\.?[0-9]*|[0-9]*\.[0-9]+)([eE][+-]?[0-9]+)?", parse_number, priority = 8)]
    Number(f64),

    // === Operators ===
    /// Less than or equal
    #[token("<=")]
    Lte,

    /// Greater than or equal
    #[token(">=")]
    Gte,

    /// Less than
    #[token("<")]
    Lt,

    /// Greater than
    #[token(">")]
    Gt,

    /// Equals
    #[token("=")]
    Eq,

    /// Plus sign
    #[token("+")]
    Plus,

    /// Minus sign
    #[token("-")]
    Minus,

    /// Single colon
    #[token(":")]
    Colon,

    /// Double colon (for SOS constraints)
    #[token("::")]
    DoubleColon,

    // === Structural ===
    /// Newline (significant for some parsing contexts)
    #[token("\n")]
    Newline,

    /// Block comment: \* ... *\
    #[regex(r"\\\*[^*]*\*\\")]
    BlockComment,

    /// Line comment: \ ...
    #[regex(r"\\[^\n*][^\n]*", allow_greedy = true)]
    LineComment,

    // === Identifiers ===
    /// Variable/constraint name identifier
    /// Allowed characters: alphanumeric and !#$%&()_,.;?@\{}~'
    #[regex(r"[a-zA-Z_!#$%&(),.;?@\\{}~']([a-zA-Z0-9_!#$%&(),.;?@\\{}~'|>]|-[a-zA-Z0-9_!#$%&(),.;?@\\{}~'|>])*", |lex| lex.slice(), priority = 5)]
    Identifier(&'input str),
}

#[allow(clippy::unnecessary_wraps)] // logos callback signature requires Option return
fn parse_number<'input>(lex: &logos::Lexer<'input, Token<'input>>) -> Option<f64> {
    let slice = lex.slice();
    let value = slice.parse::<f64>().unwrap_or_else(|_| {
        debug_assert!(false, "Logos regex matched '{slice}' but f64 parse failed - regex and parser are out of sync");
        f64::NAN
    });
    debug_assert!(!value.is_nan(), "parse_number produced NaN from '{slice}' - this indicates a regex/parser mismatch");
    Some(value)
}

impl Token<'_> {
    /// Convert comparison tokens to `ComparisonOp`
    #[must_use]
    pub const fn as_comparison_op(&self) -> Option<ComparisonOp> {
        match self {
            Token::Lte => Some(ComparisonOp::LTE),
            Token::Gte => Some(ComparisonOp::GTE),
            Token::Lt => Some(ComparisonOp::LT),
            Token::Gt => Some(ComparisonOp::GT),
            Token::Eq => Some(ComparisonOp::EQ),
            _ => None,
        }
    }

    /// Check if token is a comparison operator
    #[must_use]
    pub const fn is_comparison_op(&self) -> bool {
        matches!(self, Token::Lte | Token::Gte | Token::Lt | Token::Gt | Token::Eq)
    }
}

/// A spanned token containing position information
pub type Spanned<Tok, Loc, Error> = Result<(Loc, Tok, Loc), Error>;

/// Lexer adapter for LALRPOP
pub struct Lexer<'input> {
    inner: logos::Lexer<'input, Token<'input>>,
}

impl<'input> Lexer<'input> {
    /// Create a new lexer for the given input
    #[must_use]
    pub fn new(input: &'input str) -> Self {
        Self { inner: Token::lexer(input) }
    }
}

impl<'input> Iterator for Lexer<'input> {
    type Item = Spanned<Token<'input>, usize, LexerError>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let token = self.inner.next()?;
            let span = self.inner.span();

            match token {
                Ok(Token::BlockComment | Token::LineComment | Token::Newline) => {
                    // Skip comments and newlines in the token stream
                }
                Ok(tok) => return Some(Ok((span.start, tok, span.end))),
                Err(e) => return Some(Err(e)),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use test_case::test_case;

    use super::*;
    use crate::lp::LpProblemParser;

    fn tokenize(input: &str) -> Vec<Token<'_>> {
        Lexer::new(input).filter_map(Result::ok).map(|(_, tok, _)| tok).collect()
    }

    fn tokenize_with_positions(input: &str) -> Vec<(usize, Token<'_>, usize)> {
        Lexer::new(input).filter_map(Result::ok).collect()
    }

    fn tokenize_raw(input: &str) -> Vec<Option<Token<'_>>> {
        Token::lexer(input).map(|r| r.ok()).collect()
    }

    #[test]
    fn test_sense_keywords() {
        let cases = [
            ("minimize", Sense::Minimize),
            ("MINIMIZE", Sense::Minimize),
            ("Minimize", Sense::Minimize),
            ("minimise", Sense::Minimize),
            ("minimum", Sense::Minimize),
            ("min", Sense::Minimize),
            ("MIN", Sense::Minimize),
            ("maximize", Sense::Maximize),
            ("MAXIMIZE", Sense::Maximize),
            ("Maximize", Sense::Maximize),
            ("maximise", Sense::Maximize),
            ("maximum", Sense::Maximize),
            ("max", Sense::Maximize),
            ("MAX", Sense::Maximize),
        ];

        for (input, expected) in cases {
            let tokens = tokenize(input);
            assert_eq!(tokens, vec![Token::SenseKw(expected)], "Failed for input: {input}");
        }
    }

    #[test]
    fn test_section_keywords() {
        assert_eq!(tokenize("subject to"), vec![Token::SubjectTo]);
        assert_eq!(tokenize("SUBJECT TO"), vec![Token::SubjectTo]);
        assert_eq!(tokenize("Subject To"), vec![Token::SubjectTo]);
        assert_eq!(tokenize("such that"), vec![Token::SubjectTo]);
        assert_eq!(tokenize("s.t."), vec![Token::SubjectTo]);
        assert_eq!(tokenize("st"), vec![Token::SubjectTo]);
        assert_eq!(tokenize("st:"), vec![Token::SubjectTo]);

        assert_eq!(tokenize("bounds"), vec![Token::Bounds]);
        assert_eq!(tokenize("bound"), vec![Token::Bounds]);
        assert_eq!(tokenize("BOUNDS"), vec![Token::Bounds]);

        assert_eq!(tokenize("generals"), vec![Token::Generals]);
        assert_eq!(tokenize("general"), vec![Token::Generals]);
        assert_eq!(tokenize("gen"), vec![Token::Generals]);

        assert_eq!(tokenize("integers"), vec![Token::Integers]);
        assert_eq!(tokenize("integer"), vec![Token::Integers]);

        assert_eq!(tokenize("binaries"), vec![Token::Binaries]);
        assert_eq!(tokenize("binary"), vec![Token::Binaries]);
        assert_eq!(tokenize("bin"), vec![Token::Binaries]);

        assert_eq!(tokenize("semi-continuous"), vec![Token::SemiContinuous]);
        assert_eq!(tokenize("semis"), vec![Token::SemiContinuous]);
        assert_eq!(tokenize("semi"), vec![Token::SemiContinuous]);

        assert_eq!(tokenize("sos"), vec![Token::Sos]);
        assert_eq!(tokenize("SOS"), vec![Token::Sos]);

        assert_eq!(tokenize("end"), vec![Token::End]);
        assert_eq!(tokenize("END"), vec![Token::End]);

        assert_eq!(tokenize("free"), vec![Token::Free]);
        assert_eq!(tokenize("FREE"), vec![Token::Free]);
    }

    #[test]
    fn test_sos_types() {
        assert_eq!(tokenize("S1"), vec![Token::SosType(SOSType::S1)]);
        assert_eq!(tokenize("s1"), vec![Token::SosType(SOSType::S1)]);
        assert_eq!(tokenize("S2"), vec![Token::SosType(SOSType::S2)]);
        assert_eq!(tokenize("s2"), vec![Token::SosType(SOSType::S2)]);
    }

    #[test]
    fn test_numbers() {
        // Integers
        assert_eq!(tokenize("42"), vec![Token::Number(42.0)]);
        assert_eq!(tokenize("0"), vec![Token::Number(0.0)]);
        // Signs are separate tokens to correctly parse expressions like "+4z"
        assert_eq!(tokenize("+42"), vec![Token::Plus, Token::Number(42.0)]);
        assert_eq!(tokenize("-42"), vec![Token::Minus, Token::Number(42.0)]);

        // Floats
        assert_eq!(tokenize("3.25"), vec![Token::Number(3.25)]);
        assert_eq!(tokenize("0.5"), vec![Token::Number(0.5)]);
        assert_eq!(tokenize(".5"), vec![Token::Number(0.5)]);
        assert_eq!(tokenize("123."), vec![Token::Number(123.0)]);

        // Scientific notation
        assert_eq!(tokenize("1e5"), vec![Token::Number(100_000.0)]);
        assert_eq!(tokenize("1E5"), vec![Token::Number(100_000.0)]);
        assert_eq!(tokenize("1.5e3"), vec![Token::Number(1500.0)]);
        assert_eq!(tokenize("1.5E-3"), vec![Token::Number(0.0015)]);
        assert_eq!(tokenize("2.5e+10"), vec![Token::Number(25_000_000_000.0)]);
    }

    #[test]
    fn test_infinity() {
        assert_eq!(tokenize("inf"), vec![Token::Infinity(f64::INFINITY)]);
        assert_eq!(tokenize("INF"), vec![Token::Infinity(f64::INFINITY)]);
        assert_eq!(tokenize("Inf"), vec![Token::Infinity(f64::INFINITY)]);
        assert_eq!(tokenize("infinity"), vec![Token::Infinity(f64::INFINITY)]);
        assert_eq!(tokenize("INFINITY"), vec![Token::Infinity(f64::INFINITY)]);
        assert_eq!(tokenize("+inf"), vec![Token::Infinity(f64::INFINITY)]);
        assert_eq!(tokenize("+infinity"), vec![Token::Infinity(f64::INFINITY)]);
        assert_eq!(tokenize("-inf"), vec![Token::Infinity(f64::NEG_INFINITY)]);
        assert_eq!(tokenize("-INF"), vec![Token::Infinity(f64::NEG_INFINITY)]);
        assert_eq!(tokenize("-infinity"), vec![Token::Infinity(f64::NEG_INFINITY)]);
    }

    #[test]
    fn test_operators() {
        assert_eq!(tokenize("<="), vec![Token::Lte]);
        assert_eq!(tokenize(">="), vec![Token::Gte]);
        assert_eq!(tokenize("<"), vec![Token::Lt]);
        assert_eq!(tokenize(">"), vec![Token::Gt]);
        assert_eq!(tokenize("="), vec![Token::Eq]);
        assert_eq!(tokenize("+"), vec![Token::Plus]);
        assert_eq!(tokenize("-"), vec![Token::Minus]);
        assert_eq!(tokenize(":"), vec![Token::Colon]);
        assert_eq!(tokenize("::"), vec![Token::DoubleColon]);
    }

    #[test]
    fn test_identifiers() {
        assert_eq!(tokenize("x1"), vec![Token::Identifier("x1")]);
        assert_eq!(tokenize("variable_name"), vec![Token::Identifier("variable_name")]);
        assert_eq!(tokenize("x_123"), vec![Token::Identifier("x_123")]);
        assert_eq!(tokenize("XyZ"), vec![Token::Identifier("XyZ")]);
        assert_eq!(tokenize("_variable"), vec![Token::Identifier("_variable")]);

        // Special characters allowed in LP identifiers
        assert_eq!(tokenize("var!name"), vec![Token::Identifier("var!name")]);
        assert_eq!(tokenize("x#1"), vec![Token::Identifier("x#1")]);
    }

    #[test_case("a" => vec![Token::Identifier("a")] ; "letter_a")]
    #[test_case("Z" => vec![Token::Identifier("Z")] ; "letter_Z")]
    #[test_case("_" => vec![Token::Identifier("_")] ; "underscore")]
    #[test_case("!" => vec![Token::Identifier("!")] ; "exclamation")]
    #[test_case("#" => vec![Token::Identifier("#")] ; "hash")]
    #[test_case("$" => vec![Token::Identifier("$")] ; "dollar")]
    #[test_case("%" => vec![Token::Identifier("%")] ; "percent")]
    #[test_case("&" => vec![Token::Identifier("&")] ; "ampersand")]
    #[test_case("(" => vec![Token::Identifier("(")] ; "open_paren")]
    #[test_case(")" => vec![Token::Identifier(")")] ; "close_paren")]
    #[test_case("," => vec![Token::Identifier(",")] ; "comma")]
    #[test_case("." => vec![Token::Identifier(".")] ; "dot")]
    #[test_case(";" => vec![Token::Identifier(";")] ; "semicolon")]
    #[test_case("?" => vec![Token::Identifier("?")] ; "question")]
    #[test_case("@" => vec![Token::Identifier("@")] ; "at")]
    #[test_case("{" => vec![Token::Identifier("{")] ; "open_brace")]
    #[test_case("}" => vec![Token::Identifier("}")] ; "close_brace")]
    #[test_case("~" => vec![Token::Identifier("~")] ; "tilde")]
    #[test_case("'" => vec![Token::Identifier("'")] ; "apostrophe")]
    fn test_valid_start_chars(input: &str) -> Vec<Token<'_>> {
        tokenize(input)
    }

    #[test_case("x0" => vec![Token::Identifier("x0")] ; "digit_0")]
    #[test_case("x9" => vec![Token::Identifier("x9")] ; "digit_9")]
    #[test_case("x|" => vec![Token::Identifier("x|")] ; "pipe")]
    #[test_case("x>" => vec![Token::Identifier("x>")] ; "gt_continuation")]
    #[test_case("x!" => vec![Token::Identifier("x!")] ; "excl_cont")]
    #[test_case("x#" => vec![Token::Identifier("x#")] ; "hash_cont")]
    #[test_case("x$" => vec![Token::Identifier("x$")] ; "dollar_cont")]
    #[test_case("x%" => vec![Token::Identifier("x%")] ; "percent_cont")]
    #[test_case("x&" => vec![Token::Identifier("x&")] ; "ampersand_cont")]
    #[test_case("x(" => vec![Token::Identifier("x(")] ; "open_paren_cont")]
    #[test_case("x)" => vec![Token::Identifier("x)")] ; "close_paren_cont")]
    #[test_case("x," => vec![Token::Identifier("x,")] ; "comma_cont")]
    #[test_case("x." => vec![Token::Identifier("x.")] ; "dot_cont")]
    #[test_case("x;" => vec![Token::Identifier("x;")] ; "semicolon_cont")]
    #[test_case("x?" => vec![Token::Identifier("x?")] ; "question_cont")]
    #[test_case("x@" => vec![Token::Identifier("x@")] ; "at_cont")]
    #[test_case("x{" => vec![Token::Identifier("x{")] ; "open_brace_cont")]
    #[test_case("x}" => vec![Token::Identifier("x}")] ; "close_brace_cont")]
    #[test_case("x~" => vec![Token::Identifier("x~")] ; "tilde_cont")]
    #[test_case("x'" => vec![Token::Identifier("x'")] ; "apostrophe_cont")]
    #[test_case("x_" => vec![Token::Identifier("x_")] ; "underscore_cont")]
    fn test_valid_continuation_chars(input: &str) -> Vec<Token<'_>> {
        tokenize(input)
    }

    #[test_case("0" ; "zero_is_number")]
    #[test_case("9" ; "nine_is_number")]
    fn test_digit_not_identifier_start(input: &str) {
        let tokens = tokenize_raw(input);
        assert!(!tokens.iter().any(|t| matches!(t, Some(Token::Identifier(_)))), "digit should not produce identifier: {tokens:?}");
    }

    #[test]
    fn test_digit_prefix_splits() {
        // "0abc" → Number(0) + Identifier("abc")
        let tokens = tokenize("0abc");
        assert_eq!(tokens, vec![Token::Number(0.0), Token::Identifier("abc")]);
    }

    #[test_case("x-y", vec![Token::Identifier("x-y")] ; "simple_hyphen")]
    #[test_case("a-b-c", vec![Token::Identifier("a-b-c")] ; "double_hyphen_chain")]
    #[test_case("x-1", vec![Token::Identifier("x-1")] ; "hyphen_digit")]
    #[test_case("x->", vec![Token::Identifier("x->")] ; "hyphen_gt")]
    #[test_case("x-|", vec![Token::Identifier("x-|")] ; "hyphen_pipe")]
    fn test_hyphen_valid(input: &str, expected: Vec<Token<'_>>) {
        assert_eq!(tokenize(input), expected);
    }

    #[test]
    fn test_trailing_hyphen() {
        // "abc-" → Identifier("abc") + Minus
        let tokens = tokenize("abc-");
        assert_eq!(tokens, vec![Token::Identifier("abc"), Token::Minus]);
    }

    #[test]
    fn test_double_hyphen() {
        // "a--b" → Identifier("a") + Minus + Minus + Identifier("b")
        let tokens = tokenize("a--b");
        assert_eq!(tokens, vec![Token::Identifier("a"), Token::Minus, Token::Minus, Token::Identifier("b")]);
    }

    #[test_case("*" ; "asterisk")]
    #[test_case("/" ; "slash")]
    #[test_case("^" ; "caret")]
    #[test_case("[" ; "open_bracket")]
    #[test_case("]" ; "close_bracket")]
    #[test_case("\"" ; "double_quote")]
    fn test_invalid_start_chars(input: &str) {
        let tokens = tokenize_raw(input);
        assert!(!tokens.iter().any(|t| matches!(t, Some(Token::Identifier(_)))), "should not produce identifier for {input:?}: {tokens:?}");
    }

    #[test]
    fn test_lt_not_identifier() {
        assert_eq!(tokenize("<"), vec![Token::Lt]);
    }

    #[test_case("x*y", vec![Token::Identifier("x")], "*" ; "asterisk_breaks")]
    #[test_case("x/y", vec![Token::Identifier("x")], "/" ; "slash_breaks")]
    #[test_case("x^y", vec![Token::Identifier("x")], "^" ; "caret_breaks")]
    #[test_case("x[y", vec![Token::Identifier("x")], "[" ; "bracket_breaks")]
    fn test_invalid_char_breaks_identifier(input: &str, expected_prefix: Vec<Token<'_>>, _invalid: &str) {
        let tokens = tokenize(input);
        assert_eq!(
            &tokens[..expected_prefix.len()],
            &expected_prefix,
            "identifier should stop before invalid char in {input:?}: {tokens:?}"
        );
    }

    #[test_case("x!#$%&" => vec![Token::Identifier("x!#$%&")] ; "mixed_specials_1")]
    #[test_case("_(),.;?" => vec![Token::Identifier("_(),.;?")] ; "mixed_specials_2")]
    #[test_case("a@{}~'" => vec![Token::Identifier("a@{}~'")] ; "mixed_specials_3")]
    #[test_case("var|>" => vec![Token::Identifier("var|>")] ; "pipe_gt_continuation")]
    fn test_multi_char_mixed_specials(input: &str) -> Vec<Token<'_>> {
        tokenize(input)
    }

    #[test]
    fn test_all_letters_and_underscore_as_single_char_identifiers() {
        for c in ('a'..='z').chain('A'..='Z').chain(std::iter::once('_')) {
            let s = String::from(c);
            let tokens = tokenize_raw(&s);
            // Some letters match keywords (e.g. "s" doesn't, but groups like "gen" do)
            // We just verify no panics and at least one token is produced
            assert!(!tokens.is_empty(), "should produce at least one token for '{c}'");
        }
    }

    #[test]
    fn test_long_identifier() {
        let long = "x".repeat(10_000);
        let tokens = tokenize(&long);
        assert_eq!(tokens, vec![Token::Identifier(long.as_str())]);
    }

    #[test_case("minimize2" => vec![Token::Identifier("minimize2")] ; "minimize_with_digit")]
    #[test_case("maxx" => vec![Token::Identifier("maxx")] ; "max_with_extra_letter")]
    #[test_case("binary1" => vec![Token::Identifier("binary1")] ; "binary_with_digit")]
    fn test_keyword_like_prefixes(input: &str) -> Vec<Token<'_>> {
        tokenize(input)
    }

    #[test]
    fn test_gt_standalone_is_gt_token() {
        assert_eq!(tokenize(">"), vec![Token::Gt]);
    }

    #[test]
    fn test_x_gt_is_single_identifier() {
        assert_eq!(tokenize("x>"), vec![Token::Identifier("x>")]);
    }

    #[test]
    fn test_pipe_standalone_is_error() {
        let tokens = tokenize_raw("|");
        assert_eq!(tokens, vec![None], "standalone | should be a lexer error");
    }

    #[test]
    fn test_lt_breaks_identifier() {
        // `<` is not in the continuation set
        assert_eq!(tokenize("x<y"), vec![Token::Identifier("x"), Token::Lt, Token::Identifier("y")]);
    }

    #[test]
    fn test_operators_not_consumed_by_identifier() {
        assert_eq!(tokenize("x+y"), vec![Token::Identifier("x"), Token::Plus, Token::Identifier("y")]);
        assert_eq!(tokenize("x-"), vec![Token::Identifier("x"), Token::Minus]);
        assert_eq!(tokenize("x:y"), vec![Token::Identifier("x"), Token::Colon, Token::Identifier("y")]);
        assert_eq!(tokenize("x=y"), vec![Token::Identifier("x"), Token::Eq, Token::Identifier("y")]);
    }

    #[test]
    fn test_backslash_is_identifier() {
        // Standalone `\` matches the identifier regex (backslash is in the allowed set)
        let tokens = tokenize_raw("\\");
        assert_eq!(tokens, vec![Some(Token::Identifier("\\"))]);
    }

    #[test]
    fn test_comments_skipped() {
        // Block comments
        assert_eq!(tokenize(r"\* this is a comment *\"), Vec::<Token>::new());

        // Line comments
        assert_eq!(tokenize(r"\ this is a line comment"), Vec::<Token>::new());

        // Mixed with tokens
        let tokens = tokenize(r"\* comment *\ minimize");
        assert_eq!(tokens, vec![Token::SenseKw(Sense::Minimize)]);
    }

    #[test]
    fn test_constraint_line() {
        let tokens = tokenize("c1: 2 x1 + 3 x2 <= 10");
        assert_eq!(
            tokens,
            vec![
                Token::Identifier("c1"),
                Token::Colon,
                Token::Number(2.0),
                Token::Identifier("x1"),
                Token::Plus,
                Token::Number(3.0),
                Token::Identifier("x2"),
                Token::Lte,
                Token::Number(10.0),
            ]
        );
    }

    #[test]
    fn test_objective_line() {
        let tokens = tokenize("obj: -1 x1 + 2.5 x2");
        assert_eq!(
            tokens,
            vec![
                Token::Identifier("obj"),
                Token::Colon,
                Token::Minus,
                Token::Number(1.0),
                Token::Identifier("x1"),
                Token::Plus,
                Token::Number(2.5),
                Token::Identifier("x2"),
            ]
        );
    }

    #[test]
    fn test_sos_constraint() {
        let tokens = tokenize("csos1: S1:: V1:1 V3:2");
        assert_eq!(
            tokens,
            vec![
                Token::Identifier("csos1"),
                Token::Colon,
                Token::SosType(SOSType::S1),
                Token::DoubleColon,
                Token::Identifier("V1"),
                Token::Colon,
                Token::Number(1.0),
                Token::Identifier("V3"),
                Token::Colon,
                Token::Number(2.0),
            ]
        );
    }

    #[test]
    fn test_bounds_line() {
        let tokens = tokenize("0 <= x1 <= 10");
        assert_eq!(tokens, vec![Token::Number(0.0), Token::Lte, Token::Identifier("x1"), Token::Lte, Token::Number(10.0),]);

        let tokens = tokenize("-inf <= x2 <= +inf");
        assert_eq!(
            tokens,
            vec![Token::Infinity(f64::NEG_INFINITY), Token::Lte, Token::Identifier("x2"), Token::Lte, Token::Infinity(f64::INFINITY),]
        );

        let tokens = tokenize("x1 free");
        assert_eq!(tokens, vec![Token::Identifier("x1"), Token::Free,]);
    }

    #[test]
    fn test_full_lp_tokenization() {
        let input = r"
\* test problem *\
maximize
obj: x1 + 2 x2
subject to
c1: x1 + x2 <= 10
bounds
0 <= x1 <= 5
binary
x2
end
";
        let tokens = tokenize(input);

        // Just check that we get expected token types
        assert!(tokens.contains(&Token::SenseKw(Sense::Maximize)));
        assert!(tokens.contains(&Token::SubjectTo));
        assert!(tokens.contains(&Token::Bounds));
        assert!(tokens.contains(&Token::Binaries));
        assert!(tokens.contains(&Token::End));
    }

    #[test]
    fn test_minimal_constraint() {
        let input = "minimize\nx1\nsubject to\nc1: x1 <= 1\nend";
        let tokens = tokenize_with_positions(input);
        for (start, tok, end) in &tokens {
            println!("({start:3}, {end:3}): {tok:?}");
        }
        // Check specific tokens
        assert!(tokens.iter().any(|(_, t, _)| matches!(t, Token::SenseKw(Sense::Minimize))));
        assert!(tokens.iter().any(|(_, t, _)| matches!(t, Token::SubjectTo)));
        assert!(tokens.iter().any(|(_, t, _)| matches!(t, Token::Identifier("c1"))));
        assert!(tokens.iter().any(|(_, t, _)| matches!(t, Token::Colon)));
        assert!(tokens.iter().any(|(_, t, _)| matches!(t, Token::Identifier("x1"))));
        assert!(tokens.iter().any(|(_, t, _)| matches!(t, Token::Lte)));
    }

    #[test]
    fn test_parse_simple() {
        // Test with just objective, no constraints
        let input = "minimize\nx1\nsubject to\nend";
        let lexer = Lexer::new(input);
        let parser = LpProblemParser::new();
        let result = parser.parse(lexer);
        println!("Simple parse result: {result:?}");
        assert!(result.is_ok(), "Simple parse failed: {result:?}");
    }

    #[test]
    fn test_parse_named_objective() {
        // Test with named objective
        let input = "minimize\nobj: x1\nsubject to\nend";
        let lexer = Lexer::new(input);
        let parser = LpProblemParser::new();
        let result = parser.parse(lexer);
        println!("Named objective result: {result:?}");
        assert!(result.is_ok(), "Named objective failed: {result:?}");
    }

    #[test]
    fn test_parse_with_constraint() {
        // Test with named constraint
        let input = "minimize\nobj: x1\nsubject to\nc1: x1 <= 1\nend";
        let lexer = Lexer::new(input);
        let parser = LpProblemParser::new();
        let result = parser.parse(lexer);
        println!("With constraint result: {result:?}");
        assert!(result.is_ok(), "With constraint failed: {result:?}");
    }

    #[test]
    fn test_parse_unnamed_obj_with_constraint() {
        // Test with UNNAMED objective and named constraint - this is what minimal_parse uses
        let input = "minimize\nx1\nsubject to\nc1: x1 <= 1\nend";
        println!("Input: {input:?}");
        let tokens = tokenize_with_positions(input);
        for (start, tok, end) in &tokens {
            println!("  ({start:3}, {end:3}): {tok:?}");
        }

        let lexer = Lexer::new(input);
        let parser = LpProblemParser::new();
        let result = parser.parse(lexer);
        println!("Unnamed obj + constraint result: {result:?}");
        assert!(result.is_ok(), "Unnamed obj + constraint failed: {result:?}");
    }
}
