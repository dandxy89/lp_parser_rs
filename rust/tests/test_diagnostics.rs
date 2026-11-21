//! Scenario tests: one positive and one negative model per diagnostic code.

use std::fmt::Write as _;

use lp_parser_rs::analysis::{Certainty, DiagnosticSubject, IssueSeverity, SubjectSite};
use lp_parser_rs::{DiagnosticCode, DiagnosticsConfig, EntityKind, LpProblem, ModelDiagnostics};

fn run(text: &str) -> ModelDiagnostics {
    LpProblem::parse(text).unwrap_or_else(|e| panic!("parse failed: {e}\n{text}")).diagnostics()
}

fn has(text: &str, code: DiagnosticCode) -> bool {
    run(text).with_code(code).next().is_some()
}

fn subjects(text: &str, code: DiagnosticCode) -> Vec<String> {
    run(text).with_code(code).filter_map(|d| d.subject.as_ref().map(|s| s.name.clone())).collect()
}

/// A minimal feasible model that should raise nothing but the odd info.
const CLEAN: &str = "Minimize\n obj: x + y\nSubject To\n c1: x + y >= 2\n c2: x - y <= 1\nBounds\n x <= 10\n y <= 10\nEnd";

#[test]
fn clean_model_has_no_errors_or_warnings() {
    let result = run(CLEAN);
    assert_eq!(result.count(IssueSeverity::Error), 0, "{result:?}");
    assert_eq!(result.count(IssueSeverity::Warning), 0, "{result:?}");
}

#[test]
fn invalid_bounds() {
    let text = "Minimize\n obj: x\nSubject To\n c1: x >= 0\nBounds\n x >= 5\n x <= 3\nEnd";
    let result = run(text);
    let found: Vec<_> = result.with_code(DiagnosticCode::InvalidBounds).collect();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].severity, IssueSeverity::Error);
    assert_eq!(found[0].certainty, Certainty::Proven);
    assert_eq!(found[0].subject, Some(DiagnosticSubject::variable("x", SubjectSite::Bound)));
    assert!(!has(CLEAN, DiagnosticCode::InvalidBounds));
}

#[test]
fn negative_upper_default_lower() {
    let bad = "Minimize\n obj: x\nSubject To\n c1: x + y >= -9\nBounds\n x <= -5\nEnd";
    assert!(has(bad, DiagnosticCode::NegativeUpperDefaultLower));
    let fine = "Minimize\n obj: x\nSubject To\n c1: x + y >= -9\nBounds\n -inf <= x <= -5\nEnd";
    assert!(!has(fine, DiagnosticCode::NegativeUpperDefaultLower));
}

#[test]
fn integer_bound_checks() {
    let empty = "Minimize\n obj: x\nSubject To\n c1: x + y >= 0\nBounds\n 0.2 <= x <= 0.8\nGenerals\n x\nEnd";
    assert!(has(empty, DiagnosticCode::IntegerNoFeasibleValues));
    assert!(!has(empty, DiagnosticCode::IntegerFractionalBound), "suppressed when no integer fits");
    let fractional = "Minimize\n obj: x\nSubject To\n c1: x + y >= 0\nBounds\n 0.5 <= x <= 4.5\nGenerals\n x\nEnd";
    let result = run(fractional);
    assert!(!has(fractional, DiagnosticCode::IntegerNoFeasibleValues));
    let message = &result.with_code(DiagnosticCode::IntegerFractionalBound).next().unwrap().message;
    assert!(message.contains("acts as 1") && message.contains("acts as 4"), "{message}");
    let ok = "Minimize\n obj: x\nSubject To\n c1: x + y >= 0\nBounds\n 1 <= x <= 4\nGenerals\n x\nEnd";
    assert!(!has(ok, DiagnosticCode::IntegerFractionalBound));
}

#[test]
fn semi_integer_only_zero() {
    let text = "Minimize\n obj: x\nSubject To\n c1: x + y >= 0\nBounds\n 0.2 <= x <= 0.8\nGenerals\n x\nSemi-Continuous\n x\nEnd";
    assert!(has(text, DiagnosticCode::SemiIntegerOnlyZero), "{:?}", run(text));
}

