//! Code lenses: variable counts on labels, usage counts on variable definitions.
//!
//! Usage lenses are resolved lazily: `textDocument/codeLens` returns only
//! their range and the variable name, and `codeLens/resolve` fills in the
//! title and the reference locations, so a large model does not send every
//! variable's full location list up front.

use serde_json::{Value, json};
use tower_lsp_server::ls_types::{CodeLens, Command};

use crate::document::Document;
use crate::features::navigation;
use crate::index::EntityKind;

/// Client-side command the usage lens runs: arguments are the document URI,
/// the position and the reference locations.
pub const SHOW_REFERENCES: &str = "lp.showReferences";

/// Lenses for `doc`. Usage lenses are unresolved (see [`resolve`]).
#[must_use]
pub fn lenses(doc: &Document) -> Vec<CodeLens> {
    let index = doc.index();

    // Distinct variables per entity, in one pass over the occurrences.
    // `Variable::entities` is deduplicated: an entity's occurrences of one
    // variable are contiguous because entities are disjoint and in order.
    let mut variable_counts = vec![0usize; index.entities.len()];
    let mut lenses = Vec::with_capacity(index.entities.len() + index.variables.len());
    let uri = doc.uri.as_str();
    for variable in &index.variables {
        for entity in variable.entities() {
            variable_counts[entity] += 1;
        }
        lenses.push(CodeLens {
            range: doc.range(variable.definition().range.clone()),
            command: None,
            data: Some(json!({ "uri": uri, "variable": variable.name })),
        });
    }

    for (entity, &variables) in index.entities.iter().zip(&variable_counts) {
        let anchor = entity.name_range.clone().unwrap_or(entity.range.start..entity.range.start);
        lenses.push(CodeLens {
            range: doc.range(anchor),
            command: Some(Command::new(count(variables, "{} variable"), String::new(), None)),
            data: None,
        });
    }

    lenses.sort_by_key(|lens| (lens.range.start.line, lens.range.start.character));
    lenses
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
    debug_assert!(template.contains("{}"));
    let text = template.replace("{}", &n.to_string());
    if n == 1 { text } else { text + "s" }
}

#[cfg(test)]
mod tests {
    use tower_lsp_server::ls_types::Location;

    use super::*;
    use crate::position::Encoding;

    fn resolved(doc: &Document) -> Vec<CodeLens> {
        lenses(doc).into_iter().map(|l| resolve(doc, l).unwrap()).collect()
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
        let lens = lenses(&doc).into_iter().find(|l| l.range.start.line == 5).unwrap();
        assert!(lens.command.is_none(), "usage lens is unresolved");
        assert_eq!(uri(&lens), Ok("file:///t.lp"));

        // Resolved against a newer version where the variable is gone.
        let edited = Document::new(doc.uri.clone(), "min\n obj: y\nEnd\n".to_owned(), 1, Encoding::Utf16);
        assert!(resolve(&edited, lens).unwrap_err().contains("`x`"));
    }

    #[test]
    fn no_lenses_without_entities_or_variables() {
        assert_eq!(titles("\\ nothing here\n"), []);
    }
}
