use sea_query::{
    Alias, ColumnDef as SeaColumnDef, ForeignKey, ForeignKeyAction, ForeignKeyCreateStatement,
    ForeignKeyDropStatement, Index, IndexCreateStatement, IndexDropStatement, MysqlQueryBuilder,
    PostgresQueryBuilder, SqliteQueryBuilder, Table, TableAlterStatement, TableCreateStatement,
    TableDropStatement, TableRenameStatement,
};
use vespertide_core::{
    ColumnDef, ColumnType, ComplexColumnType, MigrationAction, ReferenceAction, SimpleColumnType,
    TableConstraint,
};

use crate::error::QueryError;

/// Database backend for SQL generation
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DatabaseBackend {
    Postgres,
    MySql,
    Sqlite,
}

/// Represents a built query that can be converted to SQL for any database backend
#[derive(Debug, Clone)]
pub enum BuiltQuery {
    CreateTable(TableCreateStatement),
    DropTable(TableDropStatement),
    AlterTable(TableAlterStatement),
    CreateIndex(IndexCreateStatement),
    DropIndex(IndexDropStatement),
    RenameTable(TableRenameStatement),
    CreateForeignKey(ForeignKeyCreateStatement),
    DropForeignKey(ForeignKeyDropStatement),
    Raw(String),
}

impl BuiltQuery {
    /// Build SQL string for the specified database backend
    pub fn build(&self, backend: DatabaseBackend) -> String {
        match self {
            BuiltQuery::CreateTable(stmt) => match backend {
                DatabaseBackend::Postgres => stmt.to_string(PostgresQueryBuilder),
                DatabaseBackend::MySql => stmt.to_string(MysqlQueryBuilder),
                DatabaseBackend::Sqlite => stmt.to_string(SqliteQueryBuilder),
            },
            BuiltQuery::DropTable(stmt) => match backend {
                DatabaseBackend::Postgres => stmt.to_string(PostgresQueryBuilder),
                DatabaseBackend::MySql => stmt.to_string(MysqlQueryBuilder),
                DatabaseBackend::Sqlite => stmt.to_string(SqliteQueryBuilder),
            },
            BuiltQuery::AlterTable(stmt) => match backend {
                DatabaseBackend::Postgres => stmt.to_string(PostgresQueryBuilder),
                DatabaseBackend::MySql => stmt.to_string(MysqlQueryBuilder),
                DatabaseBackend::Sqlite => stmt.to_string(SqliteQueryBuilder),
            },
            BuiltQuery::CreateIndex(stmt) => match backend {
                DatabaseBackend::Postgres => stmt.to_string(PostgresQueryBuilder),
                DatabaseBackend::MySql => stmt.to_string(MysqlQueryBuilder),
                DatabaseBackend::Sqlite => stmt.to_string(SqliteQueryBuilder),
            },
            BuiltQuery::DropIndex(stmt) => match backend {
                DatabaseBackend::Postgres => stmt.to_string(PostgresQueryBuilder),
                DatabaseBackend::MySql => stmt.to_string(MysqlQueryBuilder),
                DatabaseBackend::Sqlite => stmt.to_string(SqliteQueryBuilder),
            },
            BuiltQuery::RenameTable(stmt) => match backend {
                DatabaseBackend::Postgres => stmt.to_string(PostgresQueryBuilder),
                DatabaseBackend::MySql => stmt.to_string(MysqlQueryBuilder),
                DatabaseBackend::Sqlite => stmt.to_string(SqliteQueryBuilder),
            },
            BuiltQuery::CreateForeignKey(stmt) => match backend {
                DatabaseBackend::Postgres => stmt.to_string(PostgresQueryBuilder),
                DatabaseBackend::MySql => stmt.to_string(MysqlQueryBuilder),
                DatabaseBackend::Sqlite => stmt.to_string(SqliteQueryBuilder),
            },
            BuiltQuery::DropForeignKey(stmt) => match backend {
                DatabaseBackend::Postgres => stmt.to_string(PostgresQueryBuilder),
                DatabaseBackend::MySql => stmt.to_string(MysqlQueryBuilder),
                DatabaseBackend::Sqlite => stmt.to_string(SqliteQueryBuilder),
            },
            BuiltQuery::Raw(sql) => sql.clone(),
        }
    }

