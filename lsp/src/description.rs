//! `\ @description` comments: a human-readable note written directly above an
//! objective, constraint, general constraint, SOS set or variable declaration.
//!
//! # Grammar
//!
//! A **description line** is a physical line that, after stripping one
//! trailing `\r` (CRLF), matches
//!
//! ```text
//! [ \t]*  "\"  [ \t]*  "@description"  ( end-of-line | [ \t]+ TEXT )
//! ```
//!
//! - The `\` must start a tree-sitter `line_comment` node, so a `\ @description`
//!   line inside a `\* ... *\` block comment is not one.
//! - `@description` is case-sensitive. `@descriptionx`, `@description:` and
//!   `@Description` are plain comments.
//! - `TEXT` is the rest of the line, trimmed; a bare `\ @description`
//!   contributes empty text.
//! - Lines break on `\n` only (`\r\n` strips the `\r`). A lone `\r` is not a
//!   line break; as a control character it is stripped from the text.
//!
//! A **description block** is the maximal run of consecutive description lines
//! immediately above the entry's line, at most [`MAX_LINES`] lines (the nearest
//! are kept). Any other line (blank, whitespace-only, plain comment, code)
//! ends the block, so a blank line or plain comment detaches the description.
//!
//! The description is the block's non-empty `TEXT`s in document order joined
//! with one space, with control and bidi-control characters removed and cut
//! to [`MAX_CHARS`] characters (`…` appended when cut). Non-ASCII text is kept.
//!
//! **Which entry a block attaches to** (the line directly below it):
//!
//! - An objective, constraint, general constraint or SOS set whose range
//!   starts at the first non-whitespace byte of its line. A second entity on
//!   the same line, or `subject to c1: ...` on one line, takes none. Both names
//!   of a ranged constraint share the description.
//! - A variable: its first `Bounds` or type-section occurrence that is also the
//!   first variable site on its line (`b1` of `b1 b2 b3`). With several
//!   described declarations the first in document order wins. Descriptions
//!   above constraints never apply to the variables they use.
//! - Nothing else: section headers, `end`, end of file or a trailing comment.

use ls_types::Position;

use crate::document::Document;
use crate::index::Variable;
use crate::syntax::kind;

/// Most description lines read above one entry.
pub const MAX_LINES: usize = 16;
/// Longest description, in characters.
pub const MAX_CHARS: usize = 1000;

const TAG: &str = "@description";

/// The text after `@description` on one line (no `\n`, `\r` already stripped),
/// or `None` if the line is not a description line.
#[must_use]
pub fn parse_line(line: &str) -> Option<&str> {
    let rest = line.trim_start_matches([' ', '\t']).strip_prefix('\\')?;
    let rest = rest.trim_start_matches([' ', '\t']).strip_prefix(TAG)?;
    if rest.is_empty() {
        return Some("");
    }
    // The tag must be followed by blank space, not more word characters.
    rest.starts_with([' ', '\t']).then(|| rest.trim())
}

/// Description of symbol-index entity `entity`.
#[must_use]
pub fn entity(doc: &Document, entity: usize) -> Option<String> {
    debug_assert!(entity < doc.index().entities.len(), "entity index in range");
    let start = doc.index().entities.get(entity)?.range.start;
    let bytes = doc.text.as_bytes();
    // Early exit on the first byte that is not indentation, so mid-line
    // entities cost O(1).
    let indent = bytes[..start].iter().rev().take_while(|&&b| b == b' ' || b == b'\t').count();
    let line_start = start - indent;
    (line_start == 0 || bytes[line_start - 1] == b'\n').then(|| above(doc, line_start)).flatten()
}

/// Description of a variable, from its first described declaration.
#[must_use]
pub fn variable(doc: &Document, variable: &Variable) -> Option<String> {
    debug_assert!(variable.occurrences.windows(2).all(|w| w[0].range.start <= w[1].range.start), "occurrences are in document order");
    let sites = doc.index().sites();
    variable.occurrences.iter().filter(|o| o.role.is_declaration()).find_map(|o| {
        let line = doc.position(o.range.start).line;
        let line_start = doc.offset(Position { line, character: 0 });
        // Only the first variable site on the line takes the description above it.
        let first = sites.partition_point(|(range, _)| range.start < line_start);
        (sites.get(first)?.0.start == o.range.start).then(|| above(doc, line_start)).flatten()
    })
}

