//! `workspace/executeCommand` handlers.

use std::fmt::Write as _;

use lp_parser_rs::LpProblem;
use lp_parser_rs::analysis::{AnalysisIssue, IssueSeverity, ProblemAnalysis};
use lp_parser_rs::model::{ConstraintClass, VariableKind};
use lp_parser_rs::mps::writer::write_mps_string;
use serde_json::json;

use crate::config::Config;
use crate::document::Document;

/// Run the full analysis and return a markdown report.
pub const ANALYZE: &str = "lp.analyze";
/// Write an MPS file next to the source.
pub const CONVERT_TO_MPS: &str = "lp.convertToMps";
/// Summarise variables, constraints, nonzeros and types.
pub const SHOW_MODEL_STATS: &str = "lp.showModelStats";

/// Every server-side command.
pub const ALL: &[&str] = &[ANALYZE, CONVERT_TO_MPS, SHOW_MODEL_STATS];

/// Result of a command: a JSON value for the caller and a message to show.
#[derive(Debug, Clone, PartialEq)]
pub struct Output {
    /// Returned to the client.
    pub value: serde_json::Value,
    /// Shown via `window/showMessage`.
    pub message: String,
}

/// Execute `command` against `doc` (the document named by the first argument).
///
/// # Errors
/// A user-facing message for unknown commands or failures.
pub fn execute(command: &str, doc: &Document, config: &Config) -> Result<Output, String> {
    match command {
        ANALYZE => analyze(doc, config),
        CONVERT_TO_MPS => convert_to_mps(doc),
        SHOW_MODEL_STATS => model_stats(doc),
        _ => Err(format!("unknown command '{command}'")),
    }
}

/// Short name of the document for messages: the file name, else the URI.
fn display_name(doc: &Document) -> String {
    let uri = doc.uri.as_str();
    uri.rsplit('/').next().filter(|n| !n.is_empty()).unwrap_or(uri).to_owned()
}

fn parse(doc: &Document) -> Result<LpProblem, String> {
    LpProblem::parse(&doc.text).map_err(|e| format!("cannot parse {}: {e}", display_name(doc)))
}

fn analyze(doc: &Document, config: &Config) -> Result<Output, String> {
    let problem = parse(doc)?;
    let analysis = problem.analyze_with_config(&config.analysis.to_upstream());
    let markdown = analysis_markdown(&display_name(doc), &analysis);
    let count = |severity: IssueSeverity| analysis.issues.iter().filter(|i| i.severity == severity).count();
    let message = if analysis.issues.is_empty() {
        format!("{}: no issues detected", display_name(doc))
    } else {
        format!(
            "{}: {} error(s), {} warning(s), {} info",
            display_name(doc),
            count(IssueSeverity::Error),
            count(IssueSeverity::Warning),
            count(IssueSeverity::Info)
        )
    };
    Ok(Output { value: json!({ "markdown": markdown }), message })
}

