use std::collections::HashSet;

use crate::orm::OrmExporter;
use vespertide_core::{
    ColumnDef, ColumnType, ComplexColumnType, IndexDef, TableConstraint, TableDef,
};

pub struct SeaOrmExporter;

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

/// Render a single table into SeaORM entity code.
///
/// Follows the official entity format:
/// <https://www.sea-ql.org/SeaORM/docs/generate-entity/entity-format/>
pub fn render_entity(table: &TableDef) -> String {
    render_entity_with_schema(table, &[])
}

/// Render a single table into SeaORM entity code with schema context for FK chain resolution.
pub fn render_entity_with_schema(table: &TableDef, schema: &[TableDef]) -> String {
    let primary_keys = primary_key_columns(table);
    let composite_pk = primary_keys.len() > 1;
    let indexes = &table.indexes;
    let relation_fields = relation_field_defs_with_schema(table, schema);

    // Build sets of columns with single-column unique constraints and indexes
    let unique_columns = single_column_unique_set(&table.constraints);
    let indexed_columns = single_column_index_set(indexes);

    let mut lines: Vec<String> = Vec::new();
    lines.push("use sea_orm::entity::prelude::*;".into());
    lines.push("use serde::{Deserialize, Serialize};".into());
    lines.push(String::new());

    // Generate Enum definitions first
    let mut processed_enums = HashSet::new();
    for column in &table.columns {
        if let ColumnType::Complex(ComplexColumnType::Enum { name, values }) = &column.r#type {
            // Avoid duplicate enum definitions if multiple columns use the same enum
            if !processed_enums.contains(name) {
                render_enum(&mut lines, name, values);
                processed_enums.insert(name.clone());
            }
        }
    }

    lines.push("#[sea_orm::model]".into());
    lines.push(
        "#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize, Deserialize)]".into(),
    );
    lines.push(format!("#[sea_orm(table_name = \"{}\")]", table.name));
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
    render_indexes(&mut lines, indexes);

    lines.push("impl ActiveModelBehavior for ActiveModel {}".into());

    lines.push(String::new());

    lines.join("\n")
}

/// Build a set of column names that have single-column unique constraints.
fn single_column_unique_set(constraints: &[TableConstraint]) -> HashSet<String> {
    let mut unique_cols = HashSet::new();
    for constraint in constraints {
        if let TableConstraint::Unique { columns, .. } = constraint
            && columns.len() == 1
        {
            unique_cols.insert(columns[0].clone());
        }
    }
    unique_cols
}

/// Build a set of column names that have single-column indexes.
fn single_column_index_set(indexes: &[IndexDef]) -> HashSet<String> {
    let mut indexed_cols = HashSet::new();
    for index in indexes {
        if index.columns.len() == 1 {
            indexed_cols.insert(index.columns[0].clone());
        }
    }
    indexed_cols
}

fn render_column(
    lines: &mut Vec<String>,
    column: &ColumnDef,
    primary_keys: &HashSet<String>,
    composite_pk: bool,
    unique_columns: &HashSet<String>,
    indexed_columns: &HashSet<String>,
) {
    let is_pk = primary_keys.contains(&column.name);
    let is_unique = unique_columns.contains(&column.name);
    let is_indexed = indexed_columns.contains(&column.name);
    let has_default = column.default.is_some();

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

    // Output attribute if any
    if !attrs.is_empty() {
        lines.push(format!("    #[sea_orm({})]", attrs.join(", ")));
    }

    let field_name = sanitize_field_name(&column.name);

    let ty = match &column.r#type {
        ColumnType::Complex(ComplexColumnType::Enum { name, .. }) => {
            let enum_type = to_pascal_case(name);
            if column.nullable {
                format!("Option<{}>", enum_type)
            } else {
                enum_type
            }
        }
        _ => column.r#type.to_rust_type(column.nullable),
    };

    lines.push(format!("    pub {}: {},", field_name, ty));
}

/// Format default value for SeaORM attribute.
/// Returns the full attribute string like `default_value = "..."` or `default_value = 0`.
fn format_default_value(value: &str, column_type: &ColumnType) -> String {
    let trimmed = value.trim();

    // Remove surrounding single quotes if present (SQL string literals)
    let cleaned = if trimmed.starts_with('\'') && trimmed.ends_with('\'') && trimmed.len() >= 2 {
        &trimmed[1..trimmed.len() - 1]
    } else {
        trimmed
    };

    // Format based on column type
    match column_type {
        // Numeric types: no quotes
        ColumnType::Simple(simple) if is_numeric_simple_type(simple) => {
            format!("default_value = {}", cleaned)
        }
        // Boolean type: no quotes
        ColumnType::Simple(vespertide_core::SimpleColumnType::Boolean) => {
            format!("default_value = {}", cleaned)
        }
        // Numeric complex type: no quotes
        ColumnType::Complex(ComplexColumnType::Numeric { .. }) => {
            format!("default_value = {}", cleaned)
        }
        // Enum type: use enum variant format
        ColumnType::Complex(ComplexColumnType::Enum { name, .. }) => {
            let enum_name = to_pascal_case(name);
            let variant = to_pascal_case(cleaned);
            format!("default_value = {}::{}", enum_name, variant)
        }
        // All other types: use quotes
        _ => {
            format!("default_value = \"{}\"", cleaned)
        }
    }
}

/// Check if the simple column type is numeric.
fn is_numeric_simple_type(simple: &vespertide_core::SimpleColumnType) -> bool {
    use vespertide_core::SimpleColumnType;
    matches!(
        simple,
        SimpleColumnType::SmallInt
            | SimpleColumnType::Integer
            | SimpleColumnType::BigInt
            | SimpleColumnType::Real
            | SimpleColumnType::DoublePrecision
    )
}

fn primary_key_columns(table: &TableDef) -> HashSet<String> {
    use vespertide_core::schema::primary_key::PrimaryKeySyntax;
    let mut keys = HashSet::new();

    // First, check table-level constraints
    for constraint in &table.constraints {
        if let TableConstraint::PrimaryKey { columns, .. } = constraint {
            for col in columns {
                keys.insert(col.clone());
            }
        }
    }

    // Then, check inline primary_key on columns
    // This handles cases where primary_key is defined inline but not yet normalized
    for column in &table.columns {
        match &column.primary_key {
            Some(PrimaryKeySyntax::Bool(true)) | Some(PrimaryKeySyntax::Object(_)) => {
                keys.insert(column.name.clone());
            }
            _ => {}
        }
    }

    keys
}

