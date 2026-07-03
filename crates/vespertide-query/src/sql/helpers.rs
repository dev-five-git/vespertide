use std::borrow::Cow;

use sea_query::{
    Alias, ColumnDef as SeaColumnDef, ForeignKeyAction, MysqlQueryBuilder, PostgresQueryBuilder,
    QueryStatementWriter, SchemaStatementBuilder, SimpleExpr, SqliteQueryBuilder,
};

use vespertide_core::{
    ColumnDef, ColumnType, ComplexColumnType, ReferenceAction, SimpleColumnType, TableConstraint,
};

use super::create_table::build_create_table_for_backend;
use super::types::{BuiltQuery, DatabaseBackend, RawSql};

/// Normalize `fill_with` value - empty string becomes '' (SQL empty string literal)
/// Returns a Cow to avoid allocations when possible.
#[must_use]
pub fn normalize_fill_with(fill_with: Option<&str>) -> Option<Cow<'_, str>> {
    fill_with.map(|s| {
        if s.is_empty() {
            Cow::Borrowed("''")
        } else {
            Cow::Borrowed(s)
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

/// Apply vespertide `ColumnType` to `sea_query` `ColumnDef` with table-aware enum type naming
pub fn apply_column_type_with_table(
    col: &mut SeaColumnDef,
    ty: &ColumnType,
    table: &str,
    backend: DatabaseBackend,
) {
    match ty {
        ColumnType::Simple(simple) => apply_simple_column_type(col, *simple, backend),
        ColumnType::Complex(complex) => apply_complex_column_type(col, complex, table, backend),
    }
}

fn apply_simple_column_type(
    col: &mut SeaColumnDef,
    simple: SimpleColumnType,
    backend: DatabaseBackend,
) {
    match simple {
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
        SimpleColumnType::Timestamptz => apply_timestamptz_type(col, backend),
        SimpleColumnType::Interval => apply_interval_type(col, backend),
        SimpleColumnType::Bytea => {
            col.binary();
        }
        SimpleColumnType::Uuid => {
            col.uuid();
        }
        SimpleColumnType::Json => {
            col.json();
        }
        SimpleColumnType::Inet => apply_postgres_text_fallback_type(col, backend, "INET"),
        SimpleColumnType::Cidr => apply_postgres_text_fallback_type(col, backend, "CIDR"),
        SimpleColumnType::Macaddr => apply_postgres_text_fallback_type(col, backend, "MACADDR"),
        SimpleColumnType::Xml => apply_postgres_text_fallback_type(col, backend, "XML"),
        _ => unreachable!("SimpleColumnType is #[non_exhaustive]; all variants are matched above"),
    }
}

fn apply_timestamptz_type(col: &mut SeaColumnDef, backend: DatabaseBackend) {
    match backend {
        DatabaseBackend::Postgres => {
            col.timestamp_with_time_zone();
        }
        DatabaseBackend::MySql | DatabaseBackend::Sqlite => {
            col.timestamp();
        }
    }
}

fn apply_interval_type(col: &mut SeaColumnDef, backend: DatabaseBackend) {
    match backend {
        DatabaseBackend::Postgres => {
            col.interval(None, None);
        }
        DatabaseBackend::MySql | DatabaseBackend::Sqlite => {
            col.text();
        }
    }
}

fn apply_postgres_text_fallback_type(
    col: &mut SeaColumnDef,
    backend: DatabaseBackend,
    postgres_type: &str,
) {
    match backend {
        DatabaseBackend::Postgres => {
            col.custom(Alias::new(postgres_type));
        }
        DatabaseBackend::MySql | DatabaseBackend::Sqlite => {
            col.text();
        }
    }
}

fn apply_complex_column_type(
    col: &mut SeaColumnDef,
    complex: &ComplexColumnType,
    table: &str,
    backend: DatabaseBackend,
) {
    match complex {
        ComplexColumnType::Varchar { length } => {
            col.string_len(*length);
        }
        ComplexColumnType::Numeric { precision, scale } => {
            apply_numeric_type(col, *precision, *scale, backend);
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
                let variants = values
                    .variant_names()
                    .into_iter()
                    .map(Alias::new)
                    .collect::<Vec<Alias>>();
                col.enumeration(Alias::new(&type_name), variants);
            }
        }
        _ => unreachable!("ComplexColumnType is #[non_exhaustive]; all variants are matched above"),
    }
}

fn apply_numeric_type(
    col: &mut SeaColumnDef,
    precision: u32,
    scale: u32,
    backend: DatabaseBackend,
) {
    debug_assert!(
        scale <= precision,
        "numeric scale ({scale}) must be <= precision ({precision}); schema validation should reject this before SQL generation"
    );
    let safe_precision = precision.min(28);
    let safe_scale = scale.min(safe_precision);
    match backend {
        DatabaseBackend::Postgres | DatabaseBackend::MySql => {
            col.decimal_len(safe_precision, safe_scale);
        }
        DatabaseBackend::Sqlite => {
            col.double();
        }
    }
}

/// Convert vespertide `ReferenceAction` to `sea_query` `ForeignKeyAction`
pub fn to_sea_fk_action(action: &ReferenceAction) -> ForeignKeyAction {
    match action {
        ReferenceAction::Cascade => ForeignKeyAction::Cascade,
        ReferenceAction::Restrict => ForeignKeyAction::Restrict,
        ReferenceAction::SetNull => ForeignKeyAction::SetNull,
        ReferenceAction::SetDefault => ForeignKeyAction::SetDefault,
        ReferenceAction::NoAction => ForeignKeyAction::NoAction,
        _ => unreachable!("ReferenceAction is #[non_exhaustive]; all variants are matched above"),
    }
}

/// Convert vespertide `ReferenceAction` to SQL string
pub fn reference_action_sql(action: &ReferenceAction) -> &'static str {
    match action {
        ReferenceAction::Cascade => "CASCADE",
        ReferenceAction::Restrict => "RESTRICT",
        ReferenceAction::SetNull => "SET NULL",
        ReferenceAction::SetDefault => "SET DEFAULT",
        ReferenceAction::NoAction => "NO ACTION",
        _ => unreachable!("ReferenceAction is #[non_exhaustive]; all variants are matched above"),
    }
}

/// Convert a default value string to the appropriate backend-specific expression
pub fn convert_default_for_backend(default: &str, backend: DatabaseBackend) -> String {
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
        return "CURRENT_TIMESTAMP".to_string();
    }

    // PostgreSQL-style type casts: 'value'::type or expr::type
    if let Some((value, cast_type)) = parse_pg_type_cast(default) {
        return convert_type_cast(&value, &cast_type, backend);
    }

    default.to_string()
}

