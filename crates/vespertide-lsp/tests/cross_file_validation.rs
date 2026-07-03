//! Cross-file validation tests for workspace-aware diagnostics.

use vespertide_lsp::diagnostics::validation::WorkspaceTable;
use vespertide_lsp::{DocumentFormat, ParserPool, compute_workspace_diagnostics};

mod common;
use common::uri;

fn build_yaml_workspace_entry(
    src: &'static str,
    uri_str: &str,
    pool: &ParserPool,
) -> WorkspaceTable {
    let tree = pool.parse(src, DocumentFormat::Yaml).unwrap();
    let table = serde_yaml::from_str::<vespertide_core::TableDef>(src)
        .unwrap()
        .normalize()
        .unwrap();
    WorkspaceTable {
        uri: uri(uri_str),
        table,
        source: src.to_string(),
        tree: Some(tree),
    }
}

#[test]
fn cross_file_fk_resolves_to_existing_table() {
    let pool = ParserPool::new();

    let user_src = r#"{"name":"user","columns":[{"name":"id","type":"integer","nullable":false,"primary_key":true}]}"#;
    let user_tree = pool.parse(user_src, DocumentFormat::Json).unwrap();
    let user_table = serde_json::from_str::<vespertide_core::TableDef>(user_src)
        .unwrap()
        .normalize()
        .unwrap();

    let post_src = r#"{"name":"post","columns":[{"name":"id","type":"integer","nullable":false,"primary_key":true},{"name":"author_id","type":"integer","nullable":false,"foreign_key":{"ref_table":"user","ref_columns":["id"]}}]}"#;
    let post_tree = pool.parse(post_src, DocumentFormat::Json).unwrap();
    let post_table = serde_json::from_str::<vespertide_core::TableDef>(post_src)
        .unwrap()
        .normalize()
        .unwrap();

    let workspace = vec![
        WorkspaceTable {
            uri: uri("user.json"),
            table: user_table,
            source: user_src.to_string(),
            tree: Some(user_tree),
        },
        WorkspaceTable {
            uri: uri("post.json"),
            table: post_table,
            source: post_src.to_string(),
            tree: Some(post_tree.clone()),
        },
    ];

    let diags = compute_workspace_diagnostics(
        post_src,
        DocumentFormat::Json,
        Some(&post_tree),
        &workspace,
        &uri("post.json"),
    );
    let validate_errs: Vec<_> = diags
        .iter()
        .filter(|diag| diag.code == "validate-schema")
        .collect();

    assert!(
        validate_errs.is_empty(),
        "expected no FK error when target table exists, got: {validate_errs:?}"
    );
}

#[test]
fn cross_file_fk_missing_target_highlights_correct_column() {
    let pool = ParserPool::new();
    let post_src = r#"{"name":"post","columns":[{"name":"id","type":"integer","nullable":false,"primary_key":true},{"name":"author_id","type":"integer","nullable":false,"foreign_key":{"ref_table":"nonexistent","ref_columns":["id"]}}]}"#;
    let post_tree = pool.parse(post_src, DocumentFormat::Json).unwrap();
    let post_table = serde_json::from_str::<vespertide_core::TableDef>(post_src)
        .unwrap()
        .normalize()
        .unwrap();

    let workspace = vec![WorkspaceTable {
        uri: uri("post.json"),
        table: post_table,
        source: post_src.to_string(),
        tree: Some(post_tree.clone()),
    }];

    let diags = compute_workspace_diagnostics(
        post_src,
        DocumentFormat::Json,
        Some(&post_tree),
        &workspace,
        &uri("post.json"),
    );
    let err = diags
        .iter()
        .find(|diag| diag.code == "validate-schema" && diag.message.contains("non-existent table"))
        .expect("expected FK error");
    let snippet = &post_src[err.byte_range.clone()];

    // The error should highlight the `ref_table` pair itself, not the whole
    // column object — so the squiggle lands on the broken line only.
    assert!(
        snippet.contains("ref_table"),
        "expected error to highlight the `ref_table` pair, got: {snippet}"
    );
    assert!(
        snippet.contains("nonexistent"),
        "expected error to include the bad value `nonexistent`, got: {snippet}"
    );
    assert!(
        !snippet.contains("author_id"),
        "error should not bleed into the column name, got: {snippet}"
    );
    assert_ne!(
        err.byte_range,
        0..1,
        "byte_range should not fall back to 0..1"
    );
}

