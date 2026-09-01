use sea_query::{Alias, Index};

use vespertide_core::{KeepPolicy, TableDef, UniqueConstraintStrategy};

use super::super::helpers::{build_unique_constraint_name, quote_ident, quote_idents};
use super::super::types::{BuiltQuery, DatabaseBackend, RawSql};
use crate::error::QueryError;

/// Build SQL for adding a UNIQUE constraint to a table, including the F2
/// pre-cleanup `DELETE` dictated by `strategy`.
///
/// `strategy` is always emitted (no `SkipCleanup` option exists): every
/// `AddConstraint(Unique)` produces a `DELETE` ahead of the `ADD CONSTRAINT`
/// so the migration succeeds even when production data has duplicates.
///
/// `current_schema` is needed to look up the table's PRIMARY KEY: the
/// generated `DELETE` uses `(pk) NOT IN (SELECT MIN(pk) ... GROUP BY
/// <unique_columns>)` (or MAX for `KeepLast`). Tables without a PK, with
/// a composite PK, or with PK columns appearing inside the unique-key set
/// are rejected with [`QueryError::UnsupportedAction`] — the user must
/// pre-clean manually in those cases.
pub(super) fn build_unique<T: AsRef<str>>(
    backend: DatabaseBackend,
    table: &str,
    name: Option<&str>,
    columns: &[T],
    strategy: &UniqueConstraintStrategy,
    current_schema: &[TableDef],
) -> Result<Vec<BuiltQuery>, QueryError> {
    let mut queries = build_pre_cleanup_delete(backend, table, columns, strategy, current_schema)?;
    queries.push(build_unique_index(table, name, columns));
    Ok(queries)
}

/// Compose the pre-cleanup `DELETE` for `strategy`.
///
/// Currently only `DeleteDuplicates` exists; the dispatch is laid out so
/// future strategies can plug in without touching the call site.
fn build_pre_cleanup_delete<T: AsRef<str>>(
    backend: DatabaseBackend,
    table: &str,
    columns: &[T],
    strategy: &UniqueConstraintStrategy,
    current_schema: &[TableDef],
) -> Result<Vec<BuiltQuery>, QueryError> {
    // `UniqueConstraintStrategy` is `#[non_exhaustive]`; match (not `let`)
    // so future variants get a compiler warning to be handled.
    let keep = match strategy {
        UniqueConstraintStrategy::DeleteDuplicates { keep } => *keep,
        // `#[non_exhaustive]` future-variant guard; unreachable today.
        #[cfg(not(tarpaulin_include))]
        _ => {
            return Err(QueryError::UnsupportedAction(format!(
                "AddConstraint(Unique) on '{table}': unsupported strategy variant"
            )));
        }
    };
    // Graceful fallback for tables without a usable PK (no PK at all,
    // composite PK, or PK column inside the unique set): skip the
    // pre-cleanup `DELETE` and let the database decide. Production schemas
    // are guaranteed to have a single-column PK by F12 Scenario E, so this
    // path is reached only by test fixtures or hand-edited migrations.
    let Some(pk_column) = super::try_resolve_single_pk_column(table, current_schema, columns)
    else {
        return Ok(vec![]);
    };
    let agg = match keep {
        KeepPolicy::First => "MIN",
        KeepPolicy::Last => "MAX",
    };
    let quoted_table = quote_ident(table, backend);
    let quoted_pk = quote_ident(&pk_column, backend);
    let group_by = quote_idents(columns, backend);
    let sql = format!(
        "DELETE FROM {quoted_table} WHERE {quoted_pk} NOT IN (\
         SELECT {agg}({quoted_pk}) FROM {quoted_table} GROUP BY {group_by})",
    );
    Ok(vec![BuiltQuery::Raw(RawSql::uniform(sql))])
}

