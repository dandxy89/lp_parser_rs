use thiserror::Error;

use crate::lexer::{LexerError, Token};
use crate::line_index::LineIndex;

/// Kind of named entity referenced by mutation / lookup APIs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntityKind {
    /// An objective function.
    Objective,
    /// A constraint (standard or SOS).
    Constraint,
    /// A decision variable.
    Variable,
}

impl EntityKind {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Objective => "Objective",
            Self::Constraint => "Constraint",
            Self::Variable => "Variable",
        }
    }
}

impl std::fmt::Display for EntityKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// This error type provides detailed context about parsing failures,
/// including location information and specific error conditions.
#[derive(Error, Debug, Clone, PartialEq, Eq)]
pub enum LpParseError {
    /// Invalid numerical value or format
    #[error("Invalid number format '{value}' at position {position}")]
    InvalidNumber {
        /// The text that failed to parse as a number.
        value: String,
        /// Byte or line position of the value in the input.
        position: usize,
    },

    /// Missing required section in LP file
    #[error("Missing required section: {section}")]
    MissingSection {
        /// Name of the section that was expected but not found.
        section: String,
    },

    /// Invalid bound specification
    #[error("Invalid bounds for variable '{variable}': {details}")]
    InvalidBounds {
        /// Name of the variable with the invalid bound.
        variable: String,
        /// Description of what makes the bound invalid.
        details: String,
    },

    /// A named entity was not found.
    #[error("{kind} '{name}' not found")]
    NotFound {
        /// Kind of entity that was missing.
        kind: EntityKind,
        /// Name that was looked up.
        name: String,
    },

    /// A name is already in use.
    #[error("{kind} '{name}' already exists")]
    AlreadyExists {
        /// Kind of entity that already exists.
        kind: EntityKind,
        /// Conflicting name.
        name: String,
    },

    /// Operation is not valid for the target entity.
    #[error("Invalid operation: {message}")]
    InvalidOperation {
        /// Description of why the operation is invalid.
        message: String,
    },

    /// Validation error for logical consistency
    #[error("Validation error: {message}")]
    ValidationError {
        /// Description of the consistency violation.
        message: String,
    },

    /// Generic parsing error with context
    #[error("{}", format_parse_error(.position, .message, .context.as_deref()))]
    ParseError {
        /// Byte or line position where parsing failed.
        position: usize,
        /// Description of the failure.
        message: String,
        /// Optional human-readable source snippet (line/column + caret).
        context: Option<Box<ParseContext>>,
    },

    /// File I/O related errors
    #[error("File I/O error: {message}")]
    IoError {
        /// The underlying I/O error, rendered as text to keep the type `Clone`.
        message: String,
    },
}

/// Source context attached to a parse error for human-readable diagnostics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseContext {
    /// 1-based line number.
    pub line: usize,
    /// 1-based column number.
    pub column: usize,
    /// Formatted multi-line snippet with caret.
    pub snippet: String,
}

impl std::fmt::Display for ParseContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.snippet)
    }
}

// `position` is taken by reference because thiserror's `.field` shorthand in the
// `#[error(...)]` attribute expands to `&self.field`.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn format_parse_error(position: &usize, message: &str, context: Option<&ParseContext>) -> String {
    match context {
        Some(ctx) => format!("Parse error at position {position}: {message}\n{ctx}"),
        None => format!("Parse error at position {position}: {message}"),
    }
}

impl LpParseError {
    /// Create a new invalid number error
    pub fn invalid_number(value: impl Into<String>, position: usize) -> Self {
        Self::InvalidNumber { value: value.into(), position }
    }

    /// Create a new missing section error
    pub fn missing_section(section: impl Into<String>) -> Self {
        Self::MissingSection { section: section.into() }
    }

    /// Create a new invalid bounds error
    pub fn invalid_bounds(variable: impl Into<String>, details: impl Into<String>) -> Self {
        Self::InvalidBounds { variable: variable.into(), details: details.into() }
    }

    /// Create a not-found error for a named entity.
    pub fn not_found(kind: EntityKind, name: impl Into<String>) -> Self {
        Self::NotFound { kind, name: name.into() }
    }

    /// Create an already-exists error for a named entity.
    pub fn already_exists(kind: EntityKind, name: impl Into<String>) -> Self {
        Self::AlreadyExists { kind, name: name.into() }
    }

