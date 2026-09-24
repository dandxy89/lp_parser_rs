//! Tree-sitter plumbing: parsing, node-kind names and small node helpers.

use std::cell::RefCell;
use std::ops::Range;

use tree_sitter::{Language, Node, Parser, Tree};

/// Node kind names from the tree-sitter-lp grammar (`src/node-types.json`).
pub mod kind {
    pub const SOURCE_FILE: &str = "source_file";
    pub const SENSE: &str = "sense";
    pub const MULTI_OBJECTIVES_KEYWORD: &str = "multi_objectives_keyword";
    pub const OBJECTIVES_SECTION: &str = "objectives_section";
    pub const NAMED_OBJECTIVE: &str = "named_objective";
    pub const OBJECTIVE_NAME: &str = "objective_name";
    pub const OBJECTIVE_ATTRIBUTE: &str = "objective_attribute";
    pub const ATTRIBUTE_NAME: &str = "attribute_name";
    pub const CONSTRAINTS_SECTION: &str = "constraints_section";
    pub const SUBJECT_TO_KEYWORD: &str = "subject_to_keyword";
    pub const LAZY_CONSTRAINTS_SECTION: &str = "lazy_constraints_section";
    pub const LAZY_CONSTRAINTS_KEYWORD: &str = "lazy_constraints_keyword";
    pub const USER_CUTS_SECTION: &str = "user_cuts_section";
    pub const USER_CUTS_KEYWORD: &str = "user_cuts_keyword";
    pub const GENERAL_CONSTRAINTS_SECTION: &str = "general_constraints_section";
    pub const GENERAL_CONSTRAINTS_KEYWORD: &str = "general_constraints_keyword";
    pub const GENERAL_CONSTRAINT: &str = "general_constraint";
    pub const FUNCTION_NAME: &str = "function_name";
    pub const CONSTRAINT: &str = "constraint";
    pub const CONSTRAINT_NAME: &str = "constraint_name";
    pub const INDICATOR: &str = "indicator";
    pub const LINEAR_EXPRESSION: &str = "linear_expression";
    pub const TERM: &str = "term";
    pub const QUADRATIC_BLOCK: &str = "quadratic_block";
    pub const QUADRATIC_TERM: &str = "quadratic_term";
    pub const COMPARISON_OPERATOR: &str = "comparison_operator";
    pub const BOUNDS_SECTION: &str = "bounds_section";
    pub const BOUNDS_KEYWORD: &str = "bounds_keyword";
    pub const BOUND_DECLARATION: &str = "bound_declaration";
    pub const FREE_KEYWORD: &str = "free_keyword";
    pub const GENERALS_SECTION: &str = "generals_section";
    pub const GENERALS_KEYWORD: &str = "generals_keyword";
    pub const INTEGERS_SECTION: &str = "integers_section";
    pub const INTEGERS_KEYWORD: &str = "integers_keyword";
    pub const BINARIES_SECTION: &str = "binaries_section";
    pub const BINARIES_KEYWORD: &str = "binaries_keyword";
    pub const SEMI_CONTINUOUS_SECTION: &str = "semi_continuous_section";
    pub const SEMI_CONTINUOUS_KEYWORD: &str = "semi_continuous_keyword";
    pub const SOS_SECTION: &str = "sos_section";
    pub const SOS_KEYWORD: &str = "sos_keyword";
    pub const SOS_CONSTRAINT_HEADER: &str = "sos_constraint_header";
    pub const SOS_NAME: &str = "sos_name";
    pub const SOS_TYPE: &str = "sos_type";
    pub const SOS_ENTRY: &str = "sos_entry";
    pub const END_MARKER: &str = "end_marker";
    pub const IDENTIFIER: &str = "identifier";
    pub const NUMBER: &str = "number";
    pub const INFINITY: &str = "infinity";
    pub const LINE_COMMENT: &str = "line_comment";
    pub const BLOCK_COMMENT: &str = "block_comment";
    pub const ERROR: &str = "ERROR";
}

/// Section kinds after the objective, in canonical order.
pub const SECTION_KINDS: &[&str] = &[
    kind::OBJECTIVES_SECTION,
    kind::CONSTRAINTS_SECTION,
    kind::LAZY_CONSTRAINTS_SECTION,
    kind::USER_CUTS_SECTION,
    kind::GENERAL_CONSTRAINTS_SECTION,
    kind::BOUNDS_SECTION,
    kind::GENERALS_SECTION,
    kind::INTEGERS_SECTION,
    kind::BINARIES_SECTION,
    kind::SEMI_CONTINUOUS_SECTION,
    kind::SOS_SECTION,
];

/// The LP tree-sitter language.
#[must_use]
pub fn language() -> Language {
    tree_sitter_lp::LANGUAGE.into()
}

thread_local! {
    static PARSER: RefCell<Parser> = RefCell::new(new_parser());
}

