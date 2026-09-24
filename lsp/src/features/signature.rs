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
    let open = text[..offset].rfind(['(', ')', ':', '='])?;
    if text.as_bytes()[open] != b'(' {
        return None;
    }
    let head = text[..open].trim_end();
    let name_start = completion::word_start(head, completion::line_start(head, head.len()), head.len());
    let function = docs::function(&head[name_start..])?;
    if !head[..name_start].trim_end().ends_with('=') || completion::place(doc, open).section != Section::General {
        return None;
    }

    let commas = text[open..offset].matches(',').count();
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
    fn none_outside_parentheses() {
        let text = "min\n obj: x\nst\n c: x >= 1\nGeneral Constraints\n g1: r = MAX ( x , y )|\nend\n";
        assert_eq!(active(text), None);
        assert_eq!(active("min\n obj: x\nst\n c: x >= 1\nGeneral Constraints\n g1: r| = MAX ( x )\nend\n"), None);
        // A `(` in a name outside the general constraints section.
        assert_eq!(active("min\n obj: x\nst\n c: y = MAX ( x|\nend\n"), None);
    }
}
