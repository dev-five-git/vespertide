use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use vespertide_config::{FileFormat, VespertideConfig};
use vespertide_core::{MigrationPlan, TableDef};
use vespertide_planner::validate_schema;

/// Load vespertide.json config from current directory.
pub fn load_config() -> Result<VespertideConfig> {
    let path = PathBuf::from("vespertide.json");
    if !path.exists() {
        anyhow::bail!("vespertide.json not found. Run 'vespertide init' first.");
    }

    let content = fs::read_to_string(&path).context("read vespertide.json")?;
    let config: VespertideConfig =
        serde_json::from_str(&content).context("parse vespertide.json")?;
    Ok(config)
}

/// Load all model definitions from the models directory.
pub fn load_models(config: &VespertideConfig) -> Result<Vec<TableDef>> {
    let models_dir = config.models_dir();
    if !models_dir.exists() {
        return Ok(Vec::new());
    }

    let mut tables = Vec::new();
    let entries = fs::read_dir(models_dir).context("read models directory")?;

    for entry in entries {
        let entry = entry.context("read directory entry")?;
        let path = entry.path();
        if path.is_file() {
            let ext = path.extension().and_then(|s| s.to_str());
            if ext == Some("json") || ext == Some("yaml") || ext == Some("yml") {
                let content = fs::read_to_string(&path)
                    .with_context(|| format!("read model file: {}", path.display()))?;

                let table: TableDef = if ext == Some("json") {
                    serde_json::from_str(&content)
                        .with_context(|| format!("parse JSON model: {}", path.display()))?
                } else {
                    serde_yaml::from_str(&content)
                        .with_context(|| format!("parse YAML model: {}", path.display()))?
                };

                tables.push(table);
            }
        }
    }

    // Validate schema integrity before returning
    if !tables.is_empty() {
        validate_schema(&tables).map_err(|e| anyhow::anyhow!("schema validation failed: {}", e))?;
    }

    Ok(tables)
}

/// Load all migration plans from the migrations directory, sorted by version.
pub fn load_migrations(config: &VespertideConfig) -> Result<Vec<MigrationPlan>> {
    let migrations_dir = config.migrations_dir();
    if !migrations_dir.exists() {
        return Ok(Vec::new());
    }

    let mut plans = Vec::new();
    let entries = fs::read_dir(migrations_dir).context("read migrations directory")?;

    for entry in entries {
        let entry = entry.context("read directory entry")?;
        let path = entry.path();
        if path.is_file() {
            let ext = path.extension().and_then(|s| s.to_str());
            if ext == Some("json") || ext == Some("yaml") || ext == Some("yml") {
                let content = fs::read_to_string(&path)
                    .with_context(|| format!("read migration file: {}", path.display()))?;

                let plan: MigrationPlan = if ext == Some("json") {
                    serde_json::from_str(&content)
                        .with_context(|| format!("parse migration: {}", path.display()))?
                } else {
                    serde_yaml::from_str(&content)
                        .with_context(|| format!("parse migration: {}", path.display()))?
                };

                plans.push(plan);
            }
        }
    }

    // Sort by version number
    plans.sort_by_key(|p| p.version);
    Ok(plans)
}

/// Generate a migration filename from version and optional comment with format and pattern.
pub fn migration_filename_with_format_and_pattern(
    version: u32,
    comment: Option<&str>,
    format: FileFormat,
    pattern: &str,
) -> String {
    let sanitized = sanitize_comment(comment);
    let name = render_migration_name(pattern, version, &sanitized);

    let ext = match format {
        FileFormat::Json => "json",
        FileFormat::Yaml => "yaml",
        FileFormat::Yml => "yml",
    };

    format!("{name}.{ext}")
}

fn sanitize_comment(comment: Option<&str>) -> String {
    comment
        .map(|c| {
            c.to_lowercase()
                .chars()
                .map(|ch| {
                    if ch.is_alphanumeric() || ch == ' ' {
                        ch
                    } else {
                        '_'
                    }
                })
                .collect::<String>()
                .split_whitespace()
                .collect::<Vec<_>>()
                .join("_")
        })
        .unwrap_or_default()
}

