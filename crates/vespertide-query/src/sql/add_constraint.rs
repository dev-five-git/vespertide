use sea_query::{Alias, ForeignKey, Index};

use vespertide_core::TableConstraint;

use super::helpers::to_sea_fk_action;
use super::types::BuiltQuery;
use crate::error::QueryError;
use crate::sql::RawSql;

pub fn build_add_constraint(
    table: &str,
    constraint: &TableConstraint,
) -> Result<Vec<BuiltQuery>, QueryError> {
    match constraint {
        TableConstraint::PrimaryKey { columns, .. } => {
            // sea_query 0.32 doesn't support adding primary key via Table::alter() directly
            // We'll use Index::create().primary() which creates a primary key index
            // Note: This generates CREATE UNIQUE INDEX, not ALTER TABLE ADD PRIMARY KEY
            // but it's functionally equivalent for most databases
            let mut pk_idx = Index::create()
                .table(Alias::new(table))
                .primary()
                .to_owned();
            for col in columns {
                pk_idx = pk_idx.col(Alias::new(col)).to_owned();
            }
            Ok(vec![BuiltQuery::CreateIndex(Box::new(pk_idx))])
        }
        TableConstraint::Unique { name, columns } => {
            // Build unique constraint using Index::create().unique()
            let mut idx = Index::create().table(Alias::new(table)).unique().to_owned();
            if let Some(n) = name {
                idx = idx.name(n).to_owned();
            }
            for col in columns {
                idx = idx.col(Alias::new(col)).to_owned();
            }
            Ok(vec![BuiltQuery::CreateIndex(Box::new(idx))])
        }
        TableConstraint::ForeignKey {
            name,
            columns,
            ref_table,
            ref_columns,
            on_delete,
            on_update,
        } => {
            // Build foreign key using ForeignKey::create
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
            let pg_sql = format!(
                "ALTER TABLE \"{}\" ADD CONSTRAINT \"{}\" CHECK ({})",
                table, name, expr
            );
            let mysql_sql = format!(
                "ALTER TABLE `{}` ADD CONSTRAINT `{}` CHECK ({})",
                table, name, expr
            );
            Ok(vec![BuiltQuery::Raw(RawSql::per_backend(
                pg_sql.clone(),
                mysql_sql,
                pg_sql,
            ))])
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sql::types::DatabaseBackend;
    use insta::{assert_snapshot, with_settings};
    use rstest::rstest;
    use vespertide_core::{ReferenceAction, TableConstraint};

    #[rstest]
    #[case::add_constraint_primary_key_postgres(
        "add_constraint_primary_key_postgres",
        DatabaseBackend::Postgres,
        &["ALTER TABLE \"users\" ADD PRIMARY KEY (\"id\")"]
    )]
    #[case::add_constraint_primary_key_mysql(
        "add_constraint_primary_key_mysql",
        DatabaseBackend::MySql,
        &["ALTER TABLE `users` ADD PRIMARY KEY (`id`)"]
    )]
    #[case::add_constraint_primary_key_sqlite(
        "add_constraint_primary_key_sqlite",
        DatabaseBackend::Sqlite,
        &["ALTER TABLE \"users\" ADD PRIMARY KEY (\"id\")"]
    )]
    #[case::add_constraint_unique_named_postgres(
        "add_constraint_unique_named_postgres",
        DatabaseBackend::Postgres,
        &["ADD CONSTRAINT \"uq_email\" UNIQUE (\"email\")"]
    )]
    #[case::add_constraint_unique_named_mysql(
        "add_constraint_unique_named_mysql",
        DatabaseBackend::MySql,
        &["ADD CONSTRAINT `uq_email` UNIQUE (`email`)"]
    )]
    #[case::add_constraint_unique_named_sqlite(
        "add_constraint_unique_named_sqlite",
        DatabaseBackend::Sqlite,
        &["ADD CONSTRAINT \"uq_email\" UNIQUE (\"email\")"]
    )]
    #[case::add_constraint_foreign_key_postgres(
        "add_constraint_foreign_key_postgres",
        DatabaseBackend::Postgres,
        &["FOREIGN KEY (\"user_id\")", "REFERENCES \"users\" (\"id\")", "ON DELETE CASCADE", "ON UPDATE RESTRICT"]
    )]
    #[case::add_constraint_foreign_key_mysql(
        "add_constraint_foreign_key_mysql",
        DatabaseBackend::MySql,
        &["FOREIGN KEY (`user_id`)", "REFERENCES `users` (`id`)", "ON DELETE CASCADE", "ON UPDATE RESTRICT"]
    )]
    #[case::add_constraint_foreign_key_sqlite(
        "add_constraint_foreign_key_sqlite",
        DatabaseBackend::Sqlite,
        &["FOREIGN KEY (\"user_id\")", "REFERENCES \"users\" (\"id\")", "ON DELETE CASCADE", "ON UPDATE RESTRICT"]
    )]
    #[case::add_constraint_check_named_postgres(
        "add_constraint_check_named_postgres",
        DatabaseBackend::Postgres,
        &["ADD CONSTRAINT \"chk_age\" CHECK (age > 0)"]
    )]
    #[case::add_constraint_check_named_mysql(
        "add_constraint_check_named_mysql",
        DatabaseBackend::MySql,
        &["ADD CONSTRAINT `chk_age` CHECK (age > 0)"]
    )]
    #[case::add_constraint_check_named_sqlite(
        "add_constraint_check_named_sqlite",
        DatabaseBackend::Sqlite,
        &["ADD CONSTRAINT \"chk_age\" CHECK (age > 0)"]
    )]
    fn test_add_constraint(
        #[case] title: &str,
        #[case] backend: DatabaseBackend,
        #[case] expected: &[&str],
    ) {
        let constraint = if title.contains("primary_key") {
            TableConstraint::PrimaryKey {
                columns: vec!["id".into()],
                auto_increment: false,
            }
        } else if title.contains("unique") {
            TableConstraint::Unique {
                name: Some("uq_email".into()),
                columns: vec!["email".into()],
            }
        } else if title.contains("foreign_key") {
            TableConstraint::ForeignKey {
                name: Some("fk_user".into()),
                columns: vec!["user_id".into()],
                ref_table: "users".into(),
                ref_columns: vec!["id".into()],
                on_delete: Some(ReferenceAction::Cascade),
                on_update: Some(ReferenceAction::Restrict),
            }
        } else {
            TableConstraint::Check {
                name: "chk_age".into(),
                expr: "age > 0".into(),
            }
        };
        let result = build_add_constraint("users", &constraint).unwrap();
        let sql = result[0].build(backend);
        for exp in expected {
            assert!(
                sql.contains(exp),
                "Expected SQL to contain '{}', got: {}",
                exp,
                sql
            );
        }

        with_settings!({ snapshot_suffix => format!("add_constraint_{}", title) }, {
            assert_snapshot!(result.iter().map(|q| q.build(backend)).collect::<Vec<String>>().join("\n"));
        });
    }
}
