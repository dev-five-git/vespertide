use vespertide_core::{ReferenceAction, TableConstraint};

use crate::error::QueryError;
use super::types::{BuiltQuery, DatabaseBackend, RawSql};
use super::helpers::reference_action_sql;

pub fn build_add_constraint(
    table: &str,
    constraint: &TableConstraint,
) -> Result<Vec<BuiltQuery>, QueryError> {
    match constraint {
        TableConstraint::PrimaryKey { columns, .. } => {
            // Use raw SQL for adding primary key constraint
            let pg_cols = columns
                .iter()
                .map(|c| format!("\"{}\"", c))
                .collect::<Vec<_>>()
                .join(", ");
            let mysql_cols = columns
                .iter()
                .map(|c| format!("`{}`", c))
                .collect::<Vec<_>>()
                .join(", ");
            Ok(vec![BuiltQuery::Raw(RawSql::per_backend(
                format!("ALTER TABLE \"{}\" ADD PRIMARY KEY ({})", table, pg_cols),
                format!("ALTER TABLE `{}` ADD PRIMARY KEY ({})", table, mysql_cols),
                format!("ALTER TABLE \"{}\" ADD PRIMARY KEY ({})", table, pg_cols),
            ))])
        }
        TableConstraint::Unique { name, columns } => {
            let pg_cols = columns
                .iter()
                .map(|c| format!("\"{}\"", c))
                .collect::<Vec<_>>()
                .join(", ");
            let mysql_cols = columns
                .iter()
                .map(|c| format!("`{}`", c))
                .collect::<Vec<_>>()
                .join(", ");
            let (pg_sql, mysql_sql) = if let Some(n) = name {
                (
                    format!(
                        "ALTER TABLE \"{}\" ADD CONSTRAINT \"{}\" UNIQUE ({})",
                        table, n, pg_cols
                    ),
                    format!(
                        "ALTER TABLE `{}` ADD CONSTRAINT `{}` UNIQUE ({})",
                        table, n, mysql_cols
                    ),
                )
            } else {
                (
                    format!("ALTER TABLE \"{}\" ADD UNIQUE ({})", table, pg_cols),
                    format!("ALTER TABLE `{}` ADD UNIQUE ({})", table, mysql_cols),
                )
            };
            Ok(vec![BuiltQuery::Raw(RawSql::per_backend(
                pg_sql.clone(),
                mysql_sql,
                pg_sql,
            ))])
        }
        TableConstraint::ForeignKey {
            name,
            columns,
            ref_table,
            ref_columns,
            on_delete,
            on_update,
        } => {
            // Use Raw SQL for FK creation to avoid SQLite panic from sea-query
            // SQLite doesn't support ALTER TABLE ADD CONSTRAINT for FK, but we generate
            // the SQL anyway - runtime will need to handle SQLite FK differently (table recreation)
            let pg_cols = columns
                .iter()
                .map(|c| format!("\"{}\"", c))
                .collect::<Vec<_>>()
                .join(", ");
            let mysql_cols = columns
                .iter()
                .map(|c| format!("`{}`", c))
                .collect::<Vec<_>>()
                .join(", ");
            let pg_ref_cols = ref_columns
                .iter()
                .map(|c| format!("\"{}\"", c))
                .collect::<Vec<_>>()
                .join(", ");
            let mysql_ref_cols = ref_columns
                .iter()
                .map(|c| format!("`{}`", c))
                .collect::<Vec<_>>()
                .join(", ");

            let (mut pg_sql, mut mysql_sql) = if let Some(n) = name {
                (
                    format!(
                        "ALTER TABLE \"{}\" ADD CONSTRAINT \"{}\" FOREIGN KEY ({}) REFERENCES \"{}\" ({})",
                        table, n, pg_cols, ref_table, pg_ref_cols
                    ),
                    format!(
                        "ALTER TABLE `{}` ADD CONSTRAINT `{}` FOREIGN KEY ({}) REFERENCES `{}` ({})",
                        table, n, mysql_cols, ref_table, mysql_ref_cols
                    ),
                )
            } else {
                (
                    format!(
                        "ALTER TABLE \"{}\" ADD FOREIGN KEY ({}) REFERENCES \"{}\" ({})",
                        table, pg_cols, ref_table, pg_ref_cols
                    ),
                    format!(
                        "ALTER TABLE `{}` ADD FOREIGN KEY ({}) REFERENCES `{}` ({})",
                        table, mysql_cols, ref_table, mysql_ref_cols
                    ),
                )
            };

            if let Some(action) = on_delete {
                let action_sql = format!(" ON DELETE {}", reference_action_sql(action));
                pg_sql.push_str(&action_sql);
                mysql_sql.push_str(&action_sql);
            }
            if let Some(action) = on_update {
                let action_sql = format!(" ON UPDATE {}", reference_action_sql(action));
                pg_sql.push_str(&action_sql);
                mysql_sql.push_str(&action_sql);
            }

            Ok(vec![BuiltQuery::Raw(RawSql::per_backend(
                pg_sql.clone(),
                mysql_sql,
                pg_sql,
            ))])
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