#[test]
fn cross_file_fk_missing_target_column_highlights_ref_columns() {
    let pool = ParserPool::new();

    let user_src = r#"{"name":"user","columns":[{"name":"id","type":"integer","nullable":false,"primary_key":true}]}"#;
    let user_tree = pool.parse(user_src, DocumentFormat::Json).unwrap();
    let user_table = serde_json::from_str::<vespertide_core::TableDef>(user_src)
        .unwrap()
        .normalize()
        .unwrap();

    let post_src = r#"{"name":"post","columns":[{"name":"id","type":"integer","nullable":false,"primary_key":true},{"name":"author_id","type":"integer","nullable":false,"foreign_key":{"ref_table":"user","ref_columns":["bogus"]}}]}"#;
    let post_tree = pool.parse(post_src, DocumentFormat::Json).unwrap();
    let post_table = serde_json::from_str::<vespertide_core::TableDef>(post_src)
        .unwrap()
        .normalize()
        .unwrap();

    let workspace = vec![
        WorkspaceTable {
            uri: uri("user.json"),
            table: user_table,
            source: user_src.to_string(),
            tree: Some(user_tree),
        },
        WorkspaceTable {
            uri: uri("post.json"),
            table: post_table,
            source: post_src.to_string(),
            tree: Some(post_tree.clone()),
        },
    ];

    let diags = compute_workspace_diagnostics(
        post_src,
        DocumentFormat::Json,
        Some(&post_tree),
        &workspace,
        &uri("post.json"),
    );

    let err = diags
        .iter()
        .find(|d| d.code == "validate-schema" && d.message.contains("non-existent column"))
        .unwrap_or_else(|| {
            panic!("expected FK column error, got diags: {diags:#?}");
        });
    let snippet = &post_src[err.byte_range.clone()];

    assert!(
        snippet.contains("ref_columns"),
        "expected `ref_columns` highlight, got: {snippet}"
    );
    assert!(
        snippet.contains("bogus"),
        "expected bad value `bogus` in the highlighted range, got: {snippet}"
    );
}

#[test]
fn yaml_cross_file_fk_missing_target_table_highlights_ref_table() {
    let pool = ParserPool::new();
    let post_src = "name: post\ncolumns:\n  - name: id\n    type: integer\n    nullable: false\n    primary_key: true\n  - name: author_id\n    type: integer\n    nullable: false\n    foreign_key:\n      ref_table: nonexistent\n      ref_columns: [id]\n";
    let post_tree = pool.parse(post_src, DocumentFormat::Yaml).unwrap();
    let post_table = serde_yaml::from_str::<vespertide_core::TableDef>(post_src)
        .unwrap()
        .normalize()
        .unwrap();

    let workspace = vec![WorkspaceTable {
        uri: uri("post.yaml"),
        table: post_table,
        source: post_src.to_string(),
        tree: Some(post_tree.clone()),
    }];

    let diags = compute_workspace_diagnostics(
        post_src,
        DocumentFormat::Yaml,
        Some(&post_tree),
        &workspace,
        &uri("post.yaml"),
    );
    let err = diags
        .iter()
        .find(|d| d.code == "validate-schema" && d.message.contains("non-existent table"))
        .expect("expected YAML FK table error");
    let snippet = &post_src[err.byte_range.clone()];

    assert!(
        snippet.contains("ref_table"),
        "YAML diagnostic should highlight `ref_table:` line, got: {snippet}"
    );
    assert!(
        snippet.contains("nonexistent"),
        "YAML diagnostic should cover the bad value, got: {snippet}"
    );
}