#[test]
fn binary_bound_checks() {
    let text = "Minimize\n obj: x\nSubject To\n c1: x + y >= 0\nBounds\n x <= 5\nBinary\n x\nEnd";
    assert!(has(text, DiagnosticCode::BinaryBoundOutsideUnit));
    let redundant = "Minimize\n obj: x\nSubject To\n c1: x + y >= 0\nBounds\n x <= 1\nBinary\n x\nEnd";
    assert!(has(redundant, DiagnosticCode::BinaryRedundantBounds));
    assert!(!has(redundant, DiagnosticCode::BinaryBoundOutsideUnit));
    let plain = "Minimize\n obj: x\nSubject To\n c1: x + y >= 0\nBinary\n x\nEnd";
    assert!(!has(plain, DiagnosticCode::BinaryRedundantBounds));
}

#[test]
fn integer_is_binary() {
    let text = "Minimize\n obj: x\nSubject To\n c1: x + y >= 0\nBounds\n 0 <= x <= 1\nGenerals\n x\nEnd";
    assert!(has(text, DiagnosticCode::IntegerIsBinary));
    assert!(!has(CLEAN, DiagnosticCode::IntegerIsBinary));
}

#[test]
fn semicontinuous_infinite_upper() {
    let text = "Minimize\n obj: x\nSubject To\n c1: x + y >= 0\nBounds\n x >= 2\nSemi-Continuous\n x\nEnd";
    assert!(has(text, DiagnosticCode::SemicontinuousInfiniteUpper));
    let bounded = "Minimize\n obj: x\nSubject To\n c1: x + y >= 0\nBounds\n 2 <= x <= 9\nSemi-Continuous\n x\nEnd";
    assert!(!has(bounded, DiagnosticCode::SemicontinuousInfiniteUpper));
}

#[test]
fn fixed_and_unused_variables() {
    let text = "Minimize\n obj: x\nSubject To\n c1: x + y >= 0\nBounds\n y = 3\n z free\nEnd";
    assert_eq!(subjects(text, DiagnosticCode::FixedVariable), vec!["y"]);
    assert_eq!(subjects(text, DiagnosticCode::UnusedVariable), vec!["z"]);
}

#[test]
fn objective_only_and_unbounded() {
    let only = "Minimize\n obj: x + w\nSubject To\n c1: x >= 1\nBounds\n w <= 4\nEnd";
    // w minimised with default lower 0: sits at 0.
    assert_eq!(subjects(only, DiagnosticCode::ObjectiveOnlyVariable), vec!["w"]);
    let unbounded = "Maximize\n obj: x + w\nSubject To\n c1: x <= 1\nEnd";
    assert_eq!(subjects(unbounded, DiagnosticCode::NoEffectiveBound), vec!["w"]);
    assert!(!has(unbounded, DiagnosticCode::ObjectiveOnlyVariable));
    let result = run(unbounded);
    let msg = &result.with_code(DiagnosticCode::NoEffectiveBound).next().unwrap().message;
    assert!(msg.contains("if the model is feasible"), "{msg}");
    // A row that the ray would tighten blocks it.
    let tight = "Maximize\n obj: x\nSubject To\n c1: x + y <= 4\nEnd";
    assert!(!has(tight, DiagnosticCode::NoEffectiveBound));
    // Equality blocks it.
    let equality = "Maximize\n obj: x\nSubject To\n c1: x - y = 0\nEnd";
    assert!(!has(equality, DiagnosticCode::NoEffectiveBound));
    // Minimising a non-negative variable is bounded below.
    let bounded = "Minimize\n obj: x\nSubject To\n c1: x >= 1\nEnd";
    assert!(!has(bounded, DiagnosticCode::NoEffectiveBound));
    // Multi-objective files are skipped.
    let multi = "Maximize multi-objectives\n o1: x\n o2: y\nSubject To\n c1: x - y <= 1\nEnd";
    assert_eq!(run(multi).with_code(DiagnosticCode::NoEffectiveBound).count(), 0);
    assert_eq!(LpProblem::parse(multi).unwrap().objective_count(), 2);
}

