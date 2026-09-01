//! `fill_with` UPDATE emission for `ModifyColumnType`.
//!
//! `fill_with` maps a removed enum label to the surviving label that replaces
//! it. Both sides of the mapping are **bare** labels ??no SQL quoting ??because
//! [`Expr::val`] binds them as data values and the query builder adds exactly
//! one layer of quoting itself.

use std::collections::BTreeMap;
use std::sync::Once;

use sea_query::{Alias, Expr, ExprTrait, Query};

use crate::sql::types::BuiltQuery;

/// Emitted at most once per process by [`strip_legacy_outer_quotes`].
static LEGACY_QUOTE_WARNING: Once = Once::new();

#[expect(
    clippy::print_stderr,
    reason = "one-time deprecation notice for legacy pre-quoted fill_with values; stderr keeps the emitted SQL on stdout intact"
)]
fn warn_legacy_quoted_replacement(column: &str, removed: &str, replacement: &str, bare: &str) {
    LEGACY_QUOTE_WARNING.call_once(|| {
        eprintln!(
            "vespertide: warning: modify_column_type.fill_with replacement \
             {replacement} for {column}.{removed} is wrapped in SQL single \
             quotes. fill_with values are bare enum labels; the quotes were \
             stripped for compatibility. Rewrite the migration to use {bare}."
        );
    });
}

/// Backward compatibility for migration files written against the older schema
/// documentation, which showed the replacement already wrapped in SQL single
/// quotes (`{"cancelled": "'pending'"}`).
///
/// Since [`Expr::val`] binds the replacement as a *data value*, a pre-quoted
/// label is escaped into `'''pending'''`, whose content is the 9-character
/// token `'pending'` rather than the 7-character label `pending`. `PostgreSQL`
/// then rejects the UPDATE with `invalid input value for enum`.
///
/// When the value both starts and ends with a single quote, exactly one outer
/// layer is stripped and a one-time warning is emitted. Everything else is
/// passed through untouched.
fn strip_legacy_outer_quotes<'a>(column: &str, removed: &str, replacement: &'a str) -> &'a str {
    let Some(bare) = replacement
        .strip_prefix('\'')
        .and_then(|inner| inner.strip_suffix('\''))
    else {
        return replacement;
    };

    warn_legacy_quoted_replacement(column, removed, replacement, bare);
    bare
}

/// Build UPDATE statements for `fill_with` mappings (removed enum values ??replacement values).
/// Each entry generates: UPDATE "table" SET "column" = 'replacement' WHERE "column" = '`removed_value`'
///
/// Iteration follows the `BTreeMap` key order, so the emitted statements are
/// deterministic across runs and platforms.
fn build_fill_with_updates(
    table: &str,
    column: &str,
    fill_with: &BTreeMap<String, String>,
) -> Vec<BuiltQuery> {
    fill_with
        .iter()
        .map(|(removed_value, replacement)| {
            let replacement = strip_legacy_outer_quotes(column, removed_value, replacement);
            let update_stmt = Query::update()
                .table(Alias::new(table))
                .value(Alias::new(column), Expr::val(replacement))
                .and_where(Expr::col(Alias::new(column)).eq(removed_value.as_str()))
                .to_owned();
            BuiltQuery::Update(Box::new(update_stmt))
        })
        .collect()
}

