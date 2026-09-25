//! Byte offset ↔ LSP `Position` conversion. Every conversion in the server goes
//! through this module.
//!
//! Lines are split on `\n` only, matching tree-sitter's row counting, so the
//! same index serves both LSP positions and tree-sitter `Point`s. A `\r`
//! before `\n` is treated as part of the line terminator: columns are clamped
//! so a position never lands between `\r` and `\n`.

use std::ops::Range;

use tower_lsp_server::ls_types::{Position, PositionEncodingKind};

/// Negotiated position encoding for a session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Encoding {
    /// Columns count UTF-8 bytes.
    Utf8,
    /// Columns count UTF-16 code units (the LSP default).
    #[default]
    Utf16,
}

impl Encoding {
    /// Pick UTF-8 when the client offers it, else UTF-16.
    #[must_use]
    pub fn negotiate(offered: Option<&[PositionEncodingKind]>) -> Self {
        match offered {
            Some(kinds) if kinds.contains(&PositionEncodingKind::UTF8) => Self::Utf8,
            _ => Self::Utf16,
        }
    }

    /// The LSP name of this encoding.
    #[must_use]
    pub const fn kind(self) -> PositionEncodingKind {
        match self {
            Self::Utf8 => PositionEncodingKind::UTF8,
            Self::Utf16 => PositionEncodingKind::UTF16,
        }
    }
}

/// Start offsets of every line in a text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineIndex {
    line_starts: Vec<usize>,
    /// Per line: whether it is pure ASCII, so UTF-16 columns equal byte
    /// columns and need no re-encoding (which is linear in the line length).
    ascii: Vec<bool>,
    len: usize,
}

impl LineIndex {
    /// Index `text` in one linear pass.
    #[must_use]
    pub fn new(text: &str) -> Self {
        let capacity = text.len() / 32 + 1;
        let (mut line_starts, mut ascii) = (Vec::with_capacity(capacity), Vec::with_capacity(capacity));
        let mut start = 0;
        for line in text.split('\n') {
            line_starts.push(start);
            ascii.push(line.is_ascii());
            start += line.len() + 1;
        }
        debug_assert_eq!(start, text.len() + 1, "every byte belongs to one line");
        Self { line_starts, ascii, len: text.len() }
    }

    /// Update the index after `range` of the indexed text was replaced by
    /// `new_len` bytes, giving `text`. Only the edited lines are rescanned;
    /// later line starts are shifted.
    pub fn edit(&mut self, text: &str, range: Range<usize>, new_len: usize) {
        debug_assert!(range.start <= range.end && range.end <= self.len, "edit range out of bounds");
        debug_assert_eq!(text.len() + (range.end - range.start), self.len + new_len, "text does not match the edit");
        let first = self.line_of(range.start);
        let last = self.line_of(range.end);
        let removed = range.end - range.start;
        // Starts after the edit lie beyond `range.end`, so they are at least `removed`.
        for start in &mut self.line_starts[last + 1..] {
            *start = *start - removed + new_len;
        }
        let new_end = range.start + new_len;
        let inserted: Vec<usize> = text[range.start..new_end].match_indices('\n').map(|(i, _)| range.start + i + 1).collect();
        let added = inserted.len();
        self.line_starts.splice(first + 1..=last, inserted);
        self.len = text.len();
        let ascii: Vec<bool> = (first..=first + added)
            .map(|line| {
                let end = self.line_starts.get(line + 1).map_or(self.len, |&next| next - 1);
                text[self.line_starts[line]..end].is_ascii()
            })
            .collect();
        self.ascii.splice(first..=last, ascii);
        debug_assert_eq!(self.ascii.len(), self.line_starts.len(), "one flag per line");
        debug_assert!(self.line_starts.last().is_some_and(|&start| start <= self.len), "line starts stay within the text");
    }

    /// Number of lines (a trailing newline starts an empty last line).
    #[must_use]
    pub const fn line_count(&self) -> usize {
        self.line_starts.len()
    }

    /// Zero-based line containing `offset`.
    #[must_use]
    pub fn line_of(&self, offset: usize) -> usize {
        debug_assert!(offset <= self.len, "offset {offset} beyond text length {}", self.len);
        self.line_starts.partition_point(|&start| start <= offset) - 1
    }

