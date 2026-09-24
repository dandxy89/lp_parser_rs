//! Signature help for `MAX`/`MIN`/`ABS`/`AND`/`OR`.

use tower_lsp_server::ls_types::{
    Documentation, MarkupContent, MarkupKind, ParameterInformation, ParameterLabel, Position, SignatureHelp, SignatureInformation,
};

use super::completion::{self, Section};
use super::docs;
use crate::document::Document;

/// Signature help at `position`.
#[must_use]
pub fn help(doc: &Document, position: Position) -> Option<SignatureHelp> {
    let offset = doc.offset(position);
    let text = &doc.text;
    if completion::in_comment(doc, offset) {
        return None;
    }
    // Innermost unclosed `(` before the cursor within this general constraint.
    // Tokens are whitespace-separated, as upstream lexes them: `(`, `,` and
    // `)` inside a name (`x(1)`, `a,b`) belong to the name.
    let mut commas = 0;
    let mut open = None;
    for (start, token) in tokens_before(text, offset) {
        match token {
            "," => commas += 1,
            "(" => {
                open = Some(start);
                break;
            }
            ")" | "=" => return None,
            _ if token.ends_with(':') => return None,
            // `ABS(x` while typing: a function name glued to its `(`.
            _ => {
                if let Some(i) = token.find('(').filter(|&i| docs::function(&token[..i]).is_some()) {
                    open = Some(start + i);
                    break;
                }
            }
        }
    }
    let open = open?;
    let head = text[..open].trim_end();
    let name_start = completion::word_start(head, completion::line_start(head, head.len()), head.len());
    let function = docs::function(&head[name_start..])?;
    if !head[..name_start].trim_end().ends_with('=') || completion::place(doc, open).section != Section::General {
        return None;
    }

    debug_assert_ne!(function.params.len(), 0, "every function takes an argument");
    let active = u32::try_from(commas.min(function.params.len().saturating_sub(1))).unwrap_or(0);
    let (label, offsets) = function.signature();
    let parameters = function
        .params
        .iter()
        .zip(offsets)
        .map(|((_, description), range)| ParameterInformation {
            label: ParameterLabel::LabelOffsets(range),
            documentation: Some(Documentation::String((*description).to_owned())),
        })
        .collect();
    let signature = SignatureInformation {
        label,
        documentation: Some(Documentation::MarkupContent(MarkupContent { kind: MarkupKind::Markdown, value: function.summary.to_owned() })),
        parameters: Some(parameters),
        active_parameter: Some(active),
    };
    Some(SignatureHelp { signatures: vec![signature], active_signature: Some(0), active_parameter: Some(active) })
}

/// Whitespace-separated tokens ending at or before `end`, last first, as
/// `(start offset, token)`.
fn tokens_before(text: &str, end: usize) -> impl Iterator<Item = (usize, &str)> {
    debug_assert!(text.is_char_boundary(end));
    let bytes = text.as_bytes();
    let mut i = end;
    std::iter::from_fn(move || {
        while i > 0 && bytes[i - 1].is_ascii_whitespace() {
            i -= 1;
        }
        if i == 0 {
            return None;
        }
        let stop = i;
        while i > 0 && !bytes[i - 1].is_ascii_whitespace() {
            i -= 1;
        }
        // Next to ASCII whitespace or the text ends, so on char boundaries.
        Some((i, &text[i..stop]))
    })
}

#[cfg(test)]
mod tests {
    use tower_lsp_server::ls_types::Uri;

    use super::*;
    use crate::position::Encoding;

    /// Active parameter with `|` marking the cursor.
    fn active(text: &str) -> Option<u32> {
        let at = text.find('|').expect("cursor marker");
        let doc = Document::new("file:///t.lp".parse::<Uri>().unwrap(), text.replacen('|', "", 1), 1, Encoding::Utf16);
        help(&doc, doc.position(at)).map(|h| h.active_parameter.unwrap())
    }

    #[test]
    fn active_parameter_counts_commas() {
        let gc = |args: &str| format!("min\n obj: x\nst\n c: x >= 1\nGeneral Constraints\n g1: r = MAX ( {args} )\nend\n");
        assert_eq!(active(&gc("|x , y")), Some(0));
        assert_eq!(active(&gc("x , |y")), Some(1));
        assert_eq!(active(&gc("x , y , z , |3")), Some(2));
        assert_eq!(active("min\n obj: x\nst\n c: x >= 1\ngenconstrs\n g: r = ABS(x|\nend\n"), Some(0));
    }

    #[test]
    fn punctuation_inside_names_is_not_an_argument_separator() {
        let gc = |args: &str| format!("min\n obj: x\nst\n c: x >= 1\nGeneral Constraints\n g1: r = MAX ( {args} )\nend\n");
        assert_eq!(active(&gc("x(1)|")), Some(0));
        assert_eq!(active(&gc("x(1) , |y")), Some(1));
        assert_eq!(active(&gc("a,b|")), Some(0));
        assert_eq!(active(&gc("a,b , y , |z")), Some(2));
    }

    #[test]
    fn none_outside_parentheses() {
        let text = "min\n obj: x\nst\n c: x >= 1\nGeneral Constraints\n g1: r = MAX ( x , y )|\nend\n";
        assert_eq!(active(text), None);
        assert_eq!(active("min\n obj: x\nst\n c: x >= 1\nGeneral Constraints\n g1: r| = MAX ( x )\nend\n"), None);
        // A `(` in a name outside the general constraints section.
        assert_eq!(active("min\n obj: x\nst\n c: y = MAX ( x|\nend\n"), None);
    }
}
