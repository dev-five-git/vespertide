//! Workspace symbols — global `Ctrl+T` / `Ctrl+Shift+O` search across
//! every table and column in the workspace.
//!
//! For each model file (open in the editor OR sitting on disk) we emit:
//!   * one symbol per table (`name: "user"`, kind=Class)
//!   * one symbol per column (`name: "email"`, container=`"user"`, kind=Field)
//!
//! The provided query is matched as a **case-insensitive substring** —
//! mirrors what most LSP clients render in the symbol picker without
//! requiring server-side fuzzy ranking, while still keeping the result
//! set tight enough to stay responsive in workspaces with hundreds of
//! columns.

use crate::text_util::strip_quotes;
use std::ops::Range;
use std::sync::{Arc, OnceLock};

use tower_lsp_server::ls_types::Uri;

use crate::cache::{RingCache, docstore_fingerprint, hash_text};
use crate::store::DocumentStore;
use crate::workspace_tables::WorkspaceTables;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DomainSymbol {
    /// Display name in the symbol picker.
    pub name: String,
    /// Distinguishes tables (`Table`) from columns (`Column`) — the LSP
    /// layer maps these to `SymbolKind::Class` / `SymbolKind::Field`.
    pub kind: SymbolKind,
    /// Owning table name for column symbols; `None` for tables.
    pub container: Option<String>,
    /// File hosting the declaration.
    pub uri: Uri,
    /// Byte range of the identifier (table or column name value).
    pub byte_range: Range<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolKind {
    Table,
    Column,
}

/// Cached symbol extraction for `compute_workspace_symbols`. Walking the
/// tree to enumerate `(table_name, [column_names])` is the dominant cost
/// in this hot path (`~60%` of profile wall time on the synthetic
/// workload). Cache the unfiltered symbol list keyed on the source text;
/// the per-call query filter runs in microseconds on the cached vec.
///
/// 128-slot ring buffer with `(fxhash64, len)` keys. Same shape as
/// `diagnostics::validation::cache::ParseCache` (HS-3), sized to cover the
/// 100-model profiling workload without ring-buffer thrash.
type SymbolKey = (u64, usize);
type SymbolCache = RingCache<SymbolKey, Vec<RawSymbol>, 128>;

/// Pre-query symbol info extracted from a doc's tree. `compute_workspace_symbols`
/// applies the query filter to this — see `ascii_ci_contains`. The `container`
/// for a Column is the table name from the same document.
#[derive(Debug, Clone)]
struct RawSymbol {
    name: String,
    kind: SymbolKind,
    container: Option<String>,
    byte_range: Range<usize>,
}

/// One element per `(uri, raw_symbol)` for every open doc + every disk table.
/// Built once per `docstore_fingerprint` change.
#[derive(Debug, Clone)]
struct WorkspaceSymbolEntry {
    uri: Uri,
    raw: RawSymbol,
}

static SYMBOL_CACHE: OnceLock<SymbolCache> = OnceLock::new();

fn symbol_cache() -> &'static SymbolCache {
    SYMBOL_CACHE.get_or_init(SymbolCache::new)
}

/// Cache the workspace-wide flat symbol list keyed on `docstore_fingerprint`.
/// Small capacity (8 slots) because invalidation is coarse: any `did_change`
/// advances the fingerprint and the next call rebuilds the whole list. 8 slots
/// is enough to amortize across a few rapid edits.
type WorkspaceSymbolsCache = RingCache<u64, Vec<WorkspaceSymbolEntry>, 8>;

static WORKSPACE_SYMBOLS_CACHE: OnceLock<WorkspaceSymbolsCache> = OnceLock::new();

fn workspace_symbols_cache() -> &'static WorkspaceSymbolsCache {
    WORKSPACE_SYMBOLS_CACHE.get_or_init(WorkspaceSymbolsCache::new)
}

/// Cache filtered query results keyed on `(docstore_fingerprint, needle_hash)`.
/// 256 slots — accommodates ~100 fingerprints × ~3 queries cardinality without
/// thrash. The value is the final `Vec<DomainSymbol>` ready to return to the
/// caller.
type FilteredSymbolsCache = RingCache<(u64, u64), Vec<DomainSymbol>, 256>;

static FILTERED_SYMBOLS_CACHE: OnceLock<FilteredSymbolsCache> = OnceLock::new();

fn filtered_symbols_cache() -> &'static FilteredSymbolsCache {
    FILTERED_SYMBOLS_CACHE.get_or_init(FilteredSymbolsCache::new)
}

