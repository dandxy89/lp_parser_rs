//! Analysis and statistics for Linear Programming problems.
//!
//! This module provides comprehensive analysis capabilities for LP problems,
//! including summary statistics, issue detection, and structural metrics.
//!
//! # Example
//!
//! ```rust
//! use lp_parser_rs::LpProblem;
//!
//! fn analyze_problem(input: &str) -> Result<(), Box<dyn std::error::Error>> {
//!     let problem = LpProblem::parse(input)?;
//!     let analysis = problem.analyze();
//!
//!     println!("Variables: {}", analysis.summary.variable_count);
//!     println!("Density: {:.2}%", analysis.summary.density * 100.0);
//!
//!     for issue in &analysis.issues {
//!         println!("[{:?}] {}", issue.severity, issue.message);
//!     }
//!     Ok(())
//! }
//! ```

use std::fmt::{Display, Formatter, Result as FmtResult};

use rustc_hash::FxHashSet;

use crate::error::EntityKind;
use crate::interner::NameId;
use crate::model::{ComparisonOp, Constraint, ConstraintClass, SOSType, VariableKind};
use crate::problem::LpProblem;

/// Configuration for analysis behaviour and thresholds.
///
/// Deserialising fills missing fields from [`Default`] and also accepts
/// camelCase field names (as editor settings use).
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(default))]
#[derive(Debug, Clone, PartialEq)]
pub struct AnalysisConfig {
    /// Coefficient magnitude threshold for "large" warnings (default: 1e9)
    #[cfg_attr(feature = "serde", serde(alias = "largeCoefficientThreshold"))]
    pub large_coefficient_threshold: f64,
    /// Small coefficient threshold for warnings (default: 1e-9)
    #[cfg_attr(feature = "serde", serde(alias = "smallCoefficientThreshold"))]
    pub small_coefficient_threshold: f64,
    /// RHS magnitude threshold for warnings (default: 1e9)
    #[cfg_attr(feature = "serde", serde(alias = "largeRhsThreshold"))]
    pub large_rhs_threshold: f64,
    /// Coefficient ratio threshold for scaling warnings (default: 1e6)
    #[cfg_attr(feature = "serde", serde(alias = "coefficientRatioThreshold"))]
    pub coefficient_ratio_threshold: f64,
}

impl Default for AnalysisConfig {
    fn default() -> Self {
        Self {
            large_coefficient_threshold: 1e9,
            small_coefficient_threshold: 1e-9,
            large_rhs_threshold: 1e9,
            coefficient_ratio_threshold: 1e6,
        }
    }
}

/// Complete analysis results for an LP problem.
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[derive(Debug, Clone)]
pub struct ProblemAnalysis {
    /// Basic summary statistics
    pub summary: ProblemSummary,
    /// Sparsity and structure metrics
    pub sparsity: SparsityMetrics,
    /// Variable analysis results
    pub variables: VariableAnalysis,
    /// Constraint analysis results
    pub constraints: ConstraintAnalysis,
    /// Coefficient analysis results
    pub coefficients: CoefficientAnalysis,
    /// Detected issues and warnings
    pub issues: Vec<AnalysisIssue>,
}

/// Basic problem summary statistics.
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[derive(Debug, Clone)]
pub struct ProblemSummary {
    /// Problem name if available
    pub name: Option<String>,
    /// Optimisation sense (Minimize/Maximize)
    pub sense: String,
    /// Number of objectives
    pub objective_count: usize,
    /// Number of constraints
    pub constraint_count: usize,
    /// Number of variables
    pub variable_count: usize,
    /// Total non-zero coefficients across all constraints
    pub total_nonzeros: usize,
    /// Matrix density (nonzeros / (constraints * variables))
    pub density: f64,
    /// Quadratic terms across all objectives
    pub quadratic_objective_terms: usize,
    /// Quadratic terms across all constraints
    pub quadratic_constraint_terms: usize,
}

/// Sparsity and structural metrics.
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[derive(Debug, Clone)]
pub struct SparsityMetrics {
    /// Minimum variables in any constraint
    pub min_vars_per_constraint: usize,
    /// Maximum variables in any constraint
    pub max_vars_per_constraint: usize,
}

/// Variable type distribution and analysis.
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[derive(Debug, Clone)]
pub struct VariableAnalysis {
    /// Distribution of variable types
    pub type_distribution: VariableTypeDistribution,
    /// Variables declared free (`x free`), and so unbounded in both directions.
    /// A variable that simply never had bounds declared is not listed here —
    /// it takes the format default of `[0, +inf)`.
    pub free_variables: Vec<String>,
    /// Variables where lower bound equals upper bound
    pub fixed_variables: Vec<FixedVariable>,
    /// Variables with inconsistent bounds (lower > upper)
    pub invalid_bounds: Vec<InvalidBound>,
    /// Variables not appearing in any constraint or objective
    pub unused_variables: Vec<String>,
    /// Count of discrete (binary + integer + general + semi-integer) variables
    pub discrete_variable_count: usize,
}

/// Distribution of variable types.
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[derive(Debug, Clone, Default)]
pub struct VariableTypeDistribution {
    /// Variables declared unbounded in both directions (`x free`).
    pub free: usize,
    /// Continuous variables with no bounds declared at all, which take the
    /// format's default of `[0, +inf)`. Counted apart from `free`: never
    /// declaring a bound is not the same as declaring it infinite.
    pub unspecified: usize,
    /// General integer variables (LP `Generals` section)
    pub general: usize,
    /// Lower-bounded only
    pub lower_bounded: usize,
    /// Upper-bounded only
    pub upper_bounded: usize,
    /// Double-bounded (both lower and upper)
    pub double_bounded: usize,
    /// Binary variables
    pub binary: usize,
    /// Integer variables
    pub integer: usize,
    /// Semi-continuous variables
    pub semi_continuous: usize,
    /// Semi-integer variables (zero, or an integer within their bounds)
    pub semi_integer: usize,
    /// SOS variables
    pub sos: usize,
}

/// A variable that is fixed (lower == upper).
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[derive(Debug, Clone)]
pub struct FixedVariable {
    /// Variable name
    pub name: String,
    /// Fixed value
    pub value: f64,
}

/// A variable with invalid bounds (lower > upper).
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[derive(Debug, Clone)]
pub struct InvalidBound {
    /// Variable name
    pub name: String,
    /// Lower bound value
    pub lower: f64,
    /// Upper bound value
    pub upper: f64,
}

