//! Model diagnostics: stable, coded findings about a parsed problem.
//!
//! [`LpProblem::diagnostics`] runs every check in the table of
//! [`DiagnosticCode`] and returns a [`ModelDiagnostics`]. Unlike the
//! [`AnalysisIssue`](super::AnalysisIssue)s of [`LpProblem::analyze`], each
//! finding carries a stable kebab-case code, a [`Certainty`] (`proven` or
//! `heuristic`), a [`DiagnosticSubject`] naming the entity it is about and
//! [`RelatedSubject`]s naming the other entities involved. Positions are not
//! reported: callers map names to source ranges themselves.
//!
//! # Checks considered and rejected
//!
//! These are deliberately not implemented:
//!
//! - Variables that occur only in constraints and not in the objective: normal
//!   modelling.
//! - Variables in exactly one row: slack variables.
//! - Variables in several objectives: normal for multi-objective files.
//! - General redundancy across a system of inequalities: it needs an LP solve.
//! - Several objectives with conflicting senses: an LP has a single sense.
//!
//! # Example
//!
//! ```rust
//! use lp_parser_rs::{DiagnosticCode, LpProblem};
//!
//! let problem = LpProblem::parse("Minimize\n obj: x\nSubject To\n c1: x + y >= 2\n c2: 2 x + 2 y >= 4\nEnd")?;
//! let diagnostics = problem.diagnostics();
//! assert!(diagnostics.with_code(DiagnosticCode::ProportionalRow).count() == 1);
//! # Ok::<(), lp_parser_rs::LpParseError>(())
//! ```

use std::fmt::{Display, Formatter, Result as FmtResult};
use std::str::FromStr;

use rustc_hash::FxHashSet;

use super::{AnalysisConfig, IssueSeverity};
use crate::error::{EntityKind, LpParseError, LpResult};
use crate::interner::NameId;
use crate::problem::LpProblem;

/// Configuration for [`LpProblem::diagnostics_with_config`].
///
/// Deserialising fills missing fields from [`Default`] and uses camelCase
/// field names.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(default, rename_all = "camelCase"))]
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct DiagnosticsConfig {
    /// Existing magnitude thresholds, reused unchanged.
    pub thresholds: AnalysisConfig,
    /// Absolute/relative slack before a bound or row violation counts as proven (default 1e-6).
    pub feasibility_tolerance: f64,
    /// Relative tolerance for two normalised coefficients being equal (default 1e-9).
    pub equality_tolerance: f64,
    /// |rhs| / max |a| in a row above which `rhs-coefficient-ratio` fires (default 1e9).
    pub rhs_coefficient_ratio_threshold: f64,
    /// Diagnostics kept per code before `diagnostics-truncated` is emitted (default 1000).
    pub max_per_code: usize,
}

impl Default for DiagnosticsConfig {
    fn default() -> Self {
        Self {
            thresholds: AnalysisConfig::default(),
            feasibility_tolerance: 1e-6,
            equality_tolerance: 1e-9,
            rhs_coefficient_ratio_threshold: 1e9,
            max_per_code: 1000,
        }
    }
}

impl DiagnosticsConfig {
    /// Check the configuration is usable.
    ///
    /// # Errors
    ///
    /// Validation error when any tolerance is negative, NaN or infinite, or `max_per_code == 0`.
    pub fn validate(&self) -> LpResult<()> {
        for (name, value) in [
            ("feasibility_tolerance", self.feasibility_tolerance),
            ("equality_tolerance", self.equality_tolerance),
            ("rhs_coefficient_ratio_threshold", self.rhs_coefficient_ratio_threshold),
            ("large_coefficient_threshold", self.thresholds.large_coefficient_threshold),
            ("small_coefficient_threshold", self.thresholds.small_coefficient_threshold),
            ("large_rhs_threshold", self.thresholds.large_rhs_threshold),
            ("coefficient_ratio_threshold", self.thresholds.coefficient_ratio_threshold),
        ] {
            if !value.is_finite() || value < 0.0 {
                return Err(LpParseError::validation_error(format!("{name} must be finite and non-negative, got {value}")));
            }
        }
        if self.max_per_code == 0 {
            return Err(LpParseError::validation_error("max_per_code must be at least 1"));
        }
        Ok(())
    }
}

