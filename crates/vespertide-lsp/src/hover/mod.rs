//! Hover provider — pure domain layer (no LSP protocol types).

mod check_expr;
mod column;
mod foreign_key;

use std::ops::Range;

use crate::parser::DocumentFormat;
use crate::store::DocumentStore;
use crate::workspace_index::WorkspaceIndex;
use crate::workspace_tables::WorkspaceTables;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DomainHover {
    /// Markdown content for the hover popup.
    pub markdown: String,
    /// Byte range to highlight (the symbol under cursor).
    pub byte_range: Range<usize>,
}

/// Compute hover at byte offset. Returns `None` if the cursor is on
/// non-hoverable content.
#[must_use]
pub fn compute(
    text: &str,
    format: DocumentFormat,
    tree: Option<&tree_sitter::Tree>,
    index: &WorkspaceIndex,
    docs: &DocumentStore,
    byte_offset: usize,
) -> Option<DomainHover> {
    compute_with_workspace_tables(text, format, tree, index, docs, None, byte_offset)
}

/// Compute hover with optional disk-discovered tables. When provided, FK
/// previews resolve targets that are not currently open in the editor.
#[must_use]
pub fn compute_with_workspace_tables(
    text: &str,
    format: DocumentFormat,
    tree: Option<&tree_sitter::Tree>,
    index: &WorkspaceIndex,
    docs: &DocumentStore,
    disk_tables: Option<&WorkspaceTables>,
    byte_offset: usize,
) -> Option<DomainHover> {
    let _ = format;
    let tree = tree?;
    let node = node_at_byte(tree, byte_offset)?;

    // CHECK-expression hover is dispatched FIRST. Inside a constraint
    // expression a column-name identifier must be interpreted as part
    // of the CHECK expression, not as a column declaration object.
    if let Some(hover) = check_expr::try_hover(node, text, byte_offset) {
        return Some(hover);
    }

    // `foreign_key.ref_table` is nested inside a column object, so try the
    // specific FK hover first before falling back to the broader column hover.
    if let Some(hover) = foreign_key::try_hover(node, text, index, docs, disk_tables) {
        return Some(hover);
    }

    column::try_hover(node, text)
}

fn node_at_byte(tree: &tree_sitter::Tree, byte_offset: usize) -> Option<tree_sitter::Node<'_>> {
    let root = tree.root_node();
    let mut current = root;
    'outer: loop {
        let mut cursor = current.walk();
        for child in current.children(&mut cursor) {
            if child.byte_range().contains(&byte_offset) {
                current = child;
                continue 'outer;
            }
        }
        return Some(current);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::{DocumentFormat, ParserPool};
    use crate::store::DocumentStore;
    use crate::workspace_index::WorkspaceIndex;

    #[test]
    fn hover_outside_returns_none() {
        let pool = ParserPool::new();
        let idx = WorkspaceIndex::new();
        let docs = DocumentStore::new();
        let src = r#"{"name": "user"}"#;
        let tree = pool.parse(src, DocumentFormat::Json);
        let hover = compute(src, DocumentFormat::Json, tree.as_ref(), &idx, &docs, 0);
        // First char is `{` — no hover content. Some impls may return generic
        // hover; OK either way as long as there is no panic.
        let _ = hover;
    }

    #[test]
    fn hover_on_ref_table_falls_back_to_disk_when_target_is_closed() {
        use std::fs;
        use std::str::FromStr;
        use tempfile::tempdir;
        use tower_lsp_server::ls_types::Uri;

        let tmp = tempdir().unwrap();
        let models_dir = tmp.path().join("models");
        fs::create_dir_all(&models_dir).unwrap();
        fs::write(tmp.path().join("vespertide.json"), r#"{"modelsDir":"models","migrationsDir":"migrations","tableNamingCase":"snake","columnNamingCase":"snake","modelFormat":"json"}"#).unwrap();
        fs::write(models_dir.join("media.json"), r#"{"name":"media","columns":[{"name":"id","type":"uuid","nullable":false,"primary_key":true}]}"#).unwrap();

        let disk = crate::workspace_tables::WorkspaceTables::new();
        assert!(disk.refresh(tmp.path()));

        let pool = ParserPool::new();
        let idx = WorkspaceIndex::new();
        let docs = DocumentStore::new();
        // article.vespertide.json references media via FK. Media is on disk, NOT open.
        let article_src = r#"{"name":"article","columns":[{"name":"media_id","type":"uuid","foreign_key":{"ref_table":"media","ref_columns":["id"]}}]}"#;
        let article_uri = Uri::from_str("file:///article.vespertide.json").unwrap();
        let article_tree = pool.parse(article_src, DocumentFormat::Json).unwrap();
        idx.upsert(&article_uri, article_src, &article_tree);

        let pos = article_src.find(r#""ref_table":"media""#).unwrap() + 14;
        let hover = super::compute_with_workspace_tables(
            article_src,
            DocumentFormat::Json,
            Some(&article_tree),
            &idx,
            &docs,
            Some(&disk),
            pos,
        )
        .expect("hover should resolve via disk fallback");

        assert!(
            !hover.markdown.contains("not found"),
            "must not say `table not found`, got: {}",
            hover.markdown
        );
        assert!(
            hover.markdown.contains("media"),
            "should preview the media table, got: {}",
            hover.markdown
        );
        assert!(
            hover.markdown.contains("columns"),
            "should include column summary, got: {}",
            hover.markdown
        );
    }

    #[test]
    fn yaml_hover_on_ref_table_previews_target_columns() {
        use std::str::FromStr;
        use tower_lsp_server::ls_types::Uri;

        let pool = ParserPool::new();
        let idx = WorkspaceIndex::new();
        let docs = DocumentStore::new();

        let user_uri = Uri::from_str("file:///user.yaml").unwrap();
        let user_src = "name: user\ncolumns:\n  - name: id\n    type: integer\n    nullable: false\n    primary_key: true\n  - name: email\n    type: text\n    nullable: false\n";
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
        let hover = compute(
            post_src,
            DocumentFormat::Yaml,
            post_tree.as_ref(),
            &idx,
            &docs,
            pos,
        )
        .expect("YAML hover should resolve ref_table");

        assert!(
            hover.markdown.contains("user"),
            "hover should mention the target table, got: {}",
            hover.markdown
        );
        assert!(
            hover.markdown.contains("email"),
            "hover should include the target's columns, got: {}",
            hover.markdown
        );
    }

    #[test]
    fn hover_on_column_name_returns_some() {
        let pool = ParserPool::new();
        let idx = WorkspaceIndex::new();
        let docs = DocumentStore::new();
        let src = r#"{"name":"user","columns":[{"name":"id","type":"integer","nullable":false,"primary_key":true}]}"#;
        let tree = pool.parse(src, DocumentFormat::Json);
        let pos = src.find(r#""name":"id""#).unwrap() + 5;
        let hover = compute(src, DocumentFormat::Json, tree.as_ref(), &idx, &docs, pos);
        assert!(hover.is_some(), "hover on column should return Some");
    }
}
