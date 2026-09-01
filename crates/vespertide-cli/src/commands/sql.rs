use anyhow::Result;
use colored::Colorize;
use vespertide_planner::{plan_next_migration_with_baseline, schema_from_plans};
use vespertide_query::{
    DatabaseBackend, PlanQueries, PlanQueriesOptions, build_plan_queries_with_options,
};

use crate::utils::{load_config, load_migrations, load_models};

pub async fn cmd_sql(backend: DatabaseBackend, transaction: bool) -> Result<()> {
    let config = load_config()?;
    let current_models = load_models(&config)?;
    let applied_plans = load_migrations(&config)?;

    // Reconstruct the baseline schema from applied migrations (with prefix applied)
    let prefix = config.prefix();
    let prefixed_plans: Vec<_> = applied_plans
        .into_iter()
        .map(|p| p.with_prefix(prefix))
        .collect();
    let baseline_schema = schema_from_plans(&prefixed_plans)
        .map_err(|e| anyhow::anyhow!("failed to reconstruct schema: {e}"))?;

    // Plan next migration using the pre-computed baseline
    let plan =
        plan_next_migration_with_baseline(&current_models, &prefixed_plans, &baseline_schema)
            .map_err(|e| anyhow::anyhow!("planning error: {e}"))?;

    // Apply prefix to the new plan for SQL generation
    let prefixed_plan = plan.with_prefix(prefix);

    emit_sql(&prefixed_plan, backend, &baseline_schema, transaction)
}

/// Build the per-backend plan queries, optionally wrapping the whole
/// statement stream in a plan-level `BEGIN;` / `COMMIT;` transaction.
///
/// `transaction` is opt-in (CLI `--transaction`): the default emits raw
/// statements so downstream runners that own transaction control are not
/// double-wrapped. Note `MySQL` DDL implicitly commits, so the literal
/// `BEGIN;` / `COMMIT;` are advisory-only on that backend.
fn build_backend_plan_queries(
    plan: &vespertide_core::MigrationPlan,
    current_schema: &[vespertide_core::TableDef],
    transaction: bool,
) -> Result<Vec<PlanQueries>> {
    let options = PlanQueriesOptions {
        wrap_in_transaction: transaction,
    };
    build_plan_queries_with_options(plan, current_schema, options)
        .map_err(|e| anyhow::anyhow!("query build error: {e}"))
}