/// Broad grouping of a [`DiagnosticCode`].
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "kebab-case"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum DiagnosticCategory {
    /// How the model is put together.
    Structural,
    /// Magnitudes that strain floating point.
    Numerical,
    /// What the model means: feasibility, redundancy, convexity.
    Mathematical,
    /// Cost of analysis or solving.
    Performance,
}

impl DiagnosticCategory {
    /// Kebab-case name, equal to the serde name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Structural => "structural",
            Self::Numerical => "numerical",
            Self::Mathematical => "mathematical",
            Self::Performance => "performance",
        }
    }
}

/// How sure a diagnostic is.
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "kebab-case"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Certainty {
    /// Follows from the model as written (may be conditional, e.g. "if feasible"; the message says so).
    Proven,
    /// A smell: often a mistake, sometimes intended.
    Heuristic,
}

impl Certainty {
    /// Kebab-case name, equal to the serde name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Proven => "proven",
            Self::Heuristic => "heuristic",
        }
    }
}

/// Where a caller should anchor a subject in the source.
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "kebab-case"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum SubjectSite {
    /// Where the entity is defined (or first appears).
    Definition,
    /// Where the variable's bound is declared.
    Bound,
    /// Where the variable's type is declared.
    TypeDeclaration,
}

impl SubjectSite {
    /// Kebab-case name, equal to the serde name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Definition => "definition",
            Self::Bound => "bound",
            Self::TypeDeclaration => "type-declaration",
        }
    }
}

/// The entity a diagnostic is about.
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct DiagnosticSubject {
    /// Kind of entity.
    pub kind: EntityKind,
    /// Name as resolved; may be generated (`C3`, `c1_rng`).
    pub name: String,
    /// The term, for coefficient-level findings.
    pub variable: Option<String>,
    /// Where to anchor the subject.
    pub site: SubjectSite,
}

impl DiagnosticSubject {
    /// Subject naming a variable.
    pub fn variable(name: impl Into<String>, site: SubjectSite) -> Self {
        Self { kind: EntityKind::Variable, name: name.into(), variable: None, site }
    }

    /// Subject naming a constraint.
    pub fn constraint(name: impl Into<String>) -> Self {
        Self { kind: EntityKind::Constraint, name: name.into(), variable: None, site: SubjectSite::Definition }
    }

    /// Subject naming an objective.
    pub fn objective(name: impl Into<String>) -> Self {
        Self { kind: EntityKind::Objective, name: name.into(), variable: None, site: SubjectSite::Definition }
    }

    /// Narrow the subject to one term.
    #[must_use]
    pub fn with_variable(mut self, variable: impl Into<String>) -> Self {
        self.variable = Some(variable.into());
        self
    }
}

/// Another entity involved in a diagnostic, with a note on its role.
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct RelatedSubject {
    /// The entity.
    pub subject: DiagnosticSubject,
    /// Its role, for example "implied by this row".
    pub note: String,
}

impl RelatedSubject {
    pub(super) fn new(subject: DiagnosticSubject, note: impl Into<String>) -> Self {
        Self { subject, note: note.into() }
    }
}

