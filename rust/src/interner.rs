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
/// Used during parsing and problem construction. For a read-only
/// resolver after parsing is complete, see [`NameResolver`].
#[derive(Debug, Default)]
pub struct NameInterner {
    rodeo: Rodeo,
}

impl NameInterner {
    /// Create a new empty interner.
    #[must_use]
    pub fn new() -> Self {
        Self { rodeo: Rodeo::default() }
    }

    /// Create an interner pre-sized for the expected number of names.
    ///
    /// # Panics
    ///
    /// Panics if the computed byte capacity overflows (should not happen in practice).
    #[must_use]
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            // Estimate ~32 bytes average per name for byte capacity
            rodeo: Rodeo::with_capacity(Capacity::new(
                capacity,
                // SAFETY: 32 is non-zero
                NonZeroUsize::new(capacity.saturating_mul(32).max(1)).expect("capacity overflow"),
            )),
        }
    }

    /// Intern a string, returning its [`NameId`]. Idempotent — interning
    /// the same string twice returns the same ID.
    #[inline]
    pub fn intern(&mut self, name: &str) -> NameId {
        debug_assert!(!name.is_empty(), "must not intern an empty string");
        self.rodeo.get_or_intern(name)
    }

    /// Intern a static string (avoids allocation for compile-time constants).
    #[inline]
    pub fn intern_static(&mut self, name: &'static str) -> NameId {
        debug_assert!(!name.is_empty(), "must not intern an empty string");
        self.rodeo.get_or_intern_static(name)
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

    /// Number of interned strings.
    #[inline]
    #[must_use]
    pub fn len(&self) -> usize {
        self.rodeo.len()
    }

    /// Returns `true` if no strings have been interned.
    #[inline]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.rodeo.is_empty()
    }

    /// Consume the interner into a read-only resolver.
    /// Useful after parsing is complete — the resolver is `Send + Sync`.
    #[must_use]
    pub fn into_resolver(self) -> NameResolver {
        NameResolver { resolver: self.rodeo.into_resolver() }
    }
}

/// Read-only name resolver produced by [`NameInterner::into_resolver`].
///
/// `Send + Sync` — safe to share across threads.
#[derive(Debug)]
pub struct NameResolver {
    resolver: lasso::RodeoResolver,
}

impl NameResolver {
    /// Resolve a [`NameId`] to its string.
    ///
    /// # Panics
    ///
    /// Panics if the key was not produced by the interner that created this resolver.
    #[inline]
    #[must_use]
    pub fn resolve(&self, id: NameId) -> &str {
        self.resolver.resolve(&id)
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
        assert_eq!(interner.len(), 1);
    }

    #[test]
    fn distinct_names_get_distinct_ids() {
        let mut interner = NameInterner::new();
        let id1 = interner.intern("x1");
        let id2 = interner.intern("x2");
        assert_ne!(id1, id2);
        assert_eq!(interner.len(), 2);
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
    fn into_resolver_works() {
        let mut interner = NameInterner::new();
        let id = interner.intern("x1");
        let resolver = interner.into_resolver();
        assert_eq!(resolver.resolve(id), "x1");
    }

    #[test]
    fn with_capacity_works() {
        let mut interner = NameInterner::with_capacity(100);
        let id = interner.intern("x1");
        assert_eq!(interner.resolve(id), "x1");
    }

    #[test]
    fn intern_static_works() {
        let mut interner = NameInterner::new();
        let id = interner.intern_static("static_name");
        assert_eq!(interner.resolve(id), "static_name");
    }

    #[test]
    #[should_panic(expected = "must not intern an empty string")]
    fn empty_string_panics() {
        let mut interner = NameInterner::new();
        interner.intern("");
    }
}
