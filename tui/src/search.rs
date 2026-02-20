//! Search modes and compiled search cache for efficient multi-field matching.
//!
//! Query prefixes select the search mode:
//! - `r:pattern` — Regex (case-insensitive)
//! - `s:text` — Substring (case-insensitive)
//! - anything else — Fuzzy (SIMD-accelerated via `frizbee`)

use regex::Regex;

/// The kind of search being performed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchMode {
    Fuzzy,
    Regex,
    Substring,
}

impl SearchMode {
    /// Short label for display in the status bar / search indicator.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Fuzzy => "fuzzy",
            Self::Regex => "regex",
            Self::Substring => "substring",
        }
    }
}

/// Parse a raw query string into a `(SearchMode, pattern)` pair.
///
/// - `r:pattern` → `(Regex, "pattern")`
/// - `s:text` → `(Substring, "text")`
/// - anything else → `(Fuzzy, raw)`
#[allow(clippy::option_if_let_else)] // chained if-let is clearer than nested map_or_else
pub fn parse_query(raw: &str) -> (SearchMode, &str) {
    if let Some(pattern) = raw.strip_prefix("r:") {
        (SearchMode::Regex, pattern)
    } else if let Some(text) = raw.strip_prefix("s:") {
        (SearchMode::Substring, text)
    } else {
        (SearchMode::Fuzzy, raw)
    }
}

/// A compiled search, built once per query change and reused across all entries.
pub enum CompiledSearch {
    /// Fuzzy match via `frizbee`. Stores the needle pattern for per-entry matching.
    Fuzzy(String),
    /// Compiled case-insensitive regex.
    Regex(Result<Regex, regex::Error>),
    /// Lowercased substring for case-insensitive contains check.
    Substring(String),
}

impl CompiledSearch {
    /// Compile a raw query string into a `CompiledSearch`.
    pub fn compile(raw: &str) -> Self {
        let (mode, pattern) = parse_query(raw);
        match mode {
            SearchMode::Fuzzy => Self::Fuzzy(pattern.to_string()),
            SearchMode::Regex => {
                let result = regex::RegexBuilder::new(pattern).case_insensitive(true).build();
                Self::Regex(result)
            }
            SearchMode::Substring => Self::Substring(pattern.to_lowercase()),
        }
    }

    /// Test whether `searchable_text` matches this compiled search.
    pub fn matches(&self, searchable_text: &str) -> bool {
        match self {
            Self::Fuzzy(needle) => {
                if needle.is_empty() {
                    return true;
                }
                let config = frizbee::Config::default();
                let haystacks = [searchable_text];
                let results = frizbee::match_list(needle, &haystacks, &config);
                !results.is_empty()
            }
            Self::Regex(Ok(re)) => re.is_match(searchable_text),
            Self::Regex(Err(_)) => {
                // Invalid regex matches nothing.
                false
            }
            Self::Substring(lower) => {
                if lower.is_empty() {
                    return true;
                }
                searchable_text.to_lowercase().contains(lower.as_str())
            }
        }
    }

    /// Whether the compiled regex is invalid (for UI highlighting).
    pub const fn has_regex_error(&self) -> bool {
        matches!(self, Self::Regex(Err(_)))
    }
}

impl std::fmt::Debug for CompiledSearch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Fuzzy(needle) => write!(f, "CompiledSearch::Fuzzy({needle:?})"),
            Self::Regex(Ok(re)) => write!(f, "CompiledSearch::Regex({re})"),
            Self::Regex(Err(e)) => write!(f, "CompiledSearch::Regex(Err({e}))"),
            Self::Substring(s) => write!(f, "CompiledSearch::Substring({s:?})"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_query_fuzzy() {
        let (mode, pattern) = parse_query("hello");
        assert_eq!(mode, SearchMode::Fuzzy);
        assert_eq!(pattern, "hello");
    }

    #[test]
    fn test_parse_query_regex() {
        let (mode, pattern) = parse_query("r:^con.*");
        assert_eq!(mode, SearchMode::Regex);
        assert_eq!(pattern, "^con.*");
    }

    #[test]
    fn test_parse_query_substring() {
        let (mode, pattern) = parse_query("s:exact");
        assert_eq!(mode, SearchMode::Substring);
        assert_eq!(pattern, "exact");
    }

    #[test]
    fn test_parse_query_empty_prefix() {
        let (mode, pattern) = parse_query("r:");
        assert_eq!(mode, SearchMode::Regex);
        assert_eq!(pattern, "");

        let (mode, pattern) = parse_query("s:");
        assert_eq!(mode, SearchMode::Substring);
        assert_eq!(pattern, "");
    }

    #[test]
    fn test_fuzzy_match_basic() {
        let search = CompiledSearch::compile("constr");
        assert!(search.matches("my_constraint_1"));
        assert!(search.matches("constraint_abc"));
    }

    #[test]
    fn test_fuzzy_match_empty_query() {
        let search = CompiledSearch::compile("");
        assert!(search.matches("anything"));
    }

    #[test]
    fn test_fuzzy_match_no_match() {
        let search = CompiledSearch::compile("zzzzz");
        assert!(!search.matches("constraint"));
    }

    #[test]
    fn test_regex_match() {
        let search = CompiledSearch::compile("r:^con.*nt$");
        assert!(search.matches("constraint"));
        assert!(!search.matches("objective"));
    }

    #[test]
    fn test_regex_case_insensitive() {
        let search = CompiledSearch::compile("r:CONSTRAINT");
        assert!(search.matches("my_constraint"));
    }

    #[test]
    fn test_regex_invalid() {
        let search = CompiledSearch::compile("r:[invalid");
        assert!(search.has_regex_error());
        assert!(!search.matches("anything"));
    }

    #[test]
    fn test_regex_empty_pattern() {
        let search = CompiledSearch::compile("r:");
        assert!(!search.has_regex_error());
        // Empty regex matches everything.
        assert!(search.matches("anything"));
    }

    #[test]
    fn test_substring_match() {
        let search = CompiledSearch::compile("s:exact");
        assert!(search.matches("some_exact_match"));
        assert!(!search.matches("something_else"));
    }

    #[test]
    fn test_substring_case_insensitive() {
        let search = CompiledSearch::compile("s:EXACT");
        assert!(search.matches("some_exact_match"));
    }

    #[test]
    fn test_substring_empty_query() {
        let search = CompiledSearch::compile("s:");
        assert!(search.matches("anything"));
    }

    #[test]
    fn test_matches_variable_names_in_searchable_text() {
        // Simulates a constraint with name "c1" and variables "x", "flow_var"
        let searchable = "c1\0x\0flow_var";

        let fuzzy = CompiledSearch::compile("flow");
        assert!(fuzzy.matches(searchable));

        let substr = CompiledSearch::compile("s:flow_var");
        assert!(substr.matches(searchable));

        let regex = CompiledSearch::compile("r:flow_var");
        assert!(regex.matches(searchable));
    }

    #[test]
    fn test_search_mode_labels() {
        assert_eq!(SearchMode::Fuzzy.label(), "fuzzy");
        assert_eq!(SearchMode::Regex.label(), "regex");
        assert_eq!(SearchMode::Substring.label(), "substring");
    }
}
