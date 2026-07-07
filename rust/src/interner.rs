//! String interning for LP problem names.
//!
//! All variable, constraint, and objective names are stored once in a
//! [`NameInterner`] and referenced by a cheap, copyable [`NameId`].

use std::num::NonZeroUsize;

use lasso::{Capacity, Rodeo, Spur};

/// Opaque handle to an interned name string.
/// Implements `Copy`, `Eq`, `Ord`, `Hash` — suitable for use as `HashMap` key.
pub type NameId = Spur;

/// Mutable string interner for LP problem names.
///
/// Wraps [`lasso::Rodeo`] and provides convenience methods.
/// Used during parsing and problem construction.
#[derive(Debug, Default)]
pub struct NameInterner {
    rodeo: Rodeo,
}

impl Clone for NameInterner {
    /// Deep-copy by re-interning every string in key order.
    ///
    /// `lasso::Rodeo` does not implement `Clone`; re-interning in key order
    /// reproduces identical [`NameId`]s, so ids from the original remain valid
    /// against the clone (required by `LpProblem::clone`).
    fn clone(&self) -> Self {
        let mut cloned = Self::with_capacity(self.rodeo.len());
        for (id, name) in self.rodeo.iter() {
            let new_id = cloned.rodeo.get_or_intern(name);
            debug_assert_eq!(new_id, id, "clone must preserve interner ids");
        }
        cloned
    }
}

impl NameInterner {
    /// Create a new empty interner.
    #[must_use]
    pub fn new() -> Self {
        Self { rodeo: Rodeo::default() }
    }

    /// Create an interner pre-sized for the expected number of names.
    #[must_use]
    pub fn with_capacity(capacity: usize) -> Self {
        // Estimate ~32 bytes average per name for byte capacity; the max(1)
        // keeps the value non-zero even for capacity == 0.
        let bytes = NonZeroUsize::new(capacity.saturating_mul(32).max(1)).unwrap_or(NonZeroUsize::MIN);
        Self { rodeo: Rodeo::with_capacity(Capacity::new(capacity, bytes)) }
    }

    /// Intern a string, returning its [`NameId`]. Idempotent — interning
    /// the same string twice returns the same ID.
    #[inline]
    pub fn intern(&mut self, name: &str) -> NameId {
        debug_assert!(!name.is_empty(), "must not intern an empty string");
        self.rodeo.get_or_intern(name)
    }

    /// Resolve a [`NameId`] back to its string.
    ///
    /// # Panics
    ///
    /// Panics if the key was not produced by this interner.
    #[inline]
    #[must_use]
    pub fn resolve(&self, id: NameId) -> &str {
        self.rodeo.resolve(&id)
    }

    /// Try to look up a string without interning it.
    /// Returns `None` if the string has not been interned.
    #[inline]
    #[must_use]
    pub fn get(&self, name: &str) -> Option<NameId> {
        self.rodeo.get(name)
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
