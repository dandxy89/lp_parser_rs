//! Client configuration (section `lp`).

use lp_parser_rs::analysis::AnalysisConfig;
use serde::Deserialize;

/// All `lp.*` settings. Missing fields take their defaults.
#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Config {
    /// `lp.analysis`: mirrors [`AnalysisConfig`].
    pub analysis: AnalysisSettings,
    /// `lp.semantic`: full-parse scheduling.
    pub semantic: SemanticSettings,
    /// `lp.format`: formatter layout.
    pub format: FormatSettings,
    /// `lp.inlayHints`: per-hint toggles.
    pub inlay_hints: InlayHintSettings,
}

/// `lp.analysis`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct AnalysisSettings {
    /// Publish analysis issues as diagnostics.
    pub enabled: bool,
    /// Upstream thresholds (`largeCoefficientThreshold`, ...).
    #[serde(flatten)]
    pub thresholds: AnalysisConfig,
}

impl Default for AnalysisSettings {
    fn default() -> Self {
        Self { enabled: true, thresholds: AnalysisConfig::default() }
    }
}

/// `lp.semantic`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct SemanticSettings {
    /// Above this size the semantic pass only runs on save.
    pub max_file_size_mb: u64,
    /// Delay after the last edit before the semantic pass runs.
    pub debounce_ms: u64,
}

impl Default for SemanticSettings {
    fn default() -> Self {
        Self { max_file_size_mb: 20, debounce_ms: 300 }
    }
}

/// Keyword casing for `lp.format.keywordCase`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum KeywordCase {
    /// Keep keywords as written.
    #[default]
    Preserve,
    /// `subject to`.
    Lower,
    /// `SUBJECT TO`.
    Upper,
    /// `Subject To`.
    Title,
}

/// `lp.format`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct FormatSettings {
    /// Body indent in spaces.
    pub indent: usize,
    /// Wrap expressions longer than this.
    pub line_width: usize,
    /// Align comparison operators within a section.
    pub align_operators: bool,
    /// Keyword casing.
    pub keyword_case: KeywordCase,
}

impl Default for FormatSettings {
    fn default() -> Self {
        Self { indent: 2, line_width: 100, align_operators: false, keyword_case: KeywordCase::Preserve }
    }
}

/// `lp.inlayHints`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct InlayHintSettings {
    /// Generated names for unnamed constraints and objectives.
    pub generated_names: bool,
    /// The `_rng` partner of ranged constraints.
    pub range_partners: bool,
    /// Variable type after a variable's first use.
    pub variable_types: bool,
    /// Normalised RHS when constants appear on the left-hand side.
    pub normalised_rhs: bool,
}

impl Default for InlayHintSettings {
    fn default() -> Self {
        Self { generated_names: true, range_partners: true, variable_types: true, normalised_rhs: true }
    }
}

impl Config {
    /// Parse the `lp` section; invalid input yields an error message.
    ///
    /// # Errors
    /// When the value does not match the schema.
    pub fn from_value(value: serde_json::Value) -> Result<Self, String> {
        if value.is_null() {
            return Ok(Self::default());
        }
        let config: Self = serde_json::from_value(value).map_err(|e| format!("invalid `lp` configuration: {e}"))?;
        config.validate()?;
        Ok(config)
    }

    fn validate(&self) -> Result<(), String> {
        if self.format.indent > 16 {
            return Err(format!("lp.format.indent must be at most 16, got {}", self.format.indent));
        }
        if self.format.line_width < 20 {
            return Err(format!("lp.format.lineWidth must be at least 20, got {}", self.format.line_width));
        }
        Ok(())
    }

    /// Semantic size threshold in bytes.
    #[must_use]
    pub fn max_semantic_bytes(&self) -> usize {
        usize::try_from(self.semantic.max_file_size_mb.saturating_mul(1024 * 1024)).unwrap_or(usize::MAX)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partial_config_keeps_defaults() {
        let config = Config::from_value(serde_json::json!({ "format": { "indent": 4, "keywordCase": "upper" } })).unwrap();
        assert_eq!(config.format.indent, 4);
        assert_eq!(config.format.keyword_case, KeywordCase::Upper);
        assert_eq!(config.format.line_width, 100);
        assert_eq!(config.semantic.debounce_ms, 300);
    }

    #[test]
    fn analysis_thresholds_use_camel_case_and_keep_defaults() {
        let config = Config::from_value(serde_json::json!({ "analysis": { "enabled": false, "largeRhsThreshold": 5.0 } })).unwrap();
        assert!(!config.analysis.enabled);
        assert_eq!(config.analysis.thresholds.large_rhs_threshold, 5.0);
        assert_eq!(config.analysis.thresholds.coefficient_ratio_threshold, AnalysisConfig::default().coefficient_ratio_threshold);
    }

    #[test]
    fn rejects_bad_values() {
        assert!(Config::from_value(serde_json::json!({ "format": { "indent": "two" } })).is_err());
        assert!(Config::from_value(serde_json::json!({ "format": { "lineWidth": 5 } })).is_err());
    }
}
