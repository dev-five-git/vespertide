//! Daemon-free `SQLite` execution validation.
//!
//! Uses `rusqlite` with the bundled feature so no system `libsqlite` is needed.
//! Every generated `SQLite` SQL string is executed against a fresh in-memory database
//! and verified to be syntactically and semantically accepted by a real `SQLite` engine.

use proptest::prelude::*;
use rusqlite::Connection;
use vespertide_core::arbitrary::{arb_safe_ident, arb_table_def};
use vespertide_core::{
    ColumnDef, ColumnType, ComplexColumnType, MigrationAction, SimpleColumnType, TableDef,
};
use vespertide_query::{DatabaseBackend, build_action_queries};

proptest! {
    #![proptest_config(ProptestConfig { cases: 64, max_shrink_iters: 5000, ..ProptestConfig::default() })]

    /// CREATE TABLE: every emitted SQL must be accepted by a real `SQLite` engine.
    #[test]
    fn sqlite_accepts_create_table(table in arb_table_def_fk_free()) {
        let conn = Connection::open_in_memory().expect("in-memory SQLite must open");
        let action = MigrationAction::CreateTable {
            table: table.name.clone(),
            columns: table.columns.clone(),
            constraints: table.constraints.clone(),
        };
        let queries = build_action_queries(DatabaseBackend::Sqlite, &action, &[])
            .expect("query build must succeed for valid table");
        for q in &queries {
            let sql = q.build(DatabaseBackend::Sqlite);
            prop_assert!(
                conn.execute(&sql, []).is_ok(),
                "SQLite rejected:\n--- SQL ---\n{sql}\n--- END ---"
            );
        }

        // Round trip: query sqlite_master and confirm our table exists.
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?",
            [&table.name.as_str()],
            |r| r.get(0),
        ).expect("sqlite_master query must succeed");
        prop_assert_eq!(count, 1, "table not created");
    }

    /// CREATE then DROP: emitted DROP SQL must succeed after corresponding CREATE.
    #[test]
    fn sqlite_accepts_create_then_drop(table in arb_table_def_fk_free()) {
        let conn = Connection::open_in_memory().unwrap();
        let create = MigrationAction::CreateTable {
            table: table.name.clone(),
            columns: table.columns.clone(),
            constraints: table.constraints.clone(),
        };
        let drop = MigrationAction::DeleteTable { table: table.name.clone() };

        for action in [&create, &drop] {
            let queries = build_action_queries(DatabaseBackend::Sqlite, action, &[]).unwrap();
            for q in &queries {
                let sql = q.build(DatabaseBackend::Sqlite);
                prop_assert!(conn.execute(&sql, []).is_ok(), "SQLite rejected: {sql}");
            }
        }
    }

    /// PostgreSQL-only and wide NUMERIC types must still emit executable SQLite DDL.
    #[test]
    fn sqlite_accepts_backend_sensitive_column_types(column_type in arb_backend_sensitive_column_type()) {
        let conn = Connection::open_in_memory().expect("in-memory SQLite must open");
        let action = MigrationAction::CreateTable {
            table: "type_probe".into(),
            columns: vec![ColumnDef::new("value", column_type, true)],
            constraints: vec![],
        };

        let queries = build_action_queries(DatabaseBackend::Sqlite, &action, &[])
            .expect("query build must succeed for backend-sensitive types");
        for q in &queries {
            let sql = q.build(DatabaseBackend::Sqlite);
            prop_assert!(
                conn.execute(&sql, []).is_ok(),
                "SQLite rejected:\n--- SQL ---\n{sql}\n--- END ---"
            );
        }
    }
}

/// `SQLite`-safe table strategy: no FK columns/constraints (FK ordering requires
/// global topology that's out of scope for single-table property tests).
fn arb_table_def_fk_free() -> impl Strategy<Value = TableDef> {
    (arb_table_def(), arb_safe_ident()).prop_map(|(mut table, fallback_column)| {
        if table.columns.is_empty() {
            table.columns.push(ColumnDef::new(
                fallback_column,
                ColumnType::Simple(SimpleColumnType::Integer),
                true,
            ));
        }

        table.constraints.clear();
        for col in &mut table.columns {
            col.r#type = sqlite_executable_type(&col.r#type);
            col.default = None;
            col.primary_key = None;
            col.unique = None;
            col.index = None;
            col.foreign_key = None;
        }
        table
    })
}

fn sqlite_executable_type(column_type: &ColumnType) -> ColumnType {
    match column_type {
        ColumnType::Complex(ComplexColumnType::Enum { .. }) => {
            ColumnType::Simple(SimpleColumnType::Text)
        }
        other => other.clone(),
    }
}

fn arb_backend_sensitive_column_type() -> impl Strategy<Value = ColumnType> {
    prop_oneof![
        Just(ColumnType::Simple(SimpleColumnType::Interval)),
        Just(ColumnType::Simple(SimpleColumnType::Timestamptz)),
        Just(ColumnType::Simple(SimpleColumnType::Inet)),
        Just(ColumnType::Simple(SimpleColumnType::Cidr)),
        Just(ColumnType::Simple(SimpleColumnType::Macaddr)),
        Just(ColumnType::Simple(SimpleColumnType::Xml)),
        (1_u32..=100)
            .prop_flat_map(|precision| (Just(precision), 0..=precision.min(20)))
            .prop_map(|(precision, scale)| {
                ColumnType::Complex(ComplexColumnType::Numeric { precision, scale })
            }),
    ]
}
