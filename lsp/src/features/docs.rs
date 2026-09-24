//! Static documentation shared by hover, completion and signature help:
//! keywords (with the aliases the upstream lexer accepts), general-constraint
//! functions and multi-objective attributes.

use crate::syntax::kind;

/// A keyword of the LP format.
#[derive(Debug)]
pub struct Keyword {
    /// Stable key used in completion `data`.
    pub id: &'static str,
    /// Canonical spelling.
    pub label: &'static str,
    /// Tree-sitter kind of the keyword token.
    pub node_kind: &'static str,
    /// Every accepted spelling, lower case (matching is case-insensitive).
    pub aliases: &'static [&'static str],
    /// Short description (markdown).
    pub summary: &'static str,
    /// Snippet inserting the header plus an indented first line, for section
    /// headers.
    pub snippet: Option<&'static str>,
    /// Single-word keyword from the external scanner: a keyword only as the
    /// first token of a line and not before `:`.
    pub line_start_only: bool,
}

/// Every keyword. Section headers come first, in canonical order.
pub const KEYWORDS: &[Keyword] = &[
    Keyword {
        id: "minimize",
        label: "Minimize",
        node_kind: kind::SENSE,
        aliases: &["minimize", "minimise", "minimum", "min"],
        summary: "Objective sense: minimise the objective function(s) that follow.",
        snippet: Some("Minimize\n  ${1:obj}: $0"),
        line_start_only: false,
    },
    Keyword {
        id: "maximize",
        label: "Maximize",
        node_kind: kind::SENSE,
        aliases: &["maximize", "maximise", "maximum", "max"],
        summary: "Objective sense: maximise the objective function(s) that follow.",
        snippet: Some("Maximize\n  ${1:obj}: $0"),
        line_start_only: false,
    },
    Keyword {
        id: "subject_to",
        label: "Subject To",
        node_kind: kind::SUBJECT_TO_KEYWORD,
        aliases: &["subject to", "such that", "s.t.", "st"],
        summary: "Starts the constraints section: `name: expression op rhs`, ranged `lo <= expression <= hi`, \
                  indicator `b = 1 -> expression op rhs`.",
        snippet: Some("Subject To\n  ${1:c1}: $0"),
        line_start_only: false,
    },
    Keyword {
        id: "lazy_constraints",
        label: "Lazy Constraints",
        node_kind: kind::LAZY_CONSTRAINTS_KEYWORD,
        aliases: &["lazy constraints"],
        summary: "Constraints the solver may add lazily, only when a candidate solution violates them.",
        snippet: Some("Lazy Constraints\n  ${1:l1}: $0"),
        line_start_only: false,
    },
    Keyword {
        id: "user_cuts",
        label: "User Cuts",
        node_kind: kind::USER_CUTS_KEYWORD,
        aliases: &["user cuts"],
        summary: "Cutting planes that tighten the relaxation without removing integer-feasible solutions.",
        snippet: Some("User Cuts\n  ${1:u1}: $0"),
        line_start_only: false,
    },
    Keyword {
        id: "general_constraints",
        label: "General Constraints",
        node_kind: kind::GENERAL_CONSTRAINTS_KEYWORD,
        aliases: &["general constraints", "general constraint", "general constrs", "general constr", "gen cons", "genconstrs", "genconstr"],
        summary: "Gurobi general constraints: `name: r = MAX ( x , y , 3 )` with `MAX`, `MIN`, `ABS`, `AND` or `OR`.",
        snippet: Some("General Constraints\n  ${1:g1}: ${2:r} = ${3|MAX,MIN,ABS,AND,OR|} ( $0 )"),
        line_start_only: false,
    },
    Keyword {
        id: "bounds",
        label: "Bounds",
        node_kind: kind::BOUNDS_KEYWORD,
        aliases: &["bounds", "bound"],
        summary: "Variable bounds: `x <= 10`, `-5 <= y <= 5`, `z free`. Variables default to `0 <= x <= +inf`.",
        snippet: Some("Bounds\n  $0"),
        line_start_only: true,
    },
    Keyword {
        id: "generals",
        label: "Generals",
        node_kind: kind::GENERALS_KEYWORD,
        aliases: &["generals", "general", "gen"],
        summary: "Variables restricted to integer values.",
        snippet: Some("Generals\n  $0"),
        line_start_only: true,
    },
    Keyword {
        id: "integers",
        label: "Integers",
        node_kind: kind::INTEGERS_KEYWORD,
        aliases: &["integers", "integer"],
        summary: "Variables restricted to integer values (same as `Generals`).",
        snippet: Some("Integers\n  $0"),
        line_start_only: true,
    },
    Keyword {
        id: "binaries",
        label: "Binaries",
        node_kind: kind::BINARIES_KEYWORD,
        aliases: &["binaries", "binary", "bin"],
        summary: "Variables restricted to 0 or 1.",
        snippet: Some("Binaries\n  $0"),
        line_start_only: true,
    },
    Keyword {
        id: "semi_continuous",
        label: "Semi-Continuous",
        node_kind: kind::SEMI_CONTINUOUS_KEYWORD,
        aliases: &["semi-continuous", "semis", "semi"],
        summary: "Semi-continuous variables: either 0 or between their lower and upper bound.",
        snippet: Some("Semi-Continuous\n  $0"),
        line_start_only: true,
    },
    Keyword {
        id: "sos",
        label: "SOS",
        node_kind: kind::SOS_KEYWORD,
        aliases: &["sos"],
        summary: "Special ordered sets: `s1: S1 :: x : 1 y : 2` (a header, then `variable : weight` entries).",
        snippet: Some("SOS\n  ${1:s1}: ${2|S1,S2|} :: $0"),
        line_start_only: true,
    },
    Keyword {
        id: "end",
        label: "End",
        node_kind: kind::END_MARKER,
        aliases: &["end"],
        summary: "End of the model; anything after it is ignored.",
        snippet: None,
        line_start_only: true,
    },
    Keyword {
        id: "multi_objectives",
        label: "multi-objectives",
        node_kind: kind::MULTI_OBJECTIVES_KEYWORD,
        aliases: &["multi-objectives", "multi-objective"],
        summary: "Declares several named objectives, each with optional `Priority`, `Weight`, `AbsTol` and `RelTol` attributes.",
        snippet: None,
        line_start_only: false,
    },
    Keyword {
        id: "free",
        label: "free",
        node_kind: kind::FREE_KEYWORD,
        aliases: &["free"],
        summary: "Removes both bounds of a variable: `-inf <= x <= +inf`.",
        snippet: None,
        line_start_only: false,
    },
    Keyword {
        id: "s1",
        label: "S1",
        node_kind: kind::SOS_TYPE,
        aliases: &["s1"],
        summary: "SOS type 1: at most one variable of the set may be non-zero.",
        snippet: None,
        line_start_only: false,
    },
    Keyword {
        id: "s2",
        label: "S2",
        node_kind: kind::SOS_TYPE,
        aliases: &["s2"],
        summary: "SOS type 2: at most two variables may be non-zero, and they must be adjacent in weight order.",
        snippet: None,
        line_start_only: false,
    },
];

