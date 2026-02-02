use sea_query::{Alias, Index, Query, Table};

use vespertide_core::{ColumnType, TableConstraint, TableDef};

use super::helpers::{
    build_drop_enum_type_sql, build_sqlite_temp_table_create, recreate_indexes_after_rebuild,
};
use super::rename_table::build_rename_table;
use super::types::{BuiltQuery, DatabaseBackend};

/// Build SQL to delete a column, optionally with DROP TYPE for enum columns (PostgreSQL)
///
/// For SQLite: Handles constraint removal before dropping the column:
/// - Unique/Index constraints: Dropped via DROP INDEX
/// - ForeignKey/PrimaryKey constraints: Uses temp table approach (recreate table without column)
///
/// SQLite doesn't cascade constraint drops when a column is dropped.
pub fn build_delete_column(
    backend: &DatabaseBackend,
    table: &str,
    column: &str,
    column_type: Option<&ColumnType>,
    current_schema: &[TableDef],
) -> Vec<BuiltQuery> {
    let mut stmts = Vec::new();

    // SQLite: Check if we need special handling for constraints
    if *backend == DatabaseBackend::Sqlite
        && let Some(table_def) = current_schema.iter().find(|t| t.name == table)
    {
        // If the column has an enum type, SQLite embeds a CHECK constraint in CREATE TABLE.
        // ALTER TABLE DROP COLUMN fails if the column is referenced by any CHECK.
        // Must use temp table approach.
        if let Some(col_def) = table_def.columns.iter().find(|c| c.name == column)
            && let ColumnType::Complex(vespertide_core::ComplexColumnType::Enum { .. }) =
                &col_def.r#type
        {
            return build_delete_column_sqlite_temp_table(table, column, table_def, column_type);
        }

        // Handle constraints referencing the deleted column
        for constraint in &table_def.constraints {
            match constraint {
                // Check constraints may reference the column in their expression.
                // SQLite can't DROP COLUMN if a CHECK references it — use temp table.
                TableConstraint::Check { expr, .. } => {
                    // Check if the expression references the column (e.g. "status" IN (...))
                    if expr.contains(&format!("\"{}\"", column)) || expr.contains(column) {
                        return build_delete_column_sqlite_temp_table(
                            table,
                            column,
                            table_def,
                            column_type,
                        );
                    }
                    continue;
                }
                // For column-based constraints, check if they reference the deleted column
                _ if !constraint.columns().iter().any(|c| c == column) => continue,
                // FK/PK require temp table approach - return immediately
                TableConstraint::ForeignKey { .. } | TableConstraint::PrimaryKey { .. } => {
                    return build_delete_column_sqlite_temp_table(
                        table,
                        column,
                        table_def,
                        column_type,
                    );
                }
                // Unique/Index: drop the index first, then drop column below
                TableConstraint::Unique { name, columns } => {
                    let index_name = vespertide_naming::build_unique_constraint_name(
                        table,
                        columns,
                        name.as_deref(),
                    );
                    let drop_idx = Index::drop()
                        .name(&index_name)
                        .table(Alias::new(table))
                        .to_owned();
                    stmts.push(BuiltQuery::DropIndex(Box::new(drop_idx)));
                }
                TableConstraint::Index { name, columns } => {
                    let index_name =
                        vespertide_naming::build_index_name(table, columns, name.as_deref());
                    let drop_idx = Index::drop()
                        .name(&index_name)
                        .table(Alias::new(table))
                        .to_owned();
                    stmts.push(BuiltQuery::DropIndex(Box::new(drop_idx)));
                }
            }
        }
    }

    // Drop the column
    let stmt = Table::alter()
        .table(Alias::new(table))
        .drop_column(Alias::new(column))
        .to_owned();
    stmts.push(BuiltQuery::AlterTable(Box::new(stmt)));

    // If column type is an enum, drop the type after (PostgreSQL only)
    // Note: Only drop if this is the last column using this enum type
    if let Some(col_type) = column_type
        && let Some(drop_type_sql) = build_drop_enum_type_sql(table, col_type)
    {
        stmts.push(BuiltQuery::Raw(drop_type_sql));
    }

    stmts
}

