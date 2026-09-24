//! Document symbols (hierarchical) and workspace symbols (fuzzy).

use std::ops::Range;
use std::sync::Arc;

use tower_lsp_server::ls_types::{DocumentSymbol, OneOf, SymbolKind, WorkspaceSymbol};

use crate::document::Document;
use crate::features::folding;
use crate::index::{Entity, EntityKind};

/// Maximum number of workspace symbols returned.
const MAX_WORKSPACE_SYMBOLS: usize = 500;

/// Characters of entity text used to name an unnamed entity.
const UNNAMED_PREVIEW_CHARS: usize = 40;

/// Sections → objectives / constraints / general constraints / SOS sets.
#[must_use]
pub fn document_symbols(doc: &Document) -> Vec<DocumentSymbol> {
    let index = doc.index();
    let mut entities = index.entities.iter().peekable();
    let mut out = Vec::with_capacity(index.sections.len());
    for section in &index.sections {
        let node = doc.node(section.range.clone(), section.kind);
        debug_assert!(node.is_some(), "indexed sections exist in the tree");
        let start = node.map_or(section.range.start, folding::section_start);
        let range = start..section.range.end;
        let (name, selection) = match &section.header {
            Some(header) => (doc.slice(header.clone()).to_owned(), header.clone()),
            None => ("Objectives".to_owned(), start..start),
        };
        // The objective sense (`Maximize`), when the section was extended over it.
        let detail = doc.slice(start..section.range.start).split_whitespace().next().map(str::to_owned);

        // Entities are in document order and nested in exactly one section.
        let mut children = Vec::new();
        while let Some(entity) = entities.next_if(|e| e.range.start < range.end) {
            debug_assert!(range.start <= entity.range.start && entity.range.end <= range.end, "entity outside its section");
            children.push(entity_symbol(doc, entity));
        }
        out.push(symbol(name, detail, SymbolKind::NAMESPACE, doc, range, selection, children));
    }
    debug_assert!(entities.next().is_none(), "every entity belongs to a section");
    out
}

/// Fuzzy search over constraint, objective, SOS and variable names.
#[must_use]
pub fn workspace_symbols(docs: &[Arc<Document>], query: &str) -> Vec<WorkspaceSymbol> {
    let query: Vec<char> = query.trim().to_lowercase().chars().collect();

    // Score cheaply first; build LSP values only for the survivors.
    let mut candidates: Vec<(u8, usize, Candidate)> = Vec::new();
    for (d, doc) in docs.iter().enumerate() {
        for (e, entity) in doc.index().entities.iter().enumerate() {
            if let Some(score) = entity.name.as_deref().and_then(|name| score(name, &query)) {
                candidates.push((score, d, Candidate::Entity(e)));
            }
        }
        for (v, variable) in doc.index().variables.iter().enumerate() {
            if let Some(score) = score(&variable.name, &query) {
                candidates.push((score, d, Candidate::Variable(v)));
            }
        }
        if query.is_empty() && candidates.len() >= MAX_WORKSPACE_SYMBOLS {
            break;
        }
    }
    // Stable: equal scores keep document order.
    candidates.sort_by_key(|&(score, ..)| std::cmp::Reverse(score));
    candidates.truncate(MAX_WORKSPACE_SYMBOLS);

    candidates
        .into_iter()
        .map(|(_, d, candidate)| {
            let doc = &docs[d];
            let (name, kind, container, range) = match candidate {
                Candidate::Entity(e) => {
                    let entity = &doc.index().entities[e];
                    let name_range = entity.name_range.clone().unwrap_or_else(|| entity.range.clone());
                    (entity.name.clone().unwrap_or_default(), entity_kind(entity.kind), entity.section.label(), name_range)
                }
                Candidate::Variable(v) => {
                    let variable = &doc.index().variables[v];
                    (variable.name.clone(), SymbolKind::VARIABLE, "variable", variable.definition().range.clone())
                }
            };
            WorkspaceSymbol {
                name,
                kind,
                tags: None,
                container_name: Some(container.to_owned()),
                location: OneOf::Left(doc.location(range)),
                data: None,
            }
        })
        .collect()
}

