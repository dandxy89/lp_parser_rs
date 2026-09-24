//! Tree-sitter plumbing: parsing, node-kind names and small node helpers.

use std::cell::RefCell;

use lp_parser_rs::VariableBounds;
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
    std::iter::successors(Some(node), Node::parent).find(|n| n.kind() == kind)
}

/// Nearest section ancestor (inclusive) of `node`.
#[must_use]
pub fn section_of(node: Node<'_>) -> Option<Node<'_>> {
    std::iter::successors(Some(node), Node::parent).find(|n| is_section(*n))
}

/// One capture of a query pattern, as read by [`query_captures`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueryCapture {
    /// Capture name without the `@` (`keyword.type`).
    pub name: String,
    /// Node kind captured (`bounds_keyword`, or `+` for an anonymous node).
    pub kind: String,
    /// Whether `kind` is a named node (`(kind)`) rather than a literal (`"kind"`).
    pub named: bool,
    /// Kind of the enclosing node in `(parent (kind) @name)` patterns.
    pub parent: Option<String>,
}

/// Every capture in `source`, a query in the flat shape the grammar's query
/// files use: `(kind) @name`, `"literal" @name`, alternations `[ ... ] @name`
/// and one level of nesting `(parent (kind) @name)`. The source is compiled
/// first, so it is valid; anything else (predicates, deeper nesting,
/// captures on nodes with children) is an error, so a query the server cannot
/// mirror is noticed rather than silently misread.
///
/// # Errors
/// When the query does not compile or has an unsupported shape.
pub fn query_captures(source: &str) -> Result<Vec<QueryCapture>, String> {
    let query = tree_sitter::Query::new(&language(), source).map_err(|e| format!("query does not compile: {e}"))?;
    let mut out = Vec::new();
    for pattern in 0..query.pattern_count() {
        let text = &source[query.start_byte_for_pattern(pattern)..query.end_byte_for_pattern(pattern)];
        let tokens = query_tokens(text)?;
        let mut rest = tokens.as_slice();
        read_item(&mut rest, None, &mut out).map_err(|e| format!("unsupported query pattern `{}`: {e}", text.trim()))?;
        if !rest.is_empty() {
            return Err(format!("unsupported query pattern `{}`: trailing tokens", text.trim()));
        }
    }
    Ok(out)
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum QueryToken {
    Open,
    Close,
    OpenAlt,
    CloseAlt,
    Literal(String),
    Name(String),
    Capture(String),
}

fn query_tokens(text: &str) -> Result<Vec<QueryToken>, String> {
    let mut out = Vec::new();
    let mut chars = text.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        match c {
            c if c.is_whitespace() => {}
            ';' => {
                // Comment to end of line.
                while chars.next_if(|&(_, c)| c != '\n').is_some() {}
            }
            '(' => out.push(QueryToken::Open),
            ')' => out.push(QueryToken::Close),
            '[' => out.push(QueryToken::OpenAlt),
            ']' => out.push(QueryToken::CloseAlt),
            '"' => {
                let mut literal = String::new();
                loop {
                    match chars.next() {
                        Some((_, '\\')) => literal.extend(chars.next().map(|(_, c)| c)),
                        Some((_, '"')) => break,
                        Some((_, c)) => literal.push(c),
                        None => return Err("unterminated string".to_owned()),
                    }
                }
                out.push(QueryToken::Literal(literal));
            }
            _ => {
                let mut end = i + c.len_utf8();
                while let Some(&(j, c)) = chars.peek() {
                    if c.is_whitespace() || "()[]\";".contains(c) {
                        break;
                    }
                    end = j + c.len_utf8();
                    chars.next();
                }
                let word = &text[i..end];
                match word.strip_prefix('@') {
                    Some(capture) => out.push(QueryToken::Capture(capture.to_owned())),
                    // `field:` prefixes do not change what is captured.
                    None if word.ends_with(':') => {}
                    None if word.starts_with('#') || word.starts_with('!') => return Err(format!("unsupported `{word}`")),
                    None => out.push(QueryToken::Name(word.to_owned())),
                }
            }
        }
    }
    Ok(out)
}

