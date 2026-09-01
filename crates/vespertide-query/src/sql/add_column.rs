use sea_query::{Alias, Expr, Query, Table, TableAlterStatement};

use vespertide_core::{ColumnDef, TableDef};

use super::fill_with::convert_fill_with_for_backend;
use super::helpers::{
    build_create_enum_type_sql, build_sea_column_def_with_table, build_sqlite_temp_table_create,
    convert_default_for_backend, normalize_enum_default, normalize_fill_with,
    recreate_indexes_after_rebuild, require_table_in_schema,
};
use super::rename_table::build_rename_table;
use super::types::{BuiltQuery, DatabaseBackend};
use crate::error::QueryError;

fn build_add_column_alter_for_backend(
    backend: DatabaseBackend,
    table: &str,
    column: &ColumnDef,
) -> TableAlterStatement {
    let col_def = build_sea_column_def_with_table(backend, table, column);
    Table::alter()
        .table(Alias::new(table))
        .add_column(col_def)
        .to_owned()
}

/// Check if the column type is an enum
fn is_enum_column(column: &ColumnDef) -> bool {
    matches!(
        column.r#type,
        vespertide_core::ColumnType::Complex(vespertide_core::ComplexColumnType::Enum { .. })
    )
}

pub fn build_add_column(
    backend: DatabaseBackend,
    table: &str,
    column: &ColumnDef,
    fill_with: Option<&str>,
    current_schema: &[TableDef],
    pending_constraints: &[vespertide_core::TableConstraint],
) -> Result<Vec<BuiltQuery>, QueryError> {
    // SQLite: NOT NULL additions or enum columns require table recreation
    // (enum columns need CHECK constraint which requires table recreation in SQLite)
    let sqlite_needs_recreation =
        backend == DatabaseBackend::Sqlite && (!column.nullable || is_enum_column(column));

    if sqlite_needs_recreation {
        let table_def = require_table_in_schema(
            current_schema,
            table,
            "SQLite requires current schema information to add columns",
        )?;

        let mut new_columns = table_def.columns.clone();
        new_columns.push(column.clone());

        let temp_table = format!("{table}_temp");

        // 1. Create temporary table with all CHECK constraints (enum + explicit)
        let create_query = build_sqlite_temp_table_create(
            backend,
            &temp_table,
            table,
            &new_columns,
            &table_def.constraints,
        );

        // Copy existing data, filling new column. Build the existing-column
        // aliases once and reuse them for both the SELECT column list and the
        // INSERT column list (the new column's alias is appended for the INSERT
        // only, since its value comes from `expr_as(fill_expr, ...)`).
        let mut columns_alias: Vec<Alias> = Vec::with_capacity(table_def.columns.len() + 1);
        let mut select_query = Query::select();
        for col in &table_def.columns {
            let alias = Alias::new(&col.name);
            select_query.column(alias.clone());
            columns_alias.push(alias);
        }
        let fill_expr = if let Some(fill) = normalize_fill_with(fill_with) {
            let converted = convert_fill_with_for_backend(fill, backend);
            Expr::cust(normalize_enum_default(&column.r#type, &converted))
        } else if let Some(def) = &column.default {
            let converted = convert_default_for_backend(&def.to_sql(), backend);
            Expr::cust(normalize_enum_default(&column.r#type, &converted))
        } else {
            Expr::cust("NULL")
        };
        select_query
            .expr_as(fill_expr, Alias::new(&column.name))
            .from(Alias::new(table));

        columns_alias.push(Alias::new(&column.name));
        let insert_stmt = Query::insert()
            .into_table(Alias::new(&temp_table))
            .columns(columns_alias)
            .select_from(select_query)
            .expect("SQLite temp table copy SELECT should be valid")
            .to_owned();
        let insert_query = BuiltQuery::Insert(Box::new(insert_stmt));

        let drop_query = super::delete_table::build_delete_table(table);
        let rename_query = build_rename_table(&temp_table, table);

        // Recreate indexes (both regular and UNIQUE)
        // Skip pending constraints that will be created by future AddConstraint actions
        let index_queries =
            recreate_indexes_after_rebuild(table, &table_def.constraints, pending_constraints);

        let mut stmts = vec![create_query, insert_query, drop_query, rename_query];
        stmts.extend(index_queries);
        return Ok(stmts);
    }

    let mut stmts: Vec<BuiltQuery> = Vec::new();

    // If column type is an enum, create the type first (PostgreSQL only)
    if let Some(create_type_sql) = build_create_enum_type_sql(table, &column.r#type) {
        stmts.push(BuiltQuery::Raw(create_type_sql));
    }

    // If adding NOT NULL without default, we need special handling
    let needs_backfill = !column.nullable && column.default.is_none() && fill_with.is_some();

    if needs_backfill {
        // Add as nullable first
        let mut temp_col = column.clone();
        temp_col.nullable = true;

        stmts.push(BuiltQuery::AlterTable(Box::new(
            build_add_column_alter_for_backend(backend, table, &temp_col),
        )));

        // Backfill with provided value
        if let Some(fill) = normalize_fill_with(fill_with) {
            let fill = convert_fill_with_for_backend(fill, backend);
            let update_stmt = Query::update()
                .table(Alias::new(table))
                .value(Alias::new(&column.name), Expr::cust(fill))
                .to_owned();
            stmts.push(BuiltQuery::Update(Box::new(update_stmt)));
        }

        // Set NOT NULL
        let not_null_col = build_sea_column_def_with_table(backend, table, column);
        let alter_not_null = Table::alter()
            .table(Alias::new(table))
            .modify_column(not_null_col)
            .to_owned();
        stmts.push(BuiltQuery::AlterTable(Box::new(alter_not_null)));
    } else {
        stmts.push(BuiltQuery::AlterTable(Box::new(
            build_add_column_alter_for_backend(backend, table, column),
        )));
    }

    Ok(stmts)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{backend_tag, joined_sql, joined_sql_semicolon};
    use insta::{assert_snapshot, with_settings};
    use rstest::rstest;
    use vespertide_core::{ColumnType, SimpleColumnType, TableDef};

    #[rstest]
    #[case::add_column_with_backfill_postgres(
        "add_column_with_backfill_postgres",
        DatabaseBackend::Postgres,
        &["ALTER TABLE \"users\" ADD COLUMN \"nickname\" text"]
    )]
    #[case::add_column_with_backfill_mysql(
        "add_column_with_backfill_mysql",
        DatabaseBackend::MySql,
        &["ALTER TABLE `users` ADD COLUMN `nickname` text"]
    )]
    #[case::add_column_with_backfill_sqlite(
        "add_column_with_backfill_sqlite",
        DatabaseBackend::Sqlite,
        &["CREATE TABLE \"users_temp\""]
    )]
    #[case::add_column_simple_postgres(
        "add_column_simple_postgres",
        DatabaseBackend::Postgres,
        &["ALTER TABLE \"users\" ADD COLUMN \"nickname\""]
    )]
    #[case::add_column_simple_mysql(
        "add_column_simple_mysql",
        DatabaseBackend::MySql,
        &["ALTER TABLE `users` ADD COLUMN `nickname` text"]
    )]
    #[case::add_column_simple_sqlite(
        "add_column_simple_sqlite",
        DatabaseBackend::Sqlite,
        &["ALTER TABLE \"users\" ADD COLUMN \"nickname\""]
    )]
    #[case::add_column_nullable_postgres(
        "add_column_nullable_postgres",
        DatabaseBackend::Postgres,
        &["ALTER TABLE \"users\" ADD COLUMN \"email\" text"]
    )]
    #[case::add_column_nullable_mysql(
        "add_column_nullable_mysql",
        DatabaseBackend::MySql,
        &["ALTER TABLE `users` ADD COLUMN `email` text"]
    )]
    #[case::add_column_nullable_sqlite(
        "add_column_nullable_sqlite",
        DatabaseBackend::Sqlite,
        &["ALTER TABLE \"users\" ADD COLUMN \"email\" text"]
    )]
    fn test_add_column(
        #[case] title: &str,
        #[case] backend: DatabaseBackend,
        #[case] expected: &[&str],
    ) {
        let column = ColumnDef {
            name: if title.contains("age") {
                "age"
            } else if title.contains("nullable") {
                "email"
            } else {
                "nickname"
            }
            .into(),
            r#type: if title.contains("age") {
                ColumnType::Simple(SimpleColumnType::Integer)
            } else {
                ColumnType::Simple(SimpleColumnType::Text)
            },
            nullable: !title.contains("backfill"),
            default: None,
            comment: None,
            primary_key: None,
            unique: None,
            index: None,
            foreign_key: None,
        };
        let fill_with = if title.contains("backfill") {
            Some("0")
        } else {
            None
        };
        let current_schema = vec![TableDef {
            name: "users".into(),
            description: None,
            columns: vec![ColumnDef {
                name: "id".into(),
                r#type: ColumnType::Simple(SimpleColumnType::Integer),
                nullable: false,
                default: None,
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            }],
            constraints: vec![],
        }];
        let result =
            build_add_column(backend, "users", &column, fill_with, &current_schema, &[]).unwrap();
        let sql = result[0].build(backend);
        for exp in expected {
            assert!(
                sql.contains(exp),
                "Expected SQL to contain '{exp}', got: {sql}"
            );
        }

        with_settings!({ snapshot_suffix => format!("add_column_{}", title) }, {
            assert_snapshot!(joined_sql(backend, &result));
        });
    }

    #[test]
    fn test_add_column_sqlite_table_not_found() {
        let column = ColumnDef {
            name: "nickname".into(),
            r#type: ColumnType::Simple(SimpleColumnType::Text),
            nullable: false,
            default: None,
            comment: None,
            primary_key: None,
            unique: None,
            index: None,
            foreign_key: None,
        };
        let current_schema = vec![]; // Empty schema - table not found
        let result = build_add_column(
            DatabaseBackend::Sqlite,
            "users",
            &column,
            None,
            &current_schema,
            &[],
        );
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("Table 'users' not found in current schema"));
    }

    #[test]
    fn test_add_column_sqlite_with_default() {
        let column = ColumnDef {
            name: "age".into(),
            r#type: ColumnType::Simple(SimpleColumnType::Integer),
            nullable: false,
            default: Some("18".into()),
            comment: None,
            primary_key: None,
            unique: None,
            index: None,
            foreign_key: None,
        };
        let current_schema = vec![TableDef {
            name: "users".into(),
            description: None,
            columns: vec![ColumnDef {
                name: "id".into(),
                r#type: ColumnType::Simple(SimpleColumnType::Integer),
                nullable: false,
                default: None,
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            }],
            constraints: vec![],
        }];
        let result = build_add_column(
            DatabaseBackend::Sqlite,
            "users",
            &column,
            None,
            &current_schema,
            &[],
        );
        assert!(result.is_ok());
        let queries = result.unwrap();
        let sql = joined_sql(DatabaseBackend::Sqlite, &queries);
        // Should use default value (18) for fill
        assert!(sql.contains("18"));
    }

    #[test]
    fn test_add_column_sqlite_without_fill_or_default() {
        let column = ColumnDef {
            name: "age".into(),
            r#type: ColumnType::Simple(SimpleColumnType::Integer),
            nullable: false,
            default: None,
            comment: None,
            primary_key: None,
            unique: None,
            index: None,
            foreign_key: None,
        };
        let current_schema = vec![TableDef {
            name: "users".into(),
            description: None,
            columns: vec![ColumnDef {
                name: "id".into(),
                r#type: ColumnType::Simple(SimpleColumnType::Integer),
                nullable: false,
                default: None,
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            }],
            constraints: vec![],
        }];
        let result = build_add_column(
            DatabaseBackend::Sqlite,
            "users",
            &column,
            None,
            &current_schema,
            &[],
        );
        assert!(result.is_ok());
        let queries = result.unwrap();
        let sql = joined_sql(DatabaseBackend::Sqlite, &queries);
        // Should use NULL for fill
        assert!(sql.contains("NULL"));
    }

    #[test]
    fn test_add_column_sqlite_with_indexes() {
        use vespertide_core::TableConstraint;

        let column = ColumnDef {
            name: "nickname".into(),
            r#type: ColumnType::Simple(SimpleColumnType::Text),
            nullable: false,
            default: None,
            comment: None,
            primary_key: None,
            unique: None,
            index: None,
            foreign_key: None,
        };
        let current_schema = vec![TableDef {
            name: "users".into(),
            description: None,
            columns: vec![ColumnDef {
                name: "id".into(),
                r#type: ColumnType::Simple(SimpleColumnType::Integer),
                nullable: false,
                default: None,
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            }],
            constraints: vec![TableConstraint::Index {
                name: Some("idx_id".into()),
                columns: vec!["id".into()],
            }],
        }];
        let result = build_add_column(
            DatabaseBackend::Sqlite,
            "users",
            &column,
            None,
            &current_schema,
            &[],
        );
        assert!(result.is_ok());
        let queries = result.unwrap();
        let sql = joined_sql(DatabaseBackend::Sqlite, &queries);
        // Should recreate index
        assert!(sql.contains("CREATE INDEX"));
        assert!(sql.contains("idx_id"));
    }

    #[rstest]
    #[case::add_column_with_enum_type_postgres(DatabaseBackend::Postgres)]
    #[case::add_column_with_enum_type_mysql(DatabaseBackend::MySql)]
    #[case::add_column_with_enum_type_sqlite(DatabaseBackend::Sqlite)]
    fn test_add_column_with_enum_type(#[case] backend: DatabaseBackend) {
        use insta::{assert_snapshot, with_settings};
        use vespertide_core::{ComplexColumnType, EnumValues};

        // Test that adding an enum column creates the enum type first (PostgreSQL only)
        let column = ColumnDef {
            name: "status".into(),
            r#type: ColumnType::Complex(ComplexColumnType::Enum {
                name: "status_type".into(),
                values: EnumValues::String(vec!["active".into(), "inactive".into()]),
            }),
            nullable: true,
            default: None,
            comment: None,
            primary_key: None,
            unique: None,
            index: None,
            foreign_key: None,
        };
        let current_schema = vec![TableDef {
            name: "users".into(),
            description: None,
            columns: vec![ColumnDef {
                name: "id".into(),
                r#type: ColumnType::Simple(SimpleColumnType::Integer),
                nullable: false,
                default: None,
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            }],
            constraints: vec![],
        }];
        let result = build_add_column(backend, "users", &column, None, &current_schema, &[]);
        assert!(result.is_ok());
        let queries = result.unwrap();
        let sql = joined_sql_semicolon(backend, &queries);

        with_settings!({ snapshot_suffix => format!("add_column_with_enum_type_{:?}", backend) }, {
            assert_snapshot!(sql);
        });
    }

    #[rstest]
    #[case::postgres(DatabaseBackend::Postgres)]
    #[case::mysql(DatabaseBackend::MySql)]
    #[case::sqlite(DatabaseBackend::Sqlite)]
    fn test_add_column_enum_non_nullable_with_default(#[case] backend: DatabaseBackend) {
        use insta::{assert_snapshot, with_settings};
        use vespertide_core::{ComplexColumnType, EnumValues};

        // Test adding an enum column that is non-nullable with a default value
        let column = ColumnDef {
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
            default: Some("active".into()),
            comment: None,
            primary_key: None,
            unique: None,
            index: None,
            foreign_key: None,
        };
        let current_schema = vec![TableDef {
            name: "users".into(),
            description: None,
            columns: vec![ColumnDef {
                name: "id".into(),
                r#type: ColumnType::Simple(SimpleColumnType::Integer),
                nullable: false,
                default: None,
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            }],
            constraints: vec![],
        }];
        let result = build_add_column(backend, "users", &column, None, &current_schema, &[]);
        assert!(result.is_ok());
        let queries = result.unwrap();
        let sql = joined_sql_semicolon(backend, &queries);

        with_settings!({ snapshot_suffix => format!("enum_non_nullable_with_default_{:?}", backend) }, {
            assert_snapshot!(sql);
        });
    }

    #[rstest]
    #[case::postgres(DatabaseBackend::Postgres)]
    #[case::mysql(DatabaseBackend::MySql)]
    #[case::sqlite(DatabaseBackend::Sqlite)]
    fn test_add_column_with_empty_string_default(#[case] backend: DatabaseBackend) {
        use insta::{assert_snapshot, with_settings};

        // Test adding a text column with empty string default
        let column = ColumnDef {
            name: "nickname".into(),
            r#type: ColumnType::Simple(SimpleColumnType::Text),
            nullable: false,
            default: Some("".into()), // Empty string default
            comment: None,
            primary_key: None,
            unique: None,
            index: None,
            foreign_key: None,
        };
        let current_schema = vec![TableDef {
            name: "users".into(),
            description: None,
            columns: vec![ColumnDef {
                name: "id".into(),
                r#type: ColumnType::Simple(SimpleColumnType::Integer),
                nullable: false,
                default: None,
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            }],
            constraints: vec![],
        }];
        let result = build_add_column(backend, "users", &column, None, &current_schema, &[]);
        assert!(result.is_ok());
        let queries = result.unwrap();
        let sql = joined_sql_semicolon(backend, &queries);

        // Verify empty string becomes ''
        assert!(
            sql.contains("''"),
            "Expected SQL to contain empty string literal '', got: {sql}"
        );

        with_settings!({ snapshot_suffix => format!("empty_string_default_{:?}", backend) }, {
            assert_snapshot!(sql);
        });
    }

    /// Test adding NOT NULL column with '[]'`::json` default on `SQLite`
    /// `SQLite` should strip the `::json` cast, `MySQL` should use CAST(... AS JSON)
    #[rstest]
    #[case::postgres(DatabaseBackend::Postgres)]
    #[case::mysql(DatabaseBackend::MySql)]
    #[case::sqlite(DatabaseBackend::Sqlite)]
    fn test_add_column_with_pg_type_cast_default(#[case] backend: DatabaseBackend) {
        let column = ColumnDef {
            name: "story_index".into(),
            r#type: ColumnType::Simple(SimpleColumnType::Json),
            nullable: false,
            default: Some("'[]'::json".into()),
            comment: None,
            primary_key: None,
            unique: None,
            index: None,
            foreign_key: None,
        };
        let current_schema = vec![TableDef {
            name: "project".into(),
            description: None,
            columns: vec![ColumnDef {
                name: "id".into(),
                r#type: ColumnType::Simple(SimpleColumnType::Integer),
                nullable: false,
                default: None,
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            }],
            constraints: vec![],
        }];
        let result =
            build_add_column(backend, "project", &column, None, &current_schema, &[]).unwrap();
        let sql = joined_sql(backend, &result);

        // SQLite must NOT contain ::json syntax
        if backend == DatabaseBackend::Sqlite {
            assert!(
                !sql.contains("::json"),
                "SQLite SQL should not contain ::json cast, got: {sql}"
            );
        }

        // MySQL should use CAST syntax
        if backend == DatabaseBackend::MySql {
            assert!(
                !sql.contains("::json"),
                "MySQL SQL should not contain ::json cast, got: {sql}"
            );
        }

        with_settings!({ snapshot_suffix => format!("pg_type_cast_default_{:?}", backend) }, {
            assert_snapshot!(sql);
        });
    }

    #[rstest]
    #[case::postgres(DatabaseBackend::Postgres)]
    #[case::mysql(DatabaseBackend::MySql)]
    #[case::sqlite(DatabaseBackend::Sqlite)]
    fn test_add_column_with_fill_with_empty_string(#[case] backend: DatabaseBackend) {
        use insta::{assert_snapshot, with_settings};

        // Test adding a column with fill_with as empty string
        let column = ColumnDef {
            name: "nickname".into(),
            r#type: ColumnType::Simple(SimpleColumnType::Text),
            nullable: false,
            default: None,
            comment: None,
            primary_key: None,
            unique: None,
            index: None,
            foreign_key: None,
        };
        let current_schema = vec![TableDef {
            name: "users".into(),
            description: None,
            columns: vec![ColumnDef {
                name: "id".into(),
                r#type: ColumnType::Simple(SimpleColumnType::Integer),
                nullable: false,
                default: None,
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            }],
            constraints: vec![],
        }];
        // fill_with empty string should become ''
        let result = build_add_column(backend, "users", &column, Some(""), &current_schema, &[]);
        assert!(result.is_ok());
        let queries = result.unwrap();
        let sql = joined_sql_semicolon(backend, &queries);

        // Verify empty string becomes ''
        assert!(
            sql.contains("''"),
            "Expected SQL to contain empty string literal '', got: {sql}"
        );

        with_settings!({ snapshot_suffix => format!("fill_with_empty_string_{:?}", backend) }, {
            assert_snapshot!(sql);
        });
    }

    fn backfill_sql(backend: DatabaseBackend, column: &ColumnDef, fill: &str) -> String {
        use crate::test_support::{col_n, table_def};

        let current_schema = vec![table_def(
            "subscription",
            vec![
                col_n("id", ColumnType::Simple(SimpleColumnType::Integer), false),
                col_n(
                    "plan_key",
                    ColumnType::Simple(SimpleColumnType::Text),
                    false,
                ),
                col_n(
                    "plan_tag",
                    ColumnType::Simple(SimpleColumnType::Text),
                    false,
                ),
                col_n(
                    "device_os",
                    ColumnType::Simple(SimpleColumnType::Text),
                    false,
                ),
                col_n(
                    "device_family",
                    ColumnType::Simple(SimpleColumnType::Text),
                    false,
                ),
            ],
            vec![],
        )];
        let queries = build_add_column(
            backend,
            "subscription",
            column,
            Some(fill),
            &current_schema,
            &[],
        )
        .expect("add_column with fill_with should build");
        joined_sql_semicolon(backend, &queries)
    }

    fn not_null_column(name: &str, r#type: ColumnType) -> ColumnDef {
        ColumnDef {
            name: name.into(),
            r#type,
            nullable: false,
            default: None,
            comment: None,
            primary_key: None,
            unique: None,
            index: None,
            foreign_key: None,
        }
    }

    /// Regression: a `fill_with` CASE expression comparing a text-cast column
    /// to the uppercase literal `API` and returning `MONTHLY_QUOTA` / `SEAT`,
    /// wrapped in parens and cast to an enum type.
    ///
    /// Splitting at the *first* `::` and lower-casing the remainder produced
    /// `'api'` / `'monthly_quota'` / `'seat'`: the comparison never matched, so
    /// the backfill silently did nothing, and the lower-cased token was not a
    /// valid enum label so the cast failed.
    #[rstest]
    #[case::postgres(DatabaseBackend::Postgres)]
    #[case::mysql(DatabaseBackend::MySql)]
    #[case::sqlite(DatabaseBackend::Sqlite)]
    fn fill_with_enum_cast_case_expression_is_verbatim(#[case] backend: DatabaseBackend) {
        use vespertide_core::{ComplexColumnType, EnumValues};

        const FILL: &str = "(CASE WHEN plan_key::text = 'API' THEN 'MONTHLY_QUOTA' ELSE 'SEAT' END)::billing_metric";

        let column = not_null_column(
            "metric",
            ColumnType::Complex(ComplexColumnType::Enum {
                name: "billing_metric".into(),
                values: EnumValues::String(vec!["MONTHLY_QUOTA".into(), "SEAT".into()]),
            }),
        );
        let sql = backfill_sql(backend, &column, FILL);

        assert!(
            sql.contains(FILL),
            "fill_with must survive byte-for-byte, got: {sql}"
        );

        with_settings!({ snapshot_suffix => format!("fill_with_enum_cast_verbatim_{}", backend_tag(backend)) }, {
            assert_snapshot!(sql);
        });
    }

    /// Regression: uppercase `WINDOWS` sits *before* the first cast operator
    /// and survived, while the `ELSE` / `END` keywords *after* it were
    /// lower-cased — the observation that pinpointed the first-`::` split.
    #[rstest]
    #[case::postgres(DatabaseBackend::Postgres)]
    #[case::mysql(DatabaseBackend::MySql)]
    #[case::sqlite(DatabaseBackend::Sqlite)]
    fn fill_with_json_array_case_expression_is_verbatim(#[case] backend: DatabaseBackend) {
        const FILL: &str = "CASE WHEN device_os = 'win' THEN json_build_array('WINDOWS', device_family::text) ELSE '[]'::json END";

        let column = not_null_column("os_tags", ColumnType::Simple(SimpleColumnType::Json));
        let sql = backfill_sql(backend, &column, FILL);

        assert!(
            sql.contains(FILL),
            "fill_with must survive byte-for-byte, got: {sql}"
        );

        with_settings!({ snapshot_suffix => format!("fill_with_json_array_verbatim_{}", backend_tag(backend)) }, {
            assert_snapshot!(sql);
        });
    }

    /// Regression: the comparison literal itself contains a cast operator
    /// inside single quotes, followed by a trailing cast to integer. Splitting
    /// on the first `::` cut the statement open inside the string literal.
    #[rstest]
    #[case::postgres(DatabaseBackend::Postgres)]
    #[case::mysql(DatabaseBackend::MySql)]
    #[case::sqlite(DatabaseBackend::Sqlite)]
    fn fill_with_cast_operator_inside_quotes_is_verbatim(#[case] backend: DatabaseBackend) {
        const FILL: &str = "CASE WHEN plan_tag = 'legacy::v1' THEN 1 ELSE 2 END::integer";

        let column = not_null_column("tier", ColumnType::Simple(SimpleColumnType::Integer));
        let sql = backfill_sql(backend, &column, FILL);

        assert!(
            sql.contains(FILL),
            "fill_with must survive byte-for-byte, got: {sql}"
        );

        with_settings!({ snapshot_suffix => format!("fill_with_quoted_cast_verbatim_{}", backend_tag(backend)) }, {
            assert_snapshot!(sql);
        });
    }
}
