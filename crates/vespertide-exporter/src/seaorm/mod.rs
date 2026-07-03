use std::collections::HashMap;

use crate::orm::OrmExporter;
use crate::parallel_config::{SEAORM_EXPORT_PAR_TABLE_MIN_LEN, SEAORM_EXPORT_PAR_TABLE_THRESHOLD};
use rayon::prelude::*;
use vespertide_config::SeaOrmConfig;
use vespertide_core::TableDef;

mod enums;
mod imports;
mod relations;
mod render;
mod types;

#[cfg(test)]
mod tests;

use render::render_entity_with_config_and_paths;

/// Test-only accessor for the `SeaORM` `to_pascal_case` helper.
///
/// Allows the cross-ORM consolidation test in `crate::tests` to exercise
/// the `SeaORM` naming helper without making it `pub(crate)` for the entire
/// crate.
#[cfg(test)]
pub(crate) fn to_pascal_case_for_tests(s: &str) -> String {
    imports::to_pascal_case(s)
}

#[cfg(test)]
use enums::*;
#[cfg(test)]
use imports::*;
#[cfg(test)]
use relations::*;
#[cfg(test)]
use render::*;
#[cfg(test)]
use types::*;

pub struct SeaOrmExporter;

/// `SeaORM` exporter with configuration support.
pub struct SeaOrmExporterWithConfig<'a> {
    pub config: &'a SeaOrmConfig,
    pub prefix: &'a str,
}

impl OrmExporter for SeaOrmExporter {
    fn render_entity(&self, table: &TableDef) -> Result<String, String> {
        Ok(render_entity(table))
    }

    fn render_entity_with_schema(
        &self,
        table: &TableDef,
        schema: &[TableDef],
    ) -> Result<String, String> {
        Ok(render_entity_with_schema(table, schema))
    }
}

impl<'a> SeaOrmExporterWithConfig<'a> {
    pub fn new(config: &'a SeaOrmConfig, prefix: &'a str) -> Self {
        Self { config, prefix }
    }

    pub fn render_entity(&self, table: &TableDef) -> Result<String, String> {
        Ok(render_entity_with_config(
            table,
            &[],
            self.config,
            self.prefix,
        ))
    }

    pub fn render_entity_with_schema(
        &self,
        table: &TableDef,
        schema: &[TableDef],
    ) -> Result<String, String> {
        Ok(render_entity_with_config(
            table,
            schema,
            self.config,
            self.prefix,
        ))
    }

    /// Render entity with schema context and module path mappings for correct
    /// cross-directory relation paths (e.g., `super::super::admin::admin::Entity`).
    pub fn render_entity_with_schema_and_paths(
        &self,
        table: &TableDef,
        schema: &[TableDef],
        module_paths: &HashMap<String, Vec<String>>,
        crate_prefix: &str,
    ) -> Result<String, String> {
        Ok(render_entity_with_config_and_paths(
            table,
            schema,
            self.config,
            self.prefix,
            module_paths,
            crate_prefix,
        ))
    }
}

/// Render a single table into `SeaORM` entity code.
///
/// Follows the official entity format:
/// <https://www.sea-ql.org/SeaORM/docs/generate-entity/entity-format/>
#[inline]
pub fn render_entity(table: &TableDef) -> String {
    render_entity_with_schema(table, &[])
}

/// Render a single table into `SeaORM` entity code with schema context for FK chain resolution.
#[inline]
pub fn render_entity_with_schema(table: &TableDef, schema: &[TableDef]) -> String {
    render_entity_with_config(table, schema, &SeaOrmConfig::default(), "")
}

/// Render a single table into `SeaORM` entity code with schema context and configuration.
pub fn render_entity_with_config(
    table: &TableDef,
    schema: &[TableDef],
    config: &SeaOrmConfig,
    prefix: &str,
) -> String {
    render_entity_with_config_and_paths(table, schema, config, prefix, &HashMap::new(), "")
}

/// Render a complete `SeaORM` schema as ordered entity modules.
///
/// Per-table rendering is pure (`&TableDef` + immutable schema context -> `String`),
/// so larger schemas render in parallel while collecting into a `Vec` to preserve
/// input order byte-for-byte.
pub fn export(schema: &[TableDef]) -> Result<String, String> {
    let rendered: Result<Vec<String>, String> = if schema.len() < SEAORM_EXPORT_PAR_TABLE_THRESHOLD
    {
        schema
            .iter()
            .map(|table| Ok(render_entity_with_schema(table, schema)))
            .collect()
    } else {
        schema
            .par_iter()
            .with_min_len(SEAORM_EXPORT_PAR_TABLE_MIN_LEN)
            .map(|table| Ok(render_entity_with_schema(table, schema)))
            .collect()
    };

    rendered.map(|parts| parts.join("\n\n"))
}
