//! Terminal-capability switches that change which glyphs and how much motion the UI uses.
//!
//! Both are process-wide and set once at startup, from `--ascii` / `--reduce-motion`
//! (and, for ASCII, a non-UTF-8 locale).

use std::sync::atomic::{AtomicBool, Ordering};

static ASCII: AtomicBool = AtomicBool::new(false);
static REDUCE_MOTION: AtomicBool = AtomicBool::new(false);

pub fn set_ascii(on: bool) {
    ASCII.store(on, Ordering::Relaxed);
}

pub fn ascii() -> bool {
    ASCII.load(Ordering::Relaxed)
}

pub fn set_reduce_motion(on: bool) {
    REDUCE_MOTION.store(on, Ordering::Relaxed);
}

pub fn reduce_motion() -> bool {
    REDUCE_MOTION.load(Ordering::Relaxed)
}

/// The marker for a renamed entry (`⇄`).
pub fn renamed() -> &'static str {
    if ascii() { ">" } else { "\u{21c4}" }
}

/// A filled status dot (`●`).
pub fn dot() -> &'static str {
    if ascii() { "*" } else { "\u{25cf}" }
}

/// A completion tick (`✓`).
pub fn tick() -> &'static str {
    if ascii() { "v" } else { "\u{2713}" }
}

/// Whether the locale (`LC_ALL`, then `LC_CTYPE`, then `LANG`; the first one set wins)
/// names a non-UTF-8 charset. An unset locale is taken to be UTF-8.
pub fn locale_is_not_utf8(lc_all: Option<&str>, lc_ctype: Option<&str>, lang: Option<&str>) -> bool {
    [lc_all, lc_ctype, lang].into_iter().flatten().find(|value| !value.is_empty()).is_some_and(|value| {
        let lower = value.to_ascii_lowercase();
        !(lower.contains("utf-8") || lower.contains("utf8"))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_locale_variable_set_decides() {
        assert!(!locale_is_not_utf8(None, None, None), "unset is UTF-8");
        assert!(!locale_is_not_utf8(None, None, Some("en_GB.UTF-8")));
        assert!(!locale_is_not_utf8(Some("C.utf8"), None, Some("C")), "LC_ALL outranks LANG");
        assert!(locale_is_not_utf8(None, None, Some("C")));
        assert!(locale_is_not_utf8(Some("POSIX"), None, Some("en_GB.UTF-8")));
        assert!(!locale_is_not_utf8(Some(""), None, Some("en_GB.UTF-8")), "empty counts as unset");
    }
}
