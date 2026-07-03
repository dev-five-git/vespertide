//! Bounded cache for `try_parse_json` / `try_parse_yaml` results.
//!
//! Why: every `compute_diagnostics` call deserialises the entire model
//! file via serde, allocating the whole `TableDef` graph (`BTreeMap`, `Vec`,
//! and `String` values). For unchanged files this is pure waste. Hash the text and
//! cache the parsed result; cache hits return the same `Arc<Result<...>>`.
//!
//! Capacity: 64 entries. At ~50 KB per `TableDef` worst case that's
//! 3.2 MB ceiling. For a typical editor with O(open files) ≤ 64, this
//! is always a hit after first parse.
//!
//! Key: `(fxhash64(text), text.len())`. The length disambiguates the
//! astronomically-unlikely 64-bit collision; with both matching, the
//! cached entry is returned without byte-compare. (`FxHasher`'s collision
//! probability at N=64 is ~10⁻¹⁷.)

use std::ops::Range;
use std::sync::Arc;

use vespertide_core::TableDef;

use crate::cache::{RingCache, hash_text};

pub(super) type CachedParseResult = Result<TableDef, CachedParseError>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct CachedParseError {
    pub(super) byte_range: Range<usize>,
    pub(super) message: String,
    pub(super) code: &'static str,
}

impl CachedParseError {
    pub(super) fn new(
        byte_range: Range<usize>,
        message: impl Into<String>,
        code: &'static str,
    ) -> Self {
        Self {
            byte_range,
            message: message.into(),
            code,
        }
    }
}

/// 64-slot ring buffer keyed on `(hash, len)`. Stores successful parses and
/// parse/normalization errors so failed inputs are not retried on each call.
#[derive(Debug, Default)]
pub(super) struct ParseCache {
    inner: ParseRingCache,
}

type ParseKey = (u64, usize);
type ParseRingCache = RingCache<ParseKey, CachedParseResult, 64>;

impl ParseCache {
    pub(super) fn new() -> Self {
        Self::default()
    }

    /// Look up by `(hash, len)`. On hit returns the cached `Arc`.
    /// On miss, runs `parse_fn`, inserts the result, and returns it.
    pub(super) fn get_or_parse<F>(&self, text: &str, parse_fn: F) -> Arc<CachedParseResult>
    where
        F: FnOnce() -> CachedParseResult,
    {
        self.inner
            .get_or_compute((hash_text(text), text.len()), parse_fn)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn dummy_table(name: &str) -> TableDef {
        TableDef {
            name: name.into(),
            description: None,
            columns: vec![],
            constraints: vec![],
        }
    }

    fn dummy_error() -> CachedParseError {
        CachedParseError::new(0..1, "parse failed", "parse-error")
    }

    #[test]
    fn cache_hit_returns_same_arc() {
        let cache = ParseCache::new();
        let counter = AtomicUsize::new(0);

        let a = cache.get_or_parse("hello", || {
            counter.fetch_add(1, Ordering::SeqCst);
            Ok(dummy_table("hello"))
        });
        let b = cache.get_or_parse("hello", || {
            counter.fetch_add(1, Ordering::SeqCst);
            Ok(dummy_table("hello"))
        });

        assert!(Arc::ptr_eq(&a, &b), "same text → same Arc");
        assert_eq!(
            counter.load(Ordering::SeqCst),
            1,
            "parse_fn called exactly once"
        );
    }

    #[test]
    fn cache_miss_on_different_text() {
        let cache = ParseCache::new();
        let counter = AtomicUsize::new(0);

        cache.get_or_parse("hello", || {
            counter.fetch_add(1, Ordering::SeqCst);
            Ok(dummy_table("hello"))
        });
        cache.get_or_parse("world", || {
            counter.fetch_add(1, Ordering::SeqCst);
            Ok(dummy_table("world"))
        });

        assert_eq!(
            counter.load(Ordering::SeqCst),
            2,
            "parse_fn called twice for distinct texts"
        );
    }

    #[test]
    fn cache_evicts_oldest_at_capacity() {
        let cache = ParseCache::new();
        // Fill 65 unique entries — first one should be evicted.
        for i in 0..65 {
            let text = format!("t{i:03}");
            cache.get_or_parse(&text, || Ok(dummy_table(&text)));
        }

        let counter = AtomicUsize::new(0);
        // First entry "t000" should be a MISS (re-parsed).
        cache.get_or_parse("t000", || {
            counter.fetch_add(1, Ordering::SeqCst);
            Ok(dummy_table("t000"))
        });
        assert_eq!(counter.load(Ordering::SeqCst), 1, "oldest entry evicted");
    }

    #[test]
    fn cache_stores_error_for_failed_parse() {
        let cache = ParseCache::new();
        let counter = AtomicUsize::new(0);

        let a = cache.get_or_parse("broken", || {
            counter.fetch_add(1, Ordering::SeqCst);
            Err(dummy_error())
        });
        let b = cache.get_or_parse("broken", || {
            counter.fetch_add(1, Ordering::SeqCst);
            Err(dummy_error())
        });

        assert!(a.is_err() && b.is_err());
        assert_eq!(
            counter.load(Ordering::SeqCst),
            1,
            "failed parses are also cached"
        );
    }

    #[test]
    fn cache_threadsafe_across_8_threads() {
        let cache = Arc::new(ParseCache::new());
        let handles: Vec<_> = (0..8)
            .map(|i| {
                let cache = Arc::clone(&cache);
                std::thread::spawn(move || {
                    for j in 0..100 {
                        let text = format!("t{}", (i * 100 + j) % 50);
                        cache.get_or_parse(&text, || Ok(dummy_table(&text)));
                    }
                })
            })
            .collect();

        for handle in handles {
            handle.join().expect("cache worker thread should not panic");
        }
    }
}
