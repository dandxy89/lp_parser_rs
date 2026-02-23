//! Analysis and statistics for Linear Programming problems.
//!
//! This module provides comprehensive analysis capabilities for LP problems,
//! including summary statistics, issue detection, and structural metrics.
//!
//! # Example
//!
//! ```rust
//! use lp_parser::{LpProblem, analysis::ProblemAnalysis};
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

use std::collections::HashSet;
use std::fmt::{Display, Formatter, Result as FmtResult};

use crate::interner::NameId;
use crate::model::{ComparisonOp, Constraint, SOSType, VariableType};
use crate::problem::LpProblem;

/// Configuration for analysis behavior and thresholds.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone)]
pub struct AnalysisConfig {
    /// Coefficient magnitude threshold for "large" warnings (default: 1e9)
    pub large_coefficient_threshold: f64,
    /// Small coefficient threshold for warnings (default: 1e-9)
    pub small_coefficient_threshold: f64,
    /// RHS magnitude threshold for warnings (default: 1e9)
    pub large_rhs_threshold: f64,
    /// Coefficient ratio threshold for scaling warnings (default: 1e6)
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
    /// Optimization sense (Minimize/Maximize)
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
    /// Variables with no explicit bounds (truly free)
    pub free_variables: Vec<String>,
    /// Variables where lower bound equals upper bound
    pub fixed_variables: Vec<FixedVariable>,
    /// Variables with inconsistent bounds (lower > upper)
    pub invalid_bounds: Vec<InvalidBound>,
    /// Variables not appearing in any constraint or objective
    pub unused_variables: Vec<String>,
    /// Count of discrete (binary + integer) variables
    pub discrete_variable_count: usize,
}

