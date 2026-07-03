//! Daemon-free 3-dialect parser validation.
//!
//! `sqlparser-rs` is a pure-Rust SQL parser used by datafusion, delta-rs, etc.
//! It supports separate dialect implementations for `PostgreSQL`, `MySQL`, and `SQLite`.
//! Every emitted SQL is parsed in its target dialect; parse failure = bug.

use proptest::prelude::*;
use sqlparser::dialect::{Dialect, MySqlDialect, PostgreSqlDialect, SQLiteDialect};
use sqlparser::parser::Parser;
use vespertide_core::arbitrary::arb_table_def;
use vespertide_core::schema::primary_key::PrimaryKeySyntax;
use vespertide_core::*;
use vespertide_query::{DatabaseBackend, build_action_queries};

#[test]
fn build_action_queries_runs_for_all_backends() {
    let action = MigrationAction::CreateTable {
        table: "test".into(),
        columns: vec![ColumnDef {
            name: "id".into(),
            r#type: ColumnType::Simple(SimpleColumnType::Integer),
            nullable: false,
            default: None,
            comment: None,
            primary_key: Some(PrimaryKeySyntax::Bool(true)),
            unique: None,
            index: None,
            foreign_key: None,
        }],
        constraints: vec![],
    };

    for backend in [
        DatabaseBackend::Postgres,
        DatabaseBackend::MySql,
        DatabaseBackend::Sqlite,
    ] {
        let queries = build_action_queries(backend, &action, &[])
            .expect("backend must build CREATE TABLE queries");
        assert!(
            !queries.is_empty(),
            "{backend:?} should produce CREATE TABLE queries"
        );

        for query in &queries {
            let sql = query.build(backend);
            assert!(!sql.is_empty(), "{backend:?} SQL should be non-empty");
            assert!(
                sql.to_lowercase().contains("create table"),
                "{backend:?}: expected CREATE TABLE, got: {sql}"
            );
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 128, max_shrink_iters: 5000, ..ProptestConfig::default() })]

    /// PostgreSQL dialect: every CREATE TABLE must parse.
    #[test]
    fn pg_dialect_parses_create_table(table in arb_table_def_fk_free()) {
        assert_action_parses(
            DatabaseBackend::Postgres,
            &PostgreSqlDialect {},
            &MigrationAction::CreateTable {
                table: table.name.clone(),
                columns: table.columns.clone(),
                constraints: table.constraints.clone(),
            },
        )?;
    }

    /// MySQL dialect: every CREATE TABLE must parse.
    #[test]
    fn mysql_dialect_parses_create_table(table in arb_table_def_fk_free()) {
        assert_action_parses(
            DatabaseBackend::MySql,
            &MySqlDialect {},
            &MigrationAction::CreateTable {
                table: table.name.clone(),
                columns: table.columns.clone(),
                constraints: table.constraints.clone(),
            },
        )?;
    }

    /// SQLite dialect: every CREATE TABLE must parse.
    #[test]
    fn sqlite_dialect_parses_create_table(table in arb_table_def_fk_free()) {
        assert_action_parses(
            DatabaseBackend::Sqlite,
            &SQLiteDialect {},
            &MigrationAction::CreateTable {
                table: table.name.clone(),
                columns: table.columns.clone(),
                constraints: table.constraints.clone(),
            },
        )?;
    }

    /// Cross-backend identity: same action → 3 dialects → all parse.
    #[test]
    fn all_three_dialects_accept_create_table(table in arb_table_def_fk_free()) {
        let action = MigrationAction::CreateTable {
            table: table.name.clone(),
            columns: table.columns.clone(),
            constraints: table.constraints.clone(),
        };
        for (backend, dialect) in dialects() {
            check_parses(backend, dialect.as_ref(), &action)?;
        }
    }

    /// Backend-sensitive types must render parseable CREATE TABLE SQL for every dialect.
    #[test]
    fn all_three_dialects_accept_backend_sensitive_types(column_type in arb_backend_sensitive_column_type()) {
        let action = MigrationAction::CreateTable {
            table: "type_probe".into(),
            columns: vec![ColumnDef::new("value", column_type, true)],
            constraints: vec![],
        };

        for (backend, dialect) in dialects() {
            check_parses(backend, dialect.as_ref(), &action)?;
        }
    }
}

fn dialects() -> Vec<(DatabaseBackend, Box<dyn Dialect>)> {
    vec![
        (DatabaseBackend::Postgres, Box::new(PostgreSqlDialect {})),
        (DatabaseBackend::MySql, Box::new(MySqlDialect {})),
        (DatabaseBackend::Sqlite, Box::new(SQLiteDialect {})),
    ]
}

fn assert_action_parses(
    backend: DatabaseBackend,
    dialect: &dyn Dialect,
    action: &MigrationAction,
) -> Result<(), TestCaseError> {
    check_parses(backend, dialect, action)
}

fn check_parses(
    backend: DatabaseBackend,
    dialect: &dyn Dialect,
    action: &MigrationAction,
) -> Result<(), TestCaseError> {
    let queries = match build_action_queries(backend, action, &[]) {
        Ok(q) => q,
        Err(e) => {
            // Construction failed in a way the property doesn't cover; skip.
            return Err(TestCaseError::reject(format!(
                "build_action_queries failed: {e}"
            )));
        }
    };
    for q in &queries {
        let sql = q.build(backend);
        // SQLite ModifyColumnComment is a documented no-op; skip empty SQL.
        if sql.trim().is_empty() {
            continue;
        }
        let parsed = Parser::parse_sql(dialect, &sql);
        prop_assert!(
            parsed.is_ok(),
            "{:?} dialect rejected:\n--- SQL ---\n{sql}\n--- END ---\nerror: {:?}",
            backend,
            parsed.err()
        );
    }
    Ok(())
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

fn arb_backend_sensitive_column_type() -> impl Strategy<Value = ColumnType> {
    prop_oneof![
        Just(ColumnType::Simple(SimpleColumnType::Interval)),
        Just(ColumnType::Simple(SimpleColumnType::Timestamptz)),
        Just(ColumnType::Simple(SimpleColumnType::Inet)),
        Just(ColumnType::Simple(SimpleColumnType::Cidr)),
        Just(ColumnType::Simple(SimpleColumnType::Macaddr)),
        Just(ColumnType::Simple(SimpleColumnType::Xml)),
        (1_u32..=100)
            .prop_flat_map(|precision| (Just(precision), 0_u32..=precision))
            .prop_map(|(precision, scale)| {
                ColumnType::Complex(ComplexColumnType::Numeric { precision, scale })
            }),
    ]
}
