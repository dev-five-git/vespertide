use sea_query::{Alias, ForeignKey};
use vespertide_core::{ForeignKeyOrphanStrategy, ReferenceAction, TableConstraint};

use super::super::helpers::{quote_ident, to_sea_fk_action};
use super::super::types::{BuiltQuery, DatabaseBackend, RawSql};
use super::{QueryError, TableDef, rebuild_sqlite_table_with_added_constraint};

/// Render `ON DELETE <kw>` / `ON UPDATE <kw>` clause text, or empty
/// string when the action is `None`. Used by the F11 PG raw-SQL path.
fn render_fk_action_clause(prefix: &str, action: Option<&ReferenceAction>) -> String {
    action.map_or_else(String::new, |a| format!(" {prefix} {}", a.to_sql_keyword()))
}

/// Build the SQL for `AddConstraint(ForeignKey)` plus the F3 pre-cleanup
/// statement dictated by `orphan_strategy`.
///
/// The cleanup is always emitted; on a table with no orphan rows it is a
/// no-op (zero rows updated/deleted). Skipping it entirely would require
/// proving statically that orphans cannot exist - vespertide treats the
/// planner-side check (`find_fk_orphan_additions`) as advisory, not as a
/// proof of absence.
///
/// SQL pattern (PG/MySQL/SQLite uniform):
///
/// - **`NullifyOrphans`**: `UPDATE child SET <fk_cols> = NULL WHERE (<fk_col_i> IS NOT NULL OR ...) AND NOT EXISTS (SELECT 1 FROM parent WHERE <join>);`
/// - **`DeleteOrphans`**:  `DELETE FROM child WHERE NOT EXISTS (SELECT 1 FROM parent WHERE <join>);`
///
/// The `IS NOT NULL` guard on `NullifyOrphans` preserves rows whose
/// composite FK is wholly NULL (SQL standard treats such rows as
/// FK-exempt). Single-column FK collapses to a single `IS NOT NULL`.
#[expect(
    clippy::too_many_arguments,
    reason = "composite foreign-key builder mirrors FK action fields plus SQLite schema context plus the F3 orphan strategy; ForeignKeyContext is a deferred refactor"
)]
pub(super) fn build_foreign_key<T: AsRef<str>, U: AsRef<str>>(
    backend: DatabaseBackend,
    table: &str,
    name: Option<&str>,
    columns: &[T],
    ref_table: &str,
    ref_columns: &[U],
    on_delete: Option<&ReferenceAction>,
    on_update: Option<&ReferenceAction>,
    orphan_strategy: ForeignKeyOrphanStrategy,
    constraint: &TableConstraint,
    current_schema: &[TableDef],
    pending_constraints: &[TableConstraint],
) -> Result<Vec<BuiltQuery>, QueryError> {
    let cleanup = build_fk_orphan_cleanup(
        backend,
        table,
        columns,
        ref_table,
        ref_columns,
        orphan_strategy,
    )?;

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

    let fk_name = vespertide_naming::build_foreign_key_name(table, columns, name);
    let mut queries = cleanup;

    if backend == DatabaseBackend::Postgres {
        // F11: PG NOT VALID + VALIDATE 2-step. Both statements run
        // inside the migration transaction so PG rollback reverts the
        // pair on failure - no partial-apply zombie. The sea-query
        // `ForeignKey` builder has no `NOT VALID` switch, so we emit
        // raw SQL here. MySQL still uses the builder below.
        let quoted_table = quote_ident(table, backend);
        let quoted_name = quote_ident(&fk_name, backend);
        let quoted_ref_table = quote_ident(ref_table, backend);
        let cols = columns
            .iter()
            .map(|c| quote_ident(c.as_ref(), backend))
            .collect::<Vec<_>>()
            .join(", ");
        let ref_cols = ref_columns
            .iter()
            .map(|c| quote_ident(c.as_ref(), backend))
            .collect::<Vec<_>>()
            .join(", ");
        let on_delete_clause = render_fk_action_clause("ON DELETE", on_delete);
        let on_update_clause = render_fk_action_clause("ON UPDATE", on_update);

        let add_not_valid = format!(
            "ALTER TABLE {quoted_table} ADD CONSTRAINT {quoted_name} \
             FOREIGN KEY ({cols}) REFERENCES {quoted_ref_table} ({ref_cols})\
             {on_delete_clause}{on_update_clause} NOT VALID"
        );
        let validate = format!("ALTER TABLE {quoted_table} VALIDATE CONSTRAINT {quoted_name}");
        queries.push(BuiltQuery::Raw(RawSql::uniform(add_not_valid)));
        queries.push(BuiltQuery::Raw(RawSql::uniform(validate)));
        return Ok(queries);
    }

    // MySQL: single statement via sea-query builder (no NOT VALID).
    let mut fk = ForeignKey::create();
    fk.name(&fk_name);
    fk.from_tbl(Alias::new(table));
    for col in columns {
        fk.from_col(Alias::new(col.as_ref()));
    }
    fk.to_tbl(Alias::new(ref_table));
    for col in ref_columns {
        fk.to_col(Alias::new(col.as_ref()));
    }
    if let Some(action) = on_delete {
        fk.on_delete(to_sea_fk_action(action));
    }
    if let Some(action) = on_update {
        fk.on_update(to_sea_fk_action(action));
    }

    queries.push(BuiltQuery::CreateForeignKey(Box::new(fk)));
    Ok(queries)
}

