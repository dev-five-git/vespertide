use proptest::prelude::*;
use vespertide_core::{ColumnDef, ColumnType, MigrationAction, SimpleColumnType, TableDef};
use vespertide_query::sql::helpers::quote_ident;
use vespertide_query::{DatabaseBackend, build_action_queries};

const BACKENDS: [DatabaseBackend; 3] = [
    DatabaseBackend::Postgres,
    DatabaseBackend::MySql,
    DatabaseBackend::Sqlite,
];

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 128,
        ..ProptestConfig::default()
    })]

    // === Property C: determinism ===
    #[test]
    fn sql_emit_is_deterministic(
        action in arb_migration_action(),
        backend in arb_backend(),
    ) {
        let q1 = build_action_queries(backend, &action, &[]);
        let q2 = build_action_queries(backend, &action, &[]);

        prop_assert_eq!(render_result(backend, q1), render_result(backend, q2));
    }

    // === Property D: 3-backend parity ===
    #[test]
    fn create_table_emits_sql_on_all_backends(table in arb_table_def_simple()) {
        for backend in BACKENDS {
            let action = MigrationAction::CreateTable {
                table: table.name.clone(),
                columns: table.columns.clone(),
                constraints: table.constraints.clone(),
            };
            let result = build_action_queries(backend, &action, &[]);
            prop_assert!(result.is_ok(), "backend {backend:?} failed: {:?}", result.err());
            let queries = result.unwrap();
            prop_assert!(!queries.is_empty(), "backend {backend:?} produced no SQL");
            prop_assert!(
                queries.iter().all(|query| !query.build(backend).is_empty()),
                "backend {backend:?} produced an empty SQL statement"
            );
        }
    }

    #[test]
    fn stateless_actions_emit_sql_on_all_backends(action in arb_stateless_sql_action()) {
        for backend in BACKENDS {
            let result = build_action_queries(backend, &action, &[]);
            prop_assert!(result.is_ok(), "backend {backend:?} failed: {:?}", result.err());
            let queries = result.unwrap();
            prop_assert!(!queries.is_empty(), "backend {backend:?} produced no SQL for {action:?}");
            prop_assert!(
                queries.iter().all(|query| !query.build(backend).is_empty()),
                "backend {backend:?} produced an empty SQL statement for {action:?}"
            );
        }
    }

    #[test]
    fn modify_column_comment_sqlite_is_documented_noop(
        table in arb_ident(),
        column in arb_ident(),
        comment in prop::option::of(arb_comment()),
    ) {
        let action = MigrationAction::ModifyColumnComment {
            table: table.into(),
            column: column.into(),
            new_comment: comment,
        };

        let result = build_action_queries(DatabaseBackend::Sqlite, &action, &[]);

        prop_assert!(result.is_ok(), "SQLite comment no-op should succeed: {:?}", result.err());
        prop_assert!(
            result.unwrap().is_empty(),
            "SQLite does not support column comments; ModifyColumnComment is a documented no-op"
        );
    }

    // === Property E: quote_ident safety ===
    #[test]
    fn quote_ident_returns_balanced_quotes(name in ".*") {
        for backend in BACKENDS {
            let quoted = quote_ident(&name, backend);
            let (open, close) = match backend {
                DatabaseBackend::MySql => ('`', '`'),
                DatabaseBackend::Postgres | DatabaseBackend::Sqlite => ('"', '"'),
            };

            prop_assert!(
                quoted.starts_with(open),
                "expected leading {open}, got {quoted:?}"
            );
            prop_assert!(
                quoted.ends_with(close),
                "expected trailing {close}, got {quoted:?}"
            );

            let inner = &quoted[1..quoted.len() - 1];
            let bare_count = inner.matches(open).count();
            let doubled = format!("{open}{open}");
            let doubled_count = inner.matches(doubled.as_str()).count();

            prop_assert_eq!(bare_count, doubled_count * 2);
        }
    }

    #[test]
    fn quote_ident_round_trips_safe_idents(name in "[a-z][a-z0-9_]{0,20}") {
        for backend in BACKENDS {
            let quoted = quote_ident(&name, backend);
            let inner = &quoted[1..quoted.len() - 1];

            prop_assert_eq!(inner, name.as_str());
        }
    }
}

fn render_result(
    backend: DatabaseBackend,
    result: Result<Vec<vespertide_query::BuiltQuery>, vespertide_query::QueryError>,
) -> Result<Vec<String>, String> {
    result
        .map(|queries| {
            queries
                .into_iter()
                .map(|query| query.build(backend))
                .collect()
        })
        .map_err(|err| format!("{err:?}"))
}

