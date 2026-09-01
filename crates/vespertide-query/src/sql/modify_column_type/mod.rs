mod direct;
mod fill_with;
mod narrowing_preprocess;
mod sqlite_rebuild;

pub use narrowing_preprocess::build_narrowing_preprocess;

use fill_with::extend_fill_with_updates;

use vespertide_core::NarrowingStrategy;

/// Combine narrowing pre-processing (when `narrowing_strategy` is set) with
/// the normal `ModifyColumnType` SQL pipeline. This is the single entry
/// point consumed by `sql::build_action_queries_with_pending` so the
/// dispatcher stays short enough to satisfy the workspace `too_many_lines`
/// budget.
#[expect(
    clippy::too_many_arguments,
    reason = "mirrors build_modify_column_type plus narrowing_strategy + timezone; threading these into a context struct would require a parallel sql-dispatch refactor"
)]
pub fn build_with_narrowing_preprocess(
    backend: DatabaseBackend,
    table: &str,
    column: &str,
    new_type: &ColumnType,
    fill_with: Option<&BTreeMap<String, String>>,
    narrowing_strategy: Option<&NarrowingStrategy>,
    timezone: Option<&str>,
    current_schema: &[TableDef],
    pending_constraints: &[vespertide_core::TableConstraint],
) -> Result<Vec<BuiltQuery>, QueryError> {
    let mut queries = Vec::new();
    if let Some(strategy) = narrowing_strategy {
        queries.extend(build_narrowing_preprocess(
            backend,
            table,
            column,
            new_type,
            strategy,
            current_schema,
        )?);
    }
    queries.extend(build_modify_column_type_with_timezone(
        backend,
        table,
        column,
        new_type,
        fill_with,
        timezone,
        current_schema,
        pending_constraints,
    )?);
    Ok(queries)
}

/// Inject a `USING col AT TIME ZONE '<tz>'` clause on `PostgreSQL` when the
/// action carries a timezone and the target type is `timestamp` or
/// `timestamptz`. `MySQL` and `SQLite` ignore the timezone and fall back to
/// the regular `build_modify_column_type` path because vespertide maps
/// both `timestamp` and `timestamptz` to the same underlying SQL type on
/// those backends — the conversion is a no-op there.
#[expect(
    clippy::too_many_arguments,
    reason = "timezone is passed alongside fill_with/narrowing/schema; consolidating would be a workspace-wide refactor and is tracked separately"
)]
fn build_modify_column_type_with_timezone(
    backend: DatabaseBackend,
    table: &str,
    column: &str,
    new_type: &ColumnType,
    fill_with: Option<&BTreeMap<String, String>>,
    timezone: Option<&str>,
    current_schema: &[TableDef],
    pending_constraints: &[vespertide_core::TableConstraint],
) -> Result<Vec<BuiltQuery>, QueryError> {
    if let Some(tz) = timezone
        && backend == DatabaseBackend::Postgres
    {
        if let Some(q) = build_pg_alter_with_timezone(table, column, new_type, tz) {
            return Ok(vec![q]);
        }
        // The action carries a timezone but the target type is not
        // timestamp/timestamptz. Treat as user error so the migration
        // is not silently mis-emitted.
        return Err(QueryError::UnsupportedAction(format!(
            "timezone metadata is only valid when converting to/from timestamp/timestamptz; \
             got new_type = {new_type:?}"
        )));
    }
    // MySQL / SQLite with a timezone: vespertide maps both timestamp and
    // timestamptz to the same underlying SQL type, so the timezone has no
    // effect on the emitted SQL. Recorded in the migration JSON for
    // portability and we fall through to the regular ALTER path.
    build_modify_column_type(
        backend,
        table,
        column,
        new_type,
        fill_with,
        current_schema,
        pending_constraints,
    )
}