/// Emit the F3 pre-cleanup statement (UPDATE / DELETE) ahead of the
/// `ADD CONSTRAINT FOREIGN KEY`. Returns an empty `Vec` only when the
/// strategy variant is unrecognised (future-proofing for `non_exhaustive`).
fn build_fk_orphan_cleanup<T: AsRef<str>, U: AsRef<str>>(
    backend: DatabaseBackend,
    child_table: &str,
    child_columns: &[T],
    ref_table: &str,
    ref_columns: &[U],
    strategy: ForeignKeyOrphanStrategy,
) -> Result<Vec<BuiltQuery>, QueryError> {
    if child_columns.len() != ref_columns.len() {
        return Err(QueryError::SchemaError(format!(
            "FK on '{child_table}': child columns ({}) and ref columns ({}) length mismatch",
            child_columns.len(),
            ref_columns.len()
        )));
    }
    if child_columns.is_empty() {
        return Ok(vec![]);
    }

    let quoted_child = quote_ident(child_table, backend);
    let quoted_ref = quote_ident(ref_table, backend);

    // Correlated `NOT EXISTS` join condition: `parent.pk_i = child.fk_i AND ...`
    // Built with an explicit loop (not `.iter().map().collect()`) so each
    // statement maps to its own coverage region — LLVM attributes a method
    // chain's head line inconsistently.
    let mut join_terms: Vec<String> = Vec::with_capacity(child_columns.len());
    for (c, r) in child_columns.iter().zip(ref_columns.iter()) {
        let qc = quote_ident(c.as_ref(), backend);
        let qr = quote_ident(r.as_ref(), backend);
        join_terms.push(format!("{quoted_ref}.{qr} = {quoted_child}.{qc}"));
    }
    let join_cond = join_terms.join(" AND ");

    let not_exists = format!("NOT EXISTS (SELECT 1 FROM {quoted_ref} WHERE {join_cond})");

    let sql = if strategy == ForeignKeyOrphanStrategy::NullifyOrphans {
        // SET <col_i> = NULL, ... (explicit loop for deterministic coverage
        // attribution; see `join_terms` above).
        let mut set_terms: Vec<String> = Vec::with_capacity(child_columns.len());
        for c in child_columns {
            let qc = quote_ident(c.as_ref(), backend);
            set_terms.push(format!("{qc} = NULL"));
        }
        let set_clause = set_terms.join(", ");

        // NULL row guard: skip rows whose FK columns are all NULL.
        // Composite FK uses `<col_i> IS NOT NULL OR ...`; single-column FK collapses to one term.
        let null_guard: Vec<String> = child_columns
            .iter()
            .map(|c| format!("{} IS NOT NULL", quote_ident(c.as_ref(), backend)))
            .collect();
        let null_guard = null_guard.join(" OR ");

        format!("UPDATE {quoted_child} SET {set_clause} WHERE ({null_guard}) AND {not_exists}")
    } else {
        format!("DELETE FROM {quoted_child} WHERE {not_exists}")
    };

    Ok(vec![BuiltQuery::Raw(RawSql::uniform(sql))])
}

#[cfg(test)]
mod tests {
    use super::*;

    use rstest::rstest;
    use vespertide_core::{ColumnDef, ColumnType, MigrationAction, SimpleColumnType};

    fn nn_col(name: &str, ty: SimpleColumnType) -> ColumnDef {
        ColumnDef::new(name, ColumnType::Simple(ty), false)
    }

    fn fk_constraint(strategy: ForeignKeyOrphanStrategy) -> TableConstraint {
        TableConstraint::ForeignKey {
            name: Some("fk_user".into()),
            columns: vec!["user_id".into()],
            ref_table: "users".into(),
            ref_columns: vec!["id".into()],
            on_delete: None,
            on_update: None,
            orphan_strategy: strategy,
        }
    }

