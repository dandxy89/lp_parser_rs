//! `lp/diff`: what changed mathematically between the buffer and another copy
//! of the model.

use lp_parser_rs::LpProblem;
use lp_parser_rs::diff::{DiffOptions, DiffTol};
use ls_types as lsp;
use serde::{Deserialize, Serialize};

use super::sites::Sites;
use super::{Cancel, DocumentRef, FailureKind, RequestError, Source, Target};
use crate::document::Document;

/// Largest `against` text accepted (64 MiB).
pub const MAX_AGAINST_BYTES: usize = 64 * 1024 * 1024;
/// Entries per list; the counts always hold the full numbers.
pub const MAX_LIST: usize = 10_000;

/// The old side of the diff.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(untagged)]
pub enum Against {
    /// Model text sent by the client (a git revision, the previous save).
    Text {
        /// The model text.
        text: String,
        /// Shown to the user, e.g. `HEAD`.
        #[serde(default)]
        label: Option<String>,
    },
    /// Another document: the open or indexed copy, else the file on disk.
    Uri {
        /// Its URI.
        uri: String,
    },
}

/// Absolute and relative tolerances.
#[derive(Debug, Clone, Copy, Default, Deserialize)]
pub struct Tolerance {
    /// Absolute tolerance.
    #[serde(default)]
    pub abs: f64,
    /// Relative tolerance.
    #[serde(default)]
    pub rel: f64,
}

/// Params of `lp/diff`.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Params {
    /// Document (the new side) and progress token.
    #[serde(flatten)]
    pub target: Target,
    /// The old side.
    pub against: Against,
    /// When two numbers count as different (default: any difference).
    #[serde(default)]
    pub tolerance: Option<Tolerance>,
}

/// The old side once the server has fetched it.
#[derive(Debug, Clone)]
pub struct Resolved {
    /// Old model text.
    pub text: String,
    /// Label to echo.
    pub label: Option<String>,
}

impl Params {
    /// Reject a tolerance the diff engine would not accept, before any work.
    ///
    /// # Errors
    /// `-32602` for a negative, NaN or infinite tolerance, `tooLarge` for an oversized text.
    pub fn validate(&self) -> std::result::Result<DiffTol, RequestError> {
        let tolerance = self.tolerance.unwrap_or_default();
        let tol = DiffTol::new(tolerance.abs, tolerance.rel).map_err(|e| RequestError::invalid(e.to_string()))?;
        if let Against::Text { text, .. } = &self.against {
            check_size(text.len())?;
        }
        Ok(tol)
    }
}

/// `tooLarge` when `bytes` exceeds [`MAX_AGAINST_BYTES`].
///
/// # Errors
/// [`FailureKind::TooLarge`].
pub fn check_size(bytes: usize) -> std::result::Result<(), RequestError> {
    if bytes > MAX_AGAINST_BYTES {
        return Err(RequestError::failed(
            FailureKind::TooLarge,
            format!("the text to compare against is {bytes} bytes; the limit is {MAX_AGAINST_BYTES}"),
        ));
    }
    Ok(())
}

/// Result of `lp/diff`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Result {
    #[serde(flatten)]
    pub document: DocumentRef,
    pub label: Option<String>,
    pub identical: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sense: Option<SenseChange>,
    pub variables: Variables,
    pub constraints: Rows,
    pub objectives: Rows,
    pub counts: Counts,
    /// A list was cut at [`MAX_LIST`]; the counts are complete.
    pub truncated: bool,
}

/// Optimisation sense on both sides.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SenseChange {
    pub old: String,
    pub new: String,
}

/// Variable differences.
#[derive(Debug, Clone, PartialEq, Default, Serialize)]
pub struct Variables {
    pub added: Vec<Added>,
    pub removed: Vec<Removed>,
    pub changed: Vec<VariableChange>,
}

/// Constraint or objective differences.
#[derive(Debug, Clone, PartialEq, Default, Serialize)]
pub struct Rows {
    pub added: Vec<Added>,
    pub removed: Vec<Removed>,
    pub modified: Vec<Modified>,
}

