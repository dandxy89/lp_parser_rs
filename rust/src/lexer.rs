//! Lexer for LP files using Logos.
//!
//! This module provides a token-based lexer for Linear Programming files,
//! handling case-insensitive keywords, numbers, identifiers, and operators.
//!
//! # Context-sensitive keywords
//!
//! Keywords are only recognised where they can start a section or play their
//! grammatical role; elsewhere the same word lexes as an identifier, so
//! variables and constraints may be called `min`, `bin`, `s1`, `free`, ...:
//!
//! - the sense keyword (`minimize`, `max`, ...) only as the first token;
//! - `subject to` / `st` / `s.t.` only once, as the first token of a line;
//! - other section keywords (`bounds`, `generals`, `binaries`, `sos`, `end`,
//!   ...) only as the first token of a line that does not continue an
//!   expression (the previous token is not a sign, colon or comparison) and
//!   is not followed by `:` or `::` (which would make it a label);
//! - `S1` / `S2` only in `name: S1::`;
//! - `free` only directly after an identifier on the same line (`x free`).
//!
//! Remaining reserved words: `inf` / `infinity` always lex as infinity, a lone
//! `[` or `]` is always a quadratic-term bracket (names may still contain
//! brackets, as in `x[1]`, but a bracket that opens or closes a quadratic block
//! must be separated from the neighbouring name by whitespace), and a
//! section keyword alone at the start of a line (e.g. a variable named `bin`
//! listed on its own line in a `generals` section) is read as a section
//! header. The multi-word `subject to` / `such that`, `lazy constraints`,
//! `user cuts` and `general constraints` (and its variants) are always
//! keywords; the single-word `genconstrs` follows the section-keyword rule.

use std::borrow::Cow;
use std::fmt::{Display, Formatter, Result as FmtResult};
use std::ops::Range;

use logos::Logos;

use crate::model::{ComparisonOp, GeneralFunction, SOSType, Sense, VariableType};

/// Lexer error type, also used for semantic errors raised while assembling
/// objective/constraint bodies (see [`crate::assemble`]).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LexerError {
    /// Byte position of the error in the input, when known.
    pub position: usize,
    /// Human-readable description; `None` for plain tokenisation failures.
    pub message: Option<String>,
}

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
        /// Constraint name (borrowed, or owned when auto-generated).
        name: Cow<'input, str>,
        /// Left-hand-side coefficients.
        coefficients: Vec<RawCoefficient<'input>>,
        /// Comparison operator between the LHS and the RHS.
        operator: ComparisonOp,
        /// Right-hand-side value.
        rhs: f64,
        /// Byte offset of the constraint in the source text, if tracked.
        byte_offset: Option<usize>,
    },
    /// A special ordered set constraint.
    SOS {
        /// Constraint name (borrowed, or owned when auto-generated).
        name: Cow<'input, str>,
        /// SOS type (S1 or S2).
        sos_type: SOSType,
        /// Weight per participating variable.
        weights: Vec<RawCoefficient<'input>>,
        /// Byte offset of the constraint in the source text, if tracked.
        byte_offset: Option<usize>,
    },
    /// A Gurobi general constraint: `resultant = FUNCTION ( arguments )`.
    General {
        /// Constraint name (borrowed, or owned when auto-generated).
        name: Cow<'input, str>,
        /// The variable the function's value is assigned to.
        resultant: &'input str,
        /// The function and its arguments.
        function: GeneralFunction<&'input str>,
        /// Byte offset of the constraint in the source text, if tracked.
        byte_offset: Option<usize>,
    },
    /// A quadratic constraint: linear coefficients plus quadratic terms.
    Quadratic {
        /// Constraint name (borrowed, or owned when auto-generated).
        name: Cow<'input, str>,
        /// Linear left-hand-side coefficients.
        coefficients: Vec<RawCoefficient<'input>>,
        /// Quadratic left-hand-side terms.
        quadratic: Vec<RawQuadraticTerm<'input>>,
        /// Comparison operator between the LHS and the RHS.
        operator: ComparisonOp,
        /// Right-hand-side value.
        rhs: f64,
        /// Byte offset of the constraint in the source text, if tracked.
        byte_offset: Option<usize>,
    },
    /// An indicator constraint: `variable = value -> linear constraint`.
    Indicator {
        /// Constraint name (borrowed, or owned when auto-generated).
        name: Cow<'input, str>,
        /// The (binary) indicator variable.
        variable: &'input str,
        /// Whether the constraint is active when the variable is 1 (`true`) or 0.
        active_value: bool,
        /// Left-hand-side coefficients of the linear constraint.
        coefficients: Vec<RawCoefficient<'input>>,
        /// Comparison operator of the linear constraint.
        operator: ComparisonOp,
        /// Right-hand-side value of the linear constraint.
        rhs: f64,
        /// Byte offset of the constraint in the source text, if tracked.
        byte_offset: Option<usize>,
    },
}

