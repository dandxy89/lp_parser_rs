//! Single-file inspect model.
//!
//! Inspect mode (`lp_diff model.lp`, one positional file) reuses the diff
//! report machinery so navigation, search, and the detail panel work unchanged:
//! the model's section lists are built by diffing the parsed problem against an
//! empty problem, which yields exactly one `Added` entry per variable,
//! constraint, and objective. The presentation layer keys off
//! [`AppMode::Inspect`](crate::state::AppMode) to drop the diff badges/colours so
//! the result reads as a plain single-model explorer, not a diff.

use std::collections::HashMap;

use lp_parser_rs::analysis::ProblemAnalysis;
use lp_parser_rs::interner::NameId;
use lp_parser_rs::problem::LpProblem;

use crate::diff_model::{DiffInput, DiffOptions, LpDiffReport, build_diff_report};

/// Build the inspect report for a single parsed problem.
///
/// The problem is diffed against an empty problem so every entry surfaces as
/// `Added`; the diff kinds are ignored by the inspect presentation, which lists
/// entries plainly. Both stored analyses are set to the single file's analysis
/// so the Summary/Numerics inspect views render the file's own conditioning.
///
/// `line_map` is the constraint→line-number map for the file, threaded through
/// so the detail panel can show source locations.
#[must_use]
pub fn build_inspect_report(file: &str, problem: &LpProblem, line_map: &HashMap<NameId, usize>, analysis: ProblemAnalysis) -> LpDiffReport {
    debug_assert!(!file.is_empty(), "inspect file label must not be empty");

    let empty = LpProblem::default();
    let empty_line_map: HashMap<NameId, usize> = HashMap::new();

    build_diff_report(&DiffInput {
        // Both labels are the single file: nothing in the report ever reads as
        // "vs file2", and any shared rendering falls back to the real filename.
        file1: file,
        file2: file,
        // Empty base vs the real problem → every entry is Added.
        p1: &empty,
        p2: problem,
        line_map1: &empty_line_map,
        line_map2: line_map,
        analysis1: analysis.clone(),
        analysis2: analysis,
        // Inspect never applies tolerances or rename rules — it is not a comparison.
        options: DiffOptions::default(),
    })
}

#[cfg(test)]
mod tests {
    use lp_parser_rs::model::VariableType;
    use lp_parser_rs::problem::LpProblem;

    use super::*;
    use crate::diff_model::{ConstraintDiffDetail, DiffKind, ResolvedConstraint};

    fn analyse(problem: &LpProblem) -> ProblemAnalysis {
        problem.analyze()
    }

    #[test]
    fn test_inspect_lists_all_variables_as_added() {
        let problem =
            LpProblem::parse("minimize\nobj: 2 x + 3 y\nsubject to\n c1: x + y <= 10\nbinary\n x\nend").expect("problem should parse");
        let report = build_inspect_report("model.lp", &problem, &HashMap::new(), analyse(&problem));

        // Every variable in the file appears exactly once, all as Added, no removals.
        assert_eq!(report.variables.counts.removed, 0);
        assert_eq!(report.variables.counts.modified, 0);
        assert!(report.variables.entries.iter().all(|e| e.kind == DiffKind::Added));
        assert!(report.variables.entries.iter().any(|e| e.name == "x"));
        assert!(report.variables.entries.iter().any(|e| e.name == "y"));

        let x = report.variables.entries.iter().find(|e| e.name == "x").expect("x present");
        assert_eq!(x.new_type, Some(VariableType::Binary));
        assert!(x.old_type.is_none(), "inspect entries never carry an old side");
    }

    #[test]
    fn test_inspect_lists_constraints_and_objectives() {
        let problem =
            LpProblem::parse("minimize\nobj: x + y\nsubject to\n c1: x + 2 y <= 5\n c2: x >= 1\nend").expect("problem should parse");
        let report = build_inspect_report("model.lp", &problem, &HashMap::new(), analyse(&problem));

        let names: Vec<&str> = report.constraints.entries.iter().map(|e| e.name.as_str()).collect();
        assert!(names.contains(&"c1"), "c1 listed");
        assert!(names.contains(&"c2"), "c2 listed");
        assert!(report.constraints.entries.iter().all(|e| e.kind == DiffKind::Added));

        // A constraint's detail carries the full single-side data (operator/RHS/coeffs).
        let c1 = report.constraints.entries.iter().find(|e| e.name == "c1").expect("c1 present");
        let ConstraintDiffDetail::AddedOrRemoved(ResolvedConstraint::Standard { coefficients, rhs, .. }) = &c1.detail else {
            panic!("expected an added standard constraint");
        };
        assert!((*rhs - 5.0).abs() < 1e-9, "rhs should be 5.0");
        assert_eq!(coefficients.len(), 2, "x and y coefficients present");

        assert!(report.objectives.entries.iter().any(|e| e.name == "obj"));
        assert!(report.objectives.entries.iter().all(|e| e.kind == DiffKind::Added));
    }

    #[test]
    fn test_inspect_problem_without_constraints_lists_none() {
        // A model with an objective and one variable but no constraints.
        let problem = LpProblem::parse("minimize\nobj: x\nsubject to\nend").expect("problem should parse");
        let report = build_inspect_report("model.lp", &problem, &HashMap::new(), analyse(&problem));
        assert!(report.constraints.entries.is_empty(), "no constraints in the model");
        assert!(report.variables.entries.iter().any(|e| e.name == "x"), "x is listed");
        assert!(report.objectives.entries.iter().any(|e| e.name == "obj"), "obj is listed");
    }
}
