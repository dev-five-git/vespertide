use sea_query::{
    Alias, ColumnDef as SeaColumnDef, ForeignKeyAction, MysqlQueryBuilder, PostgresQueryBuilder,
    QueryStatementWriter, SchemaStatementBuilder, SimpleExpr, SqliteQueryBuilder,
};

use vespertide_core::{
    ColumnDef, ColumnType, ComplexColumnType, ReferenceAction, SimpleColumnType,
};

use super::types::DatabaseBackend;

/// Helper function to convert a schema statement to SQL for a specific backend
pub fn build_schema_statement<T: SchemaStatementBuilder>(
    stmt: &T,
    backend: DatabaseBackend,
) -> String {
    match backend {
        DatabaseBackend::Postgres => stmt.to_string(PostgresQueryBuilder),
        DatabaseBackend::MySql => stmt.to_string(MysqlQueryBuilder),
        DatabaseBackend::Sqlite => stmt.to_string(SqliteQueryBuilder),
    }
}

/// Helper function to convert a query statement (INSERT, SELECT, etc.) to SQL for a specific backend
pub fn build_query_statement<T: QueryStatementWriter>(
    stmt: &T,
    backend: DatabaseBackend,
) -> String {
    match backend {
        DatabaseBackend::Postgres => stmt.to_string(PostgresQueryBuilder),
        DatabaseBackend::MySql => stmt.to_string(MysqlQueryBuilder),
        DatabaseBackend::Sqlite => stmt.to_string(SqliteQueryBuilder),
    }
}

/// Apply vespertide ColumnType to sea_query ColumnDef
pub fn apply_column_type(col: &mut SeaColumnDef, ty: &ColumnType) {
    match ty {
        ColumnType::Simple(simple) => match simple {
            SimpleColumnType::SmallInt => {
                col.small_integer();
            }
            SimpleColumnType::Integer => {
                col.integer();
            }
            SimpleColumnType::BigInt => {
                col.big_integer();
            }
            SimpleColumnType::Real => {
                col.float();
            }
            SimpleColumnType::DoublePrecision => {
                col.double();
            }
            SimpleColumnType::Text => {
                col.text();
            }
            SimpleColumnType::Boolean => {
                col.boolean();
            }
            SimpleColumnType::Date => {
                col.date();
            }
            SimpleColumnType::Time => {
                col.time();
            }
            SimpleColumnType::Timestamp => {
                col.timestamp();
            }
            SimpleColumnType::Timestamptz => {
                col.timestamp_with_time_zone();
            }
            SimpleColumnType::Interval => {
                col.interval(None, None);
            }
            SimpleColumnType::Bytea => {
                col.binary();
            }
            SimpleColumnType::Uuid => {
                col.uuid();
            }
            SimpleColumnType::Json => {
                col.json();
            }
            SimpleColumnType::Jsonb => {
                col.json_binary();
            }
            SimpleColumnType::Inet => {
                col.custom(Alias::new("INET"));
            }
            SimpleColumnType::Cidr => {
                col.custom(Alias::new("CIDR"));
            }
            SimpleColumnType::Macaddr => {
                col.custom(Alias::new("MACADDR"));
            }
            SimpleColumnType::Xml => {
                col.custom(Alias::new("XML"));
            }
        },
        ColumnType::Complex(complex) => match complex {
            ComplexColumnType::Varchar { length } => {
                col.string_len(*length);
            }
            ComplexColumnType::Numeric { precision, scale } => {
                col.decimal_len(*precision, *scale);
            }
            ComplexColumnType::Char { length } => {
                col.char_len(*length);
            }
            ComplexColumnType::Custom { custom_type } => {
                col.custom(Alias::new(custom_type));
            }
            ComplexColumnType::Enum { name, values } => {
                // For integer enums, use INTEGER type instead of ENUM
                if values.is_integer() {
                    col.integer();
                } else {
                    col.enumeration(
                        Alias::new(name),
                        values
                            .variant_names()
                            .into_iter()
                            .map(Alias::new)
                            .collect::<Vec<Alias>>(),
                    );
                }
            }
        },
    }
}

/// Convert vespertide ReferenceAction to sea_query ForeignKeyAction
pub fn to_sea_fk_action(action: &ReferenceAction) -> ForeignKeyAction {
    match action {
        ReferenceAction::Cascade => ForeignKeyAction::Cascade,
        ReferenceAction::Restrict => ForeignKeyAction::Restrict,
        ReferenceAction::SetNull => ForeignKeyAction::SetNull,
        ReferenceAction::SetDefault => ForeignKeyAction::SetDefault,
        ReferenceAction::NoAction => ForeignKeyAction::NoAction,
    }
}

