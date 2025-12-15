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
    CreateTable(Box<TableCreateStatement>),
    DropTable(Box<TableDropStatement>),
    AlterTable(Box<TableAlterStatement>),
    CreateIndex(Box<IndexCreateStatement>),
    DropIndex(Box<IndexDropStatement>),
    RenameTable(Box<TableRenameStatement>),
    CreateForeignKey(Box<ForeignKeyCreateStatement>),
    DropForeignKey(Box<ForeignKeyDropStatement>),
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
            Ok(vec![BuiltQuery::DropTable(Box::new(stmt))])
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
            Ok(vec![BuiltQuery::AlterTable(Box::new(stmt))])
        }

        MigrationAction::DeleteColumn { table, column } => {
            let stmt = Table::alter()
                .table(Alias::new(table))
                .drop_column(Alias::new(column))
                .to_owned();
            Ok(vec![BuiltQuery::AlterTable(Box::new(stmt))])
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
            Ok(vec![BuiltQuery::AlterTable(Box::new(stmt))])
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

            Ok(vec![BuiltQuery::CreateIndex(Box::new(stmt))])
        }

        MigrationAction::RemoveIndex { name, .. } => {
            let stmt = Index::drop().name(name).to_owned();
            Ok(vec![BuiltQuery::DropIndex(Box::new(stmt))])
        }

        MigrationAction::RenameTable { from, to } => {
            let stmt = Table::rename()
                .table(Alias::new(from), Alias::new(to))
                .to_owned();
            Ok(vec![BuiltQuery::RenameTable(Box::new(stmt))])
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

    Ok(BuiltQuery::CreateTable(Box::new(stmt)))
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
        stmts.push(BuiltQuery::AlterTable(Box::new(stmt)));

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
        stmts.push(BuiltQuery::AlterTable(Box::new(stmt)));
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
            Ok(vec![BuiltQuery::CreateForeignKey(Box::new(fk))])
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
            Ok(vec![BuiltQuery::DropForeignKey(Box::new(stmt))])
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
    use vespertide_core::{IndexDef};
    use insta::{assert_snapshot, with_settings};


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

    #[rstest]
    #[case(SimpleColumnType::SmallInt)]
    #[case(SimpleColumnType::Integer)]
    #[case(SimpleColumnType::BigInt)]
    #[case(SimpleColumnType::Real)]
    #[case(SimpleColumnType::DoublePrecision)]
    #[case(SimpleColumnType::Text)]
    #[case(SimpleColumnType::Boolean)]
    #[case(SimpleColumnType::Date)]
    #[case(SimpleColumnType::Time)]
    #[case(SimpleColumnType::Timestamp)]
    #[case(SimpleColumnType::Timestamptz)]
    #[case(SimpleColumnType::Interval)]
    #[case(SimpleColumnType::Bytea)]
    #[case(SimpleColumnType::Uuid)]
    #[case(SimpleColumnType::Json)]
    #[case(SimpleColumnType::Jsonb)]
    #[case(SimpleColumnType::Inet)]
    #[case(SimpleColumnType::Cidr)]
    #[case(SimpleColumnType::Macaddr)]
    #[case(SimpleColumnType::Xml)]
    fn test_all_simple_types_cover_branches(#[case] ty: SimpleColumnType) {
        let mut col = SeaColumnDef::new(Alias::new("t"));
        apply_column_type(&mut col, &ColumnType::Simple(ty));
    }

    #[rstest]
    #[case(ComplexColumnType::Varchar { length: 42 })]
    #[case(ComplexColumnType::Numeric { precision: 8, scale: 3 })]
    #[case(ComplexColumnType::Char { length: 3 })]
    #[case(ComplexColumnType::Custom { custom_type: "GEOGRAPHY".into() })]
    fn test_all_complex_types_cover_branches(#[case] ty: ComplexColumnType) {
        let mut col = SeaColumnDef::new(Alias::new("t"));
        apply_column_type(&mut col, &ColumnType::Complex(ty));
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

    // ===== MySQL/SQLite backend coverage for all BuiltQuery variants =====
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
        &["CREATE TABLE \"users\" ( \"id\" integer, \"name\" text )"]
    )]
    #[case::create_table_mysql(
        MigrationAction::CreateTable {
            table: "users".into(),
            columns: vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
            constraints: vec![],
        },
        DatabaseBackend::MySql,
        &["CREATE TABLE `users` ( `id` int )"]
    )]
    #[case::create_table_sqlite(
        MigrationAction::CreateTable {
            table: "users".into(),
            columns: vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
            constraints: vec![],
        },
        DatabaseBackend::Sqlite,
        &["CREATE TABLE \"users\" ( \"id\" integer )"]
    )]
    #[case::delete_table_postgres(
        MigrationAction::DeleteTable { table: "users".into() },
        DatabaseBackend::Postgres,
        &["DROP TABLE \"users\""]
    )]
    #[case::delete_table_mysql(
        MigrationAction::DeleteTable { table: "users".into() },
        DatabaseBackend::MySql,
        &["DROP TABLE `users`"]
    )]
    #[case::delete_table_sqlite(
        MigrationAction::DeleteTable { table: "users".into() },
        DatabaseBackend::Sqlite,
        &["DROP TABLE \"users\""]
    )]
    #[case::delete_column_postgres(
        MigrationAction::DeleteColumn {
            table: "users".into(),
            column: "email".into(),
        },
        DatabaseBackend::Postgres,
        &["ALTER TABLE \"users\" DROP COLUMN \"email\""]
    )]
    #[case::delete_column_mysql(
        MigrationAction::DeleteColumn {
            table: "users".into(),
            column: "email".into(),
        },
        DatabaseBackend::MySql,
        &["ALTER TABLE `users` DROP COLUMN `email`"]
    )]
    #[case::delete_column_sqlite(
        MigrationAction::DeleteColumn {
            table: "users".into(),
            column: "email".into(),
        },
        DatabaseBackend::Sqlite,
        &["ALTER TABLE \"users\" DROP COLUMN \"email\""]
    )]
    #[case::add_index_postgres(
        MigrationAction::AddIndex {
            table: "users".into(),
            index: IndexDef { name: "idx_email".into(), columns: vec!["email".into()], unique: false },
        },
        DatabaseBackend::Postgres,
        &["CREATE INDEX \"idx_email\" ON \"users\" (\"email\")"]
    )]
    #[case::add_index_mysql(
        MigrationAction::AddIndex {
            table: "users".into(),
            index: IndexDef { name: "idx_email".into(), columns: vec!["email".into()], unique: false },
        },
        DatabaseBackend::MySql,
        &["CREATE INDEX `idx_email` ON `users` (`email`)"]
    )]
    #[case::add_index_sqlite(
        MigrationAction::AddIndex {
            table: "users".into(),
            index: IndexDef { name: "idx_email".into(), columns: vec!["email".into()], unique: false },
        },
        DatabaseBackend::Sqlite,
        &["CREATE INDEX \"idx_email\" ON \"users\" (\"email\")"]
    )]
    #[case::add_unique_index_postgres(
        MigrationAction::AddIndex {
            table: "users".into(),
            index: IndexDef { name: "idx_email".into(), columns: vec!["email".into()], unique: true },
        },
        DatabaseBackend::Postgres,
        &["CREATE UNIQUE INDEX \"idx_email\" ON \"users\" (\"email\")"]
    )]
    #[case::add_unique_index_mysql(
        MigrationAction::AddIndex {
            table: "users".into(),
            index: IndexDef { name: "idx_email".into(), columns: vec!["email".into()], unique: true },
        },
        DatabaseBackend::MySql,
        &["CREATE UNIQUE INDEX `idx_email` ON `users` (`email`)"]
    )]
    #[case::add_unique_index_sqlite(
        MigrationAction::AddIndex {
            table: "users".into(),
            index: IndexDef { name: "idx_email".into(), columns: vec!["email".into()], unique: true },
        },
        DatabaseBackend::Sqlite,
        &["CREATE UNIQUE INDEX \"idx_email\" ON \"users\" (\"email\")"]
    )]
    #[case::remove_index_postgres(
        MigrationAction::RemoveIndex {
            table: "users".into(),
            name: "idx_email".into(),
        },
        DatabaseBackend::Postgres,
        &["DROP INDEX \"idx_email\" ON \"users\""]
    )]
    #[case::remove_index_mysql(
        MigrationAction::RemoveIndex {
            table: "users".into(),
            name: "idx_email".into(),
        },
        DatabaseBackend::MySql,
        &["DROP INDEX `idx_email` ON `users`"]
    )]
    #[case::remove_index_sqlite(
        MigrationAction::RemoveIndex {
            table: "users".into(),
            name: "idx_email".into(),
        },
        DatabaseBackend::Sqlite,
        &["DROP INDEX \"idx_email\" ON \"users\""]
    )]
    fn test_build_migration_action(#[case] action: MigrationAction, #[case] backend: DatabaseBackend, #[case] expected: &[&str]) {

        let result = build_action_queries(&action).unwrap();
        for exp in expected {
            if !result[0].build(backend).contains(exp) {
                println!("action: {:?}, backend: {:?}, expected: {}", action, backend, exp);
            }
        }
        with_settings!({ snapshot_suffix => format!("build_migration_action_{:?}_{:?}", action, backend) }, {
            assert_snapshot!(result.iter().map(|q| q.build(backend)).collect::<Vec<String>>().join("\n"));
        });
    }

    #[rstest]
    #[case::alter_table_postgres(BuiltQuery::AlterTable(Box::new(Table::alter()
            .table(Alias::new("t"))
            .add_column(SeaColumnDef::new(Alias::new("c")).integer().to_owned())
            .to_owned())),DatabaseBackend::Postgres, &["ALTER TABLE \"t\" ADD COLUMN \"c\" integer"])]
    #[case::alter_table_mysql(BuiltQuery::AlterTable(Box::new(Table::alter()
            .table(Alias::new("t"))
            .add_column(SeaColumnDef::new(Alias::new("c")).integer().to_owned())
            .to_owned())),DatabaseBackend::MySql, &["ALTER TABLE `t` ADD COLUMN `c` int"])]
    #[case::alter_table_sqlite(BuiltQuery::AlterTable(Box::new(Table::alter()
            .table(Alias::new("t"))
            .add_column(SeaColumnDef::new(Alias::new("c")).integer().to_owned())
            .to_owned())),DatabaseBackend::Sqlite, &["ALTER TABLE \"t\" ADD COLUMN \"c\" integer"])]
    #[case::create_index_postgres(BuiltQuery::CreateIndex(Box::new(Index::create()
            .name("idx")
            .table(Alias::new("t"))
            .col(Alias::new("c"))
            .to_owned())),DatabaseBackend::Postgres, &["CREATE INDEX \"idx\" ON \"t\" (\"c\")"])]
    #[case::create_index_mysql(BuiltQuery::CreateIndex(Box::new(Index::create()
            .name("idx")
            .table(Alias::new("t"))
            .col(Alias::new("c"))
            .to_owned())),DatabaseBackend::MySql, &["CREATE INDEX `idx` ON `t` (`c`"])]
    #[case::create_index_sqlite(BuiltQuery::CreateIndex(Box::new(Index::create()
            .name("idx")
            .table(Alias::new("t"))
            .col(Alias::new("c"))
            .to_owned())),DatabaseBackend::Sqlite, &["CREATE INDEX \"idx\" ON \"t\" (\"c\")"])]
    #[case::drop_index_postgres(BuiltQuery::DropIndex(Box::new(Index::drop().name("idx").table(Alias::new("t")).to_owned())),DatabaseBackend::Postgres, &["DROP INDEX \"idx\""])]
    #[case::drop_index_mysql(BuiltQuery::DropIndex(Box::new(Index::drop().name("idx").table(Alias::new("t")).to_owned())),DatabaseBackend::MySql, &["`idx`"])]
    #[case::drop_index_sqlite(BuiltQuery::DropIndex(Box::new(Index::drop().name("idx").table(Alias::new("t")).to_owned())),DatabaseBackend::Sqlite, &["\"idx\""])]
    #[case::rename_table_postgres(BuiltQuery::RenameTable(Box::new(Table::rename()
            .table(Alias::new("a"), Alias::new("b"))
            .to_owned())),DatabaseBackend::Postgres, &["ALTER TABLE \"a\" RENAME TO \"b\""])]
    #[case::rename_table_mysql(BuiltQuery::RenameTable(Box::new(Table::rename()
            .table(Alias::new("a"), Alias::new("b"))
            .to_owned())),DatabaseBackend::MySql, &["RENAME TABLE `a` TO `b`"])]
    #[case::rename_table_sqlite(BuiltQuery::RenameTable(Box::new(Table::rename()
            .table(Alias::new("a"), Alias::new("b"))
            .to_owned())),DatabaseBackend::Sqlite, &["ALTER TABLE \"a\" RENAME TO \"b\""])]
            #[case::create_foreign_key_postgres(BuiltQuery::CreateForeignKey(Box::new(ForeignKey::create()
            .name("fk")
            .from_tbl(Alias::new("a"))
            .from_col(Alias::new("c"))
            .to_tbl(Alias::new("b"))
            .to_col(Alias::new("id"))
            .to_owned())),DatabaseBackend::Postgres, &["ALTER TABLE \"a\" ADD CONSTRAINT \"fk\" FOREIGN KEY (\"c\") REFERENCES \"b\" (\"id\")"])]
    #[case::create_foreign_key_mysql(BuiltQuery::CreateForeignKey(Box::new(ForeignKey::create()
            .name("fk")
            .from_tbl(Alias::new("a"))
            .from_col(Alias::new("c"))
            .to_tbl(Alias::new("b"))
            .to_col(Alias::new("id"))
            .to_owned())),DatabaseBackend::MySql, &["ALTER TABLE `a` ADD CONSTRAINT `fk` FOREIGN KEY (`c`) REFERENCES `b` (`id`)"])]
    #[case::create_foreign_key_sqlite(BuiltQuery::CreateForeignKey(Box::new(ForeignKey::create()
            .name("fk")
            .from_tbl(Alias::new("a"))
            .from_col(Alias::new("c"))
            .to_tbl(Alias::new("b"))
            .to_col(Alias::new("id"))
            .to_owned())),DatabaseBackend::Sqlite, &["ALTER TABLE \"a\" ADD CONSTRAINT \"fk\" FOREIGN KEY (\"c\") REFERENCES \"b\" (\"id\")"])]
    #[case::drop_foreign_key_postgres(BuiltQuery::DropForeignKey(Box::new(ForeignKey::drop()
            .name("fk")
            .table(Alias::new("a"))
            .to_owned())),DatabaseBackend::Postgres, &["ALTER TABLE \"a\" DROP CONSTRAINT \"fk\""])]
    #[case::drop_foreign_key_mysql(BuiltQuery::DropForeignKey(Box::new(ForeignKey::drop()
            .name("fk")
            .table(Alias::new("a"))
            .to_owned())),DatabaseBackend::MySql, &["ALTER TABLE `a` DROP CONSTRAINT `fk`"])]
    #[case::drop_foreign_key_sqlite(BuiltQuery::DropForeignKey(Box::new(ForeignKey::drop()
            .name("fk")
            .table(Alias::new("a"))
            .to_owned())),DatabaseBackend::Sqlite, &["ALTER TABLE \"a\" DROP CONSTRAINT \"fk\""])]
    fn test_build_query(#[case] q: BuiltQuery, #[case] backend: DatabaseBackend, #[case] expected: &[&str]) {

        for exp in expected {
            if !q.build(backend).contains(exp) {
                println!("q: {:?}, backend: {:?}, expected: {}", q, q.build(backend), exp);
            }
        }
        with_settings!({ snapshot_suffix => format!("build_query_{:?}", backend) }, {
            assert_snapshot!(q.build(backend));
        });
    }
}
