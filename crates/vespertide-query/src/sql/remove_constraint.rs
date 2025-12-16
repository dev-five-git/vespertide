use vespertide_core::TableConstraint;

use crate::error::QueryError;
use super::types::{BuiltQuery, DatabaseBackend, RawSql};

pub fn build_remove_constraint(
    table: &str,
    constraint: &TableConstraint,
) -> Result<Vec<BuiltQuery>, QueryError> {
    match constraint {
        TableConstraint::PrimaryKey { .. } => {
            let pg_sql = format!(
                "ALTER TABLE \"{}\" DROP CONSTRAINT \"{}_pkey\"",
                table, table
            );
            let mysql_sql = format!("ALTER TABLE `{}` DROP PRIMARY KEY", table);
            Ok(vec![BuiltQuery::Raw(RawSql::per_backend(
                pg_sql.clone(),
                mysql_sql,
                pg_sql,
            ))])
        }
        TableConstraint::Unique { name, columns } => {
            let constraint_name = if let Some(n) = name {
                n.clone()
            } else {
                format!("{}_{}_key", table, columns.join("_"))
            };
            let pg_sql = format!(
                "ALTER TABLE \"{}\" DROP CONSTRAINT \"{}\"",
                table, constraint_name
            );
            let mysql_sql = format!("ALTER TABLE `{}` DROP INDEX `{}`", table, constraint_name);
            Ok(vec![BuiltQuery::Raw(RawSql::per_backend(
                pg_sql.clone(),
                mysql_sql,
                pg_sql,
            ))])
        }
        TableConstraint::ForeignKey { name, columns, .. } => {
            // Use Raw SQL to avoid SQLite panic from sea-query
            // SQLite doesn't support ALTER TABLE DROP CONSTRAINT for FK
            let constraint_name = if let Some(n) = name {
                n.clone()
            } else {
                format!("{}_{}_fkey", table, columns.join("_"))
            };
            let pg_sql = format!(
                "ALTER TABLE \"{}\" DROP CONSTRAINT \"{}\"",
                table, constraint_name
            );
            let mysql_sql = format!(
                "ALTER TABLE `{}` DROP FOREIGN KEY `{}`",
                table, constraint_name
            );
            Ok(vec![BuiltQuery::Raw(RawSql::per_backend(
                pg_sql.clone(),
                mysql_sql,
                pg_sql,
            ))])
        }
        TableConstraint::Check { name, .. } => {
            let pg_sql = format!("ALTER TABLE \"{}\" DROP CONSTRAINT \"{}\"", table, name);
            let mysql_sql = format!("ALTER TABLE `{}` DROP CHECK `{}`", table, name);
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
    use insta::{assert_snapshot, with_settings};
    use rstest::rstest;
    use vespertide_core::TableConstraint;

    #[rstest]
    #[case::remove_constraint_primary_key_postgres(
        "remove_constraint_primary_key_postgres",
        DatabaseBackend::Postgres,
        &["DROP CONSTRAINT \"users_pkey\""]
    )]
    #[case::remove_constraint_primary_key_mysql(
        "remove_constraint_primary_key_mysql",
        DatabaseBackend::MySql,
        &["DROP PRIMARY KEY"]
    )]
    #[case::remove_constraint_primary_key_sqlite(
        "remove_constraint_primary_key_sqlite",
        DatabaseBackend::Sqlite,
        &["DROP CONSTRAINT \"users_pkey\""]
    )]
    #[case::remove_constraint_unique_named_postgres(
        "remove_constraint_unique_named_postgres",
        DatabaseBackend::Postgres,
        &["DROP CONSTRAINT \"uq_email\""]
    )]
    #[case::remove_constraint_unique_named_mysql(
        "remove_constraint_unique_named_mysql",
        DatabaseBackend::MySql,
        &["DROP INDEX `uq_email`"]
    )]
    #[case::remove_constraint_foreign_key_named_postgres(
        "remove_constraint_foreign_key_named_postgres",
        DatabaseBackend::Postgres,
        &["DROP CONSTRAINT \"fk_user\""]
    )]
    #[case::remove_constraint_foreign_key_named_mysql(
        "remove_constraint_foreign_key_named_mysql",
        DatabaseBackend::MySql,
        &["DROP FOREIGN KEY `fk_user`"]
    )]
    #[case::remove_constraint_check_named_postgres(
        "remove_constraint_check_named_postgres",
        DatabaseBackend::Postgres,
        &["DROP CONSTRAINT \"chk_age\""]
    )]
    #[case::remove_constraint_check_named_mysql(
        "remove_constraint_check_named_mysql",
        DatabaseBackend::MySql,
        &["DROP CHECK `chk_age`"]
    )]
    fn test_remove_constraint(
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
                on_delete: None,
                on_update: None,
            }
        } else {
            TableConstraint::Check {
                name: "chk_age".into(),
                expr: "age > 0".into(),
            }
        };
        let result = build_remove_constraint("users", &constraint).unwrap();
        let sql = result[0].build(backend);
        for exp in expected {
            assert!(
                sql.contains(exp),
                "Expected SQL to contain '{}', got: {}",
                exp,
                sql
            );
        }

        with_settings!({ snapshot_suffix => format!("remove_constraint_{}", title) }, {
            assert_snapshot!(result.iter().map(|q| q.build(backend)).collect::<Vec<String>>().join("\n"));
        });
    }
}
