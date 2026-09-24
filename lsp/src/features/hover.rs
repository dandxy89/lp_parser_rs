//! Markdown hover for variables, constraints, objectives, keywords and functions.

use std::collections::HashSet;
use std::ops::Range;

use tower_lsp_server::ls_types::{Hover, HoverContents, MarkupContent, MarkupKind, Position};
use tree_sitter::Node;

use super::docs;
use crate::document::Document;
use crate::index::{Entity, EntityKind, Namespace, Role, Symbol, Variable};
use crate::syntax::{self, kind};

/// Append a formatted line to a `String`.
macro_rules! push_line {
    ($out:expr, $($arg:tt)*) => {{
        $out.push_str(&format!($($arg)*));
        $out.push('\n');
    }};
}

/// Constraints listed in a variable hover before eliding the rest.
const LISTED_CONSTRAINTS: usize = 5;

/// Hover at `position`.
#[must_use]
pub fn hover(doc: &Document, position: Position) -> Option<Hover> {
    let offset = doc.offset(position);
    let (range, value) = match doc.index().symbol_at(offset) {
        Some((range, Symbol::Variable(var, _))) => (range, variable(doc, &doc.index().variables[var])),
        Some((range, Symbol::Entity(entity))) => (range, entity_hover(doc, entity)?),
        Some((range, Symbol::Attribute(attr))) => {
            let (name, summary) = docs::attribute(&doc.index().attributes[attr].name)?;
            (range, format!("**{name}** (objective attribute)\n\n{summary}"))
        }
        None => token_hover(doc, offset)?,
    };
    Some(Hover { contents: HoverContents::Markup(MarkupContent { kind: MarkupKind::Markdown, value }), range: Some(doc.range(range)) })
}

/// Keyword and function-name hover from the token under the cursor.
fn token_hover(doc: &Document, offset: usize) -> Option<(Range<usize>, String)> {
    let token = syntax::token_at(&doc.tree, offset)?;
    let text = doc.node_text(token);
    let value = if token.kind() == kind::FUNCTION_NAME {
        docs::function(text)?.markdown()
    } else {
        docs::keyword_markdown(docs::keyword_for_token(token.kind(), text)?)
    };
    Some((token.byte_range(), value))
}

fn variable(doc: &Document, var: &Variable) -> String {
    let index = doc.index();
    let has = |role: Role| var.occurrences.iter().any(|o| o.role == role);
    let bounds: Vec<String> = var
        .occurrences
        .iter()
        .filter(|o| o.role == Role::Bound)
        .filter_map(|o| doc.tree.root_node().descendant_for_byte_range(o.range.start, o.range.end)?.parent())
        .filter(|n| n.kind() == kind::BOUND_DECLARATION)
        .map(|n| normalise(n, &doc.text))
        .collect();
    let free = bounds.iter().any(|b| b.to_ascii_lowercase().ends_with(" free"));
    let integer = has(Role::Generals) || has(Role::Integers);
    let semi = has(Role::SemiContinuous);
    let var_type = match (has(Role::Binaries), integer, semi) {
        (true, _, _) => "binary",
        (false, true, true) => "semi-integer",
        (false, true, false) => "integer",
        (false, false, true) => "semi-continuous",
        (false, false, false) if free => "free",
        _ => "continuous",
    };

    let mut out = format!("**{}** (variable)\n\n- **Type:** {var_type}\n", var.name);
    if bounds.is_empty() {
        let default = if var_type == "binary" { "0 <= x <= 1" } else { "0 <= x <= +inf" };
        push_line!(out, "- **Bounds:** none declared (default `{}`)", default.replacen('x', &var.name, 1));
    } else {
        let listed: Vec<String> = bounds.iter().map(|b| format!("`{b}`")).collect();
        push_line!(out, "- **Bounds:** {}", listed.join(", "));
    }

    // Objective coefficients, summed per objective.
    let mut objectives: Vec<(usize, f64)> = Vec::new();
    for o in var.occurrences.iter().filter(|o| o.role == Role::ObjectiveTerm) {
        let (Some(entity), Some(c)) = (o.entity, o.coefficient) else { continue };
        match objectives.iter_mut().find(|(e, _)| *e == entity) {
            Some((_, sum)) => *sum += c,
            None => objectives.push((entity, c)),
        }
    }
    if !objectives.is_empty() {
        let listed: Vec<String> = objectives.iter().map(|&(e, c)| format!("`{}`: {c}", entity_label(doc, e))).collect();
        push_line!(out, "- **Objective:** {}", listed.join(", "));
    }

    let constraints: Vec<usize> = var
        .entities()
        .into_iter()
        .filter(|&e| matches!(index.entities[e].kind, EntityKind::Constraint | EntityKind::GeneralConstraint))
        .collect();
    if !constraints.is_empty() {
        let listed: Vec<String> = constraints
            .iter()
            .take(LISTED_CONSTRAINTS)
            .map(|&e| {
                let linear: Vec<f64> = var
                    .occurrences
                    .iter()
                    .filter(|o| o.entity == Some(e) && o.role == Role::ConstraintTerm)
                    .filter_map(|o| o.coefficient)
                    .collect();
                let detail = if linear.is_empty() {
                    var.occurrences.iter().find(|o| o.entity == Some(e)).map_or("", |o| o.role.label()).to_owned()
                } else {
                    linear.iter().sum::<f64>().to_string()
                };
                format!("`{}` ({detail})", entity_label(doc, e))
            })
            .collect();
        let more = constraints.len().saturating_sub(LISTED_CONSTRAINTS);
        let tail = if more > 0 { format!(", and {more} more") } else { String::new() };
        push_line!(out, "- **Constraints ({}):** {}{tail}", constraints.len(), listed.join(", "));
    }

    let sos: Vec<String> = var
        .occurrences
        .iter()
        .filter(|o| o.role == Role::SosEntry)
        .map(|o| {
            let set = o.entity.map_or_else(|| "?".to_owned(), |e| entity_label(doc, e));
            let weight = o.coefficient.map_or_else(|| "?".to_owned(), |w| w.to_string());
            format!("`{set}` (weight {weight})")
        })
        .collect();
    if !sos.is_empty() {
        push_line!(out, "- **SOS:** {}", sos.join(", "));
    }
    out
}