#[test]
fn nonconvex_objective() {
    let text = "Minimize\n obj: x + [ - x ^ 2 ] / 2\nSubject To\n c1: x <= 4\nEnd";
    assert!(has(text, DiagnosticCode::NonconvexObjective), "{:?}", run(text));
    let convex = "Minimize\n obj: x + [ x ^ 2 ] / 2\nSubject To\n c1: x <= 4\nEnd";
    assert!(!has(convex, DiagnosticCode::NonconvexObjective));
    let maximise = "Maximize\n obj: x + [ x ^ 2 ] / 2\nSubject To\n c1: x <= 4\nEnd";
    assert!(has(maximise, DiagnosticCode::NonconvexObjective));
}

#[test]
fn objective_relationships() {
    let text = "Minimize multi-objectives\n a: x + y\n b: 2 x + 2 y\n c: - x - y\n d:\nSubject To\n c1: x + y >= 1\nEnd";
    let result = run(text);
    assert!(result.with_code(DiagnosticCode::DuplicateObjective).any(|d| d.subject.as_ref().unwrap().name == "b"));
    assert!(result.with_code(DiagnosticCode::OpposedObjectives).any(|d| d.subject.as_ref().unwrap().name == "c"));
}

#[test]
fn empty_rows() {
    let infeasible = "Minimize\n obj: x\nSubject To\n c1: 0 x >= 1\n c2: x >= 0\nEnd";
    assert_eq!(subjects(infeasible, DiagnosticCode::EmptyRowInfeasible), vec!["c1"]);
    assert!(!has(infeasible, DiagnosticCode::EmptyRow));
    let holds = "Minimize\n obj: x\nSubject To\n c1: 0 x <= 1\n c2: x >= 0\nEnd";
    assert_eq!(subjects(holds, DiagnosticCode::EmptyRow), vec!["c1"]);
    assert_eq!(subjects(holds, DiagnosticCode::ExplicitZeroCoefficient), vec!["c1"]);
}

#[test]
fn repeated_variable_documents_parser_behaviour() {
    use lp_parser_rs::model::{Coefficient, ComparisonOp, Constraint, Objective, ObjectiveAttributes};
    let mut problem = LpProblem::new();
    let (obj, c1, x) = (problem.intern("obj"), problem.intern("c1"), problem.intern("x"));
    problem.add_objective(Objective {
        name: obj,
        coefficients: vec![Coefficient { name: x, value: 1.0 }],
        constant: 0.0,
        quadratic: Vec::new(),
        attributes: ObjectiveAttributes::default(),
        byte_offset: None,
    });
    problem.add_constraint(Constraint::Standard {
        name: c1,
        coefficients: vec![Coefficient { name: x, value: 1.0 }, Coefficient { name: x, value: 2.0 }],
        operator: ComparisonOp::GTE,
        rhs: 3.0,
        byte_offset: None,
    });
    assert_eq!(problem.diagnostics().with_code(DiagnosticCode::RepeatedVariableInRow).count(), 1);
}

#[test]
fn singleton_and_bounds_based_rows() {
    let single = "Minimize\n obj: x + y\nSubject To\n c1: 2 x <= 10\n c2: x + y >= 1\nEnd";
    let result = run(single);
    let found = result.with_code(DiagnosticCode::SingletonRow).next().unwrap();
    assert!(found.message.contains("x <= 5"), "{}", found.message);
    let negative = "Minimize\n obj: x + y\nSubject To\n c1: -2 x <= -10\n c2: x + y >= 1\nEnd";
    let result = run(negative);
    assert!(result.with_code(DiagnosticCode::SingletonRow).next().unwrap().message.contains("x >= 5"));

    let infeasible = "Minimize\n obj: x\nSubject To\n c1: x >= 100\nBounds\n x <= 50\nEnd";
    assert_eq!(subjects(infeasible, DiagnosticCode::RowInfeasibleByBounds), vec!["c1"]);
    let redundant = "Minimize\n obj: x\nSubject To\n c1: x >= 0\n c2: x + y >= 1\nEnd";
    assert_eq!(subjects(redundant, DiagnosticCode::RowRedundantByBounds), vec!["c1"]);
    assert!(!has(redundant, DiagnosticCode::SingletonRow), "redundant singleton is reported once");
}

