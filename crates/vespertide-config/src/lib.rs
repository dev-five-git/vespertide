pub mod config;
pub mod file_format;
pub mod name_case;

pub use config::{SeaOrmConfig, VespertideConfig, default_migration_filename_pattern};
pub use file_format::FileFormat;
pub use name_case::NameCase;

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

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
        let json = r#"{}"#;
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
}