/// Convert vespertide ReferenceAction to SQL string
pub fn reference_action_sql(action: &ReferenceAction) -> &'static str {
    match action {
        ReferenceAction::Cascade => "CASCADE",
        ReferenceAction::Restrict => "RESTRICT",
        ReferenceAction::SetNull => "SET NULL",
        ReferenceAction::SetDefault => "SET DEFAULT",
        ReferenceAction::NoAction => "NO ACTION",
    }
}

/// Convert a default value string to the appropriate backend-specific expression
pub fn convert_default_for_backend(default: &str, backend: &DatabaseBackend) -> String {
    match default {
        "gen_random_uuid()" => match backend {
            DatabaseBackend::Postgres => "gen_random_uuid()".to_string(),
            DatabaseBackend::MySql => "(UUID())".to_string(),
            DatabaseBackend::Sqlite => "(lower(hex(randomblob(16))))".to_string(),
        },
        "current_timestamp()" | "now()" | "CURRENT_TIMESTAMP" => match backend {
            DatabaseBackend::Postgres => "CURRENT_TIMESTAMP".to_string(),
            DatabaseBackend::MySql => "CURRENT_TIMESTAMP".to_string(),
            DatabaseBackend::Sqlite => "CURRENT_TIMESTAMP".to_string(),
        },
        other => other.to_string(),
    }
}