/// Render the analysis as markdown: headings, bullet lists and issues
/// grouped by severity.
fn analysis_markdown(title: &str, analysis: &ProblemAnalysis) -> String {
    let mut md = String::new();
    // Writing to a `String` cannot fail; `line` keeps that in one place.
    let mut line = |text: String| {
        md.push_str(&text);
        md.push('\n');
    };
    let summary = &analysis.summary;
    line(format!("# Analysis of `{title}`\n"));
    line("## Summary\n".to_owned());
    if let Some(name) = &summary.name {
        line(format!("- **Name:** {name}"));
    }
    line(format!("- **Sense:** {}", summary.sense));
    line(format!("- **Objectives:** {}", summary.objective_count));
    line(format!("- **Constraints:** {}", summary.constraint_count));
    line(format!("- **Variables:** {}", summary.variable_count));
    line(format!("- **Non-zeros:** {} (density {:.2}%)", summary.total_nonzeros, summary.density * 100.0));
    if summary.quadratic_objective_terms > 0 || summary.quadratic_constraint_terms > 0 {
        line(format!(
            "- **Quadratic terms:** {} in objectives, {} in constraints",
            summary.quadratic_objective_terms, summary.quadratic_constraint_terms
        ));
    }
    line(format!(
        "- **Variables per constraint:** min {}, max {}",
        analysis.sparsity.min_vars_per_constraint, analysis.sparsity.max_vars_per_constraint
    ));

    let vt = &analysis.variables.type_distribution;
    line("\n## Variables\n".to_owned());
    let continuous = vt.free + vt.unspecified + vt.lower_bounded + vt.upper_bounded + vt.double_bounded;
    for (label, count) in [
        ("Continuous", continuous),
        ("Free", vt.free),
        ("Binary", vt.binary),
        ("Integer", vt.integer + vt.general),
        ("Semi-continuous", vt.semi_continuous),
        ("Semi-integer", vt.semi_integer),
        ("SOS members", vt.sos),
        ("Fixed", analysis.variables.fixed_variables.len()),
        ("Unused", analysis.variables.unused_variables.len()),
    ] {
        if count > 0 || label == "Continuous" {
            line(format!("- **{label}:** {count}"));
        }
    }

    let ct = &analysis.constraints.type_distribution;
    line("\n## Constraints\n".to_owned());
    for (label, count) in [
        ("Equality (=)", ct.equality),
        ("Less or equal (<=)", ct.less_than_equal),
        ("Greater or equal (>=)", ct.greater_than_equal),
        ("Strict less (<)", ct.less_than),
        ("Strict greater (>)", ct.greater_than),
        ("Indicator", ct.indicator),
        ("Quadratic", ct.quadratic),
        ("General", ct.general),
        ("SOS1", ct.sos1),
        ("SOS2", ct.sos2),
        ("Lazy", ct.lazy),
        ("User cuts", ct.user_cuts),
        ("Empty", analysis.constraints.empty_constraints.len()),
        ("Singleton", analysis.constraints.singleton_constraints.len()),
    ] {
        if count > 0 {
            line(format!("- **{label}:** {count}"));
        }
    }

    let coefficients = &analysis.coefficients;
    if coefficients.constraint_coeff_range.count > 0 || coefficients.objective_coeff_range.count > 0 {
        line("\n## Coefficients\n".to_owned());
        for (label, range) in [("Constraint", &coefficients.constraint_coeff_range), ("Objective", &coefficients.objective_coeff_range)] {
            if range.count > 0 {
                line(format!("- **{label} |coefficients|:** {:.3e} to {:.3e} ({} values)", range.min, range.max, range.count));
            }
        }
        if coefficients.coefficient_ratio > 1.0 {
            line(format!("- **Max/min ratio:** {:.3e}", coefficients.coefficient_ratio));
        }
    }

    line("\n## Issues\n".to_owned());
    if analysis.issues.is_empty() {
        line("No issues detected.".to_owned());
    }
    for (heading, severity) in [("Errors", IssueSeverity::Error), ("Warnings", IssueSeverity::Warning), ("Info", IssueSeverity::Info)] {
        let issues: Vec<&AnalysisIssue> = analysis.issues.iter().filter(|i| i.severity == severity).collect();
        if issues.is_empty() {
            continue;
        }
        line(format!("### {heading} ({})\n", issues.len()));
        for issue in issues {
            let mut item = format!("- **{}:** {}", issue.category, issue.message);
            if let Some(details) = &issue.details {
                // Infallible: writing to a String.
                write!(item, " ({details})").expect("writing to a String cannot fail");
            }
            line(item);
        }
        line(String::new());
    }
    md
}

fn convert_to_mps(doc: &Document) -> Result<Output, String> {
    // `to_file_path` does not check the scheme (`untitled:x` would yield a
    // relative path), so insist on an absolute `file:` path.
    let not_a_file = || format!("{CONVERT_TO_MPS} needs a file on disk, not '{}'", doc.uri.as_str());
    if !doc.uri.scheme().as_str().eq_ignore_ascii_case("file") {
        return Err(not_a_file());
    }
    let source = doc.uri.to_file_path().filter(|p| p.is_absolute()).ok_or_else(not_a_file)?;
    let target = source.with_extension("mps");
    debug_assert_ne!(source.as_ref(), target.as_path(), "the MPS file must not overwrite its source");
    let problem = parse(doc)?;
    let mps = write_mps_string(&problem).map_err(|e| format!("cannot convert {} to MPS: {e}", display_name(doc)))?;
    std::fs::write(&target, mps).map_err(|e| format!("cannot write {}: {e}", target.display()))?;
    let path = target.display().to_string();
    Ok(Output { value: json!({ "path": path }), message: format!("Wrote {path}") })
}