#[derive(Debug, Clone, Copy)]
enum Candidate {
    Entity(usize),
    Variable(usize),
}

/// Symbol kind per entity kind, shared by document and workspace symbols.
const fn entity_kind(kind: EntityKind) -> SymbolKind {
    match kind {
        EntityKind::Objective => SymbolKind::FUNCTION,
        EntityKind::Constraint => SymbolKind::OPERATOR,
        EntityKind::GeneralConstraint => SymbolKind::METHOD,
        EntityKind::Sos => SymbolKind::ARRAY,
    }
}

pub(crate) fn entity_symbol(doc: &Document, entity: &Entity) -> DocumentSymbol {
    let name = entity.name.clone().unwrap_or_else(|| unnamed(doc.slice(entity.range.clone()), entity.kind));
    let selection = entity.name_range.clone().unwrap_or(entity.range.start..entity.range.start);
    symbol(name, Some(entity.kind.label().to_owned()), entity_kind(entity.kind), doc, entity.range.clone(), selection, Vec::new())
}

/// Name for an unnamed entity: its first characters with whitespace collapsed.
fn unnamed(text: &str, kind: EntityKind) -> String {
    let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.is_empty() {
        return format!("(unnamed {})", kind.label());
    }
    match collapsed.char_indices().nth(UNNAMED_PREVIEW_CHARS) {
        Some((cut, _)) => format!("{}…", &collapsed[..cut]),
        None => collapsed,
    }
}

fn symbol(
    name: String,
    detail: Option<String>,
    kind: SymbolKind,
    doc: &Document,
    range: Range<usize>,
    selection: Range<usize>,
    children: Vec<DocumentSymbol>,
) -> DocumentSymbol {
    debug_assert!(range.start <= selection.start && selection.end <= range.end, "selection range must lie inside the range");
    #[allow(deprecated, reason = "`deprecated` is a required field of the LSP type")]
    DocumentSymbol {
        name,
        detail,
        kind,
        tags: None,
        deprecated: None,
        range: doc.range(range),
        selection_range: doc.range(selection),
        children: (!children.is_empty()).then_some(children),
    }
}