impl RawConstraint<'_> {
    /// The constraint's name (`"__c__"` when unnamed).
    #[must_use]
    pub fn name(&self) -> &str {
        match self {
            Self::Standard { name, .. }
            | Self::SOS { name, .. }
            | Self::Indicator { name, .. }
            | Self::Quadratic { name, .. }
            | Self::General { name, .. } => name,
        }
    }
}

/// Raw quadratic term produced by the grammar (zero-copy): `coefficient *
/// var1 * var2`, with any `/ 2` of an objective block already applied.
#[derive(Debug, Clone, PartialEq)]
pub struct RawQuadraticTerm<'input> {
    /// First variable name.
    pub var1: &'input str,
    /// Second variable name (equal to `var1` for a square).
    pub var2: &'input str,
    /// Coefficient of the product.
    pub coefficient: f64,
}

/// Raw objective produced by the grammar (zero-copy).
#[derive(Debug, Clone, PartialEq)]
pub struct RawObjective<'input> {
    /// Objective name (may be a sentinel like `"__obj__"` if unnamed).
    pub name: Cow<'input, str>,
    /// Coefficients of the objective function.
    pub coefficients: Vec<RawCoefficient<'input>>,
    /// Quadratic terms of the objective function.
    pub quadratic: Vec<RawQuadraticTerm<'input>>,
    /// Constant term of the objective function.
    pub constant: f64,
    /// Byte offset of this objective in the source text (for line number mapping).
    pub byte_offset: Option<usize>,
}