fn render_migration_name(pattern: &str, version: u32, sanitized_comment: &str) -> String {
    let default_version = format!("{:04}", version);
    let chars: Vec<char> = pattern.chars().collect();
    let mut i = 0;
    let mut out = String::new();

    while i < chars.len() {
        if chars[i] == '%' {
            // Handle %v, %m, and %0Nv (width-padded).
            if i + 1 < chars.len() {
                let next = chars[i + 1];
                if next == 'v' {
                    out.push_str(&default_version);
                    i += 2;
                    continue;
                } else if next == 'm' {
                    out.push_str(sanitized_comment);
                    i += 2;
                    continue;
                } else if next == '0' {
                    let mut j = i + 2;
                    let mut width = String::new();
                    while j < chars.len() && chars[j].is_ascii_digit() {
                        width.push(chars[j]);
                        j += 1;
                    }
                    if j < chars.len() && chars[j] == 'v' {
                        let w: usize = width.parse().unwrap_or(0);
                        if w == 0 {
                            out.push_str(&default_version);
                        } else {
                            out.push_str(&format!("{:0width$}", version, width = w));
                        }
                        i = j + 1;
                        continue;
                    }
                }
            }
        }
        out.push(chars[i]);
        i += 1;
    }

    let mut name = out;

    // Trim redundant trailing separators when comment is empty.
    while name.ends_with('_') || name.ends_with('-') || name.ends_with('.') {
        name.pop();
    }

    if name.is_empty() {
        default_version
    } else {
        name
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use std::fs;
    use tempfile::tempdir;
    use vespertide_core::{ColumnDef, ColumnType, TableConstraint};

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

    #[test]
    #[serial]
    fn load_config_missing_file_errors() {
        let tmp = tempdir().unwrap();
        let _guard = CwdGuard::new(&tmp.path().to_path_buf());
        let err = load_config().unwrap_err();
        assert!(err.to_string().contains("vespertide.json not found"));
    }

    #[test]
    #[serial]
    fn load_models_reads_yaml_and_validates() {
        let tmp = tempdir().unwrap();
        let _guard = CwdGuard::new(&tmp.path().to_path_buf());
        write_config();

        fs::create_dir_all("models").unwrap();
        let table = TableDef {
            name: "users".into(),
            columns: vec![ColumnDef {
                name: "id".into(),
                r#type: ColumnType::Integer,
                nullable: false,
                default: None,
            }],
            constraints: vec![],
            indexes: vec![],
        };
        fs::write("models/users.yaml", serde_yaml::to_string(&table).unwrap()).unwrap();

        let models = load_models(&VespertideConfig::default()).unwrap();
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].name, "users");
    }

    #[test]
    #[serial]
    fn load_migrations_reads_yaml_and_sorts() {
        let tmp = tempdir().unwrap();
        let _guard = CwdGuard::new(&tmp.path().to_path_buf());
        write_config();

        fs::create_dir_all("migrations").unwrap();
        let plan1 = MigrationPlan {
            comment: Some("first".into()),
            created_at: None,
            version: 2,
            actions: vec![],
        };
        let plan0 = MigrationPlan {
            comment: Some("zero".into()),
            created_at: None,
            version: 1,
            actions: vec![],
        };
        fs::write(
            "migrations/0002_first.yaml",
            serde_yaml::to_string(&plan1).unwrap(),
        )
        .unwrap();
        fs::write(
            "migrations/0001_zero.yaml",
            serde_yaml::to_string(&plan0).unwrap(),
        )
        .unwrap();

        let plans = load_migrations(&VespertideConfig::default()).unwrap();
        assert_eq!(plans.len(), 2);
        assert_eq!(plans[0].version, 1);
        assert_eq!(plans[1].version, 2);
    }

    #[test]
    fn migration_filename_respects_format_and_sanitizes_comment() {
        let name = migration_filename_with_format_and_pattern(
            5,
            Some("Hello! World"),
            FileFormat::Yml,
            "%04v_%m",
        );
        assert_eq!(name, "0005_hello__world.yml");
    }

    #[test]
    fn migration_filename_handles_zero_width_and_trim() {
        // width 0 falls back to default version and trailing separators are trimmed
        let name = migration_filename_with_format_and_pattern(3, None, FileFormat::Json, "%0v__");
        assert_eq!(name, "0003.json");
    }

    #[test]
    fn migration_filename_replaces_version_directly() {
        let name = migration_filename_with_format_and_pattern(12, None, FileFormat::Json, "%v");
        assert_eq!(name, "0012.json");
    }

    #[test]
    fn migration_filename_uses_default_when_comment_only_and_empty() {
        let name = migration_filename_with_format_and_pattern(7, None, FileFormat::Json, "%m");
        assert_eq!(name, "0007.json");
    }
}
