//! Shared test-only helpers for the vespertide-planner crate.
//!
//! All items are gated under `#[cfg(test)]` via the parent module declaration
//! in `lib.rs` and exposed `pub(crate)` so every inline `mod tests` and
//! `tests/mod.rs` entry can reuse the same implementation.

use vespertide_core::{ColumnDef, ColumnType, TableConstraint, TableDef};

/// Default test column (NOT NULL). Mirrors the production convention that
/// every example model declares `nullable` explicitly and NOT NULL is the
/// implicit default unless stated otherwise.
pub(crate) fn col(name: &str, ty: ColumnType) -> ColumnDef {
    ColumnDef::new(name, ty, false)
}

/// Nullable test column.
pub(crate) fn col_nullable(name: &str, ty: ColumnType) -> ColumnDef {
    ColumnDef::new(name, ty, true)
}

/// Build a table from name + columns + constraints.
pub(crate) fn table(
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

/// PRIMARY KEY (non-auto-increment) over the given columns.
pub(crate) fn pk(columns: Vec<&str>) -> TableConstraint {
    TableConstraint::PrimaryKey {
        auto_increment: false,
        columns: columns.into_iter().map(Into::into).collect(),
        strategy: vespertide_core::PrimaryKeyAdditionStrategy::default(),
    }
}

/// Named INDEX constraint.
pub(crate) fn idx(name: &str, columns: Vec<&str>) -> TableConstraint {
    TableConstraint::Index {
        name: Some(name.to_string()),
        columns: columns.into_iter().map(Into::into).collect(),
    }
}