#[test]
fn row_relationships() {
    let text = "Minimize\n obj: x + y\nSubject To\n c1: x + y >= 2\n c2: x + y >= 2\n c3: 2 x + 2 y >= 4\n c4: x + y >= 1\n c5: x + y <= 9\n c6: x + y >= 11\nEnd";
    let result = run(text);
    assert_eq!(subjects(text, DiagnosticCode::DuplicateRow), vec!["c2"]);
    assert_eq!(subjects(text, DiagnosticCode::ProportionalRow), vec!["c3"]);
    let proportional = result.with_code(DiagnosticCode::ProportionalRow).next().unwrap();
    assert!(proportional.message.contains("scaled by 2"), "{}", proportional.message);
    assert_eq!(proportional.related[0].subject.name, "c1");
    assert!(subjects(text, DiagnosticCode::DominatedRow).contains(&"c4".to_owned()));
    assert!(subjects(text, DiagnosticCode::ContradictoryRows).contains(&"c6".to_owned()), "{:?}", result.diagnostics);
}

#[test]
fn rows_form_range_and_ranged_pairs_are_not_flagged() {
    let pair = "Minimize\n obj: x + y\nSubject To\n c1: x + y >= 2\n c2: x + y <= 9\nEnd";
    assert_eq!(subjects(pair, DiagnosticCode::RowsFormRange), vec!["c2"]);
    let ranged = "Minimize\n obj: x + y\nSubject To\n c1: 2 <= x + y <= 9\nEnd";
    let result = run(ranged);
    for code in [
        DiagnosticCode::RowsFormRange,
        DiagnosticCode::DuplicateRow,
        DiagnosticCode::ProportionalRow,
        DiagnosticCode::DominatedRow,
        DiagnosticCode::ContradictoryRows,
    ] {
        assert_eq!(result.with_code(code).count(), 0, "{code}");
    }
}

#[test]
fn lazy_rows_and_cuts_are_not_compared() {
    let text = "Minimize\n obj: x + y\nSubject To\n c1: x + y >= 2\nLazy Constraints\n l1: x + y >= 2\nUser Cuts\n u1: x + y >= 2\nEnd";
    let problem = LpProblem::parse(text).unwrap();
    assert_eq!(problem.constraint_count(), 3);
    assert_eq!(problem.diagnostics().with_code(DiagnosticCode::DuplicateRow).count(), 0);
}

#[test]
fn equalities_exceed_variables() {
    let text = "Minimize\n obj: x + y\nSubject To\n e1: x + y = 1\n e2: x - y = 0\n e3: x + 2 y = 5\nEnd";
    assert!(has(text, DiagnosticCode::EqualitiesExceedVariables));
    assert!(!has("Minimize\n obj: x + y\nSubject To\n e1: x + y = 1\n e2: x - y = 0\nEnd", DiagnosticCode::EqualitiesExceedVariables));
}

#[test]
fn numerical_codes() {
    let text =
        "Minimize\n obj: 1e12 x + y\nSubject To\n c1: 1e12 x + 0.000000000001 y >= 1\n c2: x + y >= 1e10\n c3: 1e-3 x + 1e5 y <= 1\nEnd";
    let result = run(text);
    for code in [
        DiagnosticCode::LargeCoefficient,
        DiagnosticCode::SmallCoefficient,
        DiagnosticCode::LargeRhs,
        DiagnosticCode::RowCoefficientRange,
        DiagnosticCode::ColumnCoefficientRange,
        DiagnosticCode::ObjectiveCoefficientRange,
        DiagnosticCode::CoefficientRange,
    ] {
        assert!(result.with_code(code).next().is_some(), "missing {code}: {result:?}");
    }
    assert!(result.with_code(DiagnosticCode::RowCoefficientRange).any(|d| d.message.contains("orders of magnitude")));
    assert!(has("Minimize\n obj: x\nSubject To\n c1: x + y >= 1e10\n c2: x <= 1\nEnd", DiagnosticCode::RhsCoefficientRatio));
    assert!(!has(CLEAN, DiagnosticCode::CoefficientRange));
}