macro_rules! diagnostic_codes {
    ($($(#[$doc:meta])* $variant:ident => ($text:literal, $sev:ident, $cert:ident, $cat:ident)),+ $(,)?) => {
        /// Stable identifier of a model check.
        ///
        /// Variants are declared in table order, which is also the output
        /// order within a severity. The kebab-case name ([`Self::as_str`]) is
        /// the stable public identifier.
        ///
        /// `nonconvex-objective` only proves non-convexity from the diagonal of
        /// the quadratic form; silence is not a convexity proof.
        #[cfg_attr(feature = "serde", derive(serde::Serialize))]
        #[cfg_attr(feature = "serde", serde(rename_all = "kebab-case"))]
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        #[non_exhaustive]
        pub enum DiagnosticCode {
            $($(#[$doc])* $variant,)+
        }

        impl DiagnosticCode {
            /// Every code, in declaration order.
            pub const ALL: &'static [Self] = &[$(Self::$variant),+];

            /// Kebab-case name, equal to the serde name.
            #[must_use]
            pub const fn as_str(self) -> &'static str {
                match self { $(Self::$variant => $text),+ }
            }

            /// Broad grouping of the check.
            #[must_use]
            pub const fn category(self) -> DiagnosticCategory {
                match self { $(Self::$variant => DiagnosticCategory::$cat),+ }
            }

            /// Whether the check is a proof or a smell.
            #[must_use]
            pub const fn certainty(self) -> Certainty {
                match self { $(Self::$variant => Certainty::$cert),+ }
            }

            /// Severity reported for the code.
            #[must_use]
            pub const fn default_severity(self) -> IssueSeverity {
                match self { $(Self::$variant => IssueSeverity::$sev),+ }
            }
        }
    };
}

diagnostic_codes! {
    /// A declared lower bound exceeds the declared upper bound.
    InvalidBounds => ("invalid-bounds", Error, Proven, Mathematical),
    /// An upper bound below zero with the default lower bound of zero.
    NegativeUpperDefaultLower => ("negative-upper-default-lower", Warning, Heuristic, Mathematical),
    /// An integer variable whose bounds contain no integer.
    IntegerNoFeasibleValues => ("integer-no-feasible-values", Error, Proven, Mathematical),
    /// An integer variable with a fractional bound.
    IntegerFractionalBound => ("integer-fractional-bound", Info, Proven, Mathematical),
    /// A semi-integer variable whose bounds contain no integer.
    SemiIntegerOnlyZero => ("semi-integer-only-zero", Warning, Proven, Mathematical),
    /// A binary variable with a declared bound outside [0, 1].
    BinaryBoundOutsideUnit => ("binary-bound-outside-unit", Warning, Proven, Mathematical),
    /// A binary variable with bounds that change nothing.
    BinaryRedundantBounds => ("binary-redundant-bounds", Info, Proven, Structural),
    /// An integer variable bounded to exactly [0, 1].
    IntegerIsBinary => ("integer-is-binary", Info, Heuristic, Structural),
    /// A semi-continuous variable with no upper bound.
    SemicontinuousInfiniteUpper => ("semicontinuous-infinite-upper", Warning, Proven, Mathematical),
    /// A variable with equal bounds.
    FixedVariable => ("fixed-variable", Info, Proven, Structural),
    /// A variable declared but never referenced.
    UnusedVariable => ("unused-variable", Info, Proven, Structural),
    /// A variable only in the objective, so it sits at a bound.
    ObjectiveOnlyVariable => ("objective-only-variable", Info, Heuristic, Structural),
    /// A variable that can improve the objective without limit, if the model is feasible.
    NoEffectiveBound => ("no-effective-bound", Warning, Proven, Mathematical),
    /// A quadratic objective that is provably non-convex.
    NonconvexObjective => ("nonconvex-objective", Warning, Proven, Mathematical),
    /// An objective with no terms.
    EmptyObjective => ("empty-objective", Info, Proven, Structural),
    /// Two objectives that are the same up to a positive factor.
    DuplicateObjective => ("duplicate-objective", Info, Proven, Structural),
    /// Two objectives that are the same up to a negative factor.
    OpposedObjectives => ("opposed-objectives", Warning, Proven, Structural),
    /// A row with no terms that can never hold.
    EmptyRowInfeasible => ("empty-row-infeasible", Error, Proven, Mathematical),
    /// A row with no terms that always holds.
    EmptyRow => ("empty-row", Warning, Proven, Structural),
    /// A zero coefficient written explicitly.
    ExplicitZeroCoefficient => ("explicit-zero-coefficient", Info, Proven, Structural),
    /// A variable listed twice in one row.
    RepeatedVariableInRow => ("repeated-variable-in-row", Info, Proven, Structural),
    /// A row with one term, which is really a bound.
    SingletonRow => ("singleton-row", Info, Proven, Structural),
    /// A row the variable bounds make impossible.
    RowInfeasibleByBounds => ("row-infeasible-by-bounds", Error, Proven, Mathematical),
    /// A row the variable bounds already guarantee.
    RowRedundantByBounds => ("row-redundant-by-bounds", Info, Proven, Mathematical),
    /// A row identical to an earlier one.
    DuplicateRow => ("duplicate-row", Warning, Proven, Mathematical),
    /// A row that is an earlier one scaled.
    ProportionalRow => ("proportional-row", Warning, Proven, Mathematical),
    /// A row implied by a tighter one.
    DominatedRow => ("dominated-row", Info, Proven, Mathematical),
    /// Rows that cannot hold together.
    ContradictoryRows => ("contradictory-rows", Error, Proven, Mathematical),
    /// Two rows that could be one ranged constraint.
    RowsFormRange => ("rows-form-range", Info, Proven, Structural),
    /// More equality rows than variables.
    EqualitiesExceedVariables => ("equalities-exceed-variables", Warning, Heuristic, Mathematical),
    /// A coefficient above the large threshold.
    LargeCoefficient => ("large-coefficient", Warning, Heuristic, Numerical),
    /// A non-zero coefficient below the small threshold.
    SmallCoefficient => ("small-coefficient", Warning, Heuristic, Numerical),
    /// A right-hand side above the large threshold.
    LargeRhs => ("large-rhs", Warning, Heuristic, Numerical),
    /// A row whose coefficients span many orders of magnitude.
    RowCoefficientRange => ("row-coefficient-range", Warning, Heuristic, Numerical),
    /// A column whose coefficients span many orders of magnitude.
    ColumnCoefficientRange => ("column-coefficient-range", Warning, Heuristic, Numerical),
    /// An objective whose coefficients span many orders of magnitude.
    ObjectiveCoefficientRange => ("objective-coefficient-range", Warning, Heuristic, Numerical),
    /// The whole model's coefficients span many orders of magnitude.
    CoefficientRange => ("coefficient-range", Info, Heuristic, Numerical),
    /// A right-hand side dwarfing the row's coefficients.
    RhsCoefficientRatio => ("rhs-coefficient-ratio", Info, Heuristic, Numerical),
    /// An indicator variable that is not binary.
    IndicatorNotBinary => ("indicator-not-binary", Error, Proven, Structural),
    /// An SOS with two members of equal weight.
    SosDuplicateWeights => ("sos-duplicate-weights", Warning, Proven, Structural),
    /// An SOS that is always satisfied.
    SosTrivial => ("sos-trivial", Info, Proven, Structural),
    /// A code hit the per-code cap.
    DiagnosticsTruncated => ("diagnostics-truncated", Info, Proven, Performance),
}

impl Display for DiagnosticCode {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        f.write_str(self.as_str())
    }
}

impl FromStr for DiagnosticCode {
    type Err = LpParseError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .iter()
            .copied()
            .find(|code| code.as_str() == text)
            .ok_or_else(|| LpParseError::validation_error(format!("unknown diagnostic code '{text}'")))
    }
}

/// One finding about the model.
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct ModelDiagnostic {
    /// Stable identifier.
    pub code: DiagnosticCode,
    /// Equal to [`DiagnosticCode::default_severity`] in this release.
    pub severity: IssueSeverity,
    /// Equal to [`DiagnosticCode::category`].
    pub category: DiagnosticCategory,
    /// Equal to [`DiagnosticCode::certainty`].
    pub certainty: Certainty,
    /// One sentence naming the entities.
    pub message: String,
    /// Why it matters and how to fix it.
    pub explanation: Option<String>,
    /// The entity the finding is about; `None` only for problem-wide codes.
    pub subject: Option<DiagnosticSubject>,
    /// Other rows, objectives or variables involved.
    pub related: Vec<RelatedSubject>,
}

impl Display for ModelDiagnostic {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        write!(f, "[{}] {}: {}", self.severity, self.code, self.message)
    }
}