/// Case-insensitive fuzzy score of `name` against a lower-cased `query`:
/// 3 = prefix, 2 = contiguous substring, 1 = subsequence, `None` = no match.
/// An empty query matches everything.
fn score(name: &str, query: &[char]) -> Option<u8> {
    if query.is_empty() {
        return Some(0);
    }
    let name: Vec<char> = name.to_lowercase().chars().collect();
    if name.starts_with(query) {
        return Some(3);
    }
    if name.windows(query.len()).any(|w| w == query) {
        return Some(2);
    }
    let mut rest = query.iter().peekable();
    for c in &name {
        rest.next_if_eq(&c);
    }
    rest.peek().is_none().then_some(1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::position::Encoding;

    const TEXT: &str = "Maximize\n obj: x + y\nSubject To\n c1: x + y <= 10\n -3 x + 4 y + 5 z + 6 w + 7 v + 8 u >= 2\nGeneral Constraints\n g1: r = MAX ( x , y )\nBounds\n x <= 4\nSOS\n s1: S1 :: x : 1\n y : 2\nEnd\n";

    fn doc(uri: &str, text: &str) -> Arc<Document> {
        Arc::new(Document::new(uri.parse().unwrap(), text.to_owned(), 0, Encoding::Utf16))
    }

    #[test]
    fn document_symbols_nest_entities_in_sections() {
        let doc = doc("file:///a.lp", TEXT);
        let symbols = document_symbols(&doc);
        let outline: Vec<(String, Option<String>, Vec<String>)> = symbols
            .iter()
            .map(|s| (s.name.clone(), s.detail.clone(), s.children.iter().flatten().map(|c| c.name.clone()).collect()))
            .collect();
        assert_eq!(
            outline,
            [
                ("Objectives".to_owned(), Some("Maximize".to_owned()), vec!["obj".to_owned()]),
                ("Subject To".to_owned(), None, vec!["c1".to_owned(), "-3 x + 4 y + 5 z + 6 w + 7 v + 8 u >= 2".to_owned()]),
                ("General Constraints".to_owned(), None, vec!["g1".to_owned()]),
                ("Bounds".to_owned(), None, vec![]),
                ("SOS".to_owned(), None, vec!["s1".to_owned()]),
            ]
        );
        // The objectives section starts at its sense; the SOS set spans its entries.
        assert_eq!(symbols[0].range.start.line, 0);
        let sos = &symbols[4].children.as_ref().unwrap()[0];
        assert_eq!((sos.kind, sos.range.start.line, sos.range.end.line), (SymbolKind::ARRAY, 10, 11));
        for section in &symbols {
            for child in section.children.iter().flatten() {
                assert!(section.range.start <= child.range.start && child.range.end <= section.range.end);
                assert!(child.range.start <= child.selection_range.start && child.selection_range.end <= child.range.end);
            }
        }
    }

    #[test]
    fn unnamed_entities_are_truncated() {
        assert_eq!(unnamed("a  +\n b", EntityKind::Constraint), "a + b");
        assert_eq!(unnamed(&"x ".repeat(30), EntityKind::Constraint), format!("{}…", "x ".repeat(20)));
        assert_eq!(unnamed("", EntityKind::Objective), "(unnamed objective)");
    }

    #[test]
    fn document_symbols_empty_for_empty_file() {
        assert_eq!(document_symbols(&doc("file:///e.lp", "")), []);
    }

    #[test]
    fn fuzzy_scores_rank_prefix_contiguous_subsequence() {
        let q = |s: &str| s.chars().collect::<Vec<_>>();
        assert_eq!(score("Capacity", &q("cap")), Some(3));
        assert_eq!(score("maxCap", &q("cap")), Some(2));
        assert_eq!(score("c_a_p", &q("cap")), Some(1));
        assert_eq!(score("pac", &q("cap")), None);
        assert_eq!(score("anything", &q("")), Some(0));
    }

    #[test]
    fn workspace_symbols_search_all_documents() {
        let docs = [doc("file:///a.lp", TEXT), doc("file:///b.lp", "min\n cost: xy\nst\n xcap: xy <= 1\nend\n")];
        let names = |query: &str| workspace_symbols(&docs, query).into_iter().map(|s| s.name).collect::<Vec<_>>();
        // Prefix matches first, then substrings, then subsequences; ties in document order.
        assert_eq!(names("x"), ["x", "xcap", "xy"]);
        assert_eq!(names("cp"), ["xcap"]);
        assert_eq!(names("C1"), ["c1"]);
        assert_eq!(names("nomatch"), Vec::<String>::new());
        // 4 + 7 names in the first document, 3 in the second.
        assert_eq!(names("").len(), 14);

        let x = workspace_symbols(&docs, "x").remove(0);
        assert_eq!(x.kind, SymbolKind::VARIABLE);
        let OneOf::Left(location) = x.location else { panic!("expected a location") };
        // Definition = the bound.
        assert_eq!(location.range.start.line, 8);
    }

    #[test]
    fn workspace_symbols_are_capped() {
        let terms: Vec<String> = (0..600).map(|i| format!("v{i}")).collect();
        let docs = [doc("file:///big.lp", &format!("min\n obj: {}\nst\n c: v0 >= 1\nend\n", terms.join(" + ")))];
        assert!(!docs[0].has_syntax_errors());
        assert_eq!(workspace_symbols(&docs, "").len(), MAX_WORKSPACE_SYMBOLS);
        let matches = workspace_symbols(&docs, "v5");
        // The 111 prefix matches (`v5`, `v50`..`v59`, `v500`..`v599`) come
        // first, in document order, then substring and subsequence matches.
        let prefixed = matches.iter().take_while(|s| s.name.starts_with("v5")).count();
        assert_eq!((prefixed, matches[0].name.as_str()), (111, "v5"));
        assert!(matches[prefixed..].iter().all(|s| !s.name.starts_with("v5")));
    }
}