fn arb_backend() -> impl Strategy<Value = DatabaseBackend> {
    prop_oneof![
        Just(DatabaseBackend::Postgres),
        Just(DatabaseBackend::MySql),
        Just(DatabaseBackend::Sqlite),
    ]
}

fn arb_ident() -> impl Strategy<Value = String> {
    "[a-z][a-z0-9_]{0,20}"
}

fn arb_comment() -> impl Strategy<Value = String> {
    "[ -~]{0,80}"
}

fn arb_simple_column_type() -> impl Strategy<Value = ColumnType> {
    prop_oneof![
        Just(ColumnType::Simple(SimpleColumnType::SmallInt)),
        Just(ColumnType::Simple(SimpleColumnType::Integer)),
        Just(ColumnType::Simple(SimpleColumnType::BigInt)),
        Just(ColumnType::Simple(SimpleColumnType::Text)),
        Just(ColumnType::Simple(SimpleColumnType::Boolean)),
        Just(ColumnType::Simple(SimpleColumnType::Date)),
        Just(ColumnType::Simple(SimpleColumnType::Timestamp)),
        Just(ColumnType::Simple(SimpleColumnType::Timestamptz)),
        Just(ColumnType::Simple(SimpleColumnType::Uuid)),
        Just(ColumnType::Simple(SimpleColumnType::Json)),
    ]
}

fn arb_column_def_simple() -> impl Strategy<Value = ColumnDef> {
    (arb_ident(), arb_simple_column_type(), any::<bool>())
        .prop_map(|(name, r#type, nullable)| ColumnDef::new(name, r#type, nullable))
}

fn arb_nullable_column_def_simple() -> impl Strategy<Value = ColumnDef> {
    (arb_ident(), arb_simple_column_type())
        .prop_map(|(name, r#type)| ColumnDef::new(name, r#type, true))
}

fn arb_table_def_simple() -> impl Strategy<Value = TableDef> {
    (
        arb_ident(),
        prop::collection::vec(arb_column_def_simple(), 1..=6),
    )
        .prop_map(|(name, columns)| TableDef {
            name: name.into(),
            description: None,
            columns,
            constraints: Vec::new(),
        })
}

fn arb_migration_action() -> impl Strategy<Value = MigrationAction> {
    prop_oneof![
        arb_table_def_simple().prop_map(|table| MigrationAction::CreateTable {
            table: table.name,
            columns: table.columns,
            constraints: table.constraints
        }),
        arb_ident().prop_map(|table| MigrationAction::DeleteTable {
            table: table.into()
        }),
        (arb_ident(), arb_nullable_column_def_simple()).prop_map(|(table, column)| {
            MigrationAction::AddColumn {
                table: table.into(),
                column: Box::new(column),
                fill_with: None,
            }
        }),
        (arb_ident(), arb_ident(), arb_ident()).prop_map(|(table, from, to)| {
            MigrationAction::RenameColumn {
                table: table.into(),
                from: from.into(),
                to: to.into(),
            }
        }),
        (arb_ident(), arb_ident(), prop::option::of(arb_comment())).prop_map(
            |(table, column, new_comment)| MigrationAction::ModifyColumnComment {
                table: table.into(),
                column: column.into(),
                new_comment
            },
        ),
        (arb_ident(), arb_ident()).prop_map(|(from, to)| MigrationAction::RenameTable {
            from: from.into(),
            to: to.into()
        }),
        arb_raw_sql().prop_map(|sql| MigrationAction::RawSql { sql }),
    ]
}

fn arb_stateless_sql_action() -> impl Strategy<Value = MigrationAction> {
    prop_oneof![
        arb_table_def_simple().prop_map(|table| MigrationAction::CreateTable {
            table: table.name,
            columns: table.columns,
            constraints: table.constraints
        }),
        arb_ident().prop_map(|table| MigrationAction::DeleteTable {
            table: table.into()
        }),
        (arb_ident(), arb_nullable_column_def_simple()).prop_map(|(table, column)| {
            MigrationAction::AddColumn {
                table: table.into(),
                column: Box::new(column),
                fill_with: None,
            }
        }),
        (arb_ident(), arb_ident(), arb_ident()).prop_map(|(table, from, to)| {
            MigrationAction::RenameColumn {
                table: table.into(),
                from: from.into(),
                to: to.into(),
            }
        }),
        (arb_ident(), arb_ident()).prop_map(|(from, to)| MigrationAction::RenameTable {
            from: from.into(),
            to: to.into()
        }),
        arb_raw_sql().prop_map(|sql| MigrationAction::RawSql { sql }),
    ]
}

fn arb_raw_sql() -> impl Strategy<Value = String> {
    "SELECT [0-9]{1,6}"
}