/// Something only the new side has.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Added {
    pub name: String,
    pub range: Option<lsp::Range>,
}

/// Something only the old side has (no range: it is not in the buffer).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Removed {
    pub name: String,
}

/// A variable whose kind or bounds changed.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct VariableChange {
    pub name: String,
    pub old: String,
    pub new: String,
    pub range: Option<lsp::Range>,
}

/// A constraint or objective that changed.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Modified {
    pub name: String,
    pub changes: Vec<String>,
    pub range: Option<lsp::Range>,
}

/// Number of differences by category.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Counts {
    pub variables: VariableCounts,
    pub constraints: RowCounts,
    pub objectives: RowCounts,
}

/// Variable difference counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct VariableCounts {
    pub added: usize,
    pub removed: usize,
    pub changed: usize,
}

/// Constraint or objective difference counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct RowCounts {
    pub added: usize,
    pub removed: usize,
    pub modified: usize,
}

/// Handle `lp/diff`: `doc` is the new side, `against` the fetched old side.
///
/// # Errors
/// `-32602` for a bad tolerance, `parseError` (with `data.side`), `tooLarge`, or cancellation.
pub fn handle(
    doc: &Document,
    version: Option<i32>,
    params: &Params,
    against: &Resolved,
    cancel: &Cancel,
) -> std::result::Result<Result, RequestError> {
    let tol = params.validate()?;
    check_size(against.text.len())?;
    let new = Source::acquire(doc).map_err(|e| e.with("side", "current"))?;
    cancel.check()?;
    let old = LpProblem::parse(&against.text).map_err(|e| RequestError::parse(&e).with("side", "against"))?;
    cancel.check()?;
    let new = new.problem();
    let diff = old.diff(new, &DiffOptions { tol, normalise: None });
    cancel.check()?;

    let sites = Sites::new(doc, new);
    let lookup_constraint = |name: &str| new.name_id(name).and_then(|id| sites.constraint(id)).map(|s| s.range);
    let lookup_objective = |name: &str| new.name_id(name).and_then(|id| sites.objective(id)).map(|s| s.range);
    let lookup_variable = |name: &str| sites.variable(name);

    let mut truncated = false;
    let mut cap = |len: usize| {
        truncated |= len > MAX_LIST;
        len.min(MAX_LIST)
    };

    let mut variables = Variables::default();
    for (i, name) in diff.vars_added.iter().take(cap(diff.vars_added.len())).enumerate() {
        cancel.tick(i)?;
        variables.added.push(Added { name: name.clone(), range: lookup_variable(name) });
    }
    variables.removed = diff.vars_removed.iter().take(cap(diff.vars_removed.len())).map(|name| Removed { name: name.clone() }).collect();
    for (i, (name, old_desc, new_desc)) in diff.vars_type_changed.iter().take(cap(diff.vars_type_changed.len())).enumerate() {
        cancel.tick(i)?;
        variables.changed.push(VariableChange {
            name: name.clone(),
            old: old_desc.clone(),
            new: new_desc.clone(),
            range: lookup_variable(name),
        });
    }

    let mut rows = |added: &[String],
                    removed: &[String],
                    modified: &[(String, Vec<String>)],
                    lookup: &dyn Fn(&str) -> Option<lsp::Range>|
     -> std::result::Result<Rows, RequestError> {
        let mut out = Rows::default();
        for (i, name) in added.iter().take(cap(added.len())).enumerate() {
            cancel.tick(i)?;
            out.added.push(Added { name: name.clone(), range: lookup(name) });
        }
        out.removed = removed.iter().take(cap(removed.len())).map(|name| Removed { name: name.clone() }).collect();
        for (i, (name, changes)) in modified.iter().take(cap(modified.len())).enumerate() {
            cancel.tick(i)?;
            out.modified.push(Modified { name: name.clone(), changes: changes.clone(), range: lookup(name) });
        }
        Ok(out)
    };
    let constraints = rows(&diff.cons_added, &diff.cons_removed, &diff.cons_modified, &lookup_constraint)?;
    let objectives = rows(&diff.objs_added, &diff.objs_removed, &diff.objs_modified, &lookup_objective)?;

    let counts = Counts {
        variables: VariableCounts { added: diff.vars_added.len(), removed: diff.vars_removed.len(), changed: diff.vars_type_changed.len() },
        constraints: RowCounts { added: diff.cons_added.len(), removed: diff.cons_removed.len(), modified: diff.cons_modified.len() },
        objectives: RowCounts { added: diff.objs_added.len(), removed: diff.objs_removed.len(), modified: diff.objs_modified.len() },
    };
    cancel.check()?;
    Ok(Result {
        document: DocumentRef::new(&params.target.text_document.uri, version),
        label: against.label.clone(),
        identical: diff.is_empty(),
        sense: diff.sense_changed.map(|(old, new)| SenseChange { old, new }),
        variables,
        constraints,
        objectives,
        counts,
        truncated,
    })
}