/// Parse a PostgreSQL-style type cast expression (e.g., `'[]'::json`, `0::boolean`)
/// Returns `(value, type)` if parsed, or None if not a type cast.
pub(super) fn parse_pg_type_cast(expr: &str) -> Option<(String, String)> {
    let trimmed = expr.trim();

    // Handle quoted values: 'value'::type
    if let Some(after_open) = trimmed.strip_prefix('\'') {
        // Find the closing quote (handle escaped quotes '')
        let mut chars = after_open.char_indices().peekable();
        while let Some((i, ch)) = chars.next() {
            if ch == '\'' {
                // Check for escaped quote ''
                if chars.next_if(|(_, next)| *next == '\'').is_some() {
                    continue;
                }
                // Found closing quote
                let value_end = i + ch.len_utf8(); // index in `after_open`
                let rest = after_open.get(value_end..)?;
                if let Some(stripped) = rest.strip_prefix("::") {
                    let cast_type = stripped.trim().to_lowercase();
                    if !cast_type.is_empty() {
                        let value = format!("'{}'", after_open.get(..i)?);
                        return Some((value, cast_type));
                    }
                }
                return None;
            }
        }
        return None;
    }

    // Handle unquoted values: expr::type (e.g., 0::boolean, NULL::json)
    if let Some((value, cast_type)) = trimmed.split_once("::") {
        let value = value.trim().to_string();
        let cast_type = cast_type.trim().to_lowercase();
        if !value.is_empty() && !cast_type.is_empty() {
            return Some((value, cast_type));
        }
    }

    None
}