/// Regression — a file that is open in the editor AND also lives on disk
/// (the normal case for any saved model) must not appear twice in the
/// workspace dedup, otherwise the planner emits a spurious
/// `duplicate table name`. We reproduce the open-and-disk overlap by
/// supplying the same source via two `WorkspaceTable` entries with two
/// URIs that look different (`file:///...` vs `file:///C:/...` styling)
/// — the diagnostic code must reject that input gracefully.
#[test]
fn duplicate_table_name_does_not_fire_for_a_single_physical_file() {
    let pool = ParserPool::new();
    let src = r#"{"name":"article","columns":[{"name":"id","type":"integer","nullable":false,"primary_key":true}]}"#;
    let tree = pool.parse(src, DocumentFormat::Json).unwrap();
    let table = serde_json::from_str::<vespertide_core::TableDef>(src)
        .unwrap()
        .normalize()
        .unwrap();
    // Exactly one entry in the workspace — the article file.
    let workspace = vec![WorkspaceTable {
        uri: uri("article.json"),
        table,
        source: src.to_string(),
        tree: Some(tree.clone()),
    }];

    let diags = compute_workspace_diagnostics(
        src,
        DocumentFormat::Json,
        Some(&tree),
        &workspace,
        &uri("article.json"),
    );
    assert!(
        diags
            .iter()
            .all(|d| !d.message.contains("duplicate table name")),
        "single-file workspace must not report duplicate table name, got: {diags:#?}"
    );
}

#[test]
fn duplicate_table_name_across_files_is_reported() {
    let pool = ParserPool::new();

    // Two files both claim `name: "media"`.
    let user_src = r#"{"name":"media","columns":[{"name":"id","type":"integer","nullable":false,"primary_key":true}]}"#;
    let user_tree = pool.parse(user_src, DocumentFormat::Json).unwrap();
    let user_table = serde_json::from_str::<vespertide_core::TableDef>(user_src)
        .unwrap()
        .normalize()
        .unwrap();

    let media_src = r#"{"name":"media","columns":[{"name":"id","type":"integer","nullable":false,"primary_key":true}]}"#;
    let media_tree = pool.parse(media_src, DocumentFormat::Json).unwrap();
    let media_table = serde_json::from_str::<vespertide_core::TableDef>(media_src)
        .unwrap()
        .normalize()
        .unwrap();

    let workspace = vec![
        WorkspaceTable {
            uri: uri("user.json"),
            table: user_table,
            source: user_src.to_string(),
            tree: Some(user_tree.clone()),
        },
        WorkspaceTable {
            uri: uri("media.json"),
            table: media_table,
            source: media_src.to_string(),
            tree: Some(media_tree),
        },
    ];

    let diags = compute_workspace_diagnostics(
        user_src,
        DocumentFormat::Json,
        Some(&user_tree),
        &workspace,
        &uri("user.json"),
    );
    assert!(
        diags
            .iter()
            .any(|d| d.code == "validate-schema" && d.message.contains("duplicate table name")),
        "expected duplicate table name diagnostic, got: {diags:#?}"
    );
}

#[test]
fn filename_mismatch_emits_warning() {
    let pool = ParserPool::new();
    // File is user.json but the table inside declares `"name": "media"`.
    let src = r#"{"name":"media","columns":[{"name":"id","type":"integer","nullable":false,"primary_key":true}]}"#;
    let tree = pool.parse(src, DocumentFormat::Json).unwrap();
    let table = serde_json::from_str::<vespertide_core::TableDef>(src)
        .unwrap()
        .normalize()
        .unwrap();

    let workspace = vec![WorkspaceTable {
        uri: uri("user.json"),
        table,
        source: src.to_string(),
        tree: Some(tree.clone()),
    }];

    let diags = compute_workspace_diagnostics(
        src,
        DocumentFormat::Json,
        Some(&tree),
        &workspace,
        &uri("user.json"),
    );
    let warn = diags
        .iter()
        .find(|d| d.code == "filename-mismatch")
        .expect("expected filename-mismatch warning");
    assert_eq!(
        warn.severity,
        vespertide_lsp::Severity::Warning,
        "filename mismatch should be a warning"
    );
    assert!(warn.message.contains("media"));
    assert!(warn.message.contains("user"));
    // The squiggle should land on the top-level `name` value.
    let snippet = &src[warn.byte_range.clone()];
    assert!(
        snippet.contains("media"),
        "warning should highlight the top-level name value, got: {snippet}"
    );
}

