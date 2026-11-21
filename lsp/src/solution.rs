//! The last solve result per document, kept for `lp/explain` (and, later, hover
//! and inlay hints), plus the `lp.solution.*` settings.

use std::fmt::Write as _;

use lp_highs::protocol::{ConstraintSolution, Infeasibility, SolveOutcome, VariableSolution};
use rustc_hash::FxHashMap;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::requests::finite;

/// `lp.solution`: how stored solutions are shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct SolutionSettings {
    /// Show solution values as inlay hints.
    pub inlay_hints: bool,
    /// Keep showing a solution after the document was edited.
    pub show_stale: bool,
}

impl Default for SolutionSettings {
    fn default() -> Self {
        Self { inlay_hints: true, show_stale: false }
    }
}

/// A solve result and the document version it describes.
#[derive(Debug, Clone)]
pub struct Solution {
    /// Version of the document that was solved; `None` for an unopened file.
    pub version: Option<i32>,
    /// `HiGHS` model status.
    pub status: String,
    /// Objective value, when the status carries one.
    pub objective_value: Option<f64>,
    /// Infeasibility diagnosis, if one was produced.
    pub infeasibility: Option<Infeasibility>,
    variables: FxHashMap<String, VariableSolution>,
    constraints: FxHashMap<String, ConstraintSolution>,
}

impl Solution {
    /// Index `outcome`, solved at `version`.
    #[must_use]
    pub fn new(version: Option<i32>, outcome: &SolveOutcome) -> Self {
        Self {
            version,
            status: outcome.status.clone(),
            objective_value: outcome.objective_value,
            infeasibility: outcome.infeasibility.clone(),
            variables: outcome.variables.iter().map(|v| (v.name.clone(), v.clone())).collect(),
            constraints: outcome.constraints.iter().map(|c| (c.name.clone(), c.clone())).collect(),
        }
    }

    /// Whether the document changed since the solve (`current` is its version now).
    #[must_use]
    pub fn is_stale(&self, current: Option<i32>) -> bool {
        self.version != current
    }

    /// The solution of variable `name`.
    #[must_use]
    pub fn variable(&self, name: &str) -> Option<&VariableSolution> {
        self.variables.get(name)
    }

    /// The solution of constraint `name`.
    #[must_use]
    pub fn constraint(&self, name: &str) -> Option<&ConstraintSolution> {
        self.constraints.get(name)
    }

    /// Whether the values are shown for a buffer at `current`: while the
    /// document is unchanged, or always with `lp.solution.showStale`.
    #[must_use]
    pub fn is_visible(&self, current: Option<i32>, settings: &SolutionSettings) -> bool {
        settings.show_stale || !self.is_stale(current)
    }

    /// Markdown for variable `name`, shared by hover and `lp/explain`.
    #[must_use]
    pub fn variable_markdown(&self, name: &str, current: Option<i32>) -> Option<String> {
        let v = self.variable(name)?;
        let mut out = format!("- **Solution:** {}", number(v.value));
        if let Some(cost) = v.reduced_cost.and_then(finite) {
            write!(out, ", reduced cost {}", number(cost)).expect("writing to a String cannot fail");
        }
        self.mark_stale(&mut out, current);
        Some(out)
    }

    /// Markdown for constraint `name`, shared by hover and `lp/explain`.
    #[must_use]
    pub fn constraint_markdown(&self, name: &str, current: Option<i32>) -> Option<String> {
        let c = self.constraint(name)?;
        let mut out = format!("- **Solution:** activity {}, slack {}", number(c.activity), number(c.slack));
        if let Some(dual) = c.dual.and_then(finite) {
            write!(out, ", dual {}", number(dual)).expect("writing to a String cannot fail");
        }
        if c.binding {
            out.push_str(", binding");
        }
        self.mark_stale(&mut out, current);
        Some(out)
    }

