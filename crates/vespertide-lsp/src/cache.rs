//! Generic ring-buffer LRU cache shared by HS-3/HS-7/HS-8/HS-9.
//!
//! Each call site keeps its own `OnceLock<Self>` global; the cache is
//! private to the module that creates it. Eviction is FIFO via insertion
//! order — simple and good enough for the open-files cardinality of
//! typical editor sessions.
//!
//! Usage:
//! ```ignore
//! use crate::cache::RingCache;
//! type SymbolCache = RingCache<SymbolKey, Vec<RawSymbol>, 128>;
//! ```
//! where `SymbolKey: Hash + Eq + Copy`.

use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, VecDeque};
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex, OnceLock};

use tower_lsp_server::ls_types::Uri;

use crate::store::DocumentStore;

/// Const-generic ring-buffer cache. `K` must implement `Hash + Eq + Copy`
/// (typically a small tuple like `(u64, usize, u8)`). `V` is wrapped in
/// `Arc` so cache hits return shared references without cloning the
/// underlying data.
#[derive(Debug)]
pub(crate) struct RingCache<K, V, const N: usize>
where
    K: Eq + Hash + Copy,
{
    inner: Mutex<RingCacheInner<K, V, N>>,
}

#[derive(Debug)]
struct RingCacheInner<K, V, const N: usize>
where
    K: Eq + Hash + Copy,
{
    entries: HashMap<K, Arc<V>>,
    insertion_order: VecDeque<K>,
}

impl<K, V, const N: usize> Default for RingCache<K, V, N>
where
    K: Eq + Hash + Copy,
{
    fn default() -> Self {
        Self {
            inner: Mutex::new(RingCacheInner {
                entries: HashMap::with_capacity(N),
                insertion_order: VecDeque::with_capacity(N),
            }),
        }
    }
}

impl<K, V, const N: usize> RingCache<K, V, N>
where
    K: Eq + Hash + Copy,
{
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Look up by key. On hit returns the cached `Arc<V>`. On miss,
    /// `compute_fn` runs (with the lock released), the result is wrapped
    /// in `Arc`, inserted, and returned.
    pub(crate) fn get_or_compute<F>(&self, key: K, compute_fn: F) -> Arc<V>
    where
        F: FnOnce() -> V,
    {
        {
            let inner = self
                .inner
                .lock()
                .expect("RingCache lock poisoned — invariant: compute_fn must not panic");
            if let Some(value) = inner.entries.get(&key) {
                return Arc::clone(value);
            }
        }

        let value = Arc::new(compute_fn());

        let mut inner = self
            .inner
            .lock()
            .expect("RingCache lock poisoned — invariant: compute_fn must not panic");
        if inner.entries.len() >= N
            && let Some(oldest) = inner.insertion_order.pop_front()
        {
            inner.entries.remove(&oldest);
        }
        inner.entries.insert(key, Arc::clone(&value));
        inner.insertion_order.push_back(key);
        value
    }

    /// Test-only — clear all entries.
    #[cfg(test)]
    pub(crate) fn clear(&self) {
        let mut inner = self
            .inner
            .lock()
            .expect("RingCache lock poisoned — invariant: compute_fn must not panic");
        inner.entries.clear();
        inner.insertion_order.clear();
    }
}

pub(crate) fn hash_text(text: &str) -> u64 {
    let mut h = DefaultHasher::new();
    h.write(text.as_bytes());
    h.finish()
}

#[derive(Debug, Clone)]
struct CachedDocstoreFingerprint {
    docs_addr: usize,
    len: usize,
    entries: Vec<DocstoreFingerprintEntry>,
    fingerprint: u64,
}

#[derive(Debug, Clone)]
struct DocstoreFingerprintEntry {
    uri: Uri,
    version: i32,
    text_len: usize,
}

static DOCSTORE_FINGERPRINT_CACHE: OnceLock<Mutex<Option<CachedDocstoreFingerprint>>> =
    OnceLock::new();

fn docstore_fingerprint_cache() -> &'static Mutex<Option<CachedDocstoreFingerprint>> {
    DOCSTORE_FINGERPRINT_CACHE.get_or_init(|| Mutex::new(None))
}

/// Stable digest of every open document's `(uri, text)`. Used as part of
/// cache keys for workspace-wide caches whose value depends on
/// `DocumentStore` content. Computation cost: ~70μs for 100 open docs with
/// ~2KB each on the synthetic workload — cheap enough to compute on every
/// cache lookup.
///
/// Determinism: `DocumentStore::for_each` iterates URIs in sorted order
/// (verified in `store.rs`), so the same set of open docs always produces
/// the same fingerprint.
#[must_use]
pub(crate) fn docstore_fingerprint(docs: &DocumentStore) -> u64 {
    let docs_addr = std::ptr::from_ref(docs).addr();
    if let Some(fingerprint) = cached_docstore_fingerprint(docs, docs_addr) {
        return fingerprint;
    }

    let (fingerprint, entries) = compute_docstore_fingerprint(docs);
    *docstore_fingerprint_cache()
        .lock()
        .expect("RingCache lock poisoned — invariant: compute_fn must not panic") =
        Some(CachedDocstoreFingerprint {
            docs_addr,
            len: entries.len(),
            entries,
            fingerprint,
        });
    fingerprint
}

