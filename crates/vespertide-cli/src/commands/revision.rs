use std::collections::HashMap;
use std::fs;

use anyhow::{Context, Result};
use chrono::Utc;
use colored::Colorize;
use dialoguer::{Input, Select};
use serde_json::Value;
use vespertide_config::FileFormat;
use vespertide_core::{MigrationAction, MigrationPlan};
use vespertide_planner::{find_missing_fill_with, plan_next_migration};

use crate::utils::{
    load_config, load_migrations, load_models, migration_filename_with_format_and_pattern,
};

/// Parse fill_with arguments from CLI.
/// Format: table.column=value
fn parse_fill_with_args(args: &[String]) -> HashMap<(String, String), String> {
    let mut map = HashMap::new();
    for arg in args {
        if let Some((key, value)) = arg.split_once('=')
            && let Some((table, column)) = key.split_once('.')
        {
            map.insert((table.to_string(), column.to_string()), value.to_string());
        }
    }
    map
}

/// Format the type info string for display.
/// Includes column type and default value hint if available.
fn format_type_info(column_type: Option<&String>, default_value: Option<&String>) -> String {
    match (column_type, default_value) {
        (Some(t), Some(d)) => format!(" ({}, default: {})", t, d),
        (Some(t), None) => format!(" ({})", t),
        (None, Some(d)) => format!(" (default: {})", d),
        (None, None) => String::new(),
    }
}

/// Format a single fill_with item for display.
fn format_fill_with_item(table: &str, column: &str, type_info: &str, action_type: &str) -> String {
    format!(
        "  {} {}.{}{}\n    {} {}",
        "•".bright_cyan(),
        table.bright_white(),
        column.bright_green(),
        type_info.bright_black(),
        "Action:".bright_black(),
        action_type.bright_magenta()
    )
}

/// Format the prompt string for interactive input.
fn format_fill_with_prompt(table: &str, column: &str) -> String {
    format!(
        "  Enter fill value for {}.{}",
        table.bright_white(),
        column.bright_green()
    )
}

/// Print the header for fill_with prompts.
fn print_fill_with_header() {
    println!(
        "\n{} {}",
        "⚠".bright_yellow(),
        "The following columns require fill_with values:".bright_yellow()
    );
    println!("{}", "─".repeat(60).bright_black());
}

/// Print the footer for fill_with prompts.
fn print_fill_with_footer() {
    println!("{}", "─".repeat(60).bright_black());
}

/// Print a fill_with item and return the formatted prompt.
fn print_fill_with_item_and_get_prompt(
    table: &str,
    column: &str,
    column_type: Option<&String>,
    default_value: Option<&String>,
    action_type: &str,
) -> String {
    let type_info = format_type_info(column_type, default_value);
    let item_display = format_fill_with_item(table, column, &type_info, action_type);
    println!("{}", item_display);
    format_fill_with_prompt(table, column)
}

/// Wrap a value with single quotes if it contains spaces and isn't already quoted.
fn wrap_if_spaces(value: String) -> String {
    if value.is_empty() {
        return value;
    }
    // Already wrapped with single quotes
    if value.starts_with('\'') && value.ends_with('\'') {
        return value;
    }
    // Contains spaces: wrap with single quotes
    if value.contains(' ') {
        return format!("'{}'", value);
    }
    value
}

/// Prompt the user for a fill_with value using dialoguer.
/// This function wraps terminal I/O and cannot be unit tested without a real terminal.
#[cfg(not(tarpaulin_include))]
fn prompt_fill_with_value(prompt: &str) -> Result<String> {
    let value = Input::new()
        .with_prompt(prompt)
        .allow_empty(true)
        .interact_text()
        .context("failed to read input")?;
    Ok(wrap_if_spaces(value))
}

/// Prompt the user to select an enum value using dialoguer Select.
/// Returns the selected value wrapped in single quotes for SQL.
#[cfg(not(tarpaulin_include))]
fn prompt_enum_value(prompt: &str, enum_values: &[String]) -> Result<String> {
    let selection = Select::new()
        .with_prompt(prompt)
        .items(enum_values)
        .default(0)
        .interact()
        .context("failed to read selection")?;
    // Return the selected value with single quotes for SQL enum literal
    Ok(format!("'{}'", enum_values[selection]))
}