/// Map `PostgreSQL` type name to `MySQL` CAST target type
fn pg_type_to_mysql_cast(pg_type: &str) -> &'static str {
    match pg_type {
        "json" | "jsonb" => "JSON",
        "integer" | "int" | "int4" | "smallint" | "int2" | "bigint" | "int8" => "SIGNED",
        "real" | "float4" | "double precision" | "float8" | "numeric" | "decimal" => "DECIMAL",
        "boolean" | "bool" => "UNSIGNED",
        "date" => "DATE",
        "time" => "TIME",
        "timestamp"
        | "timestamptz"
        | "timestamp with time zone"
        | "timestamp without time zone" => "DATETIME",
        "bytea" => "BINARY",
        _ => "CHAR",
    }
}

/// Convert a type cast expression to the appropriate backend syntax
fn convert_type_cast(value: &str, cast_type: &str, backend: DatabaseBackend) -> String {
    match backend {
        // PostgreSQL: keep native :: syntax
        DatabaseBackend::Postgres => format!("{value}::{cast_type}"),
        // MySQL: CAST(value AS type)
        DatabaseBackend::MySql => {
            let mysql_type = pg_type_to_mysql_cast(cast_type);
            format!("CAST({value} AS {mysql_type})")
        }
        // SQLite: strip the cast, use raw value (SQLite is dynamically typed)
        DatabaseBackend::Sqlite => value.to_string(),
    }
}

/// Check if the column type is an enum type
pub(super) fn is_enum_type(column_type: &ColumnType) -> bool {
    matches!(
        column_type,
        ColumnType::Complex(ComplexColumnType::Enum { .. })
    )
}

/// Normalize a default value for enum columns - add quotes if needed
/// This is used for SQL expressions (INSERT, UPDATE) where enum values need quoting
pub fn normalize_enum_default(column_type: &ColumnType, value: &str) -> String {
    if is_enum_type(column_type) && needs_quoting(value) {
        format!("'{value}'")
    } else {
        value.to_string()
    }
}