/// Distribution of variable types.
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[derive(Debug, Clone, Default)]
pub struct VariableTypeDistribution {
    /// Free (unbounded) variables
    pub free: usize,
    /// General (non-negative) variables
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
    /// Constraint coefficient range statistics
    pub constraint_coeff_range: RangeStats,
    /// Objective coefficient range statistics
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
        if self.count == 0 { Self::default() } else { self }
    }

    /// Create range stats from a collection of values.
    #[cfg(test)]
    fn from_values(values: &[f64]) -> Self {
        if values.is_empty() {
            return Self::default();
        }
        let min = values.iter().copied().fold(f64::INFINITY, f64::min);
        let max = values.iter().copied().fold(f64::NEG_INFINITY, f64::max);

        Self { min, max, count: values.len() }
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

/// Severity level for detected issues.
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IssueSeverity {
    /// Problem is likely unsolvable or invalid
    Error,
    /// May cause numerical issues or unexpected behavior
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
            vt.general + vt.free + vt.lower_bounded + vt.upper_bounded + vt.double_bounded,
            vt.binary,
            vt.integer
        )?;
        if vt.semi_continuous > 0 {
            writeln!(f, "  Semi-continuous: {}", vt.semi_continuous)?;
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

/// Collects coefficient statistics, classifying each as normal, large, or small.
struct CoeffCollector<'a> {
    range: &'a mut RangeStats,
    large: &'a mut Vec<CoefficientLocation>,
    small: &'a mut Vec<CoefficientLocation>,
}

impl<'a> CoeffCollector<'a> {
    /// Process a slice of coefficients for a given location.
    fn collect(
        &mut self,
        coefficients: &[crate::model::Coefficient],
        location_name: &str,
        is_objective: bool,
        config: &AnalysisConfig,
        interner: &crate::interner::NameInterner,
    ) {
        for coeff in coefficients {
            let abs_value = coeff.value.abs();
            self.range.update(abs_value);

            if abs_value > config.large_coefficient_threshold {
                self.large.push(CoefficientLocation {
                    location: location_name.to_string(),
                    is_objective,
                    variable: interner.resolve(coeff.name).to_string(),
                    value: coeff.value,
                });
            } else if abs_value > 0.0 && abs_value < config.small_coefficient_threshold {
                self.small.push(CoefficientLocation {
                    location: location_name.to_string(),
                    is_objective,
                    variable: interner.resolve(coeff.name).to_string(),
                    value: coeff.value,
                });
            }
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
            // range.min could be 0.0 (abs of a zero coeff); skip zeros for ratio.
            if range.min > 0.0 && range.min < global_min {
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
        let issues = self.detect_issues(&summary, &variables, &constraints, &coefficients, config);

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
        }
    }

    /// Count total non-zero coefficients in constraints.
    fn count_nonzeros(&self) -> usize {
        self.constraints
            .values()
            .map(|c| match c {
                Constraint::Standard { coefficients, .. } => coefficients.len(),
                Constraint::SOS { weights, .. } => weights.len(),
            })
            .sum()
    }

    /// Compute sparsity metrics.
    fn compute_sparsity_metrics(&self) -> SparsityMetrics {
        let (min_v, max_v) = self.constraints.values().fold((usize::MAX, 0usize), |(min_v, max_v), c| {
            let n = match c {
                Constraint::Standard { coefficients, .. } => coefficients.len(),
                Constraint::SOS { weights, .. } => weights.len(),
            };
            (min_v.min(n), max_v.max(n))
        });
        SparsityMetrics { min_vars_per_constraint: if min_v == usize::MAX { 0 } else { min_v }, max_vars_per_constraint: max_v }
    }

    /// Analyze variable types, bounds, and usage.
    fn analyze_variables(&self) -> VariableAnalysis {
        let mut type_distribution = VariableTypeDistribution::default();
        let mut free_variables = Vec::new();
        let mut fixed_variables = Vec::new();
        let mut invalid_bounds = Vec::new();

        for (name_id, variable) in &self.variables {
            let name_str = self.interner.resolve(*name_id);
            match &variable.var_type {
                VariableType::Free => {
                    type_distribution.free += 1;
                    free_variables.push(name_str.to_string());
                }
                VariableType::General => type_distribution.general += 1,
                VariableType::LowerBound(_) => type_distribution.lower_bounded += 1,
                VariableType::UpperBound(_) => type_distribution.upper_bounded += 1,
                VariableType::DoubleBound(lower, upper) => {
                    type_distribution.double_bounded += 1;
                    if (lower - upper).abs() < f64::EPSILON {
                        fixed_variables.push(FixedVariable { name: name_str.to_string(), value: *lower });
                    } else if lower > upper {
                        invalid_bounds.push(InvalidBound { name: name_str.to_string(), lower: *lower, upper: *upper });
                    }
                }
                VariableType::Binary => type_distribution.binary += 1,
                VariableType::Integer => type_distribution.integer += 1,
                VariableType::SemiContinuous => type_distribution.semi_continuous += 1,
                VariableType::SOS => type_distribution.sos += 1,
            }
        }

        let unused_variables = self.find_unused_variables();
        let discrete_variable_count = type_distribution.binary + type_distribution.integer;

        debug_assert_eq!(
            type_distribution.free
                + type_distribution.general
                + type_distribution.lower_bounded
                + type_distribution.upper_bounded
                + type_distribution.double_bounded
                + type_distribution.binary
                + type_distribution.integer
                + type_distribution.semi_continuous
                + type_distribution.sos,
            self.variables.len(),
            "postcondition: type distribution must sum to total variable count"
        );

        VariableAnalysis { type_distribution, free_variables, fixed_variables, invalid_bounds, unused_variables, discrete_variable_count }
    }

    /// Find variables declared but not referenced in any objective or constraint.
    fn find_unused_variables(&self) -> Vec<String> {
        let mut used_variables: HashSet<NameId> = HashSet::new();

        for objective in self.objectives.values() {
            for coeff in &objective.coefficients {
                used_variables.insert(coeff.name);
            }
        }

        for constraint in self.constraints.values() {
            match constraint {
                Constraint::Standard { coefficients, .. } => {
                    for coeff in coefficients {
                        used_variables.insert(coeff.name);
                    }
                }
                Constraint::SOS { weights, .. } => {
                    for weight in weights {
                        used_variables.insert(weight.name);
                    }
                }
            }
        }

        self.variables
            .keys()
            .filter(|name_id| !used_variables.contains(name_id))
            .map(|id| self.interner.resolve(*id).to_string())
            .collect()
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

        ConstraintAnalysis { type_distribution, empty_constraints, singleton_constraints, rhs_range: rhs_range.finalise(), sos_summary }
    }

    /// Analyze coefficients.
    fn analyze_coefficients(&self, config: &AnalysisConfig) -> CoefficientAnalysis {
        let mut constraint_range = RangeStats::new();
        let mut objective_range = RangeStats::new();
        let mut large_coefficients = Vec::new();
        let mut small_coefficients = Vec::new();

        let mut constraint_collector =
            CoeffCollector { range: &mut constraint_range, large: &mut large_coefficients, small: &mut small_coefficients };

        for (name_id, constraint) in &self.constraints {
            if let Constraint::Standard { coefficients, .. } = constraint {
                let name_str = self.interner.resolve(*name_id);
                constraint_collector.collect(coefficients, name_str, false, config, &self.interner);
            }
        }

        // Reborrow for objectives with a separate range tracker.
        let mut objective_collector =
            CoeffCollector { range: &mut objective_range, large: constraint_collector.large, small: constraint_collector.small };

        for (name_id, objective) in &self.objectives {
            let name_str = self.interner.resolve(*name_id);
            objective_collector.collect(&objective.coefficients, name_str, true, config, &self.interner);
        }

        let constraint_coeff_range = constraint_range.finalise();
        let objective_coeff_range = objective_range.finalise();
        let coefficient_ratio = compute_coefficient_ratio(&constraint_coeff_range, &objective_coeff_range);

        CoefficientAnalysis { constraint_coeff_range, objective_coeff_range, large_coefficients, small_coefficients, coefficient_ratio }
    }

    /// Detect issues and generate warnings.
    #[allow(clippy::unused_self)]
    fn detect_issues(
        &self,
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
            });
        }

        // Empty constraints (WARNING)
        for name in &constraints.empty_constraints {
            issues.push(AnalysisIssue {
                severity: IssueSeverity::Warning,
                category: IssueCategory::EmptyConstraint,
                message: format!("Constraint '{name}' has no variables"),
                details: None,
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
            });
        }

        // Large RHS warning
        if constraints.rhs_range.count > 0 && constraints.rhs_range.max > config.large_rhs_threshold {
            issues.push(AnalysisIssue {
                severity: IssueSeverity::Warning,
                category: IssueCategory::NumericalScaling,
                message: format!("Large RHS value ({:.2e}) may cause numerical issues", constraints.rhs_range.max),
                details: None,
            });
        }

        // Large coefficient ratio (WARNING)
        if coefficients.coefficient_ratio > config.coefficient_ratio_threshold {
            issues.push(AnalysisIssue {
                severity: IssueSeverity::Warning,
                category: IssueCategory::NumericalScaling,
                message: format!("Large coefficient ratio ({:.2e}) may cause numerical instability", coefficients.coefficient_ratio),
                details: Some("Consider rescaling the problem".to_string()),
            });
        }

        // Large coefficients
        for loc in &coefficients.large_coefficients {
            issues.push(AnalysisIssue {
                severity: IssueSeverity::Warning,
                category: IssueCategory::NumericalScaling,
                message: format!(
                    "Large coefficient ({:.2e}) for variable '{}' in {}",
                    loc.value,
                    loc.variable,
                    if loc.is_objective { "objective" } else { "constraint" }
                ),
                details: Some(loc.location.clone()),
            });
        }

        // Fixed variables (INFO)
        for fixed in &variables.fixed_variables {
            issues.push(AnalysisIssue {
                severity: IssueSeverity::Info,
                category: IssueCategory::FixedVariable,
                message: format!("Variable '{}' is fixed at value {}", fixed.name, fixed.value),
                details: None,
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
            });
        }

        // Unused variables (INFO)
        for name in &variables.unused_variables {
            issues.push(AnalysisIssue {
                severity: IssueSeverity::Info,
                category: IssueCategory::UnusedVariable,
                message: format!("Variable '{name}' is not used in any constraint or objective"),
                details: None,
            });
        }

        issues
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
