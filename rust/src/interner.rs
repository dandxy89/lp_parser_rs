//! String interning for LP problem names.
//!
//! All variable, constraint, and objective names are stored once in a
//! [`NameInterner`] and referenced by a cheap, copyable [`NameId`].

use rustc_hash::FxHashMap;

/// Opaque handle to an interned name string.
/// Implements `Copy`, `Eq`, `Ord`, `Hash` — suitable for use as `HashMap` key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NameId(u32);

/// Mutable string interner for LP problem names.
///
/// Used during parsing and problem construction. Ids are dense indices in
/// interning order, so `Clone` preserves them.
#[derive(Debug, Default, Clone)]
pub struct NameInterner {
    names: Vec<String>,
    ids: FxHashMap<String, NameId>,
}

impl NameInterner {
    /// Create a new empty interner.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Create an interner pre-sized for the expected number of names.
    #[must_use]
    pub fn with_capacity(capacity: usize) -> Self {
        Self { names: Vec::with_capacity(capacity), ids: FxHashMap::with_capacity_and_hasher(capacity, rustc_hash::FxBuildHasher) }
    }

    /// Intern a string, returning its [`NameId`]. Idempotent — interning
    /// the same string twice returns the same ID.
    ///
    /// # Panics
    ///
    /// Panics if more than `u32::MAX` distinct names are interned.
    #[inline]
    pub fn intern(&mut self, name: &str) -> NameId {
        debug_assert!(!name.is_empty(), "must not intern an empty string");
        if let Some(&id) = self.ids.get(name) {
            return id;
        }
        let id = NameId(u32::try_from(self.names.len()).expect("more than u32::MAX interned names"));
        self.names.push(name.to_owned());
        self.ids.insert(name.to_owned(), id);
        id
    }

    /// Resolve a [`NameId`] back to its string.
    ///
    /// # Panics
    ///
    /// Panics if the key was not produced by this interner.
    #[inline]
    #[must_use]
    pub fn resolve(&self, id: NameId) -> &str {
        &self.names[id.0 as usize]
    }

    /// Try to look up a string without interning it.
    /// Returns `None` if the string has not been interned.
    #[inline]
    #[must_use]
    pub fn get(&self, name: &str) -> Option<NameId> {
        self.ids.get(name).copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intern_and_resolve() {
        let mut interner = NameInterner::new();
        let id = interner.intern("x1");
        assert_eq!(interner.resolve(id), "x1");
    }

    #[test]
    fn idempotent_interning() {
        let mut interner = NameInterner::new();
        let id1 = interner.intern("x1");
        let id2 = interner.intern("x1");
        assert_eq!(id1, id2);
    }

    #[test]
    fn distinct_names_get_distinct_ids() {
        let mut interner = NameInterner::new();
        let id1 = interner.intern("x1");
        let id2 = interner.intern("x2");
        assert_ne!(id1, id2);
    }

    #[test]
    fn get_returns_none_for_unknown() {
        let interner = NameInterner::new();
        assert!(interner.get("unknown").is_none());
    }

    #[test]
    fn get_returns_some_for_known() {
        let mut interner = NameInterner::new();
        let id = interner.intern("x1");
        assert_eq!(interner.get("x1"), Some(id));
    }

    #[test]
    fn clone_preserves_ids() {
        let mut interner = NameInterner::new();
        let id1 = interner.intern("x1");
        let id2 = interner.intern("constraint_with_a_longer_name");
        let cloned = interner.clone();
        assert_eq!(cloned.resolve(id1), "x1");
        assert_eq!(cloned.resolve(id2), "constraint_with_a_longer_name");
        assert_eq!(cloned.get("x1"), Some(id1));
    }

    #[test]
    fn with_capacity_works() {
        let mut interner = NameInterner::with_capacity(100);
        let id = interner.intern("x1");
        assert_eq!(interner.resolve(id), "x1");
    }

    #[test]
    #[should_panic(expected = "must not intern an empty string")]
    fn empty_string_panics() {
        let mut interner = NameInterner::new();
        interner.intern("");
    }
}