/// Check if a string default value needs quoting (is a plain string literal without quotes/parens)
pub(super) fn needs_quoting(default_str: &str) -> bool {
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

/// Build `sea_query` `ColumnDef` from vespertide `ColumnDef` for a specific backend with table-aware enum naming
pub fn build_sea_column_def_with_table(
    backend: DatabaseBackend,
    table: &str,
    column: &ColumnDef,
) -> SeaColumnDef {
    let mut col = SeaColumnDef::new(Alias::new(&column.name));
    apply_column_type_with_table(&mut col, &column.r#type, table, backend);

    if !column.nullable {
        col.not_null();
    }

    if let Some(default) = &column.default {
        let default_str = default.to_sql();
        let converted = convert_default_for_backend(&default_str, backend);

        // Auto-quote enum default values if the value is a string and needs quoting
        let final_default =
            if is_enum_type(&column.r#type) && default.is_string() && needs_quoting(&converted) {
                format!("'{converted}'")
            } else {
                converted
            };

        // SQLite requires DEFAULT (expr) for expressions containing function calls.
        // Wrapping in parentheses is always safe for all backends.
        let final_default = if backend == DatabaseBackend::Sqlite
            && final_default.contains('(')
            && !final_default.starts_with('(')
        {
            format!("({final_default})")
        } else {
            final_default
        };

        col.default(Into::<SimpleExpr>::into(sea_query::Expr::cust(
            final_default,
        )));
    }

    col
}

/// Generate CREATE TYPE SQL for an enum type (`PostgreSQL` only)
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
        let type_name = quote_ident(&type_name, DatabaseBackend::Postgres);
        let pg_sql = format!("CREATE TYPE {type_name} AS ENUM ({values_sql})");

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

/// Generate DROP TYPE SQL for an enum type (`PostgreSQL` only)
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
        let type_name = quote_ident(&type_name, DatabaseBackend::Postgres);
        let pg_sql = format!("DROP TYPE {type_name}");

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

/// Generate CHECK constraint expression for `SQLite` enum column
/// Returns the constraint clause like: CONSTRAINT "`chk_table_col`" CHECK (col IN ('val1', 'val2'))
pub fn build_sqlite_enum_check_clause(
    table: &str,
    column: &str,
    column_type: &ColumnType,
) -> Option<String> {
    if let ColumnType::Complex(ComplexColumnType::Enum { values, .. }) = column_type {
        let name = build_check_constraint_name(table, column);
        let values_sql = values.to_sql_values().join(", ");
        let name = quote_ident(&name, DatabaseBackend::Sqlite);
        let column = quote_ident(column, DatabaseBackend::Sqlite);
        Some(format!(
            "CONSTRAINT {name} CHECK ({column} IN ({values_sql}))"
        ))
    } else {
        None
    }
}

/// Collect all CHECK constraints for enum columns in a table (for `SQLite`)
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
            if let TableConstraint::Check { name, expr, .. } = c {
                let name = quote_ident(name, DatabaseBackend::Sqlite);
                Some(format!("CONSTRAINT {name} CHECK ({expr})"))
            } else {
                None
            }
        })
        .collect()
}

/// Collect ALL CHECK constraint clauses for a `SQLite` temp table.
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
    backend: DatabaseBackend,
    create_stmt: &sea_query::TableCreateStatement,
    check_clauses: &[String],
) -> BuiltQuery {
    if check_clauses.is_empty() {
        BuiltQuery::CreateTable(Box::new(create_stmt.clone()))
    } else {
        let base_sql = build_schema_statement(create_stmt, backend);
        let mut modified_sql = base_sql;
        if let Some(pos) = modified_sql.rfind(')') {
            let check_sql = check_clauses.join(", ");
            modified_sql.insert_str(pos, &format!(", {check_sql}"));
        }
        BuiltQuery::Raw(RawSql::per_backend(
            modified_sql.clone(),
            modified_sql.clone(),
            modified_sql,
        ))
    }
}

/// Build the CREATE TABLE statement for a `SQLite` temp table, including all CHECK constraints.
/// This combines `build_create_table_for_backend` with CHECK constraint injection.
///
/// `table` is the ORIGINAL table name (used for constraint naming).
/// `temp_table` is the temporary table name.
pub fn build_sqlite_temp_table_create(
    backend: DatabaseBackend,
    temp_table: &str,
    table: &str,
    columns: &[ColumnDef],
    constraints: &[TableConstraint],
) -> BuiltQuery {
    let create_stmt = build_create_table_for_backend(backend, temp_table, columns, constraints);
    let check_clauses = collect_all_check_clauses(table, columns, constraints);
    build_create_with_checks(backend, &create_stmt, &check_clauses)
}