/// A general-constraint function.
#[derive(Debug)]
pub struct Function {
    /// Upper-case name.
    pub name: &'static str,
    /// Parameters `(label, description)`; a trailing `...` repeats.
    pub params: &'static [(&'static str, &'static str)],
    /// Semantics (markdown).
    pub summary: &'static str,
}

/// The functions upstream `GeneralFunction` supports.
pub const FUNCTIONS: &[Function] = &[
    Function {
        name: "MAX",
        params: &[("x1", "a variable"), ("x2", "a variable"), ("...", "more variables and/or one constant")],
        summary: "`r = MAX ( x1 , x2 , ... , c )`: `r` equals the largest argument. At least one variable; \
                  several constants fold to their maximum.",
    },
    Function {
        name: "MIN",
        params: &[("x1", "a variable"), ("x2", "a variable"), ("...", "more variables and/or one constant")],
        summary: "`r = MIN ( x1 , x2 , ... , c )`: `r` equals the smallest argument. At least one variable; \
                  several constants fold to their minimum.",
    },
    Function {
        name: "ABS",
        params: &[("x", "the variable")],
        summary: "`r = ABS ( x )`: `r` equals the absolute value of exactly one variable (no constants).",
    },
    Function {
        name: "AND",
        params: &[("b1", "a binary variable"), ("b2", "a binary variable"), ("...", "more binary variables")],
        summary: "`r = AND ( b1 , b2 , ... )`: `r` is 1 exactly when every binary argument is 1 (variables only).",
    },
    Function {
        name: "OR",
        params: &[("b1", "a binary variable"), ("b2", "a binary variable"), ("...", "more binary variables")],
        summary: "`r = OR ( b1 , b2 , ... )`: `r` is 1 when at least one binary argument is 1 (variables only).",
    },
];

