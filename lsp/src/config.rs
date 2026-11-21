//! Client configuration (section `lp`).

use std::collections::BTreeMap;

use lp_parser_rs::analysis::{AnalysisConfig, Certainty, DiagnosticCategory, DiagnosticCode, DiagnosticsConfig};
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
    /// Publish heuristic findings (smells that are sometimes intended). Proven
    /// findings are unaffected.
    pub heuristics: bool,
    /// Which diagnostic categories are published.
    pub categories: CategorySettings,
    /// Per-code severity, keyed by diagnostic code. An entry other than
    /// `off` also overrides `heuristics` and `categories` for that code.
    pub severity_overrides: BTreeMap<String, SeverityOverride>,
    /// `DiagnosticsConfig::feasibility_tolerance`.
    pub feasibility_tolerance: f64,
    /// `DiagnosticsConfig::equality_tolerance`.
    pub equality_tolerance: f64,
    /// `DiagnosticsConfig::rhs_coefficient_ratio_threshold`.
    pub rhs_coefficient_ratio_threshold: f64,
    /// `DiagnosticsConfig::max_per_code`.
    pub max_per_code: usize,
    /// Upstream thresholds (`largeCoefficientThreshold`, ...).
    #[serde(flatten)]
    pub thresholds: AnalysisConfig,
}

impl Default for AnalysisSettings {
    fn default() -> Self {
        let upstream = DiagnosticsConfig::default();
        Self {
            enabled: true,
            heuristics: true,
            categories: CategorySettings::default(),
            severity_overrides: BTreeMap::new(),
            feasibility_tolerance: upstream.feasibility_tolerance,
            equality_tolerance: upstream.equality_tolerance,
            rhs_coefficient_ratio_threshold: upstream.rhs_coefficient_ratio_threshold,
            max_per_code: upstream.max_per_code,
            thresholds: upstream.thresholds,
        }
    }
}

impl AnalysisSettings {
    /// The library configuration these settings describe.
    #[must_use]
    pub fn diagnostics_config(&self) -> DiagnosticsConfig {
        let mut config = DiagnosticsConfig::default();
        config.thresholds = self.thresholds.clone();
        config.feasibility_tolerance = self.feasibility_tolerance;
        config.equality_tolerance = self.equality_tolerance;
        config.rhs_coefficient_ratio_threshold = self.rhs_coefficient_ratio_threshold;
        config.max_per_code = self.max_per_code;
        config
    }

    /// Whether a diagnostic with `code` is published. An explicit severity
    /// override is the most specific setting: `off` hides the code, any other
    /// value shows it whatever `heuristics` and `categories` say.
    #[must_use]
    pub fn publishes(&self, code: DiagnosticCode) -> bool {
        match self.severity_override(code) {
            Some(SeverityOverride::Off) => false,
            Some(_) => true,
            None => self.categories.contains(code.category()) && (self.heuristics || code.certainty() != Certainty::Heuristic),
        }
    }

    /// The configured severity override for `code`, if any.
    #[must_use]
    pub fn severity_override(&self, code: DiagnosticCode) -> Option<SeverityOverride> {
        self.severity_overrides.get(code.as_str()).copied()
    }
}

/// `lp.analysis.categories`: one switch per [`DiagnosticCategory`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct CategorySettings {
    /// Redundant, duplicate and unused model elements.
    pub structural: bool,
    /// Badly scaled coefficients and right-hand sides.
    pub numerical: bool,
    /// Infeasible, contradictory or unbounded formulations.
    pub mathematical: bool,
    /// Findings about the analysis itself, such as truncation.
    pub performance: bool,
}

impl Default for CategorySettings {
    fn default() -> Self {
        Self { structural: true, numerical: true, mathematical: true, performance: true }
    }
}

