use thiserror::Error;

use crate::lexer::{LexerError, Token};

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

    /// Validation error for logical consistency
    #[error("Validation error: {message}")]
    ValidationError {
        /// Description of the consistency violation.
        message: String,
    },

    /// Generic parsing error with context
    #[error("Parse error at position {position}: {message}")]
    ParseError {
        /// Byte or line position where parsing failed.
        position: usize,
        /// Description of the failure.
        message: String,
    },

    /// File I/O related errors
    #[error("File I/O error: {message}")]
    IoError {
        /// The underlying I/O error, rendered as text to keep the type `Clone`.
        message: String,
    },
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

    /// Create a new validation error
    pub fn validation_error(message: impl Into<String>) -> Self {
        Self::ValidationError { message: message.into() }
    }

    /// Create a new parse error
    pub fn parse_error(position: usize, message: impl Into<String>) -> Self {
        Self::ParseError { position, message: message.into() }
    }

    /// Create a new I/O error
    pub fn io_error(message: impl Into<String>) -> Self {
        Self::IoError { message: message.into() }
    }
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
}
