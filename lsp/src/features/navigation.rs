//! Go to definition / declaration / type definition, references, document
//! highlights and linked editing.

use std::ops::Range;

use tower_lsp_server::ls_types::{DocumentHighlight, DocumentHighlightKind, LinkedEditingRanges, Location, Position};

use crate::document::Document;
use crate::index::{Role, Symbol, Variable};

/// Variable: its `Bounds` entry, else first occurrence. Names: the label.
#[must_use]
pub fn definition(doc: &Document, position: Position) -> Option<Location> {
    match symbol(doc, position)? {
        Symbol::Variable(var, _) => Some(doc.location(doc.index().variables[var].definition().range.clone())),
        Symbol::Entity(entity) => first_label(doc, entity).map(|range| doc.location(range)),
        Symbol::Attribute(_) => None,
    }
}

/// Variable: its type-section entry. Names: the label.
#[must_use]
pub fn declaration(doc: &Document, position: Position) -> Option<Location> {
    match symbol(doc, position)? {
        Symbol::Variable(var, _) => {
            let variable = &doc.index().variables[var];
            let site = variable.declaration().unwrap_or_else(|| variable.definition());
            Some(doc.location(site.range.clone()))
        }
        Symbol::Entity(entity) => first_label(doc, entity).map(|range| doc.location(range)),
        Symbol::Attribute(_) => None,
    }
}

/// Variable: its type-section entry, else its bound.
#[must_use]
pub fn type_definition(doc: &Document, position: Position) -> Option<Location> {
    let Symbol::Variable(var, _) = symbol(doc, position)? else { return None };
    let variable = &doc.index().variables[var];
    let site = variable.declaration().or_else(|| variable.occurrences.iter().find(|o| o.role == Role::Bound))?;
    Some(doc.location(site.range.clone()))
}

/// Every occurrence of the symbol at `position`.
#[must_use]
pub fn references(doc: &Document, position: Position, include_declaration: bool) -> Vec<Location> {
    let Some(symbol) = symbol(doc, position) else { return Vec::new() };
    sites(doc, symbol)
        .into_iter()
        .filter(|(_, declaration)| include_declaration || !declaration)
        .map(|(range, _)| doc.location(range))
        .collect()
}

/// Write for the definition, read for everything else.
#[must_use]
pub fn highlights(doc: &Document, position: Position) -> Vec<DocumentHighlight> {
    let Some(symbol) = symbol(doc, position) else { return Vec::new() };
    let definition = match symbol {
        Symbol::Variable(var, _) => Some(doc.index().variables[var].definition().range.clone()),
        Symbol::Entity(entity) => first_label(doc, entity),
        Symbol::Attribute(_) => None,
    };
    sites(doc, symbol)
        .into_iter()
        .map(|(range, _)| {
            let kind = if Some(&range) == definition.as_ref() { DocumentHighlightKind::WRITE } else { DocumentHighlightKind::READ };
            DocumentHighlight { range: doc.range(range), kind: Some(kind) }
        })
        .collect()
}

/// A name as the upstream identifier rule (and rename) accepts it, as a
/// JavaScript regular expression: typing outside it ends linked editing.
const NAME_PATTERN: &str = r"[A-Za-z_!#$%&(),.;?@{}~'\[\]][A-Za-z0-9_!#$%&(),.;?@{}~'\[\]|]*";

/// Every occurrence of the name under the cursor, edited together.
#[must_use]
pub fn linked_editing(doc: &Document, position: Position) -> Option<LinkedEditingRanges> {
    let ranges: Vec<_> = sites(doc, symbol(doc, position)?).into_iter().map(|(range, _)| doc.range(range)).collect();
    (!ranges.is_empty()).then(|| LinkedEditingRanges { ranges, word_pattern: Some(NAME_PATTERN.to_owned()) })
}

/// Non-declaration occurrences of a variable (shared with the code lens).
pub(crate) fn usages(doc: &Document, variable: &Variable) -> Vec<Location> {
    variable.occurrences.iter().filter(|o| !o.role.is_declaration()).map(|o| doc.location(o.range.clone())).collect()
}

fn symbol(doc: &Document, position: Position) -> Option<Symbol> {
    doc.index().symbol_at(doc.offset(position)).map(|(_, symbol)| symbol)
}

/// Label of the first entity sharing `entity`'s name and namespace.
fn first_label(doc: &Document, entity: usize) -> Option<Range<usize>> {
    let target = &doc.index().entities[entity];
    let name = target.name.as_deref()?;
    doc.index().entities_named(name, target.kind.namespace()).find_map(|(_, e)| e.name_range.clone())
}