/// Constraint analysis results.
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[derive(Debug, Clone)]
pub struct ConstraintAnalysis {
    /// Distribution of constraint types
    pub type_distribution: ConstraintTypeDistribution,
    /// Constraints with no variables
    pub empty_constraints: Vec<String>,
    /// Constraints with only one variable
    pub singleton_constraints: Vec<SingletonConstraint>,
    /// RHS value range statistics
    pub rhs_range: RangeStats,
    /// SOS constraint summary
    pub sos_summary: SOSSummary,
}

/// Distribution of constraint types.
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[derive(Debug, Clone, Default)]
pub struct ConstraintTypeDistribution {
    /// Equality constraints (=)
    pub equality: usize,
    /// Less-than-or-equal constraints (<=)
    pub less_than_equal: usize,
    /// Greater-than-or-equal constraints (>=)
    pub greater_than_equal: usize,
    /// Strict less-than constraints (<)
    pub less_than: usize,
    /// Strict greater-than constraints (>)
    pub greater_than: usize,
    /// SOS Type 1 constraints
    pub sos1: usize,
    /// SOS Type 2 constraints
    pub sos2: usize,
    /// Indicator constraints (`b = 1 -> ...`), not counted under an operator
    pub indicator: usize,
    /// Quadratic constraints, not counted under an operator
    pub quadratic: usize,
    /// Gurobi general constraints (`MAX`, `MIN`, `ABS`, `AND`, `OR`)
    pub general: usize,
    /// Lazy constraints (also counted under their operator above)
    pub lazy: usize,
    /// User cuts (also counted under their operator above)
    pub user_cuts: usize,
}

/// A singleton constraint (only one variable).
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[derive(Debug, Clone)]
pub struct SingletonConstraint {
    /// Constraint name
    pub name: String,
    /// The single variable in this constraint
    pub variable: String,
    /// Coefficient of the variable
    pub coefficient: f64,
    /// Comparison operator
    pub operator: String,
    /// Right-hand side value
    pub rhs: f64,
}

/// Summary of SOS constraints.
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[derive(Debug, Clone, Default)]
pub struct SOSSummary {
    /// Number of SOS Type 1 constraints
    pub s1_count: usize,
    /// Number of SOS Type 2 constraints
    pub s2_count: usize,
    /// Total variables involved in SOS constraints
    pub total_sos_variables: usize,
}

/// Coefficient analysis results.
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[derive(Debug, Clone)]
pub struct CoefficientAnalysis {
    /// Range of the absolute values of the non-zero constraint coefficients
    pub constraint_coeff_range: RangeStats,
    /// Range of the absolute values of the non-zero objective coefficients
    pub objective_coeff_range: RangeStats,
    /// Locations of very large coefficients
    pub large_coefficients: Vec<CoefficientLocation>,
    /// Locations of very small (non-zero) coefficients
    pub small_coefficients: Vec<CoefficientLocation>,
    /// Ratio of max to min absolute coefficient (scaling indicator)
    pub coefficient_ratio: f64,
}

/// Statistical range information.
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[derive(Debug, Clone, Default)]
pub struct RangeStats {
    /// Minimum value
    pub min: f64,
    /// Maximum value
    pub max: f64,
    /// Number of values
    pub count: usize,
}

impl RangeStats {
    /// Create a new empty `RangeStats` ready for incremental updates.
    const fn new() -> Self {
        Self { min: f64::INFINITY, max: f64::NEG_INFINITY, count: 0 }
    }

    /// Update the stats with a single value, avoiding intermediate allocations.
    const fn update(&mut self, value: f64) {
        self.min = self.min.min(value);
        self.max = self.max.max(value);
        self.count += 1;
    }

    /// Finalise the stats, normalising the sentinel values for empty sets.
    fn finalise(self) -> Self {
        let result = if self.count == 0 { Self::default() } else { self };
        debug_assert!(
            result.count == 0 || result.min <= result.max,
            "postcondition: finalised range min ({}) must not exceed max ({})",
            result.min,
            result.max
        );
        result
    }

    /// Build range stats from a collection of values via the incremental path.
    #[cfg(test)]
    fn from_values(values: &[f64]) -> Self {
        let mut stats = Self::new();
        for &v in values {
            stats.update(v);
        }
        stats.finalise()
    }
}

/// Location of a coefficient in the problem.
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[derive(Debug, Clone)]
pub struct CoefficientLocation {
    /// Name of the constraint or objective
    pub location: String,
    /// Whether this is in an objective (true) or constraint (false)
    pub is_objective: bool,
    /// Variable name
    pub variable: String,
    /// Coefficient value
    pub value: f64,
}

impl CoefficientLocation {
    /// The constraint or objective holding this coefficient, narrowed to its variable.
    fn subject(&self) -> IssueSubject {
        let kind = if self.is_objective { EntityKind::Objective } else { EntityKind::Constraint };
        IssueSubject { kind, name: self.location.clone(), variable: Some(self.variable.clone()) }
    }
}

/// Severity level for detected issues.
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IssueSeverity {
    /// Problem is likely unsolvable or invalid
    Error,
    /// May cause numerical issues or unexpected behaviour
    Warning,
    /// Informational only
    Info,
}

impl Display for IssueSeverity {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        match self {
            Self::Error => write!(f, "ERROR"),
            Self::Warning => write!(f, "WARNING"),
            Self::Info => write!(f, "INFO"),
        }
    }
}

/// Category of detected issue.
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IssueCategory {
    /// Invalid variable bounds
    InvalidBounds,
    /// Numerical scaling problems
    NumericalScaling,
    /// Empty constraint
    EmptyConstraint,
    /// Unused variable
    UnusedVariable,
    /// Fixed variable (may be intentional)
    FixedVariable,
    /// Singleton constraint
    SingletonConstraint,
    /// Other issues
    Other,
}

impl Display for IssueCategory {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        match self {
            Self::InvalidBounds => write!(f, "Invalid Bounds"),
            Self::NumericalScaling => write!(f, "Numerical Scaling"),
            Self::EmptyConstraint => write!(f, "Empty Constraint"),
            Self::UnusedVariable => write!(f, "Unused Variable"),
            Self::FixedVariable => write!(f, "Fixed Variable"),
            Self::SingletonConstraint => write!(f, "Singleton Constraint"),
            Self::Other => write!(f, "Other"),
        }
    }
}

