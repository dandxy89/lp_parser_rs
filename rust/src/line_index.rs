//! Index mapping byte offsets to 1-based line/column positions within source text.
//!
//! Built once per input then used for parse diagnostics and for converting
//! constraint/objective `byte_offset` values into human-readable locations.

/// Index mapping byte offsets to 1-based line numbers within source text.
///
/// Built once per input file then used to convert byte offsets into
/// human-readable line/column positions before the input text is dropped.
#[derive(Debug, Clone)]
pub struct LineIndex {
    /// Byte offsets of each line start (index 0 → byte 0, index 1 → first byte after first `'\n'`, etc.).
    line_starts: Vec<usize>,
    /// Total source length in bytes.
    source_len: usize,
}

/// 1-based line and column within source text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourceLocation {
    /// 1-based line number.
    pub line: usize,
    /// 1-based column number (UTF-8 bytes from the start of the line, plus one).
    pub column: usize,
}

impl LineIndex {
    /// Build a line index from the full source text.
    #[must_use]
    pub fn new(source: &str) -> Self {
        let mut line_starts = vec![0];
        for (i, byte) in source.bytes().enumerate() {
            if byte == b'\n' {
                line_starts.push(i + 1);
            }
        }
        Self { line_starts, source_len: source.len() }
    }

    /// Convert a byte offset to a 1-based line number.
    ///
    /// Returns `None` if `byte_offset` is past the end of the source.
    #[must_use]
    pub fn line_number(&self, byte_offset: usize) -> Option<usize> {
        self.location(byte_offset).map(|loc| loc.line)
    }

    /// Convert a byte offset to a 1-based line and column.
    ///
    /// Returns `None` if `byte_offset` is past the end of the source.
    #[must_use]
    pub fn location(&self, byte_offset: usize) -> Option<SourceLocation> {
        if self.line_starts.is_empty() || byte_offset > self.source_len {
            return None;
        }
        // Binary search: find the last line_start <= byte_offset.
        match self.line_starts.binary_search(&byte_offset) {
            Ok(idx) => Some(SourceLocation { line: idx + 1, column: 1 }),
            Err(idx) => {
                if idx == 0 {
                    None
                } else {
                    let line_start = self.line_starts[idx - 1];
                    Some(SourceLocation { line: idx, column: byte_offset - line_start + 1 })
                }
            }
        }
    }

    /// Extract the source line containing `byte_offset` (without trailing newline).
    #[must_use]
    pub fn line_text<'a>(&self, source: &'a str, byte_offset: usize) -> Option<&'a str> {
        let loc = self.location(byte_offset)?;
        let line_idx = loc.line - 1;
        let start = self.line_starts.get(line_idx).copied()?;
        let end = self.line_starts.get(line_idx + 1).copied().unwrap_or(source.len());
        let line = source.get(start..end)?;
        Some(line.trim_end_matches(['\r', '\n']))
    }

    /// Format a multi-line diagnostic snippet for a byte offset.
    ///
    /// Example:
    /// ```text
    ///   --> line 3, column 5
    ///    |
    ///  3 |  c1: x + y >=
    ///    |     ^
    /// ```
    #[must_use]
    pub fn format_snippet(&self, source: &str, byte_offset: usize) -> Option<String> {
        let loc = self.location(byte_offset)?;
        let line_text = self.line_text(source, byte_offset)?;
        let line_no = loc.line;
        let col = loc.column;
        let gutter = format!("{line_no}");
        let pad = gutter.len();
        // Column is 1-based; caret sits under the offending byte.
        let caret_pad = col.saturating_sub(1);
        Some(format!(
            "  --> line {line_no}, column {col}\n\
             {blank:pad$} |\n\
             {gutter} | {line_text}\n\
             {blank:pad$} | {caret:>caret_pad$}^",
            blank = "",
            caret = "",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_line_index_basic() {
        let source = "line1\nline2\nline3\n";
        let idx = LineIndex::new(source);
        assert_eq!(idx.line_number(0), Some(1));
        assert_eq!(idx.line_number(6), Some(2));
        assert_eq!(idx.line_number(12), Some(3));
        assert_eq!(idx.line_number(3), Some(1));
        assert_eq!(idx.location(6), Some(SourceLocation { line: 2, column: 1 }));
        assert_eq!(idx.location(8), Some(SourceLocation { line: 2, column: 3 }));
    }

    #[test]
    fn test_line_index_single_line() {
        let source = "no newlines";
        let idx = LineIndex::new(source);
        assert_eq!(idx.line_number(0), Some(1));
        assert_eq!(idx.line_number(5), Some(1));
        assert_eq!(idx.location(5), Some(SourceLocation { line: 1, column: 6 }));
    }

    #[test]
    fn test_snippet() {
        let source = "Minimize\n obj: x\nSubject To\n c1: x >=\nEnd\n";
        let idx = LineIndex::new(source);
        // Offset of 'c' in " c1: x >="
        let offset = source.find("c1").expect("c1 present");
        let snippet = idx.format_snippet(source, offset).expect("snippet");
        assert!(snippet.contains("line 4"), "{snippet}");
        assert!(snippet.contains("c1: x >="), "{snippet}");
        assert!(snippet.contains('^'), "{snippet}");
    }

    #[test]
    fn test_past_end() {
        let source = "abc";
        let idx = LineIndex::new(source);
        assert_eq!(idx.location(3), Some(SourceLocation { line: 1, column: 4 }));
        assert_eq!(idx.location(4), None);
    }
}