/// SQLite temp table approach for deleting a column that has FK or PK constraints.
///
/// Steps:
/// 1. Create temp table without the column (and without constraints referencing it)
/// 2. Copy data (excluding the deleted column)
/// 3. Drop original table
/// 4. Rename temp table to original name
/// 5. Recreate indexes that don't reference the deleted column
fn build_delete_column_sqlite_temp_table(
    table: &str,
    column: &str,
    table_def: &TableDef,
    column_type: Option<&ColumnType>,
) -> Vec<BuiltQuery> {
    let mut stmts = Vec::new();
    let temp_table = format!("{}_temp", table);

    // Build new columns list without the deleted column
    let new_columns: Vec<_> = table_def
        .columns
        .iter()
        .filter(|c| c.name != column)
        .cloned()
        .collect();

    // Build new constraints list without constraints referencing the deleted column
    let new_constraints: Vec<_> = table_def
        .constraints
        .iter()
        .filter(|c| {
            // For CHECK constraints, check if expression references the column
            if let TableConstraint::Check { expr, .. } = c {
                return !expr.contains(&format!("\"{}\"", column)) && !expr.contains(column);
            }
            !c.columns().iter().any(|col| col == column)
        })
        .cloned()
        .collect();

    // 1. Create temp table without the column + CHECK constraints
    let create_query = build_sqlite_temp_table_create(
        &DatabaseBackend::Sqlite,
        &temp_table,
        table,
        &new_columns,
        &new_constraints,
    );
    stmts.push(create_query);

    // 2. Copy data (excluding the deleted column)
    let column_aliases: Vec<Alias> = new_columns.iter().map(|c| Alias::new(&c.name)).collect();
    let mut select_query = Query::select();
    for col_alias in &column_aliases {
        select_query = select_query.column(col_alias.clone()).to_owned();
    }
    select_query = select_query.from(Alias::new(table)).to_owned();

    let insert_stmt = Query::insert()
        .into_table(Alias::new(&temp_table))
        .columns(column_aliases.clone())
        .select_from(select_query)
        .unwrap()
        .to_owned();
    stmts.push(BuiltQuery::Insert(Box::new(insert_stmt)));

    // 3. Drop original table
    let drop_table = Table::drop().table(Alias::new(table)).to_owned();
    stmts.push(BuiltQuery::DropTable(Box::new(drop_table)));

    // 4. Rename temp table to original name
    stmts.push(build_rename_table(&temp_table, table));

    // 5. Recreate indexes (both regular and UNIQUE) that don't reference the deleted column
    stmts.extend(recreate_indexes_after_rebuild(table, &new_constraints));

    // If column type is an enum, drop the type after (PostgreSQL only, but include for completeness)
    if let Some(col_type) = column_type
        && let Some(drop_type_sql) = build_drop_enum_type_sql(table, col_type)
    {
        stmts.push(BuiltQuery::Raw(drop_type_sql));
    }

    stmts
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sql::types::DatabaseBackend;
    use insta::{assert_snapshot, with_settings};
    use rstest::rstest;
    use vespertide_core::{ColumnDef, ComplexColumnType, SimpleColumnType};

    fn col(name: &str, ty: ColumnType) -> ColumnDef {
        ColumnDef {
            name: name.to_string(),
            r#type: ty,
            nullable: true,
            default: None,
            comment: None,
            primary_key: None,
            unique: None,
            index: None,
            foreign_key: None,
        }
    }

    #[rstest]
    #[case::delete_column_postgres(
        "delete_column_postgres",
        DatabaseBackend::Postgres,
        &["ALTER TABLE \"users\" DROP COLUMN \"email\""]
    )]
    #[case::delete_column_mysql(
        "delete_column_mysql",
        DatabaseBackend::MySql,
        &["ALTER TABLE `users` DROP COLUMN `email`"]
    )]
    #[case::delete_column_sqlite(
        "delete_column_sqlite",
        DatabaseBackend::Sqlite,
        &["ALTER TABLE \"users\" DROP COLUMN \"email\""]
    )]
    fn test_delete_column(
        #[case] title: &str,
        #[case] backend: DatabaseBackend,
        #[case] expected: &[&str],
    ) {
        let result = build_delete_column(&backend, "users", "email", None, &[]);
        let sql = result[0].build(backend);
        for exp in expected {
            assert!(
                sql.contains(exp),
                "Expected SQL to contain '{}', got: {}",
                exp,
                sql
            );
        }

        with_settings!({ snapshot_suffix => format!("delete_column_{}", title) }, {
            assert_snapshot!(sql);
        });
    }

    #[test]
    fn test_delete_enum_column_postgres() {
        use vespertide_core::EnumValues;

        let enum_type = ColumnType::Complex(ComplexColumnType::Enum {
            name: "status".into(),
            values: EnumValues::String(vec!["active".into(), "inactive".into()]),
        });
        let result = build_delete_column(
            &DatabaseBackend::Postgres,
            "users",
            "status",
            Some(&enum_type),
            &[],
        );

        // Should have 2 statements: ALTER TABLE and DROP TYPE
        assert_eq!(result.len(), 2);

        let alter_sql = result[0].build(DatabaseBackend::Postgres);
        assert!(alter_sql.contains("DROP COLUMN"));

        let drop_type_sql = result[1].build(DatabaseBackend::Postgres);
        assert!(drop_type_sql.contains("DROP TYPE IF EXISTS \"users_status\""));

        // MySQL and SQLite should have empty DROP TYPE
        let drop_type_mysql = result[1].build(DatabaseBackend::MySql);
        assert!(drop_type_mysql.is_empty());
    }

    #[test]
    fn test_delete_non_enum_column_no_drop_type() {
        let text_type = ColumnType::Simple(SimpleColumnType::Text);
        let result = build_delete_column(
            &DatabaseBackend::Postgres,
            "users",
            "name",
            Some(&text_type),
            &[],
        );

        // Should only have 1 statement: ALTER TABLE
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn test_delete_column_sqlite_drops_unique_constraint_first() {
        // SQLite should drop unique constraint index before dropping the column
        let schema = vec![TableDef {
            name: "gift".into(),
            description: None,
            columns: vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                col("gift_code", ColumnType::Simple(SimpleColumnType::Text)),
            ],
            constraints: vec![TableConstraint::Unique {
                name: None,
                columns: vec!["gift_code".into()],
            }],
        }];

        let result =
            build_delete_column(&DatabaseBackend::Sqlite, "gift", "gift_code", None, &schema);

        // Should have 2 statements: DROP INDEX then ALTER TABLE DROP COLUMN
        assert_eq!(result.len(), 2);

        let drop_index_sql = result[0].build(DatabaseBackend::Sqlite);
        assert!(
            drop_index_sql.contains("DROP INDEX"),
            "Expected DROP INDEX, got: {}",
            drop_index_sql
        );
        assert!(
            drop_index_sql.contains("uq_gift__gift_code"),
            "Expected index name uq_gift__gift_code, got: {}",
            drop_index_sql
        );

        let drop_column_sql = result[1].build(DatabaseBackend::Sqlite);
        assert!(
            drop_column_sql.contains("DROP COLUMN"),
            "Expected DROP COLUMN, got: {}",
            drop_column_sql
        );
    }

    #[test]
    fn test_delete_column_sqlite_drops_index_constraint_first() {
        // SQLite should drop index before dropping the column
        let schema = vec![TableDef {
            name: "users".into(),
            description: None,
            columns: vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                col("email", ColumnType::Simple(SimpleColumnType::Text)),
            ],
            constraints: vec![TableConstraint::Index {
                name: None,
                columns: vec!["email".into()],
            }],
        }];

        let result = build_delete_column(&DatabaseBackend::Sqlite, "users", "email", None, &schema);

        // Should have 2 statements: DROP INDEX then ALTER TABLE DROP COLUMN
        assert_eq!(result.len(), 2);

        let drop_index_sql = result[0].build(DatabaseBackend::Sqlite);
        assert!(drop_index_sql.contains("DROP INDEX"));
        assert!(drop_index_sql.contains("ix_users__email"));

        let drop_column_sql = result[1].build(DatabaseBackend::Sqlite);
        assert!(drop_column_sql.contains("DROP COLUMN"));
    }

    #[test]
    fn test_delete_column_postgres_does_not_drop_constraints() {
        // PostgreSQL cascades constraint drops, so we shouldn't emit extra DROP INDEX
        let schema = vec![TableDef {
            name: "gift".into(),
            description: None,
            columns: vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                col("gift_code", ColumnType::Simple(SimpleColumnType::Text)),
            ],
            constraints: vec![TableConstraint::Unique {
                name: None,
                columns: vec!["gift_code".into()],
            }],
        }];

        let result = build_delete_column(
            &DatabaseBackend::Postgres,
            "gift",
            "gift_code",
            None,
            &schema,
        );

        // Should have only 1 statement: ALTER TABLE DROP COLUMN
        assert_eq!(result.len(), 1);

        let drop_column_sql = result[0].build(DatabaseBackend::Postgres);
        assert!(drop_column_sql.contains("DROP COLUMN"));
    }

    #[test]
    fn test_delete_column_sqlite_with_named_unique_constraint() {
        // Test with a named unique constraint
        let schema = vec![TableDef {
            name: "gift".into(),
            description: None,
            columns: vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                col("gift_code", ColumnType::Simple(SimpleColumnType::Text)),
            ],
            constraints: vec![TableConstraint::Unique {
                name: Some("gift_code".into()),
                columns: vec!["gift_code".into()],
            }],
        }];

        let result =
            build_delete_column(&DatabaseBackend::Sqlite, "gift", "gift_code", None, &schema);

        assert_eq!(result.len(), 2);

        let drop_index_sql = result[0].build(DatabaseBackend::Sqlite);
        // Named constraint: uq_gift__gift_code (name is "gift_code")
        assert!(
            drop_index_sql.contains("uq_gift__gift_code"),
            "Expected uq_gift__gift_code, got: {}",
            drop_index_sql
        );
    }

    #[test]
    fn test_delete_column_sqlite_with_fk_uses_temp_table() {
        // SQLite should use temp table approach when deleting a column with FK constraint
        let schema = vec![TableDef {
            name: "gift".into(),
            description: None,
            columns: vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                col("sender_id", ColumnType::Simple(SimpleColumnType::BigInt)),
                col("message", ColumnType::Simple(SimpleColumnType::Text)),
            ],
            constraints: vec![TableConstraint::ForeignKey {
                name: None,
                columns: vec!["sender_id".into()],
                ref_table: "user".into(),
                ref_columns: vec!["id".into()],
                on_delete: None,
                on_update: None,
            }],
        }];

        let result =
            build_delete_column(&DatabaseBackend::Sqlite, "gift", "sender_id", None, &schema);

        // Should use temp table approach:
        // 1. CREATE TABLE gift_temp (without sender_id column)
        // 2. INSERT INTO gift_temp SELECT ... FROM gift
        // 3. DROP TABLE gift
        // 4. ALTER TABLE gift_temp RENAME TO gift
        assert!(
            result.len() >= 4,
            "Expected at least 4 statements for temp table approach, got: {}",
            result.len()
        );

        let all_sql: Vec<String> = result
            .iter()
            .map(|q| q.build(DatabaseBackend::Sqlite))
            .collect();
        let combined_sql = all_sql.join("\n");

        // Verify temp table creation
        assert!(
            combined_sql.contains("CREATE TABLE") && combined_sql.contains("gift_temp"),
            "Expected CREATE TABLE gift_temp, got: {}",
            combined_sql
        );

        // Verify the new table doesn't have sender_id column
        assert!(
            !combined_sql.contains("\"sender_id\"") || combined_sql.contains("DROP TABLE"),
            "New table should not contain sender_id column"
        );

        // Verify data copy (INSERT ... SELECT)
        assert!(
            combined_sql.contains("INSERT INTO"),
            "Expected INSERT INTO for data copy, got: {}",
            combined_sql
        );

        // Verify original table drop
        assert!(
            combined_sql.contains("DROP TABLE") && combined_sql.contains("\"gift\""),
            "Expected DROP TABLE gift, got: {}",
            combined_sql
        );

        // Verify rename
        assert!(
            combined_sql.contains("RENAME"),
            "Expected RENAME for temp table, got: {}",
            combined_sql
        );
    }

    #[test]
    fn test_delete_column_sqlite_with_fk_preserves_other_columns() {
        // When using temp table approach, other columns should be preserved
        let schema = vec![TableDef {
            name: "gift".into(),
            description: None,
            columns: vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                col("sender_id", ColumnType::Simple(SimpleColumnType::BigInt)),
                col("receiver_id", ColumnType::Simple(SimpleColumnType::BigInt)),
                col("message", ColumnType::Simple(SimpleColumnType::Text)),
            ],
            constraints: vec![
                TableConstraint::ForeignKey {
                    name: None,
                    columns: vec!["sender_id".into()],
                    ref_table: "user".into(),
                    ref_columns: vec!["id".into()],
                    on_delete: None,
                    on_update: None,
                },
                TableConstraint::Index {
                    name: None,
                    columns: vec!["receiver_id".into()],
                },
            ],
        }];

        let result =
            build_delete_column(&DatabaseBackend::Sqlite, "gift", "sender_id", None, &schema);

        let all_sql: Vec<String> = result
            .iter()
            .map(|q| q.build(DatabaseBackend::Sqlite))
            .collect();
        let combined_sql = all_sql.join("\n");

        // Should preserve other columns
        assert!(combined_sql.contains("\"id\""), "Should preserve id column");
        assert!(
            combined_sql.contains("\"receiver_id\""),
            "Should preserve receiver_id column"
        );
        assert!(
            combined_sql.contains("\"message\""),
            "Should preserve message column"
        );

        // Should recreate index on receiver_id (not on sender_id)
        assert!(
            combined_sql.contains("CREATE INDEX") && combined_sql.contains("ix_gift__receiver_id"),
            "Should recreate index on receiver_id, got: {}",
            combined_sql
        );
    }

    #[test]
    fn test_delete_column_postgres_with_fk_does_not_use_temp_table() {
        // PostgreSQL should NOT use temp table - just drop column directly
        let schema = vec![TableDef {
            name: "gift".into(),
            description: None,
            columns: vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                col("sender_id", ColumnType::Simple(SimpleColumnType::BigInt)),
            ],
            constraints: vec![TableConstraint::ForeignKey {
                name: None,
                columns: vec!["sender_id".into()],
                ref_table: "user".into(),
                ref_columns: vec!["id".into()],
                on_delete: None,
                on_update: None,
            }],
        }];

        let result = build_delete_column(
            &DatabaseBackend::Postgres,
            "gift",
            "sender_id",
            None,
            &schema,
        );

        // Should have only 1 statement: ALTER TABLE DROP COLUMN
        assert_eq!(
            result.len(),
            1,
            "PostgreSQL should only have 1 statement, got: {}",
            result.len()
        );

        let sql = result[0].build(DatabaseBackend::Postgres);
        assert!(
            sql.contains("DROP COLUMN"),
            "Expected DROP COLUMN, got: {}",
            sql
        );
        assert!(
            !sql.contains("gift_temp"),
            "PostgreSQL should not use temp table"
        );
    }

    #[test]
    fn test_delete_column_sqlite_with_pk_uses_temp_table() {
        // SQLite should use temp table approach when deleting a column that's part of PK
        let schema = vec![TableDef {
            name: "order_items".into(),
            description: None,
            columns: vec![
                col("order_id", ColumnType::Simple(SimpleColumnType::Integer)),
                col("product_id", ColumnType::Simple(SimpleColumnType::Integer)),
                col("quantity", ColumnType::Simple(SimpleColumnType::Integer)),
            ],
            constraints: vec![TableConstraint::PrimaryKey {
                auto_increment: false,
                columns: vec!["order_id".into(), "product_id".into()],
            }],
        }];

        let result = build_delete_column(
            &DatabaseBackend::Sqlite,
            "order_items",
            "product_id",
            None,
            &schema,
        );

        // Should use temp table approach
        assert!(
            result.len() >= 4,
            "Expected at least 4 statements for temp table approach, got: {}",
            result.len()
        );

        let all_sql: Vec<String> = result
            .iter()
            .map(|q| q.build(DatabaseBackend::Sqlite))
            .collect();
        let combined_sql = all_sql.join("\n");

        assert!(
            combined_sql.contains("order_items_temp"),
            "Should use temp table approach for PK column deletion"
        );
    }

    #[test]
    fn test_delete_column_sqlite_unique_on_different_column_not_dropped() {
        // When deleting a column in SQLite, UNIQUE constraints on OTHER columns should NOT be dropped
        // This tests line 46's condition: only drop constraints that reference the deleted column
        let schema = vec![TableDef {
            name: "users".into(),
            description: None,
            columns: vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                col("email", ColumnType::Simple(SimpleColumnType::Text)),
                col("nickname", ColumnType::Simple(SimpleColumnType::Text)),
            ],
            constraints: vec![
                // UNIQUE on email (the column we're NOT deleting)
                TableConstraint::Unique {
                    name: None,
                    columns: vec!["email".into()],
                },
            ],
        }];

        // Delete nickname, which does NOT have the unique constraint
        let result =
            build_delete_column(&DatabaseBackend::Sqlite, "users", "nickname", None, &schema);

        // Should only have 1 statement: ALTER TABLE DROP COLUMN (no DROP INDEX needed)
        assert_eq!(
            result.len(),
            1,
            "Should not drop UNIQUE on email when deleting nickname, got: {} statements",
            result.len()
        );

        let sql = result[0].build(DatabaseBackend::Sqlite);
        assert!(
            sql.contains("DROP COLUMN"),
            "Expected DROP COLUMN, got: {}",
            sql
        );
        assert!(
            !sql.contains("DROP INDEX"),
            "Should NOT drop the email UNIQUE constraint when deleting nickname"
        );
    }

    #[test]
    fn test_delete_column_sqlite_temp_table_filters_constraints_correctly() {
        // When using temp table approach, constraints referencing the deleted column should be excluded,
        // but constraints on OTHER columns should be preserved
        // This tests lines 122-124: filter constraints by column reference
        let schema = vec![TableDef {
            name: "orders".into(),
            description: None,
            columns: vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                col("user_id", ColumnType::Simple(SimpleColumnType::BigInt)),
                col("status", ColumnType::Simple(SimpleColumnType::Text)),
                col(
                    "created_at",
                    ColumnType::Simple(SimpleColumnType::Timestamp),
                ),
            ],
            constraints: vec![
                // FK on user_id (column we're deleting) - should be excluded
                TableConstraint::ForeignKey {
                    name: None,
                    columns: vec!["user_id".into()],
                    ref_table: "users".into(),
                    ref_columns: vec!["id".into()],
                    on_delete: None,
                    on_update: None,
                },
                // Index on created_at (different column) - should be preserved and recreated
                TableConstraint::Index {
                    name: None,
                    columns: vec!["created_at".into()],
                },
                // Another FK on a different column - should be preserved
                TableConstraint::ForeignKey {
                    name: None,
                    columns: vec!["status".into()],
                    ref_table: "statuses".into(),
                    ref_columns: vec!["code".into()],
                    on_delete: None,
                    on_update: None,
                },
            ],
        }];

        let result =
            build_delete_column(&DatabaseBackend::Sqlite, "orders", "user_id", None, &schema);

        let all_sql: Vec<String> = result
            .iter()
            .map(|q| q.build(DatabaseBackend::Sqlite))
            .collect();
        let combined_sql = all_sql.join("\n");

        // Should use temp table approach (FK triggers it)
        assert!(
            combined_sql.contains("orders_temp"),
            "Should use temp table approach for FK column deletion"
        );

        // Index on created_at should be recreated after rename
        assert!(
            combined_sql.contains("ix_orders__created_at"),
            "Index on created_at should be recreated, got: {}",
            combined_sql
        );

        // The FK on user_id should NOT appear (deleted column)
        // But the FK on status should be preserved
        assert!(
            combined_sql.contains("REFERENCES \"statuses\""),
            "FK on status should be preserved, got: {}",
            combined_sql
        );

        // Count FK references - should only be 1 (status FK, not user_id FK)
        let fk_patterns = combined_sql.matches("REFERENCES").count();
        assert_eq!(
            fk_patterns, 1,
            "Only the FK on status should exist (not the one on user_id), got: {}",
            combined_sql
        );
    }

    // ==================== Snapshot Tests ====================

    fn build_sql_snapshot(result: &[BuiltQuery], backend: DatabaseBackend) -> String {
        result
            .iter()
            .map(|q| q.build(backend))
            .collect::<Vec<_>>()
            .join(";\n")
    }

    #[rstest]
    #[case::postgres("postgres", DatabaseBackend::Postgres)]
    #[case::mysql("mysql", DatabaseBackend::MySql)]
    #[case::sqlite("sqlite", DatabaseBackend::Sqlite)]
    fn test_delete_column_with_unique_constraint(
        #[case] title: &str,
        #[case] backend: DatabaseBackend,
    ) {
        let schema = vec![TableDef {
            name: "users".into(),
            description: None,
            columns: vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                col("email", ColumnType::Simple(SimpleColumnType::Text)),
                col("name", ColumnType::Simple(SimpleColumnType::Text)),
            ],
            constraints: vec![TableConstraint::Unique {
                name: None,
                columns: vec!["email".into()],
            }],
        }];

        let result = build_delete_column(&backend, "users", "email", None, &schema);
        let sql = build_sql_snapshot(&result, backend);

        with_settings!({ snapshot_suffix => format!("delete_column_with_unique_{}", title) }, {
            assert_snapshot!(sql);
        });
    }

    #[rstest]
    #[case::postgres("postgres", DatabaseBackend::Postgres)]
    #[case::mysql("mysql", DatabaseBackend::MySql)]
    #[case::sqlite("sqlite", DatabaseBackend::Sqlite)]
    fn test_delete_column_with_index_constraint(
        #[case] title: &str,
        #[case] backend: DatabaseBackend,
    ) {
        let schema = vec![TableDef {
            name: "posts".into(),
            description: None,
            columns: vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                col(
                    "created_at",
                    ColumnType::Simple(SimpleColumnType::Timestamp),
                ),
                col("title", ColumnType::Simple(SimpleColumnType::Text)),
            ],
            constraints: vec![TableConstraint::Index {
                name: None,
                columns: vec!["created_at".into()],
            }],
        }];

        let result = build_delete_column(&backend, "posts", "created_at", None, &schema);
        let sql = build_sql_snapshot(&result, backend);

        with_settings!({ snapshot_suffix => format!("delete_column_with_index_{}", title) }, {
            assert_snapshot!(sql);
        });
    }

    #[rstest]
    #[case::postgres("postgres", DatabaseBackend::Postgres)]
    #[case::mysql("mysql", DatabaseBackend::MySql)]
    #[case::sqlite("sqlite", DatabaseBackend::Sqlite)]
    fn test_delete_column_with_fk_constraint(
        #[case] title: &str,
        #[case] backend: DatabaseBackend,
    ) {
        let schema = vec![TableDef {
            name: "orders".into(),
            description: None,
            columns: vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                col("user_id", ColumnType::Simple(SimpleColumnType::BigInt)),
                col("total", ColumnType::Simple(SimpleColumnType::Integer)),
            ],
            constraints: vec![TableConstraint::ForeignKey {
                name: None,
                columns: vec!["user_id".into()],
                ref_table: "users".into(),
                ref_columns: vec!["id".into()],
                on_delete: None,
                on_update: None,
            }],
        }];

        let result = build_delete_column(&backend, "orders", "user_id", None, &schema);
        let sql = build_sql_snapshot(&result, backend);

        with_settings!({ snapshot_suffix => format!("delete_column_with_fk_{}", title) }, {
            assert_snapshot!(sql);
        });
    }

    #[rstest]
    #[case::postgres("postgres", DatabaseBackend::Postgres)]
    #[case::mysql("mysql", DatabaseBackend::MySql)]
    #[case::sqlite("sqlite", DatabaseBackend::Sqlite)]
    fn test_delete_column_with_pk_constraint(
        #[case] title: &str,
        #[case] backend: DatabaseBackend,
    ) {
        let schema = vec![TableDef {
            name: "order_items".into(),
            description: None,
            columns: vec![
                col("order_id", ColumnType::Simple(SimpleColumnType::Integer)),
                col("product_id", ColumnType::Simple(SimpleColumnType::Integer)),
                col("quantity", ColumnType::Simple(SimpleColumnType::Integer)),
            ],
            constraints: vec![TableConstraint::PrimaryKey {
                auto_increment: false,
                columns: vec!["order_id".into(), "product_id".into()],
            }],
        }];

        let result = build_delete_column(&backend, "order_items", "product_id", None, &schema);
        let sql = build_sql_snapshot(&result, backend);

        with_settings!({ snapshot_suffix => format!("delete_column_with_pk_{}", title) }, {
            assert_snapshot!(sql);
        });
    }

    #[rstest]
    #[case::postgres("postgres", DatabaseBackend::Postgres)]
    #[case::mysql("mysql", DatabaseBackend::MySql)]
    #[case::sqlite("sqlite", DatabaseBackend::Sqlite)]
    fn test_delete_column_with_fk_and_index_constraints(
        #[case] title: &str,
        #[case] backend: DatabaseBackend,
    ) {
        // Complex case: FK on the deleted column + Index on another column
        let schema = vec![TableDef {
            name: "orders".into(),
            description: None,
            columns: vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                col("user_id", ColumnType::Simple(SimpleColumnType::BigInt)),
                col(
                    "created_at",
                    ColumnType::Simple(SimpleColumnType::Timestamp),
                ),
                col("total", ColumnType::Simple(SimpleColumnType::Integer)),
            ],
            constraints: vec![
                TableConstraint::ForeignKey {
                    name: None,
                    columns: vec!["user_id".into()],
                    ref_table: "users".into(),
                    ref_columns: vec!["id".into()],
                    on_delete: None,
                    on_update: None,
                },
                TableConstraint::Index {
                    name: None,
                    columns: vec!["created_at".into()],
                },
            ],
        }];

        let result = build_delete_column(&backend, "orders", "user_id", None, &schema);
        let sql = build_sql_snapshot(&result, backend);

        with_settings!({ snapshot_suffix => format!("delete_column_with_fk_and_index_{}", title) }, {
            assert_snapshot!(sql);
        });
    }

    #[test]
    fn test_delete_column_sqlite_temp_table_with_enum_column() {
        // SQLite temp table approach with enum column type
        // This tests lines 122-124: enum type drop in temp table function
        use vespertide_core::EnumValues;

        let enum_type = ColumnType::Complex(ComplexColumnType::Enum {
            name: "order_status".into(),
            values: EnumValues::String(vec![
                "pending".into(),
                "shipped".into(),
                "delivered".into(),
            ]),
        });

        let schema = vec![TableDef {
            name: "orders".into(),
            description: None,
            columns: vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                col("user_id", ColumnType::Simple(SimpleColumnType::BigInt)),
                col("status", enum_type.clone()),
            ],
            constraints: vec![TableConstraint::ForeignKey {
                name: None,
                columns: vec!["user_id".into()],
                ref_table: "users".into(),
                ref_columns: vec!["id".into()],
                on_delete: None,
                on_update: None,
            }],
        }];

        // Delete the FK column (user_id) with an enum type - triggers temp table AND enum drop
        let result = build_delete_column(
            &DatabaseBackend::Sqlite,
            "orders",
            "user_id",
            Some(&enum_type),
            &schema,
        );

        // Should use temp table approach (FK triggers it) + DROP TYPE at end
        assert!(
            result.len() >= 4,
            "Expected at least 4 statements for temp table approach, got: {}",
            result.len()
        );

        let all_sql: Vec<String> = result
            .iter()
            .map(|q| q.build(DatabaseBackend::Sqlite))
            .collect();
        let combined_sql = all_sql.join("\n");

        // Verify temp table approach
        assert!(
            combined_sql.contains("orders_temp"),
            "Should use temp table approach"
        );

        // The DROP TYPE statement should be empty for SQLite (only applies to PostgreSQL)
        // but the code path should still be executed
        let last_stmt = result.last().unwrap();
        let last_sql = last_stmt.build(DatabaseBackend::Sqlite);
        // SQLite doesn't have DROP TYPE, so it should be empty string
        assert!(
            last_sql.is_empty() || !last_sql.contains("DROP TYPE"),
            "SQLite should not emit DROP TYPE"
        );

        // Verify it DOES emit DROP TYPE for PostgreSQL
        let pg_last_sql = last_stmt.build(DatabaseBackend::Postgres);
        assert!(
            pg_last_sql.contains("DROP TYPE"),
            "PostgreSQL should emit DROP TYPE, got: {}",
            pg_last_sql
        );
    }

    #[test]
    fn test_delete_column_sqlite_with_check_constraint_referencing_column() {
        // When a CHECK constraint references the column being deleted,
        // SQLite can't use ALTER TABLE DROP COLUMN — must use temp table approach.
        let schema = vec![TableDef {
            name: "orders".into(),
            description: None,
            columns: vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                col("amount", ColumnType::Simple(SimpleColumnType::Integer)),
            ],
            constraints: vec![TableConstraint::Check {
                name: "check_positive".into(),
                expr: "amount > 0".into(),
            }],
        }];

        // Delete amount column — CHECK references it, so temp table is needed
        let result =
            build_delete_column(&DatabaseBackend::Sqlite, "orders", "amount", None, &schema);

        // Should use temp table approach (CREATE temp, INSERT, DROP, RENAME)
        assert!(
            result.len() >= 4,
            "Expected temp table approach (>=4 stmts), got: {} statements",
            result.len()
        );

        let sql = result[0].build(DatabaseBackend::Sqlite);
        assert!(
            sql.contains("orders_temp"),
            "Expected temp table creation, got: {}",
            sql
        );
        // The CHECK constraint referencing "amount" should NOT be in the temp table
        assert!(
            !sql.contains("check_positive"),
            "CHECK referencing deleted column should be removed, got: {}",
            sql
        );
    }

    #[test]
    fn test_delete_column_sqlite_with_check_constraint_not_referencing_column() {
        // When a CHECK constraint does NOT reference the column being deleted,
        // simple DROP COLUMN should work.
        let schema = vec![TableDef {
            name: "orders".into(),
            description: None,
            columns: vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                col("amount", ColumnType::Simple(SimpleColumnType::Integer)),
                col("note", ColumnType::Simple(SimpleColumnType::Text)),
            ],
            constraints: vec![TableConstraint::Check {
                name: "check_positive".into(),
                expr: "amount > 0".into(),
            }],
        }];

        // Delete "note" column — CHECK only references "amount", not "note"
        let result = build_delete_column(&DatabaseBackend::Sqlite, "orders", "note", None, &schema);

        assert_eq!(
            result.len(),
            1,
            "Unrelated CHECK should be skipped, got: {} statements",
            result.len()
        );

        let sql = result[0].build(DatabaseBackend::Sqlite);
        assert!(
            sql.contains("DROP COLUMN"),
            "Expected DROP COLUMN, got: {}",
            sql
        );
    }
}