/// Same as [`compute`] but returns an `Arc<Vec<DomainSymbol>>` directly from
/// the cache, avoiding a final per-call `Vec` clone. Use this entry point for
/// read-only consumers; [`compute`] wraps this and clones once for backward
/// compatibility.
#[must_use]
pub fn compute_shared(
    query: &str,
    docs: &DocumentStore,
    disk_tables: Option<&WorkspaceTables>,
) -> Arc<Vec<DomainSymbol>> {
    let needle = query.trim().to_ascii_lowercase();
    let fingerprint = docstore_fingerprint(docs);
    let needle_hash = hash_text(&needle);

    filtered_symbols_cache().get_or_compute((fingerprint, needle_hash), || {
        let flat = workspace_symbols_cache().get_or_compute(fingerprint, || {
            build_workspace_symbol_list(docs, disk_tables)
        });
        let mut result: Vec<DomainSymbol> = flat
            .iter()
            .filter(|entry| ascii_ci_contains(&entry.raw.name, &needle))
            .map(|entry| DomainSymbol {
                name: entry.raw.name.clone(),
                kind: entry.raw.kind,
                container: entry.raw.container.clone(),
                uri: entry.uri.clone(),
                byte_range: entry.raw.byte_range.clone(),
            })
            .collect();
        // Sort by (name, kind) for deterministic output across runs / clients.
        result.sort_by(|a, b| {
            a.name
                .cmp(&b.name)
                .then_with(|| (a.kind as u8).cmp(&(b.kind as u8)))
        });
        result
    })
}

/// Collect every table and column matching `query` (case-insensitive
/// substring; empty query returns everything).
#[must_use]
pub fn compute(
    query: &str,
    docs: &DocumentStore,
    disk_tables: Option<&WorkspaceTables>,
) -> Vec<DomainSymbol> {
    (*compute_shared(query, docs, disk_tables)).clone()
}

/// Build the workspace-wide flat symbol list. Iterates every open doc (using
/// HS-7 `SymbolCache` for per-doc extraction) and every disk-only table (using
/// HS-2 `cached_parse` + per-doc cache). Returns one `WorkspaceSymbolEntry` per
/// `(uri, raw_symbol)` pair, sorted by URI then `byte_range` for determinism.
fn build_workspace_symbol_list(
    docs: &DocumentStore,
    disk_tables: Option<&WorkspaceTables>,
) -> Vec<WorkspaceSymbolEntry> {
    let mut out = Vec::new();
    let mut seen_uris = std::collections::BTreeSet::new();

    docs.for_each(|uri, state| {
        if let Some(tree) = state.tree.as_ref() {
            seen_uris.insert(uri.clone());
            let text = state.text();
            let raw = symbol_cache().get_or_compute((hash_text(text), text.len()), || {
                extract_raw_symbols(tree, text)
            });
            for raw_sym in raw.iter() {
                out.push(WorkspaceSymbolEntry {
                    uri: uri.clone(),
                    raw: raw_sym.clone(),
                });
            }
        }
    });

    if let Some(disk) = disk_tables {
        let pool = shared_parser_pool();
        for name in disk.names() {
            if let Some(path) = disk.model_path(&name)
                && let Some(uri) = crate::position::path_to_uri(&path)
                && !seen_uris.contains(&uri)
                && let Some((text_arc, tree_arc)) = disk.cached_parse(&path, pool)
            {
                let text = &*text_arc;
                let tree = &*tree_arc;
                let raw = symbol_cache().get_or_compute((hash_text(text), text.len()), || {
                    extract_raw_symbols(tree, text)
                });
                for raw_sym in raw.iter() {
                    out.push(WorkspaceSymbolEntry {
                        uri: uri.clone(),
                        raw: raw_sym.clone(),
                    });
                }
            }
        }
    }

    out.sort_by(|a, b| {
        a.uri
            .as_str()
            .cmp(b.uri.as_str())
            .then_with(|| a.raw.byte_range.start.cmp(&b.raw.byte_range.start))
    });
    out
}

fn shared_parser_pool() -> &'static crate::parser::ParserPool {
    static SHARED_POOL: OnceLock<crate::parser::ParserPool> = OnceLock::new();
    SHARED_POOL.get_or_init(crate::parser::ParserPool::new)
}

