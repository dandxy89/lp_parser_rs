#[derive(Debug, thiserror::Error)]
pub enum LPParserError {
    #[error("Failed to parse file: {0}")]
    FileParseError(String),

    #[error("Invalid constraint format: {0}")]
    ConstraintError(String),

    #[error("Invalid SOS class: {0}")]
    SOSError(String),

    #[error("Invalid comparison operator: {0}")]
    ComparisonError(String),

    #[error("IO error: {0}")]
    IOError(#[from] std::io::Error),

    #[error("Failed to parse float: {0}")]
    FloatParseError(String),

    #[error("Invalid float: {0}")]
    RHSParseError(String),
}
