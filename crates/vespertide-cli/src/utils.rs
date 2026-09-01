use std::fmt::Write as _;
use std::path::Path;

use anyhow::{Context, Result};
use serde::Serialize;
use tokio::fs;
use vespertide_config::FileFormat;

// Re-export loader functions for convenience
pub use vespertide_loader::{load_config, load_migrations, load_models};

/// Serialize `value` as pretty JSON, inject a top-level `$schema` key, and
/// write the result to `path`.
pub(crate) async fn write_json_with_schema<T: Serialize>(
    path: &Path,
    value: &T,
    schema_url: &str,
) -> Result<()> {
    let mut value = serde_json::to_value(value).context("serialize value to json")?;
    if let serde_json::Value::Object(ref mut map) = value {
        map.insert(
            "$schema".to_string(),
            serde_json::Value::String(schema_url.to_string()),
        );
    }
    let text = serde_json::to_string_pretty(&value).context("stringify json with schema")?;
    fs::write(path, text)
        .await
        .with_context(|| format!("write file: {}", path.display()))?;
    Ok(())
}

/// Serialize `value` as YAML, inject a top-level `$schema` key, and write the
/// result to `path`.
pub(crate) async fn write_yaml_with_schema<T: Serialize>(
    path: &Path,
    value: &T,
    schema_url: &str,
) -> Result<()> {
    let mut value = serde_yaml::to_value(value).context("serialize value to yaml value")?;
    if let serde_yaml::Value::Mapping(ref mut map) = value {
        map.insert(
            serde_yaml::Value::String("$schema".to_string()),
            serde_yaml::Value::String(schema_url.to_string()),
        );
    }
    let text = serde_yaml::to_string(&value).context("serialize yaml with schema")?;
    fs::write(path, text)
        .await
        .with_context(|| format!("write file: {}", path.display()))?;
    Ok(())
}

pub(crate) fn schema_url(schema_filename: &str) -> String {
    // If not set, default to public raw GitHub schema location.
    // Users can override via VESP_SCHEMA_BASE_URL.
    let base = std::env::var("VESP_SCHEMA_BASE_URL").ok();
    let base = base.as_deref().unwrap_or(
        "https://raw.githubusercontent.com/dev-five-git/vespertide/refs/heads/main/schemas",
    );
    let base = base.trim_end_matches('/');
    format!("{base}/{schema_filename}")
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

    format!("{name}.vespertide.{ext}", ext = format.extension())
}

/// Lowercase `comment`, replace non-alphanumeric characters with `_`, and
/// collapse whitespace runs to a single `_` — in one pass, no intermediate
/// allocations.
fn sanitize_comment(comment: Option<&str>) -> String {
    let Some(comment) = comment else {
        return String::new();
    };

    let mut out = String::with_capacity(comment.len());
    let mut pending_separator = false;
    for ch in comment.chars() {
        if ch.is_whitespace() {
            // Leading whitespace is dropped; inner runs flush as one `_`.
            pending_separator = !out.is_empty();
            continue;
        }
        if pending_separator {
            out.push('_');
            pending_separator = false;
        }
        if ch.is_alphanumeric() {
            out.extend(ch.to_lowercase());
        } else {
            out.push('_');
        }
    }
    out
}

