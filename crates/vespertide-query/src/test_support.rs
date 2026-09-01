//! Shared test-only helpers for the vespertide-query crate.
//!
//! All items are gated under `#[cfg(test)]` via the parent module declaration
//! in `lib.rs` and exposed `pub(crate)` so every inline `mod tests` and
//! `sql/tests/mod.rs` entry can reuse the same implementation.

use vespertide_core::{ColumnDef, ColumnType, TableConstraint, TableDef};

/// Test column helper defaulting to `nullable: true`, matching the existing
/// convention in `crates/vespertide-query/src/sql/tests/mod.rs`.
pub(crate) fn col(name: &str, ty: ColumnType) -> ColumnDef {
    ColumnDef::new(name, ty, true)
}

/// Test column helper with explicit nullability.
pub(crate) fn col_n(name: &str, ty: ColumnType, nullable: bool) -> ColumnDef {
    ColumnDef::new(name, ty, nullable)
}

/// Build a `TableDef` from name + columns + constraints. Hoisted from a
/// byte-identical local helper that lived in three `sql/modify_column_*` test
/// modules (`comment`, `default`, `nullable`).
pub(crate) fn table_def(
    name: &str,
    columns: Vec<ColumnDef>,
    constraints: Vec<TableConstraint>,
) -> TableDef {
    TableDef {
        name: name.into(),
        description: None,
        columns,
        constraints,
    }
}

/// Render every `BuiltQuery` for one backend and join with newlines. Mirrors
/// the chained `.iter().map(...).collect::<Vec<...>>().join("\n")` pattern
/// repeated across `sql/**/tests` modules. The fully-qualified types keep
/// this helper independent of any consumer's `use` order.
pub(crate) fn joined_sql(
    backend: crate::sql::DatabaseBackend,
    queries: &[crate::sql::BuiltQuery],
) -> String {
    queries
        .iter()
        .map(|q| q.build(backend))
        .collect::<Vec<String>>()
        .join("\n")
}

/// Same shape as [`joined_sql`] but uses `";\n"` as the separator — matches
/// the canonical multi-statement SQL formatting that `builder::mod` and
/// `sql::delete_column::mod` snapshot. Hoisted from two byte-identical
/// `build_sql_snapshot` helpers that lived in those modules pre-0.2.0;
/// adopted crate-wide wherever a test joins built queries with `";\n"`.
pub(crate) fn joined_sql_semicolon(
    backend: crate::sql::DatabaseBackend,
    queries: &[crate::sql::BuiltQuery],
) -> String {
    queries
        .iter()
        .map(|q| q.build(backend))
        .collect::<Vec<String>>()
        .join(";\n")
}

/// Snapshot-suffix tag for a backend. The string is part of the snapshot
/// file name, so changing it would invalidate every `assert_snapshot!` that
/// uses it — keep it byte-stable. Hoisted from a `match backend { ... }`
/// block repeated across `sql/tests/naming.rs` and adopted by the
/// backend fan-out loops in `builder`, `remap_enum_values`, and the
/// `modify_column_type` test modules.
pub(crate) fn backend_tag(backend: crate::sql::DatabaseBackend) -> &'static str {
    match backend {
        crate::sql::DatabaseBackend::Postgres => "postgres",
        crate::sql::DatabaseBackend::MySql => "mysql",
        crate::sql::DatabaseBackend::Sqlite => "sqlite",
    }
}