/// A detected issue in the LP problem.
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[derive(Debug, Clone)]
pub struct AnalysisIssue {
    /// Severity of the issue
    pub severity: IssueSeverity,
    /// Category of the issue
    pub category: IssueCategory,
    /// Human-readable message
    pub message: String,
    /// Additional details if available
    pub details: Option<String>,
    /// The entity the issue is about, for locating it in the source; `None`
    /// for problem-wide issues.
    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "Option::is_none"))]
    pub subject: Option<IssueSubject>,
}

/// The entity an [`AnalysisIssue`] refers to.
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IssueSubject {
    /// Kind of entity.
    pub kind: EntityKind,
    /// Name of the variable, constraint or objective (possibly generated,
    /// e.g. `C1` or `c1_rng`, for unnamed or ranged constraints).
    pub name: String,
    /// For coefficient issues, the variable within the constraint or objective.
    pub variable: Option<String>,
}

impl IssueSubject {
    /// Subject naming a variable.
    #[must_use]
    pub fn variable(name: impl Into<String>) -> Self {
        Self { kind: EntityKind::Variable, name: name.into(), variable: None }
    }

    /// Subject naming a constraint.
    #[must_use]
    pub fn constraint(name: impl Into<String>) -> Self {
        Self { kind: EntityKind::Constraint, name: name.into(), variable: None }
    }
}

impl Display for AnalysisIssue {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        write!(f, "[{}] {}", self.severity, self.message)?;
        if let Some(ref details) = self.details {
            write!(f, " ({details})")?;
        }
        Ok(())
    }
}

impl Display for ProblemAnalysis {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        writeln!(f, "=== LP Problem Analysis ===")?;
        writeln!(f)?;

        // Summary
        writeln!(f, "Summary:")?;
        if let Some(ref name) = self.summary.name {
            writeln!(f, "  Name: {name}")?;
        }
        writeln!(f, "  Sense: {}", self.summary.sense)?;
        writeln!(
            f,
            "  Objectives: {} | Constraints: {} | Variables: {}",
            self.summary.objective_count, self.summary.constraint_count, self.summary.variable_count
        )?;
        writeln!(f, "  Non-zeros: {} | Density: {:.2}%", self.summary.total_nonzeros, self.summary.density * 100.0)?;
        if self.summary.quadratic_objective_terms > 0 || self.summary.quadratic_constraint_terms > 0 {
            writeln!(
                f,
                "  Quadratic terms: {} in objectives | {} in constraints",
                self.summary.quadratic_objective_terms, self.summary.quadratic_constraint_terms
            )?;
        }
        writeln!(f)?;

        // Sparsity
        writeln!(f, "Sparsity:")?;
        writeln!(f, "  Vars per constraint: min={}, max={}", self.sparsity.min_vars_per_constraint, self.sparsity.max_vars_per_constraint)?;
        writeln!(f)?;

        // Variable types
        writeln!(f, "Variable Types:")?;
        let vt = &self.variables.type_distribution;
        writeln!(
            f,
            "  Continuous: {} | Binary: {} | Integer: {}",
            vt.free + vt.unspecified + vt.lower_bounded + vt.upper_bounded + vt.double_bounded,
            vt.binary,
            vt.integer + vt.general
        )?;
        if vt.semi_continuous > 0 {
            writeln!(f, "  Semi-continuous: {}", vt.semi_continuous)?;
        }
        if vt.semi_integer > 0 {
            writeln!(f, "  Semi-integer: {}", vt.semi_integer)?;
        }
        if vt.sos > 0 {
            writeln!(f, "  SOS: {}", vt.sos)?;
        }
        writeln!(f)?;

        // Constraint types
        writeln!(f, "Constraint Types:")?;
        let ct = &self.constraints.type_distribution;
        writeln!(f, "  Equality (=): {} | (<=): {} | (>=): {}", ct.equality, ct.less_than_equal, ct.greater_than_equal)?;
        if ct.less_than > 0 || ct.greater_than > 0 {
            writeln!(f, "  Strict: (<): {} | (>): {}", ct.less_than, ct.greater_than)?;
        }
        if ct.indicator > 0 {
            writeln!(f, "  Indicator: {}", ct.indicator)?;
        }
        if ct.quadratic > 0 {
            writeln!(f, "  Quadratic: {}", ct.quadratic)?;
        }
        if ct.general > 0 {
            writeln!(f, "  General: {}", ct.general)?;
        }
        if ct.lazy > 0 || ct.user_cuts > 0 {
            writeln!(f, "  Lazy: {} | User cuts: {}", ct.lazy, ct.user_cuts)?;
        }
        if ct.sos1 > 0 || ct.sos2 > 0 {
            writeln!(f, "  SOS1: {} | SOS2: {}", ct.sos1, ct.sos2)?;
        }
        writeln!(f)?;

        // Coefficient analysis
        if self.coefficients.constraint_coeff_range.count > 0 {
            writeln!(f, "Coefficient Analysis:")?;
            let cr = &self.coefficients.constraint_coeff_range;
            writeln!(f, "  Constraint coeffs: min={:.2e}, max={:.2e}", cr.min, cr.max)?;
            if self.coefficients.objective_coeff_range.count > 0 {
                let or = &self.coefficients.objective_coeff_range;
                writeln!(f, "  Objective coeffs: min={:.2e}, max={:.2e}", or.min, or.max)?;
            }
            if self.coefficients.coefficient_ratio > 1.0 {
                writeln!(f, "  Coefficient ratio: {:.2e}", self.coefficients.coefficient_ratio)?;
            }
            writeln!(f)?;
        }

        // Issues
        if self.issues.is_empty() {
            writeln!(f, "No issues detected.")?;
        } else {
            writeln!(f, "Issues Found: {}", self.issues.len())?;
            for issue in &self.issues {
                writeln!(f, "  {issue}")?;
            }
        }

        Ok(())
    }
}

