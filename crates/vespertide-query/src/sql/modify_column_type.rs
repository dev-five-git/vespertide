use sea_query::{Alias, ColumnDef as SeaColumnDef, Table};

use vespertide_core::ColumnType;

use super::types::{BuiltQuery, DatabaseBackend};
use super::helpers::apply_column_type;

pub fn build_modify_column_type(
    table: &str,
    column: &str,
    new_type: &ColumnType,
) -> BuiltQuery {
    let mut col = SeaColumnDef::new(Alias::new(column));
    apply_column_type(&mut col, new_type);

    let stmt = Table::alter()
        .table(Alias::new(table))
        .modify_column(col)
        .to_owned();
    BuiltQuery::AlterTable(Box::new(stmt))
}

#[cfg(test)]
mod tests {
    use super::*;
    use insta::{assert_snapshot, with_settings};
    use rstest::rstest;
    use vespertide_core::{ColumnType, ComplexColumnType};

    #[rstest]
    #[case::modify_column_type_postgres(
        "modify_column_type_postgres",
        DatabaseBackend::Postgres,
        &["ALTER TABLE \"users\"", "\"age\""]
    )]
    #[case::modify_column_type_mysql(
        "modify_column_type_mysql",
        DatabaseBackend::MySql,
        &["ALTER TABLE `users` MODIFY COLUMN `age` varchar(50)"]
    )]
    #[case::modify_column_type_sqlite(
        "modify_column_type_sqlite",
        DatabaseBackend::Sqlite,
        &[]
    )]
    fn test_modify_column_type(
        #[case] title: &str,
        #[case] backend: DatabaseBackend,
        #[case] expected: &[&str],
    ) {
        let result = build_modify_column_type(
            "users",
            "age",
            &ColumnType::Complex(ComplexColumnType::Varchar { length: 50 }),
        );
        let sql = result.build(backend);
        for exp in expected {
            assert!(
                sql.contains(exp),
                "Expected SQL to contain '{}', got: {}",
                exp,
                sql
            );
        }

        with_settings!({ snapshot_suffix => format!("modify_column_type_{}", title) }, {
            assert_snapshot!(sql);
        });
    }
}