/// Name of an entity, or its line for an unnamed one.
fn entity_label(doc: &Document, entity: usize) -> String {
    let e = &doc.index().entities[entity];
    e.name.clone().unwrap_or_else(|| format!("line {}", doc.position(e.range.start).line + 1))
}

fn entity_hover(doc: &Document, id: usize) -> Option<String> {
    let entity = &doc.index().entities[id];
    let node = doc.node(entity.range.clone(), entity.node_kind);
    let name = entity.name.as_deref().unwrap_or("(unnamed)");
    let mut out = format!("**{name}** ({})\n\n", entity.kind.label());
    match entity.kind {
        EntityKind::Objective => {
            let sense = doc.tree.root_node().child(0).filter(|n| n.kind() == kind::SENSE);
            let sense = sense.and_then(|n| docs::keyword_for_token(kind::SENSE, doc.node_text(n))).map_or("unknown", |k| k.label);
            push_line!(out, "- **Sense:** {sense}");
            for attr in doc.index().attributes.iter().filter(|a| a.objective == id) {
                let text = doc.node(attr.range.clone(), kind::OBJECTIVE_ATTRIBUTE).map(|n| normalise(n, &doc.text));
                push_line!(out, "- **{}:** `{}`", attr.name, text.as_deref().unwrap_or(doc.slice(attr.range.clone())));
            }
            let terms = node.map_or(0, count_terms);
            push_line!(out, "- **Terms:** {terms}");
        }
        EntityKind::Constraint => {
            let node = node?;
            if let Some(form) = constraint_form(node, &doc.text) {
                push_line!(out, "```lp\n{form}\n```\n");
            }
            push_line!(out, "- **Section:** {}", entity.section.label());
            if let (Some(n), true) = (entity.name.as_deref(), is_ranged(node)) {
                push_line!(out, "- **Upper half generated as:** `{}`", range_upper_name(doc, n));
            }
            if let Some(indicator) = node.named_children(&mut node.walk()).find(|c| c.kind() == kind::INDICATOR) {
                let head = normalise(indicator, &doc.text);
                push_line!(out, "- **Active when:** `{}`", head.trim_end_matches("->").trim_end());
            }
        }
        EntityKind::GeneralConstraint | EntityKind::Sos => {
            let text = match entity.kind {
                EntityKind::Sos => normalise_range(doc, entity.range.clone()),
                _ => node.map(|n| normalise(n, &doc.text)).unwrap_or_default(),
            };
            push_line!(out, "```lp\n{text}\n```\n\n- **Section:** {}", entity.section.label());
            if entity.kind == EntityKind::GeneralConstraint
                && let Some(function) = node.and_then(|n| n.child_by_field_name("function")).and_then(|f| docs::function(doc.node_text(f)))
            {
                push_line!(out, "\n{}", function.summary);
            }
        }
    }
    Some(out)
}

