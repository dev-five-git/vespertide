use anyhow::Result;
use colored::Colorize;
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
        println!(
            "{} {}",
            "No differences found.".bright_green(),
            "Schema is up to date.".bright_white()
        );
    } else {
        println!(
            "{} {} {}",
            "Found".bright_cyan(),
            plan.actions.len().to_string().bright_yellow().bold(),
            "change(s) to apply:".bright_cyan()
        );
        println!();

        for (i, action) in plan.actions.iter().enumerate() {
            println!(
                "{}. {}",
                (i + 1).to_string().bright_magenta().bold(),
                format_action(action)
            );
        }
    }
    Ok(())
}

fn format_action(action: &MigrationAction) -> String {
    match action {
        MigrationAction::CreateTable { table, .. } => {
            format!(
                "{} {}",
                "Create table:".bright_green(),
                table.bright_cyan().bold()
            )
        }
        MigrationAction::DeleteTable { table } => {
            format!(
                "{} {}",
                "Delete table:".bright_red(),
                table.bright_cyan().bold()
            )
        }
        MigrationAction::AddColumn { table, column, .. } => {
            format!(
                "{} {}.{}",
                "Add column:".bright_green(),
                table.bright_cyan(),
                column.name.bright_cyan().bold()
            )
        }
        MigrationAction::RenameColumn { table, from, to } => {
            format!(
                "{} {}.{} {} {}",
                "Rename column:".bright_yellow(),
                table.bright_cyan(),
                from.bright_white(),
                "->".bright_white(),
                to.bright_cyan().bold()
            )
        }
        MigrationAction::DeleteColumn { table, column } => {
            format!(
                "{} {}.{}",
                "Delete column:".bright_red(),
                table.bright_cyan(),
                column.bright_cyan().bold()
            )
        }
        MigrationAction::ModifyColumnType { table, column, .. } => {
            format!(
                "{} {}.{}",
                "Modify column type:".bright_yellow(),
                table.bright_cyan(),
                column.bright_cyan().bold()
            )
        }
        MigrationAction::AddIndex { table, index } => {
            format!(
                "{} {} {} {}",
                "Add index:".bright_green(),
                index.name.bright_cyan().bold(),
                "on".bright_white(),
                table.bright_cyan()
            )
        }
        MigrationAction::RemoveIndex { table, name } => {
            format!(
                "{} {} {} {}",
                "Remove index:".bright_red(),
                name.bright_cyan().bold(),
                "from".bright_white(),
                table.bright_cyan()
            )
        }
        MigrationAction::RenameTable { from, to } => {
            format!(
                "{} {} {} {}",
                "Rename table:".bright_yellow(),
                from.bright_cyan(),
                "->".bright_white(),
                to.bright_cyan().bold()
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use colored::Colorize;
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
        format!("{} {}", "Create table:".bright_green(), "users".bright_cyan().bold())
    )]
    #[case(
        MigrationAction::DeleteTable { table: "users".into() },
        format!("{} {}", "Delete table:".bright_red(), "users".bright_cyan().bold())
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
        format!("{} {}.{}", "Add column:".bright_green(), "users".bright_cyan(), "name".bright_cyan().bold())
    )]
    #[case(
        MigrationAction::RenameColumn {
            table: "users".into(),
            from: "old".into(),
            to: "new".into(),
        },
        format!("{} {}.{} {} {}", "Rename column:".bright_yellow(), "users".bright_cyan(), "old".bright_white(), "->".bright_white(), "new".bright_cyan().bold())
    )]
    #[case(
        MigrationAction::DeleteColumn { table: "users".into(), column: "name".into() },
        format!("{} {}.{}", "Delete column:".bright_red(), "users".bright_cyan(), "name".bright_cyan().bold())
    )]
    #[case(
        MigrationAction::ModifyColumnType {
            table: "users".into(),
            column: "id".into(),
            new_type: ColumnType::Integer,
        },
        format!("{} {}.{}", "Modify column type:".bright_yellow(), "users".bright_cyan(), "id".bright_cyan().bold())
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
        format!("{} {} {} {}", "Add index:".bright_green(), "idx".bright_cyan().bold(), "on".bright_white(), "users".bright_cyan())
    )]
    #[case(
        MigrationAction::RemoveIndex { table: "users".into(), name: "idx".into() },
        format!("{} {} {} {}", "Remove index:".bright_red(), "idx".bright_cyan().bold(), "from".bright_white(), "users".bright_cyan())
    )]
    #[case(
        MigrationAction::RenameTable { from: "users".into(), to: "accounts".into() },
        format!("{} {} {} {}", "Rename table:".bright_yellow(), "users".bright_cyan(), "->".bright_white(), "accounts".bright_cyan().bold())
    )]
    #[serial]
    fn format_action_cases(#[case] action: MigrationAction, #[case] expected: String) {
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