#[test]
fn indicator_and_sos_codes() {
    let text = "Minimize\n obj: x + y\nSubject To\n c1: x + y >= 1\n ind: b = 1 -> x <= 3\nBounds\n b <= 5\nGenerals\n b\nEnd";
    let result = run(text);
    let found = result.with_code(DiagnosticCode::IndicatorNotBinary).next().expect("indicator-not-binary");
    assert_eq!(found.subject.as_ref().unwrap().site, SubjectSite::TypeDeclaration);
    let sos = "Minimize\n obj: x + y + z\nSubject To\n c1: x + y + z >= 1\nSOS\n s1: S1:: x:1 y:1 z:2\n s2: S1:: x:1\nEnd";
    let result = run(sos);
    assert!(result.with_code(DiagnosticCode::SosDuplicateWeights).next().is_some(), "{result:?}");
    assert_eq!(result.with_code(DiagnosticCode::SosTrivial).count(), 1);
}

#[test]
fn output_is_deterministic_and_sorted() {
    let text = "Minimize\n obj: 1e12 x + y\nSubject To\n c1: x >= 100\n c2: x + y >= 2\n c3: x + y >= 2\nBounds\n x <= 50\n z free\nEnd";
    let first = run(text);
    assert_eq!(first, run(text));
    let ranks: Vec<u8> = first
        .diagnostics
        .iter()
        .map(|d| match d.severity {
            IssueSeverity::Error => 0,
            IssueSeverity::Warning => 1,
            IssueSeverity::Info => 2,
        })
        .collect();
    assert!(ranks.windows(2).all(|w| w[0] <= w[1]));
    for d in &first.diagnostics {
        assert_eq!(d.severity, d.code.default_severity());
        assert_eq!(d.category, d.code.category());
        assert_eq!(d.certainty, d.code.certainty());
        if let Some(subject) = &d.subject {
            assert!(matches!(subject.kind, EntityKind::Variable | EntityKind::Constraint | EntityKind::Objective));
        }
    }
}

#[test]
fn config_changes_thresholds() {
    let problem = LpProblem::parse("Minimize\n obj: x\nSubject To\n c1: 5 x >= 1\nEnd").unwrap();
    let mut config = DiagnosticsConfig::default();
    config.thresholds.large_coefficient_threshold = 2.0;
    let result = problem.diagnostics_with_config(&config).unwrap();
    assert!(result.with_code(DiagnosticCode::LargeCoefficient).next().is_some());
    assert!(problem.diagnostics().with_code(DiagnosticCode::LargeCoefficient).next().is_none());
}

#[test]
fn analyze_is_unchanged_by_diagnostics() {
    let problem = LpProblem::parse(CLEAN).unwrap();
    let before = problem.analyze().to_string();
    let _ = problem.diagnostics();
    assert_eq!(before, problem.analyze().to_string());
}

/// Every shipped resource model runs through the driver, which checks its own
/// invariants (resolvable subjects, sorted output) with `debug_assert!`.
#[test]
fn every_resource_model_runs_cleanly() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("resources");
    let mut checked = 0;
    for entry in std::fs::read_dir(dir).expect("resources directory") {
        let path = entry.expect("directory entry").path();
        let Some(extension) = path.extension().and_then(|e| e.to_str()) else {
            continue;
        };
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let parsed = match extension {
            "lp" => LpProblem::parse(&text),
            "mps" => LpProblem::parse_mps(&text),
            _ => continue,
        };
        // Some resources are deliberately invalid input for the parser tests.
        let Ok(problem) = parsed else {
            continue;
        };
        let result = problem.diagnostics();
        assert_eq!(result, problem.diagnostics(), "{} is not deterministic", path.display());
        checked += 1;
    }
    assert!(checked > 20, "only {checked} resources were checked");
}

