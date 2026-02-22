/// Index mapping byte offsets to 1-based line numbers within source text.
///
/// Built once per input file then used to convert `Constraint::byte_offset`
/// values into human-readable line numbers before the input text is dropped.
pub struct LineIndex {
    /// Byte offsets of each line start (index 0 → byte 0, index 1 → first byte after first '\n', etc.).
    line_starts: Vec<usize>,
}

impl LineIndex {
    /// Build a line index from the full source text.
    #[must_use]
    pub fn new(source: &str) -> Self {
        debug_assert!(!source.is_empty(), "LineIndex::new called with empty source");
        let mut line_starts = vec![0];
        for (i, byte) in source.bytes().enumerate() {
            if byte == b'\n' {
                line_starts.push(i + 1);
            }
        }
        Self { line_starts }
    }

    /// Convert a byte offset to a 1-based line number.
    ///
    /// Returns `None` if `byte_offset` is past the end of the source.
    #[must_use]
    pub fn line_number(&self, byte_offset: usize) -> Option<usize> {
        if self.line_starts.is_empty() {
            return None;
        }
        // Binary search: find the last line_start <= byte_offset.
        match self.line_starts.binary_search(&byte_offset) {
            Ok(idx) => Some(idx + 1),
            Err(idx) => {
                if idx == 0 {
                    None
                } else {
                    Some(idx) // idx is insertion point; line number is idx (1-based)
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_line_index_basic() {
        let source = "line1\nline2\nline3\n";
        let idx = LineIndex::new(source);
        // "line1" starts at byte 0 → line 1
        assert_eq!(idx.line_number(0), Some(1));
        // "line2" starts at byte 6 → line 2
        assert_eq!(idx.line_number(6), Some(2));
        // "line3" starts at byte 12 → line 3
        assert_eq!(idx.line_number(12), Some(3));
        // Middle of "line1" → still line 1
        assert_eq!(idx.line_number(3), Some(1));
    }

    #[test]
    fn test_line_index_single_line() {
        let source = "no newlines";
        let idx = LineIndex::new(source);
        assert_eq!(idx.line_number(0), Some(1));
        assert_eq!(idx.line_number(5), Some(1));
    }
}