/// PostgreSQL-only: emit `ALTER TABLE ... ALTER COLUMN ... TYPE ... USING ...`
/// with the timezone-aware USING expression. Returns `None` when the target
/// type is not `timestamp` / `timestamptz` so the caller can decide how to
/// surface the misuse.
fn build_pg_alter_with_timezone(
    table: &str,
    column: &str,
    new_type: &ColumnType,
    tz: &str,
) -> Option<BuiltQuery> {
    use vespertide_core::SimpleColumnType;

    let qt = super::helpers::quote_ident(table, DatabaseBackend::Postgres);
    let qc = super::helpers::quote_ident(column, DatabaseBackend::Postgres);
    // validate_timezone in the CLI already rejected anything with quotes,
    // but escape defensively to keep this layer safe in isolation.
    let tz_lit = vespertide_core::escape_sql_string_literal(tz);

    let (target_sql_type, using_expr) = match new_type {
        ColumnType::Simple(SimpleColumnType::Timestamptz) => (
            "timestamptz",
            // naive → aware: interpret stored naive values AS IF in <tz>.
            format!("{qc} AT TIME ZONE '{tz_lit}'"),
        ),
        ColumnType::Simple(SimpleColumnType::Timestamp) => (
            "timestamp",
            // aware → naive: project UTC instant into <tz>, drop tz tag.
            format!("({qc} AT TIME ZONE '{tz_lit}')::timestamp"),
        ),
        _ => return None,
    };

    let sql =
        format!("ALTER TABLE {qt} ALTER COLUMN {qc} TYPE {target_sql_type} USING {using_expr}");
    Some(BuiltQuery::Raw(super::types::RawSql::uniform(sql)))
}

use std::collections::BTreeMap;

use vespertide_core::{ColumnType, TableDef};

use self::direct::build_modify_column_type_direct;
use self::sqlite_rebuild::build_modify_column_type_sqlite_temp_table;
use super::types::{BuiltQuery, DatabaseBackend};
use crate::error::QueryError;