/// Read one item (`(kind ...)`, `"literal"` or `[ ... ]`) and its captures.
fn read_item(tokens: &mut &[QueryToken], parent: Option<&str>, out: &mut Vec<QueryCapture>) -> Result<(), String> {
    // Leaves read before their captures: `(kind, named)`.
    let mut leaves: Vec<(String, bool)> = Vec::new();
    match tokens.split_first() {
        Some((QueryToken::Open, rest)) => {
            let Some((QueryToken::Name(kind), rest)) = rest.split_first() else { return Err("expected a node kind".to_owned()) };
            *tokens = rest;
            if tokens.first() == Some(&QueryToken::Close) {
                leaves.push((kind.clone(), true));
            } else {
                if parent.is_some() {
                    return Err("nesting deeper than one level".to_owned());
                }
                while tokens.first().is_some_and(|t| *t != QueryToken::Close) {
                    read_item(tokens, Some(kind), out)?;
                }
                if tokens.get(1).is_some_and(|t| matches!(t, QueryToken::Capture(_))) {
                    return Err("capture on a node with children".to_owned());
                }
            }
            let Some((QueryToken::Close, rest)) = tokens.split_first() else { return Err("expected `)`".to_owned()) };
            *tokens = rest;
        }
        Some((QueryToken::Literal(literal), rest)) => {
            leaves.push((literal.clone(), false));
            *tokens = rest;
        }
        Some((QueryToken::OpenAlt, rest)) => {
            *tokens = rest;
            while let Some((token, rest)) = tokens.split_first() {
                match token {
                    QueryToken::CloseAlt => break,
                    QueryToken::Literal(literal) => leaves.push((literal.clone(), false)),
                    QueryToken::Open => match rest {
                        [QueryToken::Name(kind), QueryToken::Close, ..] => {
                            leaves.push((kind.clone(), true));
                            *tokens = &rest[1..];
                        }
                        _ => return Err("alternatives must be single nodes".to_owned()),
                    },
                    _ => return Err("unexpected token in an alternation".to_owned()),
                }
                *tokens = &tokens[1..];
            }
            let Some((QueryToken::CloseAlt, rest)) = tokens.split_first() else { return Err("expected `]`".to_owned()) };
            *tokens = rest;
        }
        _ => return Err("expected a node, literal or alternation".to_owned()),
    }
    while let Some((QueryToken::Capture(name), rest)) = tokens.split_first() {
        *tokens = rest;
        for (kind, named) in &leaves {
            out.push(QueryCapture { name: name.clone(), kind: kind.clone(), named: *named, parent: parent.map(str::to_owned) });
        }
    }
    Ok(())
}

/// Grammar symbol ids whose kind and namedness match `kind`/`named` (aliases
/// can give one name several ids).
#[must_use]
pub fn kind_ids(kind: &str, named: bool) -> Vec<u16> {
    let language = language();
    let count = u16::try_from(language.node_kind_count()).unwrap_or(u16::MAX);
    (0..count).filter(|&id| language.node_kind_is_named(id) == named && language.node_kind_for_id(id) == Some(kind)).collect()
}

/// Parse a (possibly signed) numeric literal as the upstream lexer does:
/// whitespace between sign and digits is allowed, `inf`/`infinity` in any case.
#[must_use]
pub fn parse_number(text: &str) -> Option<f64> {
    // `f64::from_str` handles signs and `inf`/`infinity`; it also accepts
    // `nan`, which the lexer does not.
    let value: f64 =
        if text.contains(char::is_whitespace) { text.split_whitespace().collect::<String>().parse().ok()? } else { text.parse().ok()? };
    (!value.is_nan()).then_some(value)
}

/// Comparison operator with aliases canonicalised (`=<` → `<=`, `=>` → `>=`).
#[must_use]
pub fn canonical_operator(op: &str) -> &'static str {
    match op {
        "<=" | "=<" => "<=",
        ">=" | "=>" => ">=",
        "<" => "<",
        ">" => ">",
        _ => "=",
    }
}

/// The operator with its sides swapped (`>=` becomes `<=`); aliases accepted.
#[must_use]
pub fn flip_operator(op: &str) -> &'static str {
    match canonical_operator(op) {
        "<=" => ">=",
        ">=" => "<=",
        "<" => ">",
        ">" => "<",
        _ => "=",
    }
}

/// Bound magnitude from which upstream treats a value as infinite.
pub const INFINITE_BOUND: f64 = 1e30;