/// Collect coefficient statistics for one location, classifying each
/// coefficient as normal, large, or small.
#[allow(clippy::too_many_arguments)]
fn collect_coefficient_stats(
    coefficients: &[crate::model::Coefficient],
    location_name: &str,
    is_objective: bool,
    config: &AnalysisConfig,
    interner: &crate::interner::NameInterner,
    range: &mut RangeStats,
    large: &mut Vec<CoefficientLocation>,
    small: &mut Vec<CoefficientLocation>,
) {
    debug_assert!(!location_name.is_empty(), "location name must not be empty");
    debug_assert!(
        config.small_coefficient_threshold <= config.large_coefficient_threshold,
        "small threshold must not exceed large threshold"
    );

    for coeff in coefficients {
        let abs_value = coeff.value.abs();
        if abs_value == 0.0 {
            // An explicit zero carries no scale; letting it into the range
            // would make the minimum 0 and drop this whole range from the ratio.
            continue;
        }
        range.update(abs_value);

        if abs_value > config.large_coefficient_threshold {
            large.push(CoefficientLocation {
                location: location_name.to_string(),
                is_objective,
                variable: interner.resolve(coeff.name).to_string(),
                value: coeff.value,
            });
        } else if abs_value < config.small_coefficient_threshold {
            small.push(CoefficientLocation {
                location: location_name.to_string(),
                is_objective,
                variable: interner.resolve(coeff.name).to_string(),
                value: coeff.value,
            });
        }
    }
}

/// Compute the ratio of max to min absolute coefficient across all coefficients.
fn compute_coefficient_ratio(constraint_range: &RangeStats, objective_range: &RangeStats) -> f64 {
    // Combine the two ranges to find global min/max of positive abs values.
    let has_values = constraint_range.count > 0 || objective_range.count > 0;

    if !has_values {
        return 1.0;
    }

    // Both ranges track abs values, so min is the smallest positive and max is the largest.
    let mut global_min = f64::INFINITY;
    let mut global_max: f64 = 0.0;
    let mut has_positive = false;

    for range in [constraint_range, objective_range] {
        if range.count > 0 && range.max > 0.0 {
            has_positive = true;
            debug_assert!(range.min > 0.0, "ranges hold non-zero magnitudes only");
            if range.min < global_min {
                global_min = range.min;
            }
            if range.max > global_max {
                global_max = range.max;
            }
        }
    }

    let ratio = if has_positive && global_min > 0.0 && global_min < f64::INFINITY { global_max / global_min } else { 1.0 };
    debug_assert!(
        !has_positive || ratio >= 1.0 || global_min == f64::INFINITY,
        "postcondition: coefficient_ratio must be >= 1.0 when coefficients exist, got: {ratio}"
    );
    ratio
}

impl LpProblem {
    /// Perform comprehensive analysis on the LP problem with default configuration.
    #[must_use]
    pub fn analyze(&self) -> ProblemAnalysis {
        self.analyze_with_config(&AnalysisConfig::default())
    }

    /// Perform comprehensive analysis with custom configuration.
    #[must_use]
    pub fn analyze_with_config(&self, config: &AnalysisConfig) -> ProblemAnalysis {
        let summary = self.compute_summary();
        let sparsity = self.compute_sparsity_metrics();
        let variables = self.analyze_variables();
        let constraints = self.analyze_constraints();
        let coefficients = self.analyze_coefficients(config);
        let issues = Self::detect_issues(&summary, &variables, &constraints, &coefficients, config);

        ProblemAnalysis { summary, sparsity, variables, constraints, coefficients, issues }
    }

    /// Compute basic summary statistics.
    fn compute_summary(&self) -> ProblemSummary {
        let total_nonzeros = self.count_nonzeros();
        let constraint_count = self.constraint_count();
        let variable_count = self.variable_count();

        #[allow(clippy::cast_precision_loss)]
        let density = if constraint_count > 0 && variable_count > 0 {
            total_nonzeros as f64 / (constraint_count as f64 * variable_count as f64)
        } else {
            0.0
        };

        debug_assert!(density >= 0.0, "postcondition: density must be non-negative, got: {density}");

        ProblemSummary {
            name: self.name.as_ref().map(std::string::ToString::to_string),
            sense: self.sense.to_string(),
            objective_count: self.objective_count(),
            constraint_count,
            variable_count,
            total_nonzeros,
            density,
            quadratic_objective_terms: self.objectives.values().map(|o| o.quadratic.len()).sum(),
            quadratic_constraint_terms: self
                .constraints
                .values()
                .map(|c| if let Constraint::Quadratic { quadratic, .. } = c { quadratic.len() } else { 0 })
                .sum(),
        }
    }

    /// Count total non-zero coefficients in constraints.
    fn count_nonzeros(&self) -> usize {
        self.constraints
            .values()
            .map(|c| match c {
                // Linear nonzeros only; quadratic terms are counted in the summary.
                Constraint::Standard { coefficients, .. }
                | Constraint::Indicator { coefficients, .. }
                | Constraint::Quadratic { coefficients, .. } => coefficients.len(),
                Constraint::SOS { weights, .. } => weights.len(),
                Constraint::General { .. } => 0,
            })
            .sum()
    }

    /// Compute sparsity metrics.
    fn compute_sparsity_metrics(&self) -> SparsityMetrics {
        let (min_v, max_v) = self.constraints.values().fold((usize::MAX, 0usize), |(min_v, max_v), c| {
            let n = match c {
                Constraint::Standard { coefficients, .. }
                | Constraint::Indicator { coefficients, .. }
                | Constraint::Quadratic { coefficients, .. } => coefficients.len(),
                Constraint::SOS { weights, .. } => weights.len(),
                Constraint::General { function, .. } => 1 + function.variables().len(),
            };
            (min_v.min(n), max_v.max(n))
        });
        let metrics =
            SparsityMetrics { min_vars_per_constraint: if min_v == usize::MAX { 0 } else { min_v }, max_vars_per_constraint: max_v };
        debug_assert!(
            metrics.min_vars_per_constraint <= metrics.max_vars_per_constraint,
            "postcondition: min vars per constraint ({}) must not exceed max ({})",
            metrics.min_vars_per_constraint,
            metrics.max_vars_per_constraint
        );
        metrics
    }

