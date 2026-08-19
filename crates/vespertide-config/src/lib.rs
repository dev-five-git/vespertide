//! Configuration parsing for vespertide projects.
//!
//! Reads `vespertide.json` (or `.yaml`) with paths, naming conventions,
//! and file format preferences.

pub mod config;
pub mod file_format;
pub mod name_case;

pub use config::{
    DEFAULT_GORM_PACKAGE_NAME, DjangoConfig, GormConfig, SeaOrmConfig, VespertideConfig,
    default_migration_filename_pattern,
};
pub use file_format::FileFormat;
pub use name_case::NameCase;

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use rstest::rstest;

    use super::*;

    #[test]
    fn default_values_are_snake_and_standard_paths() {
        let cfg = VespertideConfig::default();
        assert_eq!(cfg.models_dir, PathBuf::from("models"));
        assert_eq!(cfg.migrations_dir, PathBuf::from("migrations"));
        assert!(cfg.table_case().is_snake());
        assert!(cfg.column_case().is_snake());
    }

    #[test]
    fn overrides_work_via_struct_update() {
        let cfg = VespertideConfig {
            models_dir: PathBuf::from("custom_models"),
            migrations_dir: PathBuf::from("custom_migrations"),
            table_naming_case: NameCase::Camel,
            column_naming_case: NameCase::Pascal,
            ..Default::default()
        };

        assert_eq!(cfg.models_dir(), Path::new("custom_models"));
        assert_eq!(cfg.migrations_dir(), Path::new("custom_migrations"));
        assert!(cfg.table_case().is_camel());
        assert!(cfg.column_case().is_pascal());
    }

    #[test]
    fn seaorm_config_default_has_vespera_schema() {
        let cfg = SeaOrmConfig::default();
        assert_eq!(cfg.extra_enum_derives(), &["vespera::Schema".to_string()]);
        assert!(cfg.extra_model_derives().is_empty());
    }

    #[test]
    fn seaorm_config_accessors() {
        let cfg = SeaOrmConfig {
            extra_enum_derives: vec!["A".to_string(), "B".to_string()],
            extra_model_derives: vec!["C".to_string()],
            ..Default::default()
        };
        assert_eq!(cfg.extra_enum_derives(), &["A", "B"]);
        assert_eq!(cfg.extra_model_derives(), &["C"]);
    }

    #[test]
    fn vespertide_config_seaorm_accessor() {
        let cfg = VespertideConfig::default();
        let seaorm = cfg.seaorm();
        assert_eq!(
            seaorm.extra_enum_derives(),
            &["vespera::Schema".to_string()]
        );
    }

    #[test]
    fn seaorm_config_deserialize_with_defaults() {
        let json = r"{}";
        let cfg: SeaOrmConfig = serde_json::from_str(json).unwrap();
        assert_eq!(cfg.extra_enum_derives(), &["vespera::Schema".to_string()]);
        assert!(cfg.extra_model_derives().is_empty());
    }

    #[test]
    fn seaorm_config_deserialize_with_custom_values() {
        let json = r#"{"extraEnumDerives": ["Custom"], "extraModelDerives": ["Model"]}"#;
        let cfg: SeaOrmConfig = serde_json::from_str(json).unwrap();
        assert_eq!(cfg.extra_enum_derives(), &["Custom"]);
        assert_eq!(cfg.extra_model_derives(), &["Model"]);
    }

    #[test]
    fn vespertide_config_deserialize_with_seaorm() {
        let json = r#"{
            "modelsDir": "models",
            "migrationsDir": "migrations",
            "tableNamingCase": "snake",
            "columnNamingCase": "snake",
            "seaorm": {
                "extraEnumDerives": ["MyDerive"]
            }
        }"#;
        let cfg: VespertideConfig = serde_json::from_str(json).unwrap();
        assert_eq!(cfg.seaorm().extra_enum_derives(), &["MyDerive"]);
    }

    #[test]
    fn django_config_default_has_no_app_label() {
        let cfg = DjangoConfig::default();
        assert_eq!(cfg.app_label(), None);
    }

    #[test]
    fn django_config_accessor() {
        let cfg = DjangoConfig {
            app_label: Some("myapp".to_string()),
        };
        assert_eq!(cfg.app_label(), Some("myapp"));
    }

    #[test]
    fn django_config_deserialize_with_defaults() {
        let json = r"{}";
        let cfg: DjangoConfig = serde_json::from_str(json).unwrap();
        assert_eq!(cfg.app_label(), None);
    }

    #[test]
    fn django_config_deserialize_with_app_label() {
        let json = r#"{"appLabel": "myapp"}"#;
        let cfg: DjangoConfig = serde_json::from_str(json).unwrap();
        assert_eq!(cfg.app_label(), Some("myapp"));
    }

    #[test]
    fn django_config_app_label_absent_from_json_when_none() {
        let cfg = DjangoConfig::default();
        let json = serde_json::to_string(&cfg).unwrap();
        assert!(
            !json.contains("appLabel"),
            "None app_label must not serialize: {json}"
        );
    }

    #[test]
    fn gorm_config_default_package_name_is_none() {
        let cfg = GormConfig::default();
        assert_eq!(cfg.package_name(), None);
    }

    #[test]
    fn gorm_config_accessor() {
        let cfg = GormConfig {
            package_name: Some("entities".to_string()),
        };
        assert_eq!(cfg.package_name(), Some("entities"));
    }

    #[test]
    fn gorm_config_deserialize_with_defaults() {
        let json = r"{}";
        let cfg: GormConfig = serde_json::from_str(json).unwrap();
        assert_eq!(cfg.package_name(), None);
    }

    #[test]
    fn gorm_config_deserialize_with_custom_package_name() {
        let json = r#"{"packageName": "entities"}"#;
        let cfg: GormConfig = serde_json::from_str(json).unwrap();
        assert_eq!(cfg.package_name(), Some("entities"));
    }

    #[test]
    fn vespertide_config_django_and_gorm_accessors() {
        let cfg = VespertideConfig::default();
        assert_eq!(cfg.django().app_label(), None);
        assert_eq!(cfg.gorm().package_name(), None);
        // model_export_dir defaults to "src/models", so the inferred name matches
        // the pre-existing fixed default.
        assert_eq!(cfg.gorm_package_name(cfg.model_export_dir()), "models");
    }

    #[test]
    fn vespertide_config_deserialize_with_django_and_gorm() {
        let json = r#"{
            "modelsDir": "models",
            "migrationsDir": "migrations",
            "tableNamingCase": "snake",
            "columnNamingCase": "snake",
            "django": {
                "appLabel": "myapp"
            },
            "gorm": {
                "packageName": "entities"
            }
        }"#;
        let cfg: VespertideConfig = serde_json::from_str(json).unwrap();
        assert_eq!(cfg.django().app_label(), Some("myapp"));
        assert_eq!(cfg.gorm().package_name(), Some("entities"));
        assert_eq!(cfg.gorm_package_name(cfg.model_export_dir()), "entities");
    }

    #[rstest]
    #[case::default_dir_matches_folder("src/models", "models")]
    #[case::infers_from_folder_name("src/entities", "entities")]
    #[case::strips_invalid_chars("src/db-models", "dbmodels")]
    #[case::falls_back_when_digit_led("src/2024-models", "models")]
    #[case::falls_back_on_non_ascii("src/모델", "models")]
    #[case::falls_back_on_reserved_word("src/type", "models")]
    fn gorm_package_name_inferred_from_export_dir(
        #[case] export_dir: &str,
        #[case] expected: &str,
    ) {
        let cfg = VespertideConfig::default();
        assert_eq!(cfg.gorm_package_name(Path::new(export_dir)), expected);
    }

    #[test]
    fn gorm_package_name_explicit_override_wins_over_inference() {
        let cfg = VespertideConfig {
            gorm: GormConfig {
                package_name: Some("custom".to_string()),
            },
            ..Default::default()
        };
        assert_eq!(cfg.gorm_package_name(Path::new("src/entities")), "custom");
    }

    #[test]
    fn gorm_package_name_tracks_cli_export_dir_override_not_config_default() {
        // The `--export-dir` CLI flag can point somewhere other than
        // `model_export_dir`; the inferred package name must follow the
        // actual write target, since Go requires `package` to match the
        // directory the files live in.
        let cfg = VespertideConfig::default();
        assert_eq!(cfg.model_export_dir(), Path::new("src/models"));
        assert_eq!(cfg.gorm_package_name(Path::new("generated")), "generated");
    }
}