/// Upstream `range_upper_name`: `name_rng`, else `name_rng2`, `name_rng3`, ...
/// avoiding every constraint name written in the file.
fn range_upper_name(doc: &Document, base: &str) -> String {
    let taken: HashSet<&str> = doc
        .index()
        .entities
        .iter()
        .filter(|e| e.kind.namespace() == Namespace::Constraint)
        .filter_map(|e: &Entity| e.name.as_deref())
        .collect();
    let mut candidate = format!("{base}_rng");
    let mut suffix: u32 = 1;
    while taken.contains(candidate.as_str()) {
        suffix += 1;
        candidate = format!("{base}_rng{suffix}");
    }
    candidate
}

/// Number of linear and quadratic terms under `node`.
fn count_terms(node: Node<'_>) -> usize {
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .map(|c| match c.kind() {
            kind::TERM | kind::QUADRATIC_TERM => 1,
            kind::LINEAR_EXPRESSION | kind::QUADRATIC_BLOCK => count_terms(c),
            _ => 0,
        })
        .sum()
}

/// A piece of a constraint body.
enum Part<'t> {
    Expr(Node<'t>),
    Op(&'static str),
    Num(String),
}

fn body_parts<'t>(node: Node<'t>, text: &str) -> Vec<Part<'t>> {
    let mut parts = Vec::new();
    let mut negative = false;
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            kind::LINEAR_EXPRESSION => parts.push(Part::Expr(child)),
            kind::COMPARISON_OPERATOR => parts.push(Part::Op(canonical_operator(syntax::text(child, text)))),
            kind::NUMBER | kind::INFINITY => {
                let value = syntax::text(child, text);
                parts.push(Part::Num(if negative { format!("-{value}") } else { value.to_owned() }));
                negative = false;
            }
            "-" => negative = true,
            _ => {}
        }
    }
    parts
}

fn is_ranged(node: Node<'_>) -> bool {
    let mut cursor = node.walk();
    node.children(&mut cursor).filter(|c| c.kind() == kind::COMPARISON_OPERATOR).count() == 2
}

/// `coeffs op rhs`, with flipped constraints turned around and ranged ones
/// shown as `lo <= expr <= hi`.
fn constraint_form(node: Node<'_>, text: &str) -> Option<String> {
    match body_parts(node, text).as_slice() {
        [Part::Expr(e), Part::Op(op), Part::Num(rhs)] => Some(format!("{} {op} {rhs}", expression(*e, text))),
        [Part::Num(lhs), Part::Op(op), Part::Expr(e)] => Some(format!("{} {} {lhs}", expression(*e, text), flip(op))),
        [Part::Num(lo), Part::Op(op1), Part::Expr(e), Part::Op(op2), Part::Num(hi)] => {
            let e = expression(*e, text);
            // `hi >= expr >= lo` reads better the other way round.
            Some(if op1.starts_with('>') && op2.starts_with('>') {
                format!("{hi} {} {e} {} {lo}", flip(op2), flip(op1))
            } else {
                format!("{lo} {op1} {e} {op2} {hi}")
            })
        }
        _ => None,
    }
}

/// Render a `linear_expression` as `3 x + 2 y - z`.
fn expression(node: Node<'_>, text: &str) -> String {
    let mut out = String::new();
    let mut negative = false;
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        let item = match child.kind() {
            "-" => {
                negative = !negative;
                continue;
            }
            "+" => continue,
            kind::TERM => term(child, text),
            kind::QUADRATIC_BLOCK => normalise(child, text),
            _ => continue,
        };
        match (out.is_empty(), negative) {
            (true, true) => out.push('-'),
            (true, false) => {}
            (false, true) => out.push_str(" - "),
            (false, false) => out.push_str(" + "),
        }
        out.push_str(&item);
        negative = false;
    }
    out
}