/// Multi-objective attributes: `(name, description)`.
pub const ATTRIBUTES: &[(&str, &str)] = &[
    ("Priority", "Integer priority: objectives with higher priority are optimised first (hierarchical)."),
    ("Weight", "Weight of this objective when blending objectives of equal priority."),
    ("AbsTol", "Absolute degradation allowed in this objective when optimising lower-priority ones (non-negative)."),
    ("RelTol", "Relative degradation allowed in this objective when optimising lower-priority ones (non-negative)."),
];

/// Comparison operators offered by completion: `(operator, description)`.
pub const OPERATORS: &[(&str, &str)] = &[
    ("<=", "Less than or equal (alias `=<`)."),
    (">=", "Greater than or equal (alias `=>`)."),
    ("=", "Equal."),
    ("<", "Less than (read as `<=`)."),
    (">", "Greater than (read as `>=`)."),
];

/// Keyword by id.
#[must_use]
pub fn keyword(id: &str) -> Option<&'static Keyword> {
    KEYWORDS.iter().find(|k| k.id == id)
}

/// Keyword for a keyword token of `node_kind` spelled `text`.
#[must_use]
pub fn keyword_for_token(node_kind: &str, text: &str) -> Option<&'static Keyword> {
    let lower = text.to_ascii_lowercase();
    let mut candidates = KEYWORDS.iter().filter(|k| k.node_kind == node_kind);
    match node_kind {
        kind::SENSE => candidates.find(|k| lower.starts_with(&k.aliases[0][..3])),
        kind::SOS_TYPE => candidates.find(|k| k.aliases[0] == lower),
        _ => candidates.next(),
    }
}

/// Function by name, case-insensitively.
#[must_use]
pub fn function(name: &str) -> Option<&'static Function> {
    FUNCTIONS.iter().find(|f| f.name.eq_ignore_ascii_case(name))
}

/// Attribute description by name, case-insensitively.
#[must_use]
pub fn attribute(name: &str) -> Option<(&'static str, &'static str)> {
    ATTRIBUTES.iter().copied().find(|(n, _)| n.eq_ignore_ascii_case(name))
}

/// Markdown for a keyword.
#[must_use]
pub fn keyword_markdown(keyword: &Keyword) -> String {
    let aliases: Vec<String> = keyword.aliases.iter().map(|a| format!("`{a}`")).collect();
    let mut out =
        format!("**{}** (keyword)\n\n{}\n\nAccepted spellings (case-insensitive): {}", keyword.label, keyword.summary, aliases.join(", "));
    if keyword.line_start_only {
        out.push_str("\n\nA keyword only as the first token of a line and not followed by `:`.");
    }
    out
}

impl Function {
    /// Signature label and the byte offsets of each parameter within it.
    #[must_use]
    pub fn signature(&self) -> (String, Vec<[u32; 2]>) {
        debug_assert!(!self.params.is_empty(), "every function takes at least one argument");
        let mut label = format!("{} ( ", self.name);
        let mut offsets = Vec::with_capacity(self.params.len());
        for (i, (param, _)) in self.params.iter().enumerate() {
            if i > 0 {
                label.push_str(" , ");
            }
            // Labels are short ASCII literals, so byte and UTF-16 offsets agree.
            let start = u32::try_from(label.len()).unwrap_or(u32::MAX);
            label.push_str(param);
            offsets.push([start, start.saturating_add(u32::try_from(param.len()).unwrap_or(u32::MAX))]);
        }
        label.push_str(" )");
        (label, offsets)
    }

    /// Markdown description.
    #[must_use]
    pub fn markdown(&self) -> String {
        format!("```lp\nr = {}\n```\n\n{}", self.signature().0, self.summary)
    }
}