/// Recreate all indexes (both regular and UNIQUE) after a `SQLite` temp table rebuild.
/// After DROP TABLE + RENAME, all original indexes are gone, so plain CREATE INDEX is correct.
///
/// `pending_constraints` are constraints that exist in the logical schema but haven't been
/// physically created yet (e.g., promoted from inline column definitions by `AddColumn` normalization).
/// These will be created by separate `AddConstraint` actions later, so we must NOT recreate them here.
pub fn recreate_indexes_after_rebuild(
    table: &str,
    constraints: &[TableConstraint],
    pending_constraints: &[TableConstraint],
) -> Vec<BuiltQuery> {
    // perf: capacity follows the upper bound of emitted index queries, avoiding reallocations.
    let mut queries = Vec::with_capacity(constraints.len());
    // perf: BTreeSet membership avoids nested Vec::contains scans during SQLite rebuilds.
    let pending_constraints: std::collections::BTreeSet<_> = pending_constraints.iter().collect();
    for constraint in constraints {
        // Skip constraints that will be created by future AddConstraint actions
        if pending_constraints.contains(constraint) {
            continue;
        }
        match constraint {
            TableConstraint::Index { name, columns } => {
                let index_name = build_index_name(table, columns, name.as_deref());
                let cols_sql = quote_idents(columns, DatabaseBackend::Sqlite);
                let index_name = quote_ident(&index_name, DatabaseBackend::Sqlite);
                let table = quote_ident(table, DatabaseBackend::Sqlite);
                let sql = format!("CREATE INDEX {index_name} ON {table} ({cols_sql})");
                queries.push(BuiltQuery::Raw(RawSql::per_backend(
                    sql.clone(),
                    sql.clone(),
                    sql,
                )));
            }
            TableConstraint::Unique { name, columns, .. } => {
                let index_name = build_unique_constraint_name(table, columns, name.as_deref());
                let cols_sql = quote_idents(columns, DatabaseBackend::Sqlite);
                let index_name = quote_ident(&index_name, DatabaseBackend::Sqlite);
                let table = quote_ident(table, DatabaseBackend::Sqlite);
                let sql = format!("CREATE UNIQUE INDEX {index_name} ON {table} ({cols_sql})");
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

/// Quote an identifier (table name, column name, constraint name) for the given backend.
///
/// Escapes any quote characters within the identifier to prevent SQL injection
/// via malicious model names (defense-in-depth; identifier validation upstream
/// is the primary defense).
///
/// - `PostgreSQL` / `SQLite`: `"identifier"` (double quotes; embedded `"` escaped as `""`)
/// - `MySQL`: `` `identifier` `` (backticks; embedded `` ` `` escaped as ` `` `)
#[must_use]
pub fn quote_ident(name: &str, backend: DatabaseBackend) -> String {
    match backend {
        DatabaseBackend::Postgres | DatabaseBackend::Sqlite => {
            let escaped = name.replace('"', "\"\"");
            format!("\"{escaped}\"")
        }
        DatabaseBackend::MySql => {
            let escaped = name.replace('`', "``");
            format!("`{escaped}`")
        }
    }
}

/// Quote a list of identifiers and join them with comma.
#[must_use]
pub fn quote_idents<T: AsRef<str>>(names: &[T], backend: DatabaseBackend) -> String {
    names
        .iter()
        .map(|n| quote_ident(n.as_ref(), backend))
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_query::{Alias, ColumnDef as SeaColDef, Table};

    /// `build_create_with_checks` early-returns a plain `CreateTable` query
    /// when `check_clauses` is empty (no string-injection round-trip).
    /// Covers the `if check_clauses.is_empty() { ... }` true-branch.
    #[test]
    fn build_create_with_checks_empty_clauses_returns_plain_create_table() {
        let mut stmt = Table::create();
        stmt.table(Alias::new("users"))
            .col(SeaColDef::new(Alias::new("id")).integer().not_null());
        let query = build_create_with_checks(DatabaseBackend::Postgres, &stmt, &[]);
        let sql = query.build(DatabaseBackend::Postgres);
        assert!(
            sql.contains("CREATE TABLE"),
            "expected CREATE TABLE in: {sql}"
        );
        // No CHECK clauses appended.
        assert!(
            !sql.contains("CHECK ("),
            "no CHECK should be injected: {sql}"
        );
        // The empty-branch path returns a `CreateTable` variant (not `Raw`).
        assert!(
            matches!(query, BuiltQuery::CreateTable(_)),
            "empty-checks branch must return BuiltQuery::CreateTable"
        );
    }
}
