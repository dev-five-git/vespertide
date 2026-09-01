//! Foreign-key hover: preview target table columns for `ref_table` values.

use crate::store::DocumentStore;
use crate::text_util::strip_quotes;
use crate::tree_util::is_pair;
use crate::workspace_index::WorkspaceIndex;
use crate::workspace_tables::WorkspaceTables;

use super::DomainHover;

pub(super) fn try_hover(
    node: tree_sitter::Node<'_>,
    source: &str,
    index: &WorkspaceIndex,
    docs: &DocumentStore,
    disk_tables: Option<&WorkspaceTables>,
) -> Option<DomainHover> {
    let pair = ref_table_pair(node, source)?;
    let value = pair.named_child(1)?;
    let target_name = strip_quotes(&source[value.byte_range()]).to_string();

    // Prefer an OPEN document (carries the user's current unsaved edits).
    if let Some(loc) = index.lookup(&target_name) {
        let preview = docs
            .with_doc(&loc.uri, |text, _tree| extract_column_summary(text))
            .unwrap_or_default();
        let detail = if preview.is_empty() {
            "_columns unavailable_".to_string()
        } else {
            preview
        };
        return Some(DomainHover {
            markdown: format!("**Target table**: `{target_name}`\n\n{detail}"),
            byte_range: value.byte_range(),
        });
    }

    // Fall back to on-disk discovery so closed model files still preview.
    if let Some(disk) = disk_tables
        && let Some(table) = disk.get(&target_name)
    {
        let preview = column_summary(&table);
        return Some(DomainHover {
            markdown: format!("**Target table**: `{target_name}` _(on disk)_\n\n{preview}"),
            byte_range: value.byte_range(),
        });
    }

    Some(DomainHover {
        markdown: format!("**Target table**: `{target_name}`\n\n⚠ table not found in workspace"),
        byte_range: value.byte_range(),
    })
}

fn ref_table_pair<'tree>(
    node: tree_sitter::Node<'tree>,
    source: &str,
) -> Option<tree_sitter::Node<'tree>> {
    let mut cur = Some(node);
    while let Some(candidate) = cur {
        if is_pair(candidate)
            && let Some(key) = candidate.named_child(0)
            && strip_quotes(&source[key.byte_range()]) == "ref_table"
        {
            return Some(candidate);
        }
        cur = candidate.parent();
    }
    None
}

fn extract_column_summary(text: &str) -> String {
    match serde_json::from_str::<vespertide_core::TableDef>(text) {
        Ok(table) => column_summary(&table),
        Err(_) => match serde_yaml::from_str::<vespertide_core::TableDef>(text) {
            Ok(table) => column_summary(&table),
            Err(_) => String::new(),
        },
    }
}

fn column_summary(table: &vespertide_core::TableDef) -> String {
    let columns = table
        .columns
        .iter()
        .map(|column| column.name.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    format!("columns: {columns}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::{DocumentFormat, ParserPool};
    use crate::store::DocumentStore;
    use crate::test_support::uri;
    use crate::workspace_index::WorkspaceIndex;

    fn hover_at_ref_table(
        post_src: &str,
        idx: &WorkspaceIndex,
        docs: &DocumentStore,
        disk: Option<&WorkspaceTables>,
    ) -> Option<DomainHover> {
        let pool = ParserPool::new();
        let tree = pool.parse(post_src, DocumentFormat::Json).unwrap();
        let pos = post_src.find(r#""ref_table":""#).unwrap() + 14;
        let node = tree
            .root_node()
            .descendant_for_byte_range(pos, pos)
            .unwrap();
        try_hover(node, post_src, idx, docs, disk)
    }

    #[test]
    fn hover_ref_table_target_open_but_unparseable_shows_columns_unavailable() {
        // The `user.json` document is opened with content that is NOT a
        // valid TableDef (it's a free-form blob). `extract_column_summary`
        // returns an empty string → `preview.is_empty()` → "_columns unavailable_".
        let pool = ParserPool::new();
        let idx = WorkspaceIndex::new();
        let docs = DocumentStore::new();

        let user_uri = uri("user.json");
        // Register the table name in the index using a minimal parseable doc,
        // but open the document with garbage content so the column summary
        // extraction fails.
        let index_src = r#"{"name":"user"}"#;
        let index_tree = pool.parse(index_src, DocumentFormat::Json).unwrap();
        idx.upsert(&user_uri, index_src, &index_tree);
        docs.open(
            user_uri,
            "json".to_string(),
            1,
            "this is not a json table at all".to_string(),
        );

        let post_src = r#"{"name":"post","columns":[{"name":"a","type":"integer","foreign_key":{"ref_table":"user","ref_columns":["id"]}}]}"#;
        let hover = hover_at_ref_table(post_src, &idx, &docs, None)
            .expect("FK hover must produce some hover");
        assert!(
            hover.markdown.contains("_columns unavailable_"),
            "open-but-unparseable target should report `columns unavailable`, got: {}",
            hover.markdown
        );
    }

    #[test]
    fn hover_ref_table_unknown_shows_not_found_message() {
        let idx = WorkspaceIndex::new();
        let docs = DocumentStore::new();
        // The post.json references `ghost`, which is not in the index and
        // not on disk — exercises the "table not found" fallback branch.
        let post_src = r#"{"name":"post","columns":[{"name":"a","type":"integer","foreign_key":{"ref_table":"ghost","ref_columns":["id"]}}]}"#;
        let hover = hover_at_ref_table(post_src, &idx, &docs, None)
            .expect("FK hover must produce a hover even on unknown ref_table");
        assert!(
            hover.markdown.contains("not found in workspace"),
            "unknown ref_table must surface the `not found` fallback, got: {}",
            hover.markdown
        );
        assert!(
            hover.markdown.contains("ghost"),
            "fallback markdown should mention the missing target, got: {}",
            hover.markdown
        );
    }
}