    /// Byte range of `line`'s content, excluding its `\n` / `\r\n` terminator.
    #[must_use]
    pub fn line_range(&self, text: &str, line: usize) -> Range<usize> {
        debug_assert_eq!(text.len(), self.len, "line index is stale");
        let Some(&start) = self.line_starts.get(line) else { return self.len..self.len };
        let mut end = self.line_starts.get(line + 1).map_or(self.len, |&next| next - 1);
        if end > start && text.as_bytes()[end - 1] == b'\r' && end < self.len {
            end -= 1;
        }
        start..end
    }

    /// Byte offset of the start of `line` (clamped to the text end).
    #[must_use]
    pub fn line_start(&self, line: usize) -> usize {
        self.line_starts.get(line).copied().unwrap_or(self.len)
    }

    /// Convert a byte offset to an LSP position. Offsets inside a multi-byte
    /// character snap back to its start.
    #[must_use]
    pub fn position(&self, text: &str, offset: usize, encoding: Encoding) -> Position {
        let offset = floor_char_boundary(text, offset.min(text.len()));
        let line = self.line_of(offset);
        let range = self.line_range(text, line);
        let end = offset.min(range.end).max(range.start);
        let column = match encoding {
            Encoding::Utf16 if !self.ascii[line] => text[range.start..end].encode_utf16().count(),
            Encoding::Utf8 | Encoding::Utf16 => end - range.start,
        };
        Position::new(to_u32(line), to_u32(column))
    }

    /// Convert an LSP position to a byte offset. Out-of-range lines clamp to
    /// the text end; out-of-range columns clamp to the line end; a column
    /// inside a surrogate pair snaps back to the character start.
    #[must_use]
    pub fn offset(&self, text: &str, position: Position, encoding: Encoding) -> usize {
        let line = position.line as usize;
        if line >= self.line_starts.len() {
            return self.len;
        }
        let range = self.line_range(text, line);
        let column = position.character as usize;
        let line_text = &text[range.clone()];
        match encoding {
            Encoding::Utf16 if self.ascii[line] => range.start + column.min(line_text.len()),
            Encoding::Utf8 => range.start + floor_char_boundary(line_text, column.min(line_text.len())),
            Encoding::Utf16 => {
                let mut units = 0;
                for (i, c) in line_text.char_indices() {
                    let next = units + c.len_utf16();
                    if next > column {
                        return range.start + i;
                    }
                    units = next;
                }
                range.end
            }
        }
    }

    /// Convert a byte range to an LSP range.
    #[must_use]
    pub fn range(&self, text: &str, range: Range<usize>, encoding: Encoding) -> tower_lsp_server::ls_types::Range {
        tower_lsp_server::ls_types::Range::new(self.position(text, range.start, encoding), self.position(text, range.end, encoding))
    }

    /// Convert an LSP range to a byte range (start ≤ end guaranteed).
    #[must_use]
    pub fn byte_range(&self, text: &str, range: tower_lsp_server::ls_types::Range, encoding: Encoding) -> Range<usize> {
        let start = self.offset(text, range.start, encoding);
        let end = self.offset(text, range.end, encoding);
        start.min(end)..end.max(start)
    }

    /// Tree-sitter point for a byte offset (row by `\n`, column in bytes).
    #[must_use]
    pub fn point(&self, offset: usize) -> tree_sitter::Point {
        let row = self.line_of(offset);
        tree_sitter::Point::new(row, offset - self.line_starts[row])
    }
}

/// Line terminator to write into `text`: that of its first line, so one
/// stray `\r\n` in an `\n` file (or the reverse) does not decide it.
#[must_use]
pub fn line_ending(text: &str) -> &'static str {
    match text.find('\n') {
        Some(i) if i > 0 && text.as_bytes()[i - 1] == b'\r' => "\r\n",
        _ => "\n",
    }
}

/// Largest char boundary ≤ `offset`.
#[must_use]
pub fn floor_char_boundary(text: &str, mut offset: usize) -> usize {
    offset = offset.min(text.len());
    while !text.is_char_boundary(offset) {
        offset -= 1;
    }
    offset
}