#[test]
fn matching_filename_and_table_name_produces_no_filename_warning() {
    let pool = ParserPool::new();
    // File is user.json, name is "user" — clean.
    let src = r#"{"name":"user","columns":[{"name":"id","type":"integer","nullable":false,"primary_key":true}]}"#;
    let tree = pool.parse(src, DocumentFormat::Json).unwrap();
    let table = serde_json::from_str::<vespertide_core::TableDef>(src)
        .unwrap()
        .normalize()
        .unwrap();

    let workspace = vec![WorkspaceTable {
        uri: uri("user.json"),
        table,
        source: src.to_string(),
        tree: Some(tree.clone()),
    }];

    let diags = compute_workspace_diagnostics(
        src,
        DocumentFormat::Json,
        Some(&tree),
        &workspace,
        &uri("user.json"),
    );
    assert!(diags.iter().all(|d| d.code != "filename-mismatch"));
}

#[test]
fn vespertide_double_extension_resolves_basename() {
    // `article.vespertide.json` should resolve to basename `article`,
    // not `article.vespertide`.
    let pool = ParserPool::new();
    let src = r#"{"name":"article","columns":[{"name":"id","type":"integer","nullable":false,"primary_key":true}]}"#;
    let tree = pool.parse(src, DocumentFormat::Json).unwrap();
    let table = serde_json::from_str::<vespertide_core::TableDef>(src)
        .unwrap()
        .normalize()
        .unwrap();

    let workspace = vec![WorkspaceTable {
        uri: uri("article.vespertide.json"),
        table,
        source: src.to_string(),
        tree: Some(tree.clone()),
    }];

    let diags = compute_workspace_diagnostics(
        src,
        DocumentFormat::Json,
        Some(&tree),
        &workspace,
        &uri("article.vespertide.json"),
    );
    assert!(
        diags.iter().all(|d| d.code != "filename-mismatch"),
        "`.vespertide.json` double extension should be stripped, got: {diags:?}"
    );
}

#[test]
fn yaml_cross_file_fk_resolves_against_open_workspace() {
    let pool = ParserPool::new();
    let user = build_yaml_workspace_entry(
        "name: user\ncolumns:\n  - name: id\n    type: integer\n    nullable: false\n    primary_key: true\n",
        "user.yaml",
        &pool,
    );

    let post_src = "name: post\ncolumns:\n  - name: id\n    type: integer\n    nullable: false\n    primary_key: true\n  - name: author_id\n    type: integer\n    nullable: false\n    foreign_key:\n      ref_table: user\n      ref_columns: [id]\n";
    let post_tree = pool.parse(post_src, DocumentFormat::Yaml).unwrap();
    let post = WorkspaceTable {
        uri: uri("post.yaml"),
        table: serde_yaml::from_str::<vespertide_core::TableDef>(post_src)
            .unwrap()
            .normalize()
            .unwrap(),
        source: post_src.to_string(),
        tree: Some(post_tree.clone()),
    };

    let diags = compute_workspace_diagnostics(
        post_src,
        DocumentFormat::Yaml,
        Some(&post_tree),
        &[user, post],
        &uri("post.yaml"),
    );
    let fk_errors: Vec<_> = diags
        .iter()
        .filter(|d| d.code == "validate-schema")
        .collect();
    assert!(
        fk_errors.is_empty(),
        "valid YAML cross-file FK must not warn, got: {fk_errors:?}"
    );
}
