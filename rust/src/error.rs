use thiserror::Error;

/// This error type provides detailed context about parsing failures,
/// including location information and specific error conditions.
#[derive(Error, Debug, Clone, PartialEq, Eq)]
pub enum LpParseError {
    /// Invalid or malformed constraint syntax
    #[error("Invalid constraint syntax at position {position}: {context}")]
    ConstraintSyntax { position: usize, context: String },

    /// Invalid or malformed objective syntax
    #[error("Invalid objective syntax at position {position}: {context}")]
    ObjectiveSyntax { position: usize, context: String },

    /// Unknown or invalid variable type specification
    #[error("Unknown variable type '{var_type}' for variable '{variable}'")]
    UnknownVariableType { variable: String, var_type: String },

    /// Reference to an undefined variable
    #[error("Undefined variable '{variable}' referenced in {context}")]
    UndefinedVariable { variable: String, context: String },

    /// Duplicate definition of a component
    #[error("Duplicate {component_type} '{name}' defined")]
    DuplicateDefinition { component_type: String, name: String },

    /// Invalid numerical value or format
    #[error("Invalid number format '{value}' at position {position}")]
    InvalidNumber { value: String, position: usize },

    /// Missing required section in LP file
    #[error("Missing required section: {section}")]
    MissingSection { section: String },

    /// Invalid bound specification
    #[error("Invalid bounds for variable '{variable}': {details}")]
    InvalidBounds { variable: String, details: String },

    /// Invalid SOS constraint specification
    #[error("Invalid SOS constraint '{constraint}': {details}")]
    InvalidSosConstraint { constraint: String, details: String },

    /// Validation error for logical consistency
    #[error("Validation error: {message}")]
    ValidationError { message: String },

    /// Generic parsing error with context
    #[error("Parse error at position {position}: {message}")]
    ParseError { position: usize, message: String },

    /// File I/O related errors
    #[error("File I/O error: {message}")]
    IoError { message: String },

    /// Internal parser state errors
    #[error("Internal parser error: {message}")]
    InternalError { message: String },
}

impl LpParseError {
    /// Create a new constraint syntax error
    pub fn constraint_syntax(position: usize, context: impl Into<String>) -> Self {
        Self::ConstraintSyntax { position, context: context.into() }
    }

    /// Create a new objective syntax error
    pub fn objective_syntax(position: usize, context: impl Into<String>) -> Self {
        Self::ObjectiveSyntax { position, context: context.into() }
    }

    /// Create a new unknown variable type error
    pub fn unknown_variable_type(variable: impl Into<String>, var_type: impl Into<String>) -> Self {
        Self::UnknownVariableType { variable: variable.into(), var_type: var_type.into() }
    }

    /// Create a new undefined variable error
    pub fn undefined_variable(variable: impl Into<String>, context: impl Into<String>) -> Self {
        Self::UndefinedVariable { variable: variable.into(), context: context.into() }
    }

    /// Create a new duplicate definition error
    pub fn duplicate_definition(component_type: impl Into<String>, name: impl Into<String>) -> Self {
        Self::DuplicateDefinition { component_type: component_type.into(), name: name.into() }
    }

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

    /// Create a new invalid SOS constraint error
    pub fn invalid_sos_constraint(constraint: impl Into<String>, details: impl Into<String>) -> Self {
        Self::InvalidSosConstraint { constraint: constraint.into(), details: details.into() }
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

    /// Create a new internal error
    pub fn internal_error(message: impl Into<String>) -> Self {
        Self::InternalError { message: message.into() }
    }
}

/// Convert from LALRPOP parsing errors to our custom error type.
impl<'input> From<lalrpop_util::ParseError<usize, crate::lexer::Token<'input>, crate::lexer::LexerError>> for LpParseError {
    fn from(err: lalrpop_util::ParseError<usize, crate::lexer::Token<'input>, crate::lexer::LexerError>) -> Self {
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
            lalrpop_util::ParseError::User { error } => Self::parse_error(0, format!("Lexer error: {error:?}")),
        }
    }
}

/// Convert from standard I/O errors
impl From<std::io::Error> for LpParseError {
    fn from(err: std::io::Error) -> Self {
        Self::io_error(err.to_string())
    }
}

/// Convert from boxed errors (used by CSV module)
impl From<Box<dyn std::error::Error>> for LpParseError {
    fn from(err: Box<dyn std::error::Error>) -> Self {
        Self::io_error(err.to_string())
    }
}

/// Result type alias for LP parsing operations
pub type LpResult<T> = Result<T, LpParseError>;

/// Context extension trait for adding location information to errors
pub trait ErrorContext<T> {
    /// Add position context to an error
    ///
    /// # Errors
    ///
    /// Propagates the original error with updated position information
    fn with_position(self, position: usize) -> LpResult<T>;

    /// Add general context to an error
    ///
    /// # Errors
    ///
    /// Propagates the original error with added context message
    fn with_context(self, context: &str) -> LpResult<T>;
}

impl<T> ErrorContext<T> for Result<T, LpParseError> {
    fn with_position(self, position: usize) -> Self {
        self.map_err(|mut err| {
            if let LpParseError::ParseError { position: ref mut pos, .. } = &mut err {
                *pos = position;
            }
            err
        })
    }

    fn with_context(self, context: &str) -> Self {
        self.map_err(|err| match err {
            LpParseError::ParseError { position, message } => LpParseError::parse_error(position, format!("{context}: {message}")),
            other => other,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_creation() {
        let err = LpParseError::constraint_syntax(42, "missing operator");
        assert_eq!(err.to_string(), "Invalid constraint syntax at position 42: missing operator");
    }

    #[test]
    fn test_error_context() {
        let result: LpResult<()> = Err(LpParseError::parse_error(10, "test error"));
        let with_context = result.with_context("parsing constraint");

        assert!(with_context.is_err());
        assert!(with_context.unwrap_err().to_string().contains("parsing constraint"));
    }

    #[test]
    fn test_io_error_conversion() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
        let lp_err: LpParseError = io_err.into();

        match lp_err {
            LpParseError::IoError { message } => assert!(message.contains("file not found")),
            _ => panic!("Expected IoError"),
        }
    }
}