/// Resolve FK chain to find the ultimate target table.
/// If the referenced column is itself a FK, follow the chain.
fn resolve_fk_target<'a>(
    ref_table: &'a str,
    ref_columns: &[String],
    schema: &'a [TableDef],
) -> (&'a str, Vec<String>) {
    // If no schema context or ref_columns is not a single column, return as-is
    if schema.is_empty() || ref_columns.len() != 1 {
        return (ref_table, ref_columns.to_vec());
    }

    let ref_col = &ref_columns[0];

    // Find the referenced table in schema
    let Some(target_table) = schema.iter().find(|t| t.name == ref_table) else {
        return (ref_table, ref_columns.to_vec());
    };

    // Check if the referenced column has a FK constraint
    for constraint in &target_table.constraints {
        if let TableConstraint::ForeignKey {
            columns,
            ref_table: next_ref_table,
            ref_columns: next_ref_columns,
            ..
        } = constraint
        {
            // If the FK is on the column we're referencing
            if columns.len() == 1 && columns[0] == *ref_col {
                // Recursively resolve the FK chain
                return resolve_fk_target(next_ref_table, next_ref_columns, schema);
            }
        }
    }

    // No further FK chain, return current target
    (ref_table, ref_columns.to_vec())
}

fn relation_field_defs_with_schema(table: &TableDef, schema: &[TableDef]) -> Vec<String> {
    let mut out = Vec::new();
    let mut used = HashSet::new();

    // belongs_to relations (this table has FK to other tables)
    for constraint in &table.constraints {
        if let TableConstraint::ForeignKey {
            columns,
            ref_table,
            ref_columns,
            ..
        } = constraint
        {
            // Resolve FK chain to find ultimate target
            let (resolved_table, resolved_columns) =
                resolve_fk_target(ref_table, ref_columns, schema);

            let base = sanitize_field_name(resolved_table);
            let field_name = unique_name(&base, &mut used);
            let from = fk_attr_value(columns);
            let to = fk_attr_value(&resolved_columns);
            out.push(format!(
                "    #[sea_orm(belongs_to, from = \"{from}\", to = \"{to}\")]"
            ));
            out.push(format!(
                "    pub {field_name}: HasOne<super::{resolved_table}::Entity>,"
            ));
        }
    }

    // has_one/has_many relations (other tables have FK to this table)
    let reverse_relations = reverse_relation_field_defs(table, schema, &mut used);
    out.extend(reverse_relations);

    out
}

/// Generate reverse relation fields (has_one/has_many) for tables that reference this table.
fn reverse_relation_field_defs(
    table: &TableDef,
    schema: &[TableDef],
    used: &mut HashSet<String>,
) -> Vec<String> {
    let mut out = Vec::new();

    // Find all tables that have FK to this table
    for other_table in schema {
        if other_table.name == table.name {
            continue;
        }

        // Get PK and unique columns for the other table
        let other_pk = primary_key_columns(other_table);
        let other_unique = single_column_unique_set(&other_table.constraints);

        // Check if this is a junction table (composite PK with multiple FKs)
        if let Some(m2m_relations) =
            detect_many_to_many(table, other_table, &other_pk, schema, used)
        {
            out.extend(m2m_relations);
            continue;
        }

        for constraint in &other_table.constraints {
            if let TableConstraint::ForeignKey {
                columns, ref_table, ..
            } = constraint
            {
                // Check if this FK references our table
                if ref_table == &table.name {
                    // Determine if it's has_one or has_many
                    // has_one: FK columns exactly match the entire PK, or have UNIQUE constraint
                    // has_many: FK columns don't uniquely identify the row
                    let is_one_to_one = if columns.len() == 1 {
                        let col = &columns[0];
                        // Single column FK: check if it's the entire PK (not just part of composite PK)
                        // or has a UNIQUE constraint
                        let is_sole_pk = other_pk.len() == 1 && other_pk.contains(col);
                        let is_unique = other_unique.contains(col);
                        is_sole_pk || is_unique
                    } else {
                        // Composite FK: check if FK columns exactly match the entire PK
                        columns.len() == other_pk.len()
                            && columns.iter().all(|c| other_pk.contains(c))
                    };

                    let relation_type = if is_one_to_one { "has_one" } else { "has_many" };
                    let rust_type = if is_one_to_one { "HasOne" } else { "HasMany" };

                    // Use plural form for has_many field names
                    let base = if is_one_to_one {
                        sanitize_field_name(&other_table.name)
                    } else {
                        pluralize(&sanitize_field_name(&other_table.name))
                    };
                    let field_name = unique_name(&base, used);

                    // has_one/has_many don't use from/to attributes (unlike belongs_to)
                    out.push(format!("    #[sea_orm({relation_type})]"));
                    out.push(format!(
                        "    pub {field_name}: {rust_type}<super::{}::Entity>,",
                        other_table.name
                    ));
                }
            }
        }
    }

    out
}

/// Detect if a table is a junction table for many-to-many relationship.
/// Returns Some(relations) if it's a junction table that links current table to other tables,
/// or None if it's not a junction table.
fn detect_many_to_many(
    current_table: &TableDef,
    junction_table: &TableDef,
    junction_pk: &HashSet<String>,
    schema: &[TableDef],
    used: &mut HashSet<String>,
) -> Option<Vec<String>> {
    // Junction table must have composite PK (2+ columns)
    if junction_pk.len() < 2 {
        return None;
    }

    // Collect all FKs from the junction table
    let fks: Vec<_> = junction_table
        .constraints
        .iter()
        .filter_map(|c| {
            if let TableConstraint::ForeignKey {
                columns, ref_table, ..
            } = c
            {
                Some((columns.clone(), ref_table.clone()))
            } else {
                None
            }
        })
        .collect();

    // Must have at least 2 FKs to be a junction table
    if fks.len() < 2 {
        return None;
    }

    // Check if all FK columns are part of the PK (typical junction table pattern)
    let all_fk_cols_in_pk = fks
        .iter()
        .all(|(cols, _)| cols.iter().all(|c| junction_pk.contains(c)));

    if !all_fk_cols_in_pk {
        return None;
    }

    // Find which FK references the current table
    fks.iter()
        .find(|(_, ref_table)| ref_table == &current_table.name)?;

    // Generate many-to-many relations via this junction table
    let mut out = Vec::new();

    // First, add has_many to the junction table itself
    let junction_base = pluralize(&sanitize_field_name(&junction_table.name));
    let junction_field_name = unique_name(&junction_base, used);
    out.push("    #[sea_orm(has_many)]".to_string());
    out.push(format!(
        "    pub {junction_field_name}: HasMany<super::{}::Entity>,",
        junction_table.name
    ));

    // Then add has_many with via for the target tables
    for (_, ref_table) in &fks {
        // Skip the FK to the current table itself
        if ref_table == &current_table.name {
            continue;
        }

        // Find the target table in schema
        let target_exists = schema.iter().any(|t| &t.name == ref_table);
        if !target_exists {
            continue;
        }

        // Generate has_many with via
        let base = pluralize(&sanitize_field_name(ref_table));
        let field_name = unique_name(&base, used);

        out.push(format!(
            "    #[sea_orm(has_many, via = \"{}\")]",
            junction_table.name
        ));
        out.push(format!(
            "    pub {field_name}: HasMany<super::{ref_table}::Entity>,",
        ));
    }

    Some(out)
}