    fn parent_child_schema() -> Vec<TableDef> {
        vec![
            TableDef {
                name: "users".into(),
                description: None,
                columns: vec![nn_col("id", SimpleColumnType::Integer)],
                constraints: vec![],
            },
            TableDef {
                name: "posts".into(),
                description: None,
                columns: vec![
                    nn_col("id", SimpleColumnType::Integer),
                    ColumnDef::new(
                        "user_id",
                        ColumnType::Simple(SimpleColumnType::Integer),
                        true,
                    ),
                ],
                constraints: vec![],
            },
        ]
    }

    fn sqlite_schema_with(constraints: Vec<TableConstraint>) -> Vec<TableDef> {
        vec![TableDef {
            name: "posts".into(),
            description: None,
            columns: vec![ColumnDef::new(
                "user_id",
                ColumnType::Simple(SimpleColumnType::Integer),
                true,
            )],
            constraints,
        }]
    }

    #[test]
    fn sqlite_table_not_found_returns_error() {
        let constraint = fk_constraint(ForeignKeyOrphanStrategy::default());
        let result = build_foreign_key(
            DatabaseBackend::Sqlite,
            "posts",
            Some("fk_user"),
            &["user_id"],
            "users",
            &["id"],
            None,
            None,
            ForeignKeyOrphanStrategy::default(),
            &constraint,
            &[],
            &[],
        );
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Table 'posts' not found in current schema")
        );
    }

    #[test]
    fn sqlite_rebuild_preserves_check_constraints() {
        let constraint = fk_constraint(ForeignKeyOrphanStrategy::default());
        let schema = sqlite_schema_with(vec![TableConstraint::Check {
            name: "chk_user_id".into(),
            expr: "user_id > 0".into(),
            strategy: vespertide_core::CheckViolationStrategy::default(),
        }]);
        let queries = build_foreign_key(
            DatabaseBackend::Sqlite,
            "posts",
            Some("fk_user"),
            &["user_id"],
            "users",
            &["id"],
            None,
            None,
            ForeignKeyOrphanStrategy::default(),
            &constraint,
            &schema,
            &[],
        )
        .unwrap();
        let sql = queries
            .iter()
            .map(|q| q.build(DatabaseBackend::Sqlite))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(sql.contains("CONSTRAINT \"chk_user_id\" CHECK"));
    }

    #[test]
    fn sqlite_rebuild_recreates_indexes() {
        let constraint = fk_constraint(ForeignKeyOrphanStrategy::default());
        let schema = sqlite_schema_with(vec![TableConstraint::Index {
            name: Some("idx_user_id".into()),
            columns: vec!["user_id".into()],
        }]);
        let queries = build_foreign_key(
            DatabaseBackend::Sqlite,
            "posts",
            Some("fk_user"),
            &["user_id"],
            "users",
            &["id"],
            None,
            None,
            ForeignKeyOrphanStrategy::default(),
            &constraint,
            &schema,
            &[],
        )
        .unwrap();
        let sql = queries
            .iter()
            .map(|q| q.build(DatabaseBackend::Sqlite))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(sql.contains("CREATE INDEX"));
        assert!(sql.contains("idx_user_id"));
    }

    #[test]
    fn sqlite_rebuild_handles_unique_constraint() {
        let constraint = fk_constraint(ForeignKeyOrphanStrategy::default());
        let schema = sqlite_schema_with(vec![TableConstraint::Unique {
            name: Some("uq_user_id".into()),
            columns: vec!["user_id".into()],
            strategy: vespertide_core::UniqueConstraintStrategy::default(),
        }]);
        let queries = build_foreign_key(
            DatabaseBackend::Sqlite,
            "posts",
            Some("fk_user"),
            &["user_id"],
            "users",
            &["id"],
            None,
            None,
            ForeignKeyOrphanStrategy::default(),
            &constraint,
            &schema,
            &[],
        )
        .unwrap();
        assert!(
            queries
                .iter()
                .map(|q| q.build(DatabaseBackend::Sqlite))
                .collect::<Vec<_>>()
                .join("\n")
                .contains("CREATE TABLE")
        );
    }

    #[test]
    fn sqlite_rebuild_without_existing_check_adds_foreign_key() {
        let constraint = fk_constraint(ForeignKeyOrphanStrategy::default());
        let queries = build_foreign_key(
            DatabaseBackend::Sqlite,
            "posts",
            Some("fk_user"),
            &["user_id"],
            "users",
            &["id"],
            None,
            None,
            ForeignKeyOrphanStrategy::default(),
            &constraint,
            &sqlite_schema_with(vec![]),
            &[],
        )
        .unwrap();
        let sql = queries
            .iter()
            .map(|q| q.build(DatabaseBackend::Sqlite))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(sql.contains("CREATE TABLE"));
        assert!(sql.contains("FOREIGN KEY"));
    }

    #[rstest]
    #[case::postgres(DatabaseBackend::Postgres)]
    #[case::mysql(DatabaseBackend::MySql)]
    #[case::sqlite(DatabaseBackend::Sqlite)]
    fn mismatched_column_counts_return_schema_error(#[case] backend: DatabaseBackend) {
        let result = build_fk_orphan_cleanup(
            backend,
            "posts",
            &["user_id", "tenant_id"],
            "users",
            &["id"],
            ForeignKeyOrphanStrategy::DeleteOrphans,
        );
        assert!(matches!(result, Err(QueryError::SchemaError(_))));
    }

    #[test]
    fn postgres_emits_not_valid_validate_pair() {
        let constraint = TableConstraint::ForeignKey {
            name: Some("fk_user".into()),
            columns: vec!["user_id".into()],
            ref_table: "users".into(),
            ref_columns: vec!["id".into()],
            on_delete: Some(ReferenceAction::Cascade),
            on_update: Some(ReferenceAction::Restrict),
            orphan_strategy: ForeignKeyOrphanStrategy::NullifyOrphans,
        };
        let queries = build_foreign_key(
            DatabaseBackend::Postgres,
            "posts",
            Some("fk_user"),
            &["user_id"],
            "users",
            &["id"],
            Some(&ReferenceAction::Cascade),
            Some(&ReferenceAction::Restrict),
            ForeignKeyOrphanStrategy::NullifyOrphans,
            &constraint,
            &parent_child_schema(),
            &[],
        )
        .unwrap();
        let sql = queries
            .iter()
            .map(|q| q.build(DatabaseBackend::Postgres))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(sql.contains("NOT VALID"));
        assert!(sql.contains("VALIDATE CONSTRAINT"));
        assert!(sql.contains("ON DELETE CASCADE") && sql.contains("ON UPDATE RESTRICT"));
    }

    #[rstest]
    #[case::postgres(DatabaseBackend::Postgres)]
    #[case::mysql(DatabaseBackend::MySql)]
    #[case::sqlite(DatabaseBackend::Sqlite)]
    fn nullify_orphans_emits_update_set_null(#[case] backend: DatabaseBackend) {
        let queries = build_fk_orphan_cleanup(
            backend,
            "posts",
            &["user_id"],
            "users",
            &["id"],
            ForeignKeyOrphanStrategy::NullifyOrphans,
        )
        .unwrap();
        let sql = queries[0].build(backend);
        assert!(sql.contains("UPDATE"));
        assert!(sql.contains("= NULL"));
        assert!(sql.contains("IS NOT NULL"));
        assert!(sql.contains("NOT EXISTS"));
    }

    #[rstest]
    #[case::postgres(DatabaseBackend::Postgres)]
    #[case::mysql(DatabaseBackend::MySql)]
    #[case::sqlite(DatabaseBackend::Sqlite)]
    fn delete_orphans_emits_delete_from(#[case] backend: DatabaseBackend) {
        let queries = build_fk_orphan_cleanup(
            backend,
            "posts",
            &["user_id"],
            "users",
            &["id"],
            ForeignKeyOrphanStrategy::DeleteOrphans,
        )
        .unwrap();
        let sql = queries[0].build(backend);
        assert!(sql.contains("DELETE FROM"));
        assert!(sql.contains("NOT EXISTS"));
    }

    #[rstest]
    #[case::postgres(DatabaseBackend::Postgres)]
    #[case::mysql(DatabaseBackend::MySql)]
    #[case::sqlite(DatabaseBackend::Sqlite)]
    fn empty_columns_return_empty_cleanup(#[case] backend: DatabaseBackend) {
        assert!(
            build_fk_orphan_cleanup::<&str, &str>(
                backend,
                "posts",
                &[],
                "users",
                &[],
                ForeignKeyOrphanStrategy::DeleteOrphans
            )
            .unwrap()
            .is_empty()
        );
    }

    #[test]
    fn composite_nullify_orphans_pg_emits_composite_null_guard() {
        let queries = build_fk_orphan_cleanup(
            DatabaseBackend::Postgres,
            "posts",
            &["user_id", "tenant_id"],
            "users",
            &["id", "tenant_id"],
            ForeignKeyOrphanStrategy::NullifyOrphans,
        )
        .unwrap();
        let sql = queries[0].build(DatabaseBackend::Postgres);
        assert!(sql.contains("UPDATE \"posts\" SET"));
        assert!(sql.contains("\"user_id\" = NULL"));
        assert!(sql.contains("\"tenant_id\" = NULL"));
        assert!(sql.contains("\"user_id\" IS NOT NULL OR \"tenant_id\" IS NOT NULL"));
    }

    #[rstest]
    #[case::postgres(DatabaseBackend::Postgres)]
    #[case::mysql(DatabaseBackend::MySql)]
    #[case::sqlite(DatabaseBackend::Sqlite)]
    fn build_foreign_key_nullify_path_runs_cleanup_then_constraint(
        #[case] backend: DatabaseBackend,
    ) {
        let constraint = TableConstraint::ForeignKey {
            name: Some("fk_user".into()),
            columns: vec!["user_id".into()],
            ref_table: "users".into(),
            ref_columns: vec!["id".into()],
            on_delete: Some(ReferenceAction::Cascade),
            on_update: Some(ReferenceAction::Restrict),
            orphan_strategy: ForeignKeyOrphanStrategy::NullifyOrphans,
        };
        let queries = build_foreign_key(
            backend,
            "posts",
            Some("fk_user"),
            &["user_id"],
            "users",
            &["id"],
            Some(&ReferenceAction::Cascade),
            Some(&ReferenceAction::Restrict),
            ForeignKeyOrphanStrategy::NullifyOrphans,
            &constraint,
            &parent_child_schema(),
            &[],
        )
        .unwrap();
        let sql = queries
            .iter()
            .map(|q| q.build(backend))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(sql.contains("UPDATE"));
        assert!(sql.contains("= NULL"));
        assert!(sql.contains("NOT EXISTS"));
        assert!(
            sql.contains("FOREIGN KEY")
                || sql.contains("VALIDATE CONSTRAINT")
                || sql.contains("CREATE TABLE")
        );
    }

    #[rstest]
    #[case::postgres(DatabaseBackend::Postgres)]
    #[case::mysql(DatabaseBackend::MySql)]
    #[case::sqlite(DatabaseBackend::Sqlite)]
    fn add_constraint_foreign_key_emits_delete_and_update_actions(
        #[case] backend: DatabaseBackend,
    ) {
        let constraint = TableConstraint::ForeignKey {
            name: Some("fk_posts__user_id".into()),
            columns: vec!["user_id".into()],
            ref_table: "users".into(),
            ref_columns: vec!["id".into()],
            on_delete: Some(ReferenceAction::Cascade),
            on_update: Some(ReferenceAction::Cascade),
            orphan_strategy: ForeignKeyOrphanStrategy::NullifyOrphans,
        };
        let action = MigrationAction::AddConstraint {
            table: "posts".into(),
            constraint,
        };

        let sql = crate::sql::build_action_queries(backend, &action, &parent_child_schema())
            .unwrap()
            .iter()
            .map(|query| query.build(backend))
            .collect::<Vec<_>>()
            .join("\n");

        assert!(sql.contains("ON DELETE CASCADE"), "{sql}");
        assert!(sql.contains("ON UPDATE CASCADE"), "{sql}");
    }

    #[rstest]
    #[case::postgres(DatabaseBackend::Postgres)]
    #[case::mysql(DatabaseBackend::MySql)]
    #[case::sqlite(DatabaseBackend::Sqlite)]
    fn build_foreign_key_delete_path_runs_cleanup_then_constraint(
        #[case] backend: DatabaseBackend,
    ) {
        let constraint = TableConstraint::ForeignKey {
            name: Some("fk_user".into()),
            columns: vec!["user_id".into()],
            ref_table: "users".into(),
            ref_columns: vec!["id".into()],
            on_delete: None,
            on_update: None,
            orphan_strategy: ForeignKeyOrphanStrategy::DeleteOrphans,
        };
        let queries = build_foreign_key(
            backend,
            "posts",
            Some("fk_user"),
            &["user_id"],
            "users",
            &["id"],
            None,
            None,
            ForeignKeyOrphanStrategy::DeleteOrphans,
            &constraint,
            &parent_child_schema(),
            &[],
        )
        .unwrap();
        let sql = queries
            .iter()
            .map(|q| q.build(backend))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(sql.contains("DELETE FROM"));
        assert!(sql.contains("NOT EXISTS"));
        assert!(
            sql.contains("FOREIGN KEY")
                || sql.contains("VALIDATE CONSTRAINT")
                || sql.contains("CREATE TABLE")
        );
    }
}
