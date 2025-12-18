use sea_query::{Alias, Table};

use vespertide_core::ColumnType;

use super::helpers::build_drop_enum_type_sql;
use super::types::BuiltQuery;

/// Build SQL to delete a column, optionally with DROP TYPE for enum columns (PostgreSQL)
pub fn build_delete_column(
    table: &str,
    column: &str,
    column_type: Option<&ColumnType>,
) -> Vec<BuiltQuery> {
    let mut stmts = Vec::new();

    // Drop the column first
    let stmt = Table::alter()
        .table(Alias::new(table))
        .drop_column(Alias::new(column))
        .to_owned();
    stmts.push(BuiltQuery::AlterTable(Box::new(stmt)));

    // If column type is an enum, drop the type after (PostgreSQL only)
    // Note: Only drop if this is the last column using this enum type
    if let Some(col_type) = column_type
        && let Some(drop_type_sql) = build_drop_enum_type_sql(col_type)
    {
        stmts.push(BuiltQuery::Raw(drop_type_sql));
    }

    stmts
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sql::types::DatabaseBackend;
    use insta::{assert_snapshot, with_settings};
    use rstest::rstest;
    use vespertide_core::{ComplexColumnType, SimpleColumnType};

    #[rstest]
    #[case::delete_column_postgres(
        "delete_column_postgres",
        DatabaseBackend::Postgres,
        &["ALTER TABLE \"users\" DROP COLUMN \"email\""]
    )]
    #[case::delete_column_mysql(
        "delete_column_mysql",
        DatabaseBackend::MySql,
        &["ALTER TABLE `users` DROP COLUMN `email`"]
    )]
    #[case::delete_column_sqlite(
        "delete_column_sqlite",
        DatabaseBackend::Sqlite,
        &["ALTER TABLE \"users\" DROP COLUMN \"email\""]
    )]
    fn test_delete_column(
        #[case] title: &str,
        #[case] backend: DatabaseBackend,
        #[case] expected: &[&str],
    ) {
        let result = build_delete_column("users", "email", None);
        let sql = result[0].build(backend);
        for exp in expected {
            assert!(
                sql.contains(exp),
                "Expected SQL to contain '{}', got: {}",
                exp,
                sql
            );
        }

        with_settings!({ snapshot_suffix => format!("delete_column_{}", title) }, {
            assert_snapshot!(sql);
        });
    }

    #[test]
    fn test_delete_enum_column_postgres() {
        use vespertide_core::EnumValues;

        let enum_type = ColumnType::Complex(ComplexColumnType::Enum {
            name: "status".into(),
            values: EnumValues::String(vec!["active".into(), "inactive".into()]),
        });
        let result = build_delete_column("users", "status", Some(&enum_type));

        // Should have 2 statements: ALTER TABLE and DROP TYPE
        assert_eq!(result.len(), 2);

        let alter_sql = result[0].build(DatabaseBackend::Postgres);
        assert!(alter_sql.contains("DROP COLUMN"));

        let drop_type_sql = result[1].build(DatabaseBackend::Postgres);
        assert!(drop_type_sql.contains("DROP TYPE IF EXISTS \"status\""));

        // MySQL and SQLite should have empty DROP TYPE
        let drop_type_mysql = result[1].build(DatabaseBackend::MySql);
        assert!(drop_type_mysql.is_empty());
    }

    #[test]
    fn test_delete_non_enum_column_no_drop_type() {
        let text_type = ColumnType::Simple(SimpleColumnType::Text);
        let result = build_delete_column("users", "name", Some(&text_type));

        // Should only have 1 statement: ALTER TABLE
        assert_eq!(result.len(), 1);
    }
}
