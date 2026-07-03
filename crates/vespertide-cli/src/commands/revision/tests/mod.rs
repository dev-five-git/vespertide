use super::*;
pub(super) use crate::test_support::CwdGuard;
pub(super) use anyhow::Result;
pub(super) use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fs as std_fs,
    path::PathBuf,
};
pub(super) use tempfile::tempdir;
pub(super) use vespertide_config::{FileFormat, VespertideConfig};
pub(super) use vespertide_core::{
    ColumnDef, ColumnType, MigrationAction, MigrationPlan, SimpleColumnType, TableConstraint,
    TableDef,
};

fn write_config() -> VespertideConfig {
    write_config_with_format(None)
}

fn write_config_with_format(fmt: Option<FileFormat>) -> VespertideConfig {
    let mut cfg = VespertideConfig::default();
    if let Some(f) = fmt {
        cfg.migration_format = f;
    }
    let text = serde_json::to_string_pretty(&cfg).unwrap();
    std_fs::write("vespertide.json", text).unwrap();
    cfg
}

fn write_model(name: &str) {
    let models_dir = PathBuf::from("models");
    std_fs::create_dir_all(&models_dir).unwrap();
    let table = TableDef {
        name: name.into(),
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
    let path = models_dir.join(format!("{name}.json"));
    std_fs::write(path, serde_json::to_string_pretty(&table).unwrap()).unwrap();
}

mod branches;
mod branches_more;
mod choices_apply;
mod delete_null_rows;
mod fill_with;
mod integration;
mod prompts;
mod recreate;
