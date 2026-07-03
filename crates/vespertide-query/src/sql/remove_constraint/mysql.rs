use sea_query::Alias;

use vespertide_core::TableConstraint;

use crate::sql::helpers::quote_ident;
use crate::sql::types::BuiltQuery;
use crate::sql::{DatabaseBackend, RawSql};

pub fn build_remove_constraint(table: &str, constraint: &TableConstraint) -> Vec<BuiltQuery> {
    match constraint {
        TableConstraint::PrimaryKey { .. } => {
            let pg_table = quote_ident(table, DatabaseBackend::Postgres);
            let pg_pkey = quote_ident(&format!("{table}_pkey"), DatabaseBackend::Postgres);
            let mysql_table = quote_ident(table, DatabaseBackend::MySql);
            let pg_sql = format!("ALTER TABLE {pg_table} DROP CONSTRAINT {pg_pkey}");
            vec![BuiltQuery::Raw(RawSql::per_backend(
                pg_sql.clone(),
                format!("ALTER TABLE {mysql_table} DROP PRIMARY KEY"),
                pg_sql,
            ))]
        }
        TableConstraint::Unique { name, columns, .. } => {
            let constraint_name =
                vespertide_naming::build_unique_constraint_name(table, columns, name.as_deref());
            let pg_constraint = quote_ident(&constraint_name, DatabaseBackend::Postgres);
            let mysql_table = quote_ident(table, DatabaseBackend::MySql);
            let mysql_constraint = quote_ident(&constraint_name, DatabaseBackend::MySql);
            let pg_sql = format!("DROP INDEX {pg_constraint}");
            vec![BuiltQuery::Raw(RawSql::per_backend(
                pg_sql.clone(),
                format!("ALTER TABLE {mysql_table} DROP INDEX {mysql_constraint}"),
                pg_sql,
            ))]
        }
        TableConstraint::ForeignKey { name, columns, .. } => {
            let constraint_name =
                vespertide_naming::build_foreign_key_name(table, columns, name.as_deref());
            let pg_table = quote_ident(table, DatabaseBackend::Postgres);
            let pg_constraint = quote_ident(&constraint_name, DatabaseBackend::Postgres);
            let mysql_table = quote_ident(table, DatabaseBackend::MySql);
            let mysql_constraint = quote_ident(&constraint_name, DatabaseBackend::MySql);
            let pg_sql = format!("ALTER TABLE {pg_table} DROP CONSTRAINT {pg_constraint}");
            vec![BuiltQuery::Raw(RawSql::per_backend(
                pg_sql.clone(),
                format!("ALTER TABLE {mysql_table} DROP FOREIGN KEY {mysql_constraint}"),
                pg_sql,
            ))]
        }
        TableConstraint::Index { name, columns } => {
            let index_name = vespertide_naming::build_index_name(table, columns, name.as_deref());
            let idx_drop = sea_query::Index::drop()
                .table(Alias::new(table))
                .name(&index_name)
                .to_owned();
            vec![BuiltQuery::DropIndex(Box::new(idx_drop))]
        }
        TableConstraint::Check { name, .. } => {
            let pg_table = quote_ident(table, DatabaseBackend::Postgres);
            let pg_name = quote_ident(name, DatabaseBackend::Postgres);
            let mysql_table = quote_ident(table, DatabaseBackend::MySql);
            let mysql_name = quote_ident(name, DatabaseBackend::MySql);
            let pg_sql = format!("ALTER TABLE {pg_table} DROP CONSTRAINT {pg_name}");
            vec![BuiltQuery::Raw(RawSql::per_backend(
                pg_sql.clone(),
                format!("ALTER TABLE {mysql_table} DROP CHECK {mysql_name}"),
                pg_sql,
            ))]
        }
        _ => unreachable!("TableConstraint is #[non_exhaustive]; all variants are matched above"),
    }
}