/// Collect fill_with values interactively for missing columns.
/// The `prompt_fn` parameter allows injecting a mock for testing.
/// The `enum_prompt_fn` parameter handles enum type columns with selection UI.
fn collect_fill_with_values<F, E>(
    missing: &[vespertide_planner::FillWithRequired],
    fill_values: &mut HashMap<(String, String), String>,
    prompt_fn: F,
    enum_prompt_fn: E,
) -> Result<()>
where
    F: Fn(&str) -> Result<String>,
    E: Fn(&str, &[String]) -> Result<String>,
{
    print_fill_with_header();

    for item in missing {
        let prompt = print_fill_with_item_and_get_prompt(
            &item.table,
            &item.column,
            item.column_type.as_ref(),
            item.default_value.as_ref(),
            item.action_type,
        );

        let value = if let Some(enum_values) = &item.enum_values {
            // Use selection UI for enum types
            enum_prompt_fn(&prompt, enum_values)?
        } else {
            // Use text input for other types
            prompt_fn(&prompt)?
        };
        fill_values.insert((item.table.clone(), item.column.clone()), value);
    }

    print_fill_with_footer();
    Ok(())
}

/// Apply fill_with values to a migration plan.
fn apply_fill_with_to_plan(
    plan: &mut MigrationPlan,
    fill_values: &HashMap<(String, String), String>,
) {
    for action in &mut plan.actions {
        match action {
            MigrationAction::AddColumn {
                table,
                column,
                fill_with,
            } => {
                if fill_with.is_none()
                    && let Some(value) = fill_values.get(&(table.clone(), column.name.clone()))
                {
                    *fill_with = Some(value.clone());
                }
            }
            MigrationAction::ModifyColumnNullable {
                table,
                column,
                fill_with,
                ..
            } => {
                if fill_with.is_none()
                    && let Some(value) = fill_values.get(&(table.clone(), column.clone()))
                {
                    *fill_with = Some(value.clone());
                }
            }
            _ => {}
        }
    }
}

/// Handle interactive fill_with collection if there are missing values.
/// Returns the updated fill_values map after collecting from user.
fn handle_missing_fill_with<F, E>(
    plan: &mut MigrationPlan,
    fill_values: &mut HashMap<(String, String), String>,
    prompt_fn: F,
    enum_prompt_fn: E,
) -> Result<()>
where
    F: Fn(&str) -> Result<String>,
    E: Fn(&str, &[String]) -> Result<String>,
{
    let missing = find_missing_fill_with(plan);

    if !missing.is_empty() {
        collect_fill_with_values(&missing, fill_values, prompt_fn, enum_prompt_fn)?;

        // Apply the collected fill_with values
        apply_fill_with_to_plan(plan, fill_values);
    }

    Ok(())
}

pub fn cmd_revision(message: String, fill_with_args: Vec<String>) -> Result<()> {
    let config = load_config()?;
    let current_models = load_models(&config)?;
    let applied_plans = load_migrations(&config)?;

    let mut plan = plan_next_migration(&current_models, &applied_plans)
        .map_err(|e| anyhow::anyhow!("planning error: {}", e))?;

    if plan.actions.is_empty() {
        println!(
            "{} {}",
            "No changes detected.".bright_yellow(),
            "Nothing to migrate.".bright_white()
        );
        return Ok(());
    }

    // Parse CLI fill_with arguments
    let mut fill_values = parse_fill_with_args(&fill_with_args);

    // Apply any CLI-provided fill_with values first
    apply_fill_with_to_plan(&mut plan, &fill_values);

    // Handle any missing fill_with values interactively
    handle_missing_fill_with(
        &mut plan,
        &mut fill_values,
        prompt_fill_with_value,
        prompt_enum_value,
    )?;

    plan.comment = Some(message);
    if plan.created_at.is_none() {
        // Record creation time in RFC3339 (UTC).
        plan.created_at = Some(Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true));
    }

    let migrations_dir = config.migrations_dir();
    if !migrations_dir.exists() {
        fs::create_dir_all(migrations_dir).context("create migrations directory")?;
    }

    let format = config.migration_format();
    let filename = migration_filename_with_format_and_pattern(
        plan.version,
        plan.comment.as_deref(),
        format,
        config.migration_filename_pattern(),
    );
    let path = migrations_dir.join(&filename);

    let schema_url = schema_url_for(format);
    match format {
        FileFormat::Json => write_json_with_schema(&path, &plan, &schema_url)?,
        FileFormat::Yaml | FileFormat::Yml => write_yaml(&path, &plan, &schema_url)?,
    }

    println!(
        "{} {}",
        "Created migration:".bright_green().bold(),
        format!("{}", path.display()).bright_white()
    );
    println!(
        "  {} {}",
        "Version:".bright_cyan(),
        plan.version.to_string().bright_magenta().bold()
    );
    println!(
        "  {} {}",
        "Actions:".bright_cyan(),
        plan.actions.len().to_string().bright_yellow()
    );
    if let Some(comment) = &plan.comment {
        println!("  {} {}", "Comment:".bright_cyan(), comment.bright_white());
    }

    Ok(())
}