/// Simple pluralization for field names (adds 's' suffix).
fn pluralize(name: &str) -> String {
    if name.ends_with('s') || name.ends_with("es") {
        name.to_string()
    } else if name.ends_with('y')
        && !name.ends_with("ay")
        && !name.ends_with("ey")
        && !name.ends_with("oy")
        && !name.ends_with("uy")
    {
        // e.g., category -> categories
        format!("{}ies", &name[..name.len() - 1])
    } else {
        format!("{}s", name)
    }
}

fn fk_attr_value(cols: &[String]) -> String {
    if cols.len() == 1 {
        cols[0].clone()
    } else {
        format!("({})", cols.join(", "))
    }
}

fn render_indexes(lines: &mut Vec<String>, indexes: &[IndexDef]) {
    if indexes.is_empty() {
        return;
    }
    lines.push(String::new());
    lines.push("// Index definitions (SeaORM uses Statement builders externally)".into());
    for idx in indexes {
        let cols = idx.columns.join(", ");
        lines.push(format!(
            "// {} on [{}] unique={}",
            idx.name, cols, idx.unique
        ));
    }
}

fn sanitize_field_name(name: &str) -> String {
    let mut result = String::new();

    for (idx, ch) in name.chars().enumerate() {
        if (ch.is_ascii_alphanumeric() && (idx > 0 || ch.is_ascii_alphabetic())) || ch == '_' {
            result.push(ch);
        } else if idx == 0 && ch.is_ascii_digit() {
            result.push('_');
            result.push(ch);
        } else {
            result.push('_');
        }
    }

    if result.is_empty() {
        "_col".into()
    } else {
        result
    }
}

fn unique_name(base: &str, used: &mut HashSet<String>) -> String {
    let mut name = base.to_string();
    let mut i = 1;
    while used.contains(&name) {
        name = format!("{base}_{i}");
        i += 1;
    }
    used.insert(name.clone());
    name
}

fn render_enum(lines: &mut Vec<String>, name: &str, values: &[String]) {
    let enum_name = to_pascal_case(name);

    lines.push("#[derive(Debug, Clone, PartialEq, Eq, EnumIter, DeriveActiveEnum, Serialize, Deserialize)]".into());
    lines.push(format!(
        "#[sea_orm(rs_type = \"String\", db_type = \"Enum\", enum_name = \"{}\")]",
        name
    ));
    lines.push(format!("pub enum {} {{", enum_name));

    for value in values {
        let variant_name = enum_variant_name(value);
        lines.push(format!("    #[sea_orm(string_value = \"{}\")]", value));
        lines.push(format!("    {},", variant_name));
    }
    lines.push("}".into());
    lines.push(String::new());
}

/// Convert a string to a valid Rust enum variant name (PascalCase).
/// Handles edge cases like numeric prefixes, special characters, and reserved words.
fn enum_variant_name(s: &str) -> String {
    let pascal = to_pascal_case(s);

    // Handle empty string
    if pascal.is_empty() {
        return "Value".to_string();
    }

    // Handle numeric prefix: prefix with underscore or 'N'
    if pascal
        .chars()
        .next()
        .map(|c| c.is_ascii_digit())
        .unwrap_or(false)
    {
        return format!("N{}", pascal);
    }

    pascal
}

fn to_pascal_case(s: &str) -> String {
    let mut result = String::new();
    let mut capitalize = true;
    for c in s.chars() {
        if c == '_' || c == '-' {
            capitalize = true;
        } else if capitalize {
            result.push(c.to_ascii_uppercase());
            capitalize = false;
        } else {
            result.push(c);
        }
    }
    result
}

#[cfg(test)]
mod helper_tests {
    use super::*;
    use vespertide_core::IndexDef;

    #[test]
    fn test_render_indexes() {
        let mut lines = Vec::new();
        let indexes = vec![
            IndexDef {
                name: "idx_users_email".into(),
                columns: vec!["email".into()],
                unique: false,
            },
            IndexDef {
                name: "idx_users_name_email".into(),
                columns: vec!["name".into(), "email".into()],
                unique: true,
            },
        ];
        render_indexes(&mut lines, &indexes);
        assert!(!lines.is_empty());
        assert!(lines.iter().any(|l| l.contains("idx_users_email")));
        assert!(lines.iter().any(|l| l.contains("idx_users_name_email")));
    }

    #[test]
    fn test_render_indexes_empty() {
        let mut lines = Vec::new();
        render_indexes(&mut lines, &[]);
        // Should not add anything when indexes are empty
        assert_eq!(lines.len(), 0);
    }

    #[test]
    fn test_rust_type() {
        use vespertide_core::{ColumnType, ComplexColumnType, SimpleColumnType};
        // Numeric types
        assert_eq!(
            ColumnType::Simple(SimpleColumnType::SmallInt).to_rust_type(false),
            "i16"
        );
        assert_eq!(
            ColumnType::Simple(SimpleColumnType::SmallInt).to_rust_type(true),
            "Option<i16>"
        );
        assert_eq!(
            ColumnType::Simple(SimpleColumnType::Integer).to_rust_type(false),
            "i32"
        );
        assert_eq!(
            ColumnType::Simple(SimpleColumnType::Integer).to_rust_type(true),
            "Option<i32>"
        );
        assert_eq!(
            ColumnType::Simple(SimpleColumnType::BigInt).to_rust_type(false),
            "i64"
        );
        assert_eq!(
            ColumnType::Simple(SimpleColumnType::BigInt).to_rust_type(true),
            "Option<i64>"
        );
        assert_eq!(
            ColumnType::Simple(SimpleColumnType::Real).to_rust_type(false),
            "f32"
        );
        assert_eq!(
            ColumnType::Simple(SimpleColumnType::DoublePrecision).to_rust_type(false),
            "f64"
        );

        // Text type
        assert_eq!(
            ColumnType::Simple(SimpleColumnType::Text).to_rust_type(false),
            "String"
        );
        assert_eq!(
            ColumnType::Simple(SimpleColumnType::Text).to_rust_type(true),
            "Option<String>"
        );

        // Boolean type
        assert_eq!(
            ColumnType::Simple(SimpleColumnType::Boolean).to_rust_type(false),
            "bool"
        );
        assert_eq!(
            ColumnType::Simple(SimpleColumnType::Boolean).to_rust_type(true),
            "Option<bool>"
        );

        // Date/Time types
        assert_eq!(
            ColumnType::Simple(SimpleColumnType::Date).to_rust_type(false),
            "Date"
        );
        assert_eq!(
            ColumnType::Simple(SimpleColumnType::Time).to_rust_type(false),
            "Time"
        );
        assert_eq!(
            ColumnType::Simple(SimpleColumnType::Timestamp).to_rust_type(false),
            "DateTime"
        );
        assert_eq!(
            ColumnType::Simple(SimpleColumnType::Timestamp).to_rust_type(true),
            "Option<DateTime>"
        );
        assert_eq!(
            ColumnType::Simple(SimpleColumnType::Timestamptz).to_rust_type(false),
            "DateTimeWithTimeZone"
        );
        assert_eq!(
            ColumnType::Simple(SimpleColumnType::Timestamptz).to_rust_type(true),
            "Option<DateTimeWithTimeZone>"
        );

        // Binary type
        assert_eq!(
            ColumnType::Simple(SimpleColumnType::Bytea).to_rust_type(false),
            "Vec<u8>"
        );

        // UUID type
        assert_eq!(
            ColumnType::Simple(SimpleColumnType::Uuid).to_rust_type(false),
            "Uuid"
        );

        // JSON types
        assert_eq!(
            ColumnType::Simple(SimpleColumnType::Json).to_rust_type(false),
            "Json"
        );
        assert_eq!(
            ColumnType::Simple(SimpleColumnType::Jsonb).to_rust_type(false),
            "Json"
        );

        // Network types
        assert_eq!(
            ColumnType::Simple(SimpleColumnType::Inet).to_rust_type(false),
            "String"
        );
        assert_eq!(
            ColumnType::Simple(SimpleColumnType::Cidr).to_rust_type(false),
            "String"
        );
        assert_eq!(
            ColumnType::Simple(SimpleColumnType::Macaddr).to_rust_type(false),
            "String"
        );

        // Interval type
        assert_eq!(
            ColumnType::Simple(SimpleColumnType::Interval).to_rust_type(false),
            "String"
        );

        // XML type
        assert_eq!(
            ColumnType::Simple(SimpleColumnType::Xml).to_rust_type(false),
            "String"
        );

        // Complex types
        assert_eq!(
            ColumnType::Complex(ComplexColumnType::Numeric {
                precision: 10,
                scale: 2
            })
            .to_rust_type(false),
            "Decimal"
        );
        assert_eq!(
            ColumnType::Complex(ComplexColumnType::Char { length: 10 }).to_rust_type(false),
            "String"
        );
    }