    /// Markdown for the objective, shared by hover and `lp/explain`.
    #[must_use]
    pub fn objective_markdown(&self, current: Option<i32>) -> Option<String> {
        let value = self.objective_value.and_then(finite)?;
        let mut out = format!("- **Solution:** objective value {}", number(value));
        self.mark_stale(&mut out, current);
        Some(out)
    }

    fn mark_stale(&self, out: &mut String, current: Option<i32>) {
        if self.is_stale(current) {
            out.push_str(" (stale: the document changed since the solve)");
        }
    }

    /// `solution` of an `lp/explain` variable result.
    #[must_use]
    pub fn variable_json(&self, name: &str, current: Option<i32>) -> Option<Value> {
        let v = self.variable(name)?;
        let mut out = json!({ "stale": self.is_stale(current), "value": finite(v.value) });
        if let Some(cost) = v.reduced_cost.and_then(finite) {
            out["reducedCost"] = json!(cost);
        }
        Some(out)
    }

    /// `solution` of an `lp/explain` constraint result.
    #[must_use]
    pub fn constraint_json(&self, name: &str, current: Option<i32>) -> Option<Value> {
        let c = self.constraint(name)?;
        let mut out = json!({
            "stale": self.is_stale(current),
            "activity": finite(c.activity),
            "slack": finite(c.slack),
            "binding": c.binding,
        });
        if let Some(dual) = c.dual.and_then(finite) {
            out["dual"] = json!(dual);
        }
        Some(out)
    }

    /// `solution` of an `lp/explain` objective result.
    #[must_use]
    pub fn objective_json(&self, current: Option<i32>) -> Option<Value> {
        let value = self.objective_value.and_then(finite)?;
        Some(json!({ "stale": self.is_stale(current), "value": value }))
    }
}

/// A solution number: shortest exact form, `-0` as `0`.
fn number(x: f64) -> String {
    if x == 0.0 { "0".to_owned() } else { format!("{x}") }
}

#[cfg(test)]
mod tests {
    use lp_highs::protocol::{Skipped, SolverId, Timings};

    use super::*;

    fn outcome() -> SolveOutcome {
        SolveOutcome {
            solver: SolverId { name: "HiGHS".into(), version: "1".into(), helper: "lp-highs 0.1.0".into() },
            status: "Optimal".into(),
            objective_value: Some(12.5),
            mip_gap: None,
            timings: Timings::default(),
            variables: vec![VariableSolution { name: "x".into(), value: 2.0, reduced_cost: Some(0.5) }],
            constraints: vec![ConstraintSolution { name: "c1".into(), activity: 4.0, dual: None, slack: 0.0, binding: true }],
            skipped: Skipped::default(),
            infeasibility: None,
            log: None,
        }
    }

    #[test]
    fn staleness_tracks_the_document_version() {
        let s = Solution::new(Some(3), &outcome());
        assert!(!s.is_stale(Some(3)));
        assert!(s.is_stale(Some(4)));
        assert!(Solution::new(None, &outcome()).is_stale(Some(1)), "an unopened file's solution is stale once it is opened");
    }

    #[test]
    fn explain_fragments_omit_absent_fields() {
        let s = Solution::new(Some(1), &outcome());
        assert_eq!(s.variable_json("x", Some(1)), Some(json!({ "stale": false, "value": 2.0, "reducedCost": 0.5 })));
        assert_eq!(s.constraint_json("c1", Some(2)), Some(json!({ "stale": true, "activity": 4.0, "slack": 0.0, "binding": true })));
        assert_eq!(s.objective_json(Some(1)), Some(json!({ "stale": false, "value": 12.5 })));
        assert_eq!(s.variable_json("nope", Some(1)), None);
    }

    #[test]
    fn settings_default_to_fresh_solutions_with_hints() {
        assert_eq!(SolutionSettings::default(), SolutionSettings { inlay_hints: true, show_stale: false });
        let parsed: SolutionSettings = serde_json::from_value(json!({ "showStale": true })).expect("parses");
        assert_eq!(parsed, SolutionSettings { inlay_hints: true, show_stale: true });
    }
}