fn render_migration_name(pattern: &str, version: u32, sanitized_comment: &str) -> String {
    let default_version = format!("{version:04}");
    // Byte-wise scan: every placeholder byte (`%`, `v`, `m`, digits) is ASCII,
    // and UTF-8 continuation bytes can never equal an ASCII byte, so scanning
    // bytes is correct for multi-byte patterns. Non-placeholder spans are
    // copied verbatim without the per-char `Vec<char>` staging buffer.
    let bytes = pattern.as_bytes();
    let mut out = String::with_capacity(pattern.len() + sanitized_comment.len());
    let mut verbatim_start = 0;
    let mut i = 0;

    while i < bytes.len() {
        if bytes[i] != b'%' || i + 1 >= bytes.len() {
            i += 1;
            continue;
        }
        // Handle %v, %m, and %0Nv (width-padded).
        match bytes[i + 1] {
            b'v' => {
                out.push_str(&pattern[verbatim_start..i]);
                out.push_str(&default_version);
                i += 2;
                verbatim_start = i;
            }
            b'm' => {
                out.push_str(&pattern[verbatim_start..i]);
                out.push_str(sanitized_comment);
                i += 2;
                verbatim_start = i;
            }
            b'0' => {
                let mut j = i + 2;
                while j < bytes.len() && bytes[j].is_ascii_digit() {
                    j += 1;
                }
                if j < bytes.len() && bytes[j] == b'v' {
                    out.push_str(&pattern[verbatim_start..i]);
                    let width: usize = pattern[i + 2..j].parse().unwrap_or(0);
                    if width == 0 {
                        out.push_str(&default_version);
                    } else {
                        let _ = write!(out, "{version:0width$}");
                    }
                    i = j + 1;
                    verbatim_start = i;
                } else {
                    i += 1;
                }
            }
            _ => i += 1,
        }
    }
    out.push_str(&pattern[verbatim_start..]);

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
    use crate::test_support::CwdGuard;
    use rstest::rstest;
    use serial_test::serial;
    use std::fs;
    use tempfile::tempdir;
    use vespertide_config::VespertideConfig;
    use vespertide_core::{
        ColumnDef, ColumnType, MigrationPlan, SimpleColumnType, TableConstraint, TableDef,
        schema::foreign_key::ForeignKeySyntax,
    };

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
        };
        fs::write("models/users.yaml", serde_yaml::to_string(&table).unwrap()).unwrap();

        let models = load_models(&VespertideConfig::default()).unwrap();
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].name, "users");
    }

    #[test]
    #[serial]
    fn load_models_recursive_processes_subdirectories() {
        let tmp = tempdir().unwrap();
        let _guard = CwdGuard::new(&tmp.path().to_path_buf());
        write_config();

        fs::create_dir_all("models/subdir").unwrap();

        // Create model in subdirectory
        let table = TableDef {
            name: "subtable".into(),
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
        };
        let content = serde_json::to_string_pretty(&table).unwrap();
        fs::write("models/subdir/subtable.json", content).unwrap();

        let models = load_models(&VespertideConfig::default()).unwrap();
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].name, "subtable");
    }

    #[test]
    #[serial]
    fn load_migrations_reads_yaml_and_sorts() {
        let tmp = tempdir().unwrap();
        let _guard = CwdGuard::new(&tmp.path().to_path_buf());
        write_config();

        fs::create_dir_all("migrations").unwrap();
        let plan1 = MigrationPlan {
            id: String::new(),
            comment: Some("first".into()),
            created_at: None,
            version: 2,
            actions: vec![],
        };
        let plan0 = MigrationPlan {
            id: String::new(),
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

    #[rstest]
    #[case(
        5,
        Some("Hello! World"),
        FileFormat::Yml,
        "%04v_%m",
        "0005_hello__world.vespertide.yml"
    )]
    #[case(3, None, FileFormat::Json, "%0v__", "0003.vespertide.json")] // width 0 falls back to default version and trailing separators are trimmed
    #[case(12, None, FileFormat::Json, "%v", "0012.vespertide.json")]
    #[case(7, None, FileFormat::Json, "%m", "0007.vespertide.json")] // uses default when comment only and empty
    #[case(
        4,
        Some("Añadir  Tabla"),
        FileFormat::Yaml,
        "한_%v_%m",
        "한_0004_añadir_tabla.vespertide.yaml"
    )]
    // UTF-8 pattern + comment lock the byte-scanner and one-pass sanitize
    // The three cases below close the remaining arms of `render_migration_name`'s
    // placeholder match. Each arm is a distinct `i` advance, and LLVM attributes
    // the region to a different line depending on how the crate is built (the
    // workspace-wide tarpaulin run and a single-package run disagree), so every
    // arm needs its own case rather than relying on one representative pattern.
    #[case(9, None, FileFormat::Json, "%z_%v", "%z_0009.vespertide.json")] // `_ => i += 1`: unknown placeholder is copied verbatim, scan resumes
    #[case(
        9,
        Some("Fix Bug"),
        FileFormat::Json,
        "%03x-%m-tail",
        "%03x-fix_bug-tail.vespertide.json"
    )] // `b'0'` arm's else: digits not terminated by `v` fall through untouched
    #[case(9, None, FileFormat::Json, "%v%", "0009%.vespertide.json")]
    // `i + 1 >= bytes.len()`: a trailing bare `%` is not a placeholder
    // Digits running to the very end exercise both `j < bytes.len()` bounds in
    // the `%0N` scan; relaxing either to `<=` indexes one past the slice.
    #[case(9, None, FileFormat::Json, "%012", "%012.vespertide.json")]
    // A width that differs from the 4-digit default proves the width really
    // comes from `pattern[i + 2..j]`: with `%04v` the padded and default
    // renderings coincide, so the slice bounds go unchecked.
    #[case(9, None, FileFormat::Json, "%06v", "000009.vespertide.json")]
    fn migration_filename_with_format_and_pattern_tests(
        #[case] version: u32,
        #[case] comment: Option<&str>,
        #[case] format: FileFormat,
        #[case] pattern: &str,
        #[case] expected: &str,
    ) {
        let name = migration_filename_with_format_and_pattern(version, comment, format, pattern);
        assert_eq!(name, expected);
    }

    #[test]
    #[serial]
    fn load_models_fails_on_invalid_fk_format() {
        let tmp = tempdir().unwrap();
        let _guard = CwdGuard::new(&tmp.path().to_path_buf());
        write_config();

        fs::create_dir_all("models").unwrap();

        // Create a model with invalid FK string format (missing dot separator)
        let table = TableDef {
            name: "orders".into(),
            description: None,
            columns: vec![ColumnDef {
                name: "user_id".into(),
                r#type: ColumnType::Simple(SimpleColumnType::Integer),
                nullable: false,
                default: None,
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                // Invalid FK format: should be "table.column" but missing the dot
                foreign_key: Some(ForeignKeySyntax::String("invalid_format".into())),
            }],
            constraints: vec![],
        };
        fs::write(
            "models/orders.json",
            serde_json::to_string_pretty(&table).unwrap(),
        )
        .unwrap();

        let result = load_models(&VespertideConfig::default());
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("Failed to normalize table 'orders'"));
    }
}
