//! Shared test-only helpers for `builder::parallel` and `builder::sequential`.
//!
//! Both submodules previously carried byte-identical copies of these eight
//! helpers in their inline `mod tests` blocks. They now live here as
//! `pub(super)` items, reachable from both child modules' tests via
//! `use super::test_support::*;`.

#![cfg(test)]

use vespertide_core::{
    ColumnDef, ColumnType, ForeignKeyOrphanStrategy, MigrationAction, ReferenceAction,
    SimpleColumnType, TableConstraint, TableDef,
};

use crate::DatabaseBackend;
use crate::sql::BuiltQuery;

pub(super) fn nn_col(name: &str, ty: SimpleColumnType) -> ColumnDef {
    ColumnDef::new(name, ColumnType::Simple(ty), false)
}

pub(super) fn index(name: Option<&str>, column: &str) -> TableConstraint {
    TableConstraint::Index {
        name: name.map(Into::into),
        columns: vec![column.into()],
    }
}

pub(super) fn foreign_key() -> TableConstraint {
    TableConstraint::ForeignKey {
        name: Some("fk_u__pk".into()),
        columns: vec!["pk".into()],
        ref_table: "other".into(),
        ref_columns: vec!["id".into()],
        on_delete: Some(ReferenceAction::Cascade),
        on_update: None,
        orphan_strategy: ForeignKeyOrphanStrategy::default(),
    }
}

pub(super) fn table(name: &str, constraints: Vec<TableConstraint>) -> TableDef {
    TableDef {
        name: name.into(),
        description: None,
        columns: vec![nn_col("pk", SimpleColumnType::Integer)],
        constraints,
    }
}

pub(super) fn schema_u_with_constraints(constraints: Vec<TableConstraint>) -> Vec<TableDef> {
    vec![table("u", constraints)]
}

pub(super) fn schema_u_and_v_with_u_constraints(
    constraints: Vec<TableConstraint>,
) -> Vec<TableDef> {
    vec![table("u", constraints), table("v", vec![])]
}

pub(super) fn add_required_column(table: &str, column: &str) -> MigrationAction {
    MigrationAction::AddColumn {
        table: table.into(),
        column: Box::new(nn_col(column, SimpleColumnType::Integer)),
        fill_with: None,
    }
}

pub(super) fn sqlite_sql(queries: &[BuiltQuery]) -> String {
    queries
        .iter()
        .map(|q| q.build(DatabaseBackend::Sqlite))
        .collect::<Vec<_>>()
        .join("\n")
}
