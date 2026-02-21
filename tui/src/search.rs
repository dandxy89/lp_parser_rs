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
                // Use ASCII case-insensitive comparison to avoid per-call allocation.
                searchable_text.as_bytes().windows(lower.len()).any(|w| w.eq_ignore_ascii_case(lower.as_bytes()))
            }
        }
    }

    /// Whether the compiled regex is invalid (for UI highlighting).
    #[cfg(test)]
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

    macro_rules! parse_query_tests {
        ($($name:ident: $input:expr => $mode:expr, $pattern:expr);+ $(;)?) => {
            $(#[test] fn $name() {
                let (mode, pattern) = parse_query($input);
                assert_eq!(mode, $mode);
                assert_eq!(pattern, $pattern);
            })+
        };
    }

    parse_query_tests! {
        parse_fuzzy:           "hello"    => SearchMode::Fuzzy,     "hello";
        parse_regex:           "r:^con.*" => SearchMode::Regex,     "^con.*";
        parse_substring:       "s:exact"  => SearchMode::Substring, "exact";
        parse_regex_empty:     "r:"       => SearchMode::Regex,     "";
        parse_substring_empty: "s:"       => SearchMode::Substring, ""
    }

    macro_rules! match_tests {
        ($($name:ident: $query:expr, $haystack:expr => $expected:expr);+ $(;)?) => {
            $(#[test] fn $name() {
                let search = CompiledSearch::compile($query);
                assert_eq!(search.matches($haystack), $expected,
                    "compile({:?}).matches({:?}) expected {}", $query, $haystack, $expected);
            })+
        };
    }

    match_tests! {
        fuzzy_basic_hit:         "constr",       "my_constraint_1"  => true;
        fuzzy_basic_hit2:        "constr",       "constraint_abc"   => true;
        fuzzy_empty:             "",             "anything"         => true;
        fuzzy_miss:              "zzzzz",        "constraint"       => false;
        regex_hit:               "r:^con.*nt$",  "constraint"       => true;
        regex_miss:              "r:^con.*nt$",  "objective"        => false;
        regex_case_insensitive:  "r:CONSTRAINT", "my_constraint"    => true;
        regex_empty:             "r:",           "anything"         => true;
        substr_hit:              "s:exact",      "some_exact_match" => true;
        substr_miss:             "s:exact",      "something_else"   => false;
        substr_case_insensitive: "s:EXACT",      "some_exact_match" => true;
        substr_empty:            "s:",           "anything"         => true
    }

    #[test]
    fn test_regex_invalid() {
        let search = CompiledSearch::compile("r:[invalid");
        assert!(search.has_regex_error());
        assert!(!search.matches("anything"));
    }

    #[test]
    fn test_matches_variable_names_in_searchable_text() {
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
