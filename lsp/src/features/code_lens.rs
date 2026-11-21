//! Code lenses: variable counts on labels, usage counts on variable definitions
//! and, when enabled, Solve and Explain actions.
//!
//! Usage lenses are resolved lazily: `textDocument/codeLens` returns only
//! their range and the variable name, and `codeLens/resolve` fills in the
//! title and the reference locations, so a large model does not send every
//! variable's full location list up front.

use std::fmt::Write as _;

use ls_types::{CodeLens, Command, Range};
use serde_json::{Value, json};

use crate::document::Document;
use crate::features::navigation;
use crate::index::EntityKind;

/// Client-side command the usage lens runs: arguments are the document URI,
/// the position and the reference locations.
pub const SHOW_REFERENCES: &str = "lp.showReferences";

/// Client-side command the Solve lens runs: the argument is the document URI.
pub const SOLVE: &str = "lp.solve";

/// Client-side command the Explain lens runs: arguments are the document URI
/// and the position of the constraint's label.
pub const EXPLAIN_CONSTRAINT: &str = "lp.explainConstraint";

/// What to add besides the count and usage lenses.
#[derive(Debug, Clone, Copy, Default)]
pub struct Actions {
    /// Explain on each labelled constraint.
    pub explain: bool,
    /// Solve on the first objective.
    pub solve: bool,
}

/// Lenses for `doc`. Usage lenses are unresolved (see [`resolve`]).
///
/// # Panics
/// Never in practice: serialising `Position` to JSON cannot fail.
#[must_use]
pub fn lenses(doc: &Document, actions: Actions) -> Vec<CodeLens> {
    let index = doc.index();

    // Distinct variables per entity, in one pass over the occurrences.
    // `Variable::entities` is deduplicated: an entity's occurrences of one
    // variable are contiguous because entities are disjoint and in order.
    let mut variable_counts = vec![0usize; index.entities.len()];
    // Each lens's range, rank and source (variable `i`, entity `i - variables`,
    // or action `i - variables - entities`), sorted by start with ties actions
    // first, then in source order, before any lens is built, so the sort moves
    // small keys rather than whole lenses.
    let variables = index.variables.len();
    let entities = index.entities.len();
    let mut anchors: Vec<(Range, u8, usize)> = Vec::with_capacity(variables + entities);
    let mut commands: Vec<Command> = Vec::new();
    let uri = doc.uri.as_str();
    for (i, variable) in index.variables.iter().enumerate() {
        let mut previous = None;
        for entity in variable.occurrences.iter().filter_map(|o| o.entity) {
            if previous != Some(entity) {
                variable_counts[entity] += 1;
                previous = Some(entity);
            }
        }
        anchors.push((doc.range(variable.definition().range.clone()), 1, i));
    }
    for (i, entity) in index.entities.iter().enumerate() {
        let anchor = entity.name_range.clone().unwrap_or(entity.range.start..entity.range.start);
        anchors.push((doc.range(anchor), 1, variables + i));
    }
    let mut solve_pending = actions.solve;
    for entity in &index.entities {
        let (command, anchor) = match entity.kind {
            EntityKind::Objective if solve_pending => {
                solve_pending = false;
                let anchor = entity.name_range.clone().unwrap_or(entity.range.start..entity.range.start);
                (Command::new("Solve".to_owned(), SOLVE.to_owned(), Some(vec![json!(uri)])), anchor)
            }
            EntityKind::Constraint if actions.explain => {
                let Some(anchor) = entity.name_range.clone() else { continue };
                debug_assert!(entity.range.start <= anchor.start && anchor.end <= entity.range.end, "label inside its constraint");
                // Plain data type: serialisation cannot fail.
                let position = serde_json::to_value(doc.position(anchor.start)).expect("Position serialises");
                (Command::new("Explain".to_owned(), EXPLAIN_CONSTRAINT.to_owned(), Some(vec![json!(uri), position])), anchor)
            }
            _ => continue,
        };
        anchors.push((doc.range(anchor), 0, variables + entities + commands.len()));
        commands.push(command);
    }
    anchors.sort_unstable_by_key(|(range, rank, i)| (range.start.line, range.start.character, *rank, *i));

    anchors
        .into_iter()
        .map(|(range, _, i)| match index.variables.get(i) {
            Some(variable) => CodeLens { range, command: None, data: Some(json!({ "uri": uri, "variable": variable.name })) },
            None if i < variables + entities => CodeLens {
                range,
                command: Some(Command::new(count(variable_counts[i - variables], "{} variable"), String::new(), None)),
                data: None,
            },
            None => CodeLens { range, command: Some(commands[i - variables - entities].clone()), data: None },
        })
        .collect()
}