/// All findings for one problem.
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[derive(Debug, Clone, Default, PartialEq)]
#[non_exhaustive]
pub struct ModelDiagnostics {
    /// The findings, sorted (see [`LpProblem::diagnostics`]).
    pub diagnostics: Vec<ModelDiagnostic>,
    /// Codes that hit `max_per_code`, with the number dropped.
    pub truncated: Vec<(DiagnosticCode, usize)>,
}

impl ModelDiagnostics {
    /// Number of diagnostics of one severity.
    #[must_use]
    pub fn count(&self, severity: IssueSeverity) -> usize {
        self.diagnostics.iter().filter(|d| d.severity == severity).count()
    }

    /// The diagnostics with one code.
    pub fn with_code(&self, code: DiagnosticCode) -> impl Iterator<Item = &ModelDiagnostic> {
        self.diagnostics.iter().filter(move |d| d.code == code)
    }
}

/// Shared read-only state for the checks.
pub(super) struct Ctx<'a> {
    pub problem: &'a LpProblem,
    pub config: &'a DiagnosticsConfig,
}

impl Ctx<'_> {
    pub(super) fn name(&self, id: NameId) -> &str {
        self.problem.resolve(id)
    }
}

/// Collects findings from the checks.
#[derive(Default)]
pub(super) struct Sink {
    items: Vec<ModelDiagnostic>,
}

