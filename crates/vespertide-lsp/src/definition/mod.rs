//! Go-to-definition — pure domain layer.

mod foreign_key;

use std::ops::Range;

use tower_lsp_server::ls_types::Uri;

use crate::store::DocumentStore;
use crate::tree_util::node_at_byte;
use crate::workspace_index::WorkspaceIndex;
use crate::workspace_tables::WorkspaceTables;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DomainLocation {
    pub uri: Uri,
    pub byte_range: Range<usize>,
}

/// Compute definition target at byte offset. Cross-file references are resolved
/// through [`WorkspaceIndex`].
#[must_use]
pub fn compute(
    source: &str,
    tree: Option<&tree_sitter::Tree>,
    index: &WorkspaceIndex,
    docs: &DocumentStore,
    byte_offset: usize,
) -> Option<DomainLocation> {
    compute_with_workspace_tables(source, tree, index, docs, None, byte_offset)
}

/// Compute definition target including disk-discovered tables. Falls back to
/// the on-disk model file when the referenced table is not currently open.
#[must_use]
pub fn compute_with_workspace_tables(
    source: &str,
    tree: Option<&tree_sitter::Tree>,
    index: &WorkspaceIndex,
    docs: &DocumentStore,
    disk_tables: Option<&WorkspaceTables>,
    byte_offset: usize,
) -> Option<DomainLocation> {
    let tree = tree?;
    let node = node_at_byte(tree, byte_offset)?;
    foreign_key::try_definition(node, source, index, docs, disk_tables)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::{DocumentFormat, ParserPool};
    use crate::store::DocumentStore;
    use crate::test_support::uri;
    use crate::workspace_index::WorkspaceIndex;

    #[test]
    fn cross_file_go_to_def() {
        let pool = ParserPool::new();
        let idx = WorkspaceIndex::new();
        let docs = DocumentStore::new();

        let user_uri = uri("user.json");
        let user_src = r#"{"name":"user","columns":[{"name":"id","type":"integer","nullable":false,"primary_key":true}]}"#;
        let user_tree = pool.parse(user_src, DocumentFormat::Json).unwrap();
        idx.upsert(&user_uri, user_src, &user_tree);
        docs.open(
            user_uri.clone(),
            "json".to_string(),
            1,
            user_src.to_string(),
        );

        let post_src = r#"{"name":"post","columns":[{"name":"author_id","type":"integer","nullable":false,"foreign_key":{"ref_table":"user","ref_columns":["id"]}}]}"#;
        let post_tree = pool.parse(post_src, DocumentFormat::Json);

        let pos = post_src.find(r#""ref_table":"user""#).unwrap() + 14;
        let location = compute(post_src, post_tree.as_ref(), &idx, &docs, pos);
        assert!(location.is_some(), "definition should resolve to user.json");
        assert_eq!(location.unwrap().uri, user_uri);
    }

    #[test]
    fn unknown_ref_table_returns_none() {
        let pool = ParserPool::new();
        let idx = WorkspaceIndex::new();
        let docs = DocumentStore::new();
        let src = r#"{"name":"x","columns":[{"name":"a","type":"integer","nullable":false,"foreign_key":{"ref_table":"nonexistent","ref_columns":["id"]}}]}"#;
        let tree = pool.parse(src, DocumentFormat::Json);
        let pos = src.find("nonexistent").unwrap() + 2;
        let location = compute(src, tree.as_ref(), &idx, &docs, pos);
        assert!(location.is_none());
    }

    #[test]
    fn cross_file_ref_columns_resolves_to_target_column() {
        let pool = ParserPool::new();
        let idx = WorkspaceIndex::new();
        let docs = DocumentStore::new();

        let user_uri = uri("user.json");
        let user_src = r#"{"name":"user","columns":[{"name":"id","type":"integer","nullable":false,"primary_key":true},{"name":"email","type":"text","nullable":false}]}"#;
        let user_tree = pool.parse(user_src, DocumentFormat::Json).unwrap();
        idx.upsert(&user_uri, user_src, &user_tree);
        docs.open(
            user_uri.clone(),
            "json".to_string(),
            1,
            user_src.to_string(),
        );

        let post_src = r#"{"name":"post","columns":[{"name":"author_email","type":"text","foreign_key":{"ref_table":"user","ref_columns":["email"]}}]}"#;
        let post_tree = pool.parse(post_src, DocumentFormat::Json);
        // Cursor INSIDE the `"email"` element of ref_columns.
        let pos = post_src.find(r#""email""#).unwrap() + 3;

        let location = compute(post_src, post_tree.as_ref(), &idx, &docs, pos)
            .expect("ref_columns entry should resolve to its target column");
        assert_eq!(location.uri, user_uri);
        // Range should pinpoint the user.email column's `name` value, not 0..0.
        let snippet = &user_src[location.byte_range.clone()];
        assert!(
            snippet.contains("email"),
            "expected target range to highlight the `email` column, got: {snippet}"
        );
    }

    #[test]
    fn yaml_cross_file_ref_table_resolves_to_target_name() {
        let pool = ParserPool::new();
        let idx = WorkspaceIndex::new();
        let docs = DocumentStore::new();

        let user_uri = uri("user.yaml");
        let user_src =
            "name: user\ncolumns:\n  - name: id\n    type: integer\n    primary_key: true\n";
        let user_tree = pool.parse(user_src, DocumentFormat::Yaml).unwrap();
        idx.upsert(&user_uri, user_src, &user_tree);
        docs.open(
            user_uri.clone(),
            "yaml".to_string(),
            1,
            user_src.to_string(),
        );

        let post_src = "name: post\ncolumns:\n  - name: author_id\n    type: integer\n    foreign_key:\n      ref_table: user\n      ref_columns: [id]\n";
        let post_tree = pool.parse(post_src, DocumentFormat::Yaml);
        let pos = post_src.find("ref_table: user").unwrap() + 12;

        let location = compute(post_src, post_tree.as_ref(), &idx, &docs, pos)
            .expect("YAML ref_table should resolve to user.yaml");
        assert_eq!(location.uri, user_uri);
        let snippet = &user_src[location.byte_range.clone()];
        assert!(
            snippet.contains("user"),
            "target range should land on top-level `user` name, got: {snippet}"
        );
    }

    #[test]
    fn yaml_cross_file_ref_columns_resolves_to_target_column() {
        let pool = ParserPool::new();
        let idx = WorkspaceIndex::new();
        let docs = DocumentStore::new();

        let user_uri = uri("user.yaml");
        let user_src = "name: user\ncolumns:\n  - name: id\n    type: integer\n    primary_key: true\n  - name: email\n    type: text\n";
        let user_tree = pool.parse(user_src, DocumentFormat::Yaml).unwrap();
        idx.upsert(&user_uri, user_src, &user_tree);
        docs.open(
            user_uri.clone(),
            "yaml".to_string(),
            1,
            user_src.to_string(),
        );

        let post_src = "name: post\ncolumns:\n  - name: author_email\n    type: text\n    foreign_key:\n      ref_table: user\n      ref_columns: [email]\n";
        let post_tree = pool.parse(post_src, DocumentFormat::Yaml);
        let pos = post_src.find("[email]").unwrap() + 2;

        let location = compute(post_src, post_tree.as_ref(), &idx, &docs, pos)
            .expect("YAML ref_columns entry should resolve to user.email");
        assert_eq!(location.uri, user_uri);
        let snippet = &user_src[location.byte_range.clone()];
        assert!(
            snippet.contains("email"),
            "target range should highlight user.email column name, got: {snippet}"
        );
    }

    #[test]
    fn ref_table_falls_back_to_disk_when_target_file_is_closed() {
        use std::fs;
        use tempfile::tempdir;

        use crate::workspace_tables::WorkspaceTables;

        let tmp = tempdir().unwrap();
        let models_dir = tmp.path().join("models");
        fs::create_dir_all(&models_dir).unwrap();
        fs::write(tmp.path().join("vespertide.json"), r#"{"modelsDir":"models","migrationsDir":"migrations","tableNamingCase":"snake","columnNamingCase":"snake","modelFormat":"json"}"#).unwrap();
        fs::write(models_dir.join("user.json"), r#"{"name":"user","columns":[{"name":"id","type":"integer","nullable":false,"primary_key":true}]}"#).unwrap();

        let disk = WorkspaceTables::new();
        assert!(disk.refresh(tmp.path()));

        let pool = ParserPool::new();
        let idx = WorkspaceIndex::new();
        let docs = DocumentStore::new();
        let post_src = r#"{"name":"post","columns":[{"name":"author_id","type":"integer","foreign_key":{"ref_table":"user","ref_columns":["id"]}}]}"#;
        let post_tree = pool.parse(post_src, DocumentFormat::Json);
        // Cursor inside `"user"` value of ref_table.
        let pos = post_src.find(r#""ref_table":"user""#).unwrap() + 14;

        let location = super::compute_with_workspace_tables(
            post_src,
            post_tree.as_ref(),
            &idx,
            &docs,
            Some(&disk),
            pos,
        )
        .expect("disk-only target should still resolve");
        let uri_str = location.uri.to_string();
        assert!(
            uri_str.starts_with("file://"),
            "uri should be a file URI, got: {uri_str}"
        );
        assert!(
            uri_str.contains("user.json"),
            "uri should point at the disk file, got: {uri_str}"
        );
    }
}
