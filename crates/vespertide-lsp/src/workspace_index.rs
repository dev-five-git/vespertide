//! Cross-file index: table name → URI.
//!
//! Built/maintained by parsing the top-level `name` field via tree-sitter
//! (fast path — no full serde deserialize). Updated on `did_open` /
//! `did_change` / `did_close`.
//!
//! [`BTreeMap`] is used for deterministic iteration (workspace policy; see
//! AGENTS.md). Concurrency is mediated by a single [`RwLock`] — write traffic
//! is rare (per document edit) so contention is not a concern.

use std::collections::BTreeMap;
use std::sync::RwLock;

use tower_lsp_server::ls_types::Uri;
use tree_sitter::{Node, Tree};

/// Snapshot returned by [`WorkspaceIndex::lookup`]; not held across mutations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableLocation {
    pub uri: Uri,
}

/// Workspace-wide table name → URI index.
#[derive(Debug, Default)]
pub struct WorkspaceIndex {
    inner: RwLock<WorkspaceIndexInner>,
}

#[derive(Debug, Default)]
struct WorkspaceIndexInner {
    /// `table_name` → URI
    by_name: BTreeMap<String, Uri>,
    /// URI → `table_name` (for cleanup on `did_change` / `did_close`)
    by_uri: BTreeMap<Uri, String>,
}

impl WorkspaceIndex {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Upsert: parse the document's top-level `name` field and update both
    /// maps. Returns the previous name (if any) for diagnostic display.
    ///
    /// # Panics
    /// Panics if the internal lock is poisoned (process-wide invariant).
    pub fn upsert(&self, uri: &Uri, source: &str, tree: &Tree) -> Option<String> {
        let new_name = extract_top_level_name(source, tree);
        let mut inner = self
            .inner
            .write()
            .expect("workspace_index lock poisoned — invariant: no panic while holding lock");
        let old = inner.by_uri.get(uri).cloned();
        if let Some(old_name) = &old {
            // Only remove the by_name entry if it still points to this URI
            // (handles the case where two files briefly claim the same name).
            if inner.by_name.get(old_name) == Some(uri) {
                inner.by_name.remove(old_name);
            }
        }
        match new_name {
            Some(name) => {
                inner.by_uri.insert(uri.clone(), name.clone());
                inner.by_name.insert(name, uri.clone());
            }
            None => {
                inner.by_uri.remove(uri);
            }
        }
        old
    }

    /// Drop all index entries for `uri`.
    ///
    /// # Panics
    /// Panics if the internal lock is poisoned.
    pub fn remove(&self, uri: &Uri) {
        let mut inner = self
            .inner
            .write()
            .expect("workspace_index lock poisoned — invariant: no panic while holding lock");
        if let Some(name) = inner.by_uri.remove(uri)
            && inner.by_name.get(&name) == Some(uri)
        {
            inner.by_name.remove(&name);
        }
    }

    /// Look up the URI that owns `table_name`.
    ///
    /// # Panics
    /// Panics if the internal lock is poisoned.
    #[must_use]
    pub fn lookup(&self, table_name: &str) -> Option<TableLocation> {
        let inner = self
            .inner
            .read()
            .expect("workspace_index lock poisoned — invariant: no panic while holding lock");
        inner
            .by_name
            .get(table_name)
            .map(|uri| TableLocation { uri: uri.clone() })
    }

    /// Snapshot of all known table names. Sorted by [`BTreeMap`] iteration order.
    ///
    /// # Panics
    /// Panics if the internal lock is poisoned.
    #[must_use]
    pub fn tables(&self) -> Vec<String> {
        let inner = self
            .inner
            .read()
            .expect("workspace_index lock poisoned — invariant: no panic while holding lock");
        inner.by_name.keys().cloned().collect()
    }