impl Sink {
    pub(super) fn push(
        &mut self,
        code: DiagnosticCode,
        message: String,
        explanation: Option<&str>,
        subject: Option<DiagnosticSubject>,
        related: Vec<RelatedSubject>,
    ) {
        debug_assert!(!message.is_empty(), "a diagnostic needs a message");
        debug_assert!(subject.is_some() || related.is_empty(), "related subjects need a subject");
        self.items.push(ModelDiagnostic {
            code,
            severity: code.default_severity(),
            category: code.category(),
            certainty: code.certainty(),
            message,
            explanation: explanation.map(str::to_owned),
            subject,
            related,
        });
    }
}

/// Format a number for a message: short, no float noise.
pub(super) fn fmt_num(value: f64) -> String {
    if !value.is_finite() {
        return if value.is_nan() {
            "NaN".to_owned()
        } else if value > 0.0 {
            "+inf".to_owned()
        } else {
            "-inf".to_owned()
        };
    }
    if value == 0.0 {
        return "0".to_owned();
    }
    let magnitude = value.abs();
    if !(1e-4..1e9).contains(&magnitude) {
        return format!("{value:.4e}");
    }
    let text = format!("{value:.6}");
    text.trim_end_matches('0').trim_end_matches('.').to_owned()
}

const fn severity_rank(severity: IssueSeverity) -> u8 {
    match severity {
        IssueSeverity::Error => 0,
        IssueSeverity::Warning => 1,
        IssueSeverity::Info => 2,
    }
}

/// Position of a subject in model order: objectives, constraints, variables.
fn model_position(problem: &LpProblem, subject: Option<&DiagnosticSubject>) -> (u8, usize, usize) {
    let Some(subject) = subject else {
        return (0, 0, 0);
    };
    let id = problem.name_id(&subject.name);
    let (rank, index) = match subject.kind {
        EntityKind::Objective => (1, id.and_then(|id| problem.objectives.get_index_of(&id))),
        EntityKind::Constraint => (2, id.and_then(|id| problem.constraints.get_index_of(&id))),
        EntityKind::Variable => (3, id.and_then(|id| problem.variables.get_index_of(&id))),
    };
    let term =
        subject.variable.as_deref().and_then(|v| problem.name_id(v)).and_then(|v| problem.variables.get_index_of(&v)).map_or(0, |i| i + 1);
    (rank, index.unwrap_or(usize::MAX), term)
}