    /// Analyze variable types, bounds, and usage.
    fn analyze_variables(&self) -> VariableAnalysis {
        let mut type_distribution = VariableTypeDistribution::default();
        let mut free_variables = Vec::new();
        let mut fixed_variables = Vec::new();
        let mut invalid_bounds = Vec::new();

        for (name_id, variable) in &self.variables {
            let name_str = self.interner.resolve(*name_id);
            // Count kind and bounds shape separately so integer-with-bounds is visible.
            match variable.kind {
                VariableKind::Binary => type_distribution.binary += 1,
                VariableKind::Integer => type_distribution.integer += 1,
                VariableKind::General => type_distribution.general += 1,
                VariableKind::SemiContinuous => type_distribution.semi_continuous += 1,
                VariableKind::SemiInteger => type_distribution.semi_integer += 1,
                VariableKind::Sos => type_distribution.sos += 1,
                // Declared-free is checked before the per-side shapes: it is
                // stored as an explicit [-inf, +inf], which would otherwise read
                // as an ordinary double bound.
                VariableKind::Continuous if variable.bounds.is_free() => {
                    type_distribution.free += 1;
                    free_variables.push(name_str.to_string());
                }
                VariableKind::Continuous => match (variable.bounds.lower, variable.bounds.upper) {
                    (None, None) => type_distribution.unspecified += 1,
                    (Some(_), None) => type_distribution.lower_bounded += 1,
                    (None, Some(_)) => type_distribution.upper_bounded += 1,
                    (Some(lower), Some(upper)) => {
                        type_distribution.double_bounded += 1;
                        if (lower - upper).abs() < f64::EPSILON {
                            fixed_variables.push(FixedVariable { name: name_str.to_string(), value: lower });
                        } else if lower > upper {
                            invalid_bounds.push(InvalidBound { name: name_str.to_string(), lower, upper });
                        }
                    }
                },
            }
            // Also flag fixed/invalid bounds on non-continuous kinds.
            if variable.kind != VariableKind::Continuous
                && let (Some(lower), Some(upper)) = (variable.bounds.lower, variable.bounds.upper)
            {
                if (lower - upper).abs() < f64::EPSILON {
                    fixed_variables.push(FixedVariable { name: name_str.to_string(), value: lower });
                } else if lower > upper {
                    invalid_bounds.push(InvalidBound { name: name_str.to_string(), lower, upper });
                }
            }
        }

        let unused_variables = self.find_unused_variables();
        let discrete_variable_count =
            type_distribution.binary + type_distribution.integer + type_distribution.general + type_distribution.semi_integer;

        debug_assert_eq!(
            type_distribution.free
                + type_distribution.unspecified
                + type_distribution.general
                + type_distribution.lower_bounded
                + type_distribution.upper_bounded
                + type_distribution.double_bounded
                + type_distribution.binary
                + type_distribution.integer
                + type_distribution.semi_continuous
                + type_distribution.semi_integer
                + type_distribution.sos,
            self.variables.len(),
            "postcondition: type distribution must sum to total variable count"
        );

        VariableAnalysis { type_distribution, free_variables, fixed_variables, invalid_bounds, unused_variables, discrete_variable_count }
    }

    /// Find variables declared but not referenced in any objective or constraint.
    fn find_unused_variables(&self) -> Vec<String> {
        let mut used_variables: FxHashSet<NameId> = FxHashSet::default();

        for objective in self.objectives.values() {
            for coeff in &objective.coefficients {
                used_variables.insert(coeff.name);
            }
            // A variable appearing only in quadratic terms is used too.
            used_variables.extend(objective.quadratic.iter().flat_map(|term| [term.var1, term.var2]));
        }

        for constraint in self.constraints.values() {
            constraint.for_each_variable(|id| {
                used_variables.insert(id);
            });
        }

        let unused: Vec<String> = self
            .variables
            .keys()
            .filter(|name_id| !used_variables.contains(name_id))
            .map(|id| self.interner.resolve(*id).to_string())
            .collect();
        debug_assert!(
            unused.len() <= self.variables.len(),
            "postcondition: unused variable count ({}) cannot exceed total variables ({})",
            unused.len(),
            self.variables.len()
        );
        unused
    }

    /// Analyze constraints.
    fn analyze_constraints(&self) -> ConstraintAnalysis {
        let mut type_distribution = ConstraintTypeDistribution::default();
        let mut empty_constraints = Vec::new();
        let mut singleton_constraints = Vec::new();
        let mut rhs_range = RangeStats::new();
        let mut sos_summary = SOSSummary::default();

        for (name_id, constraint) in &self.constraints {
            let name_str = self.interner.resolve(*name_id);
            match constraint {
                Constraint::Standard { coefficients, operator, rhs, .. } => {
                    match operator {
                        ComparisonOp::EQ => type_distribution.equality += 1,
                        ComparisonOp::LTE => type_distribution.less_than_equal += 1,
                        ComparisonOp::GTE => type_distribution.greater_than_equal += 1,
                        ComparisonOp::LT => type_distribution.less_than += 1,
                        ComparisonOp::GT => type_distribution.greater_than += 1,
                    }

                    rhs_range.update(*rhs);

                    if coefficients.is_empty() {
                        empty_constraints.push(name_str.to_string());
                    } else if coefficients.len() == 1 {
                        let coeff = &coefficients[0];
                        singleton_constraints.push(SingletonConstraint {
                            name: name_str.to_string(),
                            variable: self.interner.resolve(coeff.name).to_string(),
                            coefficient: coeff.value,
                            operator: operator.to_string(),
                            rhs: *rhs,
                        });
                    }
                }
                Constraint::Indicator { rhs, .. } => {
                    type_distribution.indicator += 1;
                    rhs_range.update(*rhs);
                }
                Constraint::Quadratic { rhs, .. } => {
                    type_distribution.quadratic += 1;
                    rhs_range.update(*rhs);
                }
                Constraint::General { .. } => type_distribution.general += 1,
                Constraint::SOS { sos_type, weights, .. } => {
                    match sos_type {
                        SOSType::S1 => {
                            type_distribution.sos1 += 1;
                            sos_summary.s1_count += 1;
                        }
                        SOSType::S2 => {
                            type_distribution.sos2 += 1;
                            sos_summary.s2_count += 1;
                        }
                    }
                    sos_summary.total_sos_variables += weights.len();
                }
            }
        }

        debug_assert_eq!(
            type_distribution.equality
                + type_distribution.less_than_equal
                + type_distribution.greater_than_equal
                + type_distribution.less_than
                + type_distribution.greater_than
                + type_distribution.sos1
                + type_distribution.sos2
                + type_distribution.indicator
                + type_distribution.quadratic
                + type_distribution.general,
            self.constraints.len(),
            "postcondition: constraint type distribution must sum to total constraint count"
        );

        for class in self.constraint_classes.values() {
            match class {
                ConstraintClass::Lazy => type_distribution.lazy += 1,
                ConstraintClass::UserCut => type_distribution.user_cuts += 1,
                ConstraintClass::Normal => debug_assert!(false, "constraint_classes must not store the default class"),
            }
        }

        ConstraintAnalysis { type_distribution, empty_constraints, singleton_constraints, rhs_range: rhs_range.finalise(), sos_summary }
    }