    /// Number of indexed tables.
    ///
    /// # Panics
    /// Panics if the internal lock is poisoned.
    #[must_use]
    pub fn len(&self) -> usize {
        self.inner
            .read()
            .expect("workspace_index lock poisoned — invariant: no panic while holding lock")
            .by_name
            .len()
    }

    /// `true` if no tables are indexed.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Walk the tree-sitter tree to find the TOP-LEVEL `name` string value.
///
/// Critically, we look only at direct children of the document's outermost
/// mapping. A previous version recursed unconditionally and would pick up
/// the `name` field of the first column when the file happened to put
/// `columns` before `name` — that polluted the workspace index with column
/// names like `id` and `media_id` as if they were tables.
fn extract_top_level_name(source: &str, tree: &Tree) -> Option<String> {
    let mapping = find_outer_mapping(tree.root_node())?;
    find_direct_name_value(mapping, source.as_bytes())
}

fn find_outer_mapping(node: Node<'_>) -> Option<Node<'_>> {
    if matches!(node.kind(), "object" | "block_mapping" | "flow_mapping") {
        return Some(node);
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if let Some(found) = find_outer_mapping(child) {
            return Some(found);
        }
    }
    None
}

fn find_direct_name_value(mapping: Node<'_>, source: &[u8]) -> Option<String> {
    let mut cursor = mapping.walk();
    for child in mapping.children(&mut cursor) {
        if is_name_key_node(child, source)
            && let Some(value) = find_value_sibling(child)
        {
            return source
                .get(value.byte_range())
                .and_then(|text| std::str::from_utf8(text).ok())
                .map(|text| strip_quotes(text).to_string());
        }
    }
    None
}

fn is_name_key_node(node: Node<'_>, source: &[u8]) -> bool {
    let kind = node.kind();
    matches!(kind, "pair" | "block_mapping_pair")
        && node
            .named_child(0)
            .and_then(|key| source.get(key.byte_range()))
            .and_then(|text| std::str::from_utf8(text).ok())
            .is_some_and(|key_str| strip_quotes(key_str.trim()) == "name")
}

fn find_value_sibling(pair_node: Node<'_>) -> Option<Node<'_>> {
    // JSON pair: key, ":", value → value is named_child(1)
    // YAML block_mapping_pair: key, ":", value → value is named_child(1)
    pair_node.named_child(1)
}

fn strip_quotes(s: &str) -> &str {
    let s = s.trim();
    s.trim_start_matches('"').trim_end_matches('"')
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::{DocumentFormat, ParserPool};
    use crate::test_support::uri;

    fn find_keyless_pair(node: Node<'_>) -> Option<Node<'_>> {
        if node.kind() == "pair" && node.named_child(0).is_none() {
            return Some(node);
        }
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if let Some(found) = find_keyless_pair(child) {
                return Some(found);
            }
        }
        None
    }

    #[test]
    fn upsert_json_name() {
        let pool = ParserPool::new();
        let idx = WorkspaceIndex::new();
        let u = uri("user.json");
        let src = r#"{"name": "user", "columns": []}"#;
        let tree = pool.parse(src, DocumentFormat::Json).unwrap();
        idx.upsert(&u, src, &tree);
        assert_eq!(idx.len(), 1);
        let loc = idx.lookup("user").unwrap();
        assert_eq!(loc.uri, u);
    }

    #[test]
    fn upsert_yaml_name() {
        let pool = ParserPool::new();
        let idx = WorkspaceIndex::new();
        let u = uri("user.yaml");
        let src = "name: user\ncolumns: []\n";
        let tree = pool.parse(src, DocumentFormat::Yaml).unwrap();
        idx.upsert(&u, src, &tree);
        assert_eq!(idx.len(), 1);
        assert!(idx.lookup("user").is_some());
    }

    #[test]
    fn rename_replaces_entry() {
        let pool = ParserPool::new();
        let idx = WorkspaceIndex::new();
        let u = uri("entity.json");
        let src1 = r#"{"name": "user"}"#;
        let src2 = r#"{"name": "account"}"#;
        let t1 = pool.parse(src1, DocumentFormat::Json).unwrap();
        idx.upsert(&u, src1, &t1);
        let t2 = pool.parse(src2, DocumentFormat::Json).unwrap();
        idx.upsert(&u, src2, &t2);
        assert_eq!(idx.len(), 1);
        assert!(idx.lookup("user").is_none());
        assert!(idx.lookup("account").is_some());
    }

    #[test]
    fn remove_clears_both_maps() {
        let pool = ParserPool::new();
        let idx = WorkspaceIndex::new();
        let u = uri("user.json");
        let src = r#"{"name": "user"}"#;
        let tree = pool.parse(src, DocumentFormat::Json).unwrap();
        idx.upsert(&u, src, &tree);
        idx.remove(&u);
        assert_eq!(idx.len(), 0);
        assert!(idx.lookup("user").is_none());
    }

    #[test]
    fn cjk_name() {
        let pool = ParserPool::new();
        let idx = WorkspaceIndex::new();
        let u = uri("doseo.json");
        let src = r#"{"name": "도서"}"#;
        let tree = pool.parse(src, DocumentFormat::Json).unwrap();
        idx.upsert(&u, src, &tree);
        assert!(idx.lookup("도서").is_some());
    }

    #[test]
    fn column_name_is_not_picked_up_when_columns_precedes_name() {
        let pool = ParserPool::new();
        let idx = WorkspaceIndex::new();
        let u = uri("article.json");
        // `columns` appears BEFORE `name`. A naive recursive walk would
        // surface `id` (the first column's name) as if it were a table.
        let src = r#"{
            "columns": [{"name": "id", "type": "uuid"}],
            "name": "article"
        }"#;
        let tree = pool.parse(src, DocumentFormat::Json).unwrap();
        idx.upsert(&u, src, &tree);

        let tables = idx.tables();
        assert_eq!(tables, vec!["article".to_string()]);
        assert!(idx.lookup("id").is_none(), "must NOT register column name");
        assert!(idx.lookup("article").is_some());
    }

    #[test]
    fn missing_name_is_no_op() {
        let pool = ParserPool::new();
        let idx = WorkspaceIndex::new();
        let u = uri("nameless.json");
        let src = r#"{"columns": []}"#;
        let tree = pool.parse(src, DocumentFormat::Json).unwrap();
        idx.upsert(&u, src, &tree);
        assert!(idx.is_empty());
    }

    #[test]
    fn malformed_pair_without_key_is_not_a_name_key() {
        let pool = ParserPool::new();
        let src = r#"{:"user"}"#;
        let tree = pool.parse(src, DocumentFormat::Json).unwrap();
        if let Some(pair) = find_keyless_pair(tree.root_node()) {
            assert!(!is_name_key_node(pair, src.as_bytes()));
        }
    }

    #[test]
    fn yaml_scalar_document_keeps_index_empty() {
        let idx = WorkspaceIndex::new();
        let pool = ParserPool::new();
        let src = "just_a_scalar\n";
        let tree = pool.parse(src, DocumentFormat::Yaml).unwrap();

        idx.upsert(&uri("scalar.yaml"), src, &tree);

        assert!(idx.is_empty());
    }

    #[test]
    fn tables_returns_sorted_names() {
        let idx = WorkspaceIndex::new();
        let pool = ParserPool::new();
        let z_tree = pool.parse(r#"{"name":"z"}"#, DocumentFormat::Json).unwrap();
        let a_tree = pool.parse(r#"{"name":"a"}"#, DocumentFormat::Json).unwrap();

        idx.upsert(&uri("z.json"), r#"{"name":"z"}"#, &z_tree);
        idx.upsert(&uri("a.json"), r#"{"name":"a"}"#, &a_tree);

        assert_eq!(idx.tables(), vec!["a".to_string(), "z".to_string()]);
    }
}
