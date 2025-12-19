use vespertide_config::FileFormat;

// Re-export loader functions for convenience
pub use vespertide_loader::{load_config, load_migrations, load_models};

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
    use rstest::rstest;
    use serial_test::serial;
    use std::fs;
    use std::path::PathBuf;
    use tempfile::tempdir;
    use vespertide_config::VespertideConfig;
    use vespertide_core::{
        ColumnDef, ColumnType, MigrationPlan, SimpleColumnType, TableConstraint, TableDef,
        schema::foreign_key::ForeignKeySyntax,
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

    #[rstest]
    #[case(
        5,
        Some("Hello! World"),
        FileFormat::Yml,
        "%04v_%m",
        "0005_hello__world.yml"
    )]
    #[case(3, None, FileFormat::Json, "%0v__", "0003.json")] // width 0 falls back to default version and trailing separators are trimmed
    #[case(12, None, FileFormat::Json, "%v", "0012.json")]
    #[case(7, None, FileFormat::Json, "%m", "0007.json")] // uses default when comment only and empty
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
