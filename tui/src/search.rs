//! Search modes and compiled search cache for efficient multi-field matching.
//!
//! Query prefixes select the search mode:
//! - `r:pattern` — Regex (case-insensitive) over entry names
//! - `s:text` — Substring (case-insensitive) over entry names
//! - `c:text` — Substring (case-insensitive) over entry content (referenced
//!   variables, coefficient/RHS values, types) as shown in the detail panel
//! - anything else — Fuzzy (SIMD-accelerated via `frizbee`) over entry names

use regex::Regex;

/// The kind of search being performed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchMode {
    Fuzzy,
    Regex,
    Substring,
    /// Full-text substring search over entry content rather than names.
    Content,
}

impl SearchMode {
    /// Short label for display in the status bar / search indicator.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Fuzzy => "fuzzy",
            Self::Regex => "regex",
            Self::Substring => "substring",
            Self::Content => "content",
        }
    }
}

/// Parse a raw query string into a `(SearchMode, pattern)` pair.
///
/// - `r:pattern` → `(Regex, "pattern")`
/// - `s:text` → `(Substring, "text")`
/// - `c:text` → `(Content, "text")`
/// - anything else → `(Fuzzy, raw)`
#[allow(clippy::option_if_let_else)] // chained if-let is clearer than nested map_or_else
pub fn parse_query(raw: &str) -> (SearchMode, &str) {
    if let Some(pattern) = raw.strip_prefix("r:") {
        (SearchMode::Regex, pattern)
    } else if let Some(text) = raw.strip_prefix("s:") {
        (SearchMode::Substring, text)
    } else if let Some(text) = raw.strip_prefix("c:") {
        (SearchMode::Content, text)
    } else {
        (SearchMode::Fuzzy, raw)
    }
}

/// A compiled non-fuzzy search (regex or substring), built once per query
/// change and reused across all entries. Fuzzy queries are routed to `frizbee`
/// directly by the caller and never reach this type.
pub enum CompiledSearch {
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
            SearchMode::Regex => {
                let result = regex::RegexBuilder::new(pattern).case_insensitive(true).build();
                Self::Regex(result)
            }
            // Content is a substring match over content text. Fuzzy is handled
            // by the caller via frizbee; fall back to a substring match on the
            // whole query if it ever reaches here.
            SearchMode::Substring | SearchMode::Content | SearchMode::Fuzzy => Self::Substring(pattern.to_lowercase()),
        }
    }

    /// Test whether `searchable_text` matches this compiled search.
    pub fn matches(&self, searchable_text: &str) -> bool {
        match self {
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

    /// Compact single-line description of a regex compilation failure, if any.
    ///
    /// `regex::Error`'s `Display` output spans several lines; only the final
    /// `error: ...` line is kept so it fits on a single UI row.
    pub fn regex_error(&self) -> Option<String> {
        match self {
            Self::Regex(Err(error)) => {
                let message = error.to_string();
                let last_line = message.lines().last().unwrap_or(&message);
                Some(last_line.trim_start_matches("error: ").to_owned())
            }
            _ => None,
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
        parse_content:         "c:x14"    => SearchMode::Content,   "x14";
        parse_regex_empty:     "r:"       => SearchMode::Regex,     "";
        parse_substring_empty: "s:"       => SearchMode::Substring, "";
        parse_content_empty:   "c:"       => SearchMode::Content,   ""
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
        assert!(search.regex_error().is_some());
        assert!(!search.matches("anything"));
    }

    #[test]
    fn test_matches_variable_names_in_searchable_text() {
        let searchable = "c1\0x\0flow_var";

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