/// Helper enum for parsing SOS entries in the grammar.
#[derive(Debug, Clone, PartialEq)]
pub enum SosEntryKind<'input> {
    /// SOS constraint header: name, type, and byte offset.
    Header(&'input str, SOSType, usize),
    /// SOS weight: variable and weight value.
    Weight(RawCoefficient<'input>),
}

/// Helper enum for optional sections that can appear in any order.
#[derive(Debug, Clone, PartialEq)]
pub enum OptionalSection<'input> {
    /// `Bounds` section: variable name and its bound-derived type.
    Bounds(Vec<(&'input str, VariableType)>),
    /// `Generals` section: general integer variable names.
    Generals(Vec<&'input str>),
    /// `Integers` section: integer variable names.
    Integers(Vec<&'input str>),
    /// `Binaries` section: binary variable names.
    Binaries(Vec<&'input str>),
    /// `Semi-Continuous` section: semi-continuous variable names.
    SemiContinuous(Vec<&'input str>),
    /// `SOS` section: special ordered set constraints.
    SOS(Vec<RawConstraint<'input>>),
    /// `Lazy Constraints` section (CPLEX): an unassembled constraint body.
    Lazy(Vec<crate::assemble::SpannedElem<'input>>),
    /// `User Cuts` section (CPLEX): an unassembled constraint body.
    UserCuts(Vec<crate::assemble::SpannedElem<'input>>),
    /// `General Constraints` section (Gurobi): an unassembled body.
    GeneralConstraints(Vec<crate::assemble::SpannedElem<'input>>),
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
    /// Raw constraints from `Lazy Constraints` sections (LP) or the
    /// `LAZYCONS` section (MPS).
    pub lazy_constraints: Vec<RawConstraint<'input>>,
    /// Raw constraints from `User Cuts` sections (LP) or the `USERCUTS`
    /// section (MPS).
    pub user_cuts: Vec<RawConstraint<'input>>,
}

impl Display for LexerError {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        f.write_str(self.message.as_deref().unwrap_or("lexer error"))
    }
}

/// Tokens for LP file parsing
#[derive(Logos, Debug, Clone, PartialEq)]
#[logos(skip r"[ \t\r]+")] // Skip horizontal whitespace (not newlines)
#[logos(error = LexerError)]
pub enum Token<'input> {
    // === Keywords (case-insensitive) ===
    /// Optimisation sense: minimize
    #[regex(r"(?i)minimize|minimise|minimum|min", |_| Sense::Minimize, priority = 10)]
    /// Optimisation sense: maximize
    #[regex(r"(?i)maximize|maximise|maximum|max", |_| Sense::Maximize, priority = 10)]
    SenseKw(Sense),

    /// Subject to / constraints header
    #[regex(r"(?i)subject[ \t]+to|such[ \t]+that|s\.t\.|st", priority = 10)]
    SubjectTo,

    /// Lazy constraints section header (CPLEX)
    #[regex(r"(?i)lazy[ \t]+constraints", priority = 10)]
    LazyConstraints,

    /// User cuts section header (CPLEX)
    #[regex(r"(?i)user[ \t]+cuts", priority = 10)]
    UserCuts,

    /// General constraints section header (Gurobi)
    #[regex(r"(?i)general[ \t]+constraints?|general[ \t]+constrs?|gen[ \t]+cons|genconstrs?", priority = 10)]
    GeneralConstraints,

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
    /// Infinity. Signs are separate tokens (like numbers) so that `-inflow`
    /// lexes as `-` and the identifier `inflow`, not `-inf` followed by `low`.
    #[regex(r"(?i)inf(inity)?", |_| f64::INFINITY, priority = 9)]
    Infinity(f64),

    /// Numeric value (integer, float, scientific notation)
    /// Matches: 42, 3.14, .5, 1e5, 1.5e-3
    /// Note: Leading +/- are separate tokens to correctly parse expressions like "+4z"
    #[regex(r"([0-9]+\.?[0-9]*|[0-9]*\.[0-9]+)([eE][+-]?[0-9]+)?", parse_number, priority = 8)]
    Number(f64),

    // === Operators ===
    /// Less than or equal (CPLEX accepts both `<=` and `=<`)
    #[token("<=")]
    #[token("=<")]
    Lte,

    /// Greater than or equal (CPLEX accepts both `>=` and `=>`)
    #[token(">=")]
    #[token("=>")]
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

    /// Opening bracket of a quadratic block (`[ x ^ 2 + 2 x * y ]`).
    #[token("[", priority = 10)]
    LBracket,

    /// Closing bracket of a quadratic block.
    #[token("]", priority = 10)]
    RBracket,

    /// Exponent of a squared quadratic term (`x ^ 2`).
    #[token("^")]
    Caret,

    /// Product of two variables in a quadratic term (`x * y`).
    #[token("*")]
    Star,

    /// Division of an objective's quadratic block (`[ ... ] / 2`).
    #[token("/")]
    Slash,

    /// Implication arrow of an indicator constraint (`b = 1 -> x <= 3`).
    /// A `-` directly followed by `>` never occurs in a linear expression, so
    /// this is unambiguous.
    #[token("->")]
    Implies,

    // === Structural ===
    /// Newline (significant for some parsing contexts)
    #[token("\n")]
    Newline,

    /// Block comment: \* ... *\ (may contain `*` and `\` that do not close it)
    #[regex(r"\\\*([^*]|\*+[^*\\])*\*+\\")]
    BlockComment,

    /// Line comment: \ ... (a lone `\` before a newline is an empty comment)
    #[regex(r"\\[^\n*][^\n]*", allow_greedy = true)]
    #[token("\\")]
    LineComment,

    // === Identifiers ===
    /// Variable/constraint name identifier
    /// Allowed characters: alphanumeric and !#$%&()_,.;?@{}~'[]
    /// (`\` is excluded: per the CPLEX spec it starts a comment anywhere on a line)
    ///
    /// `>` is only accepted mid-name when followed by a non-numeric name
    /// character (Gurobi writes names like `ArcFlow%>%[0]`), so `y>=3` and
    /// `y>3` still lex as a comparison rather than as a variable `y>`.
    #[regex(r"[a-zA-Z_!#$%&(),.;?@{}~'\[\]]([a-zA-Z0-9_!#$%&(),.;?@{}~'|\[\]]|-[a-zA-Z0-9_!#$%&(),.;?@{}~'|\[\]]|>[a-zA-Z_!#$%&(),;?@{}~'|\[\]])*", |lex| lex.slice(), priority = 5)]
    Identifier(&'input str),
}

/// Parse a slice matched by the number regex.
fn parse_number<'input>(lex: &logos::Lexer<'input, Token<'input>>) -> Option<f64> {
    let slice = lex.slice();
    let Ok(value) = slice.parse::<f64>() else {
        debug_assert!(false, "Logos regex matched '{slice}' but f64 parse failed - regex and parser are out of sync");
        return None;
    };
    if value.is_nan() {
        debug_assert!(false, "parse_number produced NaN from '{slice}' - this indicates a regex/parser mismatch");
        return None;
    }
    Some(value)
}

/// A spanned token containing position information
pub type Spanned<Tok, Loc, Error> = Result<(Loc, Tok, Loc), Error>;

/// A raw token from the underlying Logos lexer with its span, and whether a
/// line break separates it from the previous significant token.
type RawItem<'input> = (Result<Token<'input>, LexerError>, Range<usize>, bool);

/// Lexer adapter for LALRPOP.
///
/// Skips comments and newlines, and resolves keywords that are also valid
/// names by context (see the module documentation).
pub struct Lexer<'input> {
    inner: logos::Lexer<'input, Token<'input>>,
    input: &'input str,
    /// One significant token of lookahead (`None` also once input is
    /// exhausted: the Logos lexer keeps returning `None` at the end).
    peeked: Option<RawItem<'input>>,
    /// The last token handed to the parser.
    prev: Option<Token<'input>>,
    /// Whether the `subject to` header has been emitted.
    seen_subject_to: bool,
}

impl<'input> Lexer<'input> {
    /// Create a new lexer for the given input
    #[must_use]
    pub fn new(input: &'input str) -> Self {
        Self { inner: Token::lexer(input), input, peeked: None, prev: None, seen_subject_to: false }
    }

    /// Next significant raw token, skipping comments and newlines.
    fn raw_next(&mut self) -> Option<RawItem<'input>> {
        let mut newline_before = false;
        loop {
            let token = self.inner.next()?;
            let span = self.inner.span();
            match token {
                Ok(Token::Newline) => newline_before = true,
                Ok(Token::BlockComment) => newline_before |= self.inner.slice().contains('\n'),
                Ok(Token::LineComment) => {}
                other => return Some((other, span, newline_before)),
            }
        }
    }

    fn take_next(&mut self) -> Option<RawItem<'input>> {
        self.peeked.take().or_else(|| self.raw_next())
    }

    /// Whether the next significant token is on the same line and satisfies `pred`.
    fn peek_same_line(&mut self, pred: impl FnOnce(&Token<'input>) -> bool) -> bool {
        if self.peeked.is_none() {
            self.peeked = self.raw_next();
        }
        matches!(&self.peeked, Some((Ok(tok), _, false)) if pred(tok))
    }

    /// Whether the previous token leaves an expression or label unfinished,
    /// so the next word must be an operand rather than a section keyword.
    const fn prev_continues_expression(&self) -> bool {
        matches!(
            self.prev,
            Some(
                Token::Plus
                    | Token::Minus
                    | Token::Colon
                    | Token::DoubleColon
                    | Token::Lte
                    | Token::Gte
                    | Token::Lt
                    | Token::Gt
                    | Token::Eq
                    | Token::Implies
                    | Token::LBracket
                    | Token::Caret
                    | Token::Star
                    | Token::Slash
            )
        )
    }

    /// Resolve a keyword token by context: either keep it, or demote it to an
    /// identifier spelled as in the source.
    fn resolve_keyword(&mut self, tok: Token<'input>, span: &Range<usize>, at_line_start: bool) -> Token<'input> {
        let keep = match tok {
            Token::SenseKw(_) => self.prev.is_none(),
            Token::SubjectTo => {
                // Multi-word forms cannot be identifiers; leave them to the parser.
                let multi_word = self.input[span.clone()].contains([' ', '\t']);
                multi_word || (!self.seen_subject_to && at_line_start && !self.prev_continues_expression())
            }
            Token::GeneralConstraints if self.input[span.clone()].contains([' ', '\t']) => true,
            Token::Bounds
            | Token::Generals
            | Token::Integers
            | Token::Binaries
            | Token::SemiContinuous
            | Token::Sos
            | Token::End
            | Token::GeneralConstraints => {
                at_line_start
                    && !self.prev_continues_expression()
                    && !self.peek_same_line(|t| matches!(t, Token::Colon | Token::DoubleColon))
            }
            Token::SosType(_) => matches!(self.prev, Some(Token::Colon)) && self.peek_same_line(|t| matches!(t, Token::DoubleColon)),
            Token::Free => !at_line_start && matches!(self.prev, Some(Token::Identifier(_))),
            _ => return tok,
        };

        if keep {
            if matches!(tok, Token::SubjectTo) {
                self.seen_subject_to = true;
                // `Subject To:` -- the optional trailing colon belongs to the header.
                if self.peek_same_line(|t| matches!(t, Token::Colon)) {
                    self.peeked = None;
                }
            }
            tok
        } else {
            let slice = &self.input[span.clone()];
            debug_assert!(!slice.is_empty(), "keyword token must have a non-empty slice");
            Token::Identifier(slice)
        }
    }
}

impl<'input> Iterator for Lexer<'input> {
    type Item = Spanned<Token<'input>, usize, LexerError>;

    fn next(&mut self) -> Option<Self::Item> {
        let (token, span, newline_before) = self.take_next()?;
        let at_line_start = newline_before || self.prev.is_none();
        match token {
            Ok(tok) => {
                let tok = self.resolve_keyword(tok, &span, at_line_start);
                self.prev = Some(tok.clone());
                Some(Ok((span.start, tok, span.end)))
            }
            Err(mut e) => {
                // Logos errors carry no location: point at the offending text.
                e.position = span.start;
                if e.message.is_none() {
                    e.message = Some(format!("unrecognised token '{}'", &self.input[span]));
                }
                Some(Err(e))
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
        Token::lexer(input).map(Result::ok).collect()
    }

    /// Raw Logos tokens (no context-sensitive keyword resolution), for
    /// testing the keyword regexes in isolation.
    fn tokenize_keywords(input: &str) -> Vec<Token<'_>> {
        Token::lexer(input).filter_map(Result::ok).collect()
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
        assert_eq!(tokenize_keywords("subject to"), vec![Token::SubjectTo]);
        assert_eq!(tokenize_keywords("SUBJECT TO"), vec![Token::SubjectTo]);
        assert_eq!(tokenize_keywords("Subject To"), vec![Token::SubjectTo]);
        assert_eq!(tokenize_keywords("such that"), vec![Token::SubjectTo]);
        assert_eq!(tokenize_keywords("s.t."), vec![Token::SubjectTo]);
        assert_eq!(tokenize_keywords("st"), vec![Token::SubjectTo]);
        assert_eq!(tokenize("st:"), vec![Token::SubjectTo]);
        assert_eq!(tokenize("Subject To:"), vec![Token::SubjectTo]);

        assert_eq!(tokenize_keywords("lazy constraints"), vec![Token::LazyConstraints]);
        assert_eq!(tokenize_keywords("Lazy  Constraints"), vec![Token::LazyConstraints]);
        assert_eq!(tokenize_keywords("USER CUTS"), vec![Token::UserCuts]);
        for header in ["General Constraints", "general constraint", "General Constrs", "Gen Cons", "GenConstrs"] {
            assert_eq!(tokenize_keywords(header), vec![Token::GeneralConstraints], "{header}");
        }
        // The single-word form is a name where a section cannot start.
        assert_eq!(tokenize("x + genconstrs")[2], Token::Identifier("genconstrs"));
        // Each word alone is an ordinary name.
        assert_eq!(tokenize("lazy + cuts"), vec![Token::Identifier("lazy"), Token::Plus, Token::Identifier("cuts")]);

        assert_eq!(tokenize_keywords("bounds"), vec![Token::Bounds]);
        assert_eq!(tokenize_keywords("bound"), vec![Token::Bounds]);
        assert_eq!(tokenize_keywords("BOUNDS"), vec![Token::Bounds]);

        assert_eq!(tokenize_keywords("generals"), vec![Token::Generals]);
        assert_eq!(tokenize_keywords("general"), vec![Token::Generals]);
        assert_eq!(tokenize_keywords("gen"), vec![Token::Generals]);

        assert_eq!(tokenize_keywords("integers"), vec![Token::Integers]);
        assert_eq!(tokenize_keywords("integer"), vec![Token::Integers]);

        assert_eq!(tokenize_keywords("binaries"), vec![Token::Binaries]);
        assert_eq!(tokenize_keywords("binary"), vec![Token::Binaries]);
        assert_eq!(tokenize_keywords("bin"), vec![Token::Binaries]);

        assert_eq!(tokenize_keywords("semi-continuous"), vec![Token::SemiContinuous]);
        assert_eq!(tokenize_keywords("semis"), vec![Token::SemiContinuous]);
        assert_eq!(tokenize_keywords("semi"), vec![Token::SemiContinuous]);

        assert_eq!(tokenize_keywords("sos"), vec![Token::Sos]);
        assert_eq!(tokenize_keywords("SOS"), vec![Token::Sos]);

        assert_eq!(tokenize_keywords("end"), vec![Token::End]);
        assert_eq!(tokenize_keywords("END"), vec![Token::End]);

        assert_eq!(tokenize_keywords("free"), vec![Token::Free]);
        assert_eq!(tokenize_keywords("FREE"), vec![Token::Free]);
    }

    #[test]
    fn test_sos_types() {
        assert_eq!(tokenize_keywords("S1"), vec![Token::SosType(SOSType::S1)]);
        assert_eq!(tokenize_keywords("s1"), vec![Token::SosType(SOSType::S1)]);
        assert_eq!(tokenize_keywords("S2"), vec![Token::SosType(SOSType::S2)]);
        assert_eq!(tokenize_keywords("s2"), vec![Token::SosType(SOSType::S2)]);
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
        assert_eq!(tokenize("+inf"), vec![Token::Plus, Token::Infinity(f64::INFINITY)]);
        assert_eq!(tokenize("+infinity"), vec![Token::Plus, Token::Infinity(f64::INFINITY)]);
        assert_eq!(tokenize("-inf"), vec![Token::Minus, Token::Infinity(f64::INFINITY)]);
        assert_eq!(tokenize("-INF"), vec![Token::Minus, Token::Infinity(f64::INFINITY)]);
        assert_eq!(tokenize("-infinity"), vec![Token::Minus, Token::Infinity(f64::INFINITY)]);
    }

    #[test]
    fn test_infinity_prefix_is_identifier() {
        assert_eq!(tokenize("-inflow"), vec![Token::Minus, Token::Identifier("inflow")]);
        assert_eq!(tokenize("+infeasible"), vec![Token::Plus, Token::Identifier("infeasible")]);
        assert_eq!(tokenize("inflow"), vec![Token::Identifier("inflow")]);
    }

    #[test]
    fn test_operators() {
        assert_eq!(tokenize("<="), vec![Token::Lte]);
        assert_eq!(tokenize("=<"), vec![Token::Lte]);
        assert_eq!(tokenize(">="), vec![Token::Gte]);
        assert_eq!(tokenize("=>"), vec![Token::Gte]);
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
    #[test_case("x>%" => vec![Token::Identifier("x>%")] ; "gt_continuation")]
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
    #[test_case("x[" => vec![Token::Identifier("x[")] ; "open_bracket_cont")]
    #[test_case("x]" => vec![Token::Identifier("x]")] ; "close_bracket_cont")]
    fn test_valid_continuation_chars(input: &str) -> Vec<Token<'_>> {
        tokenize(input)
    }

    #[test]
    fn test_bracketed_subscript_identifier() {
        // Names like `ArcFlow%>%[0,0,0.0]` appear in Gurobi-written LP files
        assert_eq!(tokenize("ArcFlow%>%[0,0,0.0]"), vec![Token::Identifier("ArcFlow%>%[0,0,0.0]")]);
        assert_eq!(
            tokenize("Cnst%>%Arc-10-28|1.23%>%[0,0,0.0]:"),
            vec![Token::Identifier("Cnst%>%Arc-10-28|1.23%>%[0,0,0.0]"), Token::Colon,]
        );
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

    #[test_case("x-y", &[Token::Identifier("x-y")] ; "simple_hyphen")]
    #[test_case("a-b-c", &[Token::Identifier("a-b-c")] ; "double_hyphen_chain")]
    #[test_case("x-1", &[Token::Identifier("x-1")] ; "hyphen_digit")]
    #[test_case("x-|", &[Token::Identifier("x-|")] ; "hyphen_pipe")]
    fn test_hyphen_valid(input: &str, expected: &[Token<'_>]) {
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

    #[test_case("x*y", &[Token::Identifier("x")], "*" ; "asterisk_breaks")]
    #[test_case("x/y", &[Token::Identifier("x")], "/" ; "slash_breaks")]
    #[test_case("x^y", &[Token::Identifier("x")], "^" ; "caret_breaks")]
    fn test_invalid_char_breaks_identifier(input: &str, expected_prefix: &[Token<'_>], _invalid: &str) {
        let tokens = tokenize(input);
        assert_eq!(
            &tokens[..expected_prefix.len()],
            expected_prefix,
            "identifier should stop before invalid char in {input:?}: {tokens:?}"
        );
    }

    #[test_case("x!#$%&" => vec![Token::Identifier("x!#$%&")] ; "mixed_specials_1")]
    #[test_case("_(),.;?" => vec![Token::Identifier("_(),.;?")] ; "mixed_specials_2")]
    #[test_case("a@{}~'" => vec![Token::Identifier("a@{}~'")] ; "mixed_specials_3")]
    #[test_case("var|>a" => vec![Token::Identifier("var|>a")] ; "pipe_gt_continuation")]
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
    fn test_trailing_gt_is_comparison() {
        assert_eq!(tokenize("x>"), vec![Token::Identifier("x"), Token::Gt]);
        // `->` is the indicator implication arrow, never a sign and a comparison.
        assert_eq!(tokenize("x->"), vec![Token::Identifier("x"), Token::Implies]);
        assert_eq!(tokenize("b=1->x"), vec![Token::Identifier("b"), Token::Eq, Token::Number(1.0), Token::Implies, Token::Identifier("x")]);
    }

    #[test]
    fn test_gt_operators_not_absorbed_into_identifier() {
        assert_eq!(tokenize("y>=3"), vec![Token::Identifier("y"), Token::Gte, Token::Number(3.0)]);
        assert_eq!(tokenize("y>3"), vec![Token::Identifier("y"), Token::Gt, Token::Number(3.0)]);
        assert_eq!(tokenize("y>.5"), vec![Token::Identifier("y"), Token::Gt, Token::Number(0.5)]);
        assert_eq!(tokenize("y>-3"), vec![Token::Identifier("y"), Token::Gt, Token::Minus, Token::Number(3.0)]);
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
    fn test_backslash_starts_comment() {
        // Per the CPLEX spec, `\` starts a comment anywhere on a line
        let tokens = tokenize_raw("\\");
        assert_eq!(tokens, vec![Some(Token::LineComment)]);

        // Glued to an identifier: the identifier ends and the comment swallows the rest
        assert_eq!(tokenize("x\\y + z"), vec![Token::Identifier("x")]);
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

        // `*` and `\` inside a block comment do not end it
        assert_eq!(tokenize(r"\* 2*3 *\ minimize"), vec![Token::SenseKw(Sense::Minimize)]);
        assert_eq!(tokenize(r"\** a \ b **\ minimize"), vec![Token::SenseKw(Sense::Minimize)]);
        assert_eq!(tokenize("\\* line one *\n * line two *\\ minimize"), vec![Token::SenseKw(Sense::Minimize)]);
        // ... and the comment ends at the first `*\`
        assert_eq!(tokenize(r"\* a *\ x \* b *\"), vec![Token::Identifier("x")]);
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
            vec![
                Token::Minus,
                Token::Infinity(f64::INFINITY),
                Token::Lte,
                Token::Identifier("x2"),
                Token::Lte,
                Token::Plus,
                Token::Infinity(f64::INFINITY),
            ]
        );

        let tokens = tokenize("x1 free");
        assert_eq!(tokens, vec![Token::Identifier("x1"), Token::Free,]);
    }

    #[test]
    fn test_keywords_resolved_by_context() {
        use Token::{Colon, DoubleColon, Identifier, Number, Plus};

        // S1/S2 are only SOS types in `name: S1::`; elsewhere they are names.
        assert_eq!(
            tokenize("sos\n s1: S1:: S1:1 s2:2"),
            vec![
                Token::Sos,
                Identifier("s1"),
                Colon,
                Token::SosType(SOSType::S1),
                DoubleColon,
                Identifier("S1"),
                Colon,
                Number(1.0),
                Identifier("s2"),
                Colon,
                Number(2.0),
            ]
        );
        // `s1::` is a constraint label, not an SOS type.
        assert_eq!(tokenize("st\ns1:: x >= 1")[1..3], [Identifier("s1"), DoubleColon]);

        // Section keywords mid-expression or used as labels are names.
        let tokens = tokenize("min\nobj: min + bin\nst\nbounds: gen + free\n + end >= 1\nbounds\nfree free\nend");
        assert_eq!(
            tokens,
            vec![
                Token::SenseKw(Sense::Minimize),
                Identifier("obj"),
                Colon,
                Identifier("min"),
                Plus,
                Identifier("bin"),
                Token::SubjectTo,
                Identifier("bounds"),
                Colon,
                Identifier("gen"),
                Plus,
                Identifier("free"),
                Plus,
                Identifier("end"),
                Token::Gte,
                Number(1.0),
                Token::Bounds,
                Identifier("free"),
                Token::Free,
                Token::End,
            ]
        );

        // `st` is only the header once.
        assert_eq!(tokenize("st\nst: x <= 1")[..3], [Token::SubjectTo, Identifier("st"), Colon]);
    }

    #[test]
    fn test_lexer_error_reports_its_position() {
        let input = "min\nx\nst\nc1: x >= 1\nc2: x ? 3 | 4\nend";
        let err = Lexer::new(input).find_map(Result::err).expect("'|' must be a lexer error");
        assert_eq!(err.position, input.find('|').unwrap());
        assert_eq!(err.message.as_deref(), Some("unrecognised token '|'"));
    }

    #[test]
    fn test_quadratic_tokens() {
        use Token::{Caret, Identifier, LBracket, Number, Plus, RBracket, Slash, Star};
        assert_eq!(
            tokenize("[ x^2 + 4 x * y ] / 2"),
            vec![
                LBracket,
                Identifier("x"),
                Caret,
                Number(2.0),
                Plus,
                Number(4.0),
                Identifier("x"),
                Star,
                Identifier("y"),
                RBracket,
                Slash,
                Number(2.0)
            ]
        );
        // Brackets inside a name stay part of it.
        assert_eq!(tokenize("x[1] [y]"), vec![Identifier("x[1]"), Identifier("[y]")]);
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
