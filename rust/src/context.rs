use std::collections::HashMap;

use crate::error::{LpParseError, LpResult};

/// Represents the current section being parsed
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SectionType {
    /// Problem name and comments section
    Header,
    /// Optimisation sense (minimise/maximise)
    Sense,
    /// Objectives section
    Objectives,
    /// Constraints section
    Constraints,
    /// Variable bounds section
    Bounds,
    /// Integer variables section
    Integers,
    /// General variables section
    Generals,
    /// Binary variables section
    Binaries,
    /// Semi-continuous variables section
    SemiContinuous,
    /// SOS constraints section
    Sos,
    /// End of file
    End,
}

impl std::fmt::Display for SectionType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Header => write!(f, "Header"),
            Self::Sense => write!(f, "Sense"),
            Self::Objectives => write!(f, "Objectives"),
            Self::Constraints => write!(f, "Constraints"),
            Self::Bounds => write!(f, "Bounds"),
            Self::Integers => write!(f, "Integers"),
            Self::Generals => write!(f, "Generals"),
            Self::Binaries => write!(f, "Binaries"),
            Self::SemiContinuous => write!(f, "SemiContinuous"),
            Self::Sos => write!(f, "SOS"),
            Self::End => write!(f, "End"),
        }
    }
}

/// Warning generated during parsing
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseWarning {
    /// The section where the warning occurred
    pub section: SectionType,
    /// Position in the input where the warning occurred
    pub position: usize,
    /// Line number (1-indexed)
    pub line: usize,
    /// Column number (1-indexed)
    pub column: usize,
    /// Warning message
    pub message: String,
}

impl ParseWarning {
    /// Create a new parse warning
    pub fn new(section: SectionType, position: usize, line: usize, column: usize, message: impl Into<String>) -> Self {
        Self { section, position, line, column, message: message.into() }
    }
}

impl std::fmt::Display for ParseWarning {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Warning in {} section at line {}, column {}: {}", self.section, self.line, self.column, self.message)
    }
}

/// Parser metrics for performance monitoring
#[derive(Debug, Default, Clone)]
pub struct ParseMetrics {
    /// Number of objectives parsed
    pub objectives_count: usize,
    /// Number of constraints parsed
    pub constraints_count: usize,
    /// Number of variables encountered
    pub variables_count: usize,
    /// Time spent parsing each section (in nanoseconds)
    pub section_times: HashMap<SectionType, u64>,
    /// Total parsing time (in nanoseconds)
    pub total_time: u64,
}

impl ParseMetrics {
    #[must_use]
    /// Create new metrics instance
    pub fn new() -> Self {
        Self::default()
    }

    /// Record time spent in a section
    pub fn record_section_time(&mut self, section: SectionType, duration_ns: u64) {
        self.section_times.insert(section, duration_ns);
    }

    #[must_use]
    #[allow(clippy::cast_precision_loss)]
    /// Get formatted performance summary
    pub fn summary(&self) -> String {
        format!(
            "Parsed {} objectives, {} constraints, {} variables in {:.2}ms",
            self.objectives_count,
            self.constraints_count,
            self.variables_count,
            self.total_time as f64 / 1_000_000.0
        )
    }
}

/// Parsing configuration options
#[derive(Debug, Clone)]
pub struct ParseConfig {
    /// Whether to collect detailed metrics
    pub collect_metrics: bool,
    /// Whether to perform strict validation during parsing
    pub strict_validation: bool,
    /// Whether to collect warnings
    pub collect_warnings: bool,
    /// Maximum number of warnings to collect before stopping
    pub max_warnings: usize,
}

impl Default for ParseConfig {
    fn default() -> Self {
        Self { collect_metrics: false, strict_validation: false, collect_warnings: true, max_warnings: 100 }
    }
}

/// Context for stateful parsing operations
#[derive(Debug)]
pub struct ParseContext<'a> {
    /// Original input string
    original_input: &'a str,
    /// Current position in the input
    position: usize,
    /// Current line number (1-indexed)
    line: usize,
    /// Current column number (1-indexed)
    column: usize,
    /// Current section being parsed
    current_section: SectionType,
    /// Collected warnings during parsing
    warnings: Vec<ParseWarning>,
    /// Parsing metrics
    metrics: ParseMetrics,
    /// Parsing configuration
    config: ParseConfig,
}

impl<'a> ParseContext<'a> {
    #[must_use]
    /// Create a new parsing context
    pub fn new(input: &'a str) -> Self {
        Self::with_config(input, ParseConfig::default())
    }

    #[must_use]
    /// Create a new parsing context with specific configuration
    pub fn with_config(input: &'a str, config: ParseConfig) -> Self {
        Self {
            original_input: input,
            position: 0,
            line: 1,
            column: 1,
            current_section: SectionType::Header,
            warnings: Vec::new(),
            metrics: ParseMetrics::new(),
            config,
        }
    }

