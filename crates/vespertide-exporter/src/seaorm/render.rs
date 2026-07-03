use std::collections::{HashMap, HashSet};

use vespertide_config::SeaOrmConfig;
use vespertide_core::{ColumnDef, ColumnType, ComplexColumnType, TableConstraint, TableDef};

use super::enums::render_enum;
use super::imports::{sanitize_field_name, to_pascal_case};
use super::relations::{
    relation_field_defs_with_schema, render_self_ref_link_helpers, render_self_ref_query_helpers,
};
use super::types::{column_type_supports_eq, format_default_value};

/// Render a single table into `SeaORM` entity code with schema context, configuration,
/// and module path mappings for correct cross-directory relation paths.
pub fn render_entity_with_config_and_paths(
    table: &TableDef,
    schema: &[TableDef],
    config: &SeaOrmConfig,
    prefix: &str,
    module_paths: &HashMap<String, Vec<String>>,
    crate_prefix: &str,
) -> String {
    let primary_keys = primary_key_columns(table);
    let composite_pk = primary_keys.len() > 1;
    let relation_fields =
        relation_field_defs_with_schema(table, schema, module_paths, crate_prefix);

    // Build sets of columns with single-column unique constraints and indexes
    let unique_columns = single_column_unique_set(&table.constraints);
    let indexed_columns = single_column_index_set(&table.constraints);

    // Check if any columns use enum types (enums derive Serialize/Deserialize)
    let has_enums = table.columns.iter().any(|c| {
        matches!(
            c.r#type,
            ColumnType::Complex(ComplexColumnType::Enum { .. })
        )
    });

    let mut lines: Vec<String> = Vec::new();
    lines.push("use sea_orm::entity::prelude::*;".into());
    if has_enums {
        lines.push("use serde::{Deserialize, Serialize};".into());
    }
    lines.push(String::new());

    // Generate Enum definitions first
    let mut processed_enums = HashSet::new();
    for column in &table.columns {
        if let ColumnType::Complex(ComplexColumnType::Enum { name, values }) = &column.r#type {
            // Avoid duplicate enum definitions if multiple columns use the same enum
            if !processed_enums.contains(name) {
                render_enum(&mut lines, &table.name, name, values, config);
                processed_enums.insert(name.clone());
            }
        }
    }

    // Build model derive line with optional extra derives.
    // Float-backed fields (f32/f64) cannot implement Eq, so omit it when present.
    let mut model_derives = vec!["Clone", "Debug", "PartialEq"];
    if table
        .columns
        .iter()
        .all(|column| column_type_supports_eq(&column.r#type))
    {
        model_derives.push("Eq");
    }
    model_derives.push("DeriveEntityModel");
    let extra_model_derives: Vec<&str> = config
        .extra_model_derives()
        .iter()
        .map(std::string::String::as_str)
        .collect();
    model_derives.extend(extra_model_derives);

    // Add table description as doc comment
    if let Some(ref desc) = table.description {
        for line in desc.lines() {
            lines.push(format!("/// {line}"));
        }
    }

    lines.push("#[sea_orm::model]".into());
    lines.push(format!("#[derive({})]", model_derives.join(", ")));
    lines.push(format!(
        "#[sea_orm(table_name = \"{}{}\")]",
        prefix, table.name
    ));
    lines.push("pub struct Model {".into());

    for column in &table.columns {
        render_column(
            &mut lines,
            column,
            &primary_keys,
            composite_pk,
            &unique_columns,
            &indexed_columns,
        );
    }
    for field in relation_fields {
        lines.push(field);
    }

    lines.push("}".into());

    // Indexes (relations expressed as belongs_to fields above)
    lines.push(String::new());
    render_indexes_and_uniques(&mut lines, &table.constraints);

    // Generate vespera::schema_type! macro if enabled
    if config.vespera_schema_type() {
        let pascal_name = to_pascal_case(&table.name);
        lines.push(format!(
            "vespera::schema_type!(Schema from Model, name = \"{pascal_name}Schema\");"
        ));
    }

    lines.push("impl ActiveModelBehavior for ActiveModel {}".into());

    let self_ref_links = render_self_ref_link_helpers(table, schema, module_paths, crate_prefix);
    if !self_ref_links.is_empty() {
        lines.push(String::new());
        lines.extend(self_ref_links);
    }

    let self_ref_query_helpers = render_self_ref_query_helpers(table, schema);
    if !self_ref_query_helpers.is_empty() {
        lines.push(String::new());
        lines.extend(self_ref_query_helpers);
    }

    lines.push(String::new());

    lines.join("\n")
}

/// Build a set of column names that have single-column unique constraints.
pub(super) fn single_column_unique_set(constraints: &[TableConstraint]) -> HashSet<String> {
    let mut unique_cols = HashSet::new();
    for constraint in constraints {
        if let TableConstraint::Unique { columns, .. } = constraint
            && columns.len() == 1
        {
            unique_cols.insert(columns[0].to_string());
        }
    }
    unique_cols
}

/// Build a set of column names that have single-column indexes from constraints.
pub(super) fn single_column_index_set(constraints: &[TableConstraint]) -> HashSet<String> {
    let mut indexed_cols = HashSet::new();
    for constraint in constraints {
        if let TableConstraint::Index { columns, .. } = constraint
            && columns.len() == 1
        {
            indexed_cols.insert(columns[0].to_string());
        }
    }
    indexed_cols
}

pub(super) fn render_column(
    lines: &mut Vec<String>,
    column: &ColumnDef,
    primary_keys: &HashSet<String>,
    composite_pk: bool,
    unique_columns: &HashSet<String>,
    indexed_columns: &HashSet<String>,
) {
    let is_pk = primary_keys.contains(column.name.as_str());
    let is_unique = unique_columns.contains(column.name.as_str());
    let is_indexed = indexed_columns.contains(column.name.as_str());
    let has_default = column.default.is_some();

    // Add column comment as doc comment
    if let Some(ref comment) = column.comment {
        for line in comment.lines() {
            lines.push(format!("    /// {line}"));
        }
    }

    // Build attribute parts
    let mut attrs: Vec<String> = Vec::new();

    if is_pk {
        attrs.push("primary_key".into());
        // Only show auto_increment = false for integer types that support auto_increment
        if composite_pk && column.r#type.supports_auto_increment() {
            attrs.push("auto_increment = false".into());
        }
    }

    if is_unique && !is_pk {
        // unique is redundant if it's already a primary key
        attrs.push("unique".into());
    }

    if is_indexed && !is_pk && !is_unique {
        // indexed is redundant if it's already a primary key or unique
        attrs.push("indexed".into());
    }

    if has_default && let Some(ref default_val) = column.default {
        // Format the default value for SeaORM
        let formatted = format_default_value(default_val, &column.r#type);
        attrs.push(formatted);
    }

    // For custom types, add column_type attribute with the custom type value
    if let ColumnType::Complex(ComplexColumnType::Custom { custom_type }) = &column.r#type {
        attrs.push(format!("column_type = \"{custom_type}\""));
    }

    // Output attribute if any
    if !attrs.is_empty() {
        lines.push(format!("    #[sea_orm({})]", attrs.join(", ")));
    }

    let field_name = sanitize_field_name(&column.name);

    let ty = match &column.r#type {
        ColumnType::Complex(ComplexColumnType::Enum { name, .. }) => {
            let enum_type = to_pascal_case(name);
            if column.nullable {
                format!("Option<{enum_type}>")
            } else {
                enum_type
            }
        }
        // JSONB custom type should use Json rust type
        ColumnType::Complex(ComplexColumnType::Custom { custom_type })
            if custom_type.to_uppercase() == "JSONB" =>
        {
            if column.nullable {
                "Option<Json>".to_string()
            } else {
                "Json".to_string()
            }
        }
        _ => column.r#type.to_rust_type(column.nullable),
    };

    lines.push(format!("    pub {field_name}: {ty},"));
}
pub(super) fn primary_key_columns(table: &TableDef) -> HashSet<String> {
    use vespertide_core::schema::primary_key::PrimaryKeySyntax;
    let mut keys = HashSet::new();

    // First, check table-level constraints
    for constraint in &table.constraints {
        if let TableConstraint::PrimaryKey { columns, .. } = constraint {
            for col in columns {
                keys.insert(col.to_string());
            }
        }
    }

    // Then, check inline primary_key on columns
    // This handles cases where primary_key is defined inline but not yet normalized
    for column in &table.columns {
        if let Some(PrimaryKeySyntax::Bool(true) | PrimaryKeySyntax::Object(_)) =
            &column.primary_key
        {
            keys.insert(column.name.to_string());
        }
    }

    keys
}
pub(super) fn render_indexes_and_uniques(lines: &mut Vec<String>, constraints: &[TableConstraint]) {
    let index_constraints: Vec<_> = constraints
        .iter()
        .filter_map(|c| {
            if let TableConstraint::Index { name, columns } = c {
                Some((name, columns))
            } else {
                None
            }
        })
        .collect();

    let composite_uniques: Vec<_> = constraints
        .iter()
        .filter_map(|c| {
            if let TableConstraint::Unique { name, columns, .. } = c {
                if columns.len() > 1 {
                    Some((name, columns))
                } else {
                    None
                }
            } else {
                None
            }
        })
        .collect();

    if index_constraints.is_empty() && composite_uniques.is_empty() {
        return;
    }

    if !index_constraints.is_empty() {
        lines.push("// Index definitions (SeaORM uses Statement builders externally)".into());
        for (name, columns) in index_constraints {
            let cols = columns.join(", ");
            let idx_name = name.clone().unwrap_or_else(|| "(unnamed)".to_string());
            lines.push(format!("// {idx_name} on [{cols}]"));
        }
    }

    if !composite_uniques.is_empty() {
        lines.push(String::new());
        lines.push(
            "/// Composite unique constraints — declare in migrations or use Statement builder."
                .into(),
        );
        lines.push("pub const COMPOSITE_UNIQUES: &[&[&str]] = &[".into());
        for (name, columns) in composite_uniques {
            let cols_str = columns
                .iter()
                .map(|c| format!("\"{c}\""))
                .collect::<Vec<_>>()
                .join(", ");
            let comment = name
                .as_deref()
                .map(|n| format!(" // {n}"))
                .unwrap_or_default();
            lines.push(format!("    &[{cols_str}],{comment}"));
        }
        lines.push("];".into());
    }
}