/// Markdown for a completion item's `data` key (`keyword:<id>`,
/// `function:<name>`, `attribute:<name>`, `operator:<op>`).
#[must_use]
pub fn describe(key: &str) -> Option<String> {
    let (group, name) = key.split_once(':')?;
    match group {
        "keyword" => keyword(name).map(keyword_markdown),
        "function" => function(name).map(Function::markdown),
        "attribute" => attribute(name).map(|(n, d)| format!("**{n}** (objective attribute)\n\n{d}")),
        "operator" => OPERATORS.iter().find(|(op, _)| *op == name).map(|(op, d)| format!("`{op}`: {d}")),
        _ => None,
    }
}

/// The section header starting `line` (leading whitespace allowed), if any,
/// with the byte length of the header within `line`.
#[must_use]
pub fn section_header(line: &str) -> Option<(&'static Keyword, usize)> {
    let indent = line.len() - line.trim_start().len();
    let rest = &line[indent..];
    let mut best: Option<(&'static Keyword, usize)> = None;
    for keyword in KEYWORDS.iter().filter(|k| k.snippet.is_some() || k.id == "end") {
        for alias in keyword.aliases {
            let Some(len) = match_alias(rest, alias) else { continue };
            let after = rest[len..].trim_start();
            if keyword.line_start_only && after.starts_with(':') {
                continue;
            }
            if best.is_none_or(|(_, l)| len > l) {
                best = Some((keyword, len));
            }
        }
    }
    best.map(|(k, len)| (k, indent + len))
}

/// Length of `text`'s prefix matching `alias` case-insensitively (spaces in
/// the alias match runs of blanks), ending at a word boundary.
fn match_alias(text: &str, alias: &str) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut i = 0;
    for &a in alias.as_bytes() {
        if a == b' ' {
            let start = i;
            while i < bytes.len() && (bytes[i] == b' ' || bytes[i] == b'\t') {
                i += 1;
            }
            if i == start {
                return None;
            }
        } else if bytes.get(i).is_some_and(|b| b.eq_ignore_ascii_case(&a)) {
            i += 1;
        } else {
            return None;
        }
    }
    let boundary = alias.ends_with('.') || bytes.get(i).is_none_or(|&b| !is_name_byte(b));
    boundary.then_some(i)
}

/// Whether `b` can appear in an identifier (the upstream name regex, `-`
/// included mid-name).
#[must_use]
pub const fn is_name_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric()
        || matches!(
            b,
            b'_' | b'!'
                | b'#'
                | b'$'
                | b'%'
                | b'&'
                | b'('
                | b')'
                | b','
                | b'.'
                | b';'
                | b'?'
                | b'@'
                | b'{'
                | b'}'
                | b'~'
                | b'\''
                | b'['
                | b']'
                | b'|'
                | b'-'
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_section_headers() {
        let id = |line: &str| section_header(line).map(|(k, _)| k.id);
        assert_eq!(id("Subject To"), Some("subject_to"));
        assert_eq!(id("  s.t."), Some("subject_to"));
        assert_eq!(id("general constraints"), Some("general_constraints"));
        assert_eq!(id("GENERAL"), Some("generals"));
        assert_eq!(id("bin"), Some("binaries"));
        assert_eq!(id("bin: x >= 1"), None);
        assert_eq!(id("binx"), None);
        assert_eq!(id("semi-continuous"), Some("semi_continuous"));
        assert_eq!(id("Maximise"), Some("maximize"));
        assert_eq!(section_header("Bounds x").map(|(_, n)| n), Some(6));
    }

    #[test]
    fn signature_offsets_cover_params() {
        let (label, offsets) = function("max").unwrap().signature();
        assert_eq!(label, "MAX ( x1 , x2 , ... )");
        let [s, e] = offsets[1];
        assert_eq!(&label[s as usize..e as usize], "x2");
    }

    #[test]
    fn describes_keys() {
        assert!(describe("keyword:bounds").unwrap().contains("`bound`"));
        assert!(describe("function:ABS").unwrap().contains("absolute"));
        assert!(describe("attribute:Priority").is_some());
        assert_eq!(describe("nonsense"), None);
    }
}