    #[test]
    fn test_sanitize_field_name() {
        assert_eq!(sanitize_field_name("normal_name"), "normal_name");
        assert_eq!(sanitize_field_name("123name"), "_123name");
        assert_eq!(sanitize_field_name("name-with-dash"), "name_with_dash");
        assert_eq!(sanitize_field_name("name.with.dot"), "name_with_dot");
        assert_eq!(sanitize_field_name("name with space"), "name_with_space");
        assert_eq!(
            sanitize_field_name("name  with  multiple  spaces"),
            "name__with__multiple__spaces"
        );
        assert_eq!(
            sanitize_field_name(" name_with_leading_space"),
            "_name_with_leading_space"
        );
        assert_eq!(
            sanitize_field_name("name_with_trailing_space "),
            "name_with_trailing_space_"
        );
        assert_eq!(sanitize_field_name(""), "_col");
        assert_eq!(sanitize_field_name("a"), "a");
    }

    #[test]
    fn test_unique_name() {
        let mut used = std::collections::HashSet::new();
        assert_eq!(unique_name("test", &mut used), "test");
        assert_eq!(unique_name("test", &mut used), "test_1");
        assert_eq!(unique_name("test", &mut used), "test_2");
        assert_eq!(unique_name("other", &mut used), "other");
        assert_eq!(unique_name("other", &mut used), "other_1");
    }

    #[test]
    fn test_to_pascal_case() {
        // Basic snake_case conversion
        assert_eq!(to_pascal_case("hello_world"), "HelloWorld");
        assert_eq!(to_pascal_case("order_status"), "OrderStatus");

        // Kebab-case conversion
        assert_eq!(to_pascal_case("hello-world"), "HelloWorld");
        assert_eq!(to_pascal_case("info-level"), "InfoLevel");

        // Already PascalCase
        assert_eq!(to_pascal_case("HelloWorld"), "HelloWorld");

        // Single word
        assert_eq!(to_pascal_case("hello"), "Hello");
        assert_eq!(to_pascal_case("pending"), "Pending");

        // Mixed delimiters
        assert_eq!(to_pascal_case("hello_world-test"), "HelloWorldTest");

        // Uppercase input
        assert_eq!(to_pascal_case("HELLO_WORLD"), "HELLOWORLD");
        assert_eq!(to_pascal_case("ERROR_LEVEL"), "ERRORLEVEL");

        // With numbers
        assert_eq!(to_pascal_case("level_1"), "Level1");
        assert_eq!(to_pascal_case("1_critical"), "1Critical");

        // Empty string
        assert_eq!(to_pascal_case(""), "");
    }

    #[test]
    fn test_enum_variant_name() {
        // Normal cases
        assert_eq!(enum_variant_name("pending"), "Pending");
        assert_eq!(enum_variant_name("in_stock"), "InStock");
        assert_eq!(enum_variant_name("info-level"), "InfoLevel");

        // Numeric prefix - should add 'N' prefix
        // Note: to_pascal_case("1critical") returns "1critical" (starts with digit, so no uppercase)
        // then enum_variant_name adds 'N' prefix
        assert_eq!(enum_variant_name("1critical"), "N1critical");
        assert_eq!(enum_variant_name("123abc"), "N123abc");
        // With underscore separator, the part after underscore gets capitalized
        assert_eq!(enum_variant_name("1_critical"), "N1Critical");

        // Empty string - should return default
        assert_eq!(enum_variant_name(""), "Value");
    }

    #[test]
    fn test_render_enum() {
        let mut lines = Vec::new();
        render_enum(
            &mut lines,
            "order_status",
            &["pending".into(), "shipped".into()],
        );

        assert!(lines.iter().any(|l| l.contains("pub enum OrderStatus")));
        assert!(lines.iter().any(|l| l.contains("Pending")));
        assert!(lines.iter().any(|l| l.contains("Shipped")));
        assert!(lines.iter().any(|l| l.contains("DeriveActiveEnum")));
        assert!(lines.iter().any(|l| l.contains("EnumIter")));
        assert!(
            lines
                .iter()
                .any(|l| l.contains("enum_name = \"order_status\""))
        );
    }

    #[test]
    fn test_render_enum_with_numeric_prefix_value() {
        let mut lines = Vec::new();
        render_enum(
            &mut lines,
            "priority",
            &["1_high".into(), "2_medium".into(), "3_low".into()],
        );

        // Numeric prefixed values should be prefixed with 'N'
        assert!(lines.iter().any(|l| l.contains("N1High")));
        assert!(lines.iter().any(|l| l.contains("N2Medium")));
        assert!(lines.iter().any(|l| l.contains("N3Low")));
        // But the string_value should remain original
        assert!(
            lines
                .iter()
                .any(|l| l.contains("string_value = \"1_high\""))
        );
    }

    #[test]
    fn test_resolve_fk_target_no_schema() {
        // Without schema context, should return original ref_table
        let (table, columns) = resolve_fk_target("article", &["media_id".into()], &[]);
        assert_eq!(table, "article");
        assert_eq!(columns, vec!["media_id"]);
    }