/// Document URI of an unresolved lens from [`lenses`].
///
/// # Errors
/// When the lens carries no URI.
pub fn uri(lens: &CodeLens) -> Result<&str, String> {
    lens.data.as_ref().and_then(|d| d.get("uri")).and_then(Value::as_str).ok_or_else(|| "code lens has no document URI".to_owned())
}

/// Fill in the title and `lp.showReferences` command of a usage lens.
///
/// # Errors
/// When the lens is not a usage lens from [`lenses`], or its variable is no
/// longer in `doc` (the document changed since the lenses were computed).
///
/// # Panics
/// Never in practice: serialising `Position` and `Location` to JSON cannot fail.
pub fn resolve(doc: &Document, mut lens: CodeLens) -> Result<CodeLens, String> {
    if lens.command.is_some() {
        return Ok(lens);
    }
    let name = lens.data.as_ref().and_then(|d| d.get("variable")).and_then(Value::as_str).ok_or("code lens has no variable name")?;
    let index = doc.index();
    let variable = index.variable(name).ok_or_else(|| format!("variable `{name}` is no longer in {}", doc.uri.as_str()))?;
    let constraints = variable.entities().into_iter().filter(|&e| index.entities[e].kind != EntityKind::Objective).count();
    let arguments = vec![
        Value::String(doc.uri.as_str().to_owned()),
        // Plain data types: serialisation cannot fail.
        serde_json::to_value(doc.position(variable.definition().range.start)).expect("Position serialises"),
        serde_json::to_value(navigation::usages(doc, variable)).expect("Location serialises"),
    ];
    lens.command = Some(Command::new(count(constraints, "used in {} constraint"), SHOW_REFERENCES.to_owned(), Some(arguments)));
    Ok(lens)
}

/// `template` with `{}` replaced by `n`, pluralised with a trailing `s`.
fn count(n: usize, template: &str) -> String {
    debug_assert_eq!(template.matches("{}").count(), 1, "one placeholder");
    let Some((before, after)) = template.split_once("{}") else { return template.to_owned() };
    // One allocation: the text, up to 20 digits and the plural `s`.
    let mut text = String::with_capacity(before.len() + 20 + after.len() + 1);
    text.push_str(before);
    write!(text, "{n}").expect("writing to a String cannot fail");
    text.push_str(after);
    if n != 1 {
        text.push('s');
    }
    text
}

#[cfg(test)]
mod tests {
    use ls_types::Location;

    use super::*;
    use crate::position::Encoding;

    fn resolved(doc: &Document) -> Vec<CodeLens> {
        lenses(doc, Actions::default()).into_iter().map(|l| resolve(doc, l).unwrap()).collect()
    }

    fn titles(text: &str) -> Vec<(u32, String)> {
        let doc = Document::new("file:///t.lp".parse().unwrap(), text.to_owned(), 0, Encoding::Utf16);
        resolved(&doc).into_iter().map(|l| (l.range.start.line, l.command.unwrap().title)).collect()
    }

    #[test]
    fn counts_variables_and_usages() {
        let text = "min\n obj: x + y + x\nst\n c1: x + y >= 1\n x >= 0\nBounds\n x <= 10\nEnd\n";
        assert_eq!(
            titles(text),
            [
                (1, "2 variables".to_owned()),
                (1, "used in 1 constraint".to_owned()),
                (3, "2 variables".to_owned()),
                (4, "1 variable".to_owned()),
                (6, "used in 2 constraints".to_owned()),
            ]
        );
    }

    #[test]
    fn usage_lens_carries_show_references_arguments() {
        let text = "min\n obj: x\nst\n c1: x >= 1\nBounds\n x <= 1\nEnd\n";
        let doc = Document::new("file:///t.lp".parse().unwrap(), text.to_owned(), 0, Encoding::Utf16);
        let lens = resolved(&doc).into_iter().find(|l| l.range.start.line == 5).unwrap();
        let command = lens.command.unwrap();
        assert_eq!(command.command, SHOW_REFERENCES);
        let arguments = command.arguments.unwrap();
        assert_eq!(arguments[0], Value::String("file:///t.lp".to_owned()));
        let locations: Vec<Location> = serde_json::from_value(arguments[2].clone()).unwrap();
        assert_eq!(locations.iter().map(|l| l.range.start.line).collect::<Vec<_>>(), [1, 3]);
    }