#[test]
fn integer_with_negative_upper_reports_only_the_heuristic() {
    let text = "Minimize\n obj: x\nSubject To\n c1: x + y >= -9\nBounds\n x <= -5\nGenerals\n x\nEnd";
    assert!(has(text, DiagnosticCode::NegativeUpperDefaultLower));
    assert!(!has(text, DiagnosticCode::IntegerNoFeasibleValues), "dialects disagree on the implied lower bound");
    // Near-miss: with a declared lower bound the proven finding still fires.
    let declared = "Minimize\n obj: x\nSubject To\n c1: x + y >= -9\nBounds\n 0.2 <= x <= 0.8\nGenerals\n x\nEnd";
    assert!(has(declared, DiagnosticCode::IntegerNoFeasibleValues));
}

#[test]
fn ranged_constraint_redundancy_is_judged_per_statement() {
    // The lower half (>= -2) always holds; the upper half (<= 8) still binds.
    let one_side = "Minimize\n obj: x + y\nSubject To\n c1: -2 <= x + y <= 8\nEnd";
    let result = run(one_side);
    let found: Vec<_> = result.with_code(DiagnosticCode::RowRedundantByBounds).collect();
    assert_eq!(found.len(), 1, "{result:?}");
    assert!(found[0].message.contains("lower side") && found[0].message.contains("'c1'"), "{}", found[0].message);
    assert!(!found[0].message.contains("Constraint 'c1' always holds"), "{}", found[0].message);
    assert_eq!(found[0].subject.as_ref().unwrap().name, "c1");

    // Both halves hold: one finding naming the statement.
    let both = "Minimize\n obj: x + y\nSubject To\n c1: -2 <= x + y <= 8\nBounds\n x <= 1\n y <= 1\nEnd";
    let result = run(both);
    let found: Vec<_> = result.with_code(DiagnosticCode::RowRedundantByBounds).collect();
    assert_eq!(found.len(), 1, "{result:?}");
    assert_eq!(found[0].message, "Constraint 'c1' always holds given the variable bounds");

    // Unnamed ranges never leak the generated half name.
    let unnamed = "Minimize\n obj: x + y\nSubject To\n -2 <= x + y <= 8\nEnd";
    for subject in subjects(unnamed, DiagnosticCode::RowRedundantByBounds) {
        assert!(!subject.contains("_rng"), "{subject}");
    }
    // A plain row is unaffected.
    assert_eq!(
        subjects("Minimize\n obj: x\nSubject To\n c1: x >= 0\n c2: x + y >= 1\nEnd", DiagnosticCode::RowRedundantByBounds),
        vec!["c1"]
    );
}

#[test]
fn ranged_rows_are_classified_against_the_tightest_row() {
    // (a) c2 is dominated by the upper half of c1; nothing forms a range.
    let text = "Minimize\n obj: x + y\nSubject To\n c1: 2 <= x + y <= 8\n c2: x + y <= 10\nEnd";
    let result = run(text);
    assert_eq!(result.with_code(DiagnosticCode::RowsFormRange).count(), 0, "{result:?}");
    assert_eq!(subjects(text, DiagnosticCode::DominatedRow), vec!["c2"]);

    // (b) two identical ranged constraints are duplicates, never a range.
    let text = "Minimize\n obj: x + y\nSubject To\n c1: 2 <= x + y <= 8\n c2: 2 <= x + y <= 8\nEnd";
    let result = run(text);
    assert_eq!(result.with_code(DiagnosticCode::RowsFormRange).count(), 0, "{result:?}");
    assert!(result.with_code(DiagnosticCode::DuplicateRow).count() >= 1, "{result:?}");

    // (c) c3 dominates c2 and a single range is formed from the tightest pair.
    let text = "Minimize\n obj: x + y\nSubject To\n c1: x + y >= 2\n c2: x + y <= 10\n c3: x + y <= 5\nEnd";
    let result = run(text);
    assert_eq!(subjects(text, DiagnosticCode::DominatedRow), vec!["c2"]);
    let range: Vec<_> = result.with_code(DiagnosticCode::RowsFormRange).collect();
    assert_eq!(range.len(), 1, "{result:?}");
    assert_eq!(range[0].subject.as_ref().unwrap().name, "c3");
    assert!(range[0].message.contains("between 2 and 5"), "{}", range[0].message);
}

