use std::collections::BTreeMap;

use sea_query::{Alias, ColumnDef as SeaColumnDef, Table};

use vespertide_core::{ColumnType, ComplexColumnType, TableDef};

use crate::sql::helpers::{
    apply_column_type_with_table, build_create_enum_type_sql, build_pg_alter_column_sql,
    convert_default_for_backend, find_column_in_schema, normalize_enum_default, quote_ident,
};
use crate::sql::types::{BuiltQuery, DatabaseBackend, RawSql};

/// Push a PostgreSQL-only raw SQL statement into the per-action query list.
///
/// Centralises the `queries.push(BuiltQuery::Raw(RawSql::postgres_only(...)))`
/// four-layer constructor that the enum-migration / DROP-old-type pipelines
/// emit repeatedly. The body stays a single function call so each callsite
/// reads as "emit one PG-only statement" without burying the SQL string
/// under three wrapper types.
fn push_postgres_raw(queries: &mut Vec<BuiltQuery>, sql: String) {
    queries.push(BuiltQuery::Raw(RawSql::postgres_only(sql)));
}

/// Emit one PostgreSQL-only `ALTER TABLE ... ALTER COLUMN ...` statement.
///
/// Centralises the `push_postgres_raw(queries, build_pg_alter_column_sql(...))`
/// two-call composition used three times by `build_postgres_enum_migration`
/// (DROP DEFAULT / TYPE … USING …::text::… / SET DEFAULT …) so each call site
/// reads as "alter this column with this suffix" instead of plumbing.
fn push_pg_alter_column(queries: &mut Vec<BuiltQuery>, table: &str, column: &str, suffix: &str) {
    push_postgres_raw(queries, build_pg_alter_column_sql(table, column, suffix));
}

struct DirectBuildContext<'a> {
    backend: DatabaseBackend,
    table: &'a str,
    column: &'a str,
    new_type: &'a ColumnType,
    fill_with: Option<&'a BTreeMap<String, String>>,
    current_schema: &'a [TableDef],
    old_type: Option<&'a ColumnType>,
}

pub(super) fn build_modify_column_type_direct(
    backend: DatabaseBackend,
    table: &str,
    column: &str,
    new_type: &ColumnType,
    fill_with: Option<&BTreeMap<String, String>>,
    current_schema: &[TableDef],
) -> Vec<BuiltQuery> {
    let context = DirectBuildContext {
        backend,
        table,
        column,
        new_type,
        fill_with,
        current_schema,
        old_type: old_column_type(current_schema, table, column),
    };
    let mut queries = Vec::new();

    if needs_postgres_enum_migration(&context) {
        build_postgres_enum_migration(&mut queries, &context);
    } else {
        build_standard_type_modification(&mut queries, &context);
    }

    queries
}

fn old_column_type<'a>(
    current_schema: &'a [TableDef],
    table: &str,
    column: &str,
) -> Option<&'a ColumnType> {
    find_column_in_schema(current_schema, table, column).map(|c| &c.r#type)
}

fn needs_postgres_enum_migration(context: &DirectBuildContext<'_>) -> bool {
    context.backend == DatabaseBackend::Postgres
        && matches!(
            (context.old_type, context.new_type),
            (
                Some(ColumnType::Complex(ComplexColumnType::Enum { name: old_name, values: old_values })),
                ColumnType::Complex(ComplexColumnType::Enum { name: new_name, values: new_values })
            ) if old_name != new_name || old_values != new_values
        )
}

