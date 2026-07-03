//! Strict `PostgreSQL` parser validation using `pg_query` (PG's real C parser).
//!
//! `pg_query` builds the actual `PostgreSQL` parser as a static C library and exposes
//! it via Rust FFI. Any SQL that fails `pg_query::parse` would also be rejected
//! by a real `PostgreSQL` server. This is the strictest PG validation possible
//! without running a daemon.
//!
//! `pg_query@6` = PG 17 parser grammar. Windows (MSVC + GNU) is supported since
//! `libpg_query` 16-5.1.0 / `pg_query` v5+, so this test runs on every platform —
//! no `cfg` gate, ensuring API drift can never slip past a Windows-only dev.

use proptest::prelude::*;
use vespertide_core::arbitrary::arb_table_def;
use vespertide_core::*;
use vespertide_query::{DatabaseBackend, build_action_queries};

proptest! {
    #![proptest_config(ProptestConfig { cases: 128, max_shrink_iters: 5000, ..ProptestConfig::default() })]

    /// Every CREATE TABLE emitted for Postgres backend must be parseable
    /// by PostgreSQL's actual parser.
    #[test]
    fn pg_query_accepts_create_table(table in arb_table_def_fk_free()) {
        let action = MigrationAction::CreateTable {
            table: table.name.clone(),
            columns: table.columns.clone(),
            constraints: table.constraints.clone(),
        };
        let queries = build_action_queries(DatabaseBackend::Postgres, &action, &[])
            .expect("query build must succeed");
        for q in &queries {
            let sql = q.build(DatabaseBackend::Postgres);
            if sql.trim().is_empty() {
                continue;
            }
            match pg_query::parse(&sql) {
                Ok(_) => {},
                Err(e) => prop_assert!(
                    false,
                    "pg_query rejected:\n--- SQL ---\n{sql}\n--- END ---\nerror: {e}"
                ),
            }
        }
    }

    /// DROP TABLE / RENAME TABLE / typed enum migrations etc.
    #[test]
    fn pg_query_accepts_drop_and_rename(name in proptest::string::string_regex("[a-z][a-z0-9_]{0,15}").unwrap()) {
        let actions = [
            MigrationAction::DeleteTable { table: name.clone().into() },
            MigrationAction::RenameTable { from: name.clone().into(), to: format!("{name}_v2").into() },
        ];
        for action in &actions {
            let queries = build_action_queries(DatabaseBackend::Postgres, action, &[])
                .expect("query build must succeed");
            for q in &queries {
                let sql = q.build(DatabaseBackend::Postgres);
                if sql.trim().is_empty() {
                    continue;
                }
                let parsed = pg_query::parse(&sql);
                prop_assert!(
                    parsed.is_ok(),
                    "pg_query rejected: {sql}\nerror: {:?}",
                    parsed.err()
                );
            }
        }
    }
}

fn arb_table_def_fk_free() -> impl Strategy<Value = TableDef> {
    arb_table_def().prop_map(|mut t| {
        t.constraints
            .retain(|c| !matches!(c, TableConstraint::ForeignKey { .. }));
        for col in &mut t.columns {
            col.foreign_key = None;
        }
        t
    })
}
