//! Call hierarchy over the model: a variable's incoming calls are the
//! constraints and objectives that use it; an entity's outgoing calls are the
//! variables it uses.

use std::collections::BTreeMap;

use serde_json::{Value, json};
use tower_lsp_server::ls_types::{CallHierarchyIncomingCall, CallHierarchyItem, CallHierarchyOutgoingCall, Position, SymbolKind};

use super::symbols::entity_symbol;
use crate::document::Document;
use crate::index::{Symbol, Variable};

/// The item under the cursor: a variable, or a named entity.
#[must_use]
pub fn prepare(doc: &Document, position: Position) -> Option<Vec<CallHierarchyItem>> {
    let item = match doc.index().symbol_at(doc.offset(position))?.1 {
        Symbol::Variable(var, _) => variable_item(doc, &doc.index().variables[var]),
        Symbol::Entity(entity) => entity_item(doc, entity),
        Symbol::Attribute(_) => return None,
    };
    Some(vec![item])
}

/// Entities using the variable `item`, with the ranges where they use it.
#[must_use]
pub fn incoming(doc: &Document, item: &CallHierarchyItem) -> Vec<CallHierarchyIncomingCall> {
    let Some(variable) = item.data.as_ref().and_then(|d| d.get("variable")).and_then(Value::as_str) else { return Vec::new() };
    let Some(variable) = doc.index().variable(variable) else { return Vec::new() };
    let mut by_entity: BTreeMap<usize, Vec<_>> = BTreeMap::new();
    for occurrence in &variable.occurrences {
        if let Some(entity) = occurrence.entity {
            by_entity.entry(entity).or_default().push(doc.range(occurrence.range.clone()));
        }
    }
    by_entity.into_iter().map(|(entity, from_ranges)| CallHierarchyIncomingCall { from: entity_item(doc, entity), from_ranges }).collect()
}

/// Variables used by the entity `item`, with the ranges where it uses them.
#[must_use]
pub fn outgoing(doc: &Document, item: &CallHierarchyItem) -> Vec<CallHierarchyOutgoingCall> {
    let Some(start) = item.data.as_ref().and_then(|d| d.get("entity")).and_then(Value::as_u64) else { return Vec::new() };
    let Ok(start) = usize::try_from(start) else { return Vec::new() };
    let index = doc.index();
    // The item may be stale: only answer for an entity still starting there.
    let Some(entity) = index.entity_at(start).filter(|&e| index.entities[e].range.start == start) else { return Vec::new() };
    index
        .variables
        .iter()
        .filter_map(|variable| {
            let from_ranges: Vec<_> =
                variable.occurrences.iter().filter(|o| o.entity == Some(entity)).map(|o| doc.range(o.range.clone())).collect();
            (!from_ranges.is_empty()).then(|| CallHierarchyOutgoingCall { to: variable_item(doc, variable), from_ranges })
        })
        .collect()
}

fn variable_item(doc: &Document, variable: &Variable) -> CallHierarchyItem {
    let range = doc.range(variable.definition().range.clone());
    CallHierarchyItem {
        name: variable.name.clone(),
        kind: SymbolKind::VARIABLE,
        tags: None,
        detail: Some("variable".to_owned()),
        uri: doc.uri.clone(),
        range,
        selection_range: range,
        data: Some(json!({ "variable": variable.name })),
    }
}

fn entity_item(doc: &Document, entity: usize) -> CallHierarchyItem {
    let target = &doc.index().entities[entity];
    let symbol = entity_symbol(doc, target);
    CallHierarchyItem {
        name: symbol.name,
        kind: symbol.kind,
        tags: None,
        detail: symbol.detail,
        uri: doc.uri.clone(),
        range: symbol.range,
        selection_range: symbol.selection_range,
        data: Some(json!({ "entity": target.range.start })),
    }
}

#[cfg(test)]
mod tests {
    use tower_lsp_server::ls_types::Uri;

    use super::*;
    use crate::position::Encoding;

    const MODEL: &str = "min\n obj: 2 x + y\nst\n c1: x + y >= 1\n c2: x <= 4\nend\n";

    fn doc() -> Document {
        Document::new("file:///h.lp".parse::<Uri>().unwrap(), MODEL.to_owned(), 1, Encoding::Utf16)
    }

    fn at(doc: &Document, needle: &str) -> CallHierarchyItem {
        let offset = MODEL.find(needle).unwrap();
        prepare(doc, doc.position(offset)).unwrap().remove(0)
    }

    #[test]
    fn variable_is_called_by_the_entities_using_it() {
        let d = doc();
        let x = at(&d, "x +");
        assert_eq!(x.name, "x");
        let callers: Vec<String> = incoming(&d, &x).into_iter().map(|c| c.from.name).collect();
        assert_eq!(callers, ["obj", "c1", "c2"]);
        assert_eq!(outgoing(&d, &x), [] as [tower_lsp_server::ls_types::CallHierarchyOutgoingCall; 0]);
    }

    #[test]
    fn entity_calls_its_variables() {
        let d = doc();
        let c1 = at(&d, "c1");
        let callees: Vec<(String, usize)> = outgoing(&d, &c1).into_iter().map(|c| (c.to.name, c.from_ranges.len())).collect();
        assert_eq!(callees, [("x".to_owned(), 1), ("y".to_owned(), 1)]);
        assert_eq!(incoming(&d, &c1), [] as [tower_lsp_server::ls_types::CallHierarchyIncomingCall; 0]);
    }
}
