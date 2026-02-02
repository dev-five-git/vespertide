use sea_query::{Alias, ForeignKey, Index, Table, TableCreateStatement};

use vespertide_core::{ColumnDef, ColumnType, ComplexColumnType, TableConstraint};

use super::helpers::{
    build_create_enum_type_sql, build_schema_statement, build_sea_column_def_with_table,
    collect_sqlite_enum_check_clauses, to_sea_fk_action,
};
use super::types::{BuiltQuery, DatabaseBackend, RawSql};
use crate::error::QueryError;

pub(crate) fn build_create_table_for_backend(
    backend: &DatabaseBackend,
    table: &str,
    columns: &[ColumnDef],
    constraints: &[TableConstraint],
) -> TableCreateStatement {
    let mut stmt = Table::create().table(Alias::new(table)).to_owned();

    let has_table_primary_key = constraints
        .iter()
        .any(|c| matches!(c, TableConstraint::PrimaryKey { .. }));

    // Extract auto_increment columns from constraints
    let auto_increment_columns: std::collections::HashSet<&str> = constraints
        .iter()
        .filter_map(|c| {
            if let TableConstraint::PrimaryKey {
                columns: pk_cols,
                auto_increment: true,
            } = c
            {
                Some(pk_cols.iter().map(|s| s.as_str()).collect::<Vec<_>>())
            } else {
                None
            }
        })
        .flatten()
        .collect();

    // Add columns
    for column in columns {
        let mut col = build_sea_column_def_with_table(backend, table, column);

        // Check for inline primary key
        if column.primary_key.is_some() && !has_table_primary_key {
            col.primary_key();
        }

        // Apply auto_increment if this column is in the auto_increment primary key
        // and the column type supports it (integer types only).
        // After ModifyColumnType, the PK may still have auto_increment: true but the
        // column type may have changed to a non-integer type (e.g. varchar).
        if auto_increment_columns.contains(column.name.as_str())
            && column.r#type.supports_auto_increment()
        {
            // For SQLite, AUTOINCREMENT requires inline PRIMARY KEY (INTEGER PRIMARY KEY AUTOINCREMENT)
            // So we must call primary_key() on the column even if there's a table-level PRIMARY KEY
            if matches!(backend, DatabaseBackend::Sqlite) {
                col.primary_key();
            }
            col.auto_increment();
        }

        // NOTE: We do NOT add inline unique constraints here.
        // All unique constraints are handled as separate CREATE UNIQUE INDEX statements
        // so they have proper names and can be dropped later.

        stmt = stmt.col(col).to_owned();
    }

    // Add table-level constraints
    for constraint in constraints {
        match constraint {
            TableConstraint::PrimaryKey {
                columns: pk_cols,
                auto_increment,
            } => {
                // For SQLite with auto_increment, skip table-level PRIMARY KEY
                // because AUTOINCREMENT requires inline PRIMARY KEY on the column.
                // But only if the PK column actually supports auto_increment (integer types).
                if matches!(backend, DatabaseBackend::Sqlite)
                    && *auto_increment
                    && pk_cols.iter().all(|col_name| {
                        columns
                            .iter()
                            .find(|c| c.name == *col_name)
                            .is_some_and(|c| c.r#type.supports_auto_increment())
                    })
                {
                    continue;
                }
                // Build primary key index
                let mut pk_idx = Index::create();
                for c in pk_cols {
                    pk_idx = pk_idx.col(Alias::new(c)).to_owned();
                }
                stmt = stmt.primary_key(&mut pk_idx).to_owned();
            }
            TableConstraint::Unique {
                name,
                columns: unique_cols,
            } => {
                // For MySQL, we can add unique index directly in CREATE TABLE
                // For Postgres and SQLite, we'll handle it separately in build_create_table
                if matches!(backend, DatabaseBackend::MySql) {
                    // Always generate a proper name: uq_{table}_{key} or uq_{table}_{columns}
                    let index_name = super::helpers::build_unique_constraint_name(
                        table,
                        unique_cols,
                        name.as_deref(),
                    );
                    let mut idx = Index::create().name(&index_name).unique().to_owned();
                    for col in unique_cols {
                        idx = idx.col(Alias::new(col)).to_owned();
                    }
                    stmt = stmt.index(&mut idx).to_owned();
                }
                // For Postgres and SQLite, unique constraints will be handled in build_create_table
                // as separate CREATE UNIQUE INDEX statements
            }
            TableConstraint::ForeignKey {
                name,
                columns: fk_cols,
                ref_table,
                ref_columns,
                on_delete,
                on_update,
            } => {
                // Always generate a proper name: fk_{table}_{key} or fk_{table}_{columns}
                let fk_name =
                    super::helpers::build_foreign_key_name(table, fk_cols, name.as_deref());
                let mut fk = ForeignKey::create().name(&fk_name).to_owned();
                fk = fk.from_tbl(Alias::new(table)).to_owned();
                for col in fk_cols {
                    fk = fk.from_col(Alias::new(col)).to_owned();
                }
                fk = fk.to_tbl(Alias::new(ref_table)).to_owned();
                for col in ref_columns {
                    fk = fk.to_col(Alias::new(col)).to_owned();
                }
                if let Some(action) = on_delete {
                    fk = fk.on_delete(to_sea_fk_action(action)).to_owned();
                }
                if let Some(action) = on_update {
                    fk = fk.on_update(to_sea_fk_action(action)).to_owned();
                }
                stmt = stmt.foreign_key(&mut fk).to_owned();
            }
            TableConstraint::Check { name, expr } => {
                // sea-query doesn't have direct CHECK constraint support in TableCreateStatement
                // This would need to be handled as raw SQL or post-creation ALTER
                let _ = (name, expr);
            }
            TableConstraint::Index { .. } => {
                // Indexes are added separately after CREATE TABLE as CREATE INDEX statements
                // They will be handled in build_create_table
            }
        }
    }

    stmt
}

pub fn build_create_table(
    backend: &DatabaseBackend,
    table: &str,
    columns: &[ColumnDef],
    constraints: &[TableConstraint],
) -> Result<Vec<BuiltQuery>, QueryError> {
    // Normalize the table to convert inline constraints to table-level
    // This ensures we don't have duplicate constraints if both inline and table-level are defined
    let table_def = vespertide_core::TableDef {
        description: None,
        name: table.to_string(),
        columns: columns.to_vec(),
        constraints: constraints.to_vec(),
    };
    let normalized = table_def
        .normalize()
        .map_err(|e| QueryError::Other(format!("Failed to normalize table '{}': {}", table, e)))?;

    // Use normalized columns and constraints for SQL generation
    let columns = &normalized.columns;
    let constraints = &normalized.constraints;

    let mut queries = Vec::new();

    // Create enum types first (PostgreSQL only)
    // Collect unique enum types to avoid duplicates
    let mut created_enums = std::collections::HashSet::new();
    for column in columns {
        if let ColumnType::Complex(ComplexColumnType::Enum { name, .. }) = &column.r#type
            && created_enums.insert(name.clone())
            && let Some(create_type_sql) = build_create_enum_type_sql(table, &column.r#type)
        {
            queries.push(BuiltQuery::Raw(create_type_sql));
        }
    }

    // Separate unique constraints for Postgres and SQLite (they need separate CREATE INDEX statements)
    // For MySQL, unique constraints are added directly in CREATE TABLE via build_create_table_for_backend
    let (table_constraints, unique_constraints): (Vec<&TableConstraint>, Vec<&TableConstraint>) =
        constraints
            .iter()
            .partition(|c| !matches!(c, TableConstraint::Unique { .. }));

    // Build CREATE TABLE
    // For MySQL, include unique constraints in CREATE TABLE
    // For Postgres and SQLite, exclude them (will be added as separate CREATE INDEX statements)
    let create_table_stmt = if matches!(backend, DatabaseBackend::MySql) {
        build_create_table_for_backend(backend, table, columns, constraints)
    } else {
        // Convert references to owned values for build_create_table_for_backend
        let table_constraints_owned: Vec<TableConstraint> =
            table_constraints.iter().cloned().cloned().collect();
        build_create_table_for_backend(backend, table, columns, &table_constraints_owned)
    };

    // For SQLite, add CHECK constraints for enum columns
    if matches!(backend, DatabaseBackend::Sqlite) {
        let enum_check_clauses = collect_sqlite_enum_check_clauses(table, columns);
        if !enum_check_clauses.is_empty() {
            // Embed CHECK constraints into CREATE TABLE statement
            let base_sql = build_schema_statement(&create_table_stmt, *backend);
            let mut modified_sql = base_sql;
            if let Some(pos) = modified_sql.rfind(')') {
                let check_sql = enum_check_clauses.join(", ");
                modified_sql.insert_str(pos, &format!(", {}", check_sql));
            }
            queries.push(BuiltQuery::Raw(RawSql::per_backend(
                modified_sql.clone(),
                modified_sql.clone(),
                modified_sql,
            )));
        } else {
            queries.push(BuiltQuery::CreateTable(Box::new(create_table_stmt)));
        }
    } else {
        queries.push(BuiltQuery::CreateTable(Box::new(create_table_stmt)));
    }

    // For Postgres and SQLite, add unique constraints as separate CREATE UNIQUE INDEX statements
    if matches!(backend, DatabaseBackend::Postgres | DatabaseBackend::Sqlite) {
        for constraint in unique_constraints {
            if let TableConstraint::Unique {
                name,
                columns: unique_cols,
            } = constraint
            {
                // Always generate a proper name: uq_{table}_{key} or uq_{table}_{columns}
                let index_name = super::helpers::build_unique_constraint_name(
                    table,
                    unique_cols,
                    name.as_deref(),
                );
                let mut idx = Index::create()
                    .table(Alias::new(table))
                    .name(&index_name)
                    .unique()
                    .to_owned();
                for col in unique_cols {
                    idx = idx.col(Alias::new(col)).to_owned();
                }
                queries.push(BuiltQuery::CreateIndex(Box::new(idx)));
            }
        }
    }

    // Add Index constraints as CREATE INDEX statements (for all backends)
    for constraint in constraints {
        if let TableConstraint::Index {
            name,
            columns: index_cols,
        } = constraint
        {
            // Always generate a proper name: ix_{table}_{key} or ix_{table}_{columns}
            let index_name = super::helpers::build_index_name(table, index_cols, name.as_deref());
            let mut idx = Index::create()
                .table(Alias::new(table))
                .name(&index_name)
                .to_owned();
            for col in index_cols {
                idx = idx.col(Alias::new(col)).to_owned();
            }
            queries.push(BuiltQuery::CreateIndex(Box::new(idx)));
        }
    }

    Ok(queries)
}

#[cfg(test)]
mod tests {
    use super::*;
    use insta::{assert_snapshot, with_settings};
    use rstest::rstest;
    use vespertide_core::{ColumnType, EnumValues, SimpleColumnType};

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
    #[case::create_table_postgres(
        "create_table_postgres",
        DatabaseBackend::Postgres,
        &["CREATE TABLE \"users\" ( \"id\" integer )"]
    )]
    #[case::create_table_mysql(
        "create_table_mysql",
        DatabaseBackend::MySql,
        &["CREATE TABLE `users` ( `id` int )"]
    )]
    #[case::create_table_sqlite(
        "create_table_sqlite",
        DatabaseBackend::Sqlite,
        &["CREATE TABLE \"users\" ( \"id\" integer )"]
    )]
    fn test_create_table(
        #[case] title: &str,
        #[case] backend: DatabaseBackend,
        #[case] expected: &[&str],
    ) {
        let result = build_create_table(
            &backend,
            "users",
            &[col("id", ColumnType::Simple(SimpleColumnType::Integer))],
            &[],
        )
        .unwrap();
        let sql = result
            .iter()
            .map(|q| q.build(backend))
            .collect::<Vec<String>>()
            .join("\n");
        for exp in expected {
            assert!(
                sql.contains(exp),
                "Expected SQL to contain '{}', got: {}",
                exp,
                sql
            );
        }

        with_settings!({ snapshot_suffix => format!("create_table_{}", title) }, {
            assert_snapshot!(sql);
        });
    }

    #[rstest]
    #[case::inline_unique_postgres(DatabaseBackend::Postgres)]
    #[case::inline_unique_mysql(DatabaseBackend::MySql)]
    #[case::inline_unique_sqlite(DatabaseBackend::Sqlite)]
    fn test_create_table_with_inline_unique(#[case] backend: DatabaseBackend) {
        // Test that inline unique constraint is converted to table-level during normalization.
        // build_create_table now normalizes the table, so inline unique becomes a CREATE UNIQUE INDEX.
        use vespertide_core::schema::str_or_bool::StrOrBoolOrArray;

        let mut email_col = col("email", ColumnType::Simple(SimpleColumnType::Text));
        email_col.unique = Some(StrOrBoolOrArray::Bool(true));

        let result = build_create_table(
            &backend,
            "users",
            &[
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                email_col,
            ],
            // No explicit table-level unique constraint passed, but normalize will create one from inline
            &[],
        )
        .unwrap();
        let sql = result
            .iter()
            .map(|q| q.build(backend))
            .collect::<Vec<String>>()
            .join("\n");

        // After normalization, inline unique should produce UNIQUE constraint in SQL
        assert!(
            sql.contains("UNIQUE") || sql.to_uppercase().contains("UNIQUE"),
            "Normalized unique constraint should be in SQL, but not found: {}",
            sql
        );
        with_settings!({ snapshot_suffix => format!("create_table_with_inline_unique_{:?}", backend) }, {
            assert_snapshot!(sql);
        });
    }

    #[rstest]
    #[case::table_level_unique_postgres(DatabaseBackend::Postgres)]
    #[case::table_level_unique_mysql(DatabaseBackend::MySql)]
    #[case::table_level_unique_sqlite(DatabaseBackend::Sqlite)]
    fn test_create_table_with_table_level_unique(#[case] backend: DatabaseBackend) {
        // Test table-level unique constraint (lines 53-54, 56-58, 60-61)
        let result = build_create_table(
            &backend,
            "users",
            &[
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                col("email", ColumnType::Simple(SimpleColumnType::Text)),
            ],
            &[TableConstraint::Unique {
                name: Some("uq_email".into()),
                columns: vec!["email".into()],
            }],
        )
        .unwrap();
        let sql = result
            .iter()
            .map(|q| q.build(backend))
            .collect::<Vec<String>>()
            .join("\n");
        assert!(sql.contains("CREATE TABLE"));
        // Verify unique constraint is present
        match backend {
            DatabaseBackend::MySql => {
                assert!(
                    sql.contains("UNIQUE"),
                    "MySQL should have UNIQUE in CREATE TABLE: {}",
                    sql
                );
            }
            _ => {
                // For Postgres and SQLite, unique constraint should be in a separate CREATE UNIQUE INDEX statement
                assert!(
                    sql.contains("CREATE UNIQUE INDEX"),
                    "Postgres/SQLite should have CREATE UNIQUE INDEX: {}",
                    sql
                );
            }
        }
        with_settings!({ snapshot_suffix => format!("create_table_with_table_level_unique_{:?}", backend) }, {
            assert_snapshot!(sql);
        });
    }

    #[rstest]
    #[case::table_level_unique_no_name_postgres(DatabaseBackend::Postgres)]
    #[case::table_level_unique_no_name_mysql(DatabaseBackend::MySql)]
    #[case::table_level_unique_no_name_sqlite(DatabaseBackend::Sqlite)]
    fn test_create_table_with_table_level_unique_no_name(#[case] backend: DatabaseBackend) {
        // Test table-level unique constraint without name (lines 53-54, 56-58, 60-61)
        let result = build_create_table(
            &backend,
            "users",
            &[
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                col("email", ColumnType::Simple(SimpleColumnType::Text)),
            ],
            &[TableConstraint::Unique {
                name: None,
                columns: vec!["email".into()],
            }],
        )
        .unwrap();
        let sql = result
            .iter()
            .map(|q| q.build(backend))
            .collect::<Vec<String>>()
            .join("\n");
        assert!(sql.contains("CREATE TABLE"));
        // Verify unique constraint is present
        match backend {
            DatabaseBackend::MySql => {
                assert!(
                    sql.contains("UNIQUE"),
                    "MySQL should have UNIQUE in CREATE TABLE: {}",
                    sql
                );
            }
            _ => {
                // For Postgres and SQLite, unique constraint should be in a separate CREATE UNIQUE INDEX statement
                assert!(
                    sql.contains("CREATE UNIQUE INDEX"),
                    "Postgres/SQLite should have CREATE UNIQUE INDEX: {}",
                    sql
                );
            }
        }
        with_settings!({ snapshot_suffix => format!("create_table_with_table_level_unique_no_name_{:?}", backend) }, {
            assert_snapshot!(sql);
        });
    }

    #[rstest]
    #[case::postgres(DatabaseBackend::Postgres)]
    #[case::mysql(DatabaseBackend::MySql)]
    #[case::sqlite(DatabaseBackend::Sqlite)]
    fn test_create_table_with_enum_column(#[case] backend: DatabaseBackend) {
        // Test creating a table with an enum column (should create enum type first for PostgreSQL)
        let columns = vec![
            ColumnDef {
                name: "id".into(),
                r#type: ColumnType::Simple(SimpleColumnType::Integer),
                nullable: false,
                default: None,
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            },
            ColumnDef {
                name: "status".into(),
                r#type: ColumnType::Complex(ComplexColumnType::Enum {
                    name: "user_status".into(),
                    values: EnumValues::String(vec![
                        "active".into(),
                        "inactive".into(),
                        "pending".into(),
                    ]),
                }),
                nullable: false,
                default: Some("'active'".into()),
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            },
        ];
        let constraints = vec![TableConstraint::PrimaryKey {
            auto_increment: false,
            columns: vec!["id".into()],
        }];

        let result = build_create_table(&backend, "users", &columns, &constraints);
        assert!(result.is_ok());
        let queries = result.unwrap();
        let sql = queries
            .iter()
            .map(|q| q.build(backend))
            .collect::<Vec<String>>()
            .join(";\n");

        with_settings!({ snapshot_suffix => format!("create_table_with_enum_column_{:?}", backend) }, {
            assert_snapshot!(sql);
        });
    }

    #[rstest]
    #[case::auto_increment_postgres(DatabaseBackend::Postgres)]
    #[case::auto_increment_mysql(DatabaseBackend::MySql)]
    #[case::auto_increment_sqlite(DatabaseBackend::Sqlite)]
    fn test_create_table_with_auto_increment_primary_key(#[case] backend: DatabaseBackend) {
        // Test that auto_increment on primary key generates SERIAL/AUTO_INCREMENT/AUTOINCREMENT
        let columns = vec![ColumnDef {
            name: "id".into(),
            r#type: ColumnType::Simple(SimpleColumnType::Integer),
            nullable: false,
            default: None,
            comment: None,
            primary_key: None,
            unique: None,
            index: None,
            foreign_key: None,
        }];
        let constraints = vec![TableConstraint::PrimaryKey {
            auto_increment: true,
            columns: vec!["id".into()],
        }];

        let result = build_create_table(&backend, "users", &columns, &constraints);
        assert!(result.is_ok());
        let queries = result.unwrap();
        let sql = queries
            .iter()
            .map(|q| q.build(backend))
            .collect::<Vec<String>>()
            .join(";\n");

        // Verify auto_increment is applied correctly for each backend
        match backend {
            DatabaseBackend::Postgres => {
                assert!(
                    sql.contains("SERIAL") || sql.contains("serial"),
                    "PostgreSQL should use SERIAL for auto_increment, got: {}",
                    sql
                );
            }
            DatabaseBackend::MySql => {
                assert!(
                    sql.contains("AUTO_INCREMENT") || sql.contains("auto_increment"),
                    "MySQL should use AUTO_INCREMENT for auto_increment, got: {}",
                    sql
                );
            }
            DatabaseBackend::Sqlite => {
                assert!(
                    sql.contains("AUTOINCREMENT") || sql.contains("autoincrement"),
                    "SQLite should use AUTOINCREMENT for auto_increment, got: {}",
                    sql
                );
            }
        }

        with_settings!({ snapshot_suffix => format!("create_table_with_auto_increment_{:?}", backend) }, {
            assert_snapshot!(sql);
        });
    }

    #[rstest]
    #[case::inline_auto_increment_postgres(DatabaseBackend::Postgres)]
    #[case::inline_auto_increment_mysql(DatabaseBackend::MySql)]
    #[case::inline_auto_increment_sqlite(DatabaseBackend::Sqlite)]
    fn test_create_table_with_inline_auto_increment_primary_key(#[case] backend: DatabaseBackend) {
        // Test that inline primary_key with auto_increment generates correct SQL
        use vespertide_core::schema::primary_key::{PrimaryKeyDef, PrimaryKeySyntax};

        let columns = vec![ColumnDef {
            name: "id".into(),
            r#type: ColumnType::Simple(SimpleColumnType::Integer),
            nullable: false,
            default: None,
            comment: None,
            primary_key: Some(PrimaryKeySyntax::Object(PrimaryKeyDef {
                auto_increment: true,
            })),
            unique: None,
            index: None,
            foreign_key: None,
        }];

        let result = build_create_table(&backend, "users", &columns, &[]);
        assert!(result.is_ok());
        let queries = result.unwrap();
        let sql = queries
            .iter()
            .map(|q| q.build(backend))
            .collect::<Vec<String>>()
            .join(";\n");

        // Verify auto_increment is applied correctly for each backend
        match backend {
            DatabaseBackend::Postgres => {
                assert!(
                    sql.contains("SERIAL") || sql.contains("serial"),
                    "PostgreSQL should use SERIAL for auto_increment, got: {}",
                    sql
                );
            }
            DatabaseBackend::MySql => {
                assert!(
                    sql.contains("AUTO_INCREMENT") || sql.contains("auto_increment"),
                    "MySQL should use AUTO_INCREMENT for auto_increment, got: {}",
                    sql
                );
            }
            DatabaseBackend::Sqlite => {
                assert!(
                    sql.contains("AUTOINCREMENT") || sql.contains("autoincrement"),
                    "SQLite should use AUTOINCREMENT for auto_increment, got: {}",
                    sql
                );
            }
        }

        with_settings!({ snapshot_suffix => format!("create_table_with_inline_auto_increment_{:?}", backend) }, {
            assert_snapshot!(sql);
        });
    }

    /// Test creating a table with timestamp column and NOW() default
    /// SQLite should convert NOW() to CURRENT_TIMESTAMP
    #[rstest]
    #[case::timestamp_now_default_postgres(DatabaseBackend::Postgres)]
    #[case::timestamp_now_default_mysql(DatabaseBackend::MySql)]
    #[case::timestamp_now_default_sqlite(DatabaseBackend::Sqlite)]
    fn test_create_table_with_timestamp_now_default(#[case] backend: DatabaseBackend) {
        let columns = vec![
            ColumnDef {
                name: "id".into(),
                r#type: ColumnType::Simple(SimpleColumnType::BigInt),
                nullable: false,
                default: None,
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            },
            ColumnDef {
                name: "created_at".into(),
                r#type: ColumnType::Simple(SimpleColumnType::Timestamptz),
                nullable: false,
                default: Some("NOW()".into()), // uppercase NOW()
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            },
        ];

        let result = build_create_table(&backend, "events", &columns, &[]);
        assert!(result.is_ok(), "build_create_table failed: {:?}", result);
        let queries = result.unwrap();
        let sql = queries
            .iter()
            .map(|q| q.build(backend))
            .collect::<Vec<String>>()
            .join("\n");

        // SQLite should NOT have NOW() - it should be converted to CURRENT_TIMESTAMP
        if matches!(backend, DatabaseBackend::Sqlite) {
            assert!(
                !sql.contains("NOW()"),
                "SQLite should not contain NOW(), got: {}",
                sql
            );
            assert!(
                sql.contains("CURRENT_TIMESTAMP"),
                "SQLite should use CURRENT_TIMESTAMP, got: {}",
                sql
            );
        }

        with_settings!({ snapshot_suffix => format!("create_table_with_timestamp_now_default_{:?}", backend) }, {
            assert_snapshot!(sql);
        });
    }
}