    #[must_use]
    /// Get the current input slice from the current position
    pub fn current_input(&self) -> &'a str {
        &self.original_input[self.position..]
    }

    #[must_use]
    /// Get the original input
    pub const fn original_input(&self) -> &'a str {
        self.original_input
    }

    #[must_use]
    /// Get the current position
    pub const fn position(&self) -> usize {
        self.position
    }

    #[must_use]
    /// Get the current line number
    pub const fn line(&self) -> usize {
        self.line
    }

    #[must_use]
    /// Get the current column number
    pub const fn column(&self) -> usize {
        self.column
    }

    #[must_use]
    /// Get the current section
    pub const fn current_section(&self) -> SectionType {
        self.current_section
    }

    #[must_use]
    /// Get the collected warnings
    pub fn warnings(&self) -> &[ParseWarning] {
        &self.warnings
    }

    #[must_use]
    /// Get the parsing metrics
    pub const fn metrics(&self) -> &ParseMetrics {
        &self.metrics
    }

    /// Get mutable access to metrics
    pub fn metrics_mut(&mut self) -> &mut ParseMetrics {
        &mut self.metrics
    }

    /// Update the current position and recalculate line/column
    ///
    /// # Errors
    ///
    /// Returns an error if the new position is before the current position
    pub fn update_position(&mut self, new_position: usize) -> LpResult<()> {
        if new_position < self.position {
            return Err(LpParseError::internal_error("Cannot move position backwards"));
        }

        // Update line and column based on the consumed text
        let consumed = &self.original_input[self.position..new_position];
        for ch in consumed.chars() {
            if ch == '\n' {
                self.line += 1;
                self.column = 1;
            } else {
                self.column += 1;
            }
        }

        self.position = new_position;
        Ok(())
    }

    /// Consume input and update position
    ///
    /// # Errors
    ///
    /// Returns an error if the consumed string doesn't match the current input
    pub fn consume(&mut self, consumed_str: &str) -> LpResult<()> {
        let consumed_len = consumed_str.len();
        if consumed_len == 0 {
            return Ok(());
        }

        // Verify the consumed string matches the current input
        let current = self.current_input();
        if !current.starts_with(consumed_str) {
            return Err(LpParseError::internal_error(format!(
                "Attempted to consume '{}' but current input starts with '{}'",
                consumed_str,
                &current[..consumed_len.min(50)]
            )));
        }

        self.update_position(self.position + consumed_len)
    }

    /// Set the current section
    pub fn set_section(&mut self, section: SectionType) {
        self.current_section = section;
    }

    /// Add a warning to the context
    pub fn add_warning(&mut self, message: impl Into<String>) {
        if self.config.collect_warnings && self.warnings.len() < self.config.max_warnings {
            let warning = ParseWarning::new(self.current_section, self.position, self.line, self.column, message);
            self.warnings.push(warning);
        }
    }

    #[must_use]
    /// Check if we should stop collecting warnings
    pub fn should_stop_collecting_warnings(&self) -> bool {
        self.warnings.len() >= self.config.max_warnings
    }

    /// Get a context-aware error with position information
    pub fn error(&self, message: impl Into<String>) -> LpParseError {
        LpParseError::parse_error(
            self.position,
            format!("Error in {} section at line {}, column {}: {}", self.current_section, self.line, self.column, message.into()),
        )
    }

    /// Mark the start of parsing a section
    pub fn start_section(&mut self, section: SectionType) -> std::time::Instant {
        self.set_section(section);
        std::time::Instant::now()
    }

    /// Mark the end of parsing a section
    pub fn end_section(&mut self, section: SectionType, start_time: std::time::Instant) {
        if self.config.collect_metrics {
            let duration = start_time.elapsed();
            self.metrics.record_section_time(section, u64::try_from(duration.as_nanos()).unwrap_or(u64::MAX));
        }
    }

    #[must_use]
    /// Get summary of the parsing context
    pub fn summary(&self) -> String {
        let mut parts = Vec::new();

        parts.push(format!("Position: {}:{}", self.line, self.column));
        parts.push(format!("Section: {}", self.current_section));

        if !self.warnings.is_empty() {
            parts.push(format!("Warnings: {}", self.warnings.len()));
        }

        if self.config.collect_metrics {
            parts.push(self.metrics.summary());
        }

        parts.join(", ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_context_creation() {
        let input = "test input";
        let context = ParseContext::new(input);

        assert_eq!(context.original_input(), input);
        assert_eq!(context.position(), 0);
        assert_eq!(context.line(), 1);
        assert_eq!(context.column(), 1);
        assert_eq!(context.current_section(), SectionType::Header);
    }

    #[test]
    fn test_position_update() {
        let input = "line 1\nline 2\nline 3";
        let mut context = ParseContext::new(input);

        // Move to position 7 (start of "line 2")
        context.update_position(7).unwrap();
        assert_eq!(context.line(), 2);
        assert_eq!(context.column(), 1);

        // Move to position 10 (middle of "line 2")
        context.update_position(10).unwrap();
        assert_eq!(context.line(), 2);
        assert_eq!(context.column(), 4);
    }

    #[test]
    fn test_consume() {
        let input = "hello world";
        let mut context = ParseContext::new(input);

        context.consume("hello").unwrap();
        assert_eq!(context.position(), 5);
        assert_eq!(context.current_input(), " world");

        context.consume(" ").unwrap();
        assert_eq!(context.position(), 6);
        assert_eq!(context.current_input(), "world");
    }

    #[test]
    fn test_warnings() {
        let input = "test";
        let mut context = ParseContext::new(input);

        context.add_warning("test warning");
        assert_eq!(context.warnings().len(), 1);
        assert_eq!(context.warnings()[0].message, "test warning");
        assert_eq!(context.warnings()[0].section, SectionType::Header);
    }

    #[test]
    fn test_section_tracking() {
        let input = "test";
        let mut context = ParseContext::new(input);

        context.set_section(SectionType::Objectives);
        assert_eq!(context.current_section(), SectionType::Objectives);

        context.add_warning("objective warning");
        assert_eq!(context.warnings()[0].section, SectionType::Objectives);
    }

    #[test]
    fn test_metrics() {
        let input = "test";
        let config = ParseConfig { collect_metrics: true, ..Default::default() };
        let mut context = ParseContext::with_config(input, config);

        let start = context.start_section(SectionType::Objectives);
        std::thread::sleep(std::time::Duration::from_millis(1));
        context.end_section(SectionType::Objectives, start);

        assert!(context.metrics().section_times.contains_key(&SectionType::Objectives));
    }
}
