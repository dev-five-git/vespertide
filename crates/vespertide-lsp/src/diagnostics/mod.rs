//! Diagnostics — pure domain layer.
//!
//! `DomainDiagnostic` has zero LSP types. The backend (or external callers)
//! translate to `tower_lsp_server::ls_types::Diagnostic` via `mapper`.
//!
//! Validation tiers:
//! 1. Tree-sitter syntax errors → `Severity::Error`
//! 2. serde parse failure → `Severity::Error`
//! 3. `vespertide_planner::validate_schema` (per-table) → `Severity::Error` / `Severity::Warning`
//! 4. (future, drift detection) → `Severity::Information`

use std::ops::Range;
use std::sync::{Arc, OnceLock};

use vespertide_core::TableDef;

use crate::cache::{RingCache, hash_text};
use crate::parser::DocumentFormat;

#[expect(
    clippy::match_same_arms,
    reason = "diagnostic locator groups semantically distinct planner variants by anchor shape for maintainability"
)]
pub mod locator;
pub mod mapper;
pub mod validation;

pub(crate) use locator::{
    ErrorField, ErrorLocation, locate_column, locate_column_field, locate_constraint,
    locate_top_name,
};
pub use validation::WorkspaceTable;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DomainDiagnostic {
    /// Byte range in source text [start, end).
    pub byte_range: Range<usize>,
    pub severity: Severity,
    pub message: String,
    /// Stable diagnostic code (e.g., "syntax-error", "fk-missing", "validate-schema").
    pub code: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
    Information,
    Hint,
}

/// Cached final-result vector for `compute(text, format, tree, _)`. The
/// public `compute_diagnostics` is a pure function of `(text, format)` —
/// the tree is derived from `(text, format)` so any tree-sitter walks
/// inside produce identical output for the same text. Cache the final
/// `Vec<DomainDiagnostic>` keyed on `(fxhash64(text), text.len(), format)`.
///
/// 128-slot ring buffer (matches HS-7's `SymbolCache`). On the 100-table
/// synthetic workload, the workload's 100,000 calls across 100 unique
/// texts produce ~100 misses + ~99,900 hits.
///
/// Note: `compute_workspace` is NOT cached here — it depends on
/// `&[WorkspaceTable]` which can change independently of the document
/// text. Workspace diagnostics are typically called once per `did_change`
/// per file, so the cache wouldn't help that path anyway.
type DiagnosticsKey = (u64, usize, DocumentFormat);
type DiagnosticsCache = RingCache<DiagnosticsKey, Vec<DomainDiagnostic>, 128>;

static DIAGNOSTICS_CACHE: OnceLock<DiagnosticsCache> = OnceLock::new();

fn diagnostics_cache() -> &'static DiagnosticsCache {
    DIAGNOSTICS_CACHE.get_or_init(DiagnosticsCache::new)
}

/// Compute diagnostics for a document. Pure function — no I/O, no LSP types.
#[must_use]
pub fn compute_shared(
    text: &str,
    format: DocumentFormat,
    tree: Option<&tree_sitter::Tree>,
) -> Arc<Vec<DomainDiagnostic>> {
    diagnostics_cache().get_or_compute((hash_text(text), text.len(), format), || {
        compute_uncached(text, format, tree)
    })
}

/// Compute diagnostics for a document. Pure function — no I/O, no LSP types.
#[must_use]
pub fn compute(
    text: &str,
    format: DocumentFormat,
    tree: Option<&tree_sitter::Tree>,
) -> Vec<DomainDiagnostic> {
    (*compute_shared(text, format, tree)).clone()
}

/// Uncached implementation used by the cache on miss.
fn compute_uncached(
    text: &str,
    format: DocumentFormat,
    tree: Option<&tree_sitter::Tree>,
) -> Vec<DomainDiagnostic> {
    let (mut diagnostics, parsed) = collect_parse_diagnostics(text, format, tree);

    // Tier 3: planner validation (only if serde succeeded).
    if let Some(table) = parsed {
        validation::validate_table(&table, tree, text, &mut diagnostics);
        // Tier 3.5: static safety analyses (warnings).
        validation::validate_fk_supporting_indexes(&table, tree, text, &mut diagnostics);
        validation::validate_sequence_exhaustion(&table, tree, text, &mut diagnostics);
        validation::validate_check_type_mismatches(&table, tree, text, &mut diagnostics);
        validation::validate_check_between_order(&table, tree, text, &mut diagnostics);
        validation::validate_check_self_contradiction(&table, tree, text, &mut diagnostics);
    }

    diagnostics
}

fn collect_parse_diagnostics(
    text: &str,
    format: DocumentFormat,
    tree: Option<&tree_sitter::Tree>,
) -> (Vec<DomainDiagnostic>, Option<TableDef>) {
    let mut diagnostics = Vec::new();

    // Tier 1: syntax errors from tree-sitter.
    if let Some(tree) = tree {
        validation::collect_all(tree, text, &mut diagnostics);
    }

    let had_typed_pre_check = had_typed_pre_check(&diagnostics);

    // Tier 2: serde parse.
    let parsed = if had_typed_pre_check {
        None
    } else {
        match format {
            DocumentFormat::Json => validation::try_parse_json(text, &mut diagnostics),
            DocumentFormat::Yaml => validation::try_parse_yaml(text, &mut diagnostics),
        }
    };

    (diagnostics, parsed)
}

