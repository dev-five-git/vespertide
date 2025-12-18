use sea_query::{Alias, ForeignKey, Index, Table, TableCreateStatement};

use vespertide_core::{ColumnDef, ColumnType, ComplexColumnType, TableConstraint};

use super::helpers::{
    build_create_enum_type_sql, build_schema_statement, build_sea_column_def,
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

    // Add columns
    for column in columns {
        let mut col = build_sea_column_def(backend, column);

        // Check for inline primary key
        if column.primary_key.is_some() && !has_table_primary_key {
            col.primary_key();
        }

        // Check for inline unique constraint
        if column.unique.is_some() {
            col.unique_key();
        }

        stmt = stmt.col(col).to_owned();
    }

    // Add table-level constraints
    for constraint in constraints {
        match constraint {
            TableConstraint::PrimaryKey {
                columns: pk_cols,
                auto_increment: _,
            } => {
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
    let mut queries = Vec::new();

    // Create enum types first (PostgreSQL only)
    // Collect unique enum types to avoid duplicates
    let mut created_enums = std::collections::HashSet::new();
    for column in columns {
        if let ColumnType::Complex(ComplexColumnType::Enum { name, .. }) = &column.r#type
            && created_enums.insert(name.clone())
            && let Some(create_type_sql) = build_create_enum_type_sql(&column.r#type)
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
        // Test inline unique constraint (line 32)
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
            &[],
        )
        .unwrap();
        let sql = result
            .iter()
            .map(|q| q.build(backend))
            .collect::<Vec<String>>()
            .join("\n");
        assert!(sql.contains("UNIQUE"));
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
}