#[cfg(test)]
mod tests {
    use std::fmt::Write as _;

    use serde_json::{Value, json};

    use super::super::testing::{MODEL, doc, json, target};
    use super::*;

    fn params(tolerance: Option<Tolerance>) -> Params {
        Params { target: target(), against: Against::Text { text: String::new(), label: None }, tolerance }
    }

    fn resolved(text: &str, label: Option<&str>) -> Resolved {
        Resolved { text: text.to_owned(), label: label.map(str::to_owned) }
    }

    fn run(new: &str, old: &str) -> Value {
        json(&handle(&doc(new), Some(1), &params(None), &resolved(old, Some("HEAD")), &Cancel::new()).expect("diff"))
    }

    const OLD: &str = "Minimize\n profit: 3 x + 2 y\nSubject To\n c1: x + y >= 2\n capacity_old: x <= 9\nBounds\n y <= 4\nEnd\n";
    const NEW: &str = "Maximize\n profit: 3 x + 5 y\nSubject To\n c1: x + y >= 3\n demand_2026: x + y <= 9\nBounds\n y <= 4\n 0 <= x_new <= 7\nGenerals\n y\nEnd\n";

    #[test]
    fn reports_added_removed_and_changed_with_ranges_for_the_buffer() {
        let diff = run(NEW, OLD);
        assert_eq!((diff["identical"].clone(), diff["label"].clone()), (json!(false), json!("HEAD")));
        assert_eq!(diff["sense"], json!({ "old": "Minimize", "new": "Maximize" }));
        assert_eq!(diff["variables"]["added"][0]["name"], "x_new");
        assert!(diff["variables"]["added"][0]["range"]["start"].is_object());
        assert_eq!(diff["variables"]["changed"][0]["name"], "y");
        assert!(diff["variables"]["changed"][0]["new"].as_str().is_some_and(|s| s.starts_with("General")), "{diff}");
        assert_eq!(diff["constraints"]["added"][0]["name"], "demand_2026");
        assert!(diff["constraints"]["added"][0]["range"]["start"]["line"].is_u64());
        assert_eq!(diff["constraints"]["removed"], json!([{ "name": "capacity_old" }]), "removed names have no range");
        let c1 = &diff["constraints"]["modified"][0];
        assert_eq!(c1["name"], "c1");
        assert!(c1["changes"].as_array().is_some_and(|c| c.iter().any(|c| c.as_str().is_some_and(|c| c.contains("rhs")))), "{c1}");
        assert_eq!(diff["objectives"]["modified"][0]["name"], "profit");
        assert_eq!(
            diff["counts"],
            json!({
                "variables": { "added": 1, "removed": 0, "changed": 1 },
                "constraints": { "added": 1, "removed": 1, "modified": 1 },
                "objectives": { "added": 0, "removed": 0, "modified": 1 }
            })
        );
        assert_eq!(diff["truncated"], false);
    }

    #[test]
    fn identical_models_omit_the_sense() {
        let diff = run(MODEL, MODEL);
        assert_eq!(diff["identical"], true);
        assert!(diff.get("sense").is_none());
        assert_eq!(diff["variables"], json!({ "added": [], "removed": [], "changed": [] }));
    }