    #[test]
    fn test_resolve_fk_target_no_chain() {
        use vespertide_core::{ColumnType, SimpleColumnType};
        // media table without FK chain
        let media = TableDef {
            name: "media".into(),
            columns: vec![ColumnDef {
                name: "id".into(),
                r#type: ColumnType::Simple(SimpleColumnType::Uuid),
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
            indexes: vec![],
        };

        let schema = vec![media];
        let (table, columns) = resolve_fk_target("media", &["id".into()], &schema);
        assert_eq!(table, "media");
        assert_eq!(columns, vec!["id"]);
    }

    #[test]
    fn test_resolve_fk_target_with_chain() {
        use vespertide_core::{ColumnType, SimpleColumnType};
        // media table
        let media = TableDef {
            name: "media".into(),
            columns: vec![ColumnDef {
                name: "id".into(),
                r#type: ColumnType::Simple(SimpleColumnType::Uuid),
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
            indexes: vec![],
        };

        // article table with FK to media
        let article = TableDef {
            name: "article".into(),
            columns: vec![
                ColumnDef {
                    name: "media_id".into(),
                    r#type: ColumnType::Simple(SimpleColumnType::Uuid),
                    nullable: false,
                    default: None,
                    comment: None,
                    primary_key: None,
                    unique: None,
                    index: None,
                    foreign_key: None,
                },
                ColumnDef {
                    name: "id".into(),
                    r#type: ColumnType::Simple(SimpleColumnType::BigInt),
                    nullable: false,
                    default: None,
                    comment: None,
                    primary_key: None,
                    unique: None,
                    index: None,
                    foreign_key: None,
                },
            ],
            constraints: vec![
                TableConstraint::PrimaryKey {
                    auto_increment: false,
                    columns: vec!["media_id".into(), "id".into()],
                },
                TableConstraint::ForeignKey {
                    name: None,
                    columns: vec!["media_id".into()],
                    ref_table: "media".into(),
                    ref_columns: vec!["id".into()],
                    on_delete: None,
                    on_update: None,
                },
            ],
            indexes: vec![],
        };

        let schema = vec![media, article];
        // Resolving article.media_id should follow FK chain to media.id
        let (table, columns) = resolve_fk_target("article", &["media_id".into()], &schema);
        assert_eq!(table, "media");
        assert_eq!(columns, vec!["id"]);
    }

    #[test]
    fn test_resolve_fk_target_table_not_in_schema() {
        use vespertide_core::{ColumnType, SimpleColumnType};
        let media = TableDef {
            name: "media".into(),
            columns: vec![ColumnDef {
                name: "id".into(),
                r#type: ColumnType::Simple(SimpleColumnType::Uuid),
                nullable: false,
                default: None,
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            }],
            constraints: vec![],
            indexes: vec![],
        };

        let schema = vec![media];
        // article is not in schema, should return original
        let (table, columns) = resolve_fk_target("article", &["media_id".into()], &schema);
        assert_eq!(table, "article");
        assert_eq!(columns, vec!["media_id"]);
    }

    #[test]
    fn test_resolve_fk_target_composite_fk() {
        // Composite FK should return as-is (not follow chain)
        let (table, columns) = resolve_fk_target("article", &["media_id".into(), "id".into()], &[]);
        assert_eq!(table, "article");
        assert_eq!(columns, vec!["media_id", "id"]);
    }

    #[test]
    fn test_render_entity_with_schema_fk_chain() {
        use vespertide_core::{ColumnType, SimpleColumnType};

        // media table
        let media = TableDef {
            name: "media".into(),
            columns: vec![ColumnDef {
                name: "id".into(),
                r#type: ColumnType::Simple(SimpleColumnType::Uuid),
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
            indexes: vec![],
        };

        // article table with FK to media
        let article = TableDef {
            name: "article".into(),
            columns: vec![
                ColumnDef {
                    name: "media_id".into(),
                    r#type: ColumnType::Simple(SimpleColumnType::Uuid),
                    nullable: false,
                    default: None,
                    comment: None,
                    primary_key: None,
                    unique: None,
                    index: None,
                    foreign_key: None,
                },
                ColumnDef {
                    name: "id".into(),
                    r#type: ColumnType::Simple(SimpleColumnType::BigInt),
                    nullable: false,
                    default: None,
                    comment: None,
                    primary_key: None,
                    unique: None,
                    index: None,
                    foreign_key: None,
                },
            ],
            constraints: vec![
                TableConstraint::PrimaryKey {
                    auto_increment: false,
                    columns: vec!["media_id".into(), "id".into()],
                },
                TableConstraint::ForeignKey {
                    name: None,
                    columns: vec!["media_id".into()],
                    ref_table: "media".into(),
                    ref_columns: vec!["id".into()],
                    on_delete: None,
                    on_update: None,
                },
            ],
            indexes: vec![],
        };

        // article_user table with FK to article.media_id
        let article_user = TableDef {
            name: "article_user".into(),
            columns: vec![
                ColumnDef {
                    name: "article_media_id".into(),
                    r#type: ColumnType::Simple(SimpleColumnType::Uuid),
                    nullable: false,
                    default: None,
                    comment: None,
                    primary_key: None,
                    unique: None,
                    index: None,
                    foreign_key: None,
                },
                ColumnDef {
                    name: "user_id".into(),
                    r#type: ColumnType::Simple(SimpleColumnType::Uuid),
                    nullable: false,
                    default: None,
                    comment: None,
                    primary_key: None,
                    unique: None,
                    index: None,
                    foreign_key: None,
                },
            ],
            constraints: vec![
                TableConstraint::PrimaryKey {
                    auto_increment: false,
                    columns: vec!["article_media_id".into(), "user_id".into()],
                },
                TableConstraint::ForeignKey {
                    name: None,
                    columns: vec!["article_media_id".into()],
                    ref_table: "article".into(),
                    ref_columns: vec!["media_id".into()],
                    on_delete: None,
                    on_update: None,
                },
            ],
            indexes: vec![],
        };

        let schema = vec![media, article.clone(), article_user.clone()];

        // Render article_user with schema context
        let rendered = render_entity_with_schema(&article_user, &schema);

        // Should resolve to media, not article
        assert!(rendered.contains("super::media::Entity"));
        assert!(!rendered.contains("super::article::Entity"));
        // The from should still be article_media_id, but to should be id
        assert!(rendered.contains("from = \"article_media_id\""));
        assert!(rendered.contains("to = \"id\""));
    }

    #[test]
    fn test_pluralize() {
        assert_eq!(pluralize("user"), "users");
        assert_eq!(pluralize("post"), "posts");
        assert_eq!(pluralize("category"), "categories");
        assert_eq!(pluralize("entity"), "entities");
        assert_eq!(pluralize("users"), "users"); // already plural
        assert_eq!(pluralize("day"), "days"); // 'ay' ending
        assert_eq!(pluralize("key"), "keys"); // 'ey' ending
    }

    #[test]
    fn test_reverse_relations_has_many() {
        use vespertide_core::{ColumnType, SimpleColumnType};

        // user table
        let user = TableDef {
            name: "user".into(),
            columns: vec![ColumnDef {
                name: "id".into(),
                r#type: ColumnType::Simple(SimpleColumnType::Uuid),
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
            indexes: vec![],
        };

        // post table with FK to user (not PK, so has_many)
        let post = TableDef {
            name: "post".into(),
            columns: vec![
                ColumnDef {
                    name: "id".into(),
                    r#type: ColumnType::Simple(SimpleColumnType::Uuid),
                    nullable: false,
                    default: None,
                    comment: None,
                    primary_key: None,
                    unique: None,
                    index: None,
                    foreign_key: None,
                },
                ColumnDef {
                    name: "user_id".into(),
                    r#type: ColumnType::Simple(SimpleColumnType::Uuid),
                    nullable: false,
                    default: None,
                    comment: None,
                    primary_key: None,
                    unique: None,
                    index: None,
                    foreign_key: None,
                },
            ],
            constraints: vec![
                TableConstraint::PrimaryKey {
                    auto_increment: false,
                    columns: vec!["id".into()],
                },
                TableConstraint::ForeignKey {
                    name: None,
                    columns: vec!["user_id".into()],
                    ref_table: "user".into(),
                    ref_columns: vec!["id".into()],
                    on_delete: None,
                    on_update: None,
                },
            ],
            indexes: vec![],
        };

        let schema = vec![user.clone(), post];

        // Render user with schema context - should have has_many to posts
        let rendered = render_entity_with_schema(&user, &schema);

        assert!(rendered.contains("#[sea_orm(has_many)]"));
        assert!(rendered.contains("HasMany<super::post::Entity>"));
        assert!(rendered.contains("pub posts:")); // pluralized field name
        // has_many should NOT have from/to attributes
        assert!(!rendered.contains("has_many, from"));
    }

    #[test]
    fn test_reverse_relations_has_one() {
        use vespertide_core::{ColumnType, SimpleColumnType};

        // user table
        let user = TableDef {
            name: "user".into(),
            columns: vec![ColumnDef {
                name: "id".into(),
                r#type: ColumnType::Simple(SimpleColumnType::Uuid),
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
            indexes: vec![],
        };

        // profile table with FK to user that is also the PK (one-to-one)
        let profile = TableDef {
            name: "profile".into(),
            columns: vec![
                ColumnDef {
                    name: "user_id".into(),
                    r#type: ColumnType::Simple(SimpleColumnType::Uuid),
                    nullable: false,
                    default: None,
                    comment: None,
                    primary_key: None,
                    unique: None,
                    index: None,
                    foreign_key: None,
                },
                ColumnDef {
                    name: "bio".into(),
                    r#type: ColumnType::Simple(SimpleColumnType::Text),
                    nullable: true,
                    default: None,
                    comment: None,
                    primary_key: None,
                    unique: None,
                    index: None,
                    foreign_key: None,
                },
            ],
            constraints: vec![
                TableConstraint::PrimaryKey {
                    auto_increment: false,
                    columns: vec!["user_id".into()],
                },
                TableConstraint::ForeignKey {
                    name: None,
                    columns: vec!["user_id".into()],
                    ref_table: "user".into(),
                    ref_columns: vec!["id".into()],
                    on_delete: None,
                    on_update: None,
                },
            ],
            indexes: vec![],
        };

        let schema = vec![user.clone(), profile];

        // Render user with schema context - should have has_one to profile
        let rendered = render_entity_with_schema(&user, &schema);

        assert!(rendered.contains("#[sea_orm(has_one)]"));
        assert!(rendered.contains("HasOne<super::profile::Entity>"));
        assert!(rendered.contains("pub profile:")); // singular field name
        // has_one should NOT have from/to attributes
        assert!(!rendered.contains("has_one, from"));
    }

    #[test]
    fn test_reverse_relations_unique_fk() {
        use vespertide_core::{ColumnType, SimpleColumnType};

        // user table
        let user = TableDef {
            name: "user".into(),
            columns: vec![ColumnDef {
                name: "id".into(),
                r#type: ColumnType::Simple(SimpleColumnType::Uuid),
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
            indexes: vec![],
        };

        // settings table with unique FK to user (one-to-one via UNIQUE constraint)
        let settings = TableDef {
            name: "settings".into(),
            columns: vec![
                ColumnDef {
                    name: "id".into(),
                    r#type: ColumnType::Simple(SimpleColumnType::Uuid),
                    nullable: false,
                    default: None,
                    comment: None,
                    primary_key: None,
                    unique: None,
                    index: None,
                    foreign_key: None,
                },
                ColumnDef {
                    name: "user_id".into(),
                    r#type: ColumnType::Simple(SimpleColumnType::Uuid),
                    nullable: false,
                    default: None,
                    comment: None,
                    primary_key: None,
                    unique: None,
                    index: None,
                    foreign_key: None,
                },
            ],
            constraints: vec![
                TableConstraint::PrimaryKey {
                    auto_increment: false,
                    columns: vec!["id".into()],
                },
                TableConstraint::ForeignKey {
                    name: None,
                    columns: vec!["user_id".into()],
                    ref_table: "user".into(),
                    ref_columns: vec!["id".into()],
                    on_delete: None,
                    on_update: None,
                },
                TableConstraint::Unique {
                    name: None,
                    columns: vec!["user_id".into()],
                },
            ],
            indexes: vec![],
        };

        let schema = vec![user.clone(), settings];

        // Render user with schema context - should have has_one (because of UNIQUE)
        let rendered = render_entity_with_schema(&user, &schema);

        assert!(rendered.contains("#[sea_orm(has_one)]"));
        assert!(rendered.contains("HasOne<super::settings::Entity>"));
        assert!(rendered.contains("pub settings:")); // singular field name
        // has_one should NOT have from/to attributes
        assert!(!rendered.contains("has_one, from"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use insta::{assert_snapshot, with_settings};
    use rstest::rstest;
    use vespertide_core::schema::primary_key::PrimaryKeySyntax;
    use vespertide_core::{ColumnType, SimpleColumnType};

    #[rstest]
    #[case("basic_single_pk", TableDef {
        name: "users".into(),
        columns: vec![
            ColumnDef { name: "id".into(), r#type: ColumnType::Simple(SimpleColumnType::Integer), nullable: false, default: None, comment: None, primary_key: None, unique: None, index: None, foreign_key: None },
            ColumnDef { name: "display_name".into(), r#type: ColumnType::Simple(SimpleColumnType::Text), nullable: true, default: None, comment: None, primary_key: None, unique: None, index: None, foreign_key: None },
        ],
        constraints: vec![TableConstraint::PrimaryKey { auto_increment: false, columns: vec!["id".into()] }],
        indexes: vec![],
    })]
    #[case("composite_pk", TableDef {
        name: "accounts".into(),
        columns: vec![
            ColumnDef { name: "id".into(), r#type: ColumnType::Simple(SimpleColumnType::Integer), nullable: false, default: None, comment: None, primary_key: None, unique: None, index: None, foreign_key: None },
            ColumnDef { name: "tenant_id".into(), r#type: ColumnType::Simple(SimpleColumnType::BigInt), nullable: false, default: None, comment: None, primary_key: None, unique: None, index: None, foreign_key: None },
        ],
        constraints: vec![TableConstraint::PrimaryKey { auto_increment: false, columns: vec!["id".into(), "tenant_id".into()] }],
        indexes: vec![],
    })]
    #[case("fk_single", TableDef {
        name: "posts".into(),
        columns: vec![
            ColumnDef { name: "id".into(), r#type: ColumnType::Simple(SimpleColumnType::Integer), nullable: false, default: None, comment: None, primary_key: None, unique: None, index: None, foreign_key: None },
            ColumnDef { name: "user_id".into(), r#type: ColumnType::Simple(SimpleColumnType::Integer), nullable: false, default: None, comment: None, primary_key: None, unique: None, index: None, foreign_key: None },
            ColumnDef { name: "title".into(), r#type: ColumnType::Simple(SimpleColumnType::Text), nullable: true, default: None, comment: None, primary_key: None, unique: None, index: None, foreign_key: None },
        ],
        constraints: vec![
            TableConstraint::PrimaryKey { auto_increment: false, columns: vec!["id".into()] },
            TableConstraint::ForeignKey {
                name: None,
                columns: vec!["user_id".into()],
                ref_table: "users".into(),
                ref_columns: vec!["id".into()],
                on_delete: None,
                on_update: None,
            },
        ],
        indexes: vec![],
    })]
    #[case("fk_composite", TableDef {
        name: "invoices".into(),
        columns: vec![
            ColumnDef { name: "id".into(), r#type: ColumnType::Simple(SimpleColumnType::Integer), nullable: false, default: None, comment: None, primary_key: None, unique: None, index: None, foreign_key: None },
            ColumnDef { name: "customer_id".into(), r#type: ColumnType::Simple(SimpleColumnType::Integer), nullable: false, default: None, comment: None, primary_key: None, unique: None, index: None, foreign_key: None },
            ColumnDef { name: "customer_tenant_id".into(), r#type: ColumnType::Simple(SimpleColumnType::Integer), nullable: false, default: None, comment: None, primary_key: None, unique: None, index: None, foreign_key: None },
        ],
        constraints: vec![
            TableConstraint::PrimaryKey { auto_increment: false, columns: vec!["id".into()] },
            TableConstraint::ForeignKey {
                name: None,
                columns: vec!["customer_id".into(), "customer_tenant_id".into()],
                ref_table: "customers".into(),
                ref_columns: vec!["id".into(), "tenant_id".into()],
                on_delete: None,
                on_update: None,
            },
        ],
        indexes: vec![],
    })]
    #[case("inline_pk", TableDef {
        name: "users".into(),
        columns: vec![
            ColumnDef { name: "id".into(), r#type: ColumnType::Simple(SimpleColumnType::Uuid), nullable: false, default: Some("gen_random_uuid()".into()), comment: None, primary_key: Some(PrimaryKeySyntax::Bool(true)), unique: None, index: None, foreign_key: None },
            ColumnDef { name: "email".into(), r#type: ColumnType::Simple(SimpleColumnType::Text), nullable: false, default: None, comment: None, primary_key: None, unique: Some(vespertide_core::StrOrBoolOrArray::Bool(true)), index: None, foreign_key: None },
        ],
        constraints: vec![],
        indexes: vec![],
    })]
    #[case("pk_and_fk_together", {
        use vespertide_core::schema::foreign_key::{ForeignKeyDef, ForeignKeySyntax};
        use vespertide_core::schema::reference::ReferenceAction;
        let mut table = TableDef {
            name: "article_user".into(),
            columns: vec![
                ColumnDef {
                    name: "article_id".into(),
                    r#type: ColumnType::Simple(SimpleColumnType::Uuid),
                    nullable: false,
                    default: None,
                    comment: None,
                    primary_key: Some(PrimaryKeySyntax::Bool(true)),
                    unique: None,
                    index: Some(vespertide_core::StrOrBoolOrArray::Bool(true)),
                    foreign_key: Some(ForeignKeySyntax::Object(ForeignKeyDef {
                        ref_table: "article".into(),
                        ref_columns: vec!["id".into()],
                        on_delete: Some(ReferenceAction::Cascade),
                        on_update: None,
                    })),
                },
                ColumnDef {
                    name: "user_id".into(),
                    r#type: ColumnType::Simple(SimpleColumnType::Uuid),
                    nullable: false,
                    default: None,
                    comment: None,
                    primary_key: Some(PrimaryKeySyntax::Bool(true)),
                    unique: None,
                    index: Some(vespertide_core::StrOrBoolOrArray::Bool(true)),
                    foreign_key: Some(ForeignKeySyntax::Object(ForeignKeyDef {
                        ref_table: "user".into(),
                        ref_columns: vec!["id".into()],
                        on_delete: Some(ReferenceAction::Cascade),
                        on_update: None,
                    })),
                },
                ColumnDef {
                    name: "author_order".into(),
                    r#type: ColumnType::Simple(SimpleColumnType::Integer),
                    nullable: false,
                    default: Some("1".into()),
                    comment: None,
                    primary_key: None,
                    unique: None,
                    index: None,
                    foreign_key: None,
                },
                ColumnDef {
                    name: "role".into(),
                    r#type: ColumnType::Complex(vespertide_core::ComplexColumnType::Varchar { length: 20 }),
                    nullable: false,
                    default: Some("'contributor'".into()),
                    comment: None,
                    primary_key: None,
                    unique: None,
                    index: None,
                    foreign_key: None,
                },
                ColumnDef {
                    name: "is_lead".into(),
                    r#type: ColumnType::Simple(SimpleColumnType::Boolean),
                    nullable: false,
                    default: Some("false".into()),
                    comment: None,
                    primary_key: None,
                    unique: None,
                    index: None,
                    foreign_key: None,
                },
                ColumnDef {
                    name: "created_at".into(),
                    r#type: ColumnType::Simple(SimpleColumnType::Timestamptz),
                    nullable: false,
                    default: Some("now()".into()),
                    comment: None,
                    primary_key: None,
                    unique: None,
                    index: None,
                    foreign_key: None,
                },
            ],
            constraints: vec![],
            indexes: vec![],
        };
        // Normalize to convert inline constraints to table-level
        table = table.normalize().unwrap();
        table
    })]
    #[case("enum_type", TableDef {
        name: "orders".into(),
        columns: vec![
            ColumnDef { name: "id".into(), r#type: ColumnType::Simple(SimpleColumnType::Integer), nullable: false, default: None, comment: None, primary_key: Some(PrimaryKeySyntax::Bool(true)), unique: None, index: None, foreign_key: None },
            ColumnDef {
                name: "status".into(),
                r#type: ColumnType::Complex(ComplexColumnType::Enum {
                    name: "order_status".into(),
                    values: vec!["pending".into(), "shipped".into(), "delivered".into()]
                }),
                nullable: false,
                default: None,
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            },
        ],
        constraints: vec![],
        indexes: vec![],
    })]
    #[case("enum_nullable", TableDef {
        name: "tasks".into(),
        columns: vec![
            ColumnDef { name: "id".into(), r#type: ColumnType::Simple(SimpleColumnType::Integer), nullable: false, default: None, comment: None, primary_key: Some(PrimaryKeySyntax::Bool(true)), unique: None, index: None, foreign_key: None },
            ColumnDef {
                name: "priority".into(),
                r#type: ColumnType::Complex(ComplexColumnType::Enum {
                    name: "task_priority".into(),
                    values: vec!["low".into(), "medium".into(), "high".into(), "critical".into()]
                }),
                nullable: true,
                default: None,
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            },
        ],
        constraints: vec![],
        indexes: vec![],
    })]
    #[case("enum_multiple_columns", TableDef {
        name: "products".into(),
        columns: vec![
            ColumnDef { name: "id".into(), r#type: ColumnType::Simple(SimpleColumnType::Integer), nullable: false, default: None, comment: None, primary_key: Some(PrimaryKeySyntax::Bool(true)), unique: None, index: None, foreign_key: None },
            ColumnDef {
                name: "category".into(),
                r#type: ColumnType::Complex(ComplexColumnType::Enum {
                    name: "product_category".into(),
                    values: vec!["electronics".into(), "clothing".into(), "food".into()]
                }),
                nullable: false,
                default: None,
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            },
            ColumnDef {
                name: "availability".into(),
                r#type: ColumnType::Complex(ComplexColumnType::Enum {
                    name: "availability_status".into(),
                    values: vec!["in_stock".into(), "out_of_stock".into(), "pre_order".into()]
                }),
                nullable: false,
                default: None,
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            },
        ],
        constraints: vec![],
        indexes: vec![],
    })]
    #[case("enum_shared", TableDef {
        name: "documents".into(),
        columns: vec![
            ColumnDef { name: "id".into(), r#type: ColumnType::Simple(SimpleColumnType::Integer), nullable: false, default: None, comment: None, primary_key: Some(PrimaryKeySyntax::Bool(true)), unique: None, index: None, foreign_key: None },
            ColumnDef {
                name: "status".into(),
                r#type: ColumnType::Complex(ComplexColumnType::Enum {
                    name: "doc_status".into(),
                    values: vec!["draft".into(), "published".into(), "archived".into()]
                }),
                nullable: false,
                default: None,
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            },
            ColumnDef {
                name: "review_status".into(),
                r#type: ColumnType::Complex(ComplexColumnType::Enum {
                    name: "doc_status".into(),
                    values: vec!["draft".into(), "published".into(), "archived".into()]
                }),
                nullable: true,
                default: None,
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            },
        ],
        constraints: vec![],
        indexes: vec![],
    })]
    #[case("enum_special_values", TableDef {
        name: "events".into(),
        columns: vec![
            ColumnDef { name: "id".into(), r#type: ColumnType::Simple(SimpleColumnType::Integer), nullable: false, default: None, comment: None, primary_key: Some(PrimaryKeySyntax::Bool(true)), unique: None, index: None, foreign_key: None },
            ColumnDef {
                name: "severity".into(),
                r#type: ColumnType::Complex(ComplexColumnType::Enum {
                    name: "event_severity".into(),
                    values: vec!["info-level".into(), "warning_level".into(), "ERROR_LEVEL".into(), "1critical".into()]
                }),
                nullable: false,
                default: None,
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            },
        ],
        constraints: vec![],
        indexes: vec![],
    })]
    #[case("unique_and_indexed", TableDef {
        name: "users".into(),
        columns: vec![
            ColumnDef { name: "id".into(), r#type: ColumnType::Simple(SimpleColumnType::Integer), nullable: false, default: None, comment: None, primary_key: Some(PrimaryKeySyntax::Bool(true)), unique: None, index: None, foreign_key: None },
            ColumnDef { name: "email".into(), r#type: ColumnType::Simple(SimpleColumnType::Text), nullable: false, default: None, comment: None, primary_key: None, unique: None, index: None, foreign_key: None },
            ColumnDef { name: "username".into(), r#type: ColumnType::Simple(SimpleColumnType::Text), nullable: false, default: None, comment: None, primary_key: None, unique: None, index: None, foreign_key: None },
            ColumnDef { name: "department".into(), r#type: ColumnType::Simple(SimpleColumnType::Text), nullable: true, default: None, comment: None, primary_key: None, unique: None, index: None, foreign_key: None },
            ColumnDef { name: "status".into(), r#type: ColumnType::Simple(SimpleColumnType::Text), nullable: false, default: Some("'active'".into()), comment: None, primary_key: None, unique: None, index: None, foreign_key: None },
        ],
        constraints: vec![
            TableConstraint::Unique { name: None, columns: vec!["email".into()] },
            TableConstraint::Unique { name: Some("uq_username".into()), columns: vec!["username".into()] },
        ],
        indexes: vec![
            IndexDef { name: "idx_department".into(), columns: vec!["department".into()], unique: false },
        ],
    })]
    #[case("enum_with_default", TableDef {
        name: "tasks".into(),
        columns: vec![
            ColumnDef { name: "id".into(), r#type: ColumnType::Simple(SimpleColumnType::Integer), nullable: false, default: None, comment: None, primary_key: Some(PrimaryKeySyntax::Bool(true)), unique: None, index: None, foreign_key: None },
            ColumnDef {
                name: "status".into(),
                r#type: ColumnType::Complex(ComplexColumnType::Enum {
                    name: "task_status".into(),
                    values: vec!["pending".into(), "in_progress".into(), "completed".into()]
                }),
                nullable: false,
                default: Some("'pending'".into()),
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            },
            ColumnDef { name: "priority".into(), r#type: ColumnType::Simple(SimpleColumnType::Integer), nullable: false, default: Some("0".into()), comment: None, primary_key: None, unique: None, index: None, foreign_key: None },
            ColumnDef { name: "is_archived".into(), r#type: ColumnType::Simple(SimpleColumnType::Boolean), nullable: false, default: Some("false".into()), comment: None, primary_key: None, unique: None, index: None, foreign_key: None },
        ],
        constraints: vec![],
        indexes: vec![],
    })]
    fn render_entity_snapshots(#[case] name: &str, #[case] table: TableDef) {
        let rendered = render_entity(&table);
        with_settings!({ snapshot_suffix => format!("params_{}", name) }, {
            assert_snapshot!(rendered);
        });
    }
}