    /// Backward compatibility: get SQL string (defaults to PostgreSQL)
    pub fn sql(&self) -> String {
        self.build(DatabaseBackend::Postgres)
    }

    /// Backward compatibility: binds are now empty (DDL doesn't use bind parameters)
    pub fn binds(&self) -> Vec<String> {
        Vec::new()
    }
}

/// Apply vespertide ColumnType to sea_query ColumnDef
fn apply_column_type(col: &mut SeaColumnDef, ty: &ColumnType) {
    match ty {
        ColumnType::Simple(simple) => match simple {
            SimpleColumnType::SmallInt => {
                col.small_integer();
            }
            SimpleColumnType::Integer => {
                col.integer();
            }
            SimpleColumnType::BigInt => {
                col.big_integer();
            }
            SimpleColumnType::Real => {
                col.float();
            }
            SimpleColumnType::DoublePrecision => {
                col.double();
            }
            SimpleColumnType::Text => {
                col.text();
            }
            SimpleColumnType::Boolean => {
                col.boolean();
            }
            SimpleColumnType::Date => {
                col.date();
            }
            SimpleColumnType::Time => {
                col.time();
            }
            SimpleColumnType::Timestamp => {
                col.timestamp();
            }
            SimpleColumnType::Timestamptz => {
                col.timestamp_with_time_zone();
            }
            SimpleColumnType::Interval => {
                col.interval(None, None);
            }
            SimpleColumnType::Bytea => {
                col.binary();
            }
            SimpleColumnType::Uuid => {
                col.uuid();
            }
            SimpleColumnType::Json => {
                col.json();
            }
            SimpleColumnType::Jsonb => {
                col.json_binary();
            }
            SimpleColumnType::Inet => {
                col.custom(Alias::new("INET"));
            }
            SimpleColumnType::Cidr => {
                col.custom(Alias::new("CIDR"));
            }
            SimpleColumnType::Macaddr => {
                col.custom(Alias::new("MACADDR"));
            }
            SimpleColumnType::Xml => {
                col.custom(Alias::new("XML"));
            }
        },
        ColumnType::Complex(complex) => match complex {
            ComplexColumnType::Varchar { length } => {
                col.string_len(*length);
            }
            ComplexColumnType::Numeric { precision, scale } => {
                col.decimal_len(*precision, *scale);
            }
            ComplexColumnType::Char { length } => {
                col.char_len(*length);
            }
            ComplexColumnType::Custom { custom_type } => {
                col.custom(Alias::new(custom_type));
            }
        },
    }
}

/// Convert vespertide ReferenceAction to sea_query ForeignKeyAction
fn to_sea_fk_action(action: &ReferenceAction) -> ForeignKeyAction {
    match action {
        ReferenceAction::Cascade => ForeignKeyAction::Cascade,
        ReferenceAction::Restrict => ForeignKeyAction::Restrict,
        ReferenceAction::SetNull => ForeignKeyAction::SetNull,
        ReferenceAction::SetDefault => ForeignKeyAction::SetDefault,
        ReferenceAction::NoAction => ForeignKeyAction::NoAction,
    }
}

