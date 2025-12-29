use std::collections::HashSet;

use crate::orm::OrmExporter;
use vespertide_core::{
    ColumnDef, ColumnType, ComplexColumnType, EnumValues, NumValue, StringOrBool, TableConstraint,
    TableDef,
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
    let relation_fields = relation_field_defs_with_schema(table, schema);

    // Build sets of columns with single-column unique constraints and indexes
    let unique_columns = single_column_unique_set(&table.constraints);
    let indexed_columns = single_column_index_set(&table.constraints);

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
                render_enum(&mut lines, &table.name, name, values);
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
    render_indexes(&mut lines, &table.constraints);

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

/// Build a set of column names that have single-column indexes from constraints.
fn single_column_index_set(constraints: &[TableConstraint]) -> HashSet<String> {
    let mut indexed_cols = HashSet::new();
    for constraint in constraints {
        if let TableConstraint::Index { columns, .. } = constraint
            && columns.len() == 1
        {
            indexed_cols.insert(columns[0].clone());
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
fn format_default_value(value: &StringOrBool, column_type: &ColumnType) -> String {
    // Handle boolean values directly
    if let StringOrBool::Bool(b) = value {
        return format!("default_value = {}", b);
    }

    // For string values, process as before
    let value_str = value.to_sql();
    let trimmed = value_str.trim();

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
        // Enum type: use the actual database value (string or number), not Rust enum variant
        ColumnType::Complex(ComplexColumnType::Enum { values, .. }) => {
            match values {
                EnumValues::String(_) => {
                    // String enum: use the string value as-is with quotes
                    format!("default_value = \"{}\"", cleaned)
                }
                EnumValues::Integer(int_values) => {
                    // Integer enum: can be either a number or a variant name
                    // Try to parse as number first
                    if let Ok(num) = cleaned.parse::<i32>() {
                        // Already a number, use as-is
                        format!("default_value = {}", num)
                    } else {
                        // It's a variant name, find the corresponding numeric value
                        let numeric_value = int_values
                            .iter()
                            .find(|v| v.name.eq_ignore_ascii_case(cleaned))
                            .map(|v| v.value)
                            .unwrap_or(0); // Default to 0 if not found
                        format!("default_value = {}", numeric_value)
                    }
                }
            }
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

/// Extract FK info from a constraint as a tuple.
fn as_fk(constraint: &TableConstraint) -> Option<(&[String], &str, &[String])> {
    match constraint {
        TableConstraint::ForeignKey {
            columns,
            ref_table,
            ref_columns,
            ..
        } => Some((
            columns.as_slice(),
            ref_table.as_str(),
            ref_columns.as_slice(),
        )),
        _ => None,
    }
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

    // Check if the referenced column has a FK constraint and follow the chain
    for constraint in &target_table.constraints {
        let fk_match =
            as_fk(constraint).filter(|(cols, _, _)| cols.len() == 1 && cols[0] == *ref_col);
        if let Some((_, next_table, next_cols)) = fk_match {
            return resolve_fk_target(next_table, next_cols, schema);
        }
    }

    // No further FK chain, return current target
    (ref_table, ref_columns.to_vec())
}

fn relation_field_defs_with_schema(table: &TableDef, schema: &[TableDef]) -> Vec<String> {
    let mut out = Vec::new();
    let mut used = HashSet::new();

    // Group FKs by their target table to detect duplicates
    let mut fk_by_table: std::collections::HashMap<String, Vec<&TableConstraint>> =
        std::collections::HashMap::new();
    for constraint in &table.constraints {
        if let TableConstraint::ForeignKey {
            ref_table,
            ref_columns,
            ..
        } = constraint
        {
            let (resolved_table, _) = resolve_fk_target(ref_table, ref_columns, schema);
            fk_by_table
                .entry(resolved_table.to_string())
                .or_default()
                .push(constraint);
        }
    }

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

            let from = fk_attr_value(columns);
            let to = fk_attr_value(&resolved_columns);

            // Check if there are multiple FKs to the same target table
            let fks_to_this_table = fk_by_table
                .get(resolved_table)
                .map(|fks| fks.len())
                .unwrap_or(0);

            // Smart field name inference from FK column names
            // Try to use the FK column name (without _id suffix) as the field name
            // If that doesn't work (conflicts), fall back to table name
            let field_base = if columns.len() == 1 {
                // For single-column FKs, try to infer from column name
                infer_field_name_from_fk_column(&columns[0], resolved_table, &to)
            } else {
                // For composite FKs, use table name
                sanitize_field_name(resolved_table)
            };

            let field_name = unique_name(&field_base, &mut used);

            // Generate relation_enum name if there are multiple FKs to the same table
            let attr = if fks_to_this_table > 1 {
                // Generate a unique relation enum name from the FK column(s)
                let relation_enum_name = generate_relation_enum_name(columns);
                format!(
                    "    #[sea_orm(belongs_to, relation_enum = \"{relation_enum_name}\", from = \"{from}\", to = \"{to}\")]"
                )
            } else {
                format!("    #[sea_orm(belongs_to, from = \"{from}\", to = \"{to}\")]")
            };

            out.push(attr);
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

/// Generate a relation enum name from foreign key column names.
/// For "creator_user_id", generates "CreatorUser".
/// For composite FKs like ["org_id", "user_id"], generates "OrgUser".
fn generate_relation_enum_name(columns: &[String]) -> String {
    // Take the first column and remove common FK suffixes like "_id"
    let first_col = &columns[0];
    let without_id = if first_col.ends_with("_id") {
        &first_col[..first_col.len() - 3]
    } else {
        first_col
    };

    to_pascal_case(without_id)
}

/// Infer a field name from a single FK column.
/// For "creator_user_id" with to="id", tries "creator_user" first.
/// If that ends with the table name, use the full column name (without the to suffix).
/// Otherwise, fall back to the table name.
///
/// Examples:
/// - FK column: "creator_user_id", table: "user", to: "id" -> "creator_user"
/// - FK column: "creator_user_idx", table: "user", to: "idx" -> "creator_user"
/// - FK column: "user_id", table: "user", to: "id" -> "user" (falls back to table name)
/// - FK column: "org_id", table: "user", to: "id" -> "org"
fn infer_field_name_from_fk_column(fk_column: &str, table_name: &str, to: &str) -> String {
    let table_lower = table_name.to_lowercase();

    // Remove the "to" suffix from FK column (e.g., "user_id" for to="id", "user_idx" for to="idx")
    let without_suffix = if fk_column.ends_with(&format!("_{to}")) {
        let suffix_len = to.len() + 1; // +1 for the underscore
        &fk_column[..fk_column.len() - suffix_len]
    } else {
        fk_column
    };

    let sanitized = sanitize_field_name(without_suffix);
    let sanitized_lower = sanitized.to_lowercase();

    // If the sanitized name is exactly the table name (e.g., "user_id" -> "user" for table "user"),
    // we need to fall back to the table name for proper disambiguation
    if sanitized_lower == table_lower {
        sanitize_field_name(table_name)
    }
    // If the sanitized name ends with (but is not equal to) the table name, use it as-is
    // This handles cases like "creator_user" for table "user"
    else if sanitized_lower.ends_with(&table_lower) {
        sanitized
    } else {
        // Otherwise, use the inferred name from the column
        sanitized
    }
}

/// Generate reverse relation fields (has_one/has_many) for tables that reference this table.
fn reverse_relation_field_defs(
    table: &TableDef,
    schema: &[TableDef],
    used: &mut HashSet<String>,
) -> Vec<String> {
    let mut out = Vec::new();

    // First, count how many FKs from each table reference this table
    let mut fk_count_per_table: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();
    for other_table in schema {
        if other_table.name == table.name {
            continue;
        }
        for constraint in &other_table.constraints {
            if let TableConstraint::ForeignKey { ref_table, .. } = constraint
                && ref_table == &table.name
            {
                *fk_count_per_table
                    .entry(other_table.name.clone())
                    .or_insert(0) += 1;
            }
        }
    }

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

                    // Check if this table has multiple FKs to current table
                    let has_multiple_fks = fk_count_per_table
                        .get(&other_table.name)
                        .map(|count| *count > 1)
                        .unwrap_or(false);

                    // Generate base field name
                    let base = if has_multiple_fks {
                        // Use relation_enum name to infer field name for multiple FKs
                        let relation_enum_name = generate_relation_enum_name(columns);
                        let lowercase_enum = to_snake_case(&relation_enum_name);
                        if is_one_to_one {
                            lowercase_enum
                        } else {
                            format!(
                                "{}_{}",
                                lowercase_enum,
                                pluralize(&sanitize_field_name(&other_table.name))
                            )
                        }
                    } else {
                        // Default naming for single FK
                        if is_one_to_one {
                            sanitize_field_name(&other_table.name)
                        } else {
                            pluralize(&sanitize_field_name(&other_table.name))
                        }
                    };
                    let field_name = unique_name(&base, used);

                    // Generate relation_enum name if there are multiple FKs to this table
                    let attr = if has_multiple_fks {
                        let relation_enum_name = generate_relation_enum_name(columns);
                        format!(
                            "    #[sea_orm({relation_type}, relation_enum = \"{relation_enum_name}\", via_rel = \"{relation_enum_name}\")]"
                        )
                    } else {
                        format!("    #[sea_orm({relation_type})]")
                    };

                    out.push(attr);
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

fn render_indexes(lines: &mut Vec<String>, constraints: &[TableConstraint]) {
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

    if index_constraints.is_empty() {
        return;
    }
    lines.push(String::new());
    lines.push("// Index definitions (SeaORM uses Statement builders externally)".into());
    for (name, columns) in index_constraints {
        let cols = columns.join(", ");
        let idx_name = name.clone().unwrap_or_else(|| "(unnamed)".to_string());
        lines.push(format!("// {} on [{}]", idx_name, cols));
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

fn render_enum(lines: &mut Vec<String>, table_name: &str, name: &str, values: &EnumValues) {
    let enum_name = to_pascal_case(name);
    // Construct the full enum name with table prefix for database
    let db_enum_name = format!("{}_{}", table_name, name);

    lines.push(
        "#[derive(Debug, Clone, PartialEq, Eq, EnumIter, DeriveActiveEnum, Serialize, Deserialize)]"
            .into(),
    );

    match values {
        EnumValues::Integer(_) => {
            // Integer enum: #[sea_orm(rs_type = "i32", db_type = "Integer")]
            lines.push("#[sea_orm(rs_type = \"i32\", db_type = \"Integer\")]".into());
        }
        EnumValues::String(_) => {
            // String enum: #[sea_orm(rs_type = "String", db_type = "Enum", enum_name = "...")]
            lines.push(format!(
                "#[sea_orm(rs_type = \"String\", db_type = \"Enum\", enum_name = \"{}\")]",
                db_enum_name
            ));
        }
    }

    lines.push(format!("pub enum {} {{", enum_name));

    match values {
        EnumValues::String(string_values) => {
            for s in string_values {
                let variant_name = enum_variant_name(s);
                lines.push(format!("    #[sea_orm(string_value = \"{}\")]", s));
                lines.push(format!("    {},", variant_name));
            }
        }
        EnumValues::Integer(int_values) => {
            for NumValue {
                name: var_name,
                value: num,
            } in int_values
            {
                let variant_name = enum_variant_name(var_name);
                lines.push(format!("    {} = {},", variant_name, num));
            }
        }
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
        let is_separator = c == '_' || c == '-';
        if is_separator {
            capitalize = true;
            continue;
        }
        let ch = if capitalize {
            c.to_ascii_uppercase()
        } else {
            c
        };
        capitalize = false;
        result.push(ch);
    }
    result
}

/// Convert PascalCase to snake_case.
/// For "CreatorUser", generates "creator_user".
fn to_snake_case(s: &str) -> String {
    let mut result = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && c.is_ascii_uppercase() {
            result.push('_');
        }
        result.push(c.to_ascii_lowercase());
    }
    result
}

#[cfg(test)]
mod helper_tests {
    use super::*;
    use rstest::rstest;
    use vespertide_core::{ColumnType, ComplexColumnType, SimpleColumnType};

    #[test]
    fn test_render_indexes() {
        let mut lines = Vec::new();
        let constraints = vec![
            TableConstraint::Index {
                name: Some("idx_users_email".into()),
                columns: vec!["email".into()],
            },
            TableConstraint::Index {
                name: Some("idx_users_name_email".into()),
                columns: vec!["name".into(), "email".into()],
            },
        ];
        render_indexes(&mut lines, &constraints);
        assert!(!lines.is_empty());
        assert!(lines.iter().any(|l| l.contains("idx_users_email")));
        assert!(lines.iter().any(|l| l.contains("idx_users_name_email")));
    }

    #[test]
    fn test_render_indexes_empty() {
        let mut lines = Vec::new();
        render_indexes(&mut lines, &[]);
        assert_eq!(lines.len(), 0);
    }

    #[rstest]
    #[case(ColumnType::Simple(SimpleColumnType::SmallInt), false, "i16")]
    #[case(ColumnType::Simple(SimpleColumnType::SmallInt), true, "Option<i16>")]
    #[case(ColumnType::Simple(SimpleColumnType::Integer), false, "i32")]
    #[case(ColumnType::Simple(SimpleColumnType::Integer), true, "Option<i32>")]
    #[case(ColumnType::Simple(SimpleColumnType::BigInt), false, "i64")]
    #[case(ColumnType::Simple(SimpleColumnType::BigInt), true, "Option<i64>")]
    #[case(ColumnType::Simple(SimpleColumnType::Real), false, "f32")]
    #[case(ColumnType::Simple(SimpleColumnType::DoublePrecision), false, "f64")]
    #[case(ColumnType::Simple(SimpleColumnType::Text), false, "String")]
    #[case(ColumnType::Simple(SimpleColumnType::Text), true, "Option<String>")]
    #[case(ColumnType::Simple(SimpleColumnType::Boolean), false, "bool")]
    #[case(ColumnType::Simple(SimpleColumnType::Boolean), true, "Option<bool>")]
    #[case(ColumnType::Simple(SimpleColumnType::Date), false, "Date")]
    #[case(ColumnType::Simple(SimpleColumnType::Time), false, "Time")]
    #[case(ColumnType::Simple(SimpleColumnType::Timestamp), false, "DateTime")]
    #[case(
        ColumnType::Simple(SimpleColumnType::Timestamp),
        true,
        "Option<DateTime>"
    )]
    #[case(
        ColumnType::Simple(SimpleColumnType::Timestamptz),
        false,
        "DateTimeWithTimeZone"
    )]
    #[case(
        ColumnType::Simple(SimpleColumnType::Timestamptz),
        true,
        "Option<DateTimeWithTimeZone>"
    )]
    #[case(ColumnType::Simple(SimpleColumnType::Bytea), false, "Vec<u8>")]
    #[case(ColumnType::Simple(SimpleColumnType::Uuid), false, "Uuid")]
    #[case(ColumnType::Simple(SimpleColumnType::Json), false, "Json")]
    #[case(ColumnType::Simple(SimpleColumnType::Jsonb), false, "Json")]
    #[case(ColumnType::Simple(SimpleColumnType::Inet), false, "String")]
    #[case(ColumnType::Simple(SimpleColumnType::Cidr), false, "String")]
    #[case(ColumnType::Simple(SimpleColumnType::Macaddr), false, "String")]
    #[case(ColumnType::Simple(SimpleColumnType::Interval), false, "String")]
    #[case(ColumnType::Simple(SimpleColumnType::Xml), false, "String")]
    #[case(ColumnType::Complex(ComplexColumnType::Numeric { precision: 10, scale: 2 }), false, "Decimal")]
    #[case(ColumnType::Complex(ComplexColumnType::Char { length: 10 }), false, "String")]
    fn test_rust_type(
        #[case] col_type: ColumnType,
        #[case] nullable: bool,
        #[case] expected: &str,
    ) {
        assert_eq!(col_type.to_rust_type(nullable), expected);
    }

    #[rstest]
    #[case("normal_name", "normal_name")]
    #[case("123name", "_123name")]
    #[case("name-with-dash", "name_with_dash")]
    #[case("name.with.dot", "name_with_dot")]
    #[case("name with space", "name_with_space")]
    #[case("name  with  multiple  spaces", "name__with__multiple__spaces")]
    #[case(" name_with_leading_space", "_name_with_leading_space")]
    #[case("name_with_trailing_space ", "name_with_trailing_space_")]
    #[case("", "_col")]
    #[case("a", "a")]
    fn test_sanitize_field_name(#[case] input: &str, #[case] expected: &str) {
        assert_eq!(sanitize_field_name(input), expected);
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

    #[rstest]
    #[case(vec!["creator_user_id".into()], "CreatorUser")]
    #[case(vec!["used_by_user_id".into()], "UsedByUser")]
    #[case(vec!["user_id".into()], "User")]
    #[case(vec!["org_id".into()], "Org")]
    #[case(vec!["org_id".into(), "user_id".into()], "Org")]
    #[case(vec!["author_id".into()], "Author")]
    // FK column WITHOUT _id suffix (coverage for line 428)
    #[case(vec!["creator_user".into()], "CreatorUser")]
    #[case(vec!["user".into()], "User")]
    fn test_generate_relation_enum_name(#[case] columns: Vec<String>, #[case] expected: &str) {
        assert_eq!(generate_relation_enum_name(&columns), expected);
    }

    #[rstest]
    // FK column ends with table name -> use the FK column name
    #[case("creator_user_id", "user", "id", "creator_user")]
    #[case("used_by_user_id", "user", "id", "used_by_user")]
    #[case("author_user_id", "user", "id", "author_user")]
    // FK column is same as table -> fall back to table name
    #[case("user_id", "user", "id", "user")]
    #[case("org_id", "org", "id", "org")]
    #[case("post_id", "post", "id", "post")]
    // FK column doesn't end with table name -> use FK column name
    #[case("author_id", "user", "id", "author")]
    #[case("owner_id", "user", "id", "owner")]
    // FK column WITHOUT _id suffix (coverage for line 450)
    #[case("creator_user", "user", "id", "creator_user")]
    #[case("user", "user", "id", "user")]
    // FK column exactly matches table name with _id (coverage for line 464)
    #[case("customer_id", "customer", "id", "customer")]
    #[case("product_id", "product", "id", "product")]
    // Test with different "to" suffixes (e.g., _idx instead of _id)
    #[case("creator_user_idx", "user", "idx", "creator_user")]
    #[case("user_idx", "user", "idx", "user")]
    #[case("author_pk", "user", "pk", "author")]
    fn test_infer_field_name_from_fk_column(
        #[case] fk_column: &str,
        #[case] table_name: &str,
        #[case] to: &str,
        #[case] expected: &str,
    ) {
        assert_eq!(
            infer_field_name_from_fk_column(fk_column, table_name, to),
            expected
        );
    }

    #[rstest]
    #[case("hello_world", "HelloWorld")]
    #[case("order_status", "OrderStatus")]
    #[case("hello-world", "HelloWorld")]
    #[case("info-level", "InfoLevel")]
    #[case("HelloWorld", "HelloWorld")]
    #[case("hello", "Hello")]
    #[case("pending", "Pending")]
    #[case("hello_world-test", "HelloWorldTest")]
    #[case("HELLO_WORLD", "HELLOWORLD")]
    #[case("ERROR_LEVEL", "ERRORLEVEL")]
    #[case("level_1", "Level1")]
    #[case("1_critical", "1Critical")]
    #[case("", "")]
    fn test_to_pascal_case(#[case] input: &str, #[case] expected: &str) {
        assert_eq!(to_pascal_case(input), expected);
    }

    #[rstest]
    #[case("CreatorUser", "creator_user")]
    #[case("UsedByUser", "used_by_user")]
    #[case("PreferredUser", "preferred_user")]
    #[case("BackupUser", "backup_user")]
    #[case("User", "user")]
    #[case("ID", "i_d")]
    fn test_to_snake_case(#[case] input: &str, #[case] expected: &str) {
        assert_eq!(to_snake_case(input), expected);
    }

    #[rstest]
    #[case("pending", "Pending")]
    #[case("in_stock", "InStock")]
    #[case("info-level", "InfoLevel")]
    #[case("1critical", "N1critical")]
    #[case("123abc", "N123abc")]
    #[case("1_critical", "N1Critical")]
    #[case("", "Value")]
    fn test_enum_variant_name(#[case] input: &str, #[case] expected: &str) {
        assert_eq!(enum_variant_name(input), expected);
    }

    fn string_enum_order_status() -> (&'static str, EnumValues) {
        (
            "order_status",
            EnumValues::String(vec!["pending".into(), "shipped".into(), "delivered".into()]),
        )
    }

    fn string_enum_numeric_prefix() -> (&'static str, EnumValues) {
        (
            "priority",
            EnumValues::String(vec!["1_high".into(), "2_medium".into(), "3_low".into()]),
        )
    }

    fn integer_enum_color() -> (&'static str, EnumValues) {
        (
            "color",
            EnumValues::Integer(vec![
                NumValue {
                    name: "Black".into(),
                    value: 0,
                },
                NumValue {
                    name: "White".into(),
                    value: 1,
                },
                NumValue {
                    name: "Red".into(),
                    value: 2,
                },
            ]),
        )
    }

    fn integer_enum_status() -> (&'static str, EnumValues) {
        (
            "task_status",
            EnumValues::Integer(vec![
                NumValue {
                    name: "Pending".into(),
                    value: 0,
                },
                NumValue {
                    name: "InProgress".into(),
                    value: 1,
                },
                NumValue {
                    name: "Completed".into(),
                    value: 100,
                },
            ]),
        )
    }

    #[rstest]
    #[case::string_enum("string_order_status", "orders", string_enum_order_status())]
    #[case::string_numeric_prefix("string_numeric_prefix", "tasks", string_enum_numeric_prefix())]
    #[case::integer_color("integer_color", "products", integer_enum_color())]
    #[case::integer_status("integer_status", "tasks", integer_enum_status())]
    fn test_render_enum_snapshots(
        #[case] name: &str,
        #[case] table_name: &str,
        #[case] input: (&str, EnumValues),
    ) {
        use insta::with_settings;

        let (enum_name, values) = input;
        let mut lines = Vec::new();
        render_enum(&mut lines, table_name, enum_name, &values);
        let result = lines.join("\n");

        with_settings!({ snapshot_suffix => name }, {
            insta::assert_snapshot!(result);
        });
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
    fn test_resolve_fk_target_deep_chain() {
        use vespertide_core::{ColumnType, SimpleColumnType};

        // 3-level chain: level_c.b_id -> level_b.a_id -> level_a.id
        // level_a (root)
        let level_a = TableDef {
            name: "level_a".into(),
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
        };

        // level_b with FK to level_a
        let level_b = TableDef {
            name: "level_b".into(),
            columns: vec![ColumnDef {
                name: "a_id".into(),
                r#type: ColumnType::Simple(SimpleColumnType::Uuid),
                nullable: false,
                default: None,
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            }],
            constraints: vec![
                TableConstraint::PrimaryKey {
                    auto_increment: false,
                    columns: vec!["a_id".into()],
                },
                TableConstraint::ForeignKey {
                    name: None,
                    columns: vec!["a_id".into()],
                    ref_table: "level_a".into(),
                    ref_columns: vec!["id".into()],
                    on_delete: None,
                    on_update: None,
                },
            ],
        };

        // level_c with FK to level_b
        let level_c = TableDef {
            name: "level_c".into(),
            columns: vec![ColumnDef {
                name: "b_id".into(),
                r#type: ColumnType::Simple(SimpleColumnType::Uuid),
                nullable: false,
                default: None,
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            }],
            constraints: vec![
                TableConstraint::PrimaryKey {
                    auto_increment: false,
                    columns: vec!["b_id".into()],
                },
                TableConstraint::ForeignKey {
                    name: None,
                    columns: vec!["b_id".into()],
                    ref_table: "level_b".into(),
                    ref_columns: vec!["a_id".into()],
                    on_delete: None,
                    on_update: None,
                },
            ],
        };

        let schema = vec![level_a, level_b, level_c];
        // Resolving level_b.a_id should follow chain to level_a.id
        let (table, columns) = resolve_fk_target("level_b", &["a_id".into()], &schema);
        assert_eq!(table, "level_a");
        assert_eq!(columns, vec!["id"]);
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
    })]
    #[case("composite_pk", TableDef {
        name: "accounts".into(),
        columns: vec![
            ColumnDef { name: "id".into(), r#type: ColumnType::Simple(SimpleColumnType::Integer), nullable: false, default: None, comment: None, primary_key: None, unique: None, index: None, foreign_key: None },
            ColumnDef { name: "tenant_id".into(), r#type: ColumnType::Simple(SimpleColumnType::BigInt), nullable: false, default: None, comment: None, primary_key: None, unique: None, index: None, foreign_key: None },
        ],
        constraints: vec![TableConstraint::PrimaryKey { auto_increment: false, columns: vec!["id".into(), "tenant_id".into()] }],
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
    })]
    #[case("inline_pk", TableDef {
        name: "users".into(),
        columns: vec![
            ColumnDef { name: "id".into(), r#type: ColumnType::Simple(SimpleColumnType::Uuid), nullable: false, default: Some("gen_random_uuid()".into()), comment: None, primary_key: Some(PrimaryKeySyntax::Bool(true)), unique: None, index: None, foreign_key: None },
            ColumnDef { name: "email".into(), r#type: ColumnType::Simple(SimpleColumnType::Text), nullable: false, default: None, comment: None, primary_key: None, unique: Some(vespertide_core::StrOrBoolOrArray::Bool(true)), index: None, foreign_key: None },
        ],
        constraints: vec![],
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
                    values: EnumValues::String(vec!["pending".into(), "shipped".into(), "delivered".into()])
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
    })]
    #[case("enum_nullable", TableDef {
        name: "tasks".into(),
        columns: vec![
            ColumnDef { name: "id".into(), r#type: ColumnType::Simple(SimpleColumnType::Integer), nullable: false, default: None, comment: None, primary_key: Some(PrimaryKeySyntax::Bool(true)), unique: None, index: None, foreign_key: None },
            ColumnDef {
                name: "priority".into(),
                r#type: ColumnType::Complex(ComplexColumnType::Enum {
                    name: "task_priority".into(),
                    values: EnumValues::String(vec!["low".into(), "medium".into(), "high".into(), "critical".into()])
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
    })]
    #[case("enum_multiple_columns", TableDef {
        name: "products".into(),
        columns: vec![
            ColumnDef { name: "id".into(), r#type: ColumnType::Simple(SimpleColumnType::Integer), nullable: false, default: None, comment: None, primary_key: Some(PrimaryKeySyntax::Bool(true)), unique: None, index: None, foreign_key: None },
            ColumnDef {
                name: "category".into(),
                r#type: ColumnType::Complex(ComplexColumnType::Enum {
                    name: "product_category".into(),
                    values: EnumValues::String(vec!["electronics".into(), "clothing".into(), "food".into()])
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
                    values: EnumValues::String(vec!["in_stock".into(), "out_of_stock".into(), "pre_order".into()])
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
    })]
    #[case("enum_shared", TableDef {
        name: "documents".into(),
        columns: vec![
            ColumnDef { name: "id".into(), r#type: ColumnType::Simple(SimpleColumnType::Integer), nullable: false, default: None, comment: None, primary_key: Some(PrimaryKeySyntax::Bool(true)), unique: None, index: None, foreign_key: None },
            ColumnDef {
                name: "status".into(),
                r#type: ColumnType::Complex(ComplexColumnType::Enum {
                    name: "doc_status".into(),
                    values: EnumValues::String(vec!["draft".into(), "published".into(), "archived".into()])
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
                    values: EnumValues::String(vec!["draft".into(), "published".into(), "archived".into()])
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
    })]
    #[case("enum_special_values", TableDef {
        name: "events".into(),
        columns: vec![
            ColumnDef { name: "id".into(), r#type: ColumnType::Simple(SimpleColumnType::Integer), nullable: false, default: None, comment: None, primary_key: Some(PrimaryKeySyntax::Bool(true)), unique: None, index: None, foreign_key: None },
            ColumnDef {
                name: "severity".into(),
                r#type: ColumnType::Complex(ComplexColumnType::Enum {
                    name: "event_severity".into(),
                    values: EnumValues::String(vec!["info-level".into(), "warning_level".into(), "ERROR_LEVEL".into(), "1critical".into()])
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
            TableConstraint::Index { name: Some("idx_department".into()), columns: vec!["department".into()] },
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
                    values: EnumValues::String(vec!["pending".into(), "in_progress".into(), "completed".into()])
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
    })]
    #[case("table_level_pk", TableDef {
        name: "orders".into(),
        columns: vec![
            ColumnDef { name: "id".into(), r#type: ColumnType::Simple(SimpleColumnType::Uuid), nullable: false, default: None, comment: None, primary_key: None, unique: None, index: None, foreign_key: None },
            ColumnDef { name: "customer_id".into(), r#type: ColumnType::Simple(SimpleColumnType::Uuid), nullable: false, default: None, comment: None, primary_key: None, unique: None, index: None, foreign_key: None },
            ColumnDef { name: "total".into(), r#type: ColumnType::Simple(SimpleColumnType::Real), nullable: false, default: None, comment: None, primary_key: None, unique: None, index: None, foreign_key: None },
        ],
        constraints: vec![
            TableConstraint::PrimaryKey { columns: vec!["id".into()], auto_increment: false },
        ],
    })]
    fn render_entity_snapshots(#[case] name: &str, #[case] table: TableDef) {
        let rendered = render_entity(&table);
        with_settings!({ snapshot_suffix => format!("params_{}", name) }, {
            assert_snapshot!(rendered);
        });
    }

    // Helper to create a simple table with PK
    fn col(name: &str, ty: ColumnType) -> ColumnDef {
        ColumnDef {
            name: name.into(),
            r#type: ty,
            nullable: false,
            default: None,
            comment: None,
            primary_key: None,
            unique: None,
            index: None,
            foreign_key: None,
        }
    }

    fn table_with_pk(name: &str, columns: Vec<ColumnDef>, pk_cols: Vec<&str>) -> TableDef {
        TableDef {
            name: name.into(),
            columns,
            constraints: vec![TableConstraint::PrimaryKey {
                auto_increment: false,
                columns: pk_cols.into_iter().map(String::from).collect(),
            }],
        }
    }

    fn table_with_pk_and_fk(
        name: &str,
        columns: Vec<ColumnDef>,
        pk_cols: Vec<&str>,
        fks: Vec<(Vec<&str>, &str, Vec<&str>)>,
    ) -> TableDef {
        let mut constraints = vec![TableConstraint::PrimaryKey {
            auto_increment: false,
            columns: pk_cols.into_iter().map(String::from).collect(),
        }];
        for (cols, ref_table, ref_cols) in fks {
            constraints.push(TableConstraint::ForeignKey {
                name: None,
                columns: cols.into_iter().map(String::from).collect(),
                ref_table: ref_table.into(),
                ref_columns: ref_cols.into_iter().map(String::from).collect(),
                on_delete: None,
                on_update: None,
            });
        }
        TableDef {
            name: name.into(),
            columns,
            constraints,
        }
    }

    #[rstest]
    #[case("many_to_many_article")]
    #[case("many_to_many_user")]
    #[case("many_to_many_missing_target")]
    #[case("composite_fk_parent")]
    #[case("not_junction_single_pk")]
    #[case("not_junction_fk_not_in_pk_other")]
    #[case("not_junction_fk_not_in_pk_another")]
    #[case("multiple_fk_same_table")]
    #[case("multiple_reverse_relations")]
    #[case("multiple_has_one_relations")]
    fn render_entity_with_schema_snapshots(#[case] name: &str) {
        use vespertide_core::SimpleColumnType::*;

        let (table, schema) = match name {
            "many_to_many_article" => {
                let article = table_with_pk(
                    "article",
                    vec![col("id", ColumnType::Simple(BigInt))],
                    vec!["id"],
                );
                let user = table_with_pk(
                    "user",
                    vec![col("id", ColumnType::Simple(Uuid))],
                    vec!["id"],
                );
                let article_user = table_with_pk_and_fk(
                    "article_user",
                    vec![
                        col("article_id", ColumnType::Simple(BigInt)),
                        col("user_id", ColumnType::Simple(Uuid)),
                    ],
                    vec!["article_id", "user_id"],
                    vec![
                        (vec!["article_id"], "article", vec!["id"]),
                        (vec!["user_id"], "user", vec!["id"]),
                    ],
                );
                (article.clone(), vec![article, user, article_user])
            }
            "many_to_many_user" => {
                let article = table_with_pk(
                    "article",
                    vec![col("id", ColumnType::Simple(BigInt))],
                    vec!["id"],
                );
                let user = table_with_pk(
                    "user",
                    vec![col("id", ColumnType::Simple(Uuid))],
                    vec!["id"],
                );
                let article_user = table_with_pk_and_fk(
                    "article_user",
                    vec![
                        col("article_id", ColumnType::Simple(BigInt)),
                        col("user_id", ColumnType::Simple(Uuid)),
                    ],
                    vec!["article_id", "user_id"],
                    vec![
                        (vec!["article_id"], "article", vec!["id"]),
                        (vec!["user_id"], "user", vec!["id"]),
                    ],
                );
                (user.clone(), vec![article, user, article_user])
            }
            "many_to_many_missing_target" => {
                let article = table_with_pk(
                    "article",
                    vec![col("id", ColumnType::Simple(BigInt))],
                    vec!["id"],
                );
                let article_user = table_with_pk_and_fk(
                    "article_user",
                    vec![
                        col("article_id", ColumnType::Simple(BigInt)),
                        col("user_id", ColumnType::Simple(Uuid)),
                    ],
                    vec!["article_id", "user_id"],
                    vec![
                        (vec!["article_id"], "article", vec!["id"]),
                        (vec!["user_id"], "user", vec!["id"]), // user not in schema
                    ],
                );
                (article.clone(), vec![article, article_user])
            }
            "composite_fk_parent" => {
                let parent = table_with_pk(
                    "parent",
                    vec![
                        col("id1", ColumnType::Simple(Integer)),
                        col("id2", ColumnType::Simple(Integer)),
                    ],
                    vec!["id1", "id2"],
                );
                let child_one = table_with_pk_and_fk(
                    "child_one",
                    vec![
                        col("parent_id1", ColumnType::Simple(Integer)),
                        col("parent_id2", ColumnType::Simple(Integer)),
                    ],
                    vec!["parent_id1", "parent_id2"],
                    vec![(
                        vec!["parent_id1", "parent_id2"],
                        "parent",
                        vec!["id1", "id2"],
                    )],
                );
                let child_many = table_with_pk_and_fk(
                    "child_many",
                    vec![
                        col("id", ColumnType::Simple(Integer)),
                        col("parent_id1", ColumnType::Simple(Integer)),
                        col("parent_id2", ColumnType::Simple(Integer)),
                    ],
                    vec!["id"],
                    vec![(
                        vec!["parent_id1", "parent_id2"],
                        "parent",
                        vec!["id1", "id2"],
                    )],
                );
                (parent.clone(), vec![parent, child_one, child_many])
            }
            "not_junction_single_pk" => {
                let other = table_with_pk(
                    "other",
                    vec![col("id", ColumnType::Simple(Integer))],
                    vec!["id"],
                );
                let regular = table_with_pk_and_fk(
                    "regular",
                    vec![
                        col("id", ColumnType::Simple(Integer)),
                        col("other_id", ColumnType::Simple(Integer)),
                    ],
                    vec!["id"], // single column PK
                    vec![(vec!["other_id"], "other", vec!["id"])],
                );
                (other.clone(), vec![other, regular])
            }
            "not_junction_fk_not_in_pk_other" => {
                let other = table_with_pk(
                    "other",
                    vec![col("id", ColumnType::Simple(Integer))],
                    vec!["id"],
                );
                let another = table_with_pk(
                    "another",
                    vec![col("id", ColumnType::Simple(Integer))],
                    vec!["id"],
                );
                let not_junction = table_with_pk_and_fk(
                    "not_junction",
                    vec![
                        col("id", ColumnType::Simple(Integer)),
                        col("other_id", ColumnType::Simple(Integer)),
                        col("another_id", ColumnType::Simple(Integer)),
                    ],
                    vec!["id", "other_id"], // another_id not in PK
                    vec![
                        (vec!["other_id"], "other", vec!["id"]),
                        (vec!["another_id"], "another", vec!["id"]),
                    ],
                );
                (other.clone(), vec![other, another, not_junction])
            }
            "not_junction_fk_not_in_pk_another" => {
                let other = table_with_pk(
                    "other",
                    vec![col("id", ColumnType::Simple(Integer))],
                    vec!["id"],
                );
                let another = table_with_pk(
                    "another",
                    vec![col("id", ColumnType::Simple(Integer))],
                    vec!["id"],
                );
                let not_junction = table_with_pk_and_fk(
                    "not_junction",
                    vec![
                        col("id", ColumnType::Simple(Integer)),
                        col("other_id", ColumnType::Simple(Integer)),
                        col("another_id", ColumnType::Simple(Integer)),
                    ],
                    vec!["id", "other_id"], // another_id not in PK
                    vec![
                        (vec!["other_id"], "other", vec!["id"]),
                        (vec!["another_id"], "another", vec!["id"]),
                    ],
                );
                (another.clone(), vec![other, another, not_junction])
            }
            "multiple_fk_same_table" => {
                let user = table_with_pk(
                    "user",
                    vec![col("id", ColumnType::Simple(Uuid))],
                    vec!["id"],
                );
                let post = table_with_pk_and_fk(
                    "post",
                    vec![
                        col("id", ColumnType::Simple(Uuid)),
                        col("creator_user_id", ColumnType::Simple(Uuid)),
                        col("used_by_user_id", ColumnType::Simple(Uuid)),
                    ],
                    vec!["id"],
                    vec![
                        (vec!["creator_user_id"], "user", vec!["id"]),
                        (vec!["used_by_user_id"], "user", vec!["id"]),
                    ],
                );
                (post.clone(), vec![user, post])
            }
            "multiple_reverse_relations" => {
                // Test case where user has multiple has_one relations from profile
                let user = table_with_pk(
                    "user",
                    vec![col("id", ColumnType::Simple(Uuid))],
                    vec!["id"],
                );
                let profile = table_with_pk_and_fk(
                    "profile",
                    vec![
                        col("id", ColumnType::Simple(Uuid)),
                        col("preferred_user_id", ColumnType::Simple(Uuid)),
                        col("backup_user_id", ColumnType::Simple(Uuid)),
                    ],
                    vec!["id"],
                    vec![
                        (vec!["preferred_user_id"], "user", vec!["id"]),
                        (vec!["backup_user_id"], "user", vec!["id"]),
                    ],
                );
                (user.clone(), vec![user, profile])
            }
            "multiple_has_one_relations" => {
                // Test case where user has multiple has_one relations (UNIQUE FK)
                let user = table_with_pk(
                    "user",
                    vec![col("id", ColumnType::Simple(Uuid))],
                    vec!["id"],
                );
                let settings = table_with_pk_and_fk(
                    "settings",
                    vec![
                        col("id", ColumnType::Simple(Uuid)),
                        col("created_by_user_id", ColumnType::Simple(Uuid)),
                        col("updated_by_user_id", ColumnType::Simple(Uuid)),
                    ],
                    vec!["id"],
                    vec![
                        (vec!["created_by_user_id"], "user", vec!["id"]),
                        (vec!["updated_by_user_id"], "user", vec!["id"]),
                    ],
                );
                // Add unique constraints to make them has_one (coverage for line 553)
                let mut settings_with_unique = settings;
                settings_with_unique
                    .constraints
                    .push(TableConstraint::Unique {
                        name: None,
                        columns: vec!["created_by_user_id".into()],
                    });
                settings_with_unique
                    .constraints
                    .push(TableConstraint::Unique {
                        name: None,
                        columns: vec!["updated_by_user_id".into()],
                    });
                (user.clone(), vec![user, settings_with_unique])
            }
            _ => panic!("Unknown test case: {}", name),
        };

        let rendered = render_entity_with_schema(&table, &schema);
        with_settings!({ snapshot_suffix => format!("schema_{}", name) }, {
            assert_snapshot!(rendered);
        });
    }

    #[test]
    fn test_to_pascal_case_normal_chars() {
        assert_eq!(to_pascal_case("abc"), "Abc");
        assert_eq!(to_pascal_case("a_b_c"), "ABC");
    }

    #[test]
    fn test_numeric_default_value() {
        use vespertide_core::ComplexColumnType;
        let table = TableDef {
            name: "products".into(),
            columns: vec![ColumnDef {
                name: "price".into(),
                r#type: ColumnType::Complex(ComplexColumnType::Numeric {
                    precision: 10,
                    scale: 2,
                }),
                nullable: false,
                default: Some("0.00".into()),
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            }],
            constraints: vec![],
        };
        let rendered = render_entity(&table);
        assert!(rendered.contains("default_value = 0.00"));
    }

    #[test]
    fn test_orm_exporter_trait() {
        use crate::orm::OrmExporter;
        let table = table_with_pk(
            "test",
            vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
            vec!["id"],
        );
        let exporter = SeaOrmExporter;
        let result = exporter.render_entity(&table);
        assert!(result.is_ok());
        assert!(result.unwrap().contains("table_name = \"test\""));
        let schema = vec![table.clone()];
        let result = exporter.render_entity_with_schema(&table, &schema);
        assert!(result.is_ok());
        assert!(result.unwrap().contains("table_name = \"test\""));
    }

    fn int_enum_table(default_value: &str) -> TableDef {
        use vespertide_core::schema::primary_key::PrimaryKeySyntax;
        TableDef {
            name: "tasks".into(),
            columns: vec![
                ColumnDef {
                    name: "id".into(),
                    r#type: ColumnType::Simple(SimpleColumnType::Integer),
                    nullable: false,
                    default: None,
                    comment: None,
                    primary_key: Some(PrimaryKeySyntax::Bool(true)),
                    unique: None,
                    index: None,
                    foreign_key: None,
                },
                ColumnDef {
                    name: "status".into(),
                    r#type: ColumnType::Complex(ComplexColumnType::Enum {
                        name: "task_status".into(),
                        values: EnumValues::Integer(vec![
                            NumValue {
                                name: "Pending".into(),
                                value: 0,
                            },
                            NumValue {
                                name: "InProgress".into(),
                                value: 1,
                            },
                            NumValue {
                                name: "Completed".into(),
                                value: 100,
                            },
                        ]),
                    }),
                    nullable: false,
                    default: Some(default_value.into()),
                    comment: None,
                    primary_key: None,
                    unique: None,
                    index: None,
                    foreign_key: None,
                },
            ],
            constraints: vec![],
        }
    }

    #[rstest]
    #[case::numeric_default("1")]
    #[case::non_numeric_default("pending_status")]
    fn test_integer_enum_default_value_snapshots(#[case] default_value: &str) {
        let table = int_enum_table(default_value);
        let rendered = render_entity(&table);
        with_settings!({ snapshot_suffix => default_value }, {
            assert_snapshot!(rendered);
        });
    }

    #[test]
    fn test_boolean_default_value_with_bool_type() {
        use vespertide_core::schema::primary_key::PrimaryKeySyntax;
        let table = TableDef {
            name: "settings".into(),
            columns: vec![
                ColumnDef {
                    name: "id".into(),
                    r#type: ColumnType::Simple(SimpleColumnType::Integer),
                    nullable: false,
                    default: None,
                    comment: None,
                    primary_key: Some(PrimaryKeySyntax::Bool(true)),
                    unique: None,
                    index: None,
                    foreign_key: None,
                },
                ColumnDef {
                    name: "is_active".into(),
                    r#type: ColumnType::Simple(SimpleColumnType::Boolean),
                    nullable: false,
                    default: Some(StringOrBool::Bool(true)),
                    comment: None,
                    primary_key: None,
                    unique: None,
                    index: None,
                    foreign_key: None,
                },
                ColumnDef {
                    name: "is_deleted".into(),
                    r#type: ColumnType::Simple(SimpleColumnType::Boolean),
                    nullable: false,
                    default: Some(StringOrBool::Bool(false)),
                    comment: None,
                    primary_key: None,
                    unique: None,
                    index: None,
                    foreign_key: None,
                },
            ],
            constraints: vec![],
        };
        let rendered = render_entity(&table);
        assert!(rendered.contains("default_value = true"));
        assert!(rendered.contains("default_value = false"));
    }
}