    /// Analyze coefficients.
    fn analyze_coefficients(&self, config: &AnalysisConfig) -> CoefficientAnalysis {
        let mut constraint_range = RangeStats::new();
        let mut objective_range = RangeStats::new();
        let mut large_coefficients = Vec::new();
        let mut small_coefficients = Vec::new();

        for (name_id, constraint) in &self.constraints {
            if let Constraint::Standard { coefficients, .. } = constraint {
                let name_str = self.interner.resolve(*name_id);
                collect_coefficient_stats(
                    coefficients,
                    name_str,
                    false,
                    config,
                    &self.interner,
                    &mut constraint_range,
                    &mut large_coefficients,
                    &mut small_coefficients,
                );
            }
        }

        for (name_id, objective) in &self.objectives {
            let name_str = self.interner.resolve(*name_id);
            collect_coefficient_stats(
                &objective.coefficients,
                name_str,
                true,
                config,
                &self.interner,
                &mut objective_range,
                &mut large_coefficients,
                &mut small_coefficients,
            );
        }

        let constraint_coeff_range = constraint_range.finalise();
        let objective_coeff_range = objective_range.finalise();
        let coefficient_ratio = compute_coefficient_ratio(&constraint_coeff_range, &objective_coeff_range);
        debug_assert!(coefficient_ratio >= 1.0, "postcondition: coefficient ratio must be >= 1.0, got: {coefficient_ratio}");

        CoefficientAnalysis { constraint_coeff_range, objective_coeff_range, large_coefficients, small_coefficients, coefficient_ratio }
    }

    /// Detect issues and generate warnings.
    fn detect_issues(
        summary: &ProblemSummary,
        variables: &VariableAnalysis,
        constraints: &ConstraintAnalysis,
        coefficients: &CoefficientAnalysis,
        config: &AnalysisConfig,
    ) -> Vec<AnalysisIssue> {
        let mut issues = Vec::new();

        // Invalid bounds (ERROR)
        for invalid in &variables.invalid_bounds {
            issues.push(AnalysisIssue {
                severity: IssueSeverity::Error,
                category: IssueCategory::InvalidBounds,
                message: format!("Variable '{}' has invalid bounds: lower ({}) > upper ({})", invalid.name, invalid.lower, invalid.upper),
                details: None,
                subject: Some(IssueSubject::variable(&invalid.name)),
            });
        }

        // Empty constraints (WARNING)
        for name in &constraints.empty_constraints {
            issues.push(AnalysisIssue {
                severity: IssueSeverity::Warning,
                category: IssueCategory::EmptyConstraint,
                message: format!("Constraint '{name}' has no variables"),
                details: None,
                subject: Some(IssueSubject::constraint(name)),
            });
        }

        // Over-constrained check (WARNING) - may indicate degeneracy
        if summary.constraint_count >= summary.variable_count && summary.variable_count > 0 {
            issues.push(AnalysisIssue {
                severity: IssueSeverity::Warning,
                category: IssueCategory::Other,
                message: format!(
                    "Problem may be over-constrained: {} constraints for {} variables",
                    summary.constraint_count, summary.variable_count
                ),
                details: Some("Over-constrained problems often have degenerate or infeasible solutions".to_string()),
                subject: None,
            });
        }

        // Large RHS warning: by magnitude, so a hugely negative RHS counts too.
        if constraints.rhs_range.count > 0 {
            let (min, max) = (constraints.rhs_range.min, constraints.rhs_range.max);
            let extreme = if min.abs() > max.abs() { min } else { max };
            if extreme.abs() > config.large_rhs_threshold {
                issues.push(AnalysisIssue {
                    severity: IssueSeverity::Warning,
                    category: IssueCategory::NumericalScaling,
                    message: format!("Large RHS value ({extreme:.2e}) may cause numerical issues"),
                    details: None,
                    subject: None,
                });
            }
        }

        // Large coefficient ratio (WARNING)
        if coefficients.coefficient_ratio > config.coefficient_ratio_threshold {
            issues.push(AnalysisIssue {
                severity: IssueSeverity::Warning,
                category: IssueCategory::NumericalScaling,
                message: format!("Large coefficient ratio ({:.2e}) may cause numerical instability", coefficients.coefficient_ratio),
                details: Some("Consider rescaling the problem".to_string()),
                subject: None,
            });
        }

        // Large and small (non-zero) coefficients
        for (size, locations) in [("Large", &coefficients.large_coefficients), ("Small", &coefficients.small_coefficients)] {
            for loc in locations {
                issues.push(AnalysisIssue {
                    severity: IssueSeverity::Warning,
                    category: IssueCategory::NumericalScaling,
                    message: format!(
                        "{size} coefficient ({:.2e}) for variable '{}' in {}",
                        loc.value,
                        loc.variable,
                        if loc.is_objective { "objective" } else { "constraint" }
                    ),
                    details: Some(loc.location.clone()),
                    subject: Some(loc.subject()),
                });
            }
        }

        // Fixed variables (INFO)
        for fixed in &variables.fixed_variables {
            issues.push(AnalysisIssue {
                severity: IssueSeverity::Info,
                category: IssueCategory::FixedVariable,
                message: format!("Variable '{}' is fixed at value {}", fixed.name, fixed.value),
                details: None,
                subject: Some(IssueSubject::variable(&fixed.name)),
            });
        }

        // Singleton constraints (INFO)
        if !constraints.singleton_constraints.is_empty() {
            issues.push(AnalysisIssue {
                severity: IssueSeverity::Info,
                category: IssueCategory::SingletonConstraint,
                message: format!(
                    "{} singleton constraint(s) detected (may represent simple bounds)",
                    constraints.singleton_constraints.len()
                ),
                details: None,
                subject: None,
            });
        }

        // Unused variables (INFO)
        for name in &variables.unused_variables {
            issues.push(AnalysisIssue {
                severity: IssueSeverity::Info,
                category: IssueCategory::UnusedVariable,
                message: format!("Variable '{name}' is not used in any constraint or objective"),
                details: None,
                subject: Some(IssueSubject::variable(name)),
            });
        }

        issues
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Coefficient, Constraint, Objective, Variable, VariableType};

    #[test]
    fn test_range_stats_empty() {
        let stats = RangeStats::from_values(&[]);
        assert_eq!(stats.count, 0);
    }

