use super::*;
pub(super) use crate::test_support::{CwdGuard, write_simple_id_model};
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

mod branches;
mod branches_more;
mod choices_apply;
mod delete_null_rows;
mod fill_with;
mod integration;
mod prompts;
mod recreate;