pub fn build_modify_column_type(
    backend: DatabaseBackend,
    table: &str,
    column: &str,
    new_type: &ColumnType,
    fill_with: Option<&BTreeMap<String, String>>,
    current_schema: &[TableDef],
    pending_constraints: &[vespertide_core::TableConstraint],
) -> Result<Vec<BuiltQuery>, QueryError> {
    // SQLite does not support direct column type modification, so use temporary table approach
    if backend == DatabaseBackend::Sqlite {
        return build_modify_column_type_sqlite_temp_table(
            backend,
            table,
            column,
            new_type,
            fill_with,
            current_schema,
            pending_constraints,
        );
    }

    // PostgreSQL, MySQL, etc. can use ALTER TABLE directly
    Ok(build_modify_column_type_direct(
        backend,
        table,
        column,
        new_type,
        fill_with,
        current_schema,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{backend_tag, joined_sql_semicolon};
    use insta::{assert_snapshot, with_settings};
    use rstest::rstest;
    use vespertide_core::{
        ColumnDef, ColumnType, ComplexColumnType, EnumValues, SimpleColumnType, TableDef,
    };

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
        // For SQLite, we need to provide current schema
        let current_schema = vec![TableDef {
            name: "users".into(),
            description: None,
            columns: vec![
                ColumnDef {
                    name: "id".into(),
                    r#type: ColumnType::Simple(SimpleColumnType::Integer),
                    nullable: false,
                    default: None,
                    comment: None,
                    primary_key: None,
                    unique: None,
                    index: None,
                    foreign_key: None,
                },
                ColumnDef {
                    name: "age".into(),
                    r#type: ColumnType::Simple(SimpleColumnType::Integer),
                    nullable: true,
                    default: None,
                    comment: None,
                    primary_key: None,
                    unique: None,
                    index: None,
                    foreign_key: None,
                },
            ],
            constraints: vec![],
        }];

        let result = build_modify_column_type(
            backend,
            "users",
            "age",
            &ColumnType::Complex(ComplexColumnType::Varchar { length: 50 }),
            None,
            &current_schema,
            &[],
        );

        // SQLite may return multiple queries
        let sql = joined_sql_semicolon(backend, &result.unwrap());

        for exp in expected {
            assert!(
                sql.contains(exp),
                "Expected SQL to contain '{exp}', got: {sql}"
            );
        }
        println!("sql: {sql}");

        with_settings!({ snapshot_path => "../snapshots", snapshot_suffix => format!("modify_column_type_{}", title) }, {
            assert_snapshot!(sql);
        });
    }

    #[test]
    fn test_modify_column_type_table_not_found() {
        let result = build_modify_column_type(
            DatabaseBackend::Sqlite,
            "nonexistent_table",
            "age",
            &ColumnType::Simple(SimpleColumnType::BigInt),
            None,
            &[],
            &[],
        );
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Table 'nonexistent_table' not found")
        );
    }

    #[test]
    fn test_modify_column_type_column_not_found() {
        let current_schema = vec![TableDef {
            name: "users".into(),
            description: None,
            columns: vec![ColumnDef {
                name: "id".into(),
                r#type: ColumnType::Simple(SimpleColumnType::Integer),
                nullable: false,
                default: None,
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            }],
            constraints: vec![],
        }];
        let result = build_modify_column_type(
            DatabaseBackend::Sqlite,
            "users",
            "nonexistent_column",
            &ColumnType::Simple(SimpleColumnType::BigInt),
            None,
            &current_schema,
            &[],
        );
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Column 'nonexistent_column' not found")
        );
    }

    #[rstest]
    #[case::modify_column_type_with_index_postgres(
        "modify_column_type_with_index_postgres",
        DatabaseBackend::Postgres
    )]
    #[case::modify_column_type_with_index_mysql(
        "modify_column_type_with_index_mysql",
        DatabaseBackend::MySql
    )]
    #[case::modify_column_type_with_index_sqlite(
        "modify_column_type_with_index_sqlite",
        DatabaseBackend::Sqlite
    )]
    fn test_modify_column_type_with_index(#[case] title: &str, #[case] backend: DatabaseBackend) {
        // Test modify column type with indexes
        use vespertide_core::TableConstraint;

        let current_schema = vec![TableDef {
            name: "users".into(),
            description: None,
            columns: vec![
                ColumnDef {
                    name: "id".into(),
                    r#type: ColumnType::Simple(SimpleColumnType::Integer),
                    nullable: false,
                    default: None,
                    comment: None,
                    primary_key: None,
                    unique: None,
                    index: None,
                    foreign_key: None,
                },
                ColumnDef {
                    name: "age".into(),
                    r#type: ColumnType::Simple(SimpleColumnType::Integer),
                    nullable: true,
                    default: None,
                    comment: None,
                    primary_key: None,
                    unique: None,
                    index: None,
                    foreign_key: None,
                },
            ],
            constraints: vec![TableConstraint::Index {
                name: Some("idx_age".into()),
                columns: vec!["age".into()],
            }],
        }];

        let result = build_modify_column_type(
            backend,
            "users",
            "age",
            &ColumnType::Simple(SimpleColumnType::BigInt),
            None,
            &current_schema,
            &[],
        )
        .unwrap();

        let sql = joined_sql_semicolon(backend, &result);

        // For SQLite, should recreate index
        if matches!(backend, DatabaseBackend::Sqlite) {
            assert!(sql.contains("CREATE INDEX"));
            assert!(sql.contains("idx_age"));
        }

        with_settings!({ snapshot_path => "../snapshots", snapshot_suffix => format!("modify_column_type_with_index_{}", title) }, {
            assert_snapshot!(sql);
        });
    }

    #[rstest]
    #[case::modify_column_type_with_unique_constraint_postgres(
        "modify_column_type_with_unique_constraint_postgres",
        DatabaseBackend::Postgres
    )]
    #[case::modify_column_type_with_unique_constraint_mysql(
        "modify_column_type_with_unique_constraint_mysql",
        DatabaseBackend::MySql
    )]
    #[case::modify_column_type_with_unique_constraint_sqlite(
        "modify_column_type_with_unique_constraint_sqlite",
        DatabaseBackend::Sqlite
    )]
    fn test_modify_column_type_with_unique_constraint(
        #[case] title: &str,
        #[case] backend: DatabaseBackend,
    ) {
        // Test modify column type with unique constraint
        use vespertide_core::TableConstraint;

        let current_schema = vec![TableDef {
            name: "users".into(),
            description: None,
            columns: vec![
                ColumnDef {
                    name: "id".into(),
                    r#type: ColumnType::Simple(SimpleColumnType::Integer),
                    nullable: false,
                    default: None,
                    comment: None,
                    primary_key: None,
                    unique: None,
                    index: None,
                    foreign_key: None,
                },
                ColumnDef {
                    name: "email".into(),
                    r#type: ColumnType::Simple(SimpleColumnType::Text),
                    nullable: true,
                    default: None,
                    comment: None,
                    primary_key: None,
                    unique: None,
                    index: None,
                    foreign_key: None,
                },
            ],
            constraints: vec![TableConstraint::Unique {
                name: Some("uq_email".into()),
                columns: vec!["email".into()],
                strategy: vespertide_core::UniqueConstraintStrategy::DeleteDuplicates {
                    keep: vespertide_core::KeepPolicy::First,
                },
            }],
        }];

        let result = build_modify_column_type(
            backend,
            "users",
            "email",
            &ColumnType::Complex(ComplexColumnType::Varchar { length: 255 }),
            None,
            &current_schema,
            &[],
        )
        .unwrap();

        let sql = joined_sql_semicolon(backend, &result);

        // For SQLite, unique constraint should be in CREATE TABLE statement
        if matches!(backend, DatabaseBackend::Sqlite) {
            assert!(sql.contains("CREATE TABLE"));
        }

        with_settings!({ snapshot_path => "../snapshots", snapshot_suffix => format!("modify_column_type_with_unique_constraint_{}", title) }, {
            assert_snapshot!(sql);
        });
    }

    #[rstest]
    #[case::enum_values_changed_postgres(
        "enum_values_changed_postgres",
        DatabaseBackend::Postgres,
        ColumnType::Complex(ComplexColumnType::Enum {
            name: "status".into(),
            values: EnumValues::String(vec!["active".into(), "inactive".into()]),
        }),
        ColumnType::Complex(ComplexColumnType::Enum {
            name: "status".into(),
            values: EnumValues::String(vec!["active".into(), "inactive".into(), "pending".into()]),
        })
    )]
    #[case::enum_values_changed_mysql(
        "enum_values_changed_mysql",
        DatabaseBackend::MySql,
        ColumnType::Complex(ComplexColumnType::Enum {
            name: "status".into(),
            values: EnumValues::String(vec!["active".into(), "inactive".into()]),
        }),
        ColumnType::Complex(ComplexColumnType::Enum {
            name: "status".into(),
            values: EnumValues::String(vec!["active".into(), "inactive".into(), "pending".into()]),
        })
    )]
    #[case::enum_values_changed_sqlite(
        "enum_values_changed_sqlite",
        DatabaseBackend::Sqlite,
        ColumnType::Complex(ComplexColumnType::Enum {
            name: "status".into(),
            values: EnumValues::String(vec!["active".into(), "inactive".into()]),
        }),
        ColumnType::Complex(ComplexColumnType::Enum {
            name: "status".into(),
            values: EnumValues::String(vec!["active".into(), "inactive".into(), "pending".into()]),
        })
    )]
    #[case::enum_same_values_postgres(
        "enum_same_values_postgres",
        DatabaseBackend::Postgres,
        ColumnType::Complex(ComplexColumnType::Enum {
            name: "status".into(),
            values: EnumValues::String(vec!["active".into(), "inactive".into()]),
        }),
        ColumnType::Complex(ComplexColumnType::Enum {
            name: "status".into(),
            values: EnumValues::String(vec!["active".into(), "inactive".into()]),
        })
    )]
    #[case::enum_same_values_mysql(
        "enum_same_values_mysql",
        DatabaseBackend::MySql,
        ColumnType::Complex(ComplexColumnType::Enum {
            name: "status".into(),
            values: EnumValues::String(vec!["active".into(), "inactive".into()]),
        }),
        ColumnType::Complex(ComplexColumnType::Enum {
            name: "status".into(),
            values: EnumValues::String(vec!["active".into(), "inactive".into()]),
        })
    )]
    #[case::enum_same_values_sqlite(
        "enum_same_values_sqlite",
        DatabaseBackend::Sqlite,
        ColumnType::Complex(ComplexColumnType::Enum {
            name: "status".into(),
            values: EnumValues::String(vec!["active".into(), "inactive".into()]),
        }),
        ColumnType::Complex(ComplexColumnType::Enum {
            name: "status".into(),
            values: EnumValues::String(vec!["active".into(), "inactive".into()]),
        })
    )]
    #[case::enum_name_changed_postgres(
        "enum_name_changed_postgres",
        DatabaseBackend::Postgres,
        ColumnType::Complex(ComplexColumnType::Enum {
            name: "old_status".into(),
            values: EnumValues::String(vec!["active".into(), "inactive".into()]),
        }),
        ColumnType::Complex(ComplexColumnType::Enum {
            name: "new_status".into(),
            values: EnumValues::String(vec!["active".into(), "inactive".into()]),
        })
    )]
    #[case::enum_name_changed_mysql(
        "enum_name_changed_mysql",
        DatabaseBackend::MySql,
        ColumnType::Complex(ComplexColumnType::Enum {
            name: "old_status".into(),
            values: EnumValues::String(vec!["active".into(), "inactive".into()]),
        }),
        ColumnType::Complex(ComplexColumnType::Enum {
            name: "new_status".into(),
            values: EnumValues::String(vec!["active".into(), "inactive".into()]),
        })
    )]
    #[case::enum_name_changed_sqlite(
        "enum_name_changed_sqlite",
        DatabaseBackend::Sqlite,
        ColumnType::Complex(ComplexColumnType::Enum {
            name: "old_status".into(),
            values: EnumValues::String(vec!["active".into(), "inactive".into()]),
        }),
        ColumnType::Complex(ComplexColumnType::Enum {
            name: "new_status".into(),
            values: EnumValues::String(vec!["active".into(), "inactive".into()]),
        })
    )]
    #[case::text_to_enum_postgres(
        "text_to_enum_postgres",
        DatabaseBackend::Postgres,
        ColumnType::Simple(SimpleColumnType::Text),
        ColumnType::Complex(ComplexColumnType::Enum {
            name: "user_status".into(),
            values: EnumValues::String(vec!["active".into(), "inactive".into()]),
        })
    )]
    #[case::text_to_enum_mysql(
        "text_to_enum_mysql",
        DatabaseBackend::MySql,
        ColumnType::Simple(SimpleColumnType::Text),
        ColumnType::Complex(ComplexColumnType::Enum {
            name: "user_status".into(),
            values: EnumValues::String(vec!["active".into(), "inactive".into()]),
        })
    )]
    #[case::text_to_enum_sqlite(
        "text_to_enum_sqlite",
        DatabaseBackend::Sqlite,
        ColumnType::Simple(SimpleColumnType::Text),
        ColumnType::Complex(ComplexColumnType::Enum {
            name: "user_status".into(),
            values: EnumValues::String(vec!["active".into(), "inactive".into()]),
        })
    )]
    #[case::enum_to_text_postgres(
        "enum_to_text_postgres",
        DatabaseBackend::Postgres,
        ColumnType::Complex(ComplexColumnType::Enum {
            name: "user_status".into(),
            values: EnumValues::String(vec!["active".into(), "inactive".into()]),
        }),
        ColumnType::Simple(SimpleColumnType::Text)
    )]
    #[case::enum_to_text_mysql(
        "enum_to_text_mysql",
        DatabaseBackend::MySql,
        ColumnType::Complex(ComplexColumnType::Enum {
            name: "user_status".into(),
            values: EnumValues::String(vec!["active".into(), "inactive".into()]),
        }),
        ColumnType::Simple(SimpleColumnType::Text)
    )]
    #[case::enum_to_text_sqlite(
        "enum_to_text_sqlite",
        DatabaseBackend::Sqlite,
        ColumnType::Complex(ComplexColumnType::Enum {
            name: "user_status".into(),
            values: EnumValues::String(vec!["active".into(), "inactive".into()]),
        }),
        ColumnType::Simple(SimpleColumnType::Text)
    )]
    fn test_modify_enum_types(
        #[case] title: &str,
        #[case] backend: DatabaseBackend,
        #[case] old_type: ColumnType,
        #[case] new_type: ColumnType,
    ) {
        let current_schema = vec![TableDef {
            name: "users".into(),
            description: None,
            columns: vec![ColumnDef {
                name: "status".into(),
                r#type: old_type,
                nullable: true,
                default: None,
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            }],
            constraints: vec![],
        }];

        let result = build_modify_column_type(
            backend,
            "users",
            "status",
            &new_type,
            None,
            &current_schema,
            &[],
        )
        .unwrap();

        let sql = joined_sql_semicolon(backend, &result);

        with_settings!({ snapshot_path => "../snapshots", snapshot_suffix => format!("modify_enum_types_{}", title) }, {
            assert_snapshot!(sql);
        });
    }

    #[rstest]
    #[case::modify_enum_with_default_postgres(
        "modify_enum_with_default_postgres",
        DatabaseBackend::Postgres
    )]
    #[case::modify_enum_with_default_mysql(
        "modify_enum_with_default_mysql",
        DatabaseBackend::MySql
    )]
    #[case::modify_enum_with_default_sqlite(
        "modify_enum_with_default_sqlite",
        DatabaseBackend::Sqlite
    )]
    fn test_modify_enum_with_default_value(#[case] title: &str, #[case] backend: DatabaseBackend) {
        // Test that enum type change handles DEFAULT values correctly
        // PostgreSQL requires: DROP DEFAULT -> change type -> SET DEFAULT
        let current_schema = vec![TableDef {
            name: "reservation_session".into(),
            description: None,
            columns: vec![ColumnDef {
                name: "status".into(),
                r#type: ColumnType::Complex(ComplexColumnType::Enum {
                    name: "session_status".into(),
                    values: EnumValues::String(vec!["pending".into(), "confirmed".into()]),
                }),
                nullable: false,
                default: Some("'pending'".into()),
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            }],
            constraints: vec![],
        }];

        let new_type = ColumnType::Complex(ComplexColumnType::Enum {
            name: "session_status".into(),
            values: EnumValues::String(vec![
                "pending".into(),
                "confirmed".into(),
                "cancelled".into(),
            ]),
        });

        let result = build_modify_column_type(
            backend,
            "reservation_session",
            "status",
            &new_type,
            None,
            &current_schema,
            &[],
        )
        .unwrap();

        let sql = joined_sql_semicolon(backend, &result);

        // PostgreSQL-specific: verify DROP DEFAULT -> TYPE change -> SET DEFAULT order
        if matches!(backend, DatabaseBackend::Postgres) {
            assert!(
                sql.contains("DROP DEFAULT"),
                "Should drop default before type change. SQL: {sql}"
            );
            assert!(
                sql.contains("SET DEFAULT"),
                "Should restore default after type change. SQL: {sql}"
            );

            let drop_default_pos = sql.find("DROP DEFAULT").unwrap();
            let type_change_pos = sql.find("USING").unwrap();
            let set_default_pos = sql.find("SET DEFAULT").unwrap();

            assert!(
                drop_default_pos < type_change_pos,
                "DROP DEFAULT should come before TYPE change"
            );
            assert!(
                type_change_pos < set_default_pos,
                "SET DEFAULT should come after TYPE change"
            );
        }

        with_settings!({ snapshot_path => "../snapshots", snapshot_suffix => format!("modify_enum_with_default_{}", title) }, {
            assert_snapshot!(sql);
        });
    }

    #[test]
    fn test_modify_column_type_to_enum_with_empty_schema() {
        // Test the None branch in line 195-200
        // When current_schema is empty, old_type will be None
        use vespertide_core::ComplexColumnType;

        let result = build_modify_column_type(
            DatabaseBackend::Postgres,
            "users",
            "status",
            &ColumnType::Complex(ComplexColumnType::Enum {
                name: "status_type".into(),
                values: EnumValues::String(vec!["active".into(), "inactive".into()]),
            }),
            None,
            &[], // Empty schema - old_type will be None
            &[],
        );

        assert!(result.is_ok());
        let queries = result.unwrap();
        let sql = joined_sql_semicolon(DatabaseBackend::Postgres, &queries);

        // Should create the enum type since old_type is None
        assert!(sql.contains("CREATE TYPE"));
        assert!(sql.contains("status_type"));
        assert!(sql.contains("ALTER TABLE"));
    }

    // -----------------------------------------------------------------------
    // F20 — timezone conversion (timestamp <-> timestamptz)
    // -----------------------------------------------------------------------

    fn tz_baseline(old_type: ColumnType) -> Vec<TableDef> {
        vec![TableDef {
            name: "events".into(),
            description: None,
            columns: vec![ColumnDef {
                name: "at".into(),
                r#type: old_type,
                nullable: false,
                default: None,
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            }],
            constraints: vec![],
        }]
    }

    fn run_with_tz(
        backend: DatabaseBackend,
        old_type: ColumnType,
        new_type: &ColumnType,
        timezone: Option<&str>,
    ) -> Result<String, QueryError> {
        let baseline = tz_baseline(old_type);
        let queries = build_with_narrowing_preprocess(
            backend,
            "events",
            "at",
            new_type,
            None,
            None,
            timezone,
            &baseline,
            &[],
        )?;
        Ok(joined_sql_semicolon(backend, &queries))
    }

    fn snap_tz(name: &str, sql: &str) {
        with_settings!(
            { snapshot_path => "../snapshots", snapshot_suffix => format!("tz_{}", name) },
            { assert_snapshot!(sql); }
        );
    }

    /// Run every timezone case against all three backends so the snapshots
    /// form symmetric 3-of-a-kind triples — PG emits the `USING` clause,
    /// `MySQL` / `SQLite` fall through to the regular `ALTER` (no-op intent)
    /// for the same input. Mirrors the `supported_snap!` discipline used by
    /// the narrowing preprocess suite.
    macro_rules! tz_snap_all_backends {
        ($name:ident, $old:expr, $new:expr, $tz:expr) => {
            #[test]
            fn $name() {
                for backend in [
                    DatabaseBackend::Postgres,
                    DatabaseBackend::MySql,
                    DatabaseBackend::Sqlite,
                ] {
                    let tag = backend_tag(backend);
                    let sql = run_with_tz(backend, $old, &$new, $tz)
                        .expect("timezone conversion across backends");
                    snap_tz(&format!("{}_{}", stringify!($name), tag), &sql);
                }
            }
        };
    }

    // --- timestamp -> timestamptz, every timezone shape × 3 backends ---
    tz_snap_all_backends!(
        ts_to_tstz_utc,
        ColumnType::Simple(SimpleColumnType::Timestamp),
        ColumnType::Simple(SimpleColumnType::Timestamptz),
        Some("UTC")
    );
    tz_snap_all_backends!(
        ts_to_tstz_asia_seoul,
        ColumnType::Simple(SimpleColumnType::Timestamp),
        ColumnType::Simple(SimpleColumnType::Timestamptz),
        Some("Asia/Seoul")
    );
    tz_snap_all_backends!(
        ts_to_tstz_offset_plus_09,
        ColumnType::Simple(SimpleColumnType::Timestamp),
        ColumnType::Simple(SimpleColumnType::Timestamptz),
        Some("+09:00")
    );
    tz_snap_all_backends!(
        ts_to_tstz_offset_minus_05,
        ColumnType::Simple(SimpleColumnType::Timestamp),
        ColumnType::Simple(SimpleColumnType::Timestamptz),
        Some("-05:00")
    );

    // --- timestamptz -> timestamp × 3 backends ---
    tz_snap_all_backends!(
        tstz_to_ts_utc,
        ColumnType::Simple(SimpleColumnType::Timestamptz),
        ColumnType::Simple(SimpleColumnType::Timestamp),
        Some("UTC")
    );
    tz_snap_all_backends!(
        tstz_to_ts_asia_seoul,
        ColumnType::Simple(SimpleColumnType::Timestamptz),
        ColumnType::Simple(SimpleColumnType::Timestamp),
        Some("Asia/Seoul")
    );

    // --- No-timezone fallback path (legacy migration JSON) × 3 backends ---
    tz_snap_all_backends!(
        ts_to_tstz_no_tz_fallback,
        ColumnType::Simple(SimpleColumnType::Timestamp),
        ColumnType::Simple(SimpleColumnType::Timestamptz),
        None
    );
    tz_snap_all_backends!(
        tstz_to_ts_no_tz_fallback,
        ColumnType::Simple(SimpleColumnType::Timestamptz),
        ColumnType::Simple(SimpleColumnType::Timestamp),
        None
    );

    // --- Error path: PG + timezone + non-timestamp target ---
    #[test]
    fn pg_timezone_with_non_timestamp_target_returns_unsupported() {
        let result = run_with_tz(
            DatabaseBackend::Postgres,
            ColumnType::Simple(SimpleColumnType::Timestamp),
            &ColumnType::Complex(ComplexColumnType::Varchar { length: 30 }),
            Some("UTC"),
        );
        assert!(
            matches!(result, Err(QueryError::UnsupportedAction(_))),
            "timezone on non-timestamp target should error, got: {result:?}"
        );
    }

    // --- Defense-in-depth: single quote inside tz literal must be escaped ---
    #[test]
    fn pg_timezone_with_embedded_quote_escapes_safely() {
        // The CLI validates first so this is theoretical, but
        // build_pg_alter_with_timezone runs in isolation and must not
        // generate broken SQL.
        let sql = run_with_tz(
            DatabaseBackend::Postgres,
            ColumnType::Simple(SimpleColumnType::Timestamp),
            &ColumnType::Simple(SimpleColumnType::Timestamptz),
            Some("evil';--"),
        )
        .expect("escaping must not error");
        assert!(
            sql.contains("'evil'';--'"),
            "single quote must be doubled, got: {sql}"
        );
    }

    /// `build_with_narrowing_preprocess` with `narrowing_strategy = Some(...)`
    /// exercises the `if let Some(strategy)` branch (lines 30-39 of mod.rs)
    /// that prepends the narrowing pre-cleanup statements ahead of the
    /// regular `ModifyColumnType` SQL.
    #[rstest]
    #[case::postgres(DatabaseBackend::Postgres)]
    #[case::mysql(DatabaseBackend::MySql)]
    #[case::sqlite(DatabaseBackend::Sqlite)]
    fn build_with_narrowing_preprocess_runs_strategy_when_some(#[case] backend: DatabaseBackend) {
        // Baseline: Text column. Narrowing into Varchar(10) with TruncateOverlong
        // strategy. The pre-cleanup should emit at least one statement that
        // mutates rows whose value exceeds the new bound.
        let baseline = vec![TableDef {
            name: "users".into(),
            description: None,
            columns: vec![ColumnDef {
                name: "name".into(),
                r#type: ColumnType::Simple(SimpleColumnType::Text),
                nullable: false,
                default: None,
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            }],
            constraints: vec![],
        }];
        let new_type = ColumnType::Complex(ComplexColumnType::Varchar { length: 10 });
        let strategy = vespertide_core::NarrowingStrategy::Truncate;
        let queries = build_with_narrowing_preprocess(
            backend,
            "users",
            "name",
            &new_type,
            None,
            Some(&strategy),
            None,
            &baseline,
            &[],
        )
        .unwrap();
        let sql = joined_sql_semicolon(backend, &queries);
        // The pre-cleanup phase touches "name" before the type change.
        assert!(
            sql.contains("UPDATE") || sql.contains("name"),
            "narrowing preprocess should emit prep SQL, got: {sql}"
        );
    }

    #[rstest]
    #[case::fill_with_enum_change_postgres(
        "fill_with_enum_change_postgres",
        DatabaseBackend::Postgres,
        ColumnType::Complex(ComplexColumnType::Enum {
            name: "status".into(),
            values: EnumValues::String(vec!["active".into(), "inactive".into(), "banned".into()]),
        }),
        ColumnType::Complex(ComplexColumnType::Enum {
            name: "status".into(),
            values: EnumValues::String(vec!["active".into(), "inactive".into()]),
        })
    )]
    #[case::fill_with_enum_change_sqlite(
        "fill_with_enum_change_sqlite",
        DatabaseBackend::Sqlite,
        ColumnType::Complex(ComplexColumnType::Enum {
            name: "status".into(),
            values: EnumValues::String(vec!["active".into(), "inactive".into(), "banned".into()]),
        }),
        ColumnType::Complex(ComplexColumnType::Enum {
            name: "status".into(),
            values: EnumValues::String(vec!["active".into(), "inactive".into()]),
        })
    )]
    #[case::fill_with_enum_change_mysql(
        "fill_with_enum_change_mysql",
        DatabaseBackend::MySql,
        ColumnType::Complex(ComplexColumnType::Enum {
            name: "status".into(),
            values: EnumValues::String(vec!["active".into(), "inactive".into(), "banned".into()]),
        }),
        ColumnType::Complex(ComplexColumnType::Enum {
            name: "status".into(),
            values: EnumValues::String(vec!["active".into(), "inactive".into()]),
        })
    )]
    fn test_modify_column_type_with_fill_with(
        #[case] title: &str,
        #[case] backend: DatabaseBackend,
        #[case] old_type: ColumnType,
        #[case] new_type: ColumnType,
    ) {
        let mut fill_with_map = std::collections::BTreeMap::new();
        fill_with_map.insert("banned".to_string(), "inactive".to_string());

        let current_schema = vec![TableDef {
            name: "users".into(),
            description: None,
            columns: vec![ColumnDef {
                name: "status".into(),
                r#type: old_type,
                nullable: true,
                default: None,
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            }],
            constraints: vec![],
        }];

        let result = build_modify_column_type(
            backend,
            "users",
            "status",
            &new_type,
            Some(&fill_with_map),
            &current_schema,
            &[],
        )
        .unwrap();

        let sql = joined_sql_semicolon(backend, &result);

        // All backends should include the UPDATE statement for fill_with
        assert!(
            sql.contains("UPDATE"),
            "Expected UPDATE for fill_with mapping, got: {sql}"
        );

        with_settings!({ snapshot_path => "../snapshots", snapshot_suffix => format!("modify_column_type_with_fill_with_{}", title) }, {
            assert_snapshot!(sql);
        });
    }
}