fn subject_resolves(problem: &LpProblem, subject: &DiagnosticSubject) -> bool {
    let Some(id) = problem.name_id(&subject.name) else {
        return false;
    };
    let found = match subject.kind {
        EntityKind::Objective => problem.objectives.contains_key(&id),
        EntityKind::Constraint => problem.constraints.contains_key(&id),
        EntityKind::Variable => problem.variables.contains_key(&id),
    };
    found && subject.variable.as_deref().is_none_or(|v| problem.name_id(v).is_some_and(|v| problem.variables.contains_key(&v)))
}

fn kind_rank(kind: EntityKind) -> u8 {
    match kind {
        EntityKind::Objective => 0,
        EntityKind::Constraint => 1,
        EntityKind::Variable => 2,
    }
}

impl LpProblem {
    /// Run every model check with the default [`DiagnosticsConfig`].
    ///
    /// Output is deterministic: sorted by severity (error, warning, info), then
    /// [`DiagnosticCode`] declaration order, then the model order of the
    /// subject, then the related names. Problem-wide diagnostics come first
    /// within their code.
    #[must_use]
    pub fn diagnostics(&self) -> ModelDiagnostics {
        self.run_diagnostics(&DiagnosticsConfig::default())
    }

    /// Run every model check with a custom configuration.
    ///
    /// # Errors
    ///
    /// Returns a validation error when [`DiagnosticsConfig::validate`] fails.
    pub fn diagnostics_with_config(&self, config: &DiagnosticsConfig) -> LpResult<ModelDiagnostics> {
        config.validate()?;
        Ok(self.run_diagnostics(config))
    }

    fn run_diagnostics(&self, config: &DiagnosticsConfig) -> ModelDiagnostics {
        debug_assert!(config.validate().is_ok(), "precondition: config must be valid");
        let ctx = Ctx { problem: self, config };
        let mut sink = Sink::default();
        super::domains::run(&ctx, &mut sink);
        super::rows::run(&ctx, &mut sink);

        // One finding per (code, subject): a coefficient repeated in a row would
        // otherwise be reported twice.
        let mut seen = FxHashSet::default();
        let mut items: Vec<ModelDiagnostic> = sink
            .items
            .into_iter()
            .filter(|d| {
                let key = (d.code, d.subject.as_ref().map(|s| (kind_rank(s.kind), s.name.clone(), s.variable.clone(), s.site)));
                seen.insert(key)
            })
            .collect();

        items.sort_by_cached_key(|d| {
            let related: Vec<&str> = d.related.iter().map(|r| r.subject.name.as_str()).collect();
            (severity_rank(d.severity), d.code, model_position(self, d.subject.as_ref()), related.join("\u{0}"))
        });

        // Cap each code.
        let mut counts: Vec<usize> = vec![0; DiagnosticCode::ALL.len()];
        let mut dropped: Vec<usize> = vec![0; DiagnosticCode::ALL.len()];
        let mut kept = Vec::with_capacity(items.len());
        for item in items {
            let slot = item.code as usize;
            if counts[slot] < config.max_per_code {
                counts[slot] += 1;
                kept.push(item);
            } else {
                dropped[slot] += 1;
            }
        }
        let mut truncated = Vec::new();
        for (slot, &count) in dropped.iter().enumerate() {
            if count > 0 {
                let code = DiagnosticCode::ALL[slot];
                kept.push(ModelDiagnostic {
                    code: DiagnosticCode::DiagnosticsTruncated,
                    severity: DiagnosticCode::DiagnosticsTruncated.default_severity(),
                    category: DiagnosticCode::DiagnosticsTruncated.category(),
                    certainty: DiagnosticCode::DiagnosticsTruncated.certainty(),
                    message: format!("{count} further '{code}' diagnostics were dropped after the first {}", config.max_per_code),
                    explanation: Some("Raise `max_per_code` to see every finding.".to_owned()),
                    subject: None,
                    related: Vec::new(),
                });
                truncated.push((code, count));
            }
        }
        // The truncation notices sort like any other info diagnostic.
        kept.sort_by_cached_key(|d| {
            let related: Vec<&str> = d.related.iter().map(|r| r.subject.name.as_str()).collect();
            (severity_rank(d.severity), d.code, model_position(self, d.subject.as_ref()), related.join("\u{0}"))
        });

        debug_assert!(
            kept.iter().all(|d| d.subject.iter().chain(d.related.iter().map(|r| &r.subject)).all(|s| subject_resolves(self, s))),
            "invariant: every subject resolves in the problem"
        );
        debug_assert!(kept.iter().all(|d| d.severity == d.code.default_severity()), "invariant: severity matches the code");
        debug_assert!(
            kept.windows(2).all(|w| severity_rank(w[0].severity) <= severity_rank(w[1].severity)),
            "invariant: output is sorted by severity"
        );

        ModelDiagnostics { diagnostics: kept, truncated }
    }
}