fn cached_docstore_fingerprint(docs: &DocumentStore, docs_addr: usize) -> Option<u64> {
    let cache = docstore_fingerprint_cache()
        .lock()
        .expect("RingCache lock poisoned — invariant: compute_fn must not panic");
    let cached = cache.as_ref()?;
    if cached.docs_addr != docs_addr || cached.len != docs.len() {
        return None;
    }
    for entry in &cached.entries {
        let unchanged = docs
            .docs_iter_for_uri(&entry.uri, |state| {
                state.doc.version() == entry.version && state.text().len() == entry.text_len
            })
            .unwrap_or(false);
        if !unchanged {
            return None;
        }
    }
    Some(cached.fingerprint)
}

fn compute_docstore_fingerprint(docs: &DocumentStore) -> (u64, Vec<DocstoreFingerprintEntry>) {
    let mut h = DefaultHasher::new();
    let mut entries = Vec::with_capacity(docs.len());
    docs.for_each(|uri, state| {
        let text = state.text();
        entries.push(DocstoreFingerprintEntry {
            uri: uri.clone(),
            version: state.doc.version(),
            text_len: text.len(),
        });
        h.write(uri.as_str().as_bytes());
        h.write_u8(0);
        h.write_usize(text.len());
        h.write(text.as_bytes());
        h.write_u8(0);
    });
    (h.finish(), entries)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::DocumentStore;
    use crate::test_support::uri;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn ring_cache_hit_returns_same_arc() {
        let cache: RingCache<u64, Vec<i32>, 4> = RingCache::new();
        let counter = AtomicUsize::new(0);
        let a = cache.get_or_compute(1, || {
            counter.fetch_add(1, Ordering::SeqCst);
            vec![1, 2, 3]
        });
        let b = cache.get_or_compute(1, || {
            counter.fetch_add(1, Ordering::SeqCst);
            vec![1, 2, 3]
        });
        assert!(Arc::ptr_eq(&a, &b));
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn ring_cache_miss_on_different_key() {
        let cache: RingCache<u64, Vec<i32>, 4> = RingCache::new();
        let counter = AtomicUsize::new(0);
        cache.get_or_compute(1, || {
            counter.fetch_add(1, Ordering::SeqCst);
            vec![]
        });
        cache.get_or_compute(2, || {
            counter.fetch_add(1, Ordering::SeqCst);
            vec![]
        });
        assert_eq!(counter.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn ring_cache_evicts_oldest() {
        let cache: RingCache<u64, Vec<i32>, 2> = RingCache::new();
        let counter = AtomicUsize::new(0);
        cache.get_or_compute(1, || {
            counter.fetch_add(1, Ordering::SeqCst);
            vec![]
        });
        cache.get_or_compute(2, || {
            counter.fetch_add(1, Ordering::SeqCst);
            vec![]
        });
        cache.get_or_compute(3, || {
            counter.fetch_add(1, Ordering::SeqCst);
            vec![]
        });
        // key=1 should now be evicted (capacity is 2)
        cache.get_or_compute(1, || {
            counter.fetch_add(1, Ordering::SeqCst);
            vec![]
        });
        assert_eq!(counter.load(Ordering::SeqCst), 4);
    }

    #[test]
    fn ring_cache_threadsafe() {
        let cache: Arc<RingCache<i32, Vec<i32>, 8>> = Arc::new(RingCache::new());
        let handles: Vec<_> = (0..8)
            .map(|i| {
                let cache = Arc::clone(&cache);
                std::thread::spawn(move || {
                    for j in 0..50 {
                        cache.get_or_compute((i * 50 + j) % 8, || vec![i]);
                    }
                })
            })
            .collect();
        for h in handles {
            h.join().unwrap();
        }
    }

    #[test]
    fn docstore_fingerprint_cache_misses_for_other_store_and_len_change() {
        let docs = DocumentStore::new();
        docs.open(
            uri("one.json"),
            "json".to_string(),
            1,
            r#"{"name":"one","columns":[]}"#.to_string(),
        );
        let first = docstore_fingerprint(&docs);

        let other_docs = DocumentStore::new();
        other_docs.open(
            uri("two.json"),
            "json".to_string(),
            1,
            r#"{"name":"two","columns":[]}"#.to_string(),
        );
        let other = docstore_fingerprint(&other_docs);

        docs.open(
            uri("three.json"),
            "json".to_string(),
            1,
            r#"{"name":"three","columns":[]}"#.to_string(),
        );
        let changed_len = docstore_fingerprint(&docs);

        assert_ne!(first, other);
        assert_ne!(first, changed_len);
    }
}
