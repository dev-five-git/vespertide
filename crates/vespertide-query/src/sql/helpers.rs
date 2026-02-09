use sea_query::{
    Alias, ColumnDef as SeaColumnDef, ForeignKeyAction, MysqlQueryBuilder, PostgresQueryBuilder,
    QueryStatementWriter, SchemaStatementBuilder, SimpleExpr, SqliteQueryBuilder,
};

use vespertide_core::{
    ColumnDef, ColumnType, ComplexColumnType, ReferenceAction, SimpleColumnType, TableConstraint,
};

use super::create_table::build_create_table_for_backend;
use super::types::{BuiltQuery, DatabaseBackend, RawSql};

/// Normalize fill_with value - empty string becomes '' (SQL empty string literal)
pub fn normalize_fill_with(fill_with: Option<&str>) -> Option<String> {
    fill_with.map(|s| {
        if s.is_empty() {
            "''".to_string()
        } else {
            s.to_string()
        }
    })
}

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

/// Apply vespertide ColumnType to sea_query ColumnDef with table-aware enum type naming
pub fn apply_column_type_with_table(col: &mut SeaColumnDef, ty: &ColumnType, table: &str) {
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
                    // Use table-prefixed enum type name to avoid conflicts
                    let type_name = build_enum_type_name(table, name);
                    col.enumeration(
                        Alias::new(&type_name),
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
    let lower = default.to_lowercase();

    // UUID generation functions
    if lower == "gen_random_uuid()" || lower == "uuid()" || lower == "lower(hex(randomblob(16)))" {
        return match backend {
            DatabaseBackend::Postgres => "gen_random_uuid()".to_string(),
            DatabaseBackend::MySql => "(UUID())".to_string(),
            DatabaseBackend::Sqlite => "lower(hex(randomblob(16)))".to_string(),
        };
    }

    // Timestamp functions (case-insensitive)
    if lower == "current_timestamp()"
        || lower == "now()"
        || lower == "current_timestamp"
        || lower == "getdate()"
    {
        return match backend {
            DatabaseBackend::Postgres => "CURRENT_TIMESTAMP".to_string(),
            DatabaseBackend::MySql => "CURRENT_TIMESTAMP".to_string(),
            DatabaseBackend::Sqlite => "CURRENT_TIMESTAMP".to_string(),
        };
    }

    // PostgreSQL-style type casts: 'value'::type or expr::type
    if let Some((value, cast_type)) = parse_pg_type_cast(default) {
        return convert_type_cast(&value, &cast_type, backend);
    }

    default.to_string()
}

/// Parse a PostgreSQL-style type cast expression (e.g., `'[]'::json`, `0::boolean`)
/// Returns `(value, type)` if parsed, or None if not a type cast.
fn parse_pg_type_cast(expr: &str) -> Option<(String, String)> {
    let trimmed = expr.trim();

    // Handle quoted values: 'value'::type
    if let Some(after_open) = trimmed.strip_prefix('\'') {
        // Find the closing quote (handle escaped quotes '')
        let mut i = 0;
        let bytes = after_open.as_bytes();
        while i < bytes.len() {
            if bytes[i] == b'\'' {
                // Check for escaped quote ''
                if i + 1 < bytes.len() && bytes[i + 1] == b'\'' {
                    i += 2;
                    continue;
                }
                // Found closing quote
                let value_end = i + 1; // index in `after_open`
                let rest = &after_open[value_end..];
                if let Some(stripped) = rest.strip_prefix("::") {
                    let cast_type = stripped.trim().to_lowercase();
                    if !cast_type.is_empty() {
                        let value = format!("'{}'", &after_open[..i]);
                        return Some((value, cast_type));
                    }
                }
                return None;
            }
            i += 1;
        }
        return None;
    }

    // Handle unquoted values: expr::type (e.g., 0::boolean, NULL::json)
    if let Some(pos) = trimmed.find("::") {
        let value = trimmed[..pos].trim().to_string();
        let cast_type = trimmed[pos + 2..].trim().to_lowercase();
        if !value.is_empty() && !cast_type.is_empty() {
            return Some((value, cast_type));
        }
    }

    None
}

/// Map PostgreSQL type name to MySQL CAST target type
fn pg_type_to_mysql_cast(pg_type: &str) -> &'static str {
    match pg_type {
        "json" | "jsonb" => "JSON",
        "text" | "varchar" | "char" | "character varying" => "CHAR",
        "integer" | "int" | "int4" | "smallint" | "int2" => "SIGNED",
        "bigint" | "int8" => "SIGNED",
        "real" | "float4" | "double precision" | "float8" => "DECIMAL",
        "boolean" | "bool" => "UNSIGNED",
        "date" => "DATE",
        "time" => "TIME",
        "timestamp"
        | "timestamptz"
        | "timestamp with time zone"
        | "timestamp without time zone" => "DATETIME",
        "numeric" | "decimal" => "DECIMAL",
        "bytea" => "BINARY",
        _ => "CHAR",
    }
}