#[test]
fn near_parallel_rows_are_not_contradictory() {
    // Feasible: y = -2e10, x = 1 + 2e10.
    let text = "Minimize\n obj: x + y\nSubject To\n c1: x + 1.0000000001 y <= 0\n c2: x + y >= 1\nBounds\n x free\n y free\nEnd";
    let result = run(text);
    assert_eq!(result.with_code(DiagnosticCode::ContradictoryRows).count(), 0, "{result:?}");
    assert_eq!(result.with_code(DiagnosticCode::DominatedRow).count(), 0, "{result:?}");
    // Bit-identical rows still contradict.
    let text = "Minimize\n obj: x + y\nSubject To\n c1: x + y <= 0\n c2: x + y >= 1\nBounds\n x free\n y free\nEnd";
    assert!(has(text, DiagnosticCode::ContradictoryRows));
}

#[test]
fn ranged_halves_are_not_dominated_as_a_whole() {
    // Feasible: only r's lower side is implied by c; the upper side binds.
    let text = "Minimize\n obj: x + y\nSubject To\n r: -5 <= x + y <= 10\n c: x + y >= 3\nBounds\n x free\n y free\nEnd";
    let result = run(text);
    assert_eq!(result.with_code(DiagnosticCode::DominatedRow).count(), 0, "{result:?}");
    assert_eq!(result.with_code(DiagnosticCode::ContradictoryRows).count(), 0, "{result:?}");
    // Infeasible: r's upper side (<= 1) conflicts with c (>= 3).
    let text = "Minimize\n obj: x + y\nSubject To\n r: -5 <= x + y <= 1\n c: x + y >= 3\nBounds\n x free\n y free\nEnd";
    let result = run(text);
    assert_eq!(result.with_code(DiagnosticCode::DominatedRow).count(), 0, "{result:?}");
    assert!(result.with_code(DiagnosticCode::ContradictoryRows).count() >= 1, "{result:?}");
}

fn fixture(name: &str) -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/diagnostics").join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

#[test]
fn new_items_capacity_fixture() {
    let text = fixture("capacity.lp");
    assert_eq!(subjects(&text, DiagnosticCode::RowRedundantByBounds), vec!["redundant"]);
    assert!(subjects(&text, DiagnosticCode::ContradictoryRows).contains(&"impossible".to_owned()));
}

#[test]
fn new_items_proportional_fixture() {
    let text = fixture("proportional.lp");
    let result = run(&text);
    let found = result.with_code(DiagnosticCode::ProportionalRow).next().expect("proportional-row");
    assert_eq!(found.message, "Constraint 'c2' is 'c1' scaled by 0.5");
    assert_eq!(found.subject.as_ref().unwrap().name, "c2");
    assert_eq!(found.related[0].subject.name, "c1");
}

#[test]
fn new_items_demand_fixture() {
    assert_eq!(subjects(&fixture("demand.lp"), DiagnosticCode::RowInfeasibleByBounds), vec!["demand"]);
}

#[test]
fn new_items_capacity_42_fixture() {
    let result = run(&fixture("capacity_42.lp"));
    let found = result.with_code(DiagnosticCode::RowCoefficientRange).next().expect("row-coefficient-range");
    assert_eq!(found.message, "Constraint 'capacity_42' has coefficients spanning about 12 orders of magnitude");
    assert!(found.explanation.as_deref().is_some_and(|e| e.contains("Rescale")));
}

