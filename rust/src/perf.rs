//! Performance optimisations for LP parsing.
//!
//! This module contains optimised implementations of common parsing operations
//! using efficient algorithms and data structures.

use std::collections::HashMap;
use std::sync::OnceLock;

use memchr::memmem;
use nom::error::{Error, ErrorKind};
use nom::{Err, IResult};
use smallvec::SmallVec;

/// Cache for compiled search patterns
static PATTERN_CACHE: OnceLock<std::sync::RwLock<HashMap<String, CompiledPattern>>> = OnceLock::new();

/// Compiled pattern for efficient searching
#[derive(Clone)]
struct CompiledPattern {
    /// Original pattern
    pattern: String,
}

impl CompiledPattern {
    /// Create a new compiled pattern
    fn new(pattern: &str) -> Self {
        Self { pattern: pattern.to_string() }
    }
}

/// Optimized case-insensitive string finder
pub struct FastStringFinder {
    patterns: SmallVec<[CompiledPattern; 8]>,
}

impl FastStringFinder {
    /// Create a new finder for multiple patterns
    pub fn new(patterns: &[&str]) -> Self {
        let mut compiled_patterns = SmallVec::new();

        for &pattern in patterns {
            // Check cache first
            let cache_key = pattern.to_string();

            let compiled = {
                let cache = PATTERN_CACHE.get_or_init(|| std::sync::RwLock::new(HashMap::new())).read().unwrap();
                cache.get(&cache_key).cloned()
            };

            let compiled = compiled.unwrap_or_else(|| {
                let new_pattern = CompiledPattern::new(pattern);

                // Store in cache
                let mut cache = PATTERN_CACHE.get_or_init(|| std::sync::RwLock::new(HashMap::new())).write().unwrap();
                cache.insert(cache_key, new_pattern.clone());

                new_pattern
            });

            compiled_patterns.push(compiled);
        }

        Self { patterns: compiled_patterns }
    }

    #[must_use]
    /// Find the first occurrence of any pattern in the input
    pub fn find_first(&self, input: &str) -> Option<(usize, &str)> {
        let mut earliest_pos = usize::MAX;
        let mut found_pattern = None;

        for pattern in &self.patterns {
            if let Some(pos) = Self::find_pattern_at_word_boundary(input, &pattern.pattern) {
                if pos < earliest_pos {
                    earliest_pos = pos;
                    found_pattern = Some(&pattern.pattern);
                }
            }
        }

        found_pattern.map(|pattern| (earliest_pos, pattern.as_str()))
    }

    /// Find pattern only at word boundaries (start of line or after whitespace)
    fn find_pattern_at_word_boundary(input: &str, pattern: &str) -> Option<usize> {
        let mut start = 0;
        while let Some(pos) = Self::find_case_insensitive(&input[start..], pattern) {
            let absolute_pos = start + pos;

            // Check if this is at a word boundary
            let is_at_boundary = absolute_pos == 0 || input.chars().nth(absolute_pos - 1).is_some_and(|c| c.is_whitespace() || c == '\n');

            // Check if the pattern ends at a word boundary or end of input
            let pattern_end = absolute_pos + pattern.len();
            let is_end_boundary =
                pattern_end >= input.len() || input.chars().nth(pattern_end).map_or(true, |c| c.is_whitespace() || c == '\n' || c == ':');

            if is_at_boundary && is_end_boundary {
                return Some(absolute_pos);
            }

            start = absolute_pos + 1;
        }

        None
    }

    /// Efficient case-insensitive search without string allocations
    fn find_case_insensitive(haystack: &str, needle: &str) -> Option<usize> {
        if needle.is_empty() {
            return Some(0);
        }

        let haystack_bytes = haystack.as_bytes();
        let needle_bytes = needle.as_bytes();

        if haystack_bytes.len() < needle_bytes.len() {
            return None;
        }

        for i in 0..=(haystack_bytes.len() - needle_bytes.len()) {
            let mut matches = true;
            for j in 0..needle_bytes.len() {
                let h_byte = haystack_bytes[i + j];
                let n_byte = needle_bytes[j];

                // ASCII case-insensitive comparison
                let h_lower = if h_byte.is_ascii_uppercase() { h_byte + 32 } else { h_byte };
                let n_lower = if n_byte.is_ascii_uppercase() { n_byte + 32 } else { n_byte };

                if h_lower != n_lower {
                    matches = false;
                    break;
                }
            }
            if matches {
                return Some(i);
            }
        }
        None
    }
}

