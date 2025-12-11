//! Lexer for LP files using Logos.
//!
//! This module provides a token-based lexer for Linear Programming files,
//! handling case-insensitive keywords, numbers, identifiers, and operators.

use logos::Logos;

use crate::model::{Coefficient, ComparisonOp, SOSType, Sense};

/// Lexer error type
#[derive(Debug, Clone, PartialEq, Default)]
pub struct LexerError;

/// Helper enum for parsing SOS entries in the grammar
#[derive(Debug, Clone, PartialEq)]
pub enum SosEntryKind<'input> {
    /// SOS constraint header: name and type
    Header(&'input str, SOSType),
    /// SOS weight: variable and weight value
    Weight(Coefficient<'input>),
}

/// Helper enum for constraint continuation parsing
#[derive(Debug, Clone, PartialEq)]
pub enum ConstraintCont<'input> {
    /// Named constraint: the leading identifier was the constraint name
    Named(Vec<Coefficient<'input>>, ComparisonOp, f64),
    /// Unnamed constraint: the leading identifier was the first variable
    Unnamed(Vec<(f64, Coefficient<'input>)>, ComparisonOp, f64),
}

impl<'input> ConstraintCont<'input> {
    /// Convert to a Constraint, using the given identifier as either name or first variable
    #[must_use]
    pub fn into_constraint(self, id: &'input str) -> crate::model::Constraint<'input> {
        use std::borrow::Cow;
        match self {
            ConstraintCont::Named(coeffs, op, rhs) => {
                crate::model::Constraint::Standard { name: Cow::Borrowed(id), coefficients: coeffs, operator: op, rhs }
            }
            ConstraintCont::Unnamed(rest, op, rhs) => {
                let mut coeffs = vec![Coefficient { name: id, value: 1.0 }];
                for (s, c) in rest {
                    coeffs.push(Coefficient { name: c.name, value: s * c.value });
                }
                crate::model::Constraint::Standard { name: Cow::Borrowed("__c__"), coefficients: coeffs, operator: op, rhs }
            }
        }
    }
}

/// Helper enum for optional sections that can appear in any order
#[derive(Debug, Clone, PartialEq)]
pub enum OptionalSection<'input> {
    Bounds(Vec<(&'input str, crate::model::VariableType)>),
    Generals(Vec<&'input str>),
    Integers(Vec<&'input str>),
    Binaries(Vec<&'input str>),
    SemiContinuous(Vec<&'input str>),
    SOS(Vec<crate::model::Constraint<'input>>),
}

impl std::fmt::Display for LexerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
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
    #[regex(r"(\d+\.?\d*|\d*\.\d+)([eE][+-]?\d+)?", parse_number, priority = 8)]
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
    #[regex(r"[a-zA-Z_!#$%&(),.;?@\\{}~'][a-zA-Z0-9_!#$%&(),.;?@\\{}~']*", |lex| lex.slice(), priority = 5)]
    Identifier(&'input str),
}

fn parse_number<'input>(lex: &mut logos::Lexer<'input, Token<'input>>) -> Option<f64> {
    lex.slice().parse().ok()
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
    use super::*;

    fn tokenize(input: &str) -> Vec<Token<'_>> {
        Lexer::new(input).filter_map(std::result::Result::ok).map(|(_, tok, _)| tok).collect()
    }

    fn tokenize_with_positions(input: &str) -> Vec<(usize, Token<'_>, usize)> {
        Lexer::new(input).filter_map(std::result::Result::ok).collect()
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
        use crate::lp::LpProblemParser;

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
        use crate::lp::LpProblemParser;

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
        use crate::lp::LpProblemParser;

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
        use crate::lp::LpProblemParser;

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
