mod mysql;
mod postgres;
mod sqlite;

use vespertide_core::{TableConstraint, TableDef};

use super::types::{BuiltQuery, DatabaseBackend};
use crate::error::QueryError;

// Re-export the canonical `DROP INDEX` builder from `sql::helpers` so
// the SQLite fallback below and the `TableConstraint::Index` arms in
// `mysql.rs` / `postgres.rs` keep resolving `super::build_drop_index_query`
// unchanged. The implementation now lives in one place.
pub(super) use super::helpers::build_drop_index_query;

pub fn build_remove_constraint(
    backend: DatabaseBackend,
    table: &str,
    constraint: &TableConstraint,
    current_schema: &[TableDef],
    pending_constraints: &[TableConstraint],
) -> Result<Vec<BuiltQuery>, QueryError> {
    if backend == DatabaseBackend::Sqlite && sqlite::requires_rebuild(constraint) {
        return sqlite::build_remove_constraint(
            table,
            constraint,
            current_schema,
            pending_constraints,
        );
    }

    if backend == DatabaseBackend::Postgres {
        Ok(postgres::build_remove_constraint(table, constraint))
    } else if backend == DatabaseBackend::MySql {
        Ok(mysql::build_remove_constraint(table, constraint))
    } else {
        build_drop_index(table, constraint)
    }
}

fn build_drop_index(
    table: &str,
    constraint: &TableConstraint,
) -> Result<Vec<BuiltQuery>, QueryError> {
    let TableConstraint::Index { name, columns } = constraint else {
        return Err(QueryError::BackendError {
            backend: DatabaseBackend::Sqlite,
            message: format!(
                "SQLite constraint '{}' requires a table rebuild",
                constraint_kind(constraint)
            ),
        });
    };

    let index_name = vespertide_naming::build_index_name(table, columns, name.as_deref());
    Ok(vec![build_drop_index_query(table, &index_name)])
}