#[inline]
#[must_use]
/// Fast case-insensitive string search using optimised algorithms
pub fn fast_find_case_insensitive(haystack: &str, needle: &str) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }

    let haystack_lower = haystack.to_lowercase();
    let needle_lower = needle.to_lowercase();

    memmem::find(haystack_lower.as_bytes(), needle_lower.as_bytes())
}

/// Optimized version of `take_until_cased` using fast string search
pub fn fast_take_until_cased<'a>(tag: &'a str) -> impl Fn(&'a str) -> IResult<&'a str, &'a str> + 'a {
    move |input: &str| {
        fast_find_case_insensitive(input, tag)
            .map_or_else(|| Err(Err::Error(Error::new(input, ErrorKind::TakeUntil))), |pos| Ok((&input[pos..], &input[..pos])))
    }
}

/// Optimized version of `take_until_parser` using fast string search
pub fn fast_take_until_parser<'a>(tags: &'a [&'a str]) -> impl Fn(&'a str) -> IResult<&'a str, &'a str> + 'a {
    move |input| {
        let finder = FastStringFinder::new(tags);

        if let Some((pos, _matched_tag)) = finder.find_first(input) {
            Ok((&input[pos..], &input[..pos]))
        } else {
            Err(Err::Error(Error::new(input, ErrorKind::Alt)))
        }
    }
}

/// Memory pool for reusing allocations
pub struct MemoryPool<T> {
    pool: std::sync::Mutex<Vec<T>>,
    factory: fn() -> T,
}

impl<T> MemoryPool<T> {
    /// Create a new memory pool
    pub fn new(factory: fn() -> T) -> Self {
        Self { pool: std::sync::Mutex::new(Vec::new()), factory }
    }

    /// Get an item from the pool or create a new one
    pub fn get(&self) -> PooledItem<'_, T> {
        let item = {
            let mut pool = self.pool.lock().unwrap();
            pool.pop().unwrap_or_else(|| (self.factory)())
        };

        PooledItem { item: Some(item), pool: &self.pool }
    }
}

/// RAII wrapper for pooled items
pub struct PooledItem<'a, T> {
    item: Option<T>,
    pool: &'a std::sync::Mutex<Vec<T>>,
}

impl<T> std::ops::Deref for PooledItem<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.item.as_ref().unwrap()
    }
}

impl<T> std::ops::DerefMut for PooledItem<'_, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.item.as_mut().unwrap()
    }
}

impl<T> Drop for PooledItem<'_, T> {
    fn drop(&mut self) {
        if let Some(item) = self.item.take() {
            let mut pool = self.pool.lock().unwrap();
            pool.push(item);
        }
    }
}

/// Zero-copy string interner for reusing common strings
pub struct StringInterner {
    strings: std::sync::RwLock<HashMap<String, &'static str>>,
}

impl StringInterner {
    #[must_use]
    /// Create a new string interner
    pub fn new() -> Self {
        Self { strings: std::sync::RwLock::new(HashMap::new()) }
    }

    /// Intern a string, returning a static reference
    pub fn intern(&self, s: &str) -> &'static str {
        // First try reading
        {
            let strings = self.strings.read().unwrap();
            if let Some(&interned) = strings.get(s) {
                return interned;
            }
        }

        // Need to write
        let mut strings = self.strings.write().unwrap();

        // Double-check in case another thread added it
        if let Some(&interned) = strings.get(s) {
            return interned;
        }

        // Create a leaked string (this is intentional for the interner)
        let owned = s.to_string();
        let leaked = Box::leak(owned.into_boxed_str());
        strings.insert(s.to_string(), leaked);

