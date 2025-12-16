use sea_query::{Alias, ColumnDef as SeaColumnDef, ForeignKey, Index, Table, TableCreateStatement};

use vespertide_core::{ColumnDef, TableConstraint};

use crate::error::QueryError;
use super::types::{BuiltQuery, DatabaseBackend};
use super::helpers::{apply_column_type, build_sea_column_def, to_sea_fk_action};

fn build_create_table_for_backend(
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

    stmt
}

pub fn build_create_table(
    backend: &DatabaseBackend,
    table: &str,
    columns: &[ColumnDef],
    constraints: &[TableConstraint],
) -> Result<BuiltQuery, QueryError> {
    Ok(BuiltQuery::CreateTable(
        Box::new(build_create_table_for_backend(backend, table, columns, constraints)),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use insta::{assert_snapshot, with_settings};
    use rstest::rstest;
    use vespertide_core::schema::primary_key::PrimaryKeySyntax;
    use vespertide_core::{
        ColumnType, ComplexColumnType, IndexDef, SimpleColumnType, StrOrBoolOrArray,
    };

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
        &["CREATE TABLE \"users\" ( \"id\" integer, \"name\" text )"]
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
    fn test_create_table(#[case] title: &str, #[case] backend: DatabaseBackend, #[case] expected: &[&str]) {
        let result = build_create_table(
            &backend,
            "users",
            &[col("id", ColumnType::Simple(SimpleColumnType::Integer))],
            &[],
        )
        .unwrap();
        let sql = result.build(backend);
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
}