/// Build sea_query ColumnDef from vespertide ColumnDef
fn build_sea_column_def(column: &ColumnDef) -> SeaColumnDef {
    let mut col = SeaColumnDef::new(Alias::new(&column.name));
    apply_column_type(&mut col, &column.r#type);

    if !column.nullable {
        col.not_null();
    }

    if let Some(default) = &column.default {
        col.default(sea_query::Expr::cust(default));
    }

    col
}

pub fn build_action_queries(action: &MigrationAction) -> Result<Vec<BuiltQuery>, QueryError> {
    match action {
        MigrationAction::CreateTable {
            table,
            columns,
            constraints,
        } => Ok(vec![build_create_table(table, columns, constraints)?]),

        MigrationAction::DeleteTable { table } => {
            let stmt = Table::drop().table(Alias::new(table)).to_owned();
            Ok(vec![BuiltQuery::DropTable(stmt)])
        }

        MigrationAction::AddColumn {
            table,
            column,
            fill_with,
        } => build_add_column(table, column, fill_with.as_deref()),

        MigrationAction::RenameColumn { table, from, to } => {
            let stmt = Table::alter()
                .table(Alias::new(table))
                .rename_column(Alias::new(from), Alias::new(to))
                .to_owned();
            Ok(vec![BuiltQuery::AlterTable(stmt)])
        }

        MigrationAction::DeleteColumn { table, column } => {
            let stmt = Table::alter()
                .table(Alias::new(table))
                .drop_column(Alias::new(column))
                .to_owned();
            Ok(vec![BuiltQuery::AlterTable(stmt)])
        }

        MigrationAction::ModifyColumnType {
            table,
            column,
            new_type,
        } => {
            let mut col = SeaColumnDef::new(Alias::new(column));
            apply_column_type(&mut col, new_type);

            let stmt = Table::alter()
                .table(Alias::new(table))
                .modify_column(col)
                .to_owned();
            Ok(vec![BuiltQuery::AlterTable(stmt)])
        }

        MigrationAction::AddIndex { table, index } => {
            let mut stmt = Index::create()
                .name(&index.name)
                .table(Alias::new(table))
                .to_owned();

            for col in &index.columns {
                stmt = stmt.col(Alias::new(col)).to_owned();
            }

            if index.unique {
                stmt = stmt.unique().to_owned();
            }

            Ok(vec![BuiltQuery::CreateIndex(stmt)])
        }

        MigrationAction::RemoveIndex { name, .. } => {
            let stmt = Index::drop().name(name).to_owned();
            Ok(vec![BuiltQuery::DropIndex(stmt)])
        }

        MigrationAction::RenameTable { from, to } => {
            let stmt = Table::rename()
                .table(Alias::new(from), Alias::new(to))
                .to_owned();
            Ok(vec![BuiltQuery::RenameTable(stmt)])
        }

        MigrationAction::RawSql { sql } => Ok(vec![BuiltQuery::Raw(sql.clone())]),

        MigrationAction::AddConstraint { table, constraint } => {
            build_add_constraint(table, constraint)
        }

        MigrationAction::RemoveConstraint { table, constraint } => {
            build_remove_constraint(table, constraint)
        }
    }
}

fn build_create_table(
    table: &str,
    columns: &[ColumnDef],
    constraints: &[TableConstraint],
) -> Result<BuiltQuery, QueryError> {
    let mut stmt = Table::create().table(Alias::new(table)).to_owned();

    let has_table_primary_key = constraints
        .iter()
        .any(|c| matches!(c, TableConstraint::PrimaryKey { .. }));

    // Add columns
    for column in columns {
        let mut col = build_sea_column_def(column);

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
                let mut idx = Index::create();
                if let Some(n) = name {
                    idx = idx.name(n).to_owned();
                }
                for col in unique_cols {
                    idx = idx.col(Alias::new(col)).to_owned();
                }
                // Note: sea-query doesn't have a direct way to add named unique constraints
                // We'll handle this as a separate index if needed
            }
            TableConstraint::ForeignKey {
                name,
                columns: fk_cols,
                ref_table,
                ref_columns,
                on_delete,
                on_update,
            } => {
                let mut fk = ForeignKey::create();
                if let Some(n) = name {
                    fk = fk.name(n).to_owned();
                }
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

    Ok(BuiltQuery::CreateTable(stmt))
}

fn build_add_column(
    table: &str,
    column: &ColumnDef,
    fill_with: Option<&str>,
) -> Result<Vec<BuiltQuery>, QueryError> {
    let mut stmts: Vec<BuiltQuery> = Vec::new();

    // If adding NOT NULL without default, we need special handling
    let needs_backfill = !column.nullable && column.default.is_none() && fill_with.is_some();

    if needs_backfill {
        // Add as nullable first
        let mut temp_col = column.clone();
        temp_col.nullable = true;
        let col_def = build_sea_column_def(&temp_col);

        let stmt = Table::alter()
            .table(Alias::new(table))
            .add_column(col_def)
            .to_owned();
        stmts.push(BuiltQuery::AlterTable(stmt));

        // Backfill with provided value
        if let Some(fill) = fill_with {
            let sql = format!("UPDATE \"{}\" SET \"{}\" = {}", table, column.name, fill);
            stmts.push(BuiltQuery::Raw(sql));
        }

        // Set NOT NULL
        let sql = format!(
            "ALTER TABLE \"{}\" ALTER COLUMN \"{}\" SET NOT NULL",
            table, column.name
        );
        stmts.push(BuiltQuery::Raw(sql));
    } else {
        let col_def = build_sea_column_def(column);
        let stmt = Table::alter()
            .table(Alias::new(table))
            .add_column(col_def)
            .to_owned();
        stmts.push(BuiltQuery::AlterTable(stmt));
    }

    Ok(stmts)
}

fn build_add_constraint(
    table: &str,
    constraint: &TableConstraint,
) -> Result<Vec<BuiltQuery>, QueryError> {
    match constraint {
        TableConstraint::PrimaryKey { columns, .. } => {
            // Use raw SQL for adding primary key constraint
            let cols = columns
                .iter()
                .map(|c| format!("\"{}\"", c))
                .collect::<Vec<_>>()
                .join(", ");
            let sql = format!("ALTER TABLE \"{}\" ADD PRIMARY KEY ({})", table, cols);
            Ok(vec![BuiltQuery::Raw(sql)])
        }
        TableConstraint::Unique { name, columns } => {
            let cols = columns
                .iter()
                .map(|c| format!("\"{}\"", c))
                .collect::<Vec<_>>()
                .join(", ");
            let sql = if let Some(n) = name {
                format!(
                    "ALTER TABLE \"{}\" ADD CONSTRAINT \"{}\" UNIQUE ({})",
                    table, n, cols
                )
            } else {
                format!("ALTER TABLE \"{}\" ADD UNIQUE ({})", table, cols)
            };
            Ok(vec![BuiltQuery::Raw(sql)])
        }
        TableConstraint::ForeignKey {
            name,
            columns,
            ref_table,
            ref_columns,
            on_delete,
            on_update,
        } => {
            let mut fk = ForeignKey::create();
            if let Some(n) = name {
                fk = fk.name(n).to_owned();
            }
            fk = fk.from_tbl(Alias::new(table)).to_owned();
            for col in columns {
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
            Ok(vec![BuiltQuery::CreateForeignKey(fk)])
        }
        TableConstraint::Check { name, expr } => {
            let sql = if let Some(n) = name {
                format!(
                    "ALTER TABLE \"{}\" ADD CONSTRAINT \"{}\" CHECK ({})",
                    table, n, expr
                )
            } else {
                format!("ALTER TABLE \"{}\" ADD CHECK ({})", table, expr)
            };
            Ok(vec![BuiltQuery::Raw(sql)])
        }
    }
}

fn build_remove_constraint(
    table: &str,
    constraint: &TableConstraint,
) -> Result<Vec<BuiltQuery>, QueryError> {
    match constraint {
        TableConstraint::PrimaryKey { .. } => {
            let sql = format!(
                "ALTER TABLE \"{}\" DROP CONSTRAINT \"{}_pkey\"",
                table, table
            );
            Ok(vec![BuiltQuery::Raw(sql)])
        }
        TableConstraint::Unique { name, columns } => {
            let constraint_name = if let Some(n) = name {
                n.clone()
            } else {
                format!("{}_{}_key", table, columns.join("_"))
            };
            let sql = format!(
                "ALTER TABLE \"{}\" DROP CONSTRAINT \"{}\"",
                table, constraint_name
            );
            Ok(vec![BuiltQuery::Raw(sql)])
        }
        TableConstraint::ForeignKey { name, columns, .. } => {
            let constraint_name = if let Some(n) = name {
                n.clone()
            } else {
                format!("{}_{}_fkey", table, columns.join("_"))
            };
            let stmt = ForeignKey::drop()
                .table(Alias::new(table))
                .name(&constraint_name)
                .to_owned();
            Ok(vec![BuiltQuery::DropForeignKey(stmt)])
        }
        TableConstraint::Check { name, .. } => {
            if let Some(n) = name {
                let sql = format!("ALTER TABLE \"{}\" DROP CONSTRAINT \"{}\"", table, n);
                Ok(vec![BuiltQuery::Raw(sql)])
            } else {
                Err(QueryError::Other(
                    "Cannot drop unnamed CHECK constraint".to_string(),
                ))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;
    use vespertide_core::IndexDef;

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
        MigrationAction::CreateTable {
            table: "users".into(),
            columns: vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                col("name", ColumnType::Simple(SimpleColumnType::Text)),
            ],
            constraints: vec![],
        },
        DatabaseBackend::Postgres,
        "CREATE TABLE \"users\" ( \"id\" integer, \"name\" text )"
    )]
    #[case::create_table_mysql(
        MigrationAction::CreateTable {
            table: "users".into(),
            columns: vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            ],
            constraints: vec![],
        },
        DatabaseBackend::MySql,
        "CREATE TABLE `users` ( `id` int )"
    )]
    #[case::create_table_sqlite(
        MigrationAction::CreateTable {
            table: "users".into(),
            columns: vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            ],
            constraints: vec![],
        },
        DatabaseBackend::Sqlite,
        "CREATE TABLE \"users\" ( \"id\" integer )"
    )]
    #[case::delete_table_postgres(
        MigrationAction::DeleteTable {
            table: "users".into(),
        },
        DatabaseBackend::Postgres,
        "DROP TABLE \"users\""
    )]
    #[case::delete_table_mysql(
        MigrationAction::DeleteTable {
            table: "users".into(),
        },
        DatabaseBackend::MySql,
        "DROP TABLE `users`"
    )]
    #[case::rename_column(
        MigrationAction::RenameColumn {
            table: "users".into(),
            from: "old_name".into(),
            to: "new_name".into(),
        },
        DatabaseBackend::Postgres,
        "ALTER TABLE \"users\" RENAME COLUMN \"old_name\" TO \"new_name\""
    )]
    #[case::delete_column(
        MigrationAction::DeleteColumn {
            table: "users".into(),
            column: "email".into(),
        },
        DatabaseBackend::Postgres,
        "ALTER TABLE \"users\" DROP COLUMN \"email\""
    )]
    #[case::add_index(
        MigrationAction::AddIndex {
            table: "users".into(),
            index: IndexDef {
                name: "idx_email".into(),
                columns: vec!["email".into()],
                unique: false,
            },
        },
        DatabaseBackend::Postgres,
        "CREATE INDEX \"idx_email\" ON \"users\" (\"email\")"
    )]
    #[case::add_unique_index(
        MigrationAction::AddIndex {
            table: "users".into(),
            index: IndexDef {
                name: "idx_email".into(),
                columns: vec!["email".into()],
                unique: true,
            },
        },
        DatabaseBackend::Postgres,
        "CREATE UNIQUE INDEX \"idx_email\" ON \"users\" (\"email\")"
    )]
    #[case::remove_index(
        MigrationAction::RemoveIndex {
            table: "users".into(),
            name: "idx_email".into(),
        },
        DatabaseBackend::Postgres,
        "DROP INDEX \"idx_email\""
    )]
    #[case::rename_table(
        MigrationAction::RenameTable {
            from: "old_users".into(),
            to: "new_users".into(),
        },
        DatabaseBackend::Postgres,
        "ALTER TABLE \"old_users\" RENAME TO \"new_users\""
    )]
    #[case::raw_sql(
        MigrationAction::RawSql {
            sql: "SELECT 1;".to_string(),
        },
        DatabaseBackend::Postgres,
        "SELECT 1;"
    )]
    fn test_build_action_queries(
        #[case] action: MigrationAction,
        #[case] backend: DatabaseBackend,
        #[case] expected_sql: &str,
    ) {
        let result = build_action_queries(&action).unwrap();
        assert!(!result.is_empty());
        let sql = result[0].build(backend);
        assert_eq!(sql, expected_sql);
    }

    #[test]
    fn test_add_column_nullable() {
        let action = MigrationAction::AddColumn {
            table: "users".into(),
            column: col("email", ColumnType::Simple(SimpleColumnType::Text)),
            fill_with: None,
        };
        let result = build_action_queries(&action).unwrap();
        assert_eq!(result.len(), 1);
        let sql = result[0].build(DatabaseBackend::Postgres);
        assert!(sql.contains("ALTER TABLE"));
        assert!(sql.contains("ADD COLUMN"));
        assert!(sql.contains("email"));
    }

    #[test]
    fn test_add_column_not_null_with_fill() {
        let mut c = col("email", ColumnType::Simple(SimpleColumnType::Text));
        c.nullable = false;
        let action = MigrationAction::AddColumn {
            table: "users".into(),
            column: c,
            fill_with: Some("'test@example.com'".to_string()),
        };
        let result = build_action_queries(&action).unwrap();
        assert_eq!(result.len(), 3);
        // First: add column as nullable
        // Second: UPDATE to backfill
        // Third: SET NOT NULL
    }

    #[test]
    fn test_modify_column_type() {
        let action = MigrationAction::ModifyColumnType {
            table: "users".into(),
            column: "age".into(),
            new_type: ColumnType::Simple(SimpleColumnType::BigInt),
        };
        let result = build_action_queries(&action).unwrap();
        assert_eq!(result.len(), 1);
        let sql = result[0].build(DatabaseBackend::Postgres);
        assert!(sql.contains("ALTER TABLE"));
        assert!(sql.contains("ALTER COLUMN") || sql.contains("MODIFY COLUMN"));
    }

    #[test]
    fn test_remove_constraint_check_unnamed_error() {
        let action = MigrationAction::RemoveConstraint {
            table: "users".into(),
            constraint: TableConstraint::Check {
                name: None,
                expr: "age > 0".into(),
            },
        };
        let result = build_action_queries(&action);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Cannot drop unnamed CHECK constraint")
        );
    }

    #[rstest]
    #[case(ColumnType::Simple(SimpleColumnType::Integer))]
    #[case(ColumnType::Simple(SimpleColumnType::BigInt))]
    #[case(ColumnType::Simple(SimpleColumnType::Text))]
    #[case(ColumnType::Simple(SimpleColumnType::Boolean))]
    #[case(ColumnType::Simple(SimpleColumnType::Timestamp))]
    #[case(ColumnType::Simple(SimpleColumnType::Uuid))]
    #[case(ColumnType::Complex(ComplexColumnType::Varchar { length: 255 }))]
    #[case(ColumnType::Complex(ComplexColumnType::Numeric { precision: 10, scale: 2 }))]
    fn test_column_type_conversion(#[case] ty: ColumnType) {
        // Just ensure no panic - test by creating a column with this type
        let mut col = SeaColumnDef::new(Alias::new("test"));
        apply_column_type(&mut col, &ty);
    }

    #[test]
    fn test_reference_action_conversion() {
        // Just ensure the function doesn't panic and returns valid ForeignKeyAction
        let _ = to_sea_fk_action(&ReferenceAction::Cascade);
        let _ = to_sea_fk_action(&ReferenceAction::Restrict);
        let _ = to_sea_fk_action(&ReferenceAction::SetNull);
        let _ = to_sea_fk_action(&ReferenceAction::SetDefault);
        let _ = to_sea_fk_action(&ReferenceAction::NoAction);
    }

    #[test]
    fn test_backend_specific_quoting() {
        let action = MigrationAction::CreateTable {
            table: "users".into(),
            columns: vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
            constraints: vec![],
        };
        let result = build_action_queries(&action).unwrap();

        // PostgreSQL uses double quotes
        let pg_sql = result[0].build(DatabaseBackend::Postgres);
        assert!(pg_sql.contains("\"users\""));

        // MySQL uses backticks
        let mysql_sql = result[0].build(DatabaseBackend::MySql);
        assert!(mysql_sql.contains("`users`"));

        // SQLite uses double quotes
        let sqlite_sql = result[0].build(DatabaseBackend::Sqlite);
        assert!(sqlite_sql.contains("\"users\""));
    }
}