impl CategorySettings {
    /// Whether `category` is switched on.
    #[must_use]
    pub fn contains(&self, category: DiagnosticCategory) -> bool {
        match category {
            DiagnosticCategory::Structural => self.structural,
            DiagnosticCategory::Numerical => self.numerical,
            DiagnosticCategory::Mathematical => self.mathematical,
            DiagnosticCategory::Performance => self.performance,
            // A category added upstream is shown until a setting for it exists.
            _ => true,
        }
    }
}

/// A `lp.analysis.severityOverrides` value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SeverityOverride {
    /// Publish as an error.
    Error,
    /// Publish as a warning.
    Warning,
    /// Publish as information.
    Information,
    /// Publish as a hint.
    Hint,
    /// Do not publish.
    Off,
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
        let t = &self.analysis.thresholds;
        for (name, value) in [
            ("largeCoefficientThreshold", t.large_coefficient_threshold),
            ("smallCoefficientThreshold", t.small_coefficient_threshold),
            ("largeRhsThreshold", t.large_rhs_threshold),
            ("coefficientRatioThreshold", t.coefficient_ratio_threshold),
        ] {
            if !(value.is_finite() && value > 0.0) {
                return Err(format!("lp.analysis.{name} must be a positive number, got {value}"));
            }
        }
        if t.small_coefficient_threshold > t.large_coefficient_threshold {
            return Err(format!(
                "lp.analysis.smallCoefficientThreshold ({}) must not exceed largeCoefficientThreshold ({})",
                t.small_coefficient_threshold, t.large_coefficient_threshold
            ));
        }
        self.analysis.diagnostics_config().validate().map_err(|e| format!("invalid lp.analysis setting: {e}"))?;
        for code in self.analysis.severity_overrides.keys() {
            code.parse::<DiagnosticCode>().map_err(|_| format!("lp.analysis.severityOverrides: unknown diagnostic code `{code}`"))?;
        }
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
        // Whole-struct comparison: the other thresholds keep their defaults.
        assert_eq!(config.analysis.thresholds, AnalysisConfig { large_rhs_threshold: 5.0, ..AnalysisConfig::default() });
    }

    #[test]
    fn rejects_bad_values() {
        assert!(Config::from_value(serde_json::json!({ "format": { "indent": "two" } })).is_err());
        assert!(Config::from_value(serde_json::json!({ "format": { "lineWidth": 5 } })).is_err());
        assert!(Config::from_value(serde_json::json!({ "analysis": { "smallCoefficientThreshold": 1e12 } })).is_err());
        assert!(Config::from_value(serde_json::json!({ "analysis": { "largeRhsThreshold": -1.0 } })).is_err());
        for bad in [
            serde_json::json!({ "analysis": { "feasibilityTolerance": -1.0 } }),
            serde_json::json!({ "analysis": { "equalityTolerance": -1e-9 } }),
            serde_json::json!({ "analysis": { "maxPerCode": 0 } }),
            serde_json::json!({ "analysis": { "severityOverrides": { "no-such-code": "off" } } }),
            serde_json::json!({ "analysis": { "severityOverrides": { "duplicate-row": "loud" } } }),
        ] {
            assert!(Config::from_value(bad.clone()).is_err(), "{bad}");
        }
    }

    #[test]
    fn diagnostics_settings_map_to_the_library_config() {
        let config = Config::from_value(serde_json::json!({
            "analysis": { "feasibilityTolerance": 1e-4, "maxPerCode": 7, "largeRhsThreshold": 5.0 }
        }))
        .unwrap();
        let mapped = config.analysis.diagnostics_config();
        assert!((mapped.feasibility_tolerance - 1e-4).abs() < f64::EPSILON);
        assert_eq!(mapped.max_per_code, 7);
        assert!((mapped.thresholds.large_rhs_threshold - 5.0).abs() < f64::EPSILON);
        assert_eq!(Config::default().analysis.diagnostics_config(), DiagnosticsConfig::default());
    }

    #[test]
    fn categories_heuristics_and_overrides_decide_what_is_published() {
        let settings = |value: serde_json::Value| Config::from_value(serde_json::json!({ "analysis": value })).unwrap().analysis;
        let all = settings(serde_json::json!({}));
        assert!(DiagnosticCode::ALL.iter().all(|&code| all.publishes(code)), "everything is on by default");

        let no_numerical = settings(serde_json::json!({ "categories": { "numerical": false } }));
        assert!(!no_numerical.publishes(DiagnosticCode::LargeCoefficient));
        assert!(no_numerical.publishes(DiagnosticCode::DuplicateRow));

        let no_heuristics = settings(serde_json::json!({ "heuristics": false }));
        assert!(!no_heuristics.publishes(DiagnosticCode::IntegerIsBinary));
        assert!(no_heuristics.publishes(DiagnosticCode::InvalidBounds), "proven findings stay");

        let overridden = settings(serde_json::json!({
            "heuristics": false,
            "categories": { "mathematical": false },
            "severityOverrides": { "integer-is-binary": "hint", "invalid-bounds": "warning", "duplicate-row": "off" }
        }));
        assert!(overridden.publishes(DiagnosticCode::IntegerIsBinary), "an explicit severity beats `heuristics`");
        assert!(overridden.publishes(DiagnosticCode::InvalidBounds), "an explicit severity beats `categories`");
        assert!(!overridden.publishes(DiagnosticCode::DuplicateRow), "`off` hides the code");
        assert_eq!(overridden.severity_override(DiagnosticCode::InvalidBounds), Some(SeverityOverride::Warning));
    }

    #[test]
    fn solver_and_solution_sections_are_not_this_parsers_business() {
        // `lp.solver` / `lp.solution` are read by `requests::SolverSettings::from_value`;
        // a bad value there must not discard the settings this parser owns.
        let value = serde_json::json!({
            "analysis": { "heuristics": false },
            "solver": { "timeLimit": 0, "path": null },
            "solution": { "showStale": "yes" }
        });
        let config = Config::from_value(value.clone()).expect("solver and solution sections are ignored here");
        assert!(!config.analysis.heuristics);
        assert!(crate::requests::SolverSettings::from_value(&value).is_err());
    }

    #[test]
    fn package_json_settings_match_the_server() {
        let manifest: serde_json::Value = serde_json::from_str(include_str!("../editors/vscode/package.json")).unwrap();
        let properties = &manifest["contributes"]["configuration"]["properties"];
        let overrides: Vec<&str> =
            properties["lp.analysis.severityOverrides"]["properties"].as_object().unwrap().keys().map(String::as_str).collect();
        let mut codes: Vec<&str> = DiagnosticCode::ALL.iter().map(|c| c.as_str()).collect();
        codes.sort_unstable();
        assert_eq!(overrides, codes, "every diagnostic code is an allowed override key");

        // The declared defaults are the server's defaults.
        let defaults = Config::default();
        assert_eq!(properties["lp.analysis.heuristics"]["default"], defaults.analysis.heuristics);
        assert_eq!(properties["lp.analysis.maxPerCode"]["default"], defaults.analysis.max_per_code);
        assert_eq!(properties["lp.analysis.feasibilityTolerance"]["default"], defaults.analysis.feasibility_tolerance);
        assert_eq!(properties["lp.analysis.equalityTolerance"]["default"], defaults.analysis.equality_tolerance);
        assert_eq!(properties["lp.analysis.rhsCoefficientRatioThreshold"]["default"], defaults.analysis.rhs_coefficient_ratio_threshold);
        assert_eq!(properties["lp.solver.timeLimit"]["default"], crate::requests::SolverSettings::default().time_limit);
        assert_eq!(properties["lp.solution.inlayHints"]["default"], crate::solution::SolutionSettings::default().inlay_hints);
        assert_eq!(properties["lp.solution.showStale"]["default"], crate::solution::SolutionSettings::default().show_stale);
        assert_eq!(properties["lp.solver.path"]["scope"], "machine");
        for category in ["structural", "numerical", "mathematical", "performance"] {
            assert_eq!(properties["lp.analysis.categories"]["default"][category], true, "{category}");
        }
    }
}