fn resource_problems() -> Vec<(String, LpProblem)> {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("resources");
    let mut paths: Vec<_> = std::fs::read_dir(dir).expect("resources directory").map(|e| e.expect("directory entry").path()).collect();
    paths.sort();
    let mut problems = Vec::new();
    for path in paths {
        let Some(extension) = path.extension().and_then(|e| e.to_str()) else {
            continue;
        };
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let parsed = match extension {
            "lp" => LpProblem::parse(&text),
            "mps" => LpProblem::parse_mps(&text),
            _ => continue,
        };
        // Some resources are deliberately invalid input for the parser tests.
        if let Ok(problem) = parsed {
            let stem = path.file_name().and_then(|n| n.to_str()).expect("utf-8 file name").replace('.', "_");
            problems.push((stem, problem));
        }
    }
    problems
}

/// Every subject-bearing `analyze()` issue has a matching diagnostic (fleet.md 1.4).
#[test]
fn every_analysis_issue_maps_to_a_diagnostic() {
    use lp_parser_rs::analysis::IssueCategory;
    let mut mapped = 0;
    for (name, problem) in resource_problems() {
        let diagnostics = problem.diagnostics();
        for issue in &problem.analyze().issues {
            let Some(subject) = &issue.subject else {
                continue;
            };
            let codes: &[DiagnosticCode] = match issue.category {
                IssueCategory::InvalidBounds => &[DiagnosticCode::InvalidBounds],
                IssueCategory::EmptyConstraint => &[DiagnosticCode::EmptyRow, DiagnosticCode::EmptyRowInfeasible],
                IssueCategory::NumericalScaling => &[
                    DiagnosticCode::LargeCoefficient,
                    DiagnosticCode::SmallCoefficient,
                    DiagnosticCode::LargeRhs,
                    DiagnosticCode::CoefficientRange,
                ],
                IssueCategory::FixedVariable => &[DiagnosticCode::FixedVariable],
                IssueCategory::SingletonConstraint => &[DiagnosticCode::SingletonRow, DiagnosticCode::RowRedundantByBounds],
                IssueCategory::UnusedVariable => &[DiagnosticCode::UnusedVariable],
                IssueCategory::Other => panic!("{name}: an `Other` issue carries a subject: {}", issue.message),
            };
            let found = diagnostics
                .diagnostics
                .iter()
                .any(|d| codes.contains(&d.code) && d.subject.as_ref().is_some_and(|s| s.kind == subject.kind && s.name == subject.name));
            assert!(found, "{name}: no {codes:?} diagnostic for [{}] {}", issue.category, issue.message);
            mapped += 1;
        }
    }
    assert!(mapped > 0, "no issue was checked");
}

/// The Display form of the diagnostics of every resource model.
#[test]
fn resource_diagnostics_snapshots() {
    let problems = resource_problems();
    assert!(problems.len() > 20);
    for (name, problem) in problems {
        let result = problem.diagnostics();
        let mut text = String::new();
        for diagnostic in &result.diagnostics {
            writeln!(text, "{diagnostic}").expect("write to a String");
        }
        for (code, dropped) in &result.truncated {
            writeln!(text, "truncated {code}: {dropped}").expect("write to a String");
        }
        if text.is_empty() {
            text.push_str("no diagnostics\n");
        }
        insta::assert_snapshot!(name, text);
    }
}

/// Large models must stay linear: the dense row below made the role checks
/// quadratic (V x nnz).
#[test]
fn large_dense_model_is_analysed_quickly() {
    let n = 50_000;
    let sum = (0..n).map(|i| format!("x{i}")).collect::<Vec<_>>().join(" + ");
    let text = format!("Maximize\n obj: {sum}\nSubject To\n c1: {sum} <= 100\nEnd");
    let problem = LpProblem::parse(&text).expect("parse");
    let start = std::time::Instant::now();
    let result = problem.diagnostics();
    let elapsed = start.elapsed();
    assert!(elapsed < std::time::Duration::from_secs(5), "diagnostics took {elapsed:?}");
    assert_eq!(result.with_code(DiagnosticCode::NoEffectiveBound).count(), 0, "the dense row bounds every variable");
}
