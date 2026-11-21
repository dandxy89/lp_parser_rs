//! Document symbols (hierarchical) and workspace symbols (fuzzy).

use std::ops::Range;
use std::sync::Arc;

use ls_types::{DocumentSymbol, OneOf, SymbolKind, WorkspaceSymbol};

use crate::document::Document;
use crate::features::folding;
use crate::index::{Entity, EntityKind, Role};
use crate::syntax::kind;

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
            let mut symbol = entity_symbol(doc, entity);
            if entity.kind == EntityKind::Constraint
                && let Some(detail) = constraint_detail(doc, entity)
            {
                symbol.detail = Some(detail);
            }
            children.push(symbol);
        }
        // Bounds and type sections list the variables they declare.
        if let Some(role) = declaration_role(section.kind) {
            children.extend(declared_variables(doc, role, &range));
        }
        out.push(symbol(name, detail, SymbolKind::NAMESPACE, doc, range, selection, children));
    }
    debug_assert!(entities.peek().is_none(), "every entity belongs to a section");
    out
}

/// The variable-declaration role of a section kind, for sections that hold
/// variables rather than entities.
fn declaration_role(section_kind: &str) -> Option<Role> {
    match section_kind {
        kind::BOUNDS_SECTION => Some(Role::Bound),
        kind::GENERALS_SECTION => Some(Role::Generals),
        kind::INTEGERS_SECTION => Some(Role::Integers),
        kind::BINARIES_SECTION => Some(Role::Binaries),
        kind::SEMI_CONTINUOUS_SECTION => Some(Role::SemiContinuous),
        _ => None,
    }
}

/// One `VARIABLE` symbol per declaration of a variable inside `section`, in
/// document order.
fn declared_variables(doc: &Document, role: Role, section: &Range<usize>) -> Vec<DocumentSymbol> {
    let mut found: Vec<(&str, Range<usize>)> = doc
        .index()
        .variables
        .iter()
        .flat_map(|variable| {
            variable
                .occurrences
                .iter()
                .filter(move |o| o.role == role && section.start <= o.range.start && o.range.end <= section.end)
                .map(move |o| (variable.name.as_str(), o.range.clone()))
        })
        .collect();
    found.sort_by_key(|(_, range)| range.start);
    found
        .into_iter()
        .map(|(name, range)| symbol(name.to_owned(), None, SymbolKind::VARIABLE, doc, range.clone(), range, Vec::new()))
        .collect()
}

