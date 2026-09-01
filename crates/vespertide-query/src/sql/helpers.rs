use sea_query::{
    Alias, ColumnDef as SeaColumnDef, ForeignKeyAction, MysqlQueryBuilder, PostgresQueryBuilder,
    Query, QueryStatementWriter, SchemaStatementBuilder, SimpleExpr, SqliteQueryBuilder, Table,
};

use vespertide_core::{
    ColumnDef, ColumnType, ComplexColumnType, EnumValues, ReferenceAction, SimpleColumnType,
    TableConstraint, TableDef,
};

use super::create_table::build_create_table_for_backend;
use super::types::{BuiltQuery, DatabaseBackend, RawSql};

/// Normalize `fill_with` value - empty string becomes `''` (SQL empty-string
/// literal). Returns a borrowed `&str` because both arms are static or borrowed
/// from the caller — no allocation ever happens, so the `Cow` wrapper was
/// purely ceremonial.
#[must_use]
pub(crate) fn normalize_fill_with(fill_with: Option<&str>) -> Option<&str> {
    fill_with.map(|s| if s.is_empty() { "''" } else { s })
}

/// Helper function to convert a schema statement to SQL for a specific backend
pub(crate) fn build_schema_statement<T: SchemaStatementBuilder>(
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
pub(crate) fn build_query_statement<T: QueryStatementWriter>(
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
pub(crate) fn apply_column_type_with_table(
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
        // Matched on the variant rather than `values.is_integer()`: the former
        // `enum_variant_aliases` helper carried an `EnumValues::Integer` arm
        // that the `is_integer()` guard made unreachable.
        ComplexColumnType::Enum { name, values } => match values {
            // Integer enums are stored as INTEGER; no native enum type is emitted.
            EnumValues::Integer(_) => {
                col.integer();
            }
            EnumValues::String(variants) => {
                // Use table-prefixed enum type name to avoid conflicts
                let type_name = build_enum_type_name(table, name);
                // Map each variant name straight into `Alias::new`, skipping the
                // intermediate `Vec<&str>` that `variant_names()` would allocate.
                let aliases: Vec<Alias> = variants.iter().map(Alias::new).collect();
                col.enumeration(Alias::new(&type_name), aliases);
            }
        },
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
pub(crate) fn to_sea_fk_action(action: &ReferenceAction) -> ForeignKeyAction {
    match action {
        ReferenceAction::Cascade => ForeignKeyAction::Cascade,
        ReferenceAction::Restrict => ForeignKeyAction::Restrict,
        ReferenceAction::SetNull => ForeignKeyAction::SetNull,
        ReferenceAction::SetDefault => ForeignKeyAction::SetDefault,
        ReferenceAction::NoAction => ForeignKeyAction::NoAction,
        _ => unreachable!("ReferenceAction is #[non_exhaustive]; all variants are matched above"),
    }
}

/// Function spellings meaning "generate a UUID". Matched against the **whole**
/// input, case-insensitively, so they can never rewrite part of a larger
/// expression.
pub(super) const UUID_FUNCTION_SPELLINGS: [&str; 3] =
    ["gen_random_uuid()", "uuid()", "lower(hex(randomblob(16)))"];

/// Function spellings meaning "current timestamp". Same whole-input matching
/// rule as [`UUID_FUNCTION_SPELLINGS`].
pub(super) const TIMESTAMP_FUNCTION_SPELLINGS: [&str; 4] = [
    "current_timestamp()",
    "now()",
    "current_timestamp",
    "getdate()",
];

/// Whole-string, case-insensitive membership test.
///
/// Uses `eq_ignore_ascii_case` rather than `to_lowercase()` so no `String` is
/// allocated per call, mirroring the convention `needs_quoting` uses below.
pub(super) fn matches_any_spelling(value: &str, spellings: &[&str]) -> bool {
    spellings.iter().any(|s| value.eq_ignore_ascii_case(s))
}

/// Convert a default value string to the appropriate backend-specific expression
///
/// This is for a column **DEFAULT** — a single literal or function call the
/// generator is free to canonicalise. It is *not* safe for a raw SQL
/// expression slot such as `fill_with`; see
/// [`super::fill_with::convert_fill_with_for_backend`].
pub(crate) fn convert_default_for_backend(default: &str, backend: DatabaseBackend) -> String {
    if matches_any_spelling(default, &UUID_FUNCTION_SPELLINGS) {
        return match backend {
            DatabaseBackend::Postgres => "gen_random_uuid()".to_string(),
            DatabaseBackend::MySql => "(UUID())".to_string(),
            DatabaseBackend::Sqlite => "lower(hex(randomblob(16)))".to_string(),
        };
    }

    if matches_any_spelling(default, &TIMESTAMP_FUNCTION_SPELLINGS) {
        return "CURRENT_TIMESTAMP".to_string();
    }

    // PostgreSQL-style type casts: 'value'::type or expr::type
    if let Some((value, cast_type)) = parse_pg_type_cast(default) {
        return convert_cast_chain(value, &cast_type, backend);
    }

    default.to_string()
}

/// Byte offset of the **last top-level** `::` cast operator in `expr`.
///
/// Top-level means outside every single-quoted string literal *and* outside
/// every parenthesised group. Both properties matter:
///
/// * Taking the **last** operator makes a cast chain (`'x'::text::json`) peel
///   from the outside in, instead of treating `text::json` as one type name.
/// * Skipping quoted and nested occurrences stops
///   `CASE WHEN tag = 'a::b' THEN 1 ELSE 2 END::integer` from being split
///   inside its own string literal — the defect that let a `fill_with`
///   expression be silently truncated and case-folded.
///
/// Returns `None` when there is no top-level cast, or when a string literal is
/// left unterminated (at that point syntax cannot be told from data).
///
/// Toggling `in_quote` on every `'` also handles the SQL `''` escape for free:
/// the pair closes and immediately reopens the literal, so its content stays
/// quoted. Driving the scan from `bytes().enumerate()` keeps the cursor
/// monotonic by construction — there is no hand-rolled index arithmetic that
/// could stall the loop. Comparing bytes is sound because every byte matched
/// here is ASCII, which never occurs inside a multi-byte UTF-8 sequence, so a
/// returned index is always a `char` boundary.
pub(super) fn find_last_top_level_cast(expr: &str) -> Option<usize> {
    let mut in_quote = false;
    let mut depth: usize = 0;
    let mut pending_colon: Option<usize> = None;
    let mut last = None;

    for (index, byte) in expr.bytes().enumerate() {
        if in_quote {
            if byte == b'\'' {
                in_quote = false;
            }
            pending_colon = None;
            continue;
        }
        match byte {
            b'\'' => {
                in_quote = true;
                pending_colon = None;
            }
            b'(' => {
                depth += 1;
                pending_colon = None;
            }
            b')' => {
                depth = depth.saturating_sub(1);
                pending_colon = None;
            }
            b':' => match pending_colon.take() {
                Some(start) if depth == 0 => last = Some(start),
                Some(_) => {}
                None => pending_colon = Some(index),
            },
            _ => pending_colon = None,
        }
    }

    if in_quote { None } else { last }
}

/// Parse a PostgreSQL-style type cast expression (e.g., `'[]'::json`, `0::boolean`)
/// Returns `(value, type)` if parsed, or None if not a type cast.
///
/// The split happens at the last top-level `::` (see
/// [`find_last_top_level_cast`]), so `'x'::text::json` yields
/// `("'x'::text", "json")` and a `::` that only appears inside a string
/// literal is not a split point at all.
///
/// The value borrows `expr` (a contiguous slice of the input, quotes
/// included); only `cast_type` is owned because of the `to_lowercase()`
/// normalisation. **Only the type name is lower-cased — the value is returned
/// byte-for-byte**, so no caller can mangle user SQL through this function.
pub(super) fn parse_pg_type_cast(expr: &str) -> Option<(&str, String)> {
    let trimmed = expr.trim();
    let split = find_last_top_level_cast(trimmed)?;
    // `split` and `split + 2` index the two ASCII `:` bytes, so both slices
    // land on `char` boundaries.
    let value = trimmed[..split].trim();
    let cast_type = trimmed[split + 2..].trim().to_lowercase();
    if value.is_empty() || cast_type.is_empty() {
        return None;
    }
    Some((value, cast_type))
}

/// Convert a possibly *chained* `PostgreSQL` cast to backend syntax.
///
/// Recurses so `'x'::text::json` nests properly: MySQL emits
/// `CAST(CAST('x' AS CHAR) AS JSON)` and SQLite strips every level rather than
/// leaving a stray `::text` behind.
fn convert_cast_chain(value: &str, cast_type: &str, backend: DatabaseBackend) -> String {
    let inner = match parse_pg_type_cast(value) {
        Some((inner_value, inner_cast)) => convert_cast_chain(inner_value, &inner_cast, backend),
        None => value.to_string(),
    };
    convert_type_cast(&inner, cast_type, backend)
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
pub(crate) fn normalize_enum_default(column_type: &ColumnType, value: &str) -> String {
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
pub(crate) fn build_sea_column_def_with_table(
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
pub(crate) fn build_create_enum_type_sql(
    table: &str,
    column_type: &ColumnType,
) -> Option<super::types::RawSql> {
    if let ColumnType::Complex(ComplexColumnType::Enum { name, values }) = column_type {
        // Integer enums don't need CREATE TYPE - they use INTEGER column
        if values.is_integer() {
            return None;
        }

        let values_sql = values.sql_values_joined(", ");

        // Generate unique type name with table prefix
        let type_name = build_enum_type_name(table, name);

        // PostgreSQL: CREATE TYPE {table}_{name} AS ENUM (...)
        let type_name = quote_ident(&type_name, DatabaseBackend::Postgres);
        let pg_sql = format!("CREATE TYPE {type_name} AS ENUM ({values_sql})");

        // MySQL: ENUMs are inline, no CREATE TYPE needed
        // SQLite: Uses TEXT, no CREATE TYPE needed
        Some(super::types::RawSql::postgres_only(pg_sql))
    } else {
        None
    }
}

/// Generate DROP TYPE SQL for an enum type (`PostgreSQL` only)
/// Returns None for non-PostgreSQL backends or non-enum types
///
/// The enum type name will be prefixed with the table name to match the CREATE TYPE.
pub(crate) fn build_drop_enum_type_sql(
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
        Some(super::types::RawSql::postgres_only(pg_sql))
    } else {
        None
    }
}

// Re-export naming functions from vespertide-naming
pub(crate) use vespertide_naming::{
    build_check_constraint_name, build_enum_type_name, build_foreign_key_name, build_index_name,
    build_unique_constraint_name,
};

/// Generate CHECK constraint expression for `SQLite` enum column
/// Returns the constraint clause like: CONSTRAINT "`chk_table_col`" CHECK (col IN ('val1', 'val2'))
pub(crate) fn build_sqlite_enum_check_clause(
    table: &str,
    column: &str,
    column_type: &ColumnType,
) -> Option<String> {
    if let ColumnType::Complex(ComplexColumnType::Enum { values, .. }) = column_type {
        let name = build_check_constraint_name(table, column);
        let values_sql = values.sql_values_joined(", ");
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
pub(crate) fn collect_sqlite_enum_check_clauses(table: &str, columns: &[ColumnDef]) -> Vec<String> {
    columns
        .iter()
        .filter_map(|col| build_sqlite_enum_check_clause(table, &col.name, &col.r#type))
        .collect()
}

/// Extract CHECK constraint clauses from a list of table constraints.
/// Returns SQL fragments like: `CONSTRAINT "chk_name" CHECK (expr)`
pub(crate) fn extract_check_clauses(constraints: &[TableConstraint]) -> Vec<String> {
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
pub(crate) fn collect_all_check_clauses(
    table: &str,
    columns: &[ColumnDef],
    constraints: &[TableConstraint],
) -> Vec<String> {
    let mut clauses = collect_sqlite_enum_check_clauses(table, columns);
    // Enum clauses are already unique among themselves (one per column), so the
    // membership set only needs to reject explicit clauses that collide with an
    // enum clause or with an earlier-kept explicit clause. Build the dedup set
    // from borrowed `&str` views of the enum clauses (no per-enum-clause `String`
    // clone) plus each kept explicit clause's `&str`, filtering the explicit list
    // into `retained` BEFORE touching `clauses` so no borrow into `clauses`
    // overlaps its later mutation. Insertion order is preserved: enum clauses
    // first, then explicit clauses in source order.
    let explicit = extract_check_clauses(constraints);
    let mut retained: Vec<String> = Vec::with_capacity(explicit.len());
    {
        // `seen` borrows the enum clauses out of `clauses`, so keep it in an
        // inner scope whose borrows end before `clauses` is mutated below. This
        // seeds the dedup set from borrowed `&str` — no per-enum-clause `String`
        // clone (the win) — and interns each kept explicit clause's `&str` so
        // explicit-vs-explicit dedup stays a single O(log n) lookup. Only the
        // explicit clauses actually kept are moved into `retained`.
        let mut seen: std::collections::BTreeSet<&str> =
            clauses.iter().map(String::as_str).collect();
        for clause in &explicit {
            if seen.insert(clause.as_str()) {
                retained.push(clause.clone());
            }
        }
    }
    clauses.extend(retained);
    clauses
}

/// Build CREATE TABLE query with CHECK constraints properly embedded.
/// sea-query doesn't support CHECK constraints natively, so we inject them
/// by modifying the generated SQL string.
pub(crate) fn build_create_with_checks(
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
            // Insert `", " + check_sql` at `pos` without an extra `format!`
            // allocation: the joined text goes in first, then the separator is
            // inserted BEFORE it at the same `pos`, yielding `", <check_sql>`.
            modified_sql.insert_str(pos, &check_sql);
            modified_sql.insert_str(pos, ", ");
        }
        BuiltQuery::Raw(RawSql::uniform(modified_sql))
    }
}

/// Build the CREATE TABLE statement for a `SQLite` temp table, including all CHECK constraints.
/// This combines `build_create_table_for_backend` with CHECK constraint injection.
///
/// `table` is the ORIGINAL table name (used for constraint naming).
/// `temp_table` is the temporary table name.
pub(crate) fn build_sqlite_temp_table_create(
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

/// Canonical `INSERT INTO temp_table (cols) SELECT cols FROM table` builder used
/// by every `SQLite` temp-table rebuild path.
///
/// Six call sites previously open-coded this identical sequence (delete column,
/// modify column type, modify column default, modify column nullable, add
/// constraint, replace constraint, remove constraint). Centralising it here
/// removes the drift risk that comes with keeping six copies in lock-step.
///
/// `columns` is the column slice to copy — pass `&table_def.columns` for the
/// "copy everything" rebuilds and a filtered slice (without the dropped column)
/// for the `DELETE COLUMN` path.
pub(super) fn build_copy_into_temp_table(
    table: &str,
    temp_table: &str,
    columns: &[ColumnDef],
) -> BuiltQuery {
    let column_aliases: Vec<Alias> = columns
        .iter()
        .map(|column| Alias::new(&column.name))
        .collect();

    let mut select_query = Query::select();
    for column_alias in &column_aliases {
        select_query.column(column_alias.clone());
    }
    select_query.from(Alias::new(table));

    let insert_stmt = Query::insert()
        .into_table(Alias::new(temp_table))
        .columns(column_aliases)
        .select_from(select_query)
        .expect("SQLite temp table copy SELECT should be valid")
        .to_owned();

    BuiltQuery::Insert(Box::new(insert_stmt))
}

/// Recreate all indexes (both regular and UNIQUE) after a `SQLite` temp table rebuild.
/// After DROP TABLE + RENAME, all original indexes are gone, so plain CREATE INDEX is correct.
///
/// `pending_constraints` are constraints that exist in the logical schema but haven't been
/// physically created yet (e.g., promoted from inline column definitions by `AddColumn` normalization).
/// These will be created by separate `AddConstraint` actions later, so we must NOT recreate them here.
pub(crate) fn recreate_indexes_after_rebuild(
    table: &str,
    constraints: &[TableConstraint],
    pending_constraints: &[TableConstraint],
) -> Vec<BuiltQuery> {
    // perf: capacity follows the upper bound of emitted index queries, avoiding reallocations.
    let mut queries = Vec::with_capacity(constraints.len());
    // perf: BTreeSet membership avoids nested Vec::contains scans during SQLite rebuilds.
    let pending_constraints: std::collections::BTreeSet<_> = pending_constraints.iter().collect();
    // perf: `table` is loop-invariant — quote it once instead of per surviving constraint.
    let quoted_table = quote_ident(table, DatabaseBackend::Sqlite);
    // dedup: both Index and Unique arms differ only by name builder + `UNIQUE` keyword.
    let mut push_index =
        |index_name: String, columns: &[vespertide_core::ColumnName], unique: bool| {
            let cols_sql = quote_idents(columns, DatabaseBackend::Sqlite);
            let quoted_index = quote_ident(&index_name, DatabaseBackend::Sqlite);
            let keyword = if unique {
                "CREATE UNIQUE INDEX"
            } else {
                "CREATE INDEX"
            };
            let sql = format!("{keyword} {quoted_index} ON {quoted_table} ({cols_sql})");
            queries.push(BuiltQuery::Raw(RawSql::uniform(sql)));
        };
    for constraint in constraints {
        // Skip constraints that will be created by future AddConstraint actions
        if pending_constraints.contains(constraint) {
            continue;
        }
        match constraint {
            TableConstraint::Index { name, columns } => {
                push_index(
                    build_index_name(table, columns, name.as_deref()),
                    columns,
                    false,
                );
            }
            TableConstraint::Unique { name, columns, .. } => {
                push_index(
                    build_unique_constraint_name(table, columns, name.as_deref()),
                    columns,
                    true,
                );
            }
            _ => {}
        }
    }
    queries
}

/// Build the canonical 5-step `SQLite` temp-table rebuild sequence:
///
/// 1. `CREATE TABLE {table}_temp(...)` from `create_columns` + `create_constraints`
/// 2. `INSERT INTO {table}_temp ... SELECT FROM {table}` over `copy_columns`
/// 3. `DROP TABLE {table}`
/// 4. `ALTER TABLE {table}_temp RENAME TO {table}`
/// 5. Recreate indexes / UNIQUE indexes from `recreate_constraints`
///    minus anything already in `pending_constraints`
///
/// Centralises the seven open-coded call sites
/// (`add_constraint::rebuild_sqlite_table_with_added_constraint`,
/// `remove_constraint::sqlite::rebuild_table_without_constraint`,
/// the `SQLite` arms of `modify_column_default` / `modify_column_nullable`,
/// `modify_column_type::sqlite_rebuild::build_modify_column_type_sqlite_temp_table`,
/// `replace_constraint::build_sqlite_constraint_replace`, and
/// `delete_column::sqlite_rebuild::build_delete_column_sqlite_temp_table`)
/// that previously re-emitted the same fixed four-statement vec + index
/// extension by hand. Each call site keeps its surrounding context
/// (`fill_with` UPDATEs, enum DROP TYPE, ...) OUTSIDE this helper — the
/// helper covers only the invariant rebuild contract, so emitted SQL
/// stays byte-identical to the previous open-coded sequences (every
/// existing snapshot must continue to match without regeneration).
///
/// The seven slices ARE the rebuild contract; bundling them into a
/// struct hides which slice plays which role at each call site, so
/// the parameters stay flat (and sit exactly at clippy's default
/// 7-arg threshold for `too_many_arguments`).
pub(super) fn build_sqlite_table_rebuild(
    backend: DatabaseBackend,
    table: &str,
    create_columns: &[ColumnDef],
    create_constraints: &[TableConstraint],
    copy_columns: &[ColumnDef],
    recreate_constraints: &[TableConstraint],
    pending_constraints: &[TableConstraint],
) -> Vec<BuiltQuery> {
    let temp_table = format!("{table}_temp");
    let create_query = build_sqlite_temp_table_create(
        backend,
        &temp_table,
        table,
        create_columns,
        create_constraints,
    );
    let insert_query = build_copy_into_temp_table(table, &temp_table, copy_columns);
    let drop_query = super::delete_table::build_delete_table(table);
    let rename_query = super::rename_table::build_rename_table(&temp_table, table);
    let index_queries =
        recreate_indexes_after_rebuild(table, recreate_constraints, pending_constraints);
    let mut queries = Vec::with_capacity(4 + index_queries.len());
    queries.push(create_query);
    queries.push(insert_query);
    queries.push(drop_query);
    queries.push(rename_query);
    queries.extend(index_queries);
    queries
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
    let mut out = String::with_capacity(name.len() + 2);
    quote_ident_into(&mut out, name, backend);
    out
}

/// Append the quoted form of `name` for `backend` directly to `out`.
///
/// Core of [`quote_ident`], exposed as an append-style helper so
/// multi-identifier emitters ([`quote_idents`]) can write every identifier
/// straight into their single output buffer without a per-identifier
/// `String` round-trip (one allocation + one memcpy saved per element).
fn quote_ident_into(out: &mut String, name: &str, backend: DatabaseBackend) {
    let delim = match backend {
        DatabaseBackend::Postgres | DatabaseBackend::Sqlite => '"',
        DatabaseBackend::MySql => '`',
    };
    // Hot path: every valid identifier produced by the codebase carries no
    // embedded quote char, so one exact-size reservation suffices and the
    // `str::replace` + `format!` double-allocation is unnecessary. Mirrors
    // the borrowed-fast-path / owned-slow-path pattern already used by
    // `vespertide_core::sql_escape::escape_sql_string_literal`.
    if !name.contains(delim) {
        out.reserve(name.len() + 2);
        out.push(delim);
        out.push_str(name);
        out.push(delim);
        return;
    }
    // Slow path (defense-in-depth): identifier embeds the quote char and
    // must be escaped by doubling it. Byte-identical output to the
    // pre-fast-path implementation.
    let escaped = name.replace(delim, &format!("{delim}{delim}"));
    out.reserve(escaped.len() + 2);
    out.push(delim);
    out.push_str(&escaped);
    out.push(delim);
}

/// Quote a list of identifiers and join them with comma.
#[must_use]
pub fn quote_idents<T: AsRef<str>>(names: &[T], backend: DatabaseBackend) -> String {
    // Write straight into one buffer instead of collecting an intermediate
    // `Vec<String>` and joining — same output, one fewer allocation per call.
    let mut out = String::new();
    for (i, n) in names.iter().enumerate() {
        if i > 0 {
            out.push_str(", ");
        }
        quote_ident_into(&mut out, n.as_ref(), backend);
    }
    out
}

/// Build a PostgreSQL `ALTER TABLE "t" ALTER COLUMN "c" <suffix>` statement.
///
/// Single source of truth for the seven open-coded PG-only emit sites
/// across [`crate::sql::modify_column_default`],
/// [`crate::sql::modify_column_nullable`], and
/// [`crate::sql::modify_column_type::direct`]. Each call site used to
/// repeat the same `quote_ident(table, …) + quote_ident(column, …) +
/// format!("ALTER TABLE {qt} ALTER COLUMN {qc} <suffix>")` skeleton; this
/// helper folds them into one call so the next PG ALTER variant only has
/// to choose its suffix. SQL output is byte-identical (every existing
/// snapshot must continue to match).
#[must_use]
pub(super) fn build_pg_alter_column_sql(table: &str, column: &str, suffix: &str) -> String {
    let quoted_table = quote_ident(table, DatabaseBackend::Postgres);
    let quoted_column = quote_ident(column, DatabaseBackend::Postgres);
    format!("ALTER TABLE {quoted_table} ALTER COLUMN {quoted_column} {suffix}")
}

/// Look up `table` in `current_schema` and return a reference to its
/// [`TableDef`], or a uniform `QueryError::SchemaError` describing why the
/// lookup is mandatory for the calling backend.
///
/// Centralises the six SQL-builder call sites that previously open-coded
/// the same `current_schema.iter().find(...).ok_or_else(...)` chain. The
/// emitted message is `"Table '{table}' not found in current schema.
/// {context}."` — pass `context` WITHOUT a trailing period so the helper
/// is the single source of truth for the sentence terminator.
pub(crate) fn require_table_in_schema<'a>(
    schema: &'a [TableDef],
    table: &str,
    context: &str,
) -> Result<&'a TableDef, crate::QueryError> {
    schema.iter().find(|t| t.name == table).ok_or_else(|| {
        crate::QueryError::SchemaError(format!(
            "Table '{table}' not found in current schema. {context}."
        ))
    })
}

/// Build a `DROP INDEX` query for the given table-qualified `index_name`.
///
/// Single source of truth for the
/// `sea_query::Index::drop().table(...).name(...)` shape. Centralises the
/// four call sites that previously open-coded it (the SQLite-rebuild
/// fallback in [`crate::sql::remove_constraint`], the Postgres / MySQL
/// `Index` arms there, and the two inline constraint-drop arms in
/// [`crate::sql::delete_column`]).
#[must_use]
pub(crate) fn build_drop_index_query(table: &str, index_name: &str) -> BuiltQuery {
    let idx_drop = sea_query::Index::drop()
        .table(Alias::new(table))
        .name(index_name)
        .to_owned();
    BuiltQuery::DropIndex(Box::new(idx_drop))
}

/// Look up `column` in `table_def.columns` and return a reference to its
/// [`ColumnDef`], or a uniform `QueryError::SchemaError` describing the
/// miss. Mirrors [`require_table_in_schema`] one level down: centralises
/// the call sites that previously open-coded
/// `table_def.columns.iter().find(|c| c.name == column).ok_or_else(...)`
/// with the canonical `"Column '{column}' not found in table '{table}'."`
/// message (trailing period included so callers stay byte-identical to
/// existing string-match assertions).
pub(crate) fn require_column_in_table<'a>(
    table_def: &'a TableDef,
    column: &str,
) -> Result<&'a ColumnDef, crate::QueryError> {
    table_def
        .columns
        .iter()
        .find(|c| c.name == column)
        .ok_or_else(|| {
            crate::QueryError::SchemaError(format!(
                "Column '{column}' not found in table '{table}'.",
                table = table_def.name,
            ))
        })
}

/// Build the `MySQL ALTER TABLE ... MODIFY COLUMN ...` query produced by
/// every "modify one ColumnDef field" builder (nullability, default, …).
///
/// Folds the byte-identical six-line MySQL dispatch — look up the table,
/// look up the column, clone & mutate one field, hand the result to
/// `build_sea_column_def_with_table`, wrap as an `AlterTable` — into a
/// single helper. `context` is the descriptor appended to the canonical
/// `require_table_in_schema` error message ("MySQL requires …"); pass it
/// WITHOUT a trailing period (the helper adds it).
///
/// `mutator` rewrites whichever single field the caller cares about
/// (`|c| c.nullable = …`, `|c| c.default = …`, …) on a fresh clone, so
/// the original `current_schema` is never mutated.
///
/// `modify_column_comment` deliberately does NOT use this helper: its
/// MySQL emit hand-appends a `COMMENT '…'` suffix outside sea-query and
/// produces `BuiltQuery::Raw` rather than `BuiltQuery::AlterTable`.
pub(crate) fn build_mysql_modify_column_with<F>(
    table: &str,
    column: &str,
    current_schema: &[TableDef],
    context: &str,
    mutator: F,
) -> Result<BuiltQuery, crate::QueryError>
where
    F: FnOnce(&mut ColumnDef),
{
    let table_def = require_table_in_schema(current_schema, table, context)?;
    let column_def = require_column_in_table(table_def, column)?;
    let mut modified_col_def = column_def.clone();
    mutator(&mut modified_col_def);
    let sea_col = build_sea_column_def_with_table(DatabaseBackend::MySql, table, &modified_col_def);
    let stmt = Table::alter()
        .table(Alias::new(table))
        .modify_column(sea_col)
        .to_owned();
    Ok(BuiltQuery::AlterTable(Box::new(stmt)))
}

/// Build the canonical `SQLite` temp-table rebuild for every "modify one
/// ColumnDef field" builder (nullability, default, …). Every other column
/// stays untouched; `mutator` rewrites only the single column the action
/// targets — matching the existing
/// `new_columns.iter_mut().find(|c| c.name == column)` shape, including
/// the silent no-op when the column is absent (which is the documented
/// behaviour: see the `column_not_found` test in `modify_column_nullable`).
///
/// Folds the byte-identical nine-line SQLite dispatch — look up the
/// table, clone the column list, rewrite one column, forward the rebuild
/// contract to `build_sqlite_table_rebuild` — into a single helper.
/// `context` is the descriptor appended to the canonical
/// `require_table_in_schema` error message.
pub(crate) fn build_sqlite_modify_column_with<F>(
    table: &str,
    column: &str,
    current_schema: &[TableDef],
    pending_constraints: &[TableConstraint],
    context: &str,
    mutator: F,
) -> Result<Vec<BuiltQuery>, crate::QueryError>
where
    F: FnOnce(&mut ColumnDef),
{
    let table_def = require_table_in_schema(current_schema, table, context)?;
    let mut new_columns = table_def.columns.clone();
    if let Some(col) = new_columns.iter_mut().find(|c| c.name == column) {
        mutator(col);
    }
    Ok(build_sqlite_table_rebuild(
        DatabaseBackend::Sqlite,
        table,
        &new_columns,
        &table_def.constraints,
        &table_def.columns,
        &table_def.constraints,
        pending_constraints,
    ))
}

/// Look up `column` in the table named `table` inside `schema`, returning
/// `None` when either the table or the column is absent.
///
/// Centralises the `schema.iter().find(|t| t.name == table).and_then(|t|
/// t.columns.iter().find(|c| c.name == column))` chain used by the five
/// non-error-returning column lookups across the SQL builders (the
/// `DeleteColumn` dispatcher, the Postgres `ModifyColumnDefault` path,
/// and the three direct `ModifyColumnType` helpers). The error-returning
/// twin lives one helper up as [`require_column_in_table`].
#[must_use]
pub(crate) fn find_column_in_schema<'a>(
    schema: &'a [TableDef],
    table: &str,
    column: &str,
) -> Option<&'a ColumnDef> {
    schema
        .iter()
        .find(|t| t.name == table)
        .and_then(|t| t.columns.iter().find(|c| c.name == column))
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

    /// `require_table_in_schema` returns the matching table when present.
    #[test]
    fn require_table_in_schema_hit_returns_table_ref() {
        let schema = vec![TableDef {
            name: "users".into(),
            description: None,
            columns: vec![],
            constraints: vec![],
        }];
        let table_def = require_table_in_schema(&schema, "users", "test context")
            .expect("table 'users' should be found in the schema");
        assert_eq!(table_def.name.as_str(), "users");
    }

    /// `require_table_in_schema` emits the canonical `SchemaError`
    /// message — same template as every replaced call site, so wire-error
    /// output stays byte-identical for callers and downstream tests.
    #[test]
    fn require_table_in_schema_miss_emits_canonical_message() {
        let schema: Vec<TableDef> = vec![];
        let err = require_table_in_schema(&schema, "missing", "SQLite requires test context")
            .expect_err("missing table must error");
        let msg = err.to_string();
        assert!(
            msg.contains(
                "Table 'missing' not found in current schema. SQLite requires test context."
            ),
            "expected canonical message with trailing period, got: {msg}"
        );
    }
}