        leaked
    }
}

impl Default for StringInterner {
    fn default() -> Self {
        Self::new()
    }
}

/// Global string interner for common LP keywords
static KEYWORD_INTERNER: OnceLock<StringInterner> = OnceLock::new();

/// Intern common LP keywords for memory efficiency
pub fn intern_keyword(keyword: &str) -> &'static str {
    KEYWORD_INTERNER.get_or_init(StringInterner::new).intern(keyword)
}

/// Performance metrics collector
#[derive(Debug, Default)]
pub struct PerfMetrics {
    /// Number of strings searched
    pub search_count: std::sync::atomic::AtomicU64,
    /// Total time spent searching (nanoseconds)
    pub search_time_ns: std::sync::atomic::AtomicU64,
    /// Number of cache hits
    pub cache_hits: std::sync::atomic::AtomicU64,
    /// Number of cache misses
    pub cache_misses: std::sync::atomic::AtomicU64,
}

impl PerfMetrics {
    /// Record a search operation
    pub fn record_search(&self, duration_ns: u64) {
        self.search_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        self.search_time_ns.fetch_add(duration_ns, std::sync::atomic::Ordering::Relaxed);
    }

    /// Record a cache hit
    pub fn record_cache_hit(&self) {
        self.cache_hits.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }

    /// Record a cache miss
    pub fn record_cache_miss(&self) {
        self.cache_misses.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }

    /// Get performance summary
    pub fn summary(&self) -> String {
        let searches = self.search_count.load(std::sync::atomic::Ordering::Relaxed);
        let total_time = self.search_time_ns.load(std::sync::atomic::Ordering::Relaxed);
        let hits = self.cache_hits.load(std::sync::atomic::Ordering::Relaxed);
        let misses = self.cache_misses.load(std::sync::atomic::Ordering::Relaxed);

        let avg_time = if searches > 0 { total_time as f64 / searches as f64 } else { 0.0 };

        let hit_rate = if hits + misses > 0 { hits as f64 / (hits + misses) as f64 * 100.0 } else { 0.0 };

        format!("Searches: {searches}, Avg time: {avg_time:.2}ns, Cache hit rate: {hit_rate:.1}%")
    }
}

/// Global performance metrics
pub static PERF_METRICS: OnceLock<PerfMetrics> = OnceLock::new();

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fast_find_case_insensitive() {
        assert_eq!(fast_find_case_insensitive("Hello World", "world"), Some(6));
        assert_eq!(fast_find_case_insensitive("Hello World", "HELLO"), Some(0));
        assert_eq!(fast_find_case_insensitive("Hello World", "xyz"), None);
    }

    #[test]
    fn test_fast_string_finder() {
        let finder = FastStringFinder::new(&["subject", "bounds", "end"]);

        assert_eq!(finder.find_first("This is subject to constraints"), Some((8, "subject")));
        assert_eq!(finder.find_first("BOUNDS section here"), Some((0, "bounds")));
        assert_eq!(finder.find_first("no matches here"), None);
    }

    #[test]
    fn test_memory_pool() {
        let pool = MemoryPool::new(Vec::<i32>::new);

        {
            let mut item1 = pool.get();
            item1.push(42);
            assert_eq!(item1[0], 42);
        }

        // Item should be returned to pool
        let mut item2 = pool.get();
        // The returned item might still have the old data
        item2.clear(); // Clear it for reuse
        item2.push(100);
        assert_eq!(item2[0], 100);
    }

    #[test]
    fn test_string_interner() {
        let interner = StringInterner::new();

        let s1 = interner.intern("test");
        let s2 = interner.intern("test");

        // Should return the same pointer
        assert_eq!(s1.as_ptr(), s2.as_ptr());
    }

    #[test]
    fn test_performance_metrics() {
        let metrics = PerfMetrics::default();

        metrics.record_search(1000);
        metrics.record_cache_hit();

        let summary = metrics.summary();
        assert!(summary.contains("Searches: 1"));
        assert!(summary.contains("Cache hit rate"));
    }
}
