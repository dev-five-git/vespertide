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
                let content = fs::read_to_string(&path).with_context(|| {
                    format!("read model file: {}", path.display())
                })?;

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
        validate_schema(&tables)
            .map_err(|e| anyhow::anyhow!("schema validation failed: {}", e))?;
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
                let content = fs::read_to_string(&path).with_context(|| {
                    format!("read migration file: {}", path.display())
                })?;

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

#[allow(dead_code)]
/// Generate a migration filename from version and optional comment using defaults.
pub fn migration_filename(version: u32, comment: Option<&str>) -> String {
    migration_filename_with_format_and_pattern(
        version,
        comment,
        FileFormat::Json,
        vespertide_config::default_migration_filename_pattern().as_str(),
    )
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
                .map(|ch| if ch.is_alphanumeric() || ch == ' ' { ch } else { '_' })
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

