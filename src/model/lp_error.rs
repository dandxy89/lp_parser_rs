use std::io::Error;

#[derive(Debug, thiserror::Error)]
pub enum LPParserError {
    #[error("Invalid comparison operator: {0}")]
    ComparisonError(String),

    #[error("Invalid constraint format: {0}")]
    ConstraintError(String),

    #[error("Failed to parse file: {0}")]
    FileParseError(String),

    #[error("Failed to parse float: {0}")]
    FloatParseError(String),

    #[error("IO error: {0}")]
    IOError(#[from] Error),

    #[error("Invalid float: {0}")]
    RHSParseError(String),

    #[error("Invalid SOS class: {0}")]
    SOSError(String),
}