/// Conditionally prepend `fill_with` UPDATEs to `queries`.
///
/// Centralises the byte-identical
/// `if let Some(fw) = fill_with { queries.extend(build_fill_with_updates(...)); }`
/// dance that the three `modify_column_type` paths each previously
/// open-coded (`direct::build_postgres_enum_migration`,
/// `direct::build_standard_type_modification`, and
/// `sqlite_rebuild::build_modify_column_type_sqlite_temp_table`). Each
/// callsite now collapses to a single line whose name reads
/// "if a fill_with map exists, prepend its UPDATEs".
pub(super) fn extend_fill_with_updates(
    queries: &mut Vec<BuiltQuery>,
    table: &str,
    column: &str,
    fill_with: Option<&BTreeMap<String, String>>,
) {
    if let Some(fw) = fill_with {
        queries.extend(build_fill_with_updates(table, column, fw));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sql::DatabaseBackend;
    use crate::test_support::{backend_tag, joined_sql_semicolon};
    use insta::{assert_snapshot, with_settings};
    use rstest::rstest;

    fn updates(fill_with: &BTreeMap<String, String>, backend: DatabaseBackend) -> String {
        let mut queries = Vec::new();
        extend_fill_with_updates(&mut queries, "plan", "sheet_policy", Some(fill_with));
        joined_sql_semicolon(backend, &queries)
    }

    fn map(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect()
    }

    /// A bare enum label gets exactly one layer of SQL quoting from the query
    /// builder ??the enum label reaches the column intact.
    #[rstest]
    #[case::postgres(DatabaseBackend::Postgres)]
    #[case::mysql(DatabaseBackend::MySql)]
    #[case::sqlite(DatabaseBackend::Sqlite)]
    fn bare_replacement_gets_exactly_one_quote_layer(#[case] backend: DatabaseBackend) {
        let sql = updates(&map(&[("OVER_500", "FIXED")]), backend);

        assert!(
            sql.contains("= 'FIXED'"),
            "expected a single quote layer around FIXED, got: {sql}"
        );
        assert!(
            !sql.contains("'''FIXED'''"),
            "replacement must not be double-quoted, got: {sql}"
        );

        with_settings!({ snapshot_path => "../snapshots", snapshot_suffix => format!("fill_with_bare_replacement_{}", backend_tag(backend)) }, {
            assert_snapshot!(sql);
        });
    }

    /// Compatibility: a legacy pre-quoted replacement produces the SAME SQL as
    /// the bare form, so migrations written against the old documentation keep
    /// working.
    #[rstest]
    #[case::postgres(DatabaseBackend::Postgres)]
    #[case::mysql(DatabaseBackend::MySql)]
    #[case::sqlite(DatabaseBackend::Sqlite)]
    fn quoted_replacement_matches_bare_replacement(#[case] backend: DatabaseBackend) {
        let bare = updates(&map(&[("OVER_500", "FIXED")]), backend);
        let quoted = updates(&map(&[("OVER_500", "'FIXED'")]), backend);

        assert_eq!(quoted, bare);
    }

    /// Only the outer layer is stripped: a doubly-wrapped value keeps its inner
    /// quotes, and a value with a stray quote on one side is left alone.
    #[rstest]
    #[case::double_wrapped("''FIXED''", "'FIXED'")]
    #[case::leading_quote_only("'FIXED", "'FIXED")]
    #[case::trailing_quote_only("FIXED'", "FIXED'")]
    #[case::lone_quote("'", "'")]
    #[case::empty_quotes("''", "")]
    #[case::bare("FIXED", "FIXED")]
    fn strip_legacy_outer_quotes_removes_at_most_one_layer(
        #[case] input: &str,
        #[case] expected: &str,
    ) {
        assert_eq!(
            strip_legacy_outer_quotes("sheet_policy", "OVER_500", input),
            expected
        );
    }

    /// `fill_with` is a `BTreeMap`, so multiple mappings emit in sorted key
    /// order regardless of insertion order.
    #[rstest]
    #[case::postgres(DatabaseBackend::Postgres)]
    #[case::mysql(DatabaseBackend::MySql)]
    #[case::sqlite(DatabaseBackend::Sqlite)]
    fn multiple_mappings_are_deterministically_ordered(#[case] backend: DatabaseBackend) {
        let ascending = map(&[
            ("OVER_500", "FIXED"),
            ("PER_SHEET", "NEGOTIATION"),
            ("UNDER_100", "FIXED"),
        ]);
        let descending = map(&[
            ("UNDER_100", "FIXED"),
            ("PER_SHEET", "NEGOTIATION"),
            ("OVER_500", "FIXED"),
        ]);
        let sql = updates(&ascending, backend);

        assert_eq!(updates(&descending, backend), sql);

        with_settings!({ snapshot_path => "../snapshots", snapshot_suffix => format!("fill_with_multiple_mappings_{}", backend_tag(backend)) }, {
            assert_snapshot!(sql);
        });
    }

    /// `None` contributes no statements.
    #[test]
    fn absent_fill_with_emits_nothing() {
        let mut queries = Vec::new();
        extend_fill_with_updates(&mut queries, "plan", "sheet_policy", None);
        assert!(queries.is_empty());
    }
}
