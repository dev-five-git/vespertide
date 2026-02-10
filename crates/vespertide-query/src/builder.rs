use vespertide_core::{MigrationAction, MigrationPlan, TableDef};
use vespertide_planner::apply_action;

use crate::error::QueryError;
use crate::sql::build_action_queries_with_pending;
use crate::sql::BuiltQuery;
use crate::DatabaseBackend;

pub struct PlanQueries {
    pub action: MigrationAction,
    pub postgres: Vec<BuiltQuery>,
    pub mysql: Vec<BuiltQuery>,
    pub sqlite: Vec<BuiltQuery>,
}

pub fn build_plan_queries(
    plan: &MigrationPlan,
    current_schema: &[TableDef],
) -> Result<Vec<PlanQueries>, QueryError> {
    let mut queries: Vec<PlanQueries> = Vec::new();
    // Clone the schema so we can mutate it as we apply actions
    let mut evolving_schema = current_schema.to_vec();

    for (i, action) in plan.actions.iter().enumerate() {
        // For SQLite: collect pending AddConstraint Index/Unique actions for the same table.
        // These constraints may exist in the logical schema (from AddColumn normalization)
        // but haven't been physically created as DB indexes yet.
        // Without this, a temp table rebuild would recreate these indexes prematurely,
        // causing "index already exists" errors when their AddConstraint actions run later.
        let pending_constraints: Vec<vespertide_core::TableConstraint> =
            if let MigrationAction::AddConstraint { table, .. } = action {
                plan.actions[i + 1..]
                    .iter()
                    .filter_map(|a| {
                        if let MigrationAction::AddConstraint {
                            table: t,
                            constraint,
                        } = a
                        {
                            if t == table
                                && matches!(
                                    constraint,
                                    vespertide_core::TableConstraint::Index { .. }
                                        | vespertide_core::TableConstraint::Unique { .. }
                                )
                            {
                                Some(constraint.clone())
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    })
                    .collect()
            } else {
                vec![]
            };

        // Build queries with the current state of the schema
        let postgres_queries = build_action_queries_with_pending(
            &DatabaseBackend::Postgres,
            action,
            &evolving_schema,
            &pending_constraints,
        )?;
        let mysql_queries = build_action_queries_with_pending(
            &DatabaseBackend::MySql,
            action,
            &evolving_schema,
            &pending_constraints,
        )?;
        let sqlite_queries = build_action_queries_with_pending(
            &DatabaseBackend::Sqlite,
            action,
            &evolving_schema,
            &pending_constraints,
        )?;
        queries.push(PlanQueries {
            action: action.clone(),
            postgres: postgres_queries,
            mysql: mysql_queries,
            sqlite: sqlite_queries,
        });

        // Apply the action to update the schema for the next iteration
        // Note: We ignore errors here because some actions (like DeleteTable) may reference
        // tables that don't exist in the provided current_schema. This is OK for SQL generation
        // purposes - we still generate the correct SQL, and the schema evolution is best-effort.
        let _ = apply_action(&mut evolving_schema, action);
    }
    Ok(queries)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sql::DatabaseBackend;
    use insta::{assert_snapshot, with_settings};
    use rstest::rstest;
    use vespertide_core::{
        ColumnDef, ColumnType, MigrationAction, MigrationPlan, SimpleColumnType,
    };

    fn col(name: &str, ty: ColumnType) -> ColumnDef {
        ColumnDef {
            name: name.to_string(),
            r#type: ty,
            nullable: true,
            default: None,
            comment: None,
            primary_key: None,
            unique: None,
            index: None,
            foreign_key: None,
        }
    }

    #[rstest]
    #[case::empty(
        MigrationPlan {
            id: String::new(),
            comment: None,
            created_at: None,
            version: 1,
            actions: vec![],
        },
        0
    )]
    #[case::single_action(
        MigrationPlan {
            id: String::new(),
            comment: None,
            created_at: None,
            version: 1,
            actions: vec![MigrationAction::DeleteTable {
                table: "users".into(),
            }],
        },
        1
    )]
    #[case::multiple_actions(
        MigrationPlan {
            id: String::new(),
            comment: None,
            created_at: None,
            version: 1,
            actions: vec![
                MigrationAction::CreateTable {
                    table: "users".into(),
                    columns: vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
                    constraints: vec![],
                },
                MigrationAction::DeleteTable {
                    table: "posts".into(),
                },
            ],
        },
        2
    )]
    fn test_build_plan_queries(#[case] plan: MigrationPlan, #[case] expected_count: usize) {
        let result = build_plan_queries(&plan, &[]).unwrap();
        assert_eq!(
            result.len(),
            expected_count,
            "Expected {} queries, got {}",
            expected_count,
            result.len()
        );
    }

    fn build_sql_snapshot(result: &[BuiltQuery], backend: DatabaseBackend) -> String {
        result
            .iter()
            .map(|q| q.build(backend))
            .collect::<Vec<_>>()
            .join(";\n")
    }

    /// Regression test: SQLite must emit DROP INDEX before DROP COLUMN when
    /// the column was created with inline `unique: true` (no explicit table constraint).
    /// Previously, apply_action didn't normalize inline constraints, so the evolving
    /// schema had empty constraints and SQLite's DROP COLUMN failed.
    #[rstest]
    #[case::postgres("postgres", DatabaseBackend::Postgres)]
    #[case::mysql("mysql", DatabaseBackend::MySql)]
    #[case::sqlite("sqlite", DatabaseBackend::Sqlite)]
    fn test_delete_column_after_create_table_with_inline_unique(
        #[case] title: &str,
        #[case] backend: DatabaseBackend,
    ) {
        let mut col_with_unique = col("gift_code", ColumnType::Simple(SimpleColumnType::Text));
        col_with_unique.unique = Some(vespertide_core::StrOrBoolOrArray::Bool(true));

        let plan = MigrationPlan {
            id: String::new(),
            comment: None,
            created_at: None,
            version: 1,
            actions: vec![
                MigrationAction::CreateTable {
                    table: "gift".into(),
                    columns: vec![
                        col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                        col_with_unique,
                    ],
                    constraints: vec![], // No explicit constraints - only inline unique: true
                },
                MigrationAction::DeleteColumn {
                    table: "gift".into(),
                    column: "gift_code".into(),
                },
            ],
        };

        let result = build_plan_queries(&plan, &[]).unwrap();
        let queries = match backend {
            DatabaseBackend::Postgres => &result[1].postgres,
            DatabaseBackend::MySql => &result[1].mysql,
            DatabaseBackend::Sqlite => &result[1].sqlite,
        };
        let sql = build_sql_snapshot(queries, backend);

        with_settings!({ snapshot_suffix => format!("inline_unique_{}", title) }, {
            assert_snapshot!(sql);
        });
    }

    /// Same regression test for inline `index: true`.
    #[rstest]
    #[case::postgres("postgres", DatabaseBackend::Postgres)]
    #[case::mysql("mysql", DatabaseBackend::MySql)]
    #[case::sqlite("sqlite", DatabaseBackend::Sqlite)]
    fn test_delete_column_after_create_table_with_inline_index(
        #[case] title: &str,
        #[case] backend: DatabaseBackend,
    ) {
        let mut col_with_index = col("email", ColumnType::Simple(SimpleColumnType::Text));
        col_with_index.index = Some(vespertide_core::StrOrBoolOrArray::Bool(true));

        let plan = MigrationPlan {
            id: String::new(),
            comment: None,
            created_at: None,
            version: 1,
            actions: vec![
                MigrationAction::CreateTable {
                    table: "users".into(),
                    columns: vec![
                        col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                        col_with_index,
                    ],
                    constraints: vec![],
                },
                MigrationAction::DeleteColumn {
                    table: "users".into(),
                    column: "email".into(),
                },
            ],
        };

        let result = build_plan_queries(&plan, &[]).unwrap();
        let queries = match backend {
            DatabaseBackend::Postgres => &result[1].postgres,
            DatabaseBackend::MySql => &result[1].mysql,
            DatabaseBackend::Sqlite => &result[1].sqlite,
        };
        let sql = build_sql_snapshot(queries, backend);

        with_settings!({ snapshot_suffix => format!("inline_index_{}", title) }, {
            assert_snapshot!(sql);
        });
    }

    #[test]
    fn test_build_plan_queries_sql_content() {
        let plan = MigrationPlan {
            id: String::new(),
            comment: None,
            created_at: None,
            version: 1,
            actions: vec![
                MigrationAction::CreateTable {
                    table: "users".into(),
                    columns: vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
                    constraints: vec![],
                },
                MigrationAction::DeleteTable {
                    table: "posts".into(),
                },
            ],
        };

        let result = build_plan_queries(&plan, &[]).unwrap();
        assert_eq!(result.len(), 2);

        // Test PostgreSQL output
        let sql1 = result[0]
            .postgres
            .iter()
            .map(|q| q.build(DatabaseBackend::Postgres))
            .collect::<Vec<_>>()
            .join(";\n");
        assert!(sql1.contains("CREATE TABLE"));
        assert!(sql1.contains("\"users\""));
        assert!(sql1.contains("\"id\""));

        let sql2 = result[1]
            .postgres
            .iter()
            .map(|q| q.build(DatabaseBackend::Postgres))
            .collect::<Vec<_>>()
            .join(";\n");
        assert!(sql2.contains("DROP TABLE"));
        assert!(sql2.contains("\"posts\""));

        // Test MySQL output
        let sql1_mysql = result[0]
            .mysql
            .iter()
            .map(|q| q.build(DatabaseBackend::MySql))
            .collect::<Vec<_>>()
            .join(";\n");
        assert!(sql1_mysql.contains("`users`"));

        let sql2_mysql = result[1]
            .mysql
            .iter()
            .map(|q| q.build(DatabaseBackend::MySql))
            .collect::<Vec<_>>()
            .join(";\n");
        assert!(sql2_mysql.contains("`posts`"));
    }

    // ── Helpers for constraint migration tests ──────────────────────────

    use vespertide_core::{ReferenceAction, TableConstraint};

    fn fk_constraint() -> TableConstraint {
        TableConstraint::ForeignKey {
            name: None,
            columns: vec!["category_id".into()],
            ref_table: "category".into(),
            ref_columns: vec!["id".into()],
            on_delete: Some(ReferenceAction::Cascade),
            on_update: None,
        }
    }

    fn unique_constraint() -> TableConstraint {
        TableConstraint::Unique {
            name: None,
            columns: vec!["category_id".into()],
        }
    }

    fn index_constraint() -> TableConstraint {
        TableConstraint::Index {
            name: None,
            columns: vec!["category_id".into()],
        }
    }

    /// Build a plan that adds a column then adds constraints in the given order.
    fn plan_add_column_with_constraints(order: &[TableConstraint]) -> MigrationPlan {
        let mut actions: Vec<MigrationAction> = vec![MigrationAction::AddColumn {
            table: "product".into(),
            column: Box::new(col(
                "category_id",
                ColumnType::Simple(SimpleColumnType::BigInt),
            )),
            fill_with: None,
        }];
        for c in order {
            actions.push(MigrationAction::AddConstraint {
                table: "product".into(),
                constraint: c.clone(),
            });
        }
        MigrationPlan {
            id: String::new(),
            comment: None,
            created_at: None,
            version: 1,
            actions,
        }
    }

    /// Build a plan that removes constraints in the given order then drops the column.
    fn plan_remove_constraints_then_drop(order: &[TableConstraint]) -> MigrationPlan {
        let mut actions: Vec<MigrationAction> = Vec::new();
        for c in order {
            actions.push(MigrationAction::RemoveConstraint {
                table: "product".into(),
                constraint: c.clone(),
            });
        }
        actions.push(MigrationAction::DeleteColumn {
            table: "product".into(),
            column: "category_id".into(),
        });
        MigrationPlan {
            id: String::new(),
            comment: None,
            created_at: None,
            version: 1,
            actions,
        }
    }

    /// Schema with an existing table that has NO constraints on category_id (for add tests).
    fn base_schema_no_constraints() -> Vec<TableDef> {
        vec![TableDef {
            name: "product".into(),
            description: None,
            columns: vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
            constraints: vec![],
        }]
    }

    /// Schema with an existing table that HAS FK + Unique + Index on category_id (for remove tests).
    fn base_schema_with_all_constraints() -> Vec<TableDef> {
        vec![TableDef {
            name: "product".into(),
            description: None,
            columns: vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                col("category_id", ColumnType::Simple(SimpleColumnType::BigInt)),
            ],
            constraints: vec![fk_constraint(), unique_constraint(), index_constraint()],
        }]
    }

    /// Collect ALL SQL statements from a plan result for a given backend.
    fn collect_all_sql(result: &[PlanQueries], backend: DatabaseBackend) -> String {
        result
            .iter()
            .enumerate()
            .map(|(i, pq)| {
                let queries = match backend {
                    DatabaseBackend::Postgres => &pq.postgres,
                    DatabaseBackend::MySql => &pq.mysql,
                    DatabaseBackend::Sqlite => &pq.sqlite,
                };
                let sql = build_sql_snapshot(queries, backend);
                format!("-- Action {}: {:?}\n{}", i, pq.action, sql)
            })
            .collect::<Vec<_>>()
            .join("\n\n")
    }

    /// Assert no duplicate CREATE INDEX / CREATE UNIQUE INDEX within a single
    /// action's SQLite output. Cross-action duplicates are allowed because a
    /// temp table rebuild (DROP + RENAME) legitimately destroys and recreates
    /// indexes that a prior action already created.
    fn assert_no_duplicate_indexes_per_action(result: &[PlanQueries]) {
        for (i, pq) in result.iter().enumerate() {
            let stmts: Vec<String> = pq
                .sqlite
                .iter()
                .map(|q| q.build(DatabaseBackend::Sqlite))
                .collect();

            let index_stmts: Vec<&String> = stmts
                .iter()
                .filter(|s| s.contains("CREATE INDEX") || s.contains("CREATE UNIQUE INDEX"))
                .collect();

            let mut seen = std::collections::HashSet::new();
            for stmt in &index_stmts {
                assert!(
                    seen.insert(stmt.as_str()),
                    "Duplicate index within action {} ({:?}):\n  {}\nAll index statements in this action:\n{}",
                    i,
                    pq.action,
                    stmt,
                    index_stmts
                        .iter()
                        .map(|s| format!("  {}", s))
                        .collect::<Vec<_>>()
                        .join("\n")
                );
            }
        }
    }

    /// Assert that no AddConstraint Index/Unique action produces an index that
    /// was already recreated by a preceding temp-table rebuild within the same plan.
    /// This catches the original bug: FK temp-table rebuild creating an index that
    /// a later AddConstraint INDEX also creates (without DROP TABLE in between).
    fn assert_no_orphan_duplicate_indexes(result: &[PlanQueries]) {
        // Track indexes that exist after each action.
        // A DROP TABLE resets the set; CREATE INDEX adds to it.
        let mut live_indexes: std::collections::HashSet<String> = std::collections::HashSet::new();

        for pq in result {
            let stmts: Vec<String> = pq
                .sqlite
                .iter()
                .map(|q| q.build(DatabaseBackend::Sqlite))
                .collect();

            // If this action does a DROP TABLE, all indexes are destroyed
            if stmts.iter().any(|s| s.starts_with("DROP TABLE")) {
                live_indexes.clear();
            }

            for stmt in &stmts {
                if stmt.contains("CREATE INDEX") || stmt.contains("CREATE UNIQUE INDEX") {
                    assert!(
                        live_indexes.insert(stmt.clone()),
                        "Index would already exist when action {:?} tries to create it:\n  {}\nCurrently live indexes:\n{}",
                        pq.action,
                        stmt,
                        live_indexes
                            .iter()
                            .map(|s| format!("  {}", s))
                            .collect::<Vec<_>>()
                            .join("\n")
                    );
                }
            }

            // DROP INDEX removes from live set
            for stmt in &stmts {
                if stmt.starts_with("DROP INDEX") {
                    live_indexes.retain(|s| {
                        // Extract index name from DROP INDEX "name"
                        let drop_name = stmt
                            .strip_prefix("DROP INDEX \"")
                            .and_then(|s| s.strip_suffix('"'));
                        if let Some(name) = drop_name {
                            !s.contains(&format!("\"{}\"", name))
                        } else {
                            true
                        }
                    });
                }
            }
        }
    }

    // ── Add column + FK/Unique/Index – all orderings ─────────────────────

    #[rstest]
    #[case::fk_unique_index("fk_uq_ix", &[fk_constraint(), unique_constraint(), index_constraint()])]
    #[case::fk_index_unique("fk_ix_uq", &[fk_constraint(), index_constraint(), unique_constraint()])]
    #[case::unique_fk_index("uq_fk_ix", &[unique_constraint(), fk_constraint(), index_constraint()])]
    #[case::unique_index_fk("uq_ix_fk", &[unique_constraint(), index_constraint(), fk_constraint()])]
    #[case::index_fk_unique("ix_fk_uq", &[index_constraint(), fk_constraint(), unique_constraint()])]
    #[case::index_unique_fk("ix_uq_fk", &[index_constraint(), unique_constraint(), fk_constraint()])]
    fn test_add_column_with_fk_unique_index_all_orderings(
        #[case] title: &str,
        #[case] order: &[TableConstraint],
    ) {
        let plan = plan_add_column_with_constraints(order);
        let schema = base_schema_no_constraints();
        let result = build_plan_queries(&plan, &schema).unwrap();

        // Core invariant: no conflicting duplicate indexes in SQLite
        assert_no_duplicate_indexes_per_action(&result);
        assert_no_orphan_duplicate_indexes(&result);

        // Snapshot per backend
        for (backend, label) in [
            (DatabaseBackend::Postgres, "postgres"),
            (DatabaseBackend::MySql, "mysql"),
            (DatabaseBackend::Sqlite, "sqlite"),
        ] {
            let sql = collect_all_sql(&result, backend);
            with_settings!({ snapshot_suffix => format!("add_col_{}_{}", title, label) }, {
                assert_snapshot!(sql);
            });
        }
    }

    // ── Remove FK/Unique/Index then drop column – all orderings ──────────

    #[rstest]
    #[case::fk_unique_index("fk_uq_ix", &[fk_constraint(), unique_constraint(), index_constraint()])]
    #[case::fk_index_unique("fk_ix_uq", &[fk_constraint(), index_constraint(), unique_constraint()])]
    #[case::unique_fk_index("uq_fk_ix", &[unique_constraint(), fk_constraint(), index_constraint()])]
    #[case::unique_index_fk("uq_ix_fk", &[unique_constraint(), index_constraint(), fk_constraint()])]
    #[case::index_fk_unique("ix_fk_uq", &[index_constraint(), fk_constraint(), unique_constraint()])]
    #[case::index_unique_fk("ix_uq_fk", &[index_constraint(), unique_constraint(), fk_constraint()])]
    fn test_remove_fk_unique_index_then_drop_column_all_orderings(
        #[case] title: &str,
        #[case] order: &[TableConstraint],
    ) {
        let plan = plan_remove_constraints_then_drop(order);
        let schema = base_schema_with_all_constraints();
        let result = build_plan_queries(&plan, &schema).unwrap();

        // Snapshot per backend
        for (backend, label) in [
            (DatabaseBackend::Postgres, "postgres"),
            (DatabaseBackend::MySql, "mysql"),
            (DatabaseBackend::Sqlite, "sqlite"),
        ] {
            let sql = collect_all_sql(&result, backend);
            with_settings!({ snapshot_suffix => format!("rm_col_{}_{}", title, label) }, {
                assert_snapshot!(sql);
            });
        }
    }

    // ── Pair-wise: FK + Index only (original bug scenario) ───────────────

    #[rstest]
    #[case::fk_then_index("fk_ix", &[fk_constraint(), index_constraint()])]
    #[case::index_then_fk("ix_fk", &[index_constraint(), fk_constraint()])]
    fn test_add_column_with_fk_and_index_pair(
        #[case] title: &str,
        #[case] order: &[TableConstraint],
    ) {
        let plan = plan_add_column_with_constraints(order);
        let schema = base_schema_no_constraints();
        let result = build_plan_queries(&plan, &schema).unwrap();

        assert_no_duplicate_indexes_per_action(&result);
        assert_no_orphan_duplicate_indexes(&result);

        for (backend, label) in [
            (DatabaseBackend::Postgres, "postgres"),
            (DatabaseBackend::MySql, "mysql"),
            (DatabaseBackend::Sqlite, "sqlite"),
        ] {
            let sql = collect_all_sql(&result, backend);
            with_settings!({ snapshot_suffix => format!("add_col_pair_{}_{}", title, label) }, {
                assert_snapshot!(sql);
            });
        }
    }

    // ── Pair-wise: FK + Unique only ──────────────────────────────────────

    #[rstest]
    #[case::fk_then_unique("fk_uq", &[fk_constraint(), unique_constraint()])]
    #[case::unique_then_fk("uq_fk", &[unique_constraint(), fk_constraint()])]
    fn test_add_column_with_fk_and_unique_pair(
        #[case] title: &str,
        #[case] order: &[TableConstraint],
    ) {
        let plan = plan_add_column_with_constraints(order);
        let schema = base_schema_no_constraints();
        let result = build_plan_queries(&plan, &schema).unwrap();

        assert_no_duplicate_indexes_per_action(&result);
        assert_no_orphan_duplicate_indexes(&result);

        for (backend, label) in [
            (DatabaseBackend::Postgres, "postgres"),
            (DatabaseBackend::MySql, "mysql"),
            (DatabaseBackend::Sqlite, "sqlite"),
        ] {
            let sql = collect_all_sql(&result, backend);
            with_settings!({ snapshot_suffix => format!("add_col_pair_{}_{}", title, label) }, {
                assert_snapshot!(sql);
            });
        }
    }

    // ── Duplicate FK in temp table CREATE TABLE ──────────────────────────

    /// Regression test: when AddColumn adds a column with an inline FK, the
    /// evolving schema already contains the FK constraint (from normalization).
    /// Then AddConstraint FK pushes the same FK again into new_constraints,
    /// producing a duplicate FOREIGN KEY clause in the SQLite temp table.
    #[rstest]
    #[case::postgres("postgres", DatabaseBackend::Postgres)]
    #[case::mysql("mysql", DatabaseBackend::MySql)]
    #[case::sqlite("sqlite", DatabaseBackend::Sqlite)]
    fn test_add_column_with_fk_no_duplicate_fk_in_temp_table(
        #[case] label: &str,
        #[case] backend: DatabaseBackend,
    ) {
        let schema = vec![
            TableDef {
                name: "project".into(),
                description: None,
                columns: vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
                constraints: vec![],
            },
            TableDef {
                name: "companion".into(),
                description: None,
                columns: vec![
                    col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                    col("user_id", ColumnType::Simple(SimpleColumnType::BigInt)),
                ],
                constraints: vec![
                    TableConstraint::ForeignKey {
                        name: None,
                        columns: vec!["user_id".into()],
                        ref_table: "user".into(),
                        ref_columns: vec!["id".into()],
                        on_delete: Some(ReferenceAction::Cascade),
                        on_update: None,
                    },
                    TableConstraint::Unique {
                        name: Some("invite_code".into()),
                        columns: vec!["invite_code".into()],
                    },
                    TableConstraint::Index {
                        name: None,
                        columns: vec!["user_id".into()],
                    },
                ],
            },
        ];

        let plan = MigrationPlan {
            id: String::new(),
            comment: None,
            created_at: None,
            version: 1,
            actions: vec![
                MigrationAction::AddColumn {
                    table: "companion".into(),
                    column: Box::new(ColumnDef {
                        name: "project_id".into(),
                        r#type: ColumnType::Simple(SimpleColumnType::BigInt),
                        nullable: false,
                        default: None,
                        comment: None,
                        primary_key: None,
                        unique: None,
                        index: None,
                        foreign_key: Some(
                            vespertide_core::schema::foreign_key::ForeignKeySyntax::String(
                                "project.id".into(),
                            ),
                        ),
                    }),
                    fill_with: None,
                },
                MigrationAction::AddConstraint {
                    table: "companion".into(),
                    constraint: TableConstraint::ForeignKey {
                        name: None,
                        columns: vec!["project_id".into()],
                        ref_table: "project".into(),
                        ref_columns: vec!["id".into()],
                        on_delete: Some(ReferenceAction::Cascade),
                        on_update: None,
                    },
                },
                MigrationAction::AddConstraint {
                    table: "companion".into(),
                    constraint: TableConstraint::Index {
                        name: None,
                        columns: vec!["project_id".into()],
                    },
                },
            ],
        };

        let result = build_plan_queries(&plan, &schema).unwrap();

        assert_no_duplicate_indexes_per_action(&result);
        assert_no_orphan_duplicate_indexes(&result);

        let sql = collect_all_sql(&result, backend);
        with_settings!({ snapshot_suffix => format!("dup_fk_{}", label) }, {
            assert_snapshot!(sql);
        });
    }
}