/// A term without its sign: `3 x`, `x` (unit coefficient) or a constant.
fn term(node: Node<'_>, text: &str) -> String {
    let mut cursor = node.walk();
    let parts: Vec<&str> = node.children(&mut cursor).map(|c| syntax::text(c, text)).collect();
    match parts.as_slice() {
        [coefficient, name] if syntax::parse_number(coefficient) == Some(1.0) => (*name).to_owned(),
        _ => parts.join(" "),
    }
}

const fn flip(op: &str) -> &'static str {
    match op.as_bytes() {
        b"<=" => ">=",
        b">=" => "<=",
        b"<" => ">",
        b">" => "<",
        _ => "=",
    }
}

fn canonical_operator(op: &str) -> &'static str {
    match op {
        "<=" | "=<" => "<=",
        ">=" | "=>" => ">=",
        "<" => "<",
        ">" => ">",
        _ => "=",
    }
}

/// Source of `node` with single spaces between tokens, signs glued to the
/// number they negate and operator aliases canonicalised.
fn normalise(node: Node<'_>, text: &str) -> String {
    normalise_tokens(leaves(node), text)
}

fn normalise_range(doc: &Document, range: Range<usize>) -> String {
    let root = doc.tree.root_node();
    let nodes = leaves(root).into_iter().filter(|n| n.start_byte() >= range.start && n.end_byte() <= range.end).collect();
    normalise_tokens(nodes, &doc.text)
}

fn normalise_tokens(tokens: Vec<Node<'_>>, text: &str) -> String {
    let mut out = String::new();
    let mut glue = false;
    let mut previous_is_value = false;
    for token in tokens {
        let t = syntax::text(token, text);
        if !out.is_empty() && !glue {
            out.push(' ');
        }
        out.push_str(if token.kind() == kind::COMPARISON_OPERATOR { canonical_operator(t) } else { t });
        // A sign that does not follow a value negates the number after it.
        glue = t == "-" && !previous_is_value;
        previous_is_value = matches!(token.kind(), kind::IDENTIFIER | kind::NUMBER | kind::INFINITY | "]");
    }
    out
}