/// Operator and right-hand side of a constraint (`<= 10`): the text from its
/// last comparison operator to its end, whitespace collapsed.
fn constraint_detail(doc: &Document, entity: &Entity) -> Option<String> {
    let node = doc.node(entity.range.clone(), entity.node_kind)?;
    let mut cursor = node.walk();
    let operator = node.children(&mut cursor).filter(|child| child.kind() == kind::COMPARISON_OPERATOR).last()?;
    let text = doc.slice(operator.start_byte()..node.end_byte());
    Some(text.split_whitespace().collect::<Vec<_>>().join(" "))
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
    #[expect(deprecated, reason = "`deprecated` is a required field of the LSP type")]
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
                ("Bounds".to_owned(), None, vec!["x".to_owned()]),
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

    fn outline(text: &str) -> Vec<DocumentSymbol> {
        document_symbols(&doc("file:///o.lp", text))
    }

    #[test]
    fn variables_nest_under_their_declaring_sections() {
        let text = "Minimize\n obj: x + y + z + b + s + n\nSubject To\n c1: x + y >= 1\nBounds\n 0 <= x <= 4\n y <= 9\nGenerals\n n\nIntegers\n y\nBinaries\n b\nSemi-Continuous\n s\nEnd\n";
        let symbols = outline(text);
        let children = |name: &str| -> Vec<(String, SymbolKind)> {
            symbols
                .iter()
                .find(|s| s.name.eq_ignore_ascii_case(name))
                .unwrap_or_else(|| panic!("no section {name}"))
                .children
                .iter()
                .flatten()
                .map(|c| (c.name.clone(), c.kind))
                .collect()
        };
        let var = |n: &str| (n.to_owned(), SymbolKind::VARIABLE);
        assert_eq!(children("Bounds"), [var("x"), var("y")]);
        assert_eq!(children("Generals"), [var("n")]);
        assert_eq!(children("Integers"), [var("y")]);
        assert_eq!(children("Binaries"), [var("b")]);
        assert_eq!(children("Semi-Continuous"), [var("s")]);
        // Variables stay inside their section and the selection inside the variable.
        for section in &symbols {
            for child in section.children.iter().flatten() {
                assert!(section.range.start <= child.range.start && child.range.end <= section.range.end);
                assert!(child.range.start <= child.selection_range.start && child.selection_range.end <= child.range.end);
            }
        }
        // Sections that hold entities gain no variable children.
        let subject_to = symbols.iter().find(|s| s.name == "Subject To").unwrap();
        assert!(subject_to.children.iter().flatten().all(|c| c.kind != SymbolKind::VARIABLE));
    }

    #[test]
    fn repeated_bounds_list_each_declaration() {
        let symbols = outline("Minimize\n obj: x\nSubject To\n c1: x >= 0\nBounds\n x >= 1\n x <= 4\nEnd\n");
        let bounds = symbols.iter().find(|s| s.name == "Bounds").unwrap();
        assert_eq!(bounds.children.iter().flatten().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["x", "x"]);
    }

    #[test]
    fn constraint_detail_is_operator_and_rhs() {
        let text = "Minimize\n obj: x\nSubject To\n c1: x + y <= 10\n c2: x >= -2.5\n r1: 1 <= x - y <= 6\n e: x + y = 3\n ind: b = 1 -> x + y <= 5\nBinaries\n b\nEnd\n";
        let symbols = outline(text);
        let section = symbols.iter().find(|s| s.name == "Subject To").unwrap();
        let details: Vec<(&str, Option<&str>)> =
            section.children.iter().flatten().map(|c| (c.name.as_str(), c.detail.as_deref())).collect();
        assert_eq!(
            details,
            [("c1", Some("<= 10")), ("c2", Some(">= -2.5")), ("r1", Some("<= 6")), ("e", Some("= 3")), ("ind", Some("<= 5"))]
        );
        // Objectives keep their kind label.
        assert_eq!(symbols[0].children.as_ref().unwrap()[0].detail.as_deref(), Some("objective"));
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

    /// Replace `${n:default}`, `${n|a,b|}` and bare `$n` tab stops by their defaults.
    fn fill_placeholders(body: &str) -> String {
        let mut out = String::new();
        let mut chars = body.chars().peekable();
        while let Some(c) = chars.next() {
            if c != '$' {
                out.push(c);
                continue;
            }
            match chars.peek() {
                Some('{') => {
                    chars.next();
                    let mut inner = String::new();
                    for next in chars.by_ref() {
                        if next == '}' {
                            break;
                        }
                        inner.push(next);
                    }
                    let rest = inner.trim_start_matches(|d: char| d.is_ascii_digit());
                    if let Some(default) = rest.strip_prefix(':') {
                        out.push_str(default);
                    } else if let Some(choices) = rest.strip_prefix('|') {
                        out.push_str(choices.trim_end_matches('|').split(',').next().expect("a choice"));
                    }
                }
                Some(d) if d.is_ascii_digit() => while chars.next_if(char::is_ascii_digit).is_some() {},
                _ => out.push('$'),
            }
        }
        out
    }

    /// Wrap a filled snippet in the smallest model that can hold it.
    fn wrap_snippet(body: &str) -> String {
        let model = "Minimize\n obj: x + y\nSubject To\n c1: x + y >= 1\n";
        let first = body.lines().next().unwrap_or_default();
        if first.starts_with("Minimize") || first.starts_with("Maximize") {
            let mut text = body.to_owned();
            if !body.contains("Subject To") {
                text.push_str("Subject To\n c1: x + y >= 1\n");
            }
            if !body.contains("End") {
                text.push_str("End\n");
            }
            text
        } else if first.starts_with(' ') {
            format!("Minimize\n obj: x + y\nSubject To\n{body}End\n")
        } else {
            format!("{model}{body}End\n")
        }
    }

    #[test]
    fn filled_snippets_parse_without_errors() {
        let snippets: serde_json::Value =
            serde_json::from_str(include_str!("../../editors/vscode/snippets/lp.json")).expect("snippets are JSON");
        let snippets = snippets.as_object().expect("an object of snippets");
        assert!(snippets.len() >= 12, "only {} snippets", snippets.len());
        for (name, snippet) in snippets {
            let lines: Vec<&str> =
                snippet["body"].as_array().expect("body is an array").iter().map(|l| l.as_str().expect("line")).collect();
            let text = wrap_snippet(&fill_placeholders(&lines.join("\n")));
            assert!(!text.contains('$'), "{name}: unexpanded placeholder in {text}");
            if let Err(error) = lp_parser_rs::problem::LpProblem::parse(&text) {
                panic!("{name}: LpProblem::parse failed: {error}\n{text}");
            }
            let parsed = doc("file:///snippet.lp", &text);
            assert!(!parsed.has_syntax_errors(), "{name}: tree-sitter ERROR/MISSING node in\n{text}");
        }
    }
}