/// Convert a type cast expression to the appropriate backend syntax
fn convert_type_cast(value: &str, cast_type: &str, backend: &DatabaseBackend) -> String {
    match backend {
        // PostgreSQL: keep native :: syntax
        DatabaseBackend::Postgres => format!("{}::{}", value, cast_type),
        // MySQL: CAST(value AS type)
        DatabaseBackend::MySql => {
            let mysql_type = pg_type_to_mysql_cast(cast_type);
            format!("CAST({} AS {})", value, mysql_type)
        }
        // SQLite: strip the cast, use raw value (SQLite is dynamically typed)
        DatabaseBackend::Sqlite => value.to_string(),
    }
}

/// Check if the column type is an enum type
fn is_enum_type(column_type: &ColumnType) -> bool {
    matches!(
        column_type,
        ColumnType::Complex(ComplexColumnType::Enum { .. })
    )
}

/// Normalize a default value for enum columns - add quotes if needed
/// This is used for SQL expressions (INSERT, UPDATE) where enum values need quoting
pub fn normalize_enum_default(column_type: &ColumnType, value: &str) -> String {
    if is_enum_type(column_type) && needs_quoting(value) {
        format!("'{}'", value)
    } else {
        value.to_string()
    }
}

/// Check if a string default value needs quoting (is a plain string literal without quotes/parens)
fn needs_quoting(default_str: &str) -> bool {
    let trimmed = default_str.trim();
    // Empty string always needs quoting to become ''
    if trimmed.is_empty() {
        return true;
    }
    // Don't quote if already quoted
    if trimmed.starts_with('\'') || trimmed.starts_with('"') {
        return false;
    }
    // Don't quote if it's a function call
    if trimmed.contains('(') || trimmed.contains(')') {
        return false;
    }
    // Don't quote NULL
    if trimmed.eq_ignore_ascii_case("null") {
        return false;
    }
    // Don't quote special SQL keywords
    if trimmed.eq_ignore_ascii_case("current_timestamp")
        || trimmed.eq_ignore_ascii_case("current_date")
        || trimmed.eq_ignore_ascii_case("current_time")
    {
        return false;
    }
    true
}

