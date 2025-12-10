pub mod config;
pub mod file_format;
pub mod name_case;

pub use config::{default_migration_filename_pattern, VespertideConfig};
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
}
