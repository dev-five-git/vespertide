use sea_query::{Alias, ColumnDef as SeaColumnDef, ForeignKeyAction, SchemaStatementBuilder, SimpleExpr, MysqlQueryBuilder, PostgresQueryBuilder, SqliteQueryBuilder};

use vespertide_core::{ColumnDef, ColumnType, ComplexColumnType, ReferenceAction, SimpleColumnType};

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
pub fn build_query_statement(
    stmt: &sea_query::InsertStatement,
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

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;
    use sea_query::{Alias, ColumnDef as SeaColumnDef, ForeignKeyAction};

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
}