/// True when at least one tree-sitter-level pre-pass already pinpointed a
/// type-shape error. Used to suppress redundant (and mis-positioned) serde
/// diagnostics for the same root cause.
fn had_typed_pre_check(diagnostics: &[DomainDiagnostic]) -> bool {
    diagnostics.iter().any(|d| {
        matches!(
            d.code.as_str(),
            "unknown-type" | "complex-type" | "duplicate-column"
        )
    })
}

/// Compute diagnostics with workspace context for cross-file validation.
#[must_use]
pub fn compute_workspace(
    text: &str,
    format: DocumentFormat,
    tree: Option<&tree_sitter::Tree>,
    workspace: &[WorkspaceTable],
    current_uri: &tower_lsp_server::ls_types::Uri,
) -> Vec<DomainDiagnostic> {
    let (mut diagnostics, parsed) = collect_parse_diagnostics(text, format, tree);

    if let Some(table) = &parsed {
        // Filename/table-name consistency check — warning-level so the user
        // can still ship, but visible enough not to be missed.
        if let Some(entry) = workspace.iter().find(|t| t.uri == *current_uri) {
            validation::check_filename_table_name_mismatch(
                text,
                current_uri,
                tree,
                entry.table.name.as_str(),
                &mut diagnostics,
            );
        }
        validation::validate_workspace(workspace, current_uri, &mut diagnostics);
        // Workspace-scoped diagnostics also include the per-file FK warnings,
        // so a freshly opened file picks them up before any did_change.
        validation::validate_fk_supporting_indexes(table, tree, text, &mut diagnostics);
        validation::validate_sequence_exhaustion(table, tree, text, &mut diagnostics);
        validation::validate_check_type_mismatches(table, tree, text, &mut diagnostics);
        validation::validate_check_between_order(table, tree, text, &mut diagnostics);
        validation::validate_check_self_contradiction(table, tree, text, &mut diagnostics);
    }

    diagnostics
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::{DocumentFormat, ParserPool};
    use rstest::rstest;

    #[test]
    fn valid_table_no_diagnostics() {
        let pool = ParserPool::new();
        let src = r#"{
            "name": "user",
            "columns": [
                { "name": "id", "type": "integer", "nullable": false, "primary_key": true }
            ]
        }"#;
        let tree = pool.parse(src, DocumentFormat::Json);
        let diags = compute(src, DocumentFormat::Json, tree.as_ref());
        assert!(diags.is_empty(), "expected zero diagnostics, got {diags:?}");
    }

    #[test]
    fn diagnostics_returns_same_results_on_repeated_call() {
        let pool = ParserPool::new();
        let source = r#"{"name":"user","columns":[{"name":"id","type":"integer","nullable":false,"primary_key":true}]}"#;
        let tree = pool.parse(source, DocumentFormat::Json).unwrap();

        let first = compute(source, DocumentFormat::Json, Some(&tree));
        let second = compute(source, DocumentFormat::Json, Some(&tree));
        let third = compute(source, DocumentFormat::Json, Some(&tree));

        // Cache must not change observable behaviour.
        assert_eq!(first, second);
        assert_eq!(second, third);
    }

    #[test]
    fn diagnostics_cache_hit_returns_same_results() {
        diagnostics_cache().clear();
        let pool = ParserPool::new();
        let source = r#"{"name":"u","columns":[{"name":"id","type":"integer"}]}"#;
        let tree = pool.parse(source, DocumentFormat::Json).unwrap();

        let first = compute(source, DocumentFormat::Json, Some(&tree));
        let second = compute(source, DocumentFormat::Json, Some(&tree));
        let third = compute(source, DocumentFormat::Json, Some(&tree));

        assert_eq!(first, second);
        assert_eq!(second, third);
    }

    #[test]
    fn diagnostics_cache_format_disambiguates() {
        diagnostics_cache().clear();
        let pool = ParserPool::new();
        let source = "name: u\ncolumns: []\n";
        let json_tree = pool.parse(source, DocumentFormat::Json).unwrap();
        let yaml_tree = pool.parse(source, DocumentFormat::Yaml).unwrap();

        let json_diags = compute(source, DocumentFormat::Json, Some(&json_tree));
        let yaml_diags = compute(source, DocumentFormat::Yaml, Some(&yaml_tree));

        assert_ne!(
            json_diags, yaml_diags,
            "JSON syntax diagnostics must not pollute YAML results"
        );
        let json_diags_2 = compute(source, DocumentFormat::Json, Some(&json_tree));
        let yaml_diags_2 = compute(source, DocumentFormat::Yaml, Some(&yaml_tree));
        assert_eq!(json_diags, json_diags_2);
        assert_eq!(yaml_diags, yaml_diags_2);
    }

    #[test]
    fn diagnostics_cache_does_not_pollute_across_distinct_texts() {
        diagnostics_cache().clear();
        let pool = ParserPool::new();
        let valid = r#"{"name":"a","columns":[{"name":"id","type":"integer"}]}"#;
        let invalid = r#"{"name":"b","columns":[{"name":"id","type":"wrong"}]}"#;
        let valid_tree = pool.parse(valid, DocumentFormat::Json).unwrap();
        let invalid_tree = pool.parse(invalid, DocumentFormat::Json).unwrap();

        let valid_diags = compute(valid, DocumentFormat::Json, Some(&valid_tree));
        let invalid_diags = compute(invalid, DocumentFormat::Json, Some(&invalid_tree));

        assert!(valid_diags.iter().all(|diag| diag.code != "unknown-type"));
        assert!(invalid_diags.iter().any(|diag| diag.code == "unknown-type"));
        let valid_diags_2 = compute(valid, DocumentFormat::Json, Some(&valid_tree));
        let invalid_diags_2 = compute(invalid, DocumentFormat::Json, Some(&invalid_tree));
        assert_eq!(valid_diags, valid_diags_2);
        assert_eq!(invalid_diags, invalid_diags_2);
    }

    #[test]
    fn compute_uncached_bypasses_cache() {
        diagnostics_cache().clear();
        let pool = ParserPool::new();
        let source = r#"{"name":"u","columns":[{"name":"id","type":"unknown_type"}]}"#;
        let tree = pool.parse(source, DocumentFormat::Json).unwrap();

        let cached = compute(source, DocumentFormat::Json, Some(&tree));
        let uncached = compute_uncached(source, DocumentFormat::Json, Some(&tree));

        assert_eq!(cached, uncached);
    }

    #[test]
    fn truncated_json_produces_syntax_error() {
        let pool = ParserPool::new();
        let src = r#"{"name": "user","#;
        let tree = pool.parse(src, DocumentFormat::Json);
        let diags = compute(src, DocumentFormat::Json, tree.as_ref());
        assert!(!diags.is_empty());
        assert!(
            diags
                .iter()
                .any(|d| d.code == "syntax-error" || d.code == "parse-error")
        );
    }

    #[test]
    fn unknown_column_type_highlights_type_pair_not_braces() {
        let pool = ParserPool::new();
        let src = r#"{"name":"u","columns":[{"name":"id","type":"wrong","nullable":false,"primary_key":true}]}"#;
        let tree = pool.parse(src, DocumentFormat::Json);
        let diags = compute(src, DocumentFormat::Json, tree.as_ref());

        let err = diags
            .iter()
            .find(|d| d.code == "unknown-type")
            .expect("expected an unknown-type diagnostic");
        let snippet = &src[err.byte_range.clone()];

        assert!(
            snippet.starts_with(r#""type""#),
            "diagnostic should start at the `type` key, got: {snippet}"
        );
        assert!(
            snippet.contains("wrong"),
            "diagnostic should cover the bad value `wrong`, got: {snippet}"
        );
        assert!(
            !snippet.ends_with('}'),
            "diagnostic must NOT bleed onto the column's closing brace"
        );
        // And the redundant serde parse-error must be suppressed.
        assert!(
            !diags.iter().any(|d| d.code == "parse-error"),
            "parse-error should be suppressed when unknown-type fired"
        );
    }

    #[test]
    fn known_simple_types_produce_no_unknown_type_diagnostic() {
        let pool = ParserPool::new();
        let src = r#"{"name":"u","columns":[{"name":"id","type":"integer","nullable":false,"primary_key":true}]}"#;
        let tree = pool.parse(src, DocumentFormat::Json);
        let diags = compute(src, DocumentFormat::Json, tree.as_ref());
        assert!(diags.iter().all(|d| d.code != "unknown-type"));
    }

    #[rstest]
    #[case::json(
        r#"{"name":"u","columns":[{"name":"payload","type":"jsonb","nullable":false}]}"#,
        DocumentFormat::Json
    )]
    #[case::yaml(
        "name: u\ncolumns:\n  - name: payload\n    type: jsonb\n    nullable: false\n",
        DocumentFormat::Yaml
    )]
    fn jsonb_is_not_a_simple_type_and_gets_flagged(
        #[case] src: &str,
        #[case] format: DocumentFormat,
    ) {
        // `SimpleColumnType` has no `Jsonb` variant: the loader rejects
        // `"type": "jsonb"`, so the editor must flag it too.
        let pool = ParserPool::new();
        let tree = pool.parse(src, format);
        let diags = compute(src, format, tree.as_ref());

        let err = diags
            .iter()
            .find(|d| d.code == "unknown-type")
            .expect("`jsonb` must produce an unknown-type diagnostic");
        let suggested = err
            .message
            .split_once("Expected one of:")
            .map(|(_, tail)| tail)
            .expect("diagnostic should list the expected types");
        assert!(
            !suggested.contains("jsonb"),
            "the suggested type list must not advertise `jsonb`, got: {suggested}"
        );
    }

    #[test]
    fn yaml_unknown_column_type_highlights_type_pair() {
        let pool = ParserPool::new();
        let src = "name: u\ncolumns:\n  - name: id\n    type: wrong\n    nullable: false\n    primary_key: true\n";
        let tree = pool.parse(src, DocumentFormat::Yaml);
        let diags = compute(src, DocumentFormat::Yaml, tree.as_ref());

        let err = diags
            .iter()
            .find(|d| d.code == "unknown-type")
            .expect("YAML unknown-type diagnostic missing");
        let snippet = &src[err.byte_range.clone()];
        assert!(
            snippet.contains("type:"),
            "snippet should cover the YAML `type:` pair, got: {snippet:?}"
        );
        assert!(
            snippet.contains("wrong"),
            "snippet should include the bad value, got: {snippet:?}"
        );
    }

    #[test]
    fn yaml_valid_simple_type_produces_no_unknown_type_diagnostic() {
        let pool = ParserPool::new();
        let src = "name: u\ncolumns:\n  - name: id\n    type: uuid\n    nullable: false\n    primary_key: true\n";
        let tree = pool.parse(src, DocumentFormat::Yaml);
        let diags = compute(src, DocumentFormat::Yaml, tree.as_ref());
        assert!(diags.iter().all(|d| d.code != "unknown-type"));
    }

    #[test]
    fn yaml_complex_type_object_skips_unknown_type_check() {
        // varchar lives in an object, not a string. The pre-pass must skip it.
        let pool = ParserPool::new();
        let src = "name: u\ncolumns:\n  - name: title\n    type: {kind: varchar, length: 200}\n    nullable: false\n";
        let tree = pool.parse(src, DocumentFormat::Yaml);
        let diags = compute(src, DocumentFormat::Yaml, tree.as_ref());
        assert!(
            diags.iter().all(|d| d.code != "unknown-type"),
            "object type values must not trigger unknown-type, got: {diags:?}"
        );
    }

    #[test]
    fn enum_without_values_field_emits_complex_type_error() {
        let pool = ParserPool::new();
        let src = r#"{"name":"u","columns":[{"name":"status","type":{"kind":"enum","name":"s"},"nullable":false}]}"#;
        let tree = pool.parse(src, DocumentFormat::Json);
        let diags = compute(src, DocumentFormat::Json, tree.as_ref());

        let err = diags
            .iter()
            .find(|d| d.code == "complex-type")
            .expect("expected complex-type diagnostic");
        assert!(
            err.message.contains("values"),
            "message should mention missing `values`, got: {}",
            err.message
        );
        // No redundant serde parse-error.
        assert!(diags.iter().all(|d| d.code != "parse-error"));
    }

    #[test]
    fn enum_with_empty_values_array_is_flagged() {
        let pool = ParserPool::new();
        let src = r#"{"name":"u","columns":[{"name":"s","type":{"kind":"enum","name":"st","values":[]}}]}"#;
        let tree = pool.parse(src, DocumentFormat::Json);
        let diags = compute(src, DocumentFormat::Json, tree.as_ref());

        let err = diags
            .iter()
            .find(|d| d.code == "complex-type")
            .expect("empty `values` should be flagged");
        assert!(err.message.contains("non-empty"), "got: {}", err.message);
    }

    #[test]
    fn enum_string_duplicate_value_is_flagged_on_the_offending_element() {
        let pool = ParserPool::new();
        let src = r#"{"name":"u","columns":[{"name":"s","type":{"kind":"enum","name":"st","values":["active","banned","active"]}}]}"#;
        let tree = pool.parse(src, DocumentFormat::Json);
        let diags = compute(src, DocumentFormat::Json, tree.as_ref());

        let err = diags
            .iter()
            .find(|d| d.code == "complex-type" && d.message.contains("Duplicate enum value"))
            .expect("duplicate enum string value should be flagged");
        // Range must point at the SECOND `"active"`, not the whole column.
        let snippet = &src[err.byte_range.clone()];
        assert_eq!(
            snippet, r#""active""#,
            "diagnostic should land on the duplicate element, got: {snippet}"
        );
        // The second occurrence is later in the file.
        let first = src.find(r#""active""#).unwrap();
        assert!(err.byte_range.start > first);
    }

    #[test]
    fn varchar_without_length_field_is_flagged() {
        let pool = ParserPool::new();
        let src = r#"{"name":"u","columns":[{"name":"title","type":{"kind":"varchar"}}]}"#;
        let tree = pool.parse(src, DocumentFormat::Json);
        let diags = compute(src, DocumentFormat::Json, tree.as_ref());

        let err = diags
            .iter()
            .find(|d| d.code == "complex-type")
            .expect("varchar without length should be flagged");
        assert!(err.message.contains("length"), "got: {}", err.message);
    }

    #[test]
    fn numeric_missing_precision_and_scale_is_flagged() {
        let pool = ParserPool::new();
        let src = r#"{"name":"u","columns":[{"name":"amount","type":{"kind":"numeric"}}]}"#;
        let tree = pool.parse(src, DocumentFormat::Json);
        let diags = compute(src, DocumentFormat::Json, tree.as_ref());

        let err = diags
            .iter()
            .find(|d| d.code == "complex-type")
            .expect("numeric without precision/scale should be flagged");
        assert!(err.message.contains("precision"));
        assert!(err.message.contains("scale"));
    }

    #[test]
    fn unknown_complex_kind_is_flagged_on_kind_pair() {
        let pool = ParserPool::new();
        let src = r#"{"name":"u","columns":[{"name":"x","type":{"kind":"nope"}}]}"#;
        let tree = pool.parse(src, DocumentFormat::Json);
        let diags = compute(src, DocumentFormat::Json, tree.as_ref());

        let err = diags
            .iter()
            .find(|d| d.code == "complex-type" && d.message.contains("Unknown type kind"))
            .expect("unknown kind should be flagged");
        let snippet = &src[err.byte_range.clone()];
        assert!(
            snippet.starts_with("\"kind\""),
            "diagnostic should land on the `kind` pair, got: {snippet}"
        );
    }

    #[test]
    fn integer_enum_duplicate_numeric_value_is_flagged() {
        let pool = ParserPool::new();
        let src = r#"{"name":"u","columns":[{"name":"p","type":{"kind":"enum","name":"pl","values":[{"name":"low","value":0},{"name":"high","value":0}]}}]}"#;
        let tree = pool.parse(src, DocumentFormat::Json);
        let diags = compute(src, DocumentFormat::Json, tree.as_ref());

        let err = diags
            .iter()
            .find(|d| {
                d.code == "complex-type" && d.message.contains("Duplicate enum numeric value")
            })
            .expect("duplicate integer enum value should be flagged");
        let snippet = &src[err.byte_range.clone()];
        assert_eq!(snippet, "0", "diagnostic should land on the duplicate `0`");
    }

    /// Regression — integer-enum members `{"name": "low", "value": 0}`
    /// inside `type.values` MUST NOT be treated as columns. A recursive
    /// descent over the `columns` array would otherwise see their `name`
    /// fields and either flag false duplicates or land table-level
    /// diagnostics on enum members.
    #[test]
    fn enum_integer_member_name_is_not_treated_as_column() {
        let pool = ParserPool::new();
        // `priority.value` enum has a member literally called `id`, the
        // same as the first column's name. This MUST NOT trigger a
        // duplicate-column diagnostic.
        let src = r#"{
            "name": "u",
            "columns": [
                {"name": "id", "type": "integer", "nullable": false, "primary_key": true},
                {
                    "name": "priority",
                    "type": {
                        "kind": "enum",
                        "name": "pl",
                        "values": [
                            {"name": "id", "value": 0},
                            {"name": "high", "value": 10}
                        ]
                    },
                    "nullable": false
                }
            ]
        }"#;
        let tree = pool.parse(src, DocumentFormat::Json);
        let diags = compute(src, DocumentFormat::Json, tree.as_ref());
        assert!(
            diags.iter().all(|d| d.code != "duplicate-column"),
            "enum member name `id` must not collide with column name `id`, got: {diags:#?}"
        );
    }

    #[test]
    fn duplicate_column_name_pinpoints_the_second_occurrence() {
        let pool = ParserPool::new();
        let src = r#"{"name":"u","columns":[{"name":"id","type":"integer","nullable":false,"primary_key":true},{"name":"id","type":"text","nullable":true}]}"#;
        let tree = pool.parse(src, DocumentFormat::Json);
        let diags = compute(src, DocumentFormat::Json, tree.as_ref());

        let dup = diags
            .iter()
            .find(|d| d.code == "duplicate-column")
            .expect("expected duplicate-column diagnostic");
        let first = src.find(r#""name":"id""#).unwrap();
        // Diagnostic should land on the SECOND `"id"`, not the first.
        assert!(dup.byte_range.start > first + 5);
        let snippet = &src[dup.byte_range.clone()];
        assert_eq!(
            snippet, r#""id""#,
            "diagnostic should highlight the duplicate `id`"
        );

        // And no `validate-schema` "duplicate table name" surfaces here —
        // this is a column-level issue, not a workspace duplication.
        assert!(
            diags
                .iter()
                .all(|d| !d.message.contains("duplicate table name"))
        );
    }

    #[test]
    fn fused_walk_matches_unfused_pipeline() {
        let pool = ParserPool::new();
        let source = r#"{"name":"t","columns":[
            {"name":"id","type":"integer"},
            {"name":"id","type":"wibble"},
            {"name":"x","type":{"length":5}}
        ]"#;
        let tree = pool.parse(source, DocumentFormat::Json).unwrap();

        let mut fused = Vec::new();
        validation::collect_all(&tree, source, &mut fused);

        let mut unfused = Vec::new();
        validation::collect_syntax_errors(&tree, &mut unfused);
        validation::collect_unknown_column_types(&tree, source, &mut unfused);
        validation::collect_complex_type_violations(&tree, source, &mut unfused);
        validation::collect_duplicate_column_names(&tree, source, &mut unfused);

        fused.sort_by_key(|d| (d.byte_range.start, d.code.clone()));
        unfused.sort_by_key(|d| (d.byte_range.start, d.code.clone()));
        assert_eq!(
            fused, unfused,
            "fused walker produces same diagnostics as unfused pipeline"
        );
    }

    /// Regression — when `columns` precedes `name` at the top level, the
    /// locator used to walk into the first column and land "table" errors
    /// on a column's `name`, e.g. "duplicate table name: article" showing
    /// up on `id`. Make sure `locate_top_name` returns the OUTER `name`.
    #[test]
    fn locate_top_name_is_not_confused_when_columns_precede_name() {
        use crate::diagnostics::locator;
        let pool = ParserPool::new();
        let src = r#"{
            "columns": [
                {"name": "id", "type": "integer"}
            ],
            "name": "article"
        }"#;
        let tree = pool.parse(src, DocumentFormat::Json).unwrap();
        let range = locator::locate_top_name(Some(&tree), src).expect("range");
        let snippet = &src[range];
        assert!(
            snippet.contains("article"),
            "expected table-level name `article`, got: {snippet}"
        );
    }

    #[test]
    fn missing_columns_field_produces_validation_error() {
        let pool = ParserPool::new();
        // Valid JSON syntax but missing required `columns`.
        let src = r#"{"name": "user"}"#;
        let tree = pool.parse(src, DocumentFormat::Json);
        let diags = compute(src, DocumentFormat::Json, tree.as_ref());
        // Either serde rejects (parse-error) or validate_schema rejects. Both
        // are acceptable as long as we emit something.
        assert!(!diags.is_empty());
    }

    // -----------------------------------------------------------------------
    // F51: FK supporting index diagnostics
    // -----------------------------------------------------------------------

    #[test]
    fn fk_without_supporting_index_emits_warning() {
        diagnostics_cache().clear();
        let pool = ParserPool::new();
        // orders.user_id has an inline FK but no index → F51 warning.
        let src = r#"{
            "name": "orders_f51_uncovered",
            "columns": [
                {"name": "id", "type": "integer", "nullable": false, "primary_key": true},
                {"name": "user_id", "type": "integer", "nullable": false,
                 "foreign_key": {"ref_table": "users", "ref_columns": ["id"]}}
            ]
        }"#;
        let tree = pool.parse(src, DocumentFormat::Json);
        let diags = compute(src, DocumentFormat::Json, tree.as_ref());

        let fk_diag = diags
            .iter()
            .find(|d| d.code == "fk-supporting-index")
            .expect("expected fk-supporting-index warning");
        assert_eq!(fk_diag.severity, Severity::Warning);
        assert!(
            fk_diag.message.contains("user_id"),
            "message should mention the FK column, got: {}",
            fk_diag.message
        );
        assert!(
            fk_diag.message.contains("ix_orders_f51_uncovered__user_id"),
            "message should include suggested index name, got: {}",
            fk_diag.message
        );
        // Squiggle should land near the inline foreign_key value, not at the
        // start of the file.
        assert_ne!(
            fk_diag.byte_range,
            0..1,
            "diagnostic should be located, not a fallback 0..1"
        );
        let snippet = &src[fk_diag.byte_range.clone()];
        assert!(
            snippet.contains("ref_table") || snippet.contains("foreign_key"),
            "snippet should cover the FK declaration, got: {snippet}"
        );
    }

    #[test]
    fn fk_with_inline_index_emits_no_warning() {
        diagnostics_cache().clear();
        let pool = ParserPool::new();
        // Same orders table, this time WITH `"index": true` on user_id.
        let src = r#"{
            "name": "orders_f51_covered",
            "columns": [
                {"name": "id", "type": "integer", "nullable": false, "primary_key": true},
                {"name": "user_id", "type": "integer", "nullable": false,
                 "index": true,
                 "foreign_key": {"ref_table": "users", "ref_columns": ["id"]}}
            ]
        }"#;
        let tree = pool.parse(src, DocumentFormat::Json);
        let diags = compute(src, DocumentFormat::Json, tree.as_ref());

        assert!(
            diags.iter().all(|d| d.code != "fk-supporting-index"),
            "no FK warning expected when inline index exists, got: {diags:?}"
        );
    }

    #[test]
    fn self_referential_fk_without_index_emits_warning() {
        diagnostics_cache().clear();
        let pool = ParserPool::new();
        // Tree structure — parent_id → self.id with no index.
        let src = r#"{
            "name": "categories_f51_selfref",
            "columns": [
                {"name": "id", "type": "integer", "nullable": false, "primary_key": true},
                {"name": "parent_id", "type": "integer", "nullable": true,
                 "foreign_key": {"ref_table": "categories_f51_selfref", "ref_columns": ["id"]}}
            ]
        }"#;
        let tree = pool.parse(src, DocumentFormat::Json);
        let diags = compute(src, DocumentFormat::Json, tree.as_ref());

        let fk_diag = diags
            .iter()
            .find(|d| d.code == "fk-supporting-index")
            .expect("self-referential FK without index should warn");
        assert!(fk_diag.message.contains("parent_id"));
        assert!(
            fk_diag
                .message
                .contains("ix_categories_f51_selfref__parent_id")
        );
    }

    // -----------------------------------------------------------------------
    // F76: sequence/identity exhaustion warning (LSP file-local scope)
    // -----------------------------------------------------------------------

    #[test]
    fn integer_auto_increment_pk_emits_sequence_exhaustion_warning() {
        diagnostics_cache().clear();
        let pool = ParserPool::new();
        let src = r#"{
            "name": "events_f76_int",
            "columns": [
                {"name": "id", "type": "integer", "nullable": false,
                 "primary_key": {"auto_increment": true}},
                {"name": "payload", "type": "text", "nullable": false}
            ]
        }"#;
        let tree = pool.parse(src, DocumentFormat::Json);
        let diags = compute(src, DocumentFormat::Json, tree.as_ref());

        let warn = diags
            .iter()
            .find(|d| d.code == "sequence-exhaustion")
            .expect("expected sequence-exhaustion warning for integer PK");
        assert_eq!(warn.severity, Severity::Warning);
        assert!(
            warn.message.contains("`id`"),
            "message should mention column `id`, got: {}",
            warn.message
        );
        assert!(
            warn.message.contains("integer") && warn.message.contains("big_int"),
            "message should mention current `integer` and recommended `big_int`, got: {}",
            warn.message
        );
        assert!(
            warn.message.contains("Medium"),
            "integer PK should be Medium risk, got: {}",
            warn.message
        );
        assert_ne!(
            warn.byte_range,
            0..1,
            "diagnostic should be located, not fallback 0..1"
        );
    }

    #[test]
    fn small_int_auto_increment_pk_emits_high_severity_message() {
        diagnostics_cache().clear();
        let pool = ParserPool::new();
        let src = r#"{
            "name": "tiny_seq_f76",
            "columns": [
                {"name": "id", "type": "small_int", "nullable": false,
                 "primary_key": {"auto_increment": true}}
            ]
        }"#;
        let tree = pool.parse(src, DocumentFormat::Json);
        let diags = compute(src, DocumentFormat::Json, tree.as_ref());

        let warn = diags
            .iter()
            .find(|d| d.code == "sequence-exhaustion")
            .expect("expected sequence-exhaustion warning for small_int PK");
        assert!(
            warn.message.contains("High"),
            "small_int PK should be High risk, got: {}",
            warn.message
        );
        assert!(
            warn.message.contains("small_int"),
            "message should mention current `small_int`, got: {}",
            warn.message
        );
    }

    #[test]
    fn big_int_auto_increment_pk_emits_no_warning() {
        diagnostics_cache().clear();
        let pool = ParserPool::new();
        let src = r#"{
            "name": "safe_seq_f76",
            "columns": [
                {"name": "id", "type": "big_int", "nullable": false,
                 "primary_key": {"auto_increment": true}}
            ]
        }"#;
        let tree = pool.parse(src, DocumentFormat::Json);
        let diags = compute(src, DocumentFormat::Json, tree.as_ref());

        assert!(
            diags.iter().all(|d| d.code != "sequence-exhaustion"),
            "big_int PK should be safe, got: {diags:?}"
        );
    }

    #[test]
    fn check_valid_expression_emits_no_check_diagnostics() {
        diagnostics_cache().clear();
        let pool = ParserPool::new();
        let src = r#"{"name":"users","columns":[{"name":"id","type":"integer","nullable":false,"primary_key":true},{"name":"age","type":"integer","nullable":false}],"constraints":[{"type":"check","name":"check_age_reasonable","expr":"age > 0 AND age < 150"}]}"#;
        let tree = pool.parse(src, DocumentFormat::Json);
        let diags = compute(src, DocumentFormat::Json, tree.as_ref());

        let check_diag_count = diags
            .iter()
            .filter(|d| {
                d.code.starts_with("check-")
                    || (d.code == "validate-schema" && d.message.contains("CHECK"))
            })
            .count();
        assert_eq!(
            check_diag_count, 0,
            "valid CHECK should emit no CHECK diagnostics, got: {diags:?}"
        );
    }

    #[test]
    fn check_between_reversed_emits_error() {
        diagnostics_cache().clear();
        let pool = ParserPool::new();
        let src = r#"{"name":"users","columns":[{"name":"id","type":"integer","nullable":false,"primary_key":true},{"name":"age","type":"integer","nullable":false}],"constraints":[{"type":"check","name":"check_age_between","expr":"age BETWEEN 100 AND 0"}]}"#;
        let tree = pool.parse(src, DocumentFormat::Json);
        let diags = compute(src, DocumentFormat::Json, tree.as_ref());

        let check_diags = diags
            .iter()
            .filter(|d| d.message.contains("BETWEEN"))
            .collect::<Vec<_>>();
        assert_eq!(
            check_diags.len(),
            1,
            "reversed BETWEEN should emit exactly one diagnostic, got: {diags:?}"
        );
        let diag = check_diags[0];
        assert_eq!(diag.code, "check-between-reversed");
        assert_eq!(diag.severity, Severity::Error);
        assert_ne!(
            diag.byte_range,
            0..1,
            "diagnostic should be located, not fallback 0..1"
        );
        assert!(
            diag.byte_range.start < diag.byte_range.end,
            "diagnostic range should be non-empty, got: {:?}",
            diag.byte_range
        );
    }

    #[test]
    fn check_self_contradiction_emits_error() {
        diagnostics_cache().clear();
        let pool = ParserPool::new();
        let src = r#"{"name":"users","columns":[{"name":"id","type":"integer","nullable":false,"primary_key":true},{"name":"age","type":"integer","nullable":false}],"constraints":[{"type":"check","name":"check_age_impossible","expr":"age > 100 AND age < 0"}]}"#;
        let tree = pool.parse(src, DocumentFormat::Json);
        let diags = compute(src, DocumentFormat::Json, tree.as_ref());

        let check_diags = diags
            .iter()
            .filter(|d| d.message.contains("contradiction"))
            .collect::<Vec<_>>();
        assert_eq!(
            check_diags.len(),
            1,
            "self-contradictory CHECK should emit exactly one diagnostic, got: {diags:?}"
        );
        let diag = check_diags[0];
        assert_eq!(diag.code, "check-self-contradiction");
        assert_eq!(diag.severity, Severity::Error);
        assert!(
            diag.message.contains("CHECK self-contradiction")
                || diag.message.contains("self-contradiction")
                || diag.message.contains("contradiction"),
            "message should mention self-contradiction, got: {}",
            diag.message
        );
    }

    #[test]
    fn check_type_mismatch_emits_warning() {
        diagnostics_cache().clear();
        let pool = ParserPool::new();
        let src = r#"{"name":"users","columns":[{"name":"id","type":"integer","nullable":false,"primary_key":true},{"name":"age","type":"integer","nullable":false}],"constraints":[{"type":"check","name":"check_age_type","expr":"age = 'abc'"}]}"#;
        let tree = pool.parse(src, DocumentFormat::Json);
        let diags = compute(src, DocumentFormat::Json, tree.as_ref());

        let mismatch_diags = diags
            .iter()
            .filter(|d| d.code == "check-type-mismatch")
            .collect::<Vec<_>>();
        assert_eq!(
            mismatch_diags.len(),
            1,
            "type-mismatched CHECK should emit exactly one diagnostic, got: {diags:?}"
        );
        assert_eq!(mismatch_diags[0].severity, Severity::Warning);
    }

    #[test]
    fn f86_default_violates_check_still_works() {
        diagnostics_cache().clear();
        let pool = ParserPool::new();
        let src = r#"{"name":"users","columns":[{"name":"id","type":"integer","nullable":false,"primary_key":true},{"name":"age","type":"integer","nullable":false,"default":5}],"constraints":[{"type":"check","name":"check_age_min","expr":"age > 10"}]}"#;
        let tree = pool.parse(src, DocumentFormat::Json);
        let diags = compute(src, DocumentFormat::Json, tree.as_ref());

        let default_diags = diags
            .iter()
            .filter(|d| d.message.contains("default") && d.message.contains("CHECK"))
            .collect::<Vec<_>>();
        assert_eq!(
            default_diags.len(),
            1,
            "default violating CHECK should emit exactly one diagnostic, got: {diags:?}"
        );
        let diag = default_diags[0];
        assert_eq!(diag.severity, Severity::Error);
        let snippet = &src[diag.byte_range.clone()];
        assert!(
            snippet.contains("default"),
            "diagnostic should be anchored at the default field, got: {snippet}"
        );
    }

    #[test]
    fn yaml_check_type_mismatch_emits_warning() {
        diagnostics_cache().clear();
        let pool = ParserPool::new();
        let src = "name: users\ncolumns:\n  - name: id\n    type: integer\n    nullable: false\n    primary_key: true\n  - name: age\n    type: integer\n    nullable: false\nconstraints:\n  - type: check\n    name: check_age_type\n    expr: age = 'abc'\n";
        let tree = pool.parse(src, DocumentFormat::Yaml);
        let diags = compute(src, DocumentFormat::Yaml, tree.as_ref());

        let mismatch_diags = diags
            .iter()
            .filter(|d| d.code == "check-type-mismatch")
            .collect::<Vec<_>>();
        assert_eq!(
            mismatch_diags.len(),
            1,
            "YAML type-mismatched CHECK should emit exactly one diagnostic, got: {diags:?}"
        );
        assert_eq!(mismatch_diags[0].severity, Severity::Warning);
    }

    #[test]
    fn diagnostics_shared_returns_arc_consistently() {
        diagnostics_cache().clear();
        let pool = ParserPool::new();
        let src = r#"{"name":"u","columns":[{"name":"id","type":"integer","nullable":false,"primary_key":true}]}"#;
        let tree = pool.parse(src, DocumentFormat::Json).unwrap();

        let first = compute_shared(src, DocumentFormat::Json, Some(&tree));
        let second = compute_shared(src, DocumentFormat::Json, Some(&tree));

        assert_eq!(&*first, &*second);
    }

    #[rstest]
    #[case::index_column_missing(r#"{"name":"u","columns":[{"name":"id","type":"integer","nullable":false,"primary_key":true}],"constraints":[{"type":"index","name":"ix_missing","columns":["nonexistent_col"]}]}"#)]
    #[case::empty_index_columns(r#"{"name":"u","columns":[{"name":"id","type":"integer","nullable":false,"primary_key":true}],"constraints":[{"type":"index","name":"ix_empty","columns":[]}]}"#)]
    #[case::invalid_enum_default(r#"{"name":"u","columns":[{"name":"status","type":{"kind":"enum","name":"st","values":["active","banned"]},"nullable":false,"default":"'unknown'"}]}"#)]
    fn diagnostics_invalid_schema_cases_emit_diagnostics(#[case] src: &str) {
        diagnostics_cache().clear();
        let pool = ParserPool::new();
        let tree = pool.parse(src, DocumentFormat::Json);

        let diags = compute(src, DocumentFormat::Json, tree.as_ref());

        assert!(!diags.is_empty(), "invalid schema should emit diagnostics");
    }

    #[test]
    fn missing_primary_key_warning_path_does_not_panic() {
        diagnostics_cache().clear();
        let pool = ParserPool::new();
        let src = r#"{"name":"u","columns":[{"name":"data","type":"text","nullable":false}]}"#;
        let tree = pool.parse(src, DocumentFormat::Json);

        let _ = compute(src, DocumentFormat::Json, tree.as_ref());
    }
}