fn new_parser() -> Parser {
    let mut parser = Parser::new();
    // The grammar is compiled into this binary with a matching ABI; a failure
    // here is a build defect, not a runtime condition.
    parser.set_language(&language()).expect("tree-sitter-lp ABI must match the tree-sitter runtime");
    parser
}

/// Parse `text`, reusing `old` (already `edit`ed) for incremental parsing.
///
/// # Panics
/// Never in practice: parsing only fails without a language, timeout or
/// cancellation flag, and none is set.
#[must_use]
pub fn parse(text: &str, old: Option<&Tree>) -> Tree {
    PARSER.with(|parser| {
        // `parse` only returns `None` with no language, a timeout or a
        // cancellation flag; none are set.
        parser.borrow_mut().parse(text, old).expect("parser has a language and no timeout")
    })
}

/// `node.kind()` as a `&'static str`. tree-sitter 0.27 ties kind names to the
/// tree's lifetime; the names come from the static grammar, so they are copied
/// once into a table that lives for the whole process.
#[must_use]
pub fn static_kind(node: Node<'_>) -> &'static str {
    static NAMES: std::sync::OnceLock<Vec<&'static str>> = std::sync::OnceLock::new();
    let names = NAMES.get_or_init(|| {
        let language = language();
        (0..u16::try_from(language.node_kind_count()).unwrap_or(u16::MAX))
            .map(|id| &*Box::leak(language.node_kind_for_id(id).unwrap_or("").to_owned().into_boxed_str()))
            .collect()
    });
    if node.is_error() {
        return kind::ERROR;
    }
    names.get(usize::from(node.kind_id())).copied().unwrap_or("")
}

/// Source text of `node`.
#[must_use]
pub fn text<'a>(node: Node<'_>, source: &'a str) -> &'a str {
    &source[node.byte_range()]
}

/// Whether `node` is one of the section nodes.
#[must_use]
pub fn is_section(node: Node<'_>) -> bool {
    SECTION_KINDS.contains(&node.kind())
}

/// Smallest named node spanning `range`.
#[must_use]
pub fn named_node_at(tree: &Tree, range: Range<usize>) -> Option<Node<'_>> {
    tree.root_node().named_descendant_for_byte_range(range.start, range.end)
}

/// Leaf token touching `offset`: the token containing it, else the one ending
/// right before it (so a cursor just after a name still finds the name).
#[must_use]
pub fn token_at(tree: &Tree, offset: usize) -> Option<Node<'_>> {
    let root = tree.root_node();
    let containing = root
        .descendant_for_byte_range(offset, offset)
        .filter(|n| n.child_count() == 0 && n.start_byte() <= offset && offset < n.end_byte());
    if let Some(node) = containing.filter(Node::is_named) {
        return Some(node);
    }
    let before =
        offset.checked_sub(1).and_then(|o| root.descendant_for_byte_range(o, o)).filter(|n| n.child_count() == 0 && n.end_byte() == offset);
    before.filter(Node::is_named).or(containing).or(before)
}

/// Nearest ancestor (inclusive) of `node` with kind `kind`.
#[must_use]
pub fn ancestor<'t>(node: Node<'t>, kind: &str) -> Option<Node<'t>> {
    let mut current = Some(node);
    while let Some(n) = current {
        if n.kind() == kind {
            return Some(n);
        }
        current = n.parent();
    }
    None
}

/// Nearest section ancestor (inclusive) of `node`.
#[must_use]
pub fn section_of(node: Node<'_>) -> Option<Node<'_>> {
    let mut current = Some(node);
    while let Some(n) = current {
        if is_section(n) {
            return Some(n);
        }
        current = n.parent();
    }
    None
}

/// Parse a (possibly signed) numeric literal as the upstream lexer does.
#[must_use]
pub fn parse_number(text: &str) -> Option<f64> {
    let trimmed: String = text.chars().filter(|c| !c.is_whitespace()).collect();
    let (sign, body) = match trimmed.as_bytes().first() {
        Some(b'-') => (-1.0, &trimmed[1..]),
        Some(b'+') => (1.0, &trimmed[1..]),
        _ => (1.0, trimmed.as_str()),
    };
    if body.eq_ignore_ascii_case("inf") || body.eq_ignore_ascii_case("infinity") {
        return Some(sign * f64::INFINITY);
    }
    body.parse::<f64>().ok().map(|v| sign * v)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_numbers_and_infinity() {
        assert_eq!(parse_number("- 3.5"), Some(-3.5));
        assert_eq!(parse_number("+INF"), Some(f64::INFINITY));
        assert_eq!(parse_number("1e3"), Some(1000.0));
        assert_eq!(parse_number("x"), None);
    }

    #[test]
    fn token_at_finds_name_before_cursor() {
        let text = "min\n obj: x\nst\n c1: x >= 1\nend\n";
        let tree = parse(text, None);
        let offset = text.find("c1").unwrap() + 2;
        let token = token_at(&tree, offset).unwrap();
        assert_eq!(super::text(token, text), "c1");
    }
}