    /// Create an invalid-operation error.
    pub fn invalid_operation(message: impl Into<String>) -> Self {
        Self::InvalidOperation { message: message.into() }
    }

    /// Create a new validation error
    pub fn validation_error(message: impl Into<String>) -> Self {
        Self::ValidationError { message: message.into() }
    }

    /// Create a new parse error
    pub fn parse_error(position: usize, message: impl Into<String>) -> Self {
        Self::ParseError { position, message: message.into(), context: None }
    }

    /// Create a parse error enriched with line/column context from `source`.
    pub fn parse_error_with_source(position: usize, message: impl Into<String>, source: &str) -> Self {
        let message = message.into();
        let context = build_parse_context(source, position);
        Self::ParseError { position, message, context }
    }

    /// Attach source context to a parse error if it does not already have one.
    #[must_use]
    pub fn with_source(mut self, source: &str) -> Self {
        if let Self::ParseError { position, context, .. } = &mut self
            && context.is_none()
        {
            *context = build_parse_context(source, *position);
        }
        self
    }

    /// Create a new I/O error
    pub fn io_error(message: impl Into<String>) -> Self {
        Self::IoError { message: message.into() }
    }

    /// Byte position associated with this error, when available.
    #[must_use]
    pub const fn position(&self) -> Option<usize> {
        match self {
            Self::InvalidNumber { position, .. } | Self::ParseError { position, .. } => Some(*position),
            _ => None,
        }
    }

    /// Formatted diagnostic string including source snippet when available.
    #[must_use]
    pub fn diagnostic(&self) -> String {
        match self {
            Self::ParseError { position, message, context: Some(ctx) } => {
                format!("Parse error at position {position}: {message}\n{ctx}")
            }
            other => other.to_string(),
        }
    }
}

fn build_parse_context(source: &str, position: usize) -> Option<Box<ParseContext>> {
    let index = LineIndex::new(source);
    let loc = index.location(position)?;
    let snippet = index.format_snippet(source, position)?;
    Some(Box::new(ParseContext { line: loc.line, column: loc.column, snippet }))
}

/// Convert from LALRPOP parsing errors to our custom error type.
impl<'input> From<lalrpop_util::ParseError<usize, Token<'input>, LexerError>> for LpParseError {
    fn from(err: lalrpop_util::ParseError<usize, Token<'input>, LexerError>) -> Self {
        match err {
            lalrpop_util::ParseError::InvalidToken { location } => Self::parse_error(location, "Invalid token"),
            lalrpop_util::ParseError::UnrecognizedEof { location, expected } => {
                let expected_str = if expected.is_empty() { String::new() } else { format!(", expected one of: {}", expected.join(", ")) };
                Self::parse_error(location, format!("Unexpected end of input{expected_str}"))
            }
            lalrpop_util::ParseError::UnrecognizedToken { token: (start, tok, _), expected } => {
                let expected_str = if expected.is_empty() { String::new() } else { format!(", expected one of: {}", expected.join(", ")) };
                Self::parse_error(start, format!("Unexpected token {tok:?}{expected_str}"))
            }
            lalrpop_util::ParseError::ExtraToken { token: (start, tok, _) } => Self::parse_error(start, format!("Extra token {tok:?}")),
            lalrpop_util::ParseError::User { error } => {
                let message = error.message.clone().unwrap_or_else(|| "Lexer error".to_string());
                Self::parse_error(error.position, message)
            }
        }
    }
}

/// Result type alias for LP parsing operations
pub type LpResult<T> = Result<T, LpParseError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_creation() {
        let err = LpParseError::invalid_bounds("x", "lower exceeds upper");
        assert_eq!(err.to_string(), "Invalid bounds for variable 'x': lower exceeds upper");
    }

    #[test]
    fn test_not_found() {
        let err = LpParseError::not_found(EntityKind::Variable, "x1");
        assert_eq!(err.to_string(), "Variable 'x1' not found");
    }

    #[test]
    fn test_parse_error_with_source() {
        let source = "Minimize\n obj: x\nSubject To\n c1: @ bad\nEnd\n";
        let pos = source.find('@').expect("@ present");
        let err = LpParseError::parse_error_with_source(pos, "Unexpected token", source);
        let diag = err.diagnostic();
        assert!(diag.contains("line 4"), "{diag}");
        assert!(diag.contains('^'), "{diag}");
    }
}
