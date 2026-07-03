use vespertide_core::{KeepPolicy, PrimaryKeyAdditionStrategy, TableConstraint};

use super::super::helpers::{quote_ident, quote_idents};
use super::super::types::{BuiltQuery, DatabaseBackend, RawSql};
use super::{QueryError, TableDef, rebuild_sqlite_table_with_added_constraint};

/// Build SQL for adding a PRIMARY KEY, plus the F5 pre-cleanup
/// statement dictated by `strategy`.
///
/// Cleanup SQL is uniform across PG/MySQL/SQLite:
///
/// - **`DeleteDuplicates { keep: First }`**: `DELETE FROM t WHERE
///   <old_pk> NOT IN (SELECT MIN(<old_pk>) FROM t GROUP BY <new_pk>)`
/// - **`DeleteDuplicates { keep: Last }`**: `MAX(<old_pk>)` variant.
///
/// When the table lacks a single-column baseline PK (no PK, composite
/// PK, or the only PK column appears inside the new PK set), cleanup
/// is skipped silently. Mirrors F2's `try_resolve_single_pk_column`
/// fallback policy.
pub(super) fn build_primary_key<T: AsRef<str>>(
    backend: DatabaseBackend,
    table: &str,
    columns: &[T],
    strategy: &PrimaryKeyAdditionStrategy,
    constraint: &TableConstraint,
    current_schema: &[TableDef],
    pending_constraints: &[TableConstraint],
) -> Result<Vec<BuiltQuery>, QueryError> {
    let cleanup = build_pk_pre_cleanup(backend, table, columns, strategy, current_schema);

    if backend == DatabaseBackend::Sqlite {
        let mut queries = cleanup;
        queries.extend(rebuild_sqlite_table_with_added_constraint(
            backend,
            table,
            constraint,
            current_schema,
            pending_constraints,
        )?);
        return Ok(queries);
    }

    let pg_cols = quote_idents(columns, DatabaseBackend::Postgres);
    let mysql_cols = quote_idents(columns, DatabaseBackend::MySql);
    let pg_table = quote_ident(table, DatabaseBackend::Postgres);
    let mysql_table = quote_ident(table, DatabaseBackend::MySql);
    let pg_sql = format!("ALTER TABLE {pg_table} ADD PRIMARY KEY ({pg_cols})");
    let mysql_sql = format!("ALTER TABLE {mysql_table} ADD PRIMARY KEY ({mysql_cols})");

    let mut queries = cleanup;
    queries.push(BuiltQuery::Raw(RawSql::per_backend(
        pg_sql.clone(),
        mysql_sql,
        pg_sql,
    )));
    Ok(queries)
}

fn build_pk_pre_cleanup<T: AsRef<str>>(
    backend: DatabaseBackend,
    table: &str,
    new_pk_columns: &[T],
    strategy: &PrimaryKeyAdditionStrategy,
    current_schema: &[TableDef],
) -> Vec<BuiltQuery> {
    let keep = match strategy {
        PrimaryKeyAdditionStrategy::DeleteDuplicates { keep } => *keep,
        #[cfg(not(tarpaulin_include))]
        _ => return vec![], // `#[non_exhaustive]` future-variant guard; unreachable today.
    };
    let Some(old_pk_column) = try_resolve_single_pk_column(table, current_schema, new_pk_columns)
    else {
        return vec![];
    };
    let agg = match keep {
        KeepPolicy::First => "MIN",
        KeepPolicy::Last => "MAX",
    };
    let quoted_table = quote_ident(table, backend);
    let quoted_old_pk = quote_ident(&old_pk_column, backend);
    let group_by = new_pk_columns
        .iter()
        .map(|c| quote_ident(c.as_ref(), backend))
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        "DELETE FROM {quoted_table} WHERE {quoted_old_pk} NOT IN (\
         SELECT {agg}({quoted_old_pk}) FROM {quoted_table} GROUP BY {group_by})"
    );
    vec![BuiltQuery::Raw(RawSql::uniform(sql))]
}