fn emit_sql(
    plan: &vespertide_core::MigrationPlan,
    backend: DatabaseBackend,
    current_schema: &[vespertide_core::TableDef],
    transaction: bool,
) -> Result<()> {
    if plan.actions.is_empty() {
        println!(
            "{} {}",
            "No differences found.".bright_green(),
            "Schema is up to date; no SQL to emit.".bright_white()
        );
        return Ok(());
    }

    let plan_queries = build_backend_plan_queries(plan, current_schema, transaction)?;

    // Select queries for the specified backend
    let queries: Vec<_> = plan_queries
        .iter()
        .flat_map(|pq| match backend {
            DatabaseBackend::Postgres => &pq.postgres,
            DatabaseBackend::MySql => &pq.mysql,
            DatabaseBackend::Sqlite => &pq.sqlite,
        })
        .collect();

    println!(
        "{} {}",
        "Plan version:".bright_cyan().bold(),
        plan.version.to_string().bright_magenta()
    );
    if let Some(created_at) = &plan.created_at {
        println!(
            "{} {}",
            "Created at:".bright_cyan(),
            created_at.bright_white()
        );
    }
    if let Some(comment) = &plan.comment {
        println!("{} {}", "Comment:".bright_cyan(), comment.bright_white());
    }
    println!(
        "{} {}",
        "Actions:".bright_cyan(),
        plan.actions.len().to_string().bright_yellow()
    );
    println!(
        "{} {}",
        "SQL statements:".bright_cyan().bold(),
        queries.len().to_string().bright_yellow().bold()
    );
    println!();

    for (i, pq) in plan_queries.iter().enumerate() {
        let queries = match backend {
            DatabaseBackend::Postgres => &pq.postgres,
            DatabaseBackend::MySql => &pq.mysql,
            DatabaseBackend::Sqlite => &pq.sqlite,
        };
        println!(
            "{} {}",
            "Action:".bright_cyan(),
            pq.action.to_string().bright_white()
        );
        for (j, q) in queries.iter().enumerate() {
            println!(
                "{}{}. {}",
                (i + 1).to_string().bright_magenta().bold(),
                if queries.len() > 1 {
                    format!("-{}", j + 1)
                } else {
                    String::new()
                }
                .bright_magenta()
                .bold(),
                q.build(backend).trim().bright_white()
            );
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{CwdGuard, write_default_config, write_simple_id_model};
    use serial_test::serial;
    use std::fs;
    use tempfile::tempdir;
    use vespertide_core::{
        ColumnDef, ColumnType, MigrationAction, MigrationPlan, SimpleColumnType, TableConstraint,
        TableDef,
    };

    #[tokio::test]
    #[serial]
    async fn cmd_sql_emits_queries_postgres() {
        let tmp = tempdir().unwrap();
        let _guard = CwdGuard::new(&tmp.path().to_path_buf());

        let _cfg = write_default_config();
        write_simple_id_model("users");

        let result = cmd_sql(DatabaseBackend::Postgres, false).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    #[serial]
    async fn cmd_sql_emits_queries_mysql() {
        let tmp = tempdir().unwrap();
        let _guard = CwdGuard::new(&tmp.path().to_path_buf());

        let _cfg = write_default_config();
        write_simple_id_model("users");

        let result = cmd_sql(DatabaseBackend::MySql, false).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    #[serial]
    async fn cmd_sql_emits_queries_sqlite() {
        let tmp = tempdir().unwrap();
        let _guard = CwdGuard::new(&tmp.path().to_path_buf());

        let _cfg = write_default_config();
        write_simple_id_model("users");

        let result = cmd_sql(DatabaseBackend::Sqlite, false).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    #[serial]
    async fn cmd_sql_no_changes_postgres() {
        let tmp = tempdir().unwrap();
        let _guard = CwdGuard::new(&tmp.path().to_path_buf());

        let cfg = write_default_config();
        write_simple_id_model("users");

        let plan = MigrationPlan {
            id: String::new(),
            comment: None,
            created_at: None,
            version: 1,
            actions: vec![MigrationAction::CreateTable {
                table: "users".into(),
                columns: vec![ColumnDef {
                    name: "id".into(),
                    r#type: ColumnType::Simple(SimpleColumnType::Integer),
                    nullable: false,
                    default: None,
                    comment: None,
                    primary_key: None,
                    unique: None,
                    index: None,
                    foreign_key: None,
                }],
                constraints: vec![TableConstraint::PrimaryKey {
                    auto_increment: false,
                    columns: vec!["id".into()],
                    strategy: vespertide_core::PrimaryKeyAdditionStrategy::default(),
                }],
            }],
        };
        fs::create_dir_all(cfg.migrations_dir()).unwrap();
        let path = cfg.migrations_dir().join("0001_init.json");
        fs::write(path, serde_json::to_string_pretty(&plan).unwrap()).unwrap();

        let result = cmd_sql(DatabaseBackend::Postgres, false).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    #[serial]
    async fn cmd_sql_no_changes_mysql() {
        let tmp = tempdir().unwrap();
        let _guard = CwdGuard::new(&tmp.path().to_path_buf());

        let cfg = write_default_config();
        write_simple_id_model("users");

        let plan = MigrationPlan {
            id: String::new(),
            comment: None,
            created_at: None,
            version: 1,
            actions: vec![MigrationAction::CreateTable {
                table: "users".into(),
                columns: vec![ColumnDef {
                    name: "id".into(),
                    r#type: ColumnType::Simple(SimpleColumnType::Integer),
                    nullable: false,
                    default: None,
                    comment: None,
                    primary_key: None,
                    unique: None,
                    index: None,
                    foreign_key: None,
                }],
                constraints: vec![TableConstraint::PrimaryKey {
                    auto_increment: false,
                    columns: vec!["id".into()],
                    strategy: vespertide_core::PrimaryKeyAdditionStrategy::default(),
                }],
            }],
        };
        fs::create_dir_all(cfg.migrations_dir()).unwrap();
        let path = cfg.migrations_dir().join("0001_init.json");
        fs::write(path, serde_json::to_string_pretty(&plan).unwrap()).unwrap();

        let result = cmd_sql(DatabaseBackend::MySql, false).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    #[serial]
    async fn cmd_sql_no_changes_sqlite() {
        let tmp = tempdir().unwrap();
        let _guard = CwdGuard::new(&tmp.path().to_path_buf());

        let cfg = write_default_config();
        write_simple_id_model("users");

        let plan = MigrationPlan {
            id: String::new(),
            comment: None,
            created_at: None,
            version: 1,
            actions: vec![MigrationAction::CreateTable {
                table: "users".into(),
                columns: vec![ColumnDef {
                    name: "id".into(),
                    r#type: ColumnType::Simple(SimpleColumnType::Integer),
                    nullable: false,
                    default: None,
                    comment: None,
                    primary_key: None,
                    unique: None,
                    index: None,
                    foreign_key: None,
                }],
                constraints: vec![TableConstraint::PrimaryKey {
                    auto_increment: false,
                    columns: vec!["id".into()],
                    strategy: vespertide_core::PrimaryKeyAdditionStrategy::default(),
                }],
            }],
        };
        fs::create_dir_all(cfg.migrations_dir()).unwrap();
        let path = cfg.migrations_dir().join("0001_init.json");
        fs::write(path, serde_json::to_string_pretty(&plan).unwrap()).unwrap();

        let result = cmd_sql(DatabaseBackend::Sqlite, false).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    #[serial]
    async fn emit_sql_prints_created_at_and_comment_postgres() {
        let plan = MigrationPlan {
            id: String::new(),
            comment: Some("with comment".into()),
            created_at: Some("2024-01-02T00:00:00Z".into()),
            version: 1,
            actions: vec![MigrationAction::RawSql {
                sql: "SELECT 1;".into(),
            }],
        };

        let result = emit_sql(&plan, DatabaseBackend::Postgres, &[], false);
        assert!(result.is_ok());
    }

    #[tokio::test]
    #[serial]
    async fn emit_sql_prints_created_at_and_comment_mysql() {
        let plan = MigrationPlan {
            id: String::new(),
            comment: Some("with comment".into()),
            created_at: Some("2024-01-02T00:00:00Z".into()),
            version: 1,
            actions: vec![MigrationAction::RawSql {
                sql: "SELECT 1;".into(),
            }],
        };

        let result = emit_sql(&plan, DatabaseBackend::MySql, &[], false);
        assert!(result.is_ok());
    }

    #[tokio::test]
    #[serial]
    async fn emit_sql_prints_created_at_and_comment_sqlite() {
        let plan = MigrationPlan {
            id: String::new(),
            comment: Some("with comment".into()),
            created_at: Some("2024-01-02T00:00:00Z".into()),
            version: 1,
            actions: vec![MigrationAction::RawSql {
                sql: "SELECT 1;".into(),
            }],
        };

        let result = emit_sql(&plan, DatabaseBackend::Sqlite, &[], false);
        assert!(result.is_ok());
    }

    #[tokio::test]
    #[serial]
    async fn emit_sql_multiple_queries() {
        let plan = MigrationPlan {
            id: String::new(),
            comment: None,
            created_at: None,
            version: 1,
            actions: vec![
                MigrationAction::CreateTable {
                    table: "users".into(),
                    columns: vec![ColumnDef {
                        name: "id".into(),
                        r#type: ColumnType::Simple(SimpleColumnType::Integer),
                        nullable: false,
                        default: None,
                        comment: None,
                        primary_key: None,
                        unique: None,
                        index: None,
                        foreign_key: None,
                    }],
                    constraints: vec![],
                },
                MigrationAction::AddConstraint {
                    table: "users".into(),
                    constraint: TableConstraint::Index {
                        name: Some("idx_id".into()),
                        columns: vec!["id".into()],
                    },
                },
            ],
        };

        let result = emit_sql(&plan, DatabaseBackend::Postgres, &[], false);
        assert!(result.is_ok());
    }

    fn create_users_plan() -> MigrationPlan {
        MigrationPlan {
            id: String::new(),
            comment: None,
            created_at: None,
            version: 1,
            actions: vec![
                MigrationAction::CreateTable {
                    table: "users".into(),
                    columns: vec![ColumnDef {
                        name: "id".into(),
                        r#type: ColumnType::Simple(SimpleColumnType::Integer),
                        nullable: false,
                        default: None,
                        comment: None,
                        primary_key: None,
                        unique: None,
                        index: None,
                        foreign_key: None,
                    }],
                    constraints: vec![],
                },
                MigrationAction::CreateTable {
                    table: "posts".into(),
                    columns: vec![ColumnDef {
                        name: "id".into(),
                        r#type: ColumnType::Simple(SimpleColumnType::Integer),
                        nullable: false,
                        default: None,
                        comment: None,
                        primary_key: None,
                        unique: None,
                        index: None,
                        foreign_key: None,
                    }],
                    constraints: vec![],
                },
            ],
        }
    }

    fn backend_sql(
        plan: &MigrationPlan,
        backend: DatabaseBackend,
        transaction: bool,
    ) -> Vec<String> {
        let pqs = build_backend_plan_queries(plan, &[], transaction).unwrap();
        pqs.iter()
            .flat_map(|pq| match backend {
                DatabaseBackend::Postgres => &pq.postgres,
                DatabaseBackend::MySql => &pq.mysql,
                DatabaseBackend::Sqlite => &pq.sqlite,
            })
            .map(|q| q.build(backend).trim().to_string())
            .collect()
    }

    #[test]
    fn transaction_flag_wraps_sql_in_begin_commit() {
        let plan = create_users_plan();
        for backend in [
            DatabaseBackend::Postgres,
            DatabaseBackend::MySql,
            DatabaseBackend::Sqlite,
        ] {
            let sql = backend_sql(&plan, backend, true);
            assert_eq!(
                sql.first().map(String::as_str),
                Some("BEGIN;"),
                "transaction-wrapped {backend:?} SQL must start with BEGIN; got: {sql:?}"
            );
            assert_eq!(
                sql.last().map(String::as_str),
                Some("COMMIT;"),
                "transaction-wrapped {backend:?} SQL must end with COMMIT; got: {sql:?}"
            );
        }
    }

    #[test]
    fn no_transaction_flag_omits_begin_commit() {
        let plan = create_users_plan();
        for backend in [
            DatabaseBackend::Postgres,
            DatabaseBackend::MySql,
            DatabaseBackend::Sqlite,
        ] {
            let sql = backend_sql(&plan, backend, false);
            assert!(
                !sql.iter().any(|s| s == "BEGIN;" || s == "COMMIT;"),
                "default (no --transaction) {backend:?} SQL must not contain BEGIN/COMMIT; got: {sql:?}"
            );
        }
    }

    #[tokio::test]
    #[serial]
    async fn emit_sql_multiple_queries_per_action() {
        // Test case where a single action generates multiple queries (e.g., SQLite constraint addition)
        // This should trigger the queries.len() > 1 branch (line 89)
        let tmp = tempdir().unwrap();
        let _guard = CwdGuard::new(&tmp.path().to_path_buf());
        let _cfg = write_default_config();
        write_simple_id_model("users");

        // Create a migration that adds a NOT NULL column in SQLite, which generates multiple queries
        let plan = MigrationPlan {
            id: String::new(),
            comment: None,
            created_at: None,
            version: 1,
            actions: vec![MigrationAction::AddColumn {
                table: "users".into(),
                column: Box::new(ColumnDef {
                    name: "nickname".into(),
                    r#type: ColumnType::Simple(SimpleColumnType::Text),
                    nullable: false,
                    default: None,
                    comment: None,
                    primary_key: None,
                    unique: None,
                    index: None,
                    foreign_key: None,
                }),
                fill_with: Some("default".into()),
            }],
        };

        let current_schema = vec![TableDef {
            name: "users".into(),
            description: None,
            columns: vec![ColumnDef {
                name: "id".into(),
                r#type: ColumnType::Simple(SimpleColumnType::Integer),
                nullable: false,
                default: None,
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            }],
            constraints: vec![TableConstraint::PrimaryKey {
                auto_increment: false,
                columns: vec!["id".into()],
                strategy: vespertide_core::PrimaryKeyAdditionStrategy::default(),
            }],
        }];

        let result = emit_sql(&plan, DatabaseBackend::Sqlite, &current_schema, false);
        assert!(result.is_ok());
    }
}
