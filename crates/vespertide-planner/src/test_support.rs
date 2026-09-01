//! Shared test-only helpers for the vespertide-planner crate.
//!
//! All items are gated under `#[cfg(test)]` via the parent module declaration
//! in `lib.rs` and exposed `pub(crate)` so every inline `mod tests` and
//! `tests/mod.rs` entry can reuse the same implementation.

use vespertide_core::{
    CheckViolationStrategy, ColumnDef, ColumnType, MigrationAction, MigrationPlan,
    SimpleColumnType, TableConstraint, TableDef,
};

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

/// Integer-typed test column with explicit nullability.
///
/// Hoisted from four byte-identical `fn col(name, nullable) -> ColumnDef`
/// helpers inside `validate/*` test modules. Every caller hard-coded the
/// column type to `Integer` and set every inline-constraint field to `None`,
/// which is precisely what `ColumnDef::new(name, Integer, nullable)`
/// produces.
pub(crate) fn col_int(name: &str, nullable: bool) -> ColumnDef {
    ColumnDef::new(
        name,
        ColumnType::Simple(SimpleColumnType::Integer),
        nullable,
    )
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

/// Build a dummy [`MigrationPlan`] from a list of actions.
///
/// The `id` / `version` / `comment` / `created_at` fields are filled with
/// inert default values because every validator test we share this helper
/// with asserts only on the produced actions / errors and never on plan
/// metadata. Keeping one canonical helper avoids re-pasting an 8-line
/// constructor in every `validate/*.rs` test module.
pub(crate) fn plan(actions: Vec<MigrationAction>) -> MigrationPlan {
    MigrationPlan {
        id: String::new(),
        version: 0,
        comment: None,
        created_at: None,
        actions,
    }
}

/// CHECK [`TableConstraint`] with the default violation strategy.
///
/// Hoisted because every CHECK-related validator test reconstructed this
/// 4-line constructor (`name.to_string()`, `expr.to_string()`,
/// `strategy: CheckViolationStrategy::default()`) verbatim.
pub(crate) fn check(name: &str, expr: &str) -> TableConstraint {
    TableConstraint::Check {
        name: name.to_string(),
        expr: expr.to_string(),
        strategy: CheckViolationStrategy::default(),
    }
}

/// `AddConstraint` action wrapping a [`check`] for the given table.
///
/// Same rationale as [`check`]: collapse the duplicated `AddConstraint {
/// table, constraint: check(...) }` boilerplate that lived in every
/// `validate/check_*.rs` test module.
pub(crate) fn add_check(table: &str, name: &str, expr: &str) -> MigrationAction {
    MigrationAction::AddConstraint {
        table: table.into(),
        constraint: check(name, expr),
    }
}