/// Backslash-escapes CommonMark punctuation: `` \ ` * _ { } [ ] ( ) # + - . ! | < > ~ & ``
#[must_use]
pub fn escape_markdown(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if "\\`*_{}[]()#+-.!|<>~&".contains(c) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// Joined, sanitised description block ending just above `line_start`.
fn above(doc: &Document, line_start: usize) -> Option<String> {
    debug_assert!(line_start <= doc.text.len(), "line start inside the text");
    debug_assert!(doc.text.is_char_boundary(line_start), "line start on a char boundary");
    let text = &doc.text;
    let root = doc.tree.root_node();
    let mut parts: Vec<&str> = Vec::new();
    let mut end = line_start;
    let mut lines = 0;
    while end > 0 && lines < MAX_LINES {
        // `end` is a line start, so the byte before it is the previous `\n`.
        let body_end = end - 1;
        let start = text[..body_end].rfind('\n').map_or(0, |i| i + 1);
        let line = text[start..body_end].strip_suffix('\r').unwrap_or(&text[start..body_end]);
        let Some(body) = parse_line(line) else { break };
        let slash = start + (line.len() - line.trim_start_matches([' ', '\t']).len());
        if root.descendant_for_byte_range(slash, slash + 1).is_none_or(|n| n.kind() != kind::LINE_COMMENT) {
            break;
        }
        if !body.is_empty() {
            parts.push(body);
        }
        lines += 1;
        end = start;
    }
    parts.reverse();
    sanitise(&parts.join(" "))
}

/// Remove control and bidi-control characters and cap the length.
fn sanitise(text: &str) -> Option<String> {
    let mut out = String::new();
    for (count, c) in text.chars().filter(|&c| !c.is_control() && !is_bidi_control(c)).enumerate() {
        if count == MAX_CHARS {
            out.push('…');
            break;
        }
        out.push(c);
    }
    let out = out.trim().to_owned();
    (!out.is_empty()).then_some(out)
}

const fn is_bidi_control(c: char) -> bool {
    matches!(c, '\u{61C}' | '\u{200E}' | '\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}')
}

#[cfg(test)]
mod tests {
    use ls_types::Uri;

    use super::*;
    use crate::position::Encoding;

    fn doc(text: &str) -> Document {
        Document::new("file:///t.lp".parse::<Uri>().unwrap(), text.to_owned(), 1, Encoding::Utf8)
    }

    /// Description of the constraint named `name`.
    fn constraint(text: &str, name: &str) -> Option<String> {
        let d = doc(text);
        let (id, _) = d.index().entities_named(name, crate::index::Namespace::Constraint).next().expect("constraint");
        entity(&d, id)
    }

    fn var(text: &str, name: &str) -> Option<String> {
        let d = doc(text);
        variable(&d, d.index().variable(name).expect("variable"))
    }

    #[test]
    fn parse_line_accepts_the_tag_forms() {
        assert_eq!(parse_line("\\ @description Cost row"), Some("Cost row"));
        assert_eq!(parse_line("\\@description x"), Some("x"));
        assert_eq!(parse_line(" \t\\ \t@description   x  "), Some("x"));
        assert_eq!(parse_line("\\ @description"), Some(""));
        assert_eq!(parse_line("\\ @description\tx"), Some("x"));
    }

    #[test]
    fn parse_line_rejects_near_misses() {
        for line in [
            "\\ @descriptionx y",
            "\\ @description: y",
            "\\ @Description y",
            "\\ note @description y",
            "x \\ @description y",
            "\\* @description *\\",
            "@description y",
        ] {
            assert_eq!(parse_line(line), None, "{line}");
        }
    }

    #[test]
    fn constraint_description_directly_above() {
        let text = "min\n obj: x\nst\n \\ @description Plant cap\n c1: x <= 4\nend\n";
        assert_eq!(constraint(text, "c1").as_deref(), Some("Plant cap"));
    }

    #[test]
    fn multiple_lines_join_with_spaces_and_empty_tags_keep_the_block() {
        let text = "min\n obj: x\nst\n \\ @description a\n \\ @description\n \\ @description b\n c1: x <= 4\nend\n";
        assert_eq!(constraint(text, "c1").as_deref(), Some("a b"));
    }

    #[test]
    fn blank_or_plain_comment_line_detaches() {
        for gap in ["\n", "\\ other\n", "   \n"] {
            let text = format!("min\n obj: x\nst\n \\ @description a\n{gap} c1: x <= 4\nend\n");
            assert_eq!(constraint(&text, "c1"), None, "{gap:?}");
        }
    }

    #[test]
    fn crlf_and_unicode() {
        let text = "min\r\n obj: x\r\nst\r\n \\ @description Coût — 東京 🚀\r\n c1: x <= 4\r\nend\r\n";
        let got = constraint(text, "c1").unwrap();
        assert_eq!(got, "Coût — 東京 🚀");
        assert!(!got.contains('\r'));
    }

    #[test]
    fn entry_must_start_its_line() {
        assert_eq!(constraint("min\n obj: x\n\\ @description d\nst c1: x >= 1\nend\n", "c1"), None);
        assert_eq!(constraint("min\n obj: x\nst\n \\ @description d\n c0: x >= 0 c1: x >= 1\nend\n", "c1"), None);
        let d = doc("\\ @description d\nmaximize obj: x\nst\n c1: x <= 1\nend\n");
        let (id, _) = d.index().entities_named("obj", crate::index::Namespace::Objective).next().unwrap();
        assert_eq!(entity(&d, id), None, "objective after the sense keyword");
        let first = doc("min obj: x\nst\n c1: x <= 1\nend\n");
        assert_eq!(entity(&first, 0), None, "no line above");
    }

    #[test]
    fn block_comment_lines_are_not_descriptions() {
        let text = "min\n obj: x\nst\n\\* \n\\ @description x *\\\n c1: x <= 4\nend\n";
        assert_eq!(constraint(text, "c1"), None);
    }

    #[test]
    fn sanitises_and_caps() {
        let text = "min\n obj: x\nst\n \\ @description a\u{202E}b\rc\n c1: x <= 4\nend\n";
        assert_eq!(constraint(text, "c1").as_deref(), Some("abc"));

        let lines = (0..17).map(|i| format!(" \\ @description l{i}\n")).collect::<Vec<_>>().concat();
        let text = format!("min\n obj: x\nst\n{lines} c1: x <= 4\nend\n");
        let got = constraint(&text, "c1").unwrap();
        assert!(got.starts_with("l1 ") && got.ends_with("l16"), "{got}");
        assert!(!got.contains("l0"), "{got}");

        let long = "é".repeat(1200);
        let text = format!("min\n obj: x\nst\n \\ @description {long}\n c1: x <= 4\nend\n");
        let got = constraint(&text, "c1").unwrap();
        assert_eq!(got.chars().count(), MAX_CHARS + 1);
        assert!(got.ends_with('…'));
    }

    #[test]
    fn variable_from_bound_and_type_sections() {
        let text = "min\n obj: x + b1 + b2\nst\n \\ @description on a constraint\n c1: x + b1 <= 4\nbounds\n \\ @description cap\n x <= 4\nbinaries\n \\ @description flag\n b1 b2\nend\n";
        assert_eq!(var(text, "x").as_deref(), Some("cap"));
        assert_eq!(var(text, "b1").as_deref(), Some("flag"));
        assert_eq!(var(text, "b2"), None);

        let used = "min\n obj: x\nst\n \\ @description only here\n c1: x <= 4\nend\n";
        assert_eq!(var(used, "x"), None);

        let twice = "min\n obj: x\nst\n c1: x <= 4\nbounds\n \\ @description first\n x <= 4\ngenerals\n \\ @description second\n x\nend\n";
        assert_eq!(var(twice, "x").as_deref(), Some("first"));
    }

    #[test]
    fn escape_markdown_neutralises_links_images_and_html() {
        assert_eq!(escape_markdown("[a](command:x)"), "\\[a\\]\\(command:x\\)");
        assert_eq!(escape_markdown("![i](http://h/p.png)"), "\\!\\[i\\]\\(http://h/p\\.png\\)");
        assert_eq!(escape_markdown("<img>"), "\\<img\\>");
        assert_eq!(escape_markdown("*b*"), "\\*b\\*");
    }
}