/// Extract every table + column symbol from a parsed model file, WITHOUT
/// applying any query filter. The result is cacheable per-text; callers
/// apply `ascii_ci_contains(name, needle)` to filter.
fn extract_raw_symbols(tree: &tree_sitter::Tree, source: &str) -> Vec<RawSymbol> {
    let source_bytes = source.as_bytes();
    let mut out = Vec::new();
    if let Some(mapping) = find_outer_mapping(tree.root_node())
        && let Some((table_name, table_range)) = direct_pair_value(mapping, source_bytes, "name")
            .map(|(text, range)| (text.to_string(), range))
    {
        out.push(RawSymbol {
            name: table_name.clone(),
            kind: SymbolKind::Table,
            container: None,
            byte_range: table_range,
        });

        if let Some(columns_value) = direct_pair_node(mapping, source_bytes, "columns") {
            let columns_array = unwrap_yaml_node(columns_value);
            if matches!(
                columns_array.kind(),
                "array" | "block_sequence" | "flow_sequence"
            ) {
                let mut cursor = columns_array.walk();
                for raw_child in columns_array.children(&mut cursor) {
                    let child = unwrap_yaml_node(raw_child);
                    let mapping = match child.kind() {
                        "object" | "block_mapping" | "flow_mapping" => Some(child),
                        "block_sequence_item" => {
                            let mut inner_cursor = child.walk();
                            child
                                .children(&mut inner_cursor)
                                .map(unwrap_yaml_node)
                                .find(|n| {
                                    matches!(n.kind(), "object" | "block_mapping" | "flow_mapping")
                                })
                        }
                        _ => None,
                    };
                    if let Some(column_mapping) = mapping
                        && let Some((column_name, column_range)) =
                            direct_pair_value(column_mapping, source_bytes, "name")
                                .map(|(text, range)| (text.to_string(), range))
                    {
                        out.push(RawSymbol {
                            name: column_name,
                            kind: SymbolKind::Column,
                            container: Some(table_name.clone()),
                            byte_range: column_range,
                        });
                    }
                }
            }
        }
    }

    out
}

/// Find a direct child pair `key: …` and return `(stripped value text, value byte range)`.
fn direct_pair_value<'a>(
    mapping: tree_sitter::Node<'_>,
    source: &'a [u8],
    target_key: &str,
) -> Option<(&'a str, Range<usize>)> {
    let pair = find_pair_with_key(mapping, source, target_key)?;
    let value = unwrap_yaml_node(pair.named_child(1)?);
    let raw = source.get(value.byte_range())?;
    let text = std::str::from_utf8(raw).ok()?;
    // Adjust byte range to skip quotes when the value is a quoted string.
    let range = match value.kind() {
        "string" => value.named_child(0).map_or_else(
            || trim_one_byte(&value.byte_range()),
            |inner| inner.byte_range(),
        ),
        "double_quote_scalar" | "single_quote_scalar" => trim_one_byte(&value.byte_range()),
        _ => value.byte_range(),
    };
    Some((strip_quotes(text), range))
}

fn direct_pair_node<'tree>(
    mapping: tree_sitter::Node<'tree>,
    source: &[u8],
    target_key: &str,
) -> Option<tree_sitter::Node<'tree>> {
    find_pair_with_key(mapping, source, target_key)?.named_child(1)
}

use crate::tree_util::{
    find_outer_mapping, find_pair_with_key, trim_one_byte_each_side as trim_one_byte,
    unwrap_yaml_node,
};

