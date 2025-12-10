use anyhow::Result;
use vespertide_planner::plan_next_migration;

use crate::utils::{load_config, load_migrations, load_models};
use vespertide_core::MigrationAction;

pub fn cmd_diff() -> Result<()> {
    let config = load_config()?;
    let current_models = load_models(&config)?;
    let applied_plans = load_migrations(&config)?;

    let plan = plan_next_migration(&current_models, &applied_plans)
        .map_err(|e| anyhow::anyhow!("planning error: {}", e))?;

    if plan.actions.is_empty() {
        println!("No differences found. Schema is up to date.");
    } else {
        println!("Found {} change(s) to apply:", plan.actions.len());
        println!();

        for (i, action) in plan.actions.iter().enumerate() {
            println!("{}. {}", i + 1, format_action(action));
        }
    }
    Ok(())
}

fn format_action(action: &MigrationAction) -> String {
    match action {
        MigrationAction::CreateTable { table, .. } => {
            format!("Create table: {}", table)
        }
        MigrationAction::DeleteTable { table } => {
            format!("Delete table: {}", table)
        }
        MigrationAction::AddColumn { table, column, .. } => {
            format!("Add column: {}.{}", table, column.name)
        }
        MigrationAction::RenameColumn { table, from, to } => {
            format!("Rename column: {}.{} -> {}", table, from, to)
        }
        MigrationAction::DeleteColumn { table, column } => {
            format!("Delete column: {}.{}", table, column)
        }
        MigrationAction::ModifyColumnType { table, column, .. } => {
            format!("Modify column type: {}.{}", table, column)
        }
        MigrationAction::AddIndex { table, index } => {
            format!("Add index: {} on {}", index.name, table)
        }
        MigrationAction::RemoveIndex { table, name } => {
            format!("Remove index: {} from {}", name, table)
        }
        MigrationAction::RenameTable { from, to } => {
            format!("Rename table: {} -> {}", from, to)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;
    use serial_test::serial;
    use std::fs;
    use std::path::PathBuf;
    use tempfile::tempdir;
    use vespertide_config::VespertideConfig;
    use vespertide_core::{ColumnDef, ColumnType, TableDef};

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

    fn write_config() {
        let cfg = VespertideConfig::default();
        let text = serde_json::to_string_pretty(&cfg).unwrap();
        fs::write("vespertide.json", text).unwrap();
    }

    fn write_model(name: &str) {
        let models_dir = PathBuf::from("models");
        fs::create_dir_all(&models_dir).unwrap();
        let table = TableDef {
            name: name.to_string(),
            columns: vec![ColumnDef {
                name: "id".into(),
                r#type: ColumnType::Integer,
                nullable: false,
                default: None,
            }],
            constraints: vec![],
            indexes: vec![],
        };
        let path = models_dir.join(format!("{name}.json"));
        fs::write(path, serde_json::to_string_pretty(&table).unwrap()).unwrap();
    }

    #[rstest]
    #[case(
        MigrationAction::CreateTable { table: "users".into(), columns: vec![], constraints: vec![] },
        "Create table: users"
    )]
    #[case(
        MigrationAction::DeleteTable { table: "users".into() },
        "Delete table: users"
    )]
    #[case(
        MigrationAction::AddColumn {
            table: "users".into(),
            column: ColumnDef {
                name: "name".into(),
                r#type: ColumnType::Text,
                nullable: true,
                default: None,
            },
            fill_with: None,
        },
        "Add column: users.name"
    )]
    #[case(
        MigrationAction::RenameColumn {
            table: "users".into(),
            from: "old".into(),
            to: "new".into(),
        },
        "Rename column: users.old -> new"
    )]
    #[case(
        MigrationAction::DeleteColumn { table: "users".into(), column: "name".into() },
        "Delete column: users.name"
    )]
    #[case(
        MigrationAction::ModifyColumnType {
            table: "users".into(),
            column: "id".into(),
            new_type: ColumnType::Integer,
        },
        "Modify column type: users.id"
    )]
    #[case(
        MigrationAction::AddIndex {
            table: "users".into(),
            index: vespertide_core::IndexDef {
                name: "idx".into(),
                columns: vec!["id".into()],
                unique: false,
            },
        },
        "Add index: idx on users"
    )]
    #[case(
        MigrationAction::RemoveIndex { table: "users".into(), name: "idx".into() },
        "Remove index: idx from users"
    )]
    #[case(
        MigrationAction::RenameTable { from: "users".into(), to: "accounts".into() },
        "Rename table: users -> accounts"
    )]
    #[serial]
    fn format_action_cases(#[case] action: MigrationAction, #[case] expected: &str) {
        assert_eq!(format_action(&action), expected);
    }

    #[rstest]
    #[serial]
    fn cmd_diff_with_model_and_no_migrations() {
        let tmp = tempdir().unwrap();
        let _guard = CwdGuard::new(&tmp.path().to_path_buf());

        write_config();
        write_model("users");
        fs::create_dir_all("migrations").unwrap();

        let result = cmd_diff();
        assert!(result.is_ok());
    }

    #[rstest]
    #[serial]
    fn cmd_diff_when_no_changes() {
        let tmp = tempdir().unwrap();
        let _guard = CwdGuard::new(&tmp.path().to_path_buf());

        write_config();
        // No models, no migrations -> planner should report no actions.
        fs::create_dir_all("models").unwrap();
        fs::create_dir_all("migrations").unwrap();

        let result = cmd_diff();
        assert!(result.is_ok());
    }
}