fn to_u32(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    #[test]
    fn crlf_columns_stop_before_carriage_return() {
        let text = "ab\r\ncd";
        let index = LineIndex::new(text);
        assert_eq!(index.position(text, 3, Encoding::Utf16), Position::new(0, 2));
        assert_eq!(index.offset(text, Position::new(0, 99), Encoding::Utf16), 2);
        assert_eq!(index.offset(text, Position::new(1, 1), Encoding::Utf8), 5);
    }

    #[test]
    fn utf16_counts_surrogate_pairs() {
        let text = "a😀b";
        let index = LineIndex::new(text);
        assert_eq!(index.position(text, 5, Encoding::Utf16), Position::new(0, 3));
        assert_eq!(index.position(text, 5, Encoding::Utf8), Position::new(0, 5));
        // A column inside the surrogate pair snaps to the emoji start.
        assert_eq!(index.offset(text, Position::new(0, 2), Encoding::Utf16), 1);
    }

    #[test]
    fn line_ending_follows_the_first_line() {
        assert_eq!(line_ending("a\r\nb\nc\n"), "\r\n");
        assert_eq!(line_ending("a\nb\r\nc\r\n"), "\n");
        assert_eq!(line_ending("\r\n"), "\r\n");
        assert_eq!(line_ending("no break"), "\n");
    }

    #[test]
    fn out_of_range_positions_clamp() {
        let text = "x\n";
        let index = LineIndex::new(text);
        assert_eq!(index.offset(text, Position::new(7, 0), Encoding::Utf16), 2);
        assert_eq!(index.position(text, 99, Encoding::Utf16), Position::new(1, 0));
    }

    fn text_strategy() -> impl Strategy<Value = String> {
        prop::collection::vec(
            prop_oneof![
                Just("\n".to_owned()),
                Just("\r\n".to_owned()),
                Just("é".to_owned()),
                Just("😀".to_owned()),
                Just("中".to_owned()),
                "[a-z ]{0,3}",
            ],
            0..40,
        )
        .prop_map(|parts| parts.concat())
    }

    proptest! {
        #[test]
        fn utf16_columns_count_code_units(text in text_strategy()) {
            let index = LineIndex::new(&text);
            for (offset, _) in text.char_indices().chain(std::iter::once((text.len(), ' '))) {
                let position = index.position(&text, offset, Encoding::Utf16);
                let line = index.line_range(&text, position.line as usize);
                let expected = text[line.start..offset.min(line.end)].encode_utf16().count();
                prop_assert_eq!(position.character as usize, expected);
                prop_assert_eq!(index.offset(&text, position, Encoding::Utf16), offset.min(line.end));
            }
        }

        #[test]
        fn offsets_round_trip(text in text_strategy()) {
            let index = LineIndex::new(&text);
            for encoding in [Encoding::Utf8, Encoding::Utf16] {
                for (offset, _) in text.char_indices().chain(std::iter::once((text.len(), ' '))) {
                    // Offsets on a `\n` after `\r` are unrepresentable; they snap to the `\r`.
                    let expected = if offset > 0 && text.as_bytes().get(offset) == Some(&b'\n') && text.as_bytes()[offset - 1] == b'\r' { offset - 1 } else { offset };
                    let position = index.position(&text, offset, encoding);
                    prop_assert_eq!(index.offset(&text, position, encoding), expected);
                }
            }
        }

        #[test]
        fn positions_round_trip(text in text_strategy(), line in 0u32..50, character in 0u32..20) {
            let index = LineIndex::new(&text);
            for encoding in [Encoding::Utf8, Encoding::Utf16] {
                let offset = index.offset(&text, Position::new(line, character), encoding);
                prop_assert!(text.is_char_boundary(offset));
                let position = index.position(&text, offset, encoding);
                prop_assert_eq!(index.offset(&text, position, encoding), offset);
            }
        }

        #[test]
        fn edit_matches_a_fresh_index(text in text_strategy(), start in any::<usize>(), len in 0usize..12, insert in text_strategy()) {
            let mut index = LineIndex::new(&text);
            let start = floor_char_boundary(&text, start % (text.len() + 1));
            let end = floor_char_boundary(&text, (start + len).min(text.len()));
            let mut edited = text.clone();
            edited.replace_range(start..end, &insert);
            index.edit(&edited, start..end, insert.len());
            prop_assert_eq!(index, LineIndex::new(&edited));
        }

        #[test]
        fn point_matches_tree_sitter_rows(text in text_strategy()) {
            let index = LineIndex::new(&text);
            for (offset, _) in text.char_indices() {
                let point = index.point(offset);
                let before = &text[..offset];
                prop_assert_eq!(point.row, before.matches('\n').count());
                prop_assert_eq!(point.column, offset - before.rfind('\n').map_or(0, |i| i + 1));
            }
        }
    }
}