/// Every site of `symbol`, flagged when it is a declaration: bound and
/// type-section entries for variables, the first label for entity names.
fn sites(doc: &Document, symbol: Symbol) -> Vec<(Range<usize>, bool)> {
    match symbol {
        Symbol::Variable(var, _) => {
            doc.index().variables[var].occurrences.iter().map(|o| (o.range.clone(), o.role.is_declaration())).collect()
        }
        Symbol::Entity(entity) => {
            let target = &doc.index().entities[entity];
            let Some(name) = target.name.as_deref() else { return Vec::new() };
            doc.index()
                .entities_named(name, target.kind.namespace())
                .filter_map(|(_, e)| e.name_range.clone())
                .enumerate()
                .map(|(i, range)| (range, i == 0))
                .collect()
        }
        Symbol::Attribute(_) => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::position::Encoding;

    const TEXT: &str = "min\n obj: x + y\nst\n c1: x + y >= 1\n c1: x >= 0\nsos\n s1: S1 :: y : 1\nBounds\n x <= 10\nGenerals\n x\nEnd\n";

    fn doc() -> Document {
        Document::new("file:///t.lp".parse().unwrap(), TEXT.to_owned(), 0, Encoding::Utf16)
    }

    /// Position of the `nth` match of `needle`.
    fn at(doc: &Document, needle: &str, nth: usize) -> Position {
        doc.position(TEXT.match_indices(needle).nth(nth).unwrap().0)
    }

    fn texts(doc: &Document, locations: &[Location]) -> Vec<String> {
        locations.iter().map(|l| format!("{}:{}", l.range.start.line, doc.slice(doc.byte_range(l.range)))).collect()
    }

    #[test]
    fn linked_editing_covers_every_occurrence() {
        let d = doc();
        let linked = linked_editing(&d, at(&d, "x", 0)).unwrap();
        assert_eq!(linked.ranges.len(), 5, "objective, two constraints, bound and generals");
        assert!(linked.ranges.iter().all(|r| d.slice(d.byte_range(*r)) == "x"));
        let pattern = linked.word_pattern.unwrap();
        assert!(pattern.starts_with("[A-Za-z_") && pattern.contains(r"\["));
        assert!(linked_editing(&d, at(&d, ">=", 0)).is_none());
    }

    #[test]
    fn variable_definition_declaration_and_type() {
        let doc = doc();
        let x = at(&doc, "x", 0);
        assert_eq!(definition(&doc, x).unwrap().range.start.line, 8);
        assert_eq!(declaration(&doc, x).unwrap().range.start.line, 10);
        assert_eq!(type_definition(&doc, x).unwrap().range.start.line, 10);
        // `y` has no bound or type entry.
        let y = at(&doc, "y", 0);
        assert_eq!(definition(&doc, y).unwrap().range.start.line, 1);
        assert_eq!(declaration(&doc, y).unwrap().range.start.line, 1);
        assert_eq!(type_definition(&doc, y), None);
    }

    #[test]
    fn labels_go_to_first_definition() {
        let doc = doc();
        let second = at(&doc, "c1", 1);
        assert_eq!(definition(&doc, second).unwrap().range.start.line, 3);
        assert_eq!(declaration(&doc, second).unwrap().range.start.line, 3);
        assert_eq!(type_definition(&doc, second), None);
        assert_eq!(texts(&doc, &references(&doc, second, true)), ["3:c1", "4:c1"]);
        assert_eq!(texts(&doc, &references(&doc, second, false)), ["4:c1"]);
    }

    #[test]
    fn references_and_highlights() {
        let doc = doc();
        let x = at(&doc, "x", 1);
        assert_eq!(texts(&doc, &references(&doc, x, true)), ["1:x", "3:x", "4:x", "8:x", "10:x"]);
        assert_eq!(texts(&doc, &references(&doc, x, false)), ["1:x", "3:x", "4:x"]);
        let kinds: Vec<_> = highlights(&doc, x).into_iter().map(|h| (h.range.start.line, h.kind)).collect();
        let read = Some(DocumentHighlightKind::READ);
        assert_eq!(kinds, [(1, read), (3, read), (4, read), (8, Some(DocumentHighlightKind::WRITE)), (10, read)]);
    }

    #[test]
    fn nothing_on_keywords_or_whitespace() {
        let doc = doc();
        let keyword = at(&doc, "Bounds", 0);
        assert_eq!(definition(&doc, keyword), None);
        assert_eq!(references(&doc, keyword, true), []);
        assert_eq!(highlights(&doc, keyword), []);
        assert_eq!(definition(&doc, Position::new(99, 0)), None);
    }
}