fn model_stats(doc: &Document) -> Result<Output, String> {
    let problem = parse(doc)?;

    let mut kinds: Vec<(VariableKind, usize)> = Vec::new();
    let mut free = 0;
    for variable in problem.variables.values() {
        match kinds.iter_mut().find(|(k, _)| *k == variable.kind) {
            Some((_, count)) => *count += 1,
            None => kinds.push((variable.kind, 1)),
        }
        free += usize::from(variable.bounds.is_free());
    }

    let mut classes = [(ConstraintClass::Normal, 0usize), (ConstraintClass::Lazy, 0), (ConstraintClass::UserCut, 0)];
    let mut types: Vec<(&'static str, usize)> = Vec::new();
    let mut nonzeros = 0usize;
    let mut quadratic_terms = 0usize;
    for (&id, constraint) in &problem.constraints {
        let class = problem.constraint_class(id);
        if let Some((_, count)) = classes.iter_mut().find(|(c, _)| *c == class) {
            *count += 1;
        }
        let (kind, linear, quadratic) = match constraint {
            lp_parser_rs::model::Constraint::Standard { coefficients, .. } => ("linear", coefficients.len(), 0),
            lp_parser_rs::model::Constraint::Indicator { coefficients, .. } => ("indicator", coefficients.len(), 0),
            lp_parser_rs::model::Constraint::Quadratic { coefficients, quadratic, .. } => {
                ("quadratic", coefficients.len(), quadratic.len())
            }
            lp_parser_rs::model::Constraint::SOS { weights, .. } => ("sos", weights.len(), 0),
            lp_parser_rs::model::Constraint::General { .. } => ("general", 0, 0),
        };
        nonzeros += linear;
        quadratic_terms += quadratic;
        match types.iter_mut().find(|(k, _)| *k == kind) {
            Some((_, count)) => *count += 1,
            None => types.push((kind, 1)),
        }
    }
    let objective_nonzeros: usize = problem.objectives.values().map(|o| o.coefficients.len()).sum();
    let objective_quadratic: usize = problem.objectives.values().map(|o| o.quadratic.len()).sum();

    let variables_by_kind: serde_json::Map<String, serde_json::Value> = kinds.iter().map(|(k, n)| (k.to_string(), json!(n))).collect();
    let constraints_by_class: serde_json::Map<String, serde_json::Value> =
        classes.iter().filter(|(_, n)| *n > 0).map(|(c, n)| (c.to_string(), json!(n))).collect();
    let constraints_by_type: serde_json::Map<String, serde_json::Value> = types.iter().map(|(t, n)| ((*t).to_owned(), json!(n))).collect();
    let stats = json!({
        "variables": { "total": problem.variable_count(), "free": free, "byKind": variables_by_kind },
        "constraints": { "total": problem.constraint_count(), "byClass": constraints_by_class, "byType": constraints_by_type },
        "objectives": { "total": problem.objective_count(), "sense": problem.sense.to_string() },
        "nonzeros": { "constraints": nonzeros, "objectives": objective_nonzeros, "quadratic": quadratic_terms + objective_quadratic },
    });

    let mut md = format!("# Model statistics for `{}`\n\n", display_name(doc));
    let mut section = |heading: &str, total: usize, rows: &[(String, usize)]| {
        // Writing to a `String` cannot fail.
        writeln!(md, "## {heading} ({total})\n").expect("writing to a String cannot fail");
        for (label, count) in rows {
            writeln!(md, "- **{label}:** {count}").expect("writing to a String cannot fail");
        }
        md.push('\n');
    };
    let mut variable_rows: Vec<(String, usize)> = kinds.iter().map(|(k, n)| (k.to_string(), *n)).collect();
    if free > 0 {
        variable_rows.push(("Free (unbounded)".to_owned(), free));
    }
    section("Variables", problem.variable_count(), &variable_rows);
    let mut constraint_rows: Vec<(String, usize)> =
        classes.iter().filter(|(_, n)| *n > 0).map(|(c, n)| (format!("{c} class"), *n)).collect();
    constraint_rows.extend(types.iter().map(|(t, n)| ((*t).to_owned(), *n)));
    section("Constraints", problem.constraint_count(), &constraint_rows);
    section("Objectives", problem.objective_count(), &[(format!("Sense {}", problem.sense), problem.objective_count())]);
    section(
        "Non-zeros",
        nonzeros + objective_nonzeros,
        &[
            ("In constraints".to_owned(), nonzeros),
            ("In objectives".to_owned(), objective_nonzeros),
            ("Quadratic terms".to_owned(), quadratic_terms + objective_quadratic),
        ],
    );

    let message = format!(
        "{}: {} variables, {} constraints, {} objective(s), {} non-zeros",
        display_name(doc),
        problem.variable_count(),
        problem.constraint_count(),
        problem.objective_count(),
        nonzeros + objective_nonzeros
    );
    Ok(Output { value: json!({ "markdown": md, "stats": stats }), message })
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use tower_lsp_server::ls_types::Uri;

    use super::*;
    use crate::position::Encoding;

    const MODEL: &str = "Minimize\n obj: 3 x + 2 y\nSubject To\n c1: x + y >= 1\n c2: 2 <= x - y <= 4\nLazy Constraints\n l1: x <= 1e12\nBounds\n y free\nGenerals\n x\nEnd\n";
    const BROKEN: &str = "Minimize\n obj: 3 x +\nSubject To\n c1: >= 1\nEnd\n";

    fn doc_at(uri: &str, text: &str) -> Document {
        Document::new(uri.parse::<Uri>().unwrap(), text.to_owned(), 1, Encoding::Utf16)
    }

    fn doc(text: &str) -> Document {
        doc_at("file:///tmp/model.lp", text)
    }

    #[test]
    fn analyze_returns_markdown_report() {
        let out = execute(ANALYZE, &doc(MODEL), &Config::default()).unwrap();
        let markdown = out.value["markdown"].as_str().unwrap();
        assert!(markdown.starts_with("# Analysis of `model.lp`"));
        assert!(markdown.contains("## Summary"));
        assert!(markdown.contains("- **Constraints:** 4"));
        assert!(markdown.contains("### Warnings"), "{markdown}");
        assert!(out.message.starts_with("model.lp:"));
    }

    #[test]
    fn analyze_uses_configured_thresholds() {
        let mut config = Config::default();
        config.analysis.large_rhs_threshold = 1e20;
        config.analysis.large_coefficient_threshold = 1e20;
        let out = execute(ANALYZE, &doc("min\n obj: x\nst\n c1: x >= 1e12\nend\n"), &config).unwrap();
        assert!(!out.value["markdown"].as_str().unwrap().contains("1e12"));
    }

    #[test]
    fn parse_errors_are_readable() {
        for command in ALL {
            let err = execute(command, &doc(BROKEN), &Config::default()).unwrap_err();
            assert!(err.starts_with("cannot parse model.lp"), "{command}: {err}");
        }
        assert_eq!(execute("lp.nope", &doc(MODEL), &Config::default()).unwrap_err(), "unknown command 'lp.nope'");
    }

    #[test]
    fn model_stats_summarise_the_model() {
        let out = execute(SHOW_MODEL_STATS, &doc(MODEL), &Config::default()).unwrap();
        let stats = &out.value["stats"];
        assert_eq!(stats["variables"]["total"], 2);
        assert_eq!(stats["variables"]["free"], 1);
        assert_eq!(stats["variables"]["byKind"]["General"], 1);
        assert_eq!(stats["constraints"]["total"], 4);
        assert_eq!(stats["constraints"]["byClass"]["Normal"], 3);
        assert_eq!(stats["constraints"]["byClass"]["Lazy"], 1);
        assert_eq!(stats["constraints"]["byType"]["linear"], 4);
        assert_eq!(stats["objectives"]["total"], 1);
        assert_eq!(stats["nonzeros"]["constraints"], 7);
        assert_eq!(stats["nonzeros"]["objectives"], 2);
        let markdown = out.value["markdown"].as_str().unwrap();
        assert!(markdown.contains("## Variables (2)"));
        assert!(markdown.contains("## Non-zeros (9)"));
        assert_eq!(out.message, "model.lp: 2 variables, 4 constraints, 1 objective(s), 9 non-zeros");
    }

    #[test]
    fn convert_to_mps_writes_next_to_source() {
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let dir = std::env::temp_dir().join(format!("lp-lsp-commands-{}-{nanos}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let source = dir.join("model.lp");
        let uri = Uri::from_file_path(&source).unwrap();
        let d = doc_at(uri.as_str(), MODEL);

        let result = execute(CONVERT_TO_MPS, &d, &Config::default());
        let target = dir.join("model.mps");
        let written = std::fs::read_to_string(&target);
        std::fs::remove_dir_all(&dir).unwrap();

        let out = result.unwrap();
        assert_eq!(out.value["path"], target.display().to_string());
        let mps = written.unwrap();
        let round_trip = LpProblem::parse_mps(&mps).unwrap();
        assert_eq!(round_trip.constraint_count(), 4);
        assert_eq!(round_trip.variable_count(), 2);
    }

    #[test]
    fn convert_to_mps_errors() {
        let err = execute(CONVERT_TO_MPS, &doc_at("untitled:Untitled-1", MODEL), &Config::default()).unwrap_err();
        assert!(err.contains("needs a file on disk"), "{err}");
        // Unwritable target: the parent directory does not exist.
        let missing = std::env::temp_dir().join(format!("lp-lsp-missing-{}", std::process::id())).join("model.lp");
        let uri = Uri::from_file_path(&missing).unwrap();
        let err = execute(CONVERT_TO_MPS, &doc_at(uri.as_str(), MODEL), &Config::default()).unwrap_err();
        assert!(err.starts_with("cannot write"), "{err}");
    }
}