/// Bounds set by one `bound_declaration`, as the upstream grammar reads it;
/// `None` for shapes upstream rejects.
#[must_use]
pub fn declared_bounds(node: Node<'_>, text: &str) -> Option<VariableBounds> {
    #[derive(Clone, Copy)]
    enum Item {
        Variable,
        Free,
        /// `Some(true)` for `<=`-like, `Some(false)` for `>=`-like, `None` for `=`.
        Operator(Option<bool>),
        Value(f64),
    }
    debug_assert_eq!(node.kind(), kind::BOUND_DECLARATION);
    let mut items = Vec::new();
    let mut negative = false;
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        let source = self::text(child, text);
        match child.kind() {
            "-" => negative = true,
            kind::IDENTIFIER => items.push(Item::Variable),
            kind::FREE_KEYWORD => items.push(Item::Free),
            kind::COMPARISON_OPERATOR => items.push(Item::Operator(match canonical_operator(source) {
                "<=" | "<" => Some(true),
                ">=" | ">" => Some(false),
                _ => None,
            })),
            kind::NUMBER | kind::INFINITY => {
                let value = parse_number(source)?;
                let value = if negative { -value } else { value };
                negative = false;
                items.push(Item::Value(if value >= INFINITE_BOUND {
                    f64::INFINITY
                } else if value <= -INFINITE_BOUND {
                    f64::NEG_INFINITY
                } else {
                    value
                }));
            }
            _ => {}
        }
    }
    let bounds = match items.as_slice() {
        [Item::Variable, Item::Free] => VariableBounds::free(),
        [Item::Variable, Item::Operator(le), Item::Value(v)] => match le {
            Some(true) => VariableBounds::upper(*v),
            Some(false) => VariableBounds::lower(*v),
            None => VariableBounds::range(*v, *v),
        },
        [Item::Value(v), Item::Operator(le), Item::Variable] => match le {
            Some(true) => VariableBounds::lower(*v),
            Some(false) => VariableBounds::upper(*v),
            None => VariableBounds::range(*v, *v),
        },
        [Item::Value(a), Item::Operator(Some(true)), Item::Variable, Item::Operator(Some(true)), Item::Value(b)] => {
            VariableBounds::range(*a, *b)
        }
        [Item::Value(a), Item::Operator(Some(false)), Item::Variable, Item::Operator(Some(false)), Item::Value(b)] => {
            VariableBounds::range(*b, *a)
        }
        _ => return None,
    };
    Some(bounds)
}

/// Single-word section keywords (`src/scanner.c`, upstream
/// `Lexer::resolve_keyword`): keywords only as the first token of a line.
pub const SECTION_WORDS: &[&str] = &[
    "bound",
    "bounds",
    "gen",
    "general",
    "generals",
    "integer",
    "integers",
    "bin",
    "binary",
    "binaries",
    "semi",
    "semis",
    "semi-continuous",
    "sos",
    "end",
    "genconstr",
    "genconstrs",
];

/// Whether `word` is one of [`SECTION_WORDS`] (case-insensitive).
#[must_use]
pub fn is_section_word(word: &str) -> bool {
    SECTION_WORDS.iter().any(|k| k.eq_ignore_ascii_case(word))
}

/// Whether `word` may be read as a keyword when it starts a line: a section
/// word, or `st`/`s.t.` (`Subject To`).
#[must_use]
pub fn is_line_start_keyword(word: &str) -> bool {
    is_section_word(word) || word.eq_ignore_ascii_case("st") || word.eq_ignore_ascii_case("s.t.")
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
    fn reads_every_bundled_query() {
        for (name, source) in [
            ("highlights", tree_sitter_lp::HIGHLIGHTS_QUERY),
            ("locals", tree_sitter_lp::LOCALS_QUERY),
            ("folds", tree_sitter_lp::FOLDS_QUERY),
            ("indents", tree_sitter_lp::INDENTS_QUERY),
        ] {
            let captures = query_captures(source).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert!(!captures.is_empty(), "{name}");
            for c in &captures {
                assert!(!kind_ids(&c.kind, c.named).is_empty(), "{name}: unknown kind {c:?}");
            }
        }
        let locals = query_captures(tree_sitter_lp::LOCALS_QUERY).unwrap();
        let bound = QueryCapture {
            name: "local.definition".to_owned(),
            kind: "identifier".to_owned(),
            named: true,
            parent: Some("bound_declaration".to_owned()),
        };
        assert!(locals.contains(&bound), "{locals:?}");
        let highlights = query_captures(tree_sitter_lp::HIGHLIGHTS_QUERY).unwrap();
        assert!(highlights.iter().any(|c| c.kind == "->" && !c.named && c.name == "operator"));
    }

    #[test]
    fn rejects_shapes_it_cannot_mirror() {
        assert!(query_captures("((identifier) @x (#eq? @x \"y\"))").is_err());
        assert!(query_captures("(constraint (linear_expression (term) @t))").is_err());
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