fn build_postgres_enum_migration(queries: &mut Vec<BuiltQuery>, context: &DirectBuildContext<'_>) {
    // PostgreSQL enum-to-enum migration with USING clause for safe casting
    if let (
        Some(ColumnType::Complex(ComplexColumnType::Enum {
            name: old_enum_name,
            ..
        })),
        ColumnType::Complex(ComplexColumnType::Enum {
            name: new_enum_name,
            values: new_values,
        }),
    ) = (context.old_type, context.new_type)
    {
        let old_type_name = crate::sql::helpers::build_enum_type_name(context.table, old_enum_name);
        let new_type_name = crate::sql::helpers::build_enum_type_name(context.table, new_enum_name);
        let names_differ = old_enum_name != new_enum_name;

        // For same-name changes: create temp type, then rename back.
        // For different-name changes: create final type directly, no rename needed.
        let (target_type_name, needs_rename) = if names_differ {
            (new_type_name, false)
        } else {
            (format!("{old_type_name}_new"), true)
        };

        // 0. INSERT fill_with UPDATEs before any type changes (rows still have old enum type).
        super::extend_fill_with_updates(queries, context.table, context.column, context.fill_with);

        // 1. CREATE TYPE target_type AS ENUM (new values).
        let column_default = column_default(context);
        let create_values = new_values.sql_values_joined(", ");
        let quoted_target_type = quote_ident(&target_type_name, DatabaseBackend::Postgres);
        let quoted_column = quote_ident(context.column, DatabaseBackend::Postgres);
        let quoted_old_type = quote_ident(&old_type_name, DatabaseBackend::Postgres);
        push_postgres_raw(
            queries,
            format!("CREATE TYPE {quoted_target_type} AS ENUM ({create_values})"),
        );

        // 2. DROP DEFAULT if exists (must be done before type change).
        if column_default.is_some() {
            push_pg_alter_column(queries, context.table, context.column, "DROP DEFAULT");
        }

        // 3. ALTER TABLE ... ALTER COLUMN ... TYPE target_type USING col::text::target_type.
        push_pg_alter_column(
            queries,
            context.table,
            context.column,
            &format!("TYPE {quoted_target_type} USING {quoted_column}::text::{quoted_target_type}"),
        );

        // 4. DROP old enum type.
        push_postgres_raw(queries, format!("DROP TYPE {quoted_old_type}"));

        // 5. RENAME temp to final (only for same-name value changes).
        if needs_rename {
            push_postgres_raw(
                queries,
                format!("ALTER TYPE {quoted_target_type} RENAME TO {quoted_old_type}"),
            );
        }

        // 6. Restore DEFAULT if it existed.
        if let Some(default_value) = column_default {
            let normalized_default =
                normalize_enum_default(context.new_type, &default_value.to_sql());
            push_pg_alter_column(
                queries,
                context.table,
                context.column,
                &format!("SET DEFAULT {normalized_default}"),
            );
        }
    }
}

fn column_default(context: &DirectBuildContext<'_>) -> Option<vespertide_core::DefaultValue> {
    find_column_in_schema(context.current_schema, context.table, context.column)
        .and_then(|c| c.default.clone())
}

fn build_standard_type_modification(
    queries: &mut Vec<BuiltQuery>,
    context: &DirectBuildContext<'_>,
) {
    super::extend_fill_with_updates(queries, context.table, context.column, context.fill_with);

    create_new_enum_type_if_needed(queries, context);

    let mut col = SeaColumnDef::new(Alias::new(context.column));
    apply_column_type_with_table(&mut col, context.new_type, context.table, context.backend);
    preserve_mysql_column_attributes(&mut col, context);

    let stmt = Table::alter()
        .table(Alias::new(context.table))
        .modify_column(col)
        .to_owned();
    queries.push(BuiltQuery::AlterTable(Box::new(stmt)));

    drop_old_enum_type_if_needed(queries, context);
}

fn create_new_enum_type_if_needed(queries: &mut Vec<BuiltQuery>, context: &DirectBuildContext<'_>) {
    if let ColumnType::Complex(ComplexColumnType::Enum { name: new_name, .. }) = context.new_type {
        let should_create =
            if let Some(ColumnType::Complex(ComplexColumnType::Enum { name: old_name, .. })) =
                context.old_type
            {
                old_name != new_name
            } else {
                true
            };

        if should_create
            && let Some(create_type_sql) =
                build_create_enum_type_sql(context.table, context.new_type)
        {
            queries.push(BuiltQuery::Raw(create_type_sql));
        }
    }
}

fn preserve_mysql_column_attributes(col: &mut SeaColumnDef, context: &DirectBuildContext<'_>) {
    if context.backend == DatabaseBackend::MySql
        && let Some(column_def) =
            find_column_in_schema(context.current_schema, context.table, context.column)
    {
        if !column_def.nullable {
            col.not_null();
        }
        if let Some(default) = &column_def.default {
            let default_str = default.to_sql();
            let converted = convert_default_for_backend(&default_str, context.backend);
            let final_default = normalize_enum_default(context.new_type, &converted);
            col.default(sea_query::Expr::cust(final_default));
        }
    }
}

fn drop_old_enum_type_if_needed(queries: &mut Vec<BuiltQuery>, context: &DirectBuildContext<'_>) {
    if let Some(ColumnType::Complex(ComplexColumnType::Enum { name: old_name, .. })) =
        context.old_type
    {
        let should_drop = match context.new_type {
            ColumnType::Complex(ComplexColumnType::Enum { name: new_name, .. }) => {
                old_name != new_name
            }
            _ => true,
        };

        if should_drop {
            let old_type_name = crate::sql::helpers::build_enum_type_name(context.table, old_name);
            let old_type_name = quote_ident(&old_type_name, DatabaseBackend::Postgres);
            push_postgres_raw(queries, format!("DROP TYPE {old_type_name}"));
        }
    }
}