    #[test]
    #[allow(clippy::float_cmp)]
    fn test_range_stats_single() {
        let stats = RangeStats::from_values(&[5.0]);
        assert_eq!(stats.count, 1);
        assert_eq!(stats.min, 5.0);
        assert_eq!(stats.max, 5.0);
    }

    #[test]
    #[allow(clippy::float_cmp)]
    fn test_range_stats_multiple() {
        let stats = RangeStats::from_values(&[1.0, 2.0, 3.0, 4.0, 5.0]);
        assert_eq!(stats.count, 5);
        assert_eq!(stats.min, 1.0);
        assert_eq!(stats.max, 5.0);
    }

    #[test]
    fn test_issue_severity_display() {
        assert_eq!(IssueSeverity::Error.to_string(), "ERROR");
        assert_eq!(IssueSeverity::Warning.to_string(), "WARNING");
        assert_eq!(IssueSeverity::Info.to_string(), "INFO");
    }

    #[test]
    fn test_issue_category_display() {
        assert_eq!(IssueCategory::InvalidBounds.to_string(), "Invalid Bounds");
        assert_eq!(IssueCategory::NumericalScaling.to_string(), "Numerical Scaling");
    }

    #[test]
    fn test_analysis_issue_display() {
        let issue = AnalysisIssue {
            severity: IssueSeverity::Warning,
            category: IssueCategory::NumericalScaling,
            message: "Test message".to_string(),
            details: Some("Details here".to_string()),
            subject: None,
        };
        let display = issue.to_string();
        assert!(display.contains("WARNING"));
        assert!(display.contains("Test message"));
        assert!(display.contains("Details here"));
    }

    #[test]
    #[allow(clippy::float_cmp)]
    fn test_default_config() {
        let config = AnalysisConfig::default();
        assert_eq!(config.large_coefficient_threshold, 1e9);
        assert_eq!(config.small_coefficient_threshold, 1e-9);
    }

    /// Add a standard constraint over the named variables with unit coefficients.
    fn add_standard_constraint(problem: &mut LpProblem, name: &str, variables: &[&str], operator: ComparisonOp, rhs: f64) {
        let name_id = problem.intern(name);
        let coefficients = variables.iter().map(|v| Coefficient { name: problem.intern(v), value: 1.0 }).collect();
        problem.add_constraint(Constraint::Standard { name: name_id, coefficients, operator, rhs, byte_offset: None });
    }

    #[test]
    #[allow(clippy::float_cmp)]
    fn test_analyze_truly_empty_problem() {
        let analysis = LpProblem::new().analyze();
        assert_eq!(analysis.summary.objective_count, 0);
        assert_eq!(analysis.summary.constraint_count, 0);
        assert_eq!(analysis.summary.variable_count, 0);
        assert_eq!(analysis.summary.total_nonzeros, 0);
        assert_eq!(analysis.summary.density, 0.0);
        assert_eq!(analysis.coefficients.coefficient_ratio, 1.0);
        assert!(analysis.issues.is_empty(), "an empty problem must raise no issues: {:?}", analysis.issues);
    }

    #[test]
    #[allow(clippy::float_cmp)]
    fn test_fixed_variable_reports_info_issue() {
        let mut problem = LpProblem::new();
        let x_id = problem.intern("x");
        problem.add_variable(Variable::new(x_id).with_var_type(VariableType::DoubleBound(5.0, 5.0)));

        let analysis = problem.analyze();
        assert_eq!(analysis.variables.fixed_variables.len(), 1);
        assert_eq!(analysis.variables.fixed_variables[0].name, "x");
        assert_eq!(analysis.variables.fixed_variables[0].value, 5.0);
        assert!(
            analysis.issues.iter().any(|i| i.severity == IssueSeverity::Info && i.category == IssueCategory::FixedVariable),
            "expected a FixedVariable Info issue: {:?}",
            analysis.issues
        );
    }

    #[test]
    #[allow(clippy::float_cmp)]
    fn test_invalid_bounds_report_error_issue() {
        let mut problem = LpProblem::new();
        let x_id = problem.intern("x");
        problem.add_variable(Variable::new(x_id).with_var_type(VariableType::DoubleBound(10.0, 1.0)));

        let analysis = problem.analyze();
        assert_eq!(analysis.variables.invalid_bounds.len(), 1);
        assert_eq!(analysis.variables.invalid_bounds[0].name, "x");
        assert_eq!(analysis.variables.invalid_bounds[0].lower, 10.0);
        assert_eq!(analysis.variables.invalid_bounds[0].upper, 1.0);
        assert!(
            analysis.issues.iter().any(|i| i.severity == IssueSeverity::Error && i.category == IssueCategory::InvalidBounds),
            "expected an InvalidBounds Error issue: {:?}",
            analysis.issues
        );
    }

    #[test]
    fn test_empty_constraint_reports_warning_issue() {
        let mut problem = LpProblem::new();
        add_standard_constraint(&mut problem, "empty_c", &[], ComparisonOp::LTE, 5.0);
        // A second, populated constraint so the problem is not degenerate.
        add_standard_constraint(&mut problem, "c1", &["x", "y", "z"], ComparisonOp::LTE, 10.0);

        let analysis = problem.analyze();
        assert_eq!(analysis.constraints.empty_constraints, vec!["empty_c".to_string()]);
        assert!(
            analysis.issues.iter().any(|i| i.severity == IssueSeverity::Warning && i.category == IssueCategory::EmptyConstraint),
            "expected an EmptyConstraint Warning issue: {:?}",
            analysis.issues
        );
    }

    #[test]
    fn test_over_constrained_warning_boundary() {
        // Exactly as many constraints as variables: warning fires.
        let mut equal = LpProblem::new();
        add_standard_constraint(&mut equal, "c1", &["x", "y"], ComparisonOp::LTE, 10.0);
        add_standard_constraint(&mut equal, "c2", &["x"], ComparisonOp::GTE, 1.0);
        let analysis = equal.analyze();
        assert_eq!(analysis.summary.constraint_count, analysis.summary.variable_count);
        assert!(
            analysis.issues.iter().any(|i| i.category == IssueCategory::Other && i.message.contains("over-constrained")),
            "expected an over-constrained warning at the boundary: {:?}",
            analysis.issues
        );

        // One fewer constraint than variables: no warning.
        let mut under = LpProblem::new();
        add_standard_constraint(&mut under, "c1", &["x", "y"], ComparisonOp::LTE, 10.0);
        let analysis = under.analyze();
        assert!(
            !analysis.issues.iter().any(|i| i.message.contains("over-constrained")),
            "no over-constrained warning expected below the boundary: {:?}",
            analysis.issues
        );
    }

