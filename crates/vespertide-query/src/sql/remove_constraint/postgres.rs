use sea_query::{Alias, ForeignKey};

use vespertide_core::TableConstraint;

use crate::sql::helpers::quote_ident;
use crate::sql::types::BuiltQuery;
use crate::sql::{DatabaseBackend, RawSql};

pub fn build_remove_constraint(table: &str, constraint: &TableConstraint) -> Vec<BuiltQuery> {
    match constraint {
        TableConstraint::PrimaryKey { .. } => {
            let pg_table = quote_ident(table, DatabaseBackend::Postgres);
            let pg_pkey = quote_ident(&format!("{table}_pkey"), DatabaseBackend::Postgres);
            vec![BuiltQuery::Raw(RawSql::uniform(format!(
                "ALTER TABLE {pg_table} DROP CONSTRAINT {pg_pkey}"
            )))]
        }
        TableConstraint::Unique { name, columns, .. } => {
            let constraint_name =
                vespertide_naming::build_unique_constraint_name(table, columns, name.as_deref());
            let pg_constraint = quote_ident(&constraint_name, DatabaseBackend::Postgres);
            vec![BuiltQuery::Raw(RawSql::uniform(format!(
                "DROP INDEX {pg_constraint}"
            )))]
        }
        TableConstraint::ForeignKey { name, columns, .. } => {
            let constraint_name =
                vespertide_naming::build_foreign_key_name(table, columns, name.as_deref());
            let fk_drop = ForeignKey::drop()
                .name(&constraint_name)
                .table(Alias::new(table))
                .to_owned();
            vec![BuiltQuery::DropForeignKey(Box::new(fk_drop))]
        }
        TableConstraint::Index { name, columns } => {
            let index_name = vespertide_naming::build_index_name(table, columns, name.as_deref());
            vec![super::build_drop_index_query(table, &index_name)]
        }
        TableConstraint::Check { name, .. } => {
            let pg_table = quote_ident(table, DatabaseBackend::Postgres);
            let pg_name = quote_ident(name, DatabaseBackend::Postgres);
            vec![BuiltQuery::Raw(RawSql::uniform(format!(
                "ALTER TABLE {pg_table} DROP CONSTRAINT {pg_name}"
            )))]
        }
        _ => unreachable!("TableConstraint is #[non_exhaustive]; all variants are matched above"),
    }
}