#[cfg(test)]
mod tests {
    use std::fmt::Write as _;

    use super::*;

    #[test]
    fn codes_are_unique_kebab_case_and_round_trip() {
        let mut seen = FxHashSet::default();
        for &code in DiagnosticCode::ALL {
            let text = code.as_str();
            assert!(text.chars().all(|c| c.is_ascii_lowercase() || c == '-'), "{text}");
            assert!(seen.insert(text), "duplicate code {text}");
            assert_eq!(text.parse::<DiagnosticCode>().unwrap(), code);
            assert_eq!(code.to_string(), text);
        }
        assert_eq!(DiagnosticCode::ALL.len(), 42);
        assert!("nope".parse::<DiagnosticCode>().is_err());
    }

    #[test]
    fn code_order_is_declaration_order() {
        assert!(DiagnosticCode::ALL.windows(2).all(|w| w[0] < w[1]));
        for (i, &code) in DiagnosticCode::ALL.iter().enumerate() {
            assert_eq!(code as usize, i);
        }
    }

    #[cfg(feature = "serde")]
    #[test]
    fn serde_names_equal_as_str() {
        for &code in DiagnosticCode::ALL {
            assert_eq!(serde_json::to_value(code).unwrap(), serde_json::Value::String(code.as_str().to_owned()));
        }
        assert_eq!(serde_json::to_value(Certainty::Proven).unwrap(), "proven");
        assert_eq!(serde_json::to_value(SubjectSite::TypeDeclaration).unwrap(), "type-declaration");
    }

    #[test]
    fn config_validation() {
        assert!(DiagnosticsConfig::default().validate().is_ok());
        for bad in [f64::NAN, f64::INFINITY, -1.0] {
            let config = DiagnosticsConfig { feasibility_tolerance: bad, ..DiagnosticsConfig::default() };
            assert!(config.validate().is_err());
        }
        let config = DiagnosticsConfig { max_per_code: 0, ..DiagnosticsConfig::default() };
        assert!(config.validate().is_err());
        assert!(LpProblem::new().diagnostics_with_config(&config).is_err());
    }

    #[test]
    fn number_formatting() {
        assert_eq!(fmt_num(0.5), "0.5");
        assert_eq!(fmt_num(-2.0), "-2");
        assert_eq!(fmt_num(0.1 + 0.2), "0.3");
        assert_eq!(fmt_num(1e12), "1.0000e12");
        assert_eq!(fmt_num(f64::INFINITY), "+inf");
    }

    #[test]
    fn truncation_is_reported() {
        let mut text = String::from("Minimize\n obj: x\nSubject To\n");
        for i in 0..5 {
            writeln!(text, " c{i}: 0 x >= 1").unwrap();
        }
        text.push_str("End");
        let problem = LpProblem::parse(&text).unwrap();
        let config = DiagnosticsConfig { max_per_code: 2, ..DiagnosticsConfig::default() };
        let result = problem.diagnostics_with_config(&config).unwrap();
        assert_eq!(result.with_code(DiagnosticCode::EmptyRowInfeasible).count(), 2);
        assert!(result.truncated.contains(&(DiagnosticCode::EmptyRowInfeasible, 3)));
        assert_eq!(result.with_code(DiagnosticCode::DiagnosticsTruncated).count(), result.truncated.len());
    }
}