    #[test]
    fn usage_lenses_defer_locations_to_resolve() {
        let text = "min\n obj: x\nst\n c1: x >= 1\nBounds\n x <= 1\nEnd\n";
        let doc = Document::new("file:///t.lp".parse().unwrap(), text.to_owned(), 0, Encoding::Utf16);
        let lens = lenses(&doc, Actions::default()).into_iter().find(|l| l.range.start.line == 5).unwrap();
        assert!(lens.command.is_none(), "usage lens is unresolved");
        assert_eq!(uri(&lens), Ok("file:///t.lp"));

        // Resolved against a newer version where the variable is gone.
        let edited = Document::new(doc.uri.clone(), "min\n obj: y\nEnd\n".to_owned(), 1, Encoding::Utf16);
        assert!(resolve(&edited, lens).unwrap_err().contains("`x`"));
    }

    fn doc(text: &str) -> Document {
        Document::new("file:///t.lp".parse().unwrap(), text.to_owned(), 0, Encoding::Utf16)
    }

    fn action_titles(doc: &Document, actions: Actions) -> Vec<(u32, String)> {
        lenses(doc, actions).into_iter().filter_map(|l| l.command.map(|c| (l.range.start.line, c.title))).collect()
    }

    #[test]
    fn action_lenses_solve_the_first_objective_and_explain_named_constraints() {
        let doc = doc("min\n obj: x + y\nst\n c1: x + y >= 1\n c2: x >= 0\nEnd\n");
        let lenses = lenses(&doc, Actions { explain: true, solve: true });
        let at = |line: u32| -> Vec<&CodeLens> { lenses.iter().filter(|l| l.command.is_some() && l.range.start.line == line).collect() };
        let command = |lens: &CodeLens| lens.command.clone().unwrap();

        let [solve, count] = at(1)[..] else { panic!("Solve and count on line 1") };
        assert_eq!((command(solve).command.as_str(), command(solve).title.as_str()), (SOLVE, "Solve"));
        assert_eq!(command(solve).arguments, Some(vec![json!("file:///t.lp")]));
        assert_eq!(command(count).title, "2 variables");

        for (line, explain_at) in [(3, 3), (4, 4)] {
            let [explain, count] = at(line)[..] else { panic!("Explain and count on line {line}") };
            assert_eq!(command(explain).command, EXPLAIN_CONSTRAINT);
            assert_eq!(command(explain).title, "Explain");
            assert_eq!(command(explain).arguments, Some(vec![json!("file:///t.lp"), json!({ "line": explain_at, "character": 1 })]));
            assert!(command(count).title.ends_with("variable") || command(count).title.ends_with("variables"));
        }
    }

    #[test]
    fn no_action_lenses_by_default() {
        let doc = doc("min\n obj: x\nst\n c1: x >= 1\nEnd\n");
        let titles = action_titles(&doc, Actions::default());
        assert_eq!(titles, [(1, "1 variable".to_owned()), (3, "1 variable".to_owned())]);
    }

    #[test]
    fn no_solve_lens_without_solver_and_no_explain_on_unnamed_or_special_rows() {
        let named = doc("min\n obj: x\nst\n c1: x >= 1\nEnd\n");
        assert!(!action_titles(&named, Actions { explain: true, solve: false }).iter().any(|(_, t)| t == "Solve"));

        let other = doc("min\n obj: x + y\nst\n x + y >= 1\nSOS\n s1: S1:: x:1 y:2\nEnd\n");
        let titles = action_titles(&other, Actions { explain: true, solve: false });
        assert!(!titles.iter().any(|(_, t)| t == "Explain" || t == "Solve"), "{titles:?}");

        let general = doc("min\n obj: x\nst\n g1: y = abs(x)\nEnd\n");
        let titles = action_titles(&general, Actions { explain: true, solve: true });
        assert_eq!(titles.iter().filter(|(_, t)| t == "Explain").count(), 0, "{titles:?}");
    }

    #[test]
    fn counts_pluralise_like_replace() {
        for template in ["{} variable", "used in {} constraint"] {
            for n in [0, 1, 2, 9, 10, 11, 100, 12_345, usize::MAX] {
                let replaced = template.replace("{}", &n.to_string());
                assert_eq!(count(n, template), if n == 1 { replaced } else { replaced + "s" });
            }
        }
    }

    #[test]
    fn no_lenses_without_entities_or_variables() {
        assert_eq!(titles("\\ nothing here\n"), []);
    }
}
