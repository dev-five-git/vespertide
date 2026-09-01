use vespertide_core::TableConstraint;

use crate::sql::helpers::quote_ident;
use crate::sql::types::BuiltQuery;
use crate::sql::{DatabaseBackend, RawSql};

pub fn build_remove_constraint(table: &str, constraint: &TableConstraint) -> Vec<BuiltQuery> {
    match constraint {
        TableConstraint::PrimaryKey { .. } => {
            let mysql_table = quote_ident(table, DatabaseBackend::MySql);
            vec![BuiltQuery::Raw(RawSql::uniform(format!(
                "ALTER TABLE {mysql_table} DROP PRIMARY KEY"
            )))]
        }
        TableConstraint::Unique { name, columns, .. } => {
            let constraint_name =
                vespertide_naming::build_unique_constraint_name(table, columns, name.as_deref());
            let mysql_table = quote_ident(table, DatabaseBackend::MySql);
            let mysql_constraint = quote_ident(&constraint_name, DatabaseBackend::MySql);
            vec![BuiltQuery::Raw(RawSql::uniform(format!(
                "ALTER TABLE {mysql_table} DROP INDEX {mysql_constraint}"
            )))]
        }
        TableConstraint::ForeignKey { name, columns, .. } => {
            let constraint_name =
                vespertide_naming::build_foreign_key_name(table, columns, name.as_deref());
            let mysql_table = quote_ident(table, DatabaseBackend::MySql);
            let mysql_constraint = quote_ident(&constraint_name, DatabaseBackend::MySql);
            vec![BuiltQuery::Raw(RawSql::uniform(format!(
                "ALTER TABLE {mysql_table} DROP FOREIGN KEY {mysql_constraint}"
            )))]
        }
        TableConstraint::Index { name, columns } => {
            let index_name = vespertide_naming::build_index_name(table, columns, name.as_deref());
            vec![super::build_drop_index_query(table, &index_name)]
        }
        TableConstraint::Check { name, .. } => {
            let mysql_table = quote_ident(table, DatabaseBackend::MySql);
            let mysql_name = quote_ident(name, DatabaseBackend::MySql);
            vec![BuiltQuery::Raw(RawSql::uniform(format!(
                "ALTER TABLE {mysql_table} DROP CHECK {mysql_name}"
            )))]
        }
        _ => unreachable!("TableConstraint is #[non_exhaustive]; all variants are matched above"),
    }
}