fn schema_url_for(format: FileFormat) -> String {
    // If not set, default to public raw GitHub schema location.
    // Users can override via VESP_SCHEMA_BASE_URL.
    let base = std::env::var("VESP_SCHEMA_BASE_URL").ok();
    let base = base.as_deref().unwrap_or(
        "https://raw.githubusercontent.com/dev-five-git/vespertide/refs/heads/main/schemas",
    );
    let base = base.trim_end_matches('/');
    match format {
        FileFormat::Json => format!("{}/migration.schema.json", base),
        FileFormat::Yaml | FileFormat::Yml => format!("{}/migration.schema.json", base),
    }
}

fn write_json_with_schema(
    path: &std::path::Path,
    plan: &MigrationPlan,
    schema_url: &str,
) -> Result<()> {
    let mut value = serde_json::to_value(plan).context("serialize migration plan to json")?;
    if let Value::Object(ref mut map) = value {
        map.insert("$schema".to_string(), Value::String(schema_url.to_string()));
    }
    let text = serde_json::to_string_pretty(&value).context("stringify json with schema")?;
    fs::write(path, text).with_context(|| format!("write file: {}", path.display()))?;
    Ok(())
}

fn write_yaml(path: &std::path::Path, plan: &MigrationPlan, schema_url: &str) -> Result<()> {
    let mut value = serde_yaml::to_value(plan).context("serialize migration plan to yaml value")?;
    if let serde_yaml::Value::Mapping(ref mut map) = value {
        map.insert(
            serde_yaml::Value::String("$schema".to_string()),
            serde_yaml::Value::String(schema_url.to_string()),
        );
    }
    let text = serde_yaml::to_string(&value).context("serialize yaml with schema")?;
    fs::write(path, text).with_context(|| format!("write file: {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{env, fs, path::PathBuf};
    use tempfile::tempdir;
    use vespertide_config::{FileFormat, VespertideConfig};
    use vespertide_core::{ColumnDef, ColumnType, SimpleColumnType, TableConstraint, TableDef};

    struct CwdGuard {
        original: PathBuf,
    }

    impl CwdGuard {
        fn new(dir: &PathBuf) -> Self {
            let original = env::current_dir().unwrap();
            env::set_current_dir(dir).unwrap();
            Self { original }
        }
    }

    impl Drop for CwdGuard {
        fn drop(&mut self) {
            let _ = env::set_current_dir(&self.original);
        }
    }

    fn write_config() -> VespertideConfig {
        write_config_with_format(None)
    }

    fn write_config_with_format(fmt: Option<FileFormat>) -> VespertideConfig {
        let mut cfg = VespertideConfig::default();
        if let Some(f) = fmt {
            cfg.migration_format = f;
        }
        let text = serde_json::to_string_pretty(&cfg).unwrap();
        fs::write("vespertide.json", text).unwrap();
        cfg
    }

    fn write_model(name: &str) {
        let models_dir = PathBuf::from("models");
        fs::create_dir_all(&models_dir).unwrap();
        let table = TableDef {
            name: name.to_string(),
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
            }],
        };
        let path = models_dir.join(format!("{name}.json"));
        fs::write(path, serde_json::to_string_pretty(&table).unwrap()).unwrap();
    }

    #[test]
    #[serial_test::serial]
    fn cmd_revision_writes_migration() {
        let tmp = tempdir().unwrap();
        let _guard = CwdGuard::new(&tmp.path().to_path_buf());

        let cfg = write_config();
        write_model("users");
        fs::create_dir_all(cfg.migrations_dir()).unwrap();

        cmd_revision("init".into(), vec![]).unwrap();

        let entries: Vec<_> = fs::read_dir(cfg.migrations_dir()).unwrap().collect();
        assert!(!entries.is_empty());
    }

    #[test]
    #[serial_test::serial]
    fn cmd_revision_no_changes_short_circuits() {
        let tmp = tempdir().unwrap();
        let _guard = CwdGuard::new(&tmp.path().to_path_buf());

        let cfg = write_config();
        // no models, no migrations -> plan with no actions -> early return
        assert!(cmd_revision("noop".into(), vec![]).is_ok());
        // migrations dir should not be created
        assert!(!cfg.migrations_dir().exists());
    }

    #[test]
    #[serial_test::serial]
    fn cmd_revision_writes_yaml_when_configured() {
        let tmp = tempdir().unwrap();
        let _guard = CwdGuard::new(&tmp.path().to_path_buf());

        let cfg = write_config_with_format(Some(FileFormat::Yaml));
        write_model("users");
        // ensure migrations dir absent to exercise create_dir_all branch
        if cfg.migrations_dir().exists() {
            fs::remove_dir_all(cfg.migrations_dir()).unwrap();
        }

        cmd_revision("yaml".into(), vec![]).unwrap();

        let entries: Vec<_> = fs::read_dir(cfg.migrations_dir()).unwrap().collect();
        assert!(!entries.is_empty());
        let has_yaml = entries.iter().any(|e| {
            e.as_ref()
                .unwrap()
                .path()
                .extension()
                .map(|s| s == "yaml")
                .unwrap_or(false)
        });
        assert!(has_yaml);
    }

    #[test]
    fn test_parse_fill_with_args() {
        let args = vec![
            "users.email=default@example.com".to_string(),
            "orders.status=pending".to_string(),
        ];
        let result = parse_fill_with_args(&args);

        assert_eq!(result.len(), 2);
        assert_eq!(
            result.get(&("users".to_string(), "email".to_string())),
            Some(&"default@example.com".to_string())
        );
        assert_eq!(
            result.get(&("orders".to_string(), "status".to_string())),
            Some(&"pending".to_string())
        );
    }

    #[test]
    fn test_parse_fill_with_args_invalid_format() {
        let args = vec![
            "invalid_format".to_string(),
            "no_equals_sign".to_string(),
            "users.email=valid".to_string(),
        ];
        let result = parse_fill_with_args(&args);

        // Only the valid one should be parsed
        assert_eq!(result.len(), 1);
        assert_eq!(
            result.get(&("users".to_string(), "email".to_string())),
            Some(&"valid".to_string())
        );
    }

    #[test]
    fn test_apply_fill_with_to_plan_add_column() {
        use vespertide_core::MigrationPlan;

        let mut plan = MigrationPlan {
            comment: None,
            created_at: None,
            version: 1,
            actions: vec![MigrationAction::AddColumn {
                table: "users".into(),
                column: Box::new(ColumnDef {
                    name: "email".into(),
                    r#type: ColumnType::Simple(SimpleColumnType::Text),
                    nullable: false,
                    default: None,
                    comment: None,
                    primary_key: None,
                    unique: None,
                    index: None,
                    foreign_key: None,
                }),
                fill_with: None,
            }],
        };

        let mut fill_values = HashMap::new();
        fill_values.insert(
            ("users".to_string(), "email".to_string()),
            "'default@example.com'".to_string(),
        );

        apply_fill_with_to_plan(&mut plan, &fill_values);

        match &plan.actions[0] {
            MigrationAction::AddColumn { fill_with, .. } => {
                assert_eq!(fill_with, &Some("'default@example.com'".to_string()));
            }
            _ => panic!("Expected AddColumn action"),
        }
    }

    #[test]
    fn test_apply_fill_with_to_plan_modify_column_nullable() {
        use vespertide_core::MigrationPlan;

        let mut plan = MigrationPlan {
            comment: None,
            created_at: None,
            version: 1,
            actions: vec![MigrationAction::ModifyColumnNullable {
                table: "users".into(),
                column: "status".into(),
                nullable: false,
                fill_with: None,
            }],
        };

        let mut fill_values = HashMap::new();
        fill_values.insert(
            ("users".to_string(), "status".to_string()),
            "'active'".to_string(),
        );

        apply_fill_with_to_plan(&mut plan, &fill_values);

        match &plan.actions[0] {
            MigrationAction::ModifyColumnNullable { fill_with, .. } => {
                assert_eq!(fill_with, &Some("'active'".to_string()));
            }
            _ => panic!("Expected ModifyColumnNullable action"),
        }
    }

    #[test]
    fn test_apply_fill_with_to_plan_skips_existing_fill_with() {
        use vespertide_core::MigrationPlan;

        let mut plan = MigrationPlan {
            comment: None,
            created_at: None,
            version: 1,
            actions: vec![MigrationAction::AddColumn {
                table: "users".into(),
                column: Box::new(ColumnDef {
                    name: "email".into(),
                    r#type: ColumnType::Simple(SimpleColumnType::Text),
                    nullable: false,
                    default: None,
                    comment: None,
                    primary_key: None,
                    unique: None,
                    index: None,
                    foreign_key: None,
                }),
                fill_with: Some("'existing@example.com'".to_string()),
            }],
        };

        let mut fill_values = HashMap::new();
        fill_values.insert(
            ("users".to_string(), "email".to_string()),
            "'new@example.com'".to_string(),
        );

        apply_fill_with_to_plan(&mut plan, &fill_values);

        // Should keep existing value, not replace with new
        match &plan.actions[0] {
            MigrationAction::AddColumn { fill_with, .. } => {
                assert_eq!(fill_with, &Some("'existing@example.com'".to_string()));
            }
            _ => panic!("Expected AddColumn action"),
        }
    }

    #[test]
    fn test_apply_fill_with_to_plan_no_match() {
        use vespertide_core::MigrationPlan;

        let mut plan = MigrationPlan {
            comment: None,
            created_at: None,
            version: 1,
            actions: vec![MigrationAction::AddColumn {
                table: "users".into(),
                column: Box::new(ColumnDef {
                    name: "email".into(),
                    r#type: ColumnType::Simple(SimpleColumnType::Text),
                    nullable: false,
                    default: None,
                    comment: None,
                    primary_key: None,
                    unique: None,
                    index: None,
                    foreign_key: None,
                }),
                fill_with: None,
            }],
        };

        let mut fill_values = HashMap::new();
        fill_values.insert(
            ("orders".to_string(), "status".to_string()),
            "'pending'".to_string(),
        );

        apply_fill_with_to_plan(&mut plan, &fill_values);

        // Should remain None since no match
        match &plan.actions[0] {
            MigrationAction::AddColumn { fill_with, .. } => {
                assert_eq!(fill_with, &None);
            }
            _ => panic!("Expected AddColumn action"),
        }
    }

    #[test]
    fn test_apply_fill_with_to_plan_multiple_actions() {
        use vespertide_core::MigrationPlan;

        let mut plan = MigrationPlan {
            comment: None,
            created_at: None,
            version: 1,
            actions: vec![
                MigrationAction::AddColumn {
                    table: "users".into(),
                    column: Box::new(ColumnDef {
                        name: "email".into(),
                        r#type: ColumnType::Simple(SimpleColumnType::Text),
                        nullable: false,
                        default: None,
                        comment: None,
                        primary_key: None,
                        unique: None,
                        index: None,
                        foreign_key: None,
                    }),
                    fill_with: None,
                },
                MigrationAction::ModifyColumnNullable {
                    table: "orders".into(),
                    column: "status".into(),
                    nullable: false,
                    fill_with: None,
                },
            ],
        };

        let mut fill_values = HashMap::new();
        fill_values.insert(
            ("users".to_string(), "email".to_string()),
            "'user@example.com'".to_string(),
        );
        fill_values.insert(
            ("orders".to_string(), "status".to_string()),
            "'pending'".to_string(),
        );

        apply_fill_with_to_plan(&mut plan, &fill_values);

        match &plan.actions[0] {
            MigrationAction::AddColumn { fill_with, .. } => {
                assert_eq!(fill_with, &Some("'user@example.com'".to_string()));
            }
            _ => panic!("Expected AddColumn action"),
        }

        match &plan.actions[1] {
            MigrationAction::ModifyColumnNullable { fill_with, .. } => {
                assert_eq!(fill_with, &Some("'pending'".to_string()));
            }
            _ => panic!("Expected ModifyColumnNullable action"),
        }
    }

    #[test]
    fn test_apply_fill_with_to_plan_other_actions_ignored() {
        use vespertide_core::MigrationPlan;

        let mut plan = MigrationPlan {
            comment: None,
            created_at: None,
            version: 1,
            actions: vec![MigrationAction::DeleteColumn {
                table: "users".into(),
                column: "old_column".into(),
            }],
        };

        let mut fill_values = HashMap::new();
        fill_values.insert(
            ("users".to_string(), "old_column".to_string()),
            "'value'".to_string(),
        );

        // Should not panic or modify anything
        apply_fill_with_to_plan(&mut plan, &fill_values);

        match &plan.actions[0] {
            MigrationAction::DeleteColumn { table, column } => {
                assert_eq!(table, "users");
                assert_eq!(column, "old_column");
            }
            _ => panic!("Expected DeleteColumn action"),
        }
    }

    #[test]
    fn test_format_type_info_with_type_and_default() {
        let column_type = Some("integer".to_string());
        let default_value = Some("0".to_string());
        let result = format_type_info(column_type.as_ref(), default_value.as_ref());
        assert_eq!(result, " (integer, default: 0)");
    }

    #[test]
    fn test_format_type_info_with_type_only() {
        let column_type = Some("text".to_string());
        let result = format_type_info(column_type.as_ref(), None);
        assert_eq!(result, " (text)");
    }

    #[test]
    fn test_format_type_info_with_default_only() {
        let default_value = Some("0".to_string());
        let result = format_type_info(None, default_value.as_ref());
        assert_eq!(result, " (default: 0)");
    }

    #[test]
    fn test_format_type_info_with_none() {
        let result = format_type_info(None, None);
        assert_eq!(result, "");
    }

    #[test]
    fn test_format_fill_with_item() {
        let result = format_fill_with_item("users", "email", " (Text)", "AddColumn");
        // The result should contain the table, column, type info, and action type
        // Colors make exact matching difficult, but we can check structure
        assert!(result.contains("users"));
        assert!(result.contains("email"));
        assert!(result.contains("(Text)"));
        assert!(result.contains("AddColumn"));
        assert!(result.contains("Action:"));
    }

    #[test]
    fn test_format_fill_with_item_empty_type_info() {
        let result = format_fill_with_item("orders", "status", "", "ModifyColumnNullable");
        assert!(result.contains("orders"));
        assert!(result.contains("status"));
        assert!(result.contains("ModifyColumnNullable"));
    }

    #[test]
    fn test_format_fill_with_prompt() {
        let result = format_fill_with_prompt("users", "email");
        assert!(result.contains("Enter fill value for"));
        assert!(result.contains("users"));
        assert!(result.contains("email"));
    }

    #[test]
    fn test_print_fill_with_item_and_get_prompt() {
        // This function prints to stdout and returns the prompt string
        let prompt = print_fill_with_item_and_get_prompt(
            "users",
            "email",
            Some(&"text".to_string()),
            Some(&"''".to_string()),
            "AddColumn",
        );
        assert!(prompt.contains("Enter fill value for"));
        assert!(prompt.contains("users"));
        assert!(prompt.contains("email"));
    }

    #[test]
    fn test_print_fill_with_item_and_get_prompt_no_type() {
        let prompt = print_fill_with_item_and_get_prompt(
            "orders",
            "status",
            None,
            None,
            "ModifyColumnNullable",
        );
        assert!(prompt.contains("Enter fill value for"));
        assert!(prompt.contains("orders"));
        assert!(prompt.contains("status"));
    }

    #[test]
    fn test_print_fill_with_item_and_get_prompt_with_default() {
        let prompt = print_fill_with_item_and_get_prompt(
            "users",
            "age",
            Some(&"integer".to_string()),
            Some(&"0".to_string()),
            "AddColumn",
        );
        assert!(prompt.contains("Enter fill value for"));
        assert!(prompt.contains("users"));
        assert!(prompt.contains("age"));
    }

    #[test]
    fn test_print_fill_with_header() {
        // Just verify it doesn't panic - output goes to stdout
        print_fill_with_header();
    }

    #[test]
    fn test_print_fill_with_footer() {
        // Just verify it doesn't panic - output goes to stdout
        print_fill_with_footer();
    }

    // Mock enum prompt function for tests - returns first enum value quoted
    fn mock_enum_prompt(_prompt: &str, values: &[String]) -> Result<String> {
        Ok(format!("'{}'", values[0]))
    }

    #[test]
    fn test_collect_fill_with_values_single_item() {
        use vespertide_planner::FillWithRequired;

        let missing = vec![FillWithRequired {
            action_index: 0,
            table: "users".to_string(),
            column: "email".to_string(),
            action_type: "AddColumn",
            column_type: Some("text".to_string()),
            default_value: Some("''".to_string()),
            enum_values: None,
        }];

        let mut fill_values = HashMap::new();

        // Mock prompt function that returns a fixed value
        let mock_prompt =
            |_prompt: &str| -> Result<String> { Ok("'test@example.com'".to_string()) };

        let result =
            collect_fill_with_values(&missing, &mut fill_values, mock_prompt, mock_enum_prompt);
        assert!(result.is_ok());
        assert_eq!(fill_values.len(), 1);
        assert_eq!(
            fill_values.get(&("users".to_string(), "email".to_string())),
            Some(&"'test@example.com'".to_string())
        );
    }

    #[test]
    fn test_collect_fill_with_values_multiple_items() {
        use vespertide_planner::FillWithRequired;

        let missing = vec![
            FillWithRequired {
                action_index: 0,
                table: "users".to_string(),
                column: "email".to_string(),
                action_type: "AddColumn",
                column_type: Some("text".to_string()),
                default_value: Some("''".to_string()),
                enum_values: None,
            },
            FillWithRequired {
                action_index: 1,
                table: "orders".to_string(),
                column: "status".to_string(),
                action_type: "ModifyColumnNullable",
                column_type: None,
                default_value: None,
                enum_values: None,
            },
        ];

        let mut fill_values = HashMap::new();

        // Mock prompt function that returns different values based on call count
        let call_count = std::cell::RefCell::new(0);
        let mock_prompt = |_prompt: &str| -> Result<String> {
            let mut count = call_count.borrow_mut();
            *count += 1;
            match *count {
                1 => Ok("'user@example.com'".to_string()),
                2 => Ok("'pending'".to_string()),
                _ => Ok("'default'".to_string()),
            }
        };

        let result =
            collect_fill_with_values(&missing, &mut fill_values, mock_prompt, mock_enum_prompt);
        assert!(result.is_ok());
        assert_eq!(fill_values.len(), 2);
        assert_eq!(
            fill_values.get(&("users".to_string(), "email".to_string())),
            Some(&"'user@example.com'".to_string())
        );
        assert_eq!(
            fill_values.get(&("orders".to_string(), "status".to_string())),
            Some(&"'pending'".to_string())
        );
    }

    #[test]
    fn test_collect_fill_with_values_empty() {
        let missing: Vec<vespertide_planner::FillWithRequired> = vec![];
        let mut fill_values = HashMap::new();

        // This function should handle empty list gracefully (though it won't be called in practice)
        // But we can't test the header/footer without items since the function still prints them
        // So we test with a mock that would fail if called
        let mock_prompt = |_prompt: &str| -> Result<String> {
            panic!("Should not be called for empty list");
        };

        // Note: The function still prints header/footer even for empty list
        // This is a design choice - in practice, cmd_revision won't call this with empty list
        let result =
            collect_fill_with_values(&missing, &mut fill_values, mock_prompt, mock_enum_prompt);
        assert!(result.is_ok());
        assert!(fill_values.is_empty());
    }

    #[test]
    fn test_collect_fill_with_values_prompt_error() {
        use vespertide_planner::FillWithRequired;

        let missing = vec![FillWithRequired {
            action_index: 0,
            table: "users".to_string(),
            column: "email".to_string(),
            action_type: "AddColumn",
            column_type: Some("text".to_string()),
            default_value: Some("''".to_string()),
            enum_values: None,
        }];

        let mut fill_values = HashMap::new();

        // Mock prompt function that returns an error
        let mock_prompt =
            |_prompt: &str| -> Result<String> { Err(anyhow::anyhow!("input cancelled")) };

        let result =
            collect_fill_with_values(&missing, &mut fill_values, mock_prompt, mock_enum_prompt);
        assert!(result.is_err());
        assert!(fill_values.is_empty());
    }

    #[test]
    fn test_prompt_fill_with_value_function_exists() {
        // This test verifies that prompt_fill_with_value has the correct signature.
        // We cannot actually call it in tests because dialoguer::Input blocks waiting for terminal input.
        // The function is excluded from coverage with #[cfg_attr(coverage_nightly, coverage(off))].
        let _: fn(&str) -> Result<String> = prompt_fill_with_value;
    }

    #[test]
    fn test_handle_missing_fill_with_collects_and_applies() {
        use vespertide_core::MigrationPlan;

        let mut plan = MigrationPlan {
            comment: None,
            created_at: None,
            version: 1,
            actions: vec![MigrationAction::AddColumn {
                table: "users".into(),
                column: Box::new(ColumnDef {
                    name: "email".into(),
                    r#type: ColumnType::Simple(SimpleColumnType::Text),
                    nullable: false,
                    default: None,
                    comment: None,
                    primary_key: None,
                    unique: None,
                    index: None,
                    foreign_key: None,
                }),
                fill_with: None,
            }],
        };

        let mut fill_values = HashMap::new();

        // Mock prompt function
        let mock_prompt =
            |_prompt: &str| -> Result<String> { Ok("'test@example.com'".to_string()) };

        let result =
            handle_missing_fill_with(&mut plan, &mut fill_values, mock_prompt, mock_enum_prompt);
        assert!(result.is_ok());

        // Verify fill_with was applied to the plan
        match &plan.actions[0] {
            MigrationAction::AddColumn { fill_with, .. } => {
                assert_eq!(fill_with, &Some("'test@example.com'".to_string()));
            }
            _ => panic!("Expected AddColumn action"),
        }

        // Verify fill_values map was updated
        assert_eq!(
            fill_values.get(&("users".to_string(), "email".to_string())),
            Some(&"'test@example.com'".to_string())
        );
    }

    #[test]
    fn test_handle_missing_fill_with_no_missing() {
        use vespertide_core::MigrationPlan;

        // Plan with no missing fill_with values (nullable column)
        let mut plan = MigrationPlan {
            comment: None,
            created_at: None,
            version: 1,
            actions: vec![MigrationAction::AddColumn {
                table: "users".into(),
                column: Box::new(ColumnDef {
                    name: "email".into(),
                    r#type: ColumnType::Simple(SimpleColumnType::Text),
                    nullable: true, // nullable, so no fill_with required
                    default: None,
                    comment: None,
                    primary_key: None,
                    unique: None,
                    index: None,
                    foreign_key: None,
                }),
                fill_with: None,
            }],
        };

        let mut fill_values = HashMap::new();

        // Mock prompt that should never be called
        let mock_prompt = |_prompt: &str| -> Result<String> {
            panic!("Should not be called when no missing fill_with values");
        };

        let result =
            handle_missing_fill_with(&mut plan, &mut fill_values, mock_prompt, mock_enum_prompt);
        assert!(result.is_ok());
        assert!(fill_values.is_empty());
    }

    #[test]
    fn test_handle_missing_fill_with_prompt_error() {
        use vespertide_core::MigrationPlan;

        let mut plan = MigrationPlan {
            comment: None,
            created_at: None,
            version: 1,
            actions: vec![MigrationAction::AddColumn {
                table: "users".into(),
                column: Box::new(ColumnDef {
                    name: "email".into(),
                    r#type: ColumnType::Simple(SimpleColumnType::Text),
                    nullable: false,
                    default: None,
                    comment: None,
                    primary_key: None,
                    unique: None,
                    index: None,
                    foreign_key: None,
                }),
                fill_with: None,
            }],
        };

        let mut fill_values = HashMap::new();

        // Mock prompt that returns an error
        let mock_prompt =
            |_prompt: &str| -> Result<String> { Err(anyhow::anyhow!("user cancelled")) };

        let result =
            handle_missing_fill_with(&mut plan, &mut fill_values, mock_prompt, mock_enum_prompt);
        assert!(result.is_err());

        // Plan should not be modified on error
        match &plan.actions[0] {
            MigrationAction::AddColumn { fill_with, .. } => {
                assert_eq!(fill_with, &None);
            }
            _ => panic!("Expected AddColumn action"),
        }
    }

    #[test]
    fn test_handle_missing_fill_with_multiple_columns() {
        use vespertide_core::MigrationPlan;

        let mut plan = MigrationPlan {
            comment: None,
            created_at: None,
            version: 1,
            actions: vec![
                MigrationAction::AddColumn {
                    table: "users".into(),
                    column: Box::new(ColumnDef {
                        name: "email".into(),
                        r#type: ColumnType::Simple(SimpleColumnType::Text),
                        nullable: false,
                        default: None,
                        comment: None,
                        primary_key: None,
                        unique: None,
                        index: None,
                        foreign_key: None,
                    }),
                    fill_with: None,
                },
                MigrationAction::ModifyColumnNullable {
                    table: "orders".into(),
                    column: "status".into(),
                    nullable: false,
                    fill_with: None,
                },
            ],
        };

        let mut fill_values = HashMap::new();

        // Mock prompt that returns different values based on call count
        let call_count = std::cell::RefCell::new(0);
        let mock_prompt = |_prompt: &str| -> Result<String> {
            let mut count = call_count.borrow_mut();
            *count += 1;
            match *count {
                1 => Ok("'user@example.com'".to_string()),
                2 => Ok("'pending'".to_string()),
                _ => Ok("'default'".to_string()),
            }
        };

        let result =
            handle_missing_fill_with(&mut plan, &mut fill_values, mock_prompt, mock_enum_prompt);
        assert!(result.is_ok());

        // Verify both actions were updated
        match &plan.actions[0] {
            MigrationAction::AddColumn { fill_with, .. } => {
                assert_eq!(fill_with, &Some("'user@example.com'".to_string()));
            }
            _ => panic!("Expected AddColumn action"),
        }

        match &plan.actions[1] {
            MigrationAction::ModifyColumnNullable { fill_with, .. } => {
                assert_eq!(fill_with, &Some("'pending'".to_string()));
            }
            _ => panic!("Expected ModifyColumnNullable action"),
        }
    }

    #[test]
    fn test_collect_fill_with_values_enum_column() {
        use vespertide_planner::FillWithRequired;

        let missing = vec![FillWithRequired {
            action_index: 0,
            table: "orders".to_string(),
            column: "status".to_string(),
            action_type: "AddColumn",
            column_type: Some("enum<order_status>".to_string()),
            default_value: None,
            enum_values: Some(vec![
                "pending".to_string(),
                "confirmed".to_string(),
                "shipped".to_string(),
            ]),
        }];

        let mut fill_values = HashMap::new();

        // Mock prompt function that should NOT be called for enum columns
        let mock_prompt = |_prompt: &str| -> Result<String> {
            panic!("Should not be called for enum columns");
        };

        // Mock enum prompt that selects the second value
        let mock_enum = |_prompt: &str, values: &[String]| -> Result<String> {
            // Select "confirmed" (index 1)
            Ok(format!("'{}'", values[1]))
        };

        let result = collect_fill_with_values(&missing, &mut fill_values, mock_prompt, mock_enum);
        assert!(result.is_ok());
        assert_eq!(fill_values.len(), 1);
        assert_eq!(
            fill_values.get(&("orders".to_string(), "status".to_string())),
            Some(&"'confirmed'".to_string())
        );
    }

    #[test]
    fn test_wrap_if_spaces_empty() {
        assert_eq!(wrap_if_spaces("".to_string()), "");
    }

    #[test]
    fn test_wrap_if_spaces_no_spaces() {
        assert_eq!(wrap_if_spaces("value".to_string()), "value");
    }

    #[test]
    fn test_wrap_if_spaces_with_spaces() {
        assert_eq!(wrap_if_spaces("my value".to_string()), "'my value'");
    }

    #[test]
    fn test_wrap_if_spaces_already_quoted() {
        assert_eq!(
            wrap_if_spaces("'already quoted'".to_string()),
            "'already quoted'"
        );
    }

    #[test]
    fn test_wrap_if_spaces_multiple_spaces() {
        assert_eq!(wrap_if_spaces("a b c".to_string()), "'a b c'");
    }
}