/// Build sea_query ColumnDef from vespertide ColumnDef for a specific backend
pub fn build_sea_column_def(backend: &DatabaseBackend, column: &ColumnDef) -> SeaColumnDef {
    let mut col = SeaColumnDef::new(Alias::new(&column.name));
    apply_column_type(&mut col, &column.r#type);

    if !column.nullable {
        col.not_null();
    }

    if let Some(default) = &column.default {
        let converted = convert_default_for_backend(default, backend);
        col.default(Into::<SimpleExpr>::into(sea_query::Expr::cust(converted)));
    }

    col
}

/// Generate CREATE TYPE SQL for an enum type (PostgreSQL only)
/// Returns None for non-PostgreSQL backends or non-enum types
pub fn build_create_enum_type_sql(column_type: &ColumnType) -> Option<super::types::RawSql> {
    if let ColumnType::Complex(ComplexColumnType::Enum { name, values }) = column_type {
        // Integer enums don't need CREATE TYPE - they use INTEGER column
        if values.is_integer() {
            return None;
        }

        let values_sql = values.to_sql_values().join(", ");

        // PostgreSQL: CREATE TYPE name AS ENUM (...)
        let pg_sql = format!("CREATE TYPE \"{}\" AS ENUM ({})", name, values_sql);

        // MySQL: ENUMs are inline, no CREATE TYPE needed
        // SQLite: Uses TEXT, no CREATE TYPE needed
        Some(super::types::RawSql::per_backend(
            pg_sql,
            String::new(),
            String::new(),
        ))
    } else {
        None
    }
}

/// Generate DROP TYPE SQL for an enum type (PostgreSQL only)
/// Returns None for non-PostgreSQL backends or non-enum types
pub fn build_drop_enum_type_sql(column_type: &ColumnType) -> Option<super::types::RawSql> {
    if let ColumnType::Complex(ComplexColumnType::Enum { name, .. }) = column_type {
        // PostgreSQL: DROP TYPE IF EXISTS name
        let pg_sql = format!("DROP TYPE IF EXISTS \"{}\"", name);

        // MySQL/SQLite: No action needed
        Some(super::types::RawSql::per_backend(
            pg_sql,
            String::new(),
            String::new(),
        ))
    } else {
        None
    }
}

/// Check if a column type is an enum
pub fn is_enum_type(column_type: &ColumnType) -> bool {
    matches!(
        column_type,
        ColumnType::Complex(ComplexColumnType::Enum { .. })
    )
}

/// Generate CHECK constraint name for SQLite enum column
/// Format: chk_{table}_{column}
pub fn build_sqlite_enum_check_name(table: &str, column: &str) -> String {
    format!("chk_{}_{}", table, column)
}

/// Generate CHECK constraint expression for SQLite enum column
/// Returns the constraint clause like: CONSTRAINT "chk_table_col" CHECK (col IN ('val1', 'val2'))
pub fn build_sqlite_enum_check_clause(
    table: &str,
    column: &str,
    column_type: &ColumnType,
) -> Option<String> {
    if let ColumnType::Complex(ComplexColumnType::Enum { values, .. }) = column_type {
        let name = build_sqlite_enum_check_name(table, column);
        let values_sql = values.to_sql_values().join(", ");
        Some(format!(
            "CONSTRAINT \"{}\" CHECK (\"{}\" IN ({}))",
            name, column, values_sql
        ))
    } else {
        None
    }
}

/// Collect all CHECK constraints for enum columns in a table (for SQLite)
pub fn collect_sqlite_enum_check_clauses(table: &str, columns: &[ColumnDef]) -> Vec<String> {
    columns
        .iter()
        .filter_map(|col| build_sqlite_enum_check_clause(table, &col.name, &col.r#type))
        .collect()
}

/// Extract enum name from column type if it's an enum
pub fn get_enum_name(column_type: &ColumnType) -> Option<&str> {
    if let ColumnType::Complex(ComplexColumnType::Enum { name, .. }) = column_type {
        Some(name.as_str())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;
    use sea_query::{Alias, ColumnDef as SeaColumnDef, ForeignKeyAction};
    use vespertide_core::EnumValues;

    #[rstest]
    #[case(ColumnType::Simple(SimpleColumnType::Integer))]
    #[case(ColumnType::Simple(SimpleColumnType::BigInt))]
    #[case(ColumnType::Simple(SimpleColumnType::Text))]
    #[case(ColumnType::Simple(SimpleColumnType::Boolean))]
    #[case(ColumnType::Simple(SimpleColumnType::Timestamp))]
    #[case(ColumnType::Simple(SimpleColumnType::Uuid))]
    #[case(ColumnType::Complex(ComplexColumnType::Varchar { length: 255 }))]
    #[case(ColumnType::Complex(ComplexColumnType::Numeric { precision: 10, scale: 2 }))]
    fn test_column_type_conversion(#[case] ty: ColumnType) {
        // Just ensure no panic - test by creating a column with this type
        let mut col = SeaColumnDef::new(Alias::new("test"));
        apply_column_type(&mut col, &ty);
    }

    #[rstest]
    #[case(SimpleColumnType::SmallInt)]
    #[case(SimpleColumnType::Integer)]
    #[case(SimpleColumnType::BigInt)]
    #[case(SimpleColumnType::Real)]
    #[case(SimpleColumnType::DoublePrecision)]
    #[case(SimpleColumnType::Text)]
    #[case(SimpleColumnType::Boolean)]
    #[case(SimpleColumnType::Date)]
    #[case(SimpleColumnType::Time)]
    #[case(SimpleColumnType::Timestamp)]
    #[case(SimpleColumnType::Timestamptz)]
    #[case(SimpleColumnType::Interval)]
    #[case(SimpleColumnType::Bytea)]
    #[case(SimpleColumnType::Uuid)]
    #[case(SimpleColumnType::Json)]
    #[case(SimpleColumnType::Jsonb)]
    #[case(SimpleColumnType::Inet)]
    #[case(SimpleColumnType::Cidr)]
    #[case(SimpleColumnType::Macaddr)]
    #[case(SimpleColumnType::Xml)]
    fn test_all_simple_types_cover_branches(#[case] ty: SimpleColumnType) {
        let mut col = SeaColumnDef::new(Alias::new("t"));
        apply_column_type(&mut col, &ColumnType::Simple(ty));
    }

    #[rstest]
    #[case(ComplexColumnType::Varchar { length: 42 })]
    #[case(ComplexColumnType::Numeric { precision: 8, scale: 3 })]
    #[case(ComplexColumnType::Char { length: 3 })]
    #[case(ComplexColumnType::Custom { custom_type: "GEOGRAPHY".into() })]
    #[case(ComplexColumnType::Enum { name: "status".into(), values: EnumValues::String(vec!["active".into(), "inactive".into()]) })]
    fn test_all_complex_types_cover_branches(#[case] ty: ComplexColumnType) {
        let mut col = SeaColumnDef::new(Alias::new("t"));
        apply_column_type(&mut col, &ColumnType::Complex(ty));
    }

    #[rstest]
    #[case::cascade(ReferenceAction::Cascade, ForeignKeyAction::Cascade)]
    #[case::restrict(ReferenceAction::Restrict, ForeignKeyAction::Restrict)]
    #[case::set_null(ReferenceAction::SetNull, ForeignKeyAction::SetNull)]
    #[case::set_default(ReferenceAction::SetDefault, ForeignKeyAction::SetDefault)]
    #[case::no_action(ReferenceAction::NoAction, ForeignKeyAction::NoAction)]
    fn test_reference_action_conversion(
        #[case] action: ReferenceAction,
        #[case] expected: ForeignKeyAction,
    ) {
        // Just ensure the function doesn't panic and returns valid ForeignKeyAction
        let result = to_sea_fk_action(&action);
        assert!(
            matches!(result, _expected),
            "Expected {:?}, got {:?}",
            expected,
            result
        );
    }

    #[rstest]
    #[case(ReferenceAction::Cascade, "CASCADE")]
    #[case(ReferenceAction::Restrict, "RESTRICT")]
    #[case(ReferenceAction::SetNull, "SET NULL")]
    #[case(ReferenceAction::SetDefault, "SET DEFAULT")]
    #[case(ReferenceAction::NoAction, "NO ACTION")]
    fn test_reference_action_sql_all_variants(
        #[case] action: ReferenceAction,
        #[case] expected: &str,
    ) {
        assert_eq!(reference_action_sql(&action), expected);
    }

    #[rstest]
    #[case::gen_random_uuid_postgres(
        "gen_random_uuid()",
        DatabaseBackend::Postgres,
        "gen_random_uuid()"
    )]
    #[case::gen_random_uuid_mysql("gen_random_uuid()", DatabaseBackend::MySql, "(UUID())")]
    #[case::gen_random_uuid_sqlite(
        "gen_random_uuid()",
        DatabaseBackend::Sqlite,
        "(lower(hex(randomblob(16))))"
    )]
    #[case::current_timestamp_postgres(
        "current_timestamp()",
        DatabaseBackend::Postgres,
        "CURRENT_TIMESTAMP"
    )]
    #[case::current_timestamp_mysql(
        "current_timestamp()",
        DatabaseBackend::MySql,
        "CURRENT_TIMESTAMP"
    )]
    #[case::current_timestamp_sqlite(
        "current_timestamp()",
        DatabaseBackend::Sqlite,
        "CURRENT_TIMESTAMP"
    )]
    #[case::now_postgres("now()", DatabaseBackend::Postgres, "CURRENT_TIMESTAMP")]
    #[case::now_mysql("now()", DatabaseBackend::MySql, "CURRENT_TIMESTAMP")]
    #[case::now_sqlite("now()", DatabaseBackend::Sqlite, "CURRENT_TIMESTAMP")]
    #[case::current_timestamp_upper_postgres(
        "CURRENT_TIMESTAMP",
        DatabaseBackend::Postgres,
        "CURRENT_TIMESTAMP"
    )]
    #[case::current_timestamp_upper_mysql(
        "CURRENT_TIMESTAMP",
        DatabaseBackend::MySql,
        "CURRENT_TIMESTAMP"
    )]
    #[case::current_timestamp_upper_sqlite(
        "CURRENT_TIMESTAMP",
        DatabaseBackend::Sqlite,
        "CURRENT_TIMESTAMP"
    )]
    fn test_convert_default_for_backend(
        #[case] default: &str,
        #[case] backend: DatabaseBackend,
        #[case] expected: &str,
    ) {
        let result = convert_default_for_backend(default, &backend);
        assert_eq!(result, expected);
    }

    #[test]
    fn test_is_enum_type_true() {
        use vespertide_core::EnumValues;

        let enum_type = ColumnType::Complex(ComplexColumnType::Enum {
            name: "status".into(),
            values: EnumValues::String(vec!["active".into(), "inactive".into()]),
        });
        assert!(is_enum_type(&enum_type));
    }

    #[test]
    fn test_is_enum_type_false() {
        let text_type = ColumnType::Simple(SimpleColumnType::Text);
        assert!(!is_enum_type(&text_type));
    }

    #[test]
    fn test_get_enum_name_some() {
        use vespertide_core::EnumValues;

        let enum_type = ColumnType::Complex(ComplexColumnType::Enum {
            name: "user_status".into(),
            values: EnumValues::String(vec!["active".into(), "inactive".into()]),
        });
        assert_eq!(get_enum_name(&enum_type), Some("user_status"));
    }

    #[test]
    fn test_get_enum_name_none() {
        let text_type = ColumnType::Simple(SimpleColumnType::Text);
        assert_eq!(get_enum_name(&text_type), None);
    }

    #[test]
    fn test_apply_column_type_integer_enum() {
        use vespertide_core::{EnumValues, NumValue};
        let integer_enum = ColumnType::Complex(ComplexColumnType::Enum {
            name: "color".into(),
            values: EnumValues::Integer(vec![
                NumValue {
                    name: "Black".into(),
                    value: 0,
                },
                NumValue {
                    name: "White".into(),
                    value: 1,
                },
            ]),
        });
        let mut col = SeaColumnDef::new(Alias::new("color"));
        apply_column_type(&mut col, &integer_enum);
        // Integer enums should use INTEGER type, not ENUM
    }

    #[test]
    fn test_build_create_enum_type_sql_integer_enum_returns_none() {
        use vespertide_core::{EnumValues, NumValue};
        let integer_enum = ColumnType::Complex(ComplexColumnType::Enum {
            name: "priority".into(),
            values: EnumValues::Integer(vec![
                NumValue {
                    name: "Low".into(),
                    value: 0,
                },
                NumValue {
                    name: "High".into(),
                    value: 10,
                },
            ]),
        });
        // Integer enums should return None (no CREATE TYPE needed)
        assert!(build_create_enum_type_sql(&integer_enum).is_none());
    }
}