fn build_unique_index<T: AsRef<str>>(table: &str, name: Option<&str>, columns: &[T]) -> BuiltQuery {
    let index_name = build_unique_constraint_name(table, columns, name);
    let mut idx = Index::create()
        .table(Alias::new(table))
        .name(&index_name)
        .unique()
        .to_owned();
    for col in columns {
        idx.col(Alias::new(col.as_ref()));
    }
    BuiltQuery::CreateIndex(Box::new(idx))
}

#[cfg(test)]
mod tests {
    use super::super::try_resolve_single_pk_column;
    use super::*;
    use rstest::rstest;
    use vespertide_core::{ColumnDef, ColumnType, SimpleColumnType, TableConstraint};

    fn schema_with_single_pk() -> Vec<TableDef> {
        let mut id_col = ColumnDef::new("id", ColumnType::Simple(SimpleColumnType::Integer), false);
        id_col.primary_key =
            Some(vespertide_core::schema::primary_key::PrimaryKeySyntax::Bool(true));
        vec![TableDef {
            name: "users".into(),
            description: None,
            columns: vec![
                id_col,
                ColumnDef::new("email", ColumnType::Simple(SimpleColumnType::Text), false),
            ],
            constraints: vec![],
        }]
    }

    #[rstest]
    #[case::keep_first(KeepPolicy::First, "MIN")]
    #[case::keep_last(KeepPolicy::Last, "MAX")]
    fn build_pre_cleanup_delete_emits_pk_keyed_delete(
        #[case] keep: KeepPolicy,
        #[case] expected_agg: &str,
    ) {
        // Single-column PK exists (`id`), unique cols ≠ PK column → real
        // DELETE body executes and emits the aggregate-keyed dedup.
        let schema = schema_with_single_pk();
        let queries = build_pre_cleanup_delete(
            DatabaseBackend::Postgres,
            "users",
            &["email"],
            &UniqueConstraintStrategy::DeleteDuplicates { keep },
            &schema,
        )
        .expect("body should succeed");
        assert_eq!(queries.len(), 1);
        let sql = queries[0].build(DatabaseBackend::Postgres);
        assert!(sql.contains("DELETE FROM"));
        assert!(sql.contains(expected_agg));
        assert!(sql.contains("\"id\""));
        assert!(sql.contains("\"email\""));
    }

    #[test]
    fn build_pre_cleanup_delete_no_pk_returns_empty() {
        // Unknown table → try_resolve returns None → fallback Ok(vec![]).
        let queries = build_pre_cleanup_delete(
            DatabaseBackend::Postgres,
            "nonexistent",
            &["email"],
            &UniqueConstraintStrategy::DeleteDuplicates {
                keep: KeepPolicy::First,
            },
            &[],
        )
        .expect("None pk → Ok(empty)");
        assert!(queries.is_empty());
    }

    #[test]
    fn try_resolve_single_pk_column_returns_none_for_composite_pk() {
        // Composite PK (len != 1) → covers the `pk_columns.len() != 1`
        // guard returning None.
        let schema = vec![TableDef {
            name: "events".into(),
            description: None,
            columns: vec![
                ColumnDef::new(
                    "tenant_id",
                    ColumnType::Simple(SimpleColumnType::Integer),
                    false,
                ),
                ColumnDef::new("ts", ColumnType::Simple(SimpleColumnType::BigInt), false),
            ],
            constraints: vec![TableConstraint::PrimaryKey {
                auto_increment: false,
                columns: vec!["tenant_id".into(), "ts".into()],
                strategy: vespertide_core::PrimaryKeyAdditionStrategy::default(),
            }],
        }];
        let resolved = try_resolve_single_pk_column("events", &schema, &["new_col"]);
        assert!(resolved.is_none());
    }

    #[test]
    fn try_resolve_single_pk_column_returns_none_when_pk_in_unique_set() {
        // PK column appears inside the unique column set → tautology guard
        // returns None (NOT IN (SELECT MIN(pk) GROUP BY pk) would match all).
        let schema = schema_with_single_pk();
        let resolved = try_resolve_single_pk_column("users", &schema, &["id", "email"]);
        assert!(resolved.is_none());
    }
}