/// Build sea_query ColumnDef from vespertide ColumnDef for a specific backend with table-aware enum naming
pub fn build_sea_column_def_with_table(
    backend: &DatabaseBackend,
    table: &str,
    column: &ColumnDef,
) -> SeaColumnDef {
    let mut col = SeaColumnDef::new(Alias::new(&column.name));
    apply_column_type_with_table(&mut col, &column.r#type, table);

    if !column.nullable {
        col.not_null();
    }

    if let Some(default) = &column.default {
        let default_str = default.to_sql();
        let converted = convert_default_for_backend(&default_str, backend);

        // Auto-quote enum default values if the value is a string and needs quoting
        let final_default =
            if is_enum_type(&column.r#type) && default.is_string() && needs_quoting(&converted) {
                format!("'{}'", converted)
            } else {
                converted
            };

        // SQLite requires DEFAULT (expr) for expressions containing function calls.
        // Wrapping in parentheses is always safe for all backends.
        let final_default = if *backend == DatabaseBackend::Sqlite
            && final_default.contains('(')
            && !final_default.starts_with('(')
        {
            format!("({})", final_default)
        } else {
            final_default
        };

        col.default(Into::<SimpleExpr>::into(sea_query::Expr::cust(
            final_default,
        )));
    }

    col
}

/// Generate CREATE TYPE SQL for an enum type (PostgreSQL only)
/// Returns None for non-PostgreSQL backends or non-enum types
///
/// The enum type name will be prefixed with the table name to avoid conflicts
/// across tables using the same enum name (e.g., "status", "gender").
pub fn build_create_enum_type_sql(
    table: &str,
    column_type: &ColumnType,
) -> Option<super::types::RawSql> {
    if let ColumnType::Complex(ComplexColumnType::Enum { name, values }) = column_type {
        // Integer enums don't need CREATE TYPE - they use INTEGER column
        if values.is_integer() {
            return None;
        }

        let values_sql = values.to_sql_values().join(", ");

        // Generate unique type name with table prefix
        let type_name = build_enum_type_name(table, name);

        // PostgreSQL: CREATE TYPE {table}_{name} AS ENUM (...)
        let pg_sql = format!("CREATE TYPE \"{}\" AS ENUM ({})", type_name, values_sql);

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
///
/// The enum type name will be prefixed with the table name to match the CREATE TYPE.
pub fn build_drop_enum_type_sql(
    table: &str,
    column_type: &ColumnType,
) -> Option<super::types::RawSql> {
    if let ColumnType::Complex(ComplexColumnType::Enum { name, .. }) = column_type {
        // Generate the same unique type name used in CREATE TYPE
        let type_name = build_enum_type_name(table, name);

        // PostgreSQL: DROP TYPE {table}_{name}
        let pg_sql = format!("DROP TYPE \"{}\"", type_name);

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

// Re-export naming functions from vespertide-naming
pub use vespertide_naming::{
    build_check_constraint_name, build_enum_type_name, build_foreign_key_name, build_index_name,
    build_unique_constraint_name,
};

/// Alias for build_check_constraint_name for SQLite enum columns
pub fn build_sqlite_enum_check_name(table: &str, column: &str) -> String {
    build_check_constraint_name(table, column)
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

/// Extract CHECK constraint clauses from a list of table constraints.
/// Returns SQL fragments like: `CONSTRAINT "chk_name" CHECK (expr)`
pub fn extract_check_clauses(constraints: &[TableConstraint]) -> Vec<String> {
    constraints
        .iter()
        .filter_map(|c| {
            if let TableConstraint::Check { name, expr } = c {
                Some(format!("CONSTRAINT \"{}\" CHECK ({})", name, expr))
            } else {
                None
            }
        })
        .collect()
}

/// Collect ALL CHECK constraint clauses for a SQLite temp table.
/// Combines both:
/// - Enum-based CHECK constraints (from column types)
/// - Explicit CHECK constraints (from `TableConstraint::Check`)
///
/// Returns deduplicated union of both.
pub fn collect_all_check_clauses(
    table: &str,
    columns: &[ColumnDef],
    constraints: &[TableConstraint],
) -> Vec<String> {
    let mut clauses = collect_sqlite_enum_check_clauses(table, columns);
    let explicit = extract_check_clauses(constraints);
    for clause in explicit {
        if !clauses.contains(&clause) {
            clauses.push(clause);
        }
    }
    clauses
}

/// Build CREATE TABLE query with CHECK constraints properly embedded.
/// sea-query doesn't support CHECK constraints natively, so we inject them
/// by modifying the generated SQL string.
pub fn build_create_with_checks(
    backend: &DatabaseBackend,
    create_stmt: &sea_query::TableCreateStatement,
    check_clauses: &[String],
) -> BuiltQuery {
    if check_clauses.is_empty() {
        BuiltQuery::CreateTable(Box::new(create_stmt.clone()))
    } else {
        let base_sql = build_schema_statement(create_stmt, *backend);
        let mut modified_sql = base_sql;
        if let Some(pos) = modified_sql.rfind(')') {
            let check_sql = check_clauses.join(", ");
            modified_sql.insert_str(pos, &format!(", {}", check_sql));
        }
        BuiltQuery::Raw(RawSql::per_backend(
            modified_sql.clone(),
            modified_sql.clone(),
            modified_sql,
        ))
    }
}

/// Build the CREATE TABLE statement for a SQLite temp table, including all CHECK constraints.
/// This combines `build_create_table_for_backend` with CHECK constraint injection.
///
/// `table` is the ORIGINAL table name (used for constraint naming).
/// `temp_table` is the temporary table name.
pub fn build_sqlite_temp_table_create(
    backend: &DatabaseBackend,
    temp_table: &str,
    table: &str,
    columns: &[ColumnDef],
    constraints: &[TableConstraint],
) -> BuiltQuery {
    let create_stmt = build_create_table_for_backend(backend, temp_table, columns, constraints);
    let check_clauses = collect_all_check_clauses(table, columns, constraints);
    build_create_with_checks(backend, &create_stmt, &check_clauses)
}

/// Recreate all indexes (both regular and UNIQUE) after a SQLite temp table rebuild.
/// After DROP TABLE + RENAME, all original indexes are gone, so plain CREATE INDEX is correct.
///
/// `pending_constraints` are constraints that exist in the logical schema but haven't been
/// physically created yet (e.g., promoted from inline column definitions by AddColumn normalization).
/// These will be created by separate AddConstraint actions later, so we must NOT recreate them here.
pub fn recreate_indexes_after_rebuild(
    table: &str,
    constraints: &[TableConstraint],
    pending_constraints: &[TableConstraint],
) -> Vec<BuiltQuery> {
    let mut queries = Vec::new();
    for constraint in constraints {
        // Skip constraints that will be created by future AddConstraint actions
        if pending_constraints.contains(constraint) {
            continue;
        }
        match constraint {
            TableConstraint::Index { name, columns } => {
                let index_name = build_index_name(table, columns, name.as_deref());
                let cols_sql = columns
                    .iter()
                    .map(|c| format!("\"{}\"", c))
                    .collect::<Vec<_>>()
                    .join(", ");
                let sql = format!(
                    "CREATE INDEX \"{}\" ON \"{}\" ({})",
                    index_name, table, cols_sql
                );
                queries.push(BuiltQuery::Raw(RawSql::per_backend(
                    sql.clone(),
                    sql.clone(),
                    sql,
                )));
            }
            TableConstraint::Unique { name, columns } => {
                let index_name = build_unique_constraint_name(table, columns, name.as_deref());
                let cols_sql = columns
                    .iter()
                    .map(|c| format!("\"{}\"", c))
                    .collect::<Vec<_>>()
                    .join(", ");
                let sql = format!(
                    "CREATE UNIQUE INDEX \"{}\" ON \"{}\" ({})",
                    index_name, table, cols_sql
                );
                queries.push(BuiltQuery::Raw(RawSql::per_backend(
                    sql.clone(),
                    sql.clone(),
                    sql,
                )));
            }
            _ => {}
        }
    }
    queries
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
        apply_column_type_with_table(&mut col, &ty, "test_table");
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
    #[case(SimpleColumnType::Inet)]
    #[case(SimpleColumnType::Cidr)]
    #[case(SimpleColumnType::Macaddr)]
    #[case(SimpleColumnType::Xml)]
    fn test_all_simple_types_cover_branches(#[case] ty: SimpleColumnType) {
        let mut col = SeaColumnDef::new(Alias::new("t"));
        apply_column_type_with_table(&mut col, &ColumnType::Simple(ty), "test_table");
    }

    #[rstest]
    #[case(ComplexColumnType::Varchar { length: 42 })]
    #[case(ComplexColumnType::Numeric { precision: 8, scale: 3 })]
    #[case(ComplexColumnType::Char { length: 3 })]
    #[case(ComplexColumnType::Custom { custom_type: "GEOGRAPHY".into() })]
    #[case(ComplexColumnType::Enum { name: "status".into(), values: EnumValues::String(vec!["active".into(), "inactive".into()]) })]
    fn test_all_complex_types_cover_branches(#[case] ty: ComplexColumnType) {
        let mut col = SeaColumnDef::new(Alias::new("t"));
        apply_column_type_with_table(&mut col, &ColumnType::Complex(ty), "test_table");
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
        "lower(hex(randomblob(16)))"
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
    #[case::now_upper_postgres("NOW()", DatabaseBackend::Postgres, "CURRENT_TIMESTAMP")]
    #[case::now_upper_mysql("NOW()", DatabaseBackend::MySql, "CURRENT_TIMESTAMP")]
    #[case::now_upper_sqlite("NOW()", DatabaseBackend::Sqlite, "CURRENT_TIMESTAMP")]
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

    // --- PostgreSQL type cast conversion tests ---

    #[rstest]
    // JSON type cast: '[]'::json
    #[case::json_cast_postgres("'[]'::json", DatabaseBackend::Postgres, "'[]'::json")]
    #[case::json_cast_mysql("'[]'::json", DatabaseBackend::MySql, "CAST('[]' AS JSON)")]
    #[case::json_cast_sqlite("'[]'::json", DatabaseBackend::Sqlite, "'[]'")]
    // JSONB type cast: '{}'::jsonb
    #[case::jsonb_cast_postgres("'{}'::jsonb", DatabaseBackend::Postgres, "'{}'::jsonb")]
    #[case::jsonb_cast_mysql("'{}'::jsonb", DatabaseBackend::MySql, "CAST('{}' AS JSON)")]
    #[case::jsonb_cast_sqlite("'{}'::jsonb", DatabaseBackend::Sqlite, "'{}'")]
    // Text type cast: 'hello'::text
    #[case::text_cast_postgres("'hello'::text", DatabaseBackend::Postgres, "'hello'::text")]
    #[case::text_cast_mysql("'hello'::text", DatabaseBackend::MySql, "CAST('hello' AS CHAR)")]
    #[case::text_cast_sqlite("'hello'::text", DatabaseBackend::Sqlite, "'hello'")]
    // Integer type cast: 0::integer
    #[case::int_cast_postgres("0::integer", DatabaseBackend::Postgres, "0::integer")]
    #[case::int_cast_mysql("0::integer", DatabaseBackend::MySql, "CAST(0 AS SIGNED)")]
    #[case::int_cast_sqlite("0::integer", DatabaseBackend::Sqlite, "0")]
    // Boolean type cast: 0::boolean
    #[case::bool_cast_postgres("0::boolean", DatabaseBackend::Postgres, "0::boolean")]
    #[case::bool_cast_mysql("0::boolean", DatabaseBackend::MySql, "CAST(0 AS UNSIGNED)")]
    #[case::bool_cast_sqlite("0::boolean", DatabaseBackend::Sqlite, "0")]
    // Nested JSON object: '{"key":"value"}'::json
    #[case::json_obj_cast_postgres(
        "'{\"key\":\"value\"}'::json",
        DatabaseBackend::Postgres,
        "'{\"key\":\"value\"}'::json"
    )]
    #[case::json_obj_cast_mysql(
        "'{\"key\":\"value\"}'::json",
        DatabaseBackend::MySql,
        "CAST('{\"key\":\"value\"}' AS JSON)"
    )]
    #[case::json_obj_cast_sqlite(
        "'{\"key\":\"value\"}'::json",
        DatabaseBackend::Sqlite,
        "'{\"key\":\"value\"}'"
    )]
    // Timestamp type cast: '2024-01-01'::timestamp
    #[case::timestamp_cast_postgres(
        "'2024-01-01'::timestamp",
        DatabaseBackend::Postgres,
        "'2024-01-01'::timestamp"
    )]
    #[case::timestamp_cast_mysql(
        "'2024-01-01'::timestamp",
        DatabaseBackend::MySql,
        "CAST('2024-01-01' AS DATETIME)"
    )]
    #[case::timestamp_cast_sqlite(
        "'2024-01-01'::timestamp",
        DatabaseBackend::Sqlite,
        "'2024-01-01'"
    )]
    fn test_convert_default_for_backend_type_cast(
        #[case] default: &str,
        #[case] backend: DatabaseBackend,
        #[case] expected: &str,
    ) {
        let result = convert_default_for_backend(default, &backend);
        assert_eq!(result, expected);
    }

    #[test]
    fn test_parse_pg_type_cast_no_cast() {
        // Regular values should not be parsed as type casts
        assert!(parse_pg_type_cast("'hello'").is_none());
        assert!(parse_pg_type_cast("42").is_none());
        assert!(parse_pg_type_cast("NOW()").is_none());
        assert!(parse_pg_type_cast("CURRENT_TIMESTAMP").is_none());
    }

    #[test]
    fn test_parse_pg_type_cast_valid() {
        let (value, cast_type) = parse_pg_type_cast("'[]'::json").unwrap();
        assert_eq!(value, "'[]'");
        assert_eq!(cast_type, "json");

        let (value, cast_type) = parse_pg_type_cast("0::boolean").unwrap();
        assert_eq!(value, "0");
        assert_eq!(cast_type, "boolean");
    }

    #[test]
    fn test_parse_pg_type_cast_escaped_quotes() {
        // Value with escaped quotes: 'it''s'::text
        let (value, cast_type) = parse_pg_type_cast("'it''s'::text").unwrap();
        assert_eq!(value, "'it''s'");
        assert_eq!(cast_type, "text");
    }

    #[test]
    fn test_parse_pg_type_cast_unterminated_quote() {
        // Unterminated quoted string should return None (line 203)
        assert!(parse_pg_type_cast("'unclosed").is_none());
        assert!(parse_pg_type_cast("'no close quote::json").is_none());
    }

    #[rstest]
    #[case::numeric("'0.5'::numeric", DatabaseBackend::MySql, "CAST('0.5' AS DECIMAL)")]
    #[case::decimal("'1.23'::decimal", DatabaseBackend::MySql, "CAST('1.23' AS DECIMAL)")]
    #[case::bytea("'\\xDE'::bytea", DatabaseBackend::MySql, "CAST('\\xDE' AS BINARY)")]
    #[case::unknown("'x'::citext", DatabaseBackend::MySql, "CAST('x' AS CHAR)")]
    fn test_convert_default_for_backend_type_cast_extra(
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
        apply_column_type_with_table(&mut col, &integer_enum, "test_table");
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
        assert!(build_create_enum_type_sql("test_table", &integer_enum).is_none());
    }

    #[rstest]
    // Empty strings need quoting
    #[case::empty("", true)]
    #[case::whitespace_only("   ", true)]
    // Function calls should not be quoted
    #[case::now_func("now()", false)]
    #[case::coalesce_func("COALESCE(old_value, 'default')", false)]
    #[case::uuid_func("gen_random_uuid()", false)]
    // NULL keyword should not be quoted
    #[case::null_upper("NULL", false)]
    #[case::null_lower("null", false)]
    #[case::null_mixed("Null", false)]
    // SQL date/time keywords should not be quoted
    #[case::current_timestamp_upper("CURRENT_TIMESTAMP", false)]
    #[case::current_timestamp_lower("current_timestamp", false)]
    #[case::current_date_upper("CURRENT_DATE", false)]
    #[case::current_date_lower("current_date", false)]
    #[case::current_time_upper("CURRENT_TIME", false)]
    #[case::current_time_lower("current_time", false)]
    // Already quoted strings should not be re-quoted
    #[case::single_quoted("'active'", false)]
    #[case::double_quoted("\"active\"", false)]
    // Plain strings need quoting
    #[case::plain_active("active", true)]
    #[case::plain_pending("pending", true)]
    #[case::plain_underscore("some_value", true)]
    fn test_needs_quoting(#[case] input: &str, #[case] expected: bool) {
        assert_eq!(needs_quoting(input), expected);
    }

    #[test]
    fn test_recreate_indexes_after_rebuild_skips_pending() {
        use vespertide_core::TableConstraint;
        let idx1 = TableConstraint::Index {
            name: Some("idx_a".into()),
            columns: vec!["a".into()],
        };
        let idx2 = TableConstraint::Index {
            name: Some("idx_b".into()),
            columns: vec!["b".into()],
        };
        let uq1 = TableConstraint::Unique {
            name: Some("uq_c".into()),
            columns: vec!["c".into()],
        };

        // All three in table constraints, but idx1 and uq1 are pending
        let constraints = vec![idx1.clone(), idx2.clone(), uq1.clone()];
        let pending = vec![idx1.clone(), uq1.clone()];

        let queries = recreate_indexes_after_rebuild("t", &constraints, &pending);
        // Only idx_b should be recreated
        assert_eq!(queries.len(), 1);
        let sql = queries[0].build(DatabaseBackend::Sqlite);
        assert!(sql.contains("idx_b"));
    }

    #[test]
    fn test_recreate_indexes_after_rebuild_no_pending() {
        use vespertide_core::TableConstraint;
        let idx = TableConstraint::Index {
            name: Some("idx_a".into()),
            columns: vec!["a".into()],
        };
        let uq = TableConstraint::Unique {
            name: Some("uq_b".into()),
            columns: vec!["b".into()],
        };

        let queries = recreate_indexes_after_rebuild("t", &[idx, uq], &[]);
        assert_eq!(queries.len(), 2);
    }

    #[test]
    fn test_recreate_indexes_after_rebuild_skips_non_index_constraints() {
        use vespertide_core::TableConstraint;
        let pk = TableConstraint::PrimaryKey {
            columns: vec!["id".into()],
            auto_increment: false,
        };
        let fk = TableConstraint::ForeignKey {
            name: None,
            columns: vec!["uid".into()],
            ref_table: "u".into(),
            ref_columns: vec!["id".into()],
            on_delete: None,
            on_update: None,
        };
        let chk = TableConstraint::Check {
            name: "chk".into(),
            expr: "id > 0".into(),
        };

        let queries = recreate_indexes_after_rebuild("t", &[pk, fk, chk], &[]);
        assert_eq!(queries.len(), 0);
    }
}
