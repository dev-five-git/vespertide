use anyhow::Result;
use colored::Colorize;
use vespertide_planner::{plan_next_migration_with_baseline, schema_from_plans};
use vespertide_query::{DatabaseBackend, build_plan_queries};

use crate::utils::{load_config, load_migrations, load_models};

pub fn cmd_sql(backend: DatabaseBackend) -> Result<()> {
    let config = load_config()?;
    let current_models = load_models(&config)?;
    let applied_plans = load_migrations(&config)?;

    // Reconstruct the baseline schema from applied migrations
    let baseline_schema = schema_from_plans(&applied_plans)
        .map_err(|e| anyhow::anyhow!("failed to reconstruct schema: {}", e))?;

    // Plan next migration using the pre-computed baseline
    let plan = plan_next_migration_with_baseline(&current_models, &applied_plans, &baseline_schema)
        .map_err(|e| anyhow::anyhow!("planning error: {}", e))?;

    emit_sql(&plan, backend, &baseline_schema)
}

fn emit_sql(
    plan: &vespertide_core::MigrationPlan,
    backend: DatabaseBackend,
    current_schema: &[vespertide_core::TableDef],
) -> Result<()> {
    if plan.actions.is_empty() {
        println!(
            "{} {}",
            "No differences found.".bright_green(),
            "Schema is up to date; no SQL to emit.".bright_white()
        );
        return Ok(());
    }

    let plan_queries = build_plan_queries(plan, current_schema)
        .map_err(|e| anyhow::anyhow!("query build error: {}", e))?;

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
                    "".to_string()
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
    use serial_test::serial;
    use std::fs;
    use std::path::PathBuf;
    use tempfile::tempdir;
    use vespertide_config::VespertideConfig;
    use vespertide_core::{
        ColumnDef, ColumnType, MigrationAction, MigrationPlan, SimpleColumnType, TableConstraint,
        TableDef,
    };

    struct CwdGuard {
        original: PathBuf,
    }

    impl CwdGuard {
        fn new(dir: &PathBuf) -> Self {
            let original = std::env::current_dir().unwrap();
            std::env::set_current_dir(dir).unwrap();
            Self { original }
        }
    }

    impl Drop for CwdGuard {
        fn drop(&mut self) {
            let _ = std::env::set_current_dir(&self.original);
        }
    }

    fn write_config() -> VespertideConfig {
        let cfg = VespertideConfig::default();
        let text = serde_json::to_string_pretty(&cfg).unwrap();
        fs::write("vespertide.json", text).unwrap();
        cfg
    }

    fn write_model(name: &str) {
        let models_dir = PathBuf::from("models");
        fs::create_dir_all(&models_dir).unwrap();
        let table = TableDef {
            name: name.to_string(),
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
            }],
            indexes: vec![],
        };
        let path = models_dir.join(format!("{name}.json"));
        fs::write(path, serde_json::to_string_pretty(&table).unwrap()).unwrap();
    }

    #[test]
    #[serial]
    fn cmd_sql_emits_queries_postgres() {
        let tmp = tempdir().unwrap();
        let _guard = CwdGuard::new(&tmp.path().to_path_buf());

        let _cfg = write_config();
        write_model("users");

        let result = cmd_sql(DatabaseBackend::Postgres);
        assert!(result.is_ok());
    }

    #[test]
    #[serial]
    fn cmd_sql_emits_queries_mysql() {
        let tmp = tempdir().unwrap();
        let _guard = CwdGuard::new(&tmp.path().to_path_buf());

        let _cfg = write_config();
        write_model("users");

        let result = cmd_sql(DatabaseBackend::MySql);
        assert!(result.is_ok());
    }

    #[test]
    #[serial]
    fn cmd_sql_emits_queries_sqlite() {
        let tmp = tempdir().unwrap();
        let _guard = CwdGuard::new(&tmp.path().to_path_buf());

        let _cfg = write_config();
        write_model("users");

        let result = cmd_sql(DatabaseBackend::Sqlite);
        assert!(result.is_ok());
    }

    #[test]
    #[serial]
    fn cmd_sql_no_changes_postgres() {
        let tmp = tempdir().unwrap();
        let _guard = CwdGuard::new(&tmp.path().to_path_buf());

        let cfg = write_config();
        write_model("users");

        let plan = MigrationPlan {
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
                }],
            }],
        };
        fs::create_dir_all(cfg.migrations_dir()).unwrap();
        let path = cfg.migrations_dir().join("0001_init.json");
        fs::write(path, serde_json::to_string_pretty(&plan).unwrap()).unwrap();

        let result = cmd_sql(DatabaseBackend::Postgres);
        assert!(result.is_ok());
    }

    #[test]
    #[serial]
    fn cmd_sql_no_changes_mysql() {
        let tmp = tempdir().unwrap();
        let _guard = CwdGuard::new(&tmp.path().to_path_buf());

        let cfg = write_config();
        write_model("users");

        let plan = MigrationPlan {
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
                }],
            }],
        };
        fs::create_dir_all(cfg.migrations_dir()).unwrap();
        let path = cfg.migrations_dir().join("0001_init.json");
        fs::write(path, serde_json::to_string_pretty(&plan).unwrap()).unwrap();

        let result = cmd_sql(DatabaseBackend::MySql);
        assert!(result.is_ok());
    }

    #[test]
    #[serial]
    fn cmd_sql_no_changes_sqlite() {
        let tmp = tempdir().unwrap();
        let _guard = CwdGuard::new(&tmp.path().to_path_buf());

        let cfg = write_config();
        write_model("users");

        let plan = MigrationPlan {
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
                }],
            }],
        };
        fs::create_dir_all(cfg.migrations_dir()).unwrap();
        let path = cfg.migrations_dir().join("0001_init.json");
        fs::write(path, serde_json::to_string_pretty(&plan).unwrap()).unwrap();

        let result = cmd_sql(DatabaseBackend::Sqlite);
        assert!(result.is_ok());
    }

    #[test]
    #[serial]
    fn emit_sql_prints_created_at_and_comment_postgres() {
        let plan = MigrationPlan {
            comment: Some("with comment".into()),
            created_at: Some("2024-01-02T00:00:00Z".into()),
            version: 1,
            actions: vec![MigrationAction::RawSql {
                sql: "SELECT 1;".into(),
            }],
        };

        let result = emit_sql(&plan, DatabaseBackend::Postgres, &[]);
        assert!(result.is_ok());
    }

    #[test]
    #[serial]
    fn emit_sql_prints_created_at_and_comment_mysql() {
        let plan = MigrationPlan {
            comment: Some("with comment".into()),
            created_at: Some("2024-01-02T00:00:00Z".into()),
            version: 1,
            actions: vec![MigrationAction::RawSql {
                sql: "SELECT 1;".into(),
            }],
        };

        let result = emit_sql(&plan, DatabaseBackend::MySql, &[]);
        assert!(result.is_ok());
    }

    #[test]
    #[serial]
    fn emit_sql_prints_created_at_and_comment_sqlite() {
        let plan = MigrationPlan {
            comment: Some("with comment".into()),
            created_at: Some("2024-01-02T00:00:00Z".into()),
            version: 1,
            actions: vec![MigrationAction::RawSql {
                sql: "SELECT 1;".into(),
            }],
        };

        let result = emit_sql(&plan, DatabaseBackend::Sqlite, &[]);
        assert!(result.is_ok());
    }

    #[test]
    #[serial]
    fn emit_sql_multiple_queries() {
        let plan = MigrationPlan {
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
                MigrationAction::AddIndex {
                    table: "users".into(),
                    index: vespertide_core::IndexDef {
                        name: "idx_id".into(),
                        columns: vec!["id".into()],
                        unique: false,
                    },
                },
            ],
        };

        let result = emit_sql(&plan, DatabaseBackend::Postgres, &[]);
        assert!(result.is_ok());
    }

    #[test]
    #[serial]
    fn emit_sql_multiple_queries_per_action() {
        // Test case where a single action generates multiple queries (e.g., SQLite constraint addition)
        // This should trigger the queries.len() > 1 branch (line 89)
        let tmp = tempdir().unwrap();
        let _guard = CwdGuard::new(&tmp.path().to_path_buf());
        let _cfg = write_config();
        write_model("users");

        // Create a migration that adds a NOT NULL column in SQLite, which generates multiple queries
        let plan = MigrationPlan {
            comment: None,
            created_at: None,
            version: 1,
            actions: vec![MigrationAction::AddColumn {
                table: "users".into(),
                column: ColumnDef {
                    name: "nickname".into(),
                    r#type: ColumnType::Simple(SimpleColumnType::Text),
                    nullable: false,
                    default: None,
                    comment: None,
                    primary_key: None,
                    unique: None,
                    index: None,
                    foreign_key: None,
                },
                fill_with: Some("default".into()),
            }],
        };

        let current_schema = vec![TableDef {
            name: "users".into(),
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
            }],
            indexes: vec![],
        }];

        let result = emit_sql(&plan, DatabaseBackend::Sqlite, &current_schema);
        assert!(result.is_ok());
    }
}