fn try_resolve_single_pk_column<T: AsRef<str>>(
    table: &str,
    current_schema: &[TableDef],
    new_pk_columns: &[T],
) -> Option<String> {
    let table_def = current_schema.iter().find(|t| t.name.as_str() == table)?;

    let pk_columns: Vec<String> = table_def
        .constraints
        .iter()
        .find_map(|c| {
            if let TableConstraint::PrimaryKey { columns, .. } = c {
                Some(columns.iter().map(ToString::to_string).collect())
            } else {
                None
            }
        })
        .or_else(|| {
            let inline: Vec<String> = table_def
                .columns
                .iter()
                .filter(|col| col.primary_key.is_some())
                .map(|col| col.name.to_string())
                .collect();
            if inline.is_empty() {
                None
            } else {
                Some(inline)
            }
        })?;

    if pk_columns.len() != 1 {
        return None;
    }
    let pk_column = pk_columns.into_iter().next().expect("len == 1");
    let new_set: Vec<&str> = new_pk_columns.iter().map(AsRef::as_ref).collect();
    if new_set.iter().any(|c| *c == pk_column) {
        return None;
    }
    Some(pk_column)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;
    use vespertide_core::{ColumnDef, ColumnType, SimpleColumnType};

    fn schema_with_single_pk() -> Vec<TableDef> {
        // single-column PK on `id` so try_resolve_single_pk_column succeeds.
        let mut id_col = ColumnDef::new("id", ColumnType::Simple(SimpleColumnType::Integer), false);
        id_col.primary_key =
            Some(vespertide_core::schema::primary_key::PrimaryKeySyntax::Bool(true));
        let email_col = ColumnDef::new("email", ColumnType::Simple(SimpleColumnType::Text), false);
        vec![TableDef {
            name: "users".into(),
            description: None,
            columns: vec![id_col, email_col],
            constraints: vec![],
        }]
    }

    #[rstest]
    #[case::keep_first(KeepPolicy::First, "MIN")]
    #[case::keep_last(KeepPolicy::Last, "MAX")]
    fn build_pk_pre_cleanup_emits_delete_with_aggregate(
        #[case] keep: KeepPolicy,
        #[case] expected_agg: &str,
    ) {
        // new PK columns differ from baseline (`id`), so try_resolve returns
        // Some("id"), and DeleteDuplicates → real DELETE body executes.
        let schema = schema_with_single_pk();
        let new_pk: Vec<&str> = vec!["email"];
        let queries = build_pk_pre_cleanup(
            DatabaseBackend::Postgres,
            "users",
            &new_pk,
            &PrimaryKeyAdditionStrategy::DeleteDuplicates { keep },
            &schema,
        );
        assert_eq!(queries.len(), 1);
        let sql = queries[0].build(DatabaseBackend::Postgres);
        assert!(sql.contains("DELETE FROM"));
        assert!(sql.contains(expected_agg));
        assert!(sql.contains("GROUP BY"));
        assert!(sql.contains("\"id\""));
        assert!(sql.contains("\"email\""));
    }

    #[test]
    fn build_pk_pre_cleanup_no_baseline_pk_skips_cleanup() {
        // Empty schema → table_def lookup fails → try_resolve returns None →
        // the let-else fallback path returns vec![] without DELETE.
        let queries = build_pk_pre_cleanup::<&str>(
            DatabaseBackend::Postgres,
            "nonexistent",
            &["id"],
            &PrimaryKeyAdditionStrategy::DeleteDuplicates {
                keep: KeepPolicy::First,
            },
            &[],
        );
        assert!(queries.is_empty());
    }

    #[test]
    fn try_resolve_single_pk_column_returns_none_for_composite_pk() {
        // Composite PK (len != 1) → returns None.
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
                strategy: PrimaryKeyAdditionStrategy::default(),
            }],
        }];
        let resolved = try_resolve_single_pk_column("events", &schema, &["new_col"]);
        assert!(resolved.is_none());
    }

    #[test]
    fn try_resolve_single_pk_column_returns_none_when_pk_in_new_set() {
        // PK column appears inside the new PK set → returns None (tautology
        // guard: NOT IN (SELECT MIN(pk) GROUP BY pk) would match every row).
        let schema = schema_with_single_pk();
        let resolved = try_resolve_single_pk_column("users", &schema, &["id"]);
        assert!(resolved.is_none());
    }

    #[test]
    fn try_resolve_single_pk_column_returns_none_for_unknown_table() {
        let resolved = try_resolve_single_pk_column::<&str>("nonexistent", &[], &["id"]);
        assert!(resolved.is_none());
    }

    #[test]
    fn try_resolve_single_pk_column_returns_none_for_table_without_pk() {
        // Table exists but has no PK (no constraints, no inline primary_key) →
        // the `find_map(...).or_else(...)?` returns None.
        let schema = vec![TableDef {
            name: "logs".into(),
            description: None,
            columns: vec![ColumnDef::new(
                "msg",
                ColumnType::Simple(SimpleColumnType::Text),
                false,
            )],
            constraints: vec![],
        }];
        let resolved = try_resolve_single_pk_column("logs", &schema, &["id"]);
        assert!(resolved.is_none());
    }
}