/// Leaf tokens under `node`, skipping comments.
fn leaves(node: Node<'_>) -> Vec<Node<'_>> {
    let mut out = Vec::new();
    let mut stack = vec![node];
    while let Some(n) = stack.pop() {
        if matches!(n.kind(), kind::LINE_COMMENT | kind::BLOCK_COMMENT) {
            continue;
        }
        if n.child_count() == 0 || n.kind() == kind::COMPARISON_OPERATOR {
            out.push(n);
            continue;
        }
        let mut cursor = n.walk();
        let children: Vec<Node<'_>> = n.children(&mut cursor).collect();
        stack.extend(children.into_iter().rev());
    }
    out
}

#[cfg(test)]
mod tests {
    use tower_lsp_server::ls_types::Uri;

    use super::*;
    use crate::position::Encoding;

    const MODEL: &str = r"Maximize multi-objectives
 o1: Priority=2 Weight=1 3 x + 2 y - z
 o2: - x
Subject To
 c1: 3 x + 2 y <= 10
 f1: 10 >= x
 r1: 2 <= x + z <= 8
 r1_rng: y >= 0
 ind1: b = 1 -> x + y =< 3
 c4: x >= 0
 c5: x >= 1
 c6: x >= 2
General Constraints
 g1: r = MAX ( x , y , 3 )
Bounds
 0 <= x <= 40
 y free
 z >= - inf
Generals
 x
Binaries
 b
SOS
 s1: S1 :: x : 1 y : -2
End
";

    fn doc(text: &str) -> Document {
        Document::new("file:///t.lp".parse::<Uri>().unwrap(), text.to_owned(), 1, Encoding::Utf8)
    }

    /// Hover markdown at the `nth` occurrence of `needle` (cursor inside it).
    fn hover_at(d: &Document, needle: &str, nth: usize) -> Option<String> {
        let at = d.text.match_indices(needle).nth(nth).unwrap().0 + 1;
        let hover = hover(d, d.position(at))?;
        let HoverContents::Markup(markup) = hover.contents else { panic!("markdown expected") };
        let range = d.byte_range(hover.range.unwrap());
        assert!(range.start <= at && at <= range.end, "range covers the token");
        Some(markup.value)
    }

    #[test]
    fn variable_hover() {
        let d = doc(MODEL);
        let var_x = hover_at(&d, " x", 0).unwrap();
        assert!(var_x.contains("**Type:** integer"), "{var_x}");
        assert!(var_x.contains("`0 <= x <= 40`"), "{var_x}");
        assert!(var_x.contains("`o1`: 3, `o2`: -1"), "{var_x}");
        assert!(var_x.contains("**Constraints (8):** `c1` (3), `f1` (1), `r1` (1), `ind1` (1), `c4` (1), and 3 more"), "{var_x}");
        assert!(var_x.contains("`s1` (weight 1)"), "{var_x}");
        let var_y = hover_at(&d, "y free", 0).unwrap();
        assert!(var_y.contains("**Type:** free") && var_y.contains("`y free`") && var_y.contains("weight -2"), "{var_y}");
        let var_z = hover_at(&d, "z >=", 0).unwrap();
        assert!(var_z.contains("`z >= -inf`") && var_z.contains("continuous"), "{var_z}");
        let var_b = hover_at(&d, "b =", 0).unwrap();
        assert!(var_b.contains("binary") && var_b.contains("indicator variable"), "{var_b}");
    }

    #[test]
    fn constraint_hover() {
        let d = doc(MODEL);
        let c1 = hover_at(&d, "c1", 0).unwrap();
        assert!(c1.contains("3 x + 2 y <= 10") && c1.contains("subject to"), "{c1}");
        let f1 = hover_at(&d, "f1", 0).unwrap();
        assert!(f1.contains("x <= 10"), "{f1}");
        let r1 = hover_at(&d, "r1:", 0).unwrap();
        assert!(r1.contains("2 <= x + z <= 8") && r1.contains("`r1_rng2`"), "{r1}");
        let ind = hover_at(&d, "ind1", 0).unwrap();
        assert!(ind.contains("x + y <= 3") && ind.contains("`b = 1`"), "{ind}");
        let g1 = hover_at(&d, "g1", 0).unwrap();
        assert!(g1.contains("r = MAX ( x , y , 3 )") && g1.contains("general constraints"), "{g1}");
        let s1 = hover_at(&d, "s1", 0).unwrap();
        assert!(s1.contains("s1 : S1 :: x : 1 y : -2"), "{s1}");
    }

    #[test]
    fn objective_hover() {
        let d = doc(MODEL);
        let o1 = hover_at(&d, "o1", 0).unwrap();
        assert!(o1.contains("**Sense:** Maximize"), "{o1}");
        assert!(o1.contains("**Priority:** `Priority = 2`") && o1.contains("**Weight:**"), "{o1}");
        assert!(o1.contains("**Terms:** 3"), "{o1}");
        let attr = hover_at(&d, "Priority", 0).unwrap();
        assert!(attr.contains("optimised first"), "{attr}");
    }

    #[test]
    fn keyword_and_function_hover() {
        let d = doc(MODEL);
        assert!(hover_at(&d, "Maximize", 0).unwrap().contains("`maximise`"));
        assert!(hover_at(&d, "multi-objectives", 0).unwrap().contains("Priority"));
        assert!(hover_at(&d, "Subject To", 0).unwrap().contains("`s.t.`"));
        assert!(hover_at(&d, "Bounds", 0).unwrap().contains("`bound`"));
        assert!(hover_at(&d, "free", 0).unwrap().contains("-inf <= x <= +inf"));
        assert!(hover_at(&d, "S1", 0).unwrap().contains("at most one"));
        assert!(hover_at(&d, "End", 0).unwrap().contains("ignored"));
        assert!(hover_at(&d, "Binaries", 0).unwrap().contains("`bin`"));
        assert!(hover_at(&d, "MAX", 0).unwrap().contains("largest"));
    }

    #[test]
    fn nothing_on_whitespace() {
        let d = doc("min\n obj: x   +   y\nst\n c: x >= 1\nend\n");
        let at = d.text.find("x   +").unwrap() + 2;
        assert_eq!(hover(&d, d.position(at)), None);
    }
}