    #[test]
    fn tolerance_hides_small_changes() {
        let old = "Minimize\n obj: x\nSubject To\n c1: x >= 100\nEnd\n";
        let new = "Minimize\n obj: x\nSubject To\n c1: x >= 100.5\nEnd\n";
        let strict = handle(&doc(new), Some(1), &params(None), &resolved(old, None), &Cancel::new()).expect("diff");
        assert!(!strict.identical);
        let loose = handle(&doc(new), Some(1), &params(Some(Tolerance { abs: 1.0, rel: 0.0 })), &resolved(old, None), &Cancel::new())
            .expect("diff");
        assert!(loose.identical);
        let relative = handle(&doc(new), Some(1), &params(Some(Tolerance { abs: 0.0, rel: 0.01 })), &resolved(old, None), &Cancel::new())
            .expect("diff");
        assert!(relative.identical);
    }

    #[test]
    fn rejects_invalid_tolerances_and_oversized_text() {
        for bad in [Tolerance { abs: -1.0, rel: 0.0 }, Tolerance { abs: 0.0, rel: f64::NAN }, Tolerance { abs: f64::INFINITY, rel: 0.0 }] {
            assert!(
                matches!(
                    handle(&doc(MODEL), None, &params(Some(bad)), &resolved(MODEL, None), &Cancel::new()),
                    Err(RequestError::InvalidParams(_))
                ),
                "{bad:?}"
            );
        }
        assert!(matches!(check_size(MAX_AGAINST_BYTES + 1), Err(RequestError::Failed(f)) if f.kind == FailureKind::TooLarge));
        assert!(check_size(MAX_AGAINST_BYTES).is_ok());
        let mut p = params(None);
        p.against = Against::Text { text: "x".repeat(MAX_AGAINST_BYTES + 1), label: None };
        assert!(matches!(p.validate(), Err(RequestError::Failed(f)) if f.kind == FailureKind::TooLarge));
    }

    #[test]
    fn parse_errors_name_the_failing_side() {
        let broken = "Minimize\n obj: 3 x +\nSubject To\n c1: >= \nEnd\n";
        let side = |error: Option<RequestError>| error.and_then(|e| e.data()).map(|d| d["side"].clone());
        let current = handle(&doc(broken), None, &params(None), &resolved(MODEL, None), &Cancel::new()).err();
        assert_eq!(side(current), Some(json!("current")));
        let against = handle(&doc(MODEL), None, &params(None), &resolved(broken, None), &Cancel::new()).err();
        assert_eq!(side(against), Some(json!("against")));
    }

    #[test]
    fn lists_are_capped_but_counts_are_not() {
        let mut new = String::from("Minimize\n obj: x\nSubject To\n");
        for i in 0..MAX_LIST + 3 {
            writeln!(new, " r{i}: x >= {i}").unwrap();
        }
        new.push_str("End\n");
        let diff = run(&new, "Minimize\n obj: x\nSubject To\n c: x >= 1\nEnd\n");
        assert_eq!(diff["constraints"]["added"].as_array().map(Vec::len), Some(MAX_LIST));
        assert_eq!(diff["counts"]["constraints"]["added"], MAX_LIST + 3);
        assert_eq!(diff["truncated"], true);
    }

    #[test]
    fn cancellation_is_honoured() {
        let cancel = Cancel::new();
        cancel.cancel();
        assert_eq!(handle(&doc(MODEL), None, &params(None), &resolved(MODEL, None), &cancel).err(), Some(RequestError::Cancelled));
    }

    #[test]
    fn untagged_against_deserialises_both_forms() {
        let text: Against = serde_json::from_value(json!({ "text": "min\nend", "label": "HEAD" })).expect("text form");
        assert!(matches!(text, Against::Text { label: Some(l), .. } if l == "HEAD"));
        let uri: Against = serde_json::from_value(json!({ "uri": "file:///a.lp" })).expect("uri form");
        assert_eq!(uri, Against::Uri { uri: "file:///a.lp".into() });
        assert!(serde_json::from_value::<Against>(json!({ "label": "only" })).is_err());
    }
}