fn constraint_kind(constraint: &TableConstraint) -> &'static str {
    match constraint {
        TableConstraint::PrimaryKey { .. } => "primary key",
        TableConstraint::Unique { .. } => "unique",
        TableConstraint::ForeignKey { .. } => "foreign key",
        TableConstraint::Index { .. } => "index",
        TableConstraint::Check { .. } => "check",
        _ => unreachable!("TableConstraint is #[non_exhaustive]; all variants are matched above"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sql::types::DatabaseBackend;
    use crate::test_support::joined_sql;
    use insta::{assert_snapshot, with_settings};
    use rstest::rstest;
    use vespertide_core::{
        ColumnDef, ColumnType, SimpleColumnType, StrOrBoolOrArray, TableConstraint,
    };

    fn int_col(name: &str) -> ColumnDef {
        col(name, SimpleColumnType::Integer)
    }

    fn text_col(name: &str) -> ColumnDef {
        col(name, SimpleColumnType::Text)
    }

    fn col(name: &str, ty: SimpleColumnType) -> ColumnDef {
        ColumnDef {
            name: name.into(),
            r#type: ColumnType::Simple(ty),
            nullable: name != "id",
            default: None,
            comment: None,
            primary_key: None,
            unique: None,
            index: None,
            foreign_key: None,
        }
    }

    fn table(name: &str, columns: Vec<ColumnDef>, constraints: Vec<TableConstraint>) -> TableDef {
        TableDef {
            name: name.into(),
            description: None,
            columns,
            constraints,
        }
    }

    fn pk() -> TableConstraint {
        TableConstraint::PrimaryKey {
            columns: vec!["id".into()],
            auto_increment: false,
            strategy: vespertide_core::PrimaryKeyAdditionStrategy::default(),
        }
    }

    fn unique(name: Option<&str>, columns: &[&str]) -> TableConstraint {
        TableConstraint::Unique {
            name: name.map(Into::into),
            columns: columns.iter().copied().map(Into::into).collect(),
            strategy: vespertide_core::UniqueConstraintStrategy::DeleteDuplicates {
                keep: vespertide_core::KeepPolicy::First,
            },
        }
    }

    fn fk(name: Option<&str>) -> TableConstraint {
        TableConstraint::ForeignKey {
            name: name.map(Into::into),
            columns: vec!["user_id".into()],
            ref_table: "users".into(),
            ref_columns: vec!["id".into()],
            on_delete: None,
            on_update: None,
            orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
        }
    }

    fn check(name: &str) -> TableConstraint {
        TableConstraint::Check {
            name: name.into(),
            expr: "age > 0".into(),
            strategy: vespertide_core::CheckViolationStrategy::default(),
        }
    }

    fn index(name: &str, columns: &[&str]) -> TableConstraint {
        TableConstraint::Index {
            name: Some(name.into()),
            columns: columns.iter().copied().map(Into::into).collect(),
        }
    }

    fn render(
        backend: DatabaseBackend,
        table_name: &str,
        constraint: &TableConstraint,
        schema: &[TableDef],
    ) -> String {
        joined_sql(
            backend,
            &build_remove_constraint(backend, table_name, constraint, schema, &[]).unwrap(),
        )
    }

    fn assert_rendered(
        backend: DatabaseBackend,
        table_name: &str,
        constraint: &TableConstraint,
        schema: &[TableDef],
        expected: &[&str],
    ) -> String {
        let sql = render(backend, table_name, constraint, schema);
        for fragment in expected {
            assert!(
                sql.contains(fragment),
                "Expected SQL to contain '{fragment}', got: {sql}"
            );
        }
        sql
    }

    #[rstest]
    #[case::remove_constraint_primary_key_postgres(
        "remove_constraint_primary_key_postgres",
        DatabaseBackend::Postgres,
        pk(),
        vec![int_col("id")],
        &["DROP CONSTRAINT \"users_pkey\""]
    )]
    #[case::remove_constraint_primary_key_mysql(
        "remove_constraint_primary_key_mysql",
        DatabaseBackend::MySql,
        pk(),
        vec![int_col("id")],
        &["DROP PRIMARY KEY"]
    )]
    #[case::remove_constraint_primary_key_sqlite(
        "remove_constraint_primary_key_sqlite",
        DatabaseBackend::Sqlite,
        pk(),
        vec![int_col("id")],
        &["CREATE TABLE \"users_temp\""]
    )]
    #[case::remove_constraint_unique_named_postgres(
        "remove_constraint_unique_named_postgres",
        DatabaseBackend::Postgres,
        unique(Some("uq_email"), &["email"]),
        vec![int_col("id")],
        &["DROP INDEX \"uq_users__uq_email\""]
    )]
    #[case::remove_constraint_unique_named_mysql(
        "remove_constraint_unique_named_mysql",
        DatabaseBackend::MySql,
        unique(Some("uq_email"), &["email"]),
        vec![int_col("id")],
        &["DROP INDEX `uq_users__uq_email`"]
    )]
    #[case::remove_constraint_unique_named_sqlite(
        "remove_constraint_unique_named_sqlite",
        DatabaseBackend::Sqlite,
        unique(Some("uq_email"), &["email"]),
        vec![int_col("id")],
        &["CREATE TABLE \"users_temp\""]
    )]
    #[case::remove_constraint_foreign_key_named_postgres(
        "remove_constraint_foreign_key_named_postgres",
        DatabaseBackend::Postgres,
        fk(Some("fk_user")),
        vec![int_col("id"), int_col("user_id")],
        &["DROP CONSTRAINT \"fk_users__fk_user\""]
    )]
    #[case::remove_constraint_foreign_key_named_mysql(
        "remove_constraint_foreign_key_named_mysql",
        DatabaseBackend::MySql,
        fk(Some("fk_user")),
        vec![int_col("id"), int_col("user_id")],
        &["DROP FOREIGN KEY `fk_users__fk_user`"]
    )]
    #[case::remove_constraint_foreign_key_named_sqlite(
        "remove_constraint_foreign_key_named_sqlite",
        DatabaseBackend::Sqlite,
        fk(Some("fk_user")),
        vec![int_col("id"), int_col("user_id")],
        &["CREATE TABLE \"users_temp\""]
    )]
    #[case::remove_constraint_check_named_postgres(
        "remove_constraint_check_named_postgres",
        DatabaseBackend::Postgres,
        check("chk_age"),
        vec![int_col("id"), int_col("age")],
        &["DROP CONSTRAINT \"chk_age\""]
    )]
    #[case::remove_constraint_check_named_mysql(
        "remove_constraint_check_named_mysql",
        DatabaseBackend::MySql,
        check("chk_age"),
        vec![int_col("id"), int_col("age")],
        &["DROP CHECK `chk_age`"]
    )]
    #[case::remove_constraint_check_named_sqlite(
        "remove_constraint_check_named_sqlite",
        DatabaseBackend::Sqlite,
        check("chk_age"),
        vec![int_col("id"), int_col("age")],
        &["CREATE TABLE \"users_temp\""]
    )]
    fn test_remove_constraint(
        #[case] title: &str,
        #[case] backend: DatabaseBackend,
        #[case] constraint: TableConstraint,
        #[case] columns: Vec<ColumnDef>,
        #[case] expected: &[&str],
    ) {
        let schema = vec![table("users", columns, vec![constraint.clone()])];
        let sql = assert_rendered(backend, "users", &constraint, &schema, expected);

        with_settings!({ snapshot_path => "../snapshots", snapshot_suffix => format!("remove_constraint_{title}") }, {
            assert_snapshot!(sql);
        });
    }

    #[rstest]
    #[case::primary_key(DatabaseBackend::Sqlite, pk())]
    #[case::unique(DatabaseBackend::Sqlite, unique(Some("uq_email"), &["email"]))]
    #[case::foreign_key(DatabaseBackend::Sqlite, fk(Some("fk_user")))]
    #[case::check(DatabaseBackend::Sqlite, check("chk_age"))]
    fn test_remove_constraint_sqlite_table_not_found(
        #[case] backend: DatabaseBackend,
        #[case] constraint: TableConstraint,
    ) {
        let result = build_remove_constraint(backend, "nonexistent_table", &constraint, &[], &[]);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Table 'nonexistent_table' not found in current schema")
        );
    }

    #[rstest]
    #[case::remove_primary_key_with_index(
        "remove_primary_key_with_index",
        "users",
        pk(),
        vec![int_col("id")],
        vec![pk(), index("idx_id", &["id"])],
        Some("ix_users__idx_id")
    )]
    #[case::remove_unique_with_index(
        "remove_unique_with_index",
        "users",
        unique(Some("uq_email"), &["email"]),
        vec![int_col("id"), text_col("email")],
        vec![unique(Some("uq_email"), &["email"]), index("idx_id", &["id"])],
        Some("ix_users__idx_id")
    )]
    #[case::remove_foreign_key_with_index(
        "remove_foreign_key_with_index",
        "posts",
        fk(Some("fk_user")),
        vec![int_col("id"), int_col("user_id")],
        vec![fk(Some("fk_user")), index("idx_user_id", &["user_id"])],
        Some("idx_user_id")
    )]
    #[case::remove_check_with_index(
        "remove_check_with_index",
        "users",
        check("chk_age"),
        vec![int_col("id"), int_col("age")],
        vec![check("chk_age"), index("idx_age", &["age"])],
        Some("idx_age")
    )]
    #[case::remove_primary_key_with_unique_constraint(
        "remove_primary_key_with_unique_constraint",
        "users",
        pk(),
        vec![int_col("id"), text_col("email")],
        vec![pk(), unique(Some("uq_email"), &["email"])],
        None
    )]
    #[case::remove_unique_with_other_unique_constraint(
        "remove_unique_with_other_unique_constraint",
        "users",
        unique(Some("uq_email"), &["email"]),
        vec![int_col("id"), text_col("email"), text_col("name")],
        vec![unique(Some("uq_email"), &["email"]), unique(Some("uq_name"), &["name"])],
        None
    )]
    #[case::remove_foreign_key_with_unique_constraint(
        "remove_foreign_key_with_unique_constraint",
        "posts",
        fk(Some("fk_user")),
        vec![int_col("id"), int_col("user_id")],
        vec![fk(Some("fk_user")), unique(Some("uq_user_id"), &["user_id"])],
        None
    )]
    #[case::remove_check_with_unique_constraint(
        "remove_check_with_unique_constraint",
        "users",
        check("chk_age"),
        vec![int_col("id"), int_col("age")],
        vec![check("chk_age"), unique(Some("uq_age"), &["age"])],
        None
    )]
    #[case::remove_unique_with_other_constraints(
        "remove_unique_with_other_constraints",
        "users",
        unique(Some("uq_email"), &["email"]),
        vec![int_col("id"), text_col("email")],
        vec![pk(), unique(Some("uq_email"), &["email"]), TableConstraint::Check { name: "chk_email".into(), expr: "email IS NOT NULL".into(), strategy: vespertide_core::CheckViolationStrategy::default() }],
        None
    )]
    #[case::remove_foreign_key_with_other_constraints(
        "remove_foreign_key_with_other_constraints",
        "posts",
        fk(Some("fk_user")),
        vec![int_col("id"), int_col("user_id")],
        vec![pk(), fk(Some("fk_user")), unique(Some("uq_user_id"), &["user_id"]), TableConstraint::Check { name: "chk_user_id".into(), expr: "user_id > 0".into(), strategy: vespertide_core::CheckViolationStrategy::default() }],
        None
    )]
    #[case::remove_check_with_other_constraints(
        "remove_check_with_other_constraints",
        "users",
        check("chk_age"),
        vec![int_col("id"), int_col("age")],
        vec![pk(), unique(Some("uq_age"), &["age"]), check("chk_age")],
        None
    )]
    fn test_remove_constraint_with_companion_constraints(
        #[case] title: &str,
        #[case] table_name: &str,
        #[case] constraint: TableConstraint,
        #[case] columns: Vec<ColumnDef>,
        #[case] constraints: Vec<TableConstraint>,
        #[case] sqlite_fragment: Option<&str>,
    ) {
        for backend in [
            DatabaseBackend::Postgres,
            DatabaseBackend::MySql,
            DatabaseBackend::Sqlite,
        ] {
            let schema = vec![table(table_name, columns.clone(), constraints.clone())];
            let sql = render(backend, table_name, &constraint, &schema);

            if matches!(backend, DatabaseBackend::Sqlite) {
                assert!(sql.contains("CREATE TABLE"));
                if let Some(fragment) = sqlite_fragment {
                    assert!(sql.contains(fragment), "Expected {fragment} in {sql}");
                }
            } else {
                assert!(sql.contains("DROP"));
            }

            let _ = title;
        }
    }

    #[rstest]
    #[case::remove_unique_without_name(
        "remove_unique_without_name",
        "users",
        unique(None, &["email"]),
        vec![int_col("id"), text_col("email")]
    )]
    #[case::remove_foreign_key_without_name(
        "remove_foreign_key_without_name",
        "posts",
        fk(None),
        vec![int_col("id"), int_col("user_id")]
    )]
    fn test_remove_constraint_without_name(
        #[case] title: &str,
        #[case] table_name: &str,
        #[case] constraint: TableConstraint,
        #[case] columns: Vec<ColumnDef>,
    ) {
        for backend in [
            DatabaseBackend::Postgres,
            DatabaseBackend::MySql,
            DatabaseBackend::Sqlite,
        ] {
            let schema = vec![table(table_name, columns.clone(), vec![constraint.clone()])];
            let sql = render(backend, table_name, &constraint, &schema);
            if matches!(backend, DatabaseBackend::Sqlite) {
                assert!(sql.contains("CREATE TABLE"));
            } else {
                assert!(sql.contains("email") || sql.contains("user_id"));
            }

            let _ = title;
        }
    }

    #[test]
    fn test_remove_constraint_primary_key_postgres_direct() {
        let constraint = pk();
        let schema = vec![table(
            "orders",
            vec![int_col("id")],
            vec![constraint.clone()],
        )];
        let result = build_remove_constraint(
            DatabaseBackend::Postgres,
            "orders",
            &constraint,
            &schema,
            &[],
        )
        .unwrap();
        assert_eq!(result.len(), 1);
        assert!(
            result[0]
                .build(DatabaseBackend::Postgres)
                .contains("ALTER TABLE \"orders\" DROP CONSTRAINT \"orders_pkey\"")
        );
    }

    #[test]
    fn test_remove_constraint_primary_key_mysql_direct() {
        let constraint = pk();
        let schema = vec![table(
            "orders",
            vec![int_col("id")],
            vec![constraint.clone()],
        )];
        let result =
            build_remove_constraint(DatabaseBackend::MySql, "orders", &constraint, &schema, &[])
                .unwrap();
        assert_eq!(result.len(), 1);
        assert!(
            result[0]
                .build(DatabaseBackend::MySql)
                .contains("ALTER TABLE `orders` DROP PRIMARY KEY")
        );
    }

    #[rstest]
    #[case::remove_index_with_custom_inline_name_postgres(DatabaseBackend::Postgres)]
    #[case::remove_index_with_custom_inline_name_mysql(DatabaseBackend::MySql)]
    #[case::remove_index_with_custom_inline_name_sqlite(DatabaseBackend::Sqlite)]
    fn test_remove_constraint_index_with_custom_inline_name(#[case] backend: DatabaseBackend) {
        let constraint = index("custom_idx_email", &["email"]);
        let mut email = text_col("email");
        email.index = Some(StrOrBoolOrArray::Str("custom_idx_email".into()));
        let schema = vec![table("users", vec![email], vec![])];

        let sql = render(backend, "users", &constraint, &schema);
        assert!(sql.contains("custom_idx_email"));

        let _ = backend;
    }

    /// Drives line 18 — the `DatabaseBackend::Postgres` dispatch arm of
    /// `build_remove_constraint` — for an `Index` constraint (which never
    /// triggers the SQLite early-return on line 13-15 because
    /// `sqlite::requires_rebuild` returns false for `Index`).
    #[test]
    fn build_remove_constraint_postgres_index_takes_top_level_dispatch() {
        let constraint = index("idx_email", &["email"]);
        let queries =
            build_remove_constraint(DatabaseBackend::Postgres, "users", &constraint, &[], &[])
                .unwrap();
        assert_eq!(queries.len(), 1);
        let sql = queries[0].build(DatabaseBackend::Postgres);
        assert!(sql.contains("DROP INDEX"));
    }

    /// `build_drop_index` is the body of the `DatabaseBackend::Sqlite`
    /// dispatch arm. The public `build_remove_constraint` reaches it only
    /// when the constraint is an `Index` (everything else routes to the
    /// SQLite rebuild path), so the `Err` branch at L26 is unreachable
    /// via the public API. Calling `build_drop_index` directly from the
    /// in-module test target covers the error path (L26) plus
    /// `constraint_kind` (L34-40), and also documents that the guard
    /// must remain in place even if `requires_rebuild` is later
    /// expanded to leave new constraint kinds in the drop-index path.
    #[rstest]
    #[case::primary_key(pk(), "primary key")]
    #[case::unique(unique(Some("uq_email"), &["email"]), "unique")]
    #[case::foreign_key(fk(Some("fk_user")), "foreign key")]
    #[case::check(check("chk_age"), "check")]
    fn build_drop_index_rejects_non_index_constraint(
        #[case] constraint: TableConstraint,
        #[case] expected_kind: &str,
    ) {
        let result = build_drop_index("users", &constraint);
        assert!(result.is_err());
        let err = result.unwrap_err();
        let message = err.to_string();
        assert!(
            message.contains(expected_kind),
            "error message should mention '{expected_kind}', got: {message}"
        );
        assert!(
            message.contains("requires a table rebuild"),
            "error must explain why drop_index is not applicable, got: {message}"
        );
    }

    /// Direct cover for `constraint_kind` (L34-40). Although the helper is
    /// only used inside the error message in [`build_drop_index_rejects_non_index_constraint`],
    /// asserting the returned label for every variant pins the mapping
    /// and exercises every match arm explicitly.
    #[rstest]
    #[case::primary_key(pk(), "primary key")]
    #[case::unique(unique(None, &["email"]), "unique")]
    #[case::foreign_key(fk(None), "foreign key")]
    #[case::index(index("idx_email", &["email"]), "index")]
    #[case::check(check("chk_age"), "check")]
    fn constraint_kind_maps_every_variant(
        #[case] constraint: TableConstraint,
        #[case] expected: &str,
    ) {
        assert_eq!(constraint_kind(&constraint), expected);
    }

    #[rstest]
    #[case::named(Some("idx_user_email"))]
    #[case::unnamed(None)]
    fn remove_index_sqlite_uses_drop_index_path(#[case] name: Option<&str>) {
        let constraint = TableConstraint::Index {
            name: name.map(Into::into),
            columns: vec!["email".into()],
        };
        let queries =
            build_remove_constraint(DatabaseBackend::Sqlite, "users", &constraint, &[], &[])
                .unwrap();
        assert_eq!(queries.len(), 1);
        assert!(
            queries[0]
                .build(DatabaseBackend::Sqlite)
                .contains("DROP INDEX")
        );
    }

    #[rstest]
    #[case::postgres(DatabaseBackend::Postgres)]
    #[case::mysql(DatabaseBackend::MySql)]
    fn remove_index_non_sqlite_uses_backend_module(#[case] backend: DatabaseBackend) {
        let constraint = TableConstraint::Index {
            name: Some("idx_user_email".into()),
            columns: vec!["email".into()],
        };
        let queries = build_remove_constraint(backend, "users", &constraint, &[], &[]).unwrap();
        assert_eq!(queries.len(), 1);
        assert!(queries[0].build(backend).contains("DROP INDEX"));
    }

    /// Mutant target: `sqlite::same_named_or_column_constraint` —
    /// `candidate_columns.len() == removed_columns.len()` guard and the
    /// `&&` joining it to the zipped-`all` column comparison. Without
    /// the guard, an unnamed single-column unique `(email)` is treated
    /// as "same as" a removed unnamed composite `(email, tenant_id)`
    /// and is silently dropped from the SQLite temp-table rebuild.
    #[rstest]
    #[case::postgres(DatabaseBackend::Postgres)]
    #[case::mysql(DatabaseBackend::MySql)]
    #[case::sqlite(DatabaseBackend::Sqlite)]
    fn remove_composite_unique_preserves_overlapping_single_column_unique(
        #[case] backend: DatabaseBackend,
    ) {
        let single_email = unique(None, &["email"]);
        let composite = unique(None, &["email", "tenant_id"]);
        let schema = vec![table(
            "users",
            vec![int_col("id"), text_col("email"), int_col("tenant_id")],
            vec![single_email.clone(), composite.clone()],
        )];

        let sql = render(backend, "users", &composite, &schema);

        match backend {
            DatabaseBackend::Sqlite => {
                assert!(
                    sql.contains(
                        "CREATE UNIQUE INDEX \"uq_users__email\" ON \"users\" (\"email\")"
                    ),
                    "SQLite rebuild must recreate the (email) unique index \
                     after removing the composite (email, tenant_id); got: {sql}"
                );
                assert!(
                    !sql.contains("uq_users__email_tenant_id"),
                    "Composite unique must not appear in the recreated \
                     index set; got: {sql}"
                );
                assert_eq!(
                    sql.matches("CREATE UNIQUE INDEX").count(),
                    1,
                    "Expected exactly one CREATE UNIQUE INDEX (single \
                     email unique recreated, composite gone); got: {sql}"
                );
            }
            DatabaseBackend::Postgres => {
                assert!(
                    sql.contains("DROP INDEX \"uq_users__email_tenant_id\""),
                    "Postgres must DROP the composite-unique index by its \
                     auto-derived name; got: {sql}"
                );
            }
            DatabaseBackend::MySql => {
                assert!(
                    sql.contains("DROP INDEX `uq_users__email_tenant_id`"),
                    "MySQL must DROP the composite-unique index by its \
                     auto-derived name; got: {sql}"
                );
            }
        }
    }
}