/// ASCII case-insensitive substring search. `needle_lower` must already be
/// lowercase (the public `compute()` entry-point lowercases the query once).
/// Allocates zero — walks `haystack` byte-by-byte folding only ASCII case.
/// Non-ASCII bytes are compared exactly (same semantics as the prior
/// `to_ascii_lowercase().contains(...)` because that function also only
/// folds ASCII).
fn ascii_ci_contains(haystack: &str, needle_lower: &str) -> bool {
    if needle_lower.is_empty() {
        return true;
    }
    let hay = haystack.as_bytes();
    let nee = needle_lower.as_bytes();
    if nee.len() > hay.len() {
        return false;
    }
    'outer: for start in 0..=(hay.len() - nee.len()) {
        for i in 0..nee.len() {
            if hay[start + i].to_ascii_lowercase() != nee[i] {
                continue 'outer;
            }
        }
        return true;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::{DocumentFormat, ParserPool};
    use crate::test_support::uri;
    use std::fs;
    use std::sync::Arc;
    use tempfile::tempdir;

    fn write_workspace(root: &std::path::Path, models: &[(&str, &str)]) {
        let models_dir = root.join("models");
        fs::create_dir_all(&models_dir).unwrap();
        fs::write(root.join("vespertide.json"), r#"{"modelsDir":"models","migrationsDir":"migrations","tableNamingCase":"snake","columnNamingCase":"snake","modelFormat":"json"}"#).unwrap();
        for (name, content) in models {
            fs::write(models_dir.join(name), content).unwrap();
        }
    }

    fn find_empty_yaml_wrapper(node: tree_sitter::Node<'_>) -> Option<tree_sitter::Node<'_>> {
        if matches!(node.kind(), "flow_node" | "block_node") && node.named_child(0).is_none() {
            return Some(node);
        }
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if let Some(found) = find_empty_yaml_wrapper(child) {
                return Some(found);
            }
        }
        None
    }

    #[test]
    fn empty_query_returns_all_tables_and_columns() {
        let docs = DocumentStore::new();
        let src = r#"{"name":"user","columns":[{"name":"id","type":"integer"},{"name":"email","type":"text"}]}"#;
        let u = uri("user.json");
        // `DocumentStore::open` parses the tree via the internal ParserPool,
        // so we do not need to feed a tree manually.
        docs.open(u, "json".to_string(), 1, src.to_string());

        let symbols = compute("", &docs, None);
        let names: Vec<_> = symbols.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"user"), "got: {names:?}");
        assert!(names.contains(&"id"));
        assert!(names.contains(&"email"));

        // Each column's container points at its owning table.
        let email = symbols.iter().find(|s| s.name == "email").unwrap();
        assert_eq!(email.kind, SymbolKind::Column);
        assert_eq!(email.container.as_deref(), Some("user"));
    }

    #[test]
    fn extract_raw_symbols_emits_table_symbol_with_identifier_range() {
        let pool = ParserPool::new();
        let src = r#"{"name":"user","columns":[]}"#;
        let tree = pool.parse(src, DocumentFormat::Json).unwrap();
        let symbols = extract_raw_symbols(&tree, src);

        assert_eq!(symbols.len(), 1);
        assert_eq!(symbols[0].name, "user");
        assert_eq!(symbols[0].kind, SymbolKind::Table);
        assert_eq!(&src[symbols[0].byte_range.clone()], "user");
    }

    #[test]
    fn query_filters_case_insensitively() {
        let docs = DocumentStore::new();
        let src = r#"{"name":"User","columns":[{"name":"emAil","type":"text"}]}"#;
        docs.open(uri("user.json"), "json".to_string(), 1, src.to_string());

        let s1 = compute("user", &docs, None);
        assert!(s1.iter().any(|s| s.name == "User"));

        let s2 = compute("EMAIL", &docs, None);
        assert!(s2.iter().any(|s| s.name == "emAil"));
    }

    #[test]
    fn output_is_sorted_for_deterministic_picker_ordering() {
        let docs = DocumentStore::new();
        let src = r#"{"name":"zeta","columns":[{"name":"alpha","type":"integer"},{"name":"beta","type":"integer"}]}"#;
        docs.open(uri("z.json"), "json".to_string(), 1, src.to_string());

        let symbols = compute("", &docs, None);
        let names: Vec<_> = symbols.iter().map(|s| s.name.clone()).collect();
        let mut sorted = names.clone();
        sorted.sort();
        assert_eq!(names, sorted, "symbols must be sorted alphabetically");
    }

    /// Regression-style: column name picker must not silently drop
    /// columns when the file uses YAML scalars.
    #[test]
    fn yaml_workspace_symbols() {
        let docs = DocumentStore::new();
        let src = "name: account\ncolumns:\n  - name: id\n    type: integer\n  - name: balance\n    type: numeric\n";
        docs.open(uri("account.yaml"), "yaml".to_string(), 1, src.to_string());

        let symbols = compute("", &docs, None);
        let names: Vec<_> = symbols.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"account"));
        assert!(names.contains(&"id"));
        assert!(names.contains(&"balance"));
    }

    // Silence dead-code lints on the parser pool helper used by tests above
    // when they otherwise stop importing it.
    fn _force_parser_pool() {
        let _ = ParserPool::new();
        let _ = DocumentFormat::Json;
    }

    #[test]
    fn ascii_ci_contains_empty_needle_matches() {
        assert!(ascii_ci_contains("anything", ""));
        assert!(ascii_ci_contains("", ""));
    }

    #[test]
    fn ascii_ci_contains_needle_longer_than_haystack() {
        assert!(!ascii_ci_contains("a", "abc"));
    }

    #[test]
    fn ascii_ci_contains_mixed_case_match() {
        assert!(ascii_ci_contains("FooBar", "oob"));
        assert!(ascii_ci_contains("USER", "use"));
    }

    #[test]
    fn ascii_ci_contains_no_match() {
        assert!(!ascii_ci_contains("foo", "xyz"));
    }

    #[test]
    fn ascii_ci_contains_non_ascii_passes_through() {
        // Matches existing `to_ascii_lowercase().contains(...)` semantics:
        // non-ASCII bytes are NOT folded; they're compared byte-wise.
        assert!(ascii_ci_contains("카페", "카페"));
        assert!(!ascii_ci_contains("카페", "café"));
    }

    #[test]
    fn symbol_cache_hit_returns_same_arc() {
        let cache = SymbolCache::default();
        let source = r#"{"name":"user","columns":[{"name":"id","type":"integer"}]}"#;
        let pool = crate::parser::ParserPool::new();
        let tree = pool
            .parse(source, crate::parser::DocumentFormat::Json)
            .unwrap();
        let key = (hash_text(source), source.len());
        let a = cache.get_or_compute(key, || extract_raw_symbols(&tree, source));
        let b = cache.get_or_compute(key, || extract_raw_symbols(&tree, source));
        assert!(Arc::ptr_eq(&a, &b), "cache hit returns same Arc");
        assert_eq!(a.len(), 2, "1 table + 1 column");
    }

    #[test]
    fn symbol_cache_miss_on_different_text() {
        let cache = SymbolCache::default();
        let source_a = r#"{"name":"user","columns":[]}"#;
        let source_b = r#"{"name":"post","columns":[]}"#;
        let pool = crate::parser::ParserPool::new();
        let tree_a = pool
            .parse(source_a, crate::parser::DocumentFormat::Json)
            .unwrap();
        let tree_b = pool
            .parse(source_b, crate::parser::DocumentFormat::Json)
            .unwrap();
        let a = cache.get_or_compute((hash_text(source_a), source_a.len()), || {
            extract_raw_symbols(&tree_a, source_a)
        });
        let b = cache.get_or_compute((hash_text(source_b), source_b.len()), || {
            extract_raw_symbols(&tree_b, source_b)
        });
        assert_eq!(a[0].name, "user");
        assert_eq!(b[0].name, "post");
    }

    #[test]
    fn workspace_symbols_cache_hit_returns_arc() {
        let cache = WorkspaceSymbolsCache::new();
        let entries = vec![WorkspaceSymbolEntry {
            uri: "file:///t.json".parse().unwrap(),
            raw: RawSymbol {
                name: "user".into(),
                kind: SymbolKind::Table,
                container: None,
                byte_range: 0..4,
            },
        }];
        let a = cache.get_or_compute(42, || entries.clone());
        let b = cache.get_or_compute(42, Vec::new);
        assert!(Arc::ptr_eq(&a, &b));
        assert_eq!(a.len(), 1);
    }

    #[test]
    fn filtered_cache_hit_returns_arc() {
        let cache = FilteredSymbolsCache::new();
        let syms = vec![DomainSymbol {
            name: "user".into(),
            kind: SymbolKind::Table,
            container: None,
            uri: "file:///t.json".parse().unwrap(),
            byte_range: 0..4,
        }];
        let a = cache.get_or_compute((1, 2), || syms.clone());
        let b = cache.get_or_compute((1, 2), Vec::new);
        assert!(Arc::ptr_eq(&a, &b));
    }

    #[test]
    fn filtered_cache_miss_on_different_fingerprint() {
        let cache = FilteredSymbolsCache::new();
        let counter = std::sync::atomic::AtomicUsize::new(0);
        cache.get_or_compute((1, 99), || {
            counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            vec![]
        });
        cache.get_or_compute((2, 99), || {
            counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            vec![]
        });
        assert_eq!(counter.load(std::sync::atomic::Ordering::SeqCst), 2);
    }

    #[test]
    fn workspace_symbols_cache_miss_on_different_fingerprint() {
        let cache = WorkspaceSymbolsCache::new();
        let counter = std::sync::atomic::AtomicUsize::new(0);
        cache.get_or_compute(1, || {
            counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            vec![]
        });
        cache.get_or_compute(2, || {
            counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            vec![]
        });
        assert_eq!(counter.load(std::sync::atomic::Ordering::SeqCst), 2);
    }

    #[test]
    fn filtered_cache_miss_on_different_needle_hash() {
        let cache = FilteredSymbolsCache::new();
        let counter = std::sync::atomic::AtomicUsize::new(0);
        cache.get_or_compute((1, 10), || {
            counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            vec![]
        });
        cache.get_or_compute((1, 11), || {
            counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            vec![]
        });
        assert_eq!(counter.load(std::sync::atomic::Ordering::SeqCst), 2);
    }

    #[test]
    fn build_workspace_symbol_list_includes_open_doc_columns() {
        let docs = DocumentStore::new();
        let uri: Uri = "file:///t.json".parse().unwrap();
        let source = r#"{"name":"user","columns":[{"name":"id","type":"integer"},{"name":"email","type":"text"}]}"#;
        docs.open(uri, "json".to_string(), 1, source.to_string());

        let entries = build_workspace_symbol_list(&docs, None);
        assert!(entries.iter().any(|e| e.raw.name == "user"));
        assert!(entries.iter().any(|e| e.raw.name == "id"));
        assert!(entries.iter().any(|e| e.raw.name == "email"));
    }

    #[test]
    fn build_workspace_symbol_list_sorts_by_uri_then_byte_range() {
        let docs = DocumentStore::new();
        docs.open(
            "file:///z.json".parse().unwrap(),
            "json".to_string(),
            1,
            r#"{"name":"zeta","columns":[{"name":"z_col","type":"integer"}]}"#.to_string(),
        );
        docs.open(
            "file:///a.json".parse().unwrap(),
            "json".to_string(),
            1,
            r#"{"name":"alpha","columns":[{"name":"a_col","type":"integer"}]}"#.to_string(),
        );

        let entries = build_workspace_symbol_list(&docs, None);
        let positions: Vec<_> = entries
            .iter()
            .map(|entry| (entry.uri.as_str().to_string(), entry.raw.byte_range.start))
            .collect();
        let mut sorted = positions.clone();
        sorted.sort();
        assert_eq!(positions, sorted);
    }

    #[test]
    fn compute_with_filtered_cache_returns_same_results_as_unfiltered_iteration() {
        // Smoke test the end-to-end caching: same input → same output across 3 calls.
        let docs = DocumentStore::new();
        let uri: Uri = "file:///t.json".parse().unwrap();
        docs.open(uri, "json".to_string(), 1, r#"{"name":"user","columns":[{"name":"id","type":"integer"},{"name":"email","type":"text"}]}"#.to_string());

        let first = compute("email", &docs, None);
        let second = compute("email", &docs, None);
        let third = compute("email", &docs, None);
        assert_eq!(first, second);
        assert_eq!(second, third);
        assert!(first.iter().any(|s| s.name == "email"));
    }

    #[test]
    fn compute_shared_and_compute_return_same_results() {
        let docs = DocumentStore::new();
        let uri: Uri = "file:///t.json".parse().unwrap();
        docs.open(uri, "json".to_string(), 1, r#"{"name":"user","columns":[{"name":"id","type":"integer"},{"name":"email","type":"text"}]}"#.to_string());

        let shared = compute_shared("email", &docs, None);
        let owned = compute("email", &docs, None);
        assert_eq!(*shared, owned, "Arc deref equals owned clone");
    }

    #[test]
    fn compute_shared_hit_returns_same_arc() {
        let docs = DocumentStore::new();
        let uri: Uri = "file:///t2.json".parse().unwrap();
        docs.open(
            uri,
            "json".to_string(),
            1,
            r#"{"name":"order","columns":[{"name":"id","type":"integer"}]}"#.to_string(),
        );

        let a = compute_shared("id", &docs, None);
        let b = compute_shared("id", &docs, None);
        assert!(Arc::ptr_eq(&a, &b), "warm cache returns same Arc");
    }

    #[test]
    fn compute_clones_arc_for_compat() {
        let docs = DocumentStore::new();
        let uri: Uri = "file:///t3.json".parse().unwrap();
        docs.open(
            uri,
            "json".to_string(),
            1,
            r#"{"name":"x","columns":[{"name":"id","type":"integer"}]}"#.to_string(),
        );

        let owned = compute("id", &docs, None);
        let shared = compute_shared("id", &docs, None);
        assert_eq!(owned.len(), shared.len());
        for (a, b) in owned.iter().zip(shared.iter()) {
            assert_eq!(a, b);
        }
    }

    #[test]
    fn compute_misses_filtered_cache_after_doc_text_change() {
        let docs = DocumentStore::new();
        let uri: Uri = "file:///change.json".parse().unwrap();
        docs.open(
            uri.clone(),
            "json".to_string(),
            1,
            r#"{"name":"user","columns":[{"name":"email","type":"text"}]}"#.to_string(),
        );
        assert!(
            compute("email", &docs, None)
                .iter()
                .any(|s| s.name == "email")
        );

        docs.update_full(
            &uri,
            r#"{"name":"post","columns":[{"name":"title","type":"text"}]}"#.to_string(),
            2,
        );
        assert!(compute("email", &docs, None).is_empty());
        assert!(
            compute("title", &docs, None)
                .iter()
                .any(|s| s.name == "title")
        );
    }

    #[test]
    fn compute_trims_query_before_filtering() {
        let docs = DocumentStore::new();
        let uri: Uri = "file:///trim.json".parse().unwrap();
        let source = r#"{"name":"user","columns":[{"name":"email","type":"text"}]}"#;
        docs.open(uri, "json".to_string(), 1, source.to_string());

        let out = compute("  EMAIL  ", &docs, None);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].name, "email");
    }

    #[test]
    fn compute_filters_workspace_entries_by_query() {
        let docs = DocumentStore::new();
        let uri: Uri = "file:///filter.json".parse().unwrap();
        let source = r#"{"name":"user","columns":[{"name":"id","type":"integer"},{"name":"email","type":"text"}]}"#;
        docs.open(uri, "json".to_string(), 1, source.to_string());

        let out = compute("mail", &docs, None);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].name, "email");
    }

    #[test]
    fn compute_caches_symbol_extraction_across_calls() {
        // Verify that calling compute() twice on the same DocumentStore content
        // produces the same DomainSymbol vec — and that the cache speeds up the
        // second call (we don't time it, but we DO verify the cached path returns
        // the same Arc-backed raw list).
        use crate::DocumentStore;

        let docs = DocumentStore::new();
        let uri: Uri = "file:///t.json".parse().unwrap();
        let source = r#"{"name":"user","columns":[{"name":"email","type":"text"}]}"#;
        docs.open(uri.clone(), "json".to_string(), 1, source.to_string());
        let first = compute("", &docs, None);
        let second = compute("", &docs, None);
        assert_eq!(first.len(), second.len(), "deterministic across calls");
        assert!(first.iter().any(|s| s.name == "email"));
    }

    #[test]
    fn extract_raw_symbols_returns_empty_without_mapping_or_table_name() {
        let pool = ParserPool::new();
        let scalar = "just_a_scalar\n";
        let scalar_tree = pool.parse(scalar, DocumentFormat::Yaml).unwrap();
        assert!(extract_raw_symbols(&scalar_tree, scalar).is_empty());

        let nameless = r#"{"columns":[]}"#;
        let nameless_tree = pool.parse(nameless, DocumentFormat::Json).unwrap();
        assert!(extract_raw_symbols(&nameless_tree, nameless).is_empty());
    }

    #[test]
    fn extract_raw_symbols_handles_missing_or_non_array_columns_and_nameless_column() {
        let pool = ParserPool::new();
        let no_columns = r#"{"name":"user"}"#;
        let no_columns_tree = pool.parse(no_columns, DocumentFormat::Json).unwrap();
        let symbols = extract_raw_symbols(&no_columns_tree, no_columns);
        assert_eq!(symbols.len(), 1);

        let non_array = r#"{"name":"user","columns":"oops"}"#;
        let non_array_tree = pool.parse(non_array, DocumentFormat::Json).unwrap();
        let symbols = extract_raw_symbols(&non_array_tree, non_array);
        assert_eq!(symbols.len(), 1);

        let nameless_column = r#"{"name":"user","columns":[{"type":"integer"}]}"#;
        let nameless_column_tree = pool.parse(nameless_column, DocumentFormat::Json).unwrap();
        let symbols = extract_raw_symbols(&nameless_column_tree, nameless_column);
        assert_eq!(symbols.len(), 1);
    }

    #[test]
    fn extract_raw_symbols_trims_yaml_quoted_identifier_ranges() {
        let pool = ParserPool::new();
        let src = "name: \"user\"\ncolumns:\n  - name: 'id'\n    type: integer\n";
        let tree = pool.parse(src, DocumentFormat::Yaml).unwrap();
        let symbols = extract_raw_symbols(&tree, src);

        let table = symbols
            .iter()
            .find(|symbol| symbol.kind == SymbolKind::Table)
            .expect("table symbol");
        let column = symbols
            .iter()
            .find(|symbol| symbol.kind == SymbolKind::Column)
            .expect("column symbol");

        assert_eq!(&src[table.byte_range.clone()], "user");
        assert_eq!(&src[column.byte_range.clone()], "id");
    }

    #[test]
    fn unwrap_yaml_and_trim_one_byte_defensive_branches() {
        let pool = ParserPool::new();
        let src = "name:\n";
        let tree = pool.parse(src, DocumentFormat::Yaml).unwrap();
        if let Some(pair) = find_pair_with_key(
            find_outer_mapping(tree.root_node()).unwrap(),
            src.as_bytes(),
            "name",
        ) && let Some(value) = pair.named_child(1)
        {
            let _ = unwrap_yaml_node(value);
        }

        assert_eq!(trim_one_byte(&(4..5)), 4..5);
    }

    #[test]
    fn workspace_symbol_disk_scan_skips_unreadable_model_after_refresh() {
        let tmp = tempdir().unwrap();
        let model = r#"{"name":"user","columns":[{"name":"id","type":"integer","nullable":false,"primary_key":true}]}"#;
        write_workspace(tmp.path(), &[("user.json", model)]);
        let tables = WorkspaceTables::new();
        assert!(tables.refresh(tmp.path()));
        fs::remove_file(tmp.path().join("models").join("user.json")).unwrap();

        let docs = DocumentStore::new();
        let entries = build_workspace_symbol_list(&docs, Some(&tables));
        assert!(
            entries.is_empty(),
            "deleted disk model should be skipped: {entries:?}"
        );
    }

    #[test]
    fn workspace_symbol_disk_scan_skips_paths_that_are_not_valid_uris() {
        let tmp = tempdir().unwrap();
        let model = r#"{"name":"user","columns":[{"name":"id","type":"integer","nullable":false,"primary_key":true}]}"#;
        write_workspace(tmp.path(), &[("user model.json", model)]);
        let tables = WorkspaceTables::new();
        assert!(tables.refresh(tmp.path()));

        let docs = DocumentStore::new();
        let entries = build_workspace_symbol_list(&docs, Some(&tables));
        assert!(
            entries.is_empty(),
            "space-containing path should fail raw URI construction and be skipped: {entries:?}"
        );
    }

    #[test]
    fn unwrap_yaml_node_handles_empty_wrapper_node() {
        let pool = ParserPool::new();
        let src = "name:\n";
        let tree = pool.parse(src, DocumentFormat::Yaml).unwrap();
        if let Some(wrapper) = find_empty_yaml_wrapper(tree.root_node()) {
            let unwrapped = unwrap_yaml_node(wrapper);
            assert_eq!(unwrapped.id(), wrapper.id());
        }
    }

    /// After the L263 restructure (`while-let` fuses the empty-wrapper and
    /// kind-mismatch exits), `unwrap_yaml_node` is exit-via-fallthrough only.
    /// This regression test asserts the fused loop still returns the original
    /// node for both a non-wrapper input and a fully nested wrapper chain.
    #[test]
    fn unwrap_yaml_node_returns_input_on_non_wrapper_kind() {
        let pool = ParserPool::new();
        let src = "name: user\ncolumns:\n  - name: id\n    type: integer\n";
        let tree = pool.parse(src, DocumentFormat::Yaml).unwrap();
        let root = tree.root_node();
        // Root is `stream`, not `flow_node`/`block_node` — must return unchanged.
        assert_eq!(unwrap_yaml_node(root).id(), root.id());

        // Visit every flow_node/block_node in the tree and confirm the unwrap
        // terminates at a node whose kind is NOT a wrapper (peeled cleanly).
        let mut found_wrapper = false;
        walk_assert_unwrap_terminates(root, &mut found_wrapper);
        assert!(
            found_wrapper,
            "test fixture must contain at least one yaml wrapper node"
        );
    }

    fn walk_assert_unwrap_terminates(node: tree_sitter::Node<'_>, found_wrapper: &mut bool) {
        if matches!(node.kind(), "flow_node" | "block_node") {
            *found_wrapper = true;
            let unwrapped = unwrap_yaml_node(node);
            assert!(
                !matches!(unwrapped.kind(), "flow_node" | "block_node")
                    || unwrapped.id() == node.id(),
                "unwrap must either peel to a non-wrapper or return itself when peel impossible"
            );
        }
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            walk_assert_unwrap_terminates(child, found_wrapper);
        }
    }

    #[test]
    fn workspace_symbols_shared_returns_same_arc_on_warm_cache() {
        let docs = DocumentStore::new();
        docs.open(
            uri("t.json"),
            "json".to_string(),
            1,
            r#"{"name":"x","columns":[{"name":"id","type":"integer"}]}"#.to_string(),
        );

        let first = compute_shared("", &docs, None);
        let second = compute_shared("", &docs, None);

        assert!(Arc::ptr_eq(&first, &second), "warm cache returns same Arc");
        assert!(first.iter().any(|s| s.name == "x"));
    }

    #[test]
    fn workspace_symbols_filter_excludes_non_matches() {
        let docs = DocumentStore::new();
        docs.open(
            uri("t.json"),
            "json".to_string(),
            1,
            r#"{"name":"user","columns":[{"name":"email","type":"text"}]}"#.to_string(),
        );

        assert!(compute("xyz_nothing_matches", &docs, None).is_empty());
    }
}