    #[test]
    #[allow(clippy::float_cmp)]
    fn test_zero_coefficient_does_not_hide_the_coefficient_ratio() {
        let problem = LpProblem::parse("min\n obj: x\nst\n c1: 0 x + 0.001 y + 1000 z >= 1\nend").unwrap();
        let analysis = problem.analyze();
        assert_eq!(analysis.summary.total_nonzeros, 3, "the explicit zero must be kept for this test to mean anything");
        assert_eq!(analysis.coefficients.constraint_coeff_range.min, 0.001);
        assert!((analysis.coefficients.coefficient_ratio - 1e6).abs() < 1.0, "ratio: {}", analysis.coefficients.coefficient_ratio);
    }

    #[test]
    fn test_large_negative_rhs_is_flagged() {
        let problem = LpProblem::parse("min\n obj: x\nst\n c1: x >= -1e12\n c2: x <= 5\nend").unwrap();
        let analysis = problem.analyze();
        assert!(
            analysis.issues.iter().any(|i| i.message.contains("Large RHS value (-1.00e12)")),
            "a -1e12 RHS must be flagged: {:?}",
            analysis.issues
        );
    }

    #[test]
    fn test_small_coefficient_is_reported_as_an_issue() {
        let problem = LpProblem::parse("min\n obj: x\nst\n c1: 1e-12 x + y >= 1\nend").unwrap();
        let analysis = problem.analyze();
        assert!(
            analysis.issues.iter().any(|i| i.message.contains("Small coefficient (1.00e-12) for variable 'x'")),
            "a coefficient below the small threshold must be reported: {:?}",
            analysis.issues
        );
        let lenient = AnalysisConfig { small_coefficient_threshold: 1e-15, ..AnalysisConfig::default() };
        assert!(!problem.analyze_with_config(&lenient).issues.iter().any(|i| i.message.starts_with("Small coefficient")));
    }

    #[test]
    fn test_general_variables_count_as_integer() {
        let problem = LpProblem::parse("min\n obj: x + y + z\nst\n c1: x + y + z >= 1\ngenerals\n x\nintegers\n y\nend").unwrap();
        let analysis = problem.analyze();
        assert_eq!(analysis.variables.discrete_variable_count, 2, "general x and integer y are discrete");
        let text = analysis.to_string();
        assert!(text.contains("Continuous: 1 | Binary: 0 | Integer: 2"), "{text}");
    }

    #[test]
    fn test_unused_variable_reports_info_issue() {
        let mut problem = LpProblem::new();
        let obj_id = problem.intern("obj");
        let x_id = problem.intern("x");
        problem.add_objective(Objective {
            name: obj_id,
            coefficients: vec![Coefficient { name: x_id, value: 1.0 }],
            constant: 0.0,
            quadratic: Vec::new(),
            attributes: crate::model::ObjectiveAttributes::default(),
            byte_offset: None,
        });
        add_standard_constraint(&mut problem, "c1", &["x"], ComparisonOp::GTE, 1.0);
        // `unused` is declared (as if via Bounds) but referenced nowhere.
        let unused_id = problem.intern("unused");
        problem.add_variable(Variable::new(unused_id).with_var_type(VariableType::LowerBound(0.0)));

        let analysis = problem.analyze();
        assert_eq!(analysis.variables.unused_variables, vec!["unused".to_string()]);
        assert!(
            analysis
                .issues
                .iter()
                .any(|i| i.severity == IssueSeverity::Info && i.category == IssueCategory::UnusedVariable && i.message.contains("unused")),
            "expected an UnusedVariable Info issue: {:?}",
            analysis.issues
        );
    }

    #[test]
    fn test_objective_quadratic_variable_is_not_unused() {
        let problem = LpProblem::parse("min\n obj: x + [ y ^ 2 + 2 x * z ] / 2\nst\n c1: x >= 1\nend").unwrap();
        let analysis = problem.analyze();
        assert!(analysis.variables.unused_variables.is_empty(), "{:?}", analysis.variables.unused_variables);
    }

    #[test]
    fn test_issue_subjects_locate_entities() {
        let text =
            "min\n obj: 1e12 x + y\nst\n c1: 0 x >= 1\n c2: y + 1e-12 z >= 1\n 2 <= x + y <= 8\nbounds\n 5 <= y <= 1\n z = 3\n w >= 0\nend";
        let mut problem = LpProblem::parse(text).unwrap();
        add_standard_constraint(&mut problem, "e1", &[], ComparisonOp::LTE, 5.0);
        let analysis = problem.analyze();
        let subject = |category: IssueCategory| -> Vec<Option<IssueSubject>> {
            analysis.issues.iter().filter(|i| i.category == category).map(|i| i.subject.clone()).collect()
        };

        assert_eq!(subject(IssueCategory::InvalidBounds), [Some(IssueSubject::variable("y"))]);
        assert_eq!(subject(IssueCategory::FixedVariable), [Some(IssueSubject::variable("z"))]);
        assert_eq!(subject(IssueCategory::UnusedVariable), [Some(IssueSubject::variable("w"))]);
        assert_eq!(subject(IssueCategory::SingletonConstraint), [None]);
        let empty = analysis.issues.iter().find(|i| i.category == IssueCategory::EmptyConstraint).unwrap();
        assert_eq!(empty.subject, Some(IssueSubject::constraint("e1")));
        let coefficients: Vec<IssueSubject> = analysis
            .issues
            .iter()
            .filter(|i| i.category == IssueCategory::NumericalScaling && i.message.contains("coefficient ("))
            .filter_map(|i| i.subject.clone())
            .collect();
        assert_eq!(
            coefficients,
            [
                IssueSubject { kind: EntityKind::Objective, name: "obj".to_owned(), variable: Some("x".to_owned()) },
                IssueSubject { kind: EntityKind::Constraint, name: "c2".to_owned(), variable: Some("z".to_owned()) },
            ]
        );
        // Problem-wide issues carry no subject, and the subject leaves the text unchanged.
        let ratio = analysis.issues.iter().find(|i| i.message.contains("coefficient ratio")).unwrap();
        assert_eq!(ratio.subject, None);
        assert_eq!(empty.to_string(), "[WARNING] Constraint 'e1' has no variables");
    }
}
