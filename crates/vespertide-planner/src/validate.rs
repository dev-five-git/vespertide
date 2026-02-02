use std::collections::HashSet;

use vespertide_core::{
    ColumnDef, ColumnType, ComplexColumnType, EnumValues, MigrationAction, MigrationPlan,
    TableConstraint, TableDef,
};

use crate::error::{InvalidEnumDefaultError, PlannerError};

/// Validate a schema for data integrity issues.
/// Checks for:
/// - Duplicate table names
/// - Foreign keys referencing non-existent tables
/// - Foreign keys referencing non-existent columns
/// - Indexes referencing non-existent columns
/// - Constraints referencing non-existent columns
/// - Empty constraint column lists
pub fn validate_schema(schema: &[TableDef]) -> Result<(), PlannerError> {
    // Check for duplicate table names
    let mut table_names = HashSet::new();
    for table in schema {
        if !table_names.insert(&table.name) {
            return Err(PlannerError::DuplicateTableName(table.name.clone()));
        }
    }

    // Build a map of table names to their column names for quick lookup
    let table_map: std::collections::HashMap<_, _> = schema
        .iter()
        .map(|t| {
            let columns: HashSet<_> = t.columns.iter().map(|c| c.name.as_str()).collect();
            (t.name.as_str(), columns)
        })
        .collect();

    // Validate each table
    for table in schema {
        validate_table(table, &table_map)?;
    }

    Ok(())
}

fn validate_table(
    table: &TableDef,
    table_map: &std::collections::HashMap<&str, HashSet<&str>>,
) -> Result<(), PlannerError> {
    let table_columns: HashSet<_> = table.columns.iter().map(|c| c.name.as_str()).collect();

    // Check that the table has a primary key
    // Primary key can be defined either:
    // 1. As a table-level constraint (TableConstraint::PrimaryKey)
    // 2. As an inline column definition (column.primary_key = Some(...))
    let has_table_pk = table
        .constraints
        .iter()
        .any(|c| matches!(c, TableConstraint::PrimaryKey { .. }));
    let has_inline_pk = table.columns.iter().any(|c| c.primary_key.is_some());

    if !has_table_pk && !has_inline_pk {
        return Err(PlannerError::MissingPrimaryKey(table.name.clone()));
    }

    // Validate auto_increment columns have integer types
    for constraint in &table.constraints {
        if let TableConstraint::PrimaryKey {
            auto_increment: true,
            columns,
        } = constraint
        {
            for col_name in columns {
                if let Some(column) = table.columns.iter().find(|c| c.name == *col_name)
                    && !column.r#type.supports_auto_increment() {
                        return Err(PlannerError::InvalidAutoIncrement(
                            table.name.clone(),
                            col_name.clone(),
                            format!("{:?}", column.r#type),
                        ));
                    }
            }
        }
    }

    // Validate auto_increment on inline primary_key definitions
    use vespertide_core::schema::primary_key::PrimaryKeySyntax;
    for column in &table.columns {
        if let Some(pk_syntax) = &column.primary_key {
            let has_auto_increment = match pk_syntax {
                PrimaryKeySyntax::Bool(_) => false,
                PrimaryKeySyntax::Object(pk_def) => pk_def.auto_increment,
            };
            if has_auto_increment && !column.r#type.supports_auto_increment() {
                return Err(PlannerError::InvalidAutoIncrement(
                    table.name.clone(),
                    column.name.clone(),
                    format!("{:?}", column.r#type),
                ));
            }
        }
    }

    // Validate columns (enum types)
    for column in &table.columns {
        validate_column(column, &table.name)?;
    }

    // Validate constraints (including indexes)
    for constraint in &table.constraints {
        validate_constraint(constraint, &table.name, &table_columns, table_map)?;
    }

    Ok(())
}

/// Extract the unquoted value from a potentially quoted string.
/// Returns None if the value is a SQL expression (contains parentheses or is a keyword).
fn extract_enum_value(value: &str) -> Option<&str> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }
    // Check for SQL expressions/keywords that shouldn't be validated
    if trimmed.contains('(')
        || trimmed.contains(')')
        || trimmed.eq_ignore_ascii_case("null")
        || trimmed.eq_ignore_ascii_case("current_timestamp")
        || trimmed.eq_ignore_ascii_case("now")
    {
        return None;
    }
    // Strip quotes if present
    if ((trimmed.starts_with('\'') && trimmed.ends_with('\''))
        || (trimmed.starts_with('"') && trimmed.ends_with('"')))
        && trimmed.len() >= 2
    {
        return Some(&trimmed[1..trimmed.len() - 1]);
    }
    // Unquoted value
    Some(trimmed)
}

/// Validate that an enum default/fill_with value is in the allowed enum values.
fn validate_enum_value(
    value: &str,
    enum_name: &str,
    enum_values: &EnumValues,
    table_name: &str,
    column_name: &str,
    value_type: &str, // "default" or "fill_with"
) -> Result<(), PlannerError> {
    let extracted = match extract_enum_value(value) {
        Some(v) => v,
        None => return Ok(()), // Skip validation for SQL expressions
    };

    let is_valid = match enum_values {
        EnumValues::String(variants) => variants.iter().any(|v| v == extracted),
        EnumValues::Integer(variants) => variants.iter().any(|v| v.name == extracted),
    };

    if !is_valid {
        let allowed = enum_values.variant_names().join(", ");
        return Err(Box::new(InvalidEnumDefaultError {
            enum_name: enum_name.to_string(),
            table_name: table_name.to_string(),
            column_name: column_name.to_string(),
            value_type: value_type.to_string(),
            value: extracted.to_string(),
            allowed,
        })
        .into());
    }

    Ok(())
}

fn validate_column(column: &ColumnDef, table_name: &str) -> Result<(), PlannerError> {
    // Validate enum types for duplicate names/values
    if let ColumnType::Complex(ComplexColumnType::Enum { name, values }) = &column.r#type {
        match values {
            EnumValues::String(variants) => {
                let mut seen = HashSet::new();
                for variant in variants {
                    if !seen.insert(variant.as_str()) {
                        return Err(PlannerError::DuplicateEnumVariantName(
                            name.clone(),
                            table_name.to_string(),
                            column.name.clone(),
                            variant.clone(),
                        ));
                    }
                }
            }
            EnumValues::Integer(variants) => {
                // Check duplicate names
                let mut seen_names = HashSet::new();
                for variant in variants {
                    if !seen_names.insert(variant.name.as_str()) {
                        return Err(PlannerError::DuplicateEnumVariantName(
                            name.clone(),
                            table_name.to_string(),
                            column.name.clone(),
                            variant.name.clone(),
                        ));
                    }
                }
                // Check duplicate values
                let mut seen_values = HashSet::new();
                for variant in variants {
                    if !seen_values.insert(variant.value) {
                        return Err(PlannerError::DuplicateEnumValue(
                            name.clone(),
                            table_name.to_string(),
                            column.name.clone(),
                            variant.value,
                        ));
                    }
                }
            }
        }

        // Validate default value is in enum values
        if let Some(default) = &column.default {
            let default_str = default.to_sql();
            validate_enum_value(
                &default_str,
                name,
                values,
                table_name,
                &column.name,
                "default",
            )?;
        }
    }
    Ok(())
}

fn validate_constraint(
    constraint: &TableConstraint,
    table_name: &str,
    table_columns: &HashSet<&str>,
    table_map: &std::collections::HashMap<&str, HashSet<&str>>,
) -> Result<(), PlannerError> {
    match constraint {
        TableConstraint::PrimaryKey { columns, .. } => {
            if columns.is_empty() {
                return Err(PlannerError::EmptyConstraintColumns(
                    table_name.to_string(),
                    "PrimaryKey".to_string(),
                ));
            }
            for col in columns {
                if !table_columns.contains(col.as_str()) {
                    return Err(PlannerError::ConstraintColumnNotFound(
                        table_name.to_string(),
                        "PrimaryKey".to_string(),
                        col.clone(),
                    ));
                }
            }
        }
        TableConstraint::Unique { columns, .. } => {
            if columns.is_empty() {
                return Err(PlannerError::EmptyConstraintColumns(
                    table_name.to_string(),
                    "Unique".to_string(),
                ));
            }
            for col in columns {
                if !table_columns.contains(col.as_str()) {
                    return Err(PlannerError::ConstraintColumnNotFound(
                        table_name.to_string(),
                        "Unique".to_string(),
                        col.clone(),
                    ));
                }
            }
        }
        TableConstraint::ForeignKey {
            columns,
            ref_table,
            ref_columns,
            ..
        } => {
            if columns.is_empty() {
                return Err(PlannerError::EmptyConstraintColumns(
                    table_name.to_string(),
                    "ForeignKey".to_string(),
                ));
            }
            if ref_columns.is_empty() {
                return Err(PlannerError::EmptyConstraintColumns(
                    ref_table.clone(),
                    "ForeignKey (ref_columns)".to_string(),
                ));
            }

            // Check that referenced table exists
            let ref_table_columns = table_map.get(ref_table.as_str()).ok_or_else(|| {
                PlannerError::ForeignKeyTableNotFound(
                    table_name.to_string(),
                    columns.join(", "),
                    ref_table.clone(),
                )
            })?;

            // Check that all columns in this table exist
            for col in columns {
                if !table_columns.contains(col.as_str()) {
                    return Err(PlannerError::ConstraintColumnNotFound(
                        table_name.to_string(),
                        "ForeignKey".to_string(),
                        col.clone(),
                    ));
                }
            }

            // Check that all referenced columns exist in the referenced table
            for ref_col in ref_columns {
                if !ref_table_columns.contains(ref_col.as_str()) {
                    return Err(PlannerError::ForeignKeyColumnNotFound(
                        table_name.to_string(),
                        columns.join(", "),
                        ref_table.clone(),
                        ref_col.clone(),
                    ));
                }
            }

            // Check that column counts match
            if columns.len() != ref_columns.len() {
                return Err(PlannerError::ForeignKeyColumnNotFound(
                    table_name.to_string(),
                    format!(
                        "column count mismatch: {} != {}",
                        columns.len(),
                        ref_columns.len()
                    ),
                    ref_table.clone(),
                    "".to_string(),
                ));
            }
        }
        TableConstraint::Check { .. } => {
            // Check constraints are just expressions, no validation needed
        }
        TableConstraint::Index { name, columns } => {
            if columns.is_empty() {
                let index_name = name.clone().unwrap_or_else(|| "(unnamed)".to_string());
                return Err(PlannerError::EmptyConstraintColumns(
                    table_name.to_string(),
                    format!("Index({})", index_name),
                ));
            }

            for col in columns {
                if !table_columns.contains(col.as_str()) {
                    let index_name = name.clone().unwrap_or_else(|| "(unnamed)".to_string());
                    return Err(PlannerError::IndexColumnNotFound(
                        table_name.to_string(),
                        index_name,
                        col.clone(),
                    ));
                }
            }
        }
    }

    Ok(())
}

/// Validate a migration plan for correctness.
/// Checks for:
/// - AddColumn actions with NOT NULL columns without default must have fill_with
/// - ModifyColumnNullable actions changing from nullable to non-nullable must have fill_with
/// - Enum columns with default/fill_with values must have valid enum values
pub fn validate_migration_plan(plan: &MigrationPlan) -> Result<(), PlannerError> {
    for action in &plan.actions {
        match action {
            MigrationAction::AddColumn {
                table,
                column,
                fill_with,
            } => {
                // If column is NOT NULL and has no default, fill_with is required
                if !column.nullable && column.default.is_none() && fill_with.is_none() {
                    return Err(PlannerError::MissingFillWith(
                        table.clone(),
                        column.name.clone(),
                    ));
                }

                // Validate enum default/fill_with values
                if let ColumnType::Complex(ComplexColumnType::Enum { name, values }) =
                    &column.r#type
                {
                    if let Some(fill) = fill_with {
                        validate_enum_value(fill, name, values, table, &column.name, "fill_with")?;
                    }
                    if let Some(default) = &column.default {
                        let default_str = default.to_sql();
                        validate_enum_value(
                            &default_str,
                            name,
                            values,
                            table,
                            &column.name,
                            "default",
                        )?;
                    }
                }
            }
            MigrationAction::ModifyColumnNullable {
                table,
                column,
                nullable,
                fill_with,
            } => {
                // If changing from nullable to non-nullable, fill_with is required
                if !nullable && fill_with.is_none() {
                    return Err(PlannerError::MissingFillWith(table.clone(), column.clone()));
                }
            }
            _ => {}
        }
    }
    Ok(())
}

/// Information about an action that requires a fill_with value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FillWithRequired {
    /// Index of the action in the migration plan.
    pub action_index: usize,
    /// Table name.
    pub table: String,
    /// Column name.
    pub column: String,
    /// Type of action: "AddColumn" or "ModifyColumnNullable".
    pub action_type: &'static str,
    /// Column type (for display purposes).
    pub column_type: Option<String>,
    /// Default fill value hint for this column type.
    pub default_value: Option<String>,
    /// Enum values if the column is an enum type (for selection UI).
    pub enum_values: Option<Vec<String>>,
}

/// Find all actions in a migration plan that require fill_with values.
/// Returns a list of actions that need fill_with but don't have one.
pub fn find_missing_fill_with(plan: &MigrationPlan) -> Vec<FillWithRequired> {
    let mut missing = Vec::new();

    for (idx, action) in plan.actions.iter().enumerate() {
        match action {
            MigrationAction::AddColumn {
                table,
                column,
                fill_with,
            } => {
                // If column is NOT NULL and has no default, fill_with is required
                if !column.nullable && column.default.is_none() && fill_with.is_none() {
                    missing.push(FillWithRequired {
                        action_index: idx,
                        table: table.clone(),
                        column: column.name.clone(),
                        action_type: "AddColumn",
                        column_type: Some(column.r#type.to_display_string()),
                        default_value: column.r#type.default_fill_value().map(String::from),
                        enum_values: column.r#type.enum_variant_names(),
                    });
                }
            }
            MigrationAction::ModifyColumnNullable {
                table,
                column,
                nullable,
                fill_with,
            } => {
                // If changing from nullable to non-nullable, fill_with is required
                if !nullable && fill_with.is_none() {
                    missing.push(FillWithRequired {
                        action_index: idx,
                        table: table.clone(),
                        column: column.clone(),
                        action_type: "ModifyColumnNullable",
                        column_type: None,
                        default_value: None,
                        enum_values: None, // We don't have column type info here
                    });
                }
            }
            _ => {}
        }
    }

    missing
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;
    use vespertide_core::schema::primary_key::{PrimaryKeyDef, PrimaryKeySyntax};
    use vespertide_core::{
        ColumnDef, ColumnType, ComplexColumnType, EnumValues, NumValue, SimpleColumnType,
        TableConstraint,
    };

    fn col(name: &str, ty: ColumnType) -> ColumnDef {
        ColumnDef {
            name: name.to_string(),
            r#type: ty,
            nullable: true,
            default: None,
            comment: None,
            primary_key: None,
            unique: None,
            index: None,
            foreign_key: None,
        }
    }

    fn table(name: &str, columns: Vec<ColumnDef>, constraints: Vec<TableConstraint>) -> TableDef {
        TableDef {
            name: name.to_string(),
            description: None,
            columns,
            constraints,
        }
    }

    fn idx(name: &str, columns: Vec<&str>) -> TableConstraint {
        TableConstraint::Index {
            name: Some(name.to_string()),
            columns: columns.into_iter().map(|s| s.to_string()).collect(),
        }
    }

    fn is_duplicate(err: &PlannerError) -> bool {
        matches!(err, PlannerError::DuplicateTableName(_))
    }

    fn is_fk_table(err: &PlannerError) -> bool {
        matches!(err, PlannerError::ForeignKeyTableNotFound(_, _, _))
    }

    fn is_fk_column(err: &PlannerError) -> bool {
        matches!(err, PlannerError::ForeignKeyColumnNotFound(_, _, _, _))
    }

    fn is_index_column(err: &PlannerError) -> bool {
        matches!(err, PlannerError::IndexColumnNotFound(_, _, _))
    }

    fn is_constraint_column(err: &PlannerError) -> bool {
        matches!(err, PlannerError::ConstraintColumnNotFound(_, _, _))
    }

    fn is_empty_columns(err: &PlannerError) -> bool {
        matches!(err, PlannerError::EmptyConstraintColumns(_, _))
    }

    fn is_missing_pk(err: &PlannerError) -> bool {
        matches!(err, PlannerError::MissingPrimaryKey(_))
    }

    fn pk(columns: Vec<&str>) -> TableConstraint {
        TableConstraint::PrimaryKey {
            auto_increment: false,
            columns: columns.into_iter().map(|s| s.to_string()).collect(),
        }
    }

    #[rstest]
    #[case::valid_schema(
        vec![table(
            "users",
            vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
            vec![TableConstraint::PrimaryKey{ auto_increment: false, columns: vec!["id".into()] }],
        )],
        None
    )]
    #[case::duplicate_table(
        vec![
            table("users", vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))], vec![]),
            table("users", vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))], vec![]),
        ],
        Some(is_duplicate as fn(&PlannerError) -> bool)
    )]
    #[case::fk_missing_table(
        vec![table(
            "users",
            vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
            vec![pk(vec!["id"]), TableConstraint::ForeignKey {
                name: None,
                columns: vec!["id".into()],
                ref_table: "nonexistent".into(),
                ref_columns: vec!["id".into()],
                on_delete: None,
                on_update: None,
            }],
        )],
        Some(is_fk_table as fn(&PlannerError) -> bool)
    )]
    #[case::fk_missing_column(
        vec![
            table("posts", vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))], vec![pk(vec!["id"])]),
            table(
                "users",
                vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
                vec![pk(vec!["id"]), TableConstraint::ForeignKey {
                    name: None,
                    columns: vec!["id".into()],
                    ref_table: "posts".into(),
                    ref_columns: vec!["nonexistent".into()],
                    on_delete: None,
                    on_update: None,
                }],
            ),
        ],
        Some(is_fk_column as fn(&PlannerError) -> bool)
    )]
    #[case::fk_local_missing_column(
        vec![
            table("posts", vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))], vec![pk(vec!["id"])]),
            table(
                "users",
                vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
                vec![pk(vec!["id"]), TableConstraint::ForeignKey {
                    name: None,
                    columns: vec!["missing".into()],
                    ref_table: "posts".into(),
                    ref_columns: vec!["id".into()],
                    on_delete: None,
                    on_update: None,
                }],
            ),
        ],
        Some(is_constraint_column as fn(&PlannerError) -> bool)
    )]
    #[case::fk_valid(
        vec![
            table(
                "posts",
                vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
                vec![pk(vec!["id"])],
            ),
            table(
                "users",
                vec![col("id", ColumnType::Simple(SimpleColumnType::Integer)), col("post_id", ColumnType::Simple(SimpleColumnType::Integer))],
                vec![pk(vec!["id"]), TableConstraint::ForeignKey {
                    name: None,
                    columns: vec!["post_id".into()],
                    ref_table: "posts".into(),
                    ref_columns: vec!["id".into()],
                    on_delete: None,
                    on_update: None,
                }],
            ),
        ],
        None
    )]
    #[case::index_missing_column(
        vec![table(
            "users",
            vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
            vec![pk(vec!["id"]), idx("idx_name", vec!["nonexistent"])],
        )],
        Some(is_index_column as fn(&PlannerError) -> bool)
    )]
    #[case::constraint_missing_column(
        vec![table(
            "users",
            vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
            vec![TableConstraint::PrimaryKey{ auto_increment: false, columns: vec!["nonexistent".into()] }],
        )],
        Some(is_constraint_column as fn(&PlannerError) -> bool)
    )]
    #[case::unique_empty_columns(
        vec![table(
            "users",
            vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
            vec![pk(vec!["id"]), TableConstraint::Unique {
                name: Some("u".into()),
                columns: vec![],
            }],
        )],
        Some(is_empty_columns as fn(&PlannerError) -> bool)
    )]
    #[case::unique_missing_column(
        vec![table(
            "users",
            vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
            vec![pk(vec!["id"]), TableConstraint::Unique {
                name: None,
                columns: vec!["missing".into()],
            }],
        )],
        Some(is_constraint_column as fn(&PlannerError) -> bool)
    )]
    #[case::empty_primary_key(
        vec![table(
            "users",
            vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
            vec![TableConstraint::PrimaryKey{ auto_increment: false, columns: vec![] }],
        )],
        Some(is_empty_columns as fn(&PlannerError) -> bool)
    )]
    #[case::fk_column_count_mismatch(
        vec![
            table(
                "posts",
                vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
                vec![pk(vec!["id"])],
            ),
            table(
                "users",
                vec![col("id", ColumnType::Simple(SimpleColumnType::Integer)), col("post_id", ColumnType::Simple(SimpleColumnType::Integer))],
                vec![pk(vec!["id"]), TableConstraint::ForeignKey {
                    name: None,
                    columns: vec!["id".into(), "post_id".into()],
                    ref_table: "posts".into(),
                    ref_columns: vec!["id".into()],
                    on_delete: None,
                    on_update: None,
                }],
            ),
        ],
        Some(is_fk_column as fn(&PlannerError) -> bool)
    )]
    #[case::fk_empty_columns(
        vec![table(
            "users",
            vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
            vec![pk(vec!["id"]), TableConstraint::ForeignKey {
                name: None,
                columns: vec![],
                ref_table: "posts".into(),
                ref_columns: vec!["id".into()],
                on_delete: None,
                on_update: None,
            }],
        )],
        Some(is_empty_columns as fn(&PlannerError) -> bool)
    )]
    #[case::fk_empty_ref_columns(
        vec![
            table(
                "posts",
                vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
                vec![pk(vec!["id"])],
            ),
            table(
                "users",
                vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
                vec![pk(vec!["id"]), TableConstraint::ForeignKey {
                    name: None,
                    columns: vec!["id".into()],
                    ref_table: "posts".into(),
                    ref_columns: vec![],
                    on_delete: None,
                    on_update: None,
                }],
            ),
        ],
        Some(is_empty_columns as fn(&PlannerError) -> bool)
    )]
    #[case::index_empty_columns(
        vec![table(
            "users",
            vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
            vec![pk(vec!["id"]), TableConstraint::Index {
                name: Some("idx".into()),
                columns: vec![],
            }],
        )],
        Some(is_empty_columns as fn(&PlannerError) -> bool)
    )]
    #[case::index_valid(
        vec![table(
            "users",
            vec![col("id", ColumnType::Simple(SimpleColumnType::Integer)), col("name", ColumnType::Simple(SimpleColumnType::Text))],
            vec![pk(vec!["id"]), idx("idx_name", vec!["name"])],
        )],
        None
    )]
    #[case::check_constraint_ok(
        vec![table(
            "users",
            vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
            vec![pk(vec!["id"]), TableConstraint::Check {
                name: "ck".into(),
                expr: "id > 0".into(),
            }],
        )],
        None
    )]
    #[case::missing_primary_key(
        vec![table(
            "users",
            vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
            vec![],
        )],
        Some(is_missing_pk as fn(&PlannerError) -> bool)
    )]
    fn validate_schema_cases(
        #[case] schema: Vec<TableDef>,
        #[case] expected_err: Option<fn(&PlannerError) -> bool>,
    ) {
        let result = validate_schema(&schema);
        match expected_err {
            None => assert!(result.is_ok()),
            Some(pred) => {
                let err = result.unwrap_err();
                assert!(pred(&err), "unexpected error: {:?}", err);
            }
        }
    }

    #[test]
    fn validate_migration_plan_missing_fill_with() {
        use vespertide_core::{ColumnDef, ColumnType, MigrationAction, MigrationPlan};

        let plan = MigrationPlan {
            comment: None,
            created_at: None,
            version: 1,
            actions: vec![MigrationAction::AddColumn {
                table: "users".into(),
                column: Box::new(ColumnDef {
                    name: "email".into(),
                    r#type: ColumnType::Simple(SimpleColumnType::Text),
                    nullable: false,
                    default: None,
                    comment: None,
                    primary_key: None,
                    unique: None,
                    index: None,
                    foreign_key: None,
                }),
                fill_with: None,
            }],
        };

        let result = validate_migration_plan(&plan);
        assert!(result.is_err());
        match result.unwrap_err() {
            PlannerError::MissingFillWith(table, column) => {
                assert_eq!(table, "users");
                assert_eq!(column, "email");
            }
            _ => panic!("expected MissingFillWith error"),
        }
    }

    #[test]
    fn validate_migration_plan_with_fill_with() {
        use vespertide_core::{ColumnDef, ColumnType, MigrationAction, MigrationPlan};

        let plan = MigrationPlan {
            comment: None,
            created_at: None,
            version: 1,
            actions: vec![MigrationAction::AddColumn {
                table: "users".into(),
                column: Box::new(ColumnDef {
                    name: "email".into(),
                    r#type: ColumnType::Simple(SimpleColumnType::Text),
                    nullable: false,
                    default: None,
                    comment: None,
                    primary_key: None,
                    unique: None,
                    index: None,
                    foreign_key: None,
                }),
                fill_with: Some("default@example.com".into()),
            }],
        };

        let result = validate_migration_plan(&plan);
        assert!(result.is_ok());
    }

    #[test]
    fn validate_migration_plan_nullable_column() {
        use vespertide_core::{ColumnDef, ColumnType, MigrationAction, MigrationPlan};

        let plan = MigrationPlan {
            comment: None,
            created_at: None,
            version: 1,
            actions: vec![MigrationAction::AddColumn {
                table: "users".into(),
                column: Box::new(ColumnDef {
                    name: "email".into(),
                    r#type: ColumnType::Simple(SimpleColumnType::Text),
                    nullable: true,
                    default: None,
                    comment: None,
                    primary_key: None,
                    unique: None,
                    index: None,
                    foreign_key: None,
                }),
                fill_with: None,
            }],
        };

        let result = validate_migration_plan(&plan);
        assert!(result.is_ok());
    }

    #[test]
    fn validate_migration_plan_with_default() {
        use vespertide_core::{ColumnDef, ColumnType, MigrationAction, MigrationPlan};

        let plan = MigrationPlan {
            comment: None,
            created_at: None,
            version: 1,
            actions: vec![MigrationAction::AddColumn {
                table: "users".into(),
                column: Box::new(ColumnDef {
                    name: "email".into(),
                    r#type: ColumnType::Simple(SimpleColumnType::Text),
                    nullable: false,
                    default: Some("default@example.com".into()),
                    comment: None,
                    primary_key: None,
                    unique: None,
                    index: None,
                    foreign_key: None,
                }),
                fill_with: None,
            }],
        };

        let result = validate_migration_plan(&plan);
        assert!(result.is_ok());
    }

    #[test]
    fn validate_string_enum_duplicate_variant_name() {
        let schema = vec![table(
            "users",
            vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                col(
                    "status",
                    ColumnType::Complex(ComplexColumnType::Enum {
                        name: "user_status".into(),
                        values: EnumValues::String(vec![
                            "active".into(),
                            "inactive".into(),
                            "active".into(), // duplicate
                        ]),
                    }),
                ),
            ],
            vec![TableConstraint::PrimaryKey {
                auto_increment: false,
                columns: vec!["id".into()],
            }],
        )];

        let result = validate_schema(&schema);
        assert!(result.is_err());
        match result.unwrap_err() {
            PlannerError::DuplicateEnumVariantName(enum_name, table, column, variant) => {
                assert_eq!(enum_name, "user_status");
                assert_eq!(table, "users");
                assert_eq!(column, "status");
                assert_eq!(variant, "active");
            }
            err => panic!("expected DuplicateEnumVariantName, got {:?}", err),
        }
    }

    #[test]
    fn validate_integer_enum_duplicate_variant_name() {
        let schema = vec![table(
            "tasks",
            vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                col(
                    "priority",
                    ColumnType::Complex(ComplexColumnType::Enum {
                        name: "priority_level".into(),
                        values: EnumValues::Integer(vec![
                            NumValue {
                                name: "Low".into(),
                                value: 0,
                            },
                            NumValue {
                                name: "High".into(),
                                value: 1,
                            },
                            NumValue {
                                name: "Low".into(), // duplicate name
                                value: 2,
                            },
                        ]),
                    }),
                ),
            ],
            vec![TableConstraint::PrimaryKey {
                auto_increment: false,
                columns: vec!["id".into()],
            }],
        )];

        let result = validate_schema(&schema);
        assert!(result.is_err());
        match result.unwrap_err() {
            PlannerError::DuplicateEnumVariantName(enum_name, table, column, variant) => {
                assert_eq!(enum_name, "priority_level");
                assert_eq!(table, "tasks");
                assert_eq!(column, "priority");
                assert_eq!(variant, "Low");
            }
            err => panic!("expected DuplicateEnumVariantName, got {:?}", err),
        }
    }

    #[test]
    fn validate_integer_enum_duplicate_value() {
        let schema = vec![table(
            "tasks",
            vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                col(
                    "priority",
                    ColumnType::Complex(ComplexColumnType::Enum {
                        name: "priority_level".into(),
                        values: EnumValues::Integer(vec![
                            NumValue {
                                name: "Low".into(),
                                value: 0,
                            },
                            NumValue {
                                name: "Medium".into(),
                                value: 1,
                            },
                            NumValue {
                                name: "High".into(),
                                value: 0, // duplicate value
                            },
                        ]),
                    }),
                ),
            ],
            vec![TableConstraint::PrimaryKey {
                auto_increment: false,
                columns: vec!["id".into()],
            }],
        )];

        let result = validate_schema(&schema);
        assert!(result.is_err());
        match result.unwrap_err() {
            PlannerError::DuplicateEnumValue(enum_name, table, column, value) => {
                assert_eq!(enum_name, "priority_level");
                assert_eq!(table, "tasks");
                assert_eq!(column, "priority");
                assert_eq!(value, 0);
            }
            err => panic!("expected DuplicateEnumValue, got {:?}", err),
        }
    }

    #[test]
    fn validate_enum_valid() {
        let schema = vec![table(
            "tasks",
            vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                col(
                    "status",
                    ColumnType::Complex(ComplexColumnType::Enum {
                        name: "task_status".into(),
                        values: EnumValues::String(vec![
                            "pending".into(),
                            "in_progress".into(),
                            "completed".into(),
                        ]),
                    }),
                ),
                col(
                    "priority",
                    ColumnType::Complex(ComplexColumnType::Enum {
                        name: "priority_level".into(),
                        values: EnumValues::Integer(vec![
                            NumValue {
                                name: "Low".into(),
                                value: 0,
                            },
                            NumValue {
                                name: "Medium".into(),
                                value: 50,
                            },
                            NumValue {
                                name: "High".into(),
                                value: 100,
                            },
                        ]),
                    }),
                ),
            ],
            vec![TableConstraint::PrimaryKey {
                auto_increment: false,
                columns: vec!["id".into()],
            }],
        )];

        let result = validate_schema(&schema);
        assert!(result.is_ok());
    }

    #[test]
    fn validate_migration_plan_modify_nullable_to_non_nullable_missing_fill_with() {
        let plan = MigrationPlan {
            comment: None,
            created_at: None,
            version: 1,
            actions: vec![MigrationAction::ModifyColumnNullable {
                table: "users".into(),
                column: "email".into(),
                nullable: false,
                fill_with: None,
            }],
        };

        let result = validate_migration_plan(&plan);
        assert!(result.is_err());
        match result.unwrap_err() {
            PlannerError::MissingFillWith(table, column) => {
                assert_eq!(table, "users");
                assert_eq!(column, "email");
            }
            _ => panic!("expected MissingFillWith error"),
        }
    }

    #[test]
    fn validate_migration_plan_modify_nullable_to_non_nullable_with_fill_with() {
        let plan = MigrationPlan {
            comment: None,
            created_at: None,
            version: 1,
            actions: vec![MigrationAction::ModifyColumnNullable {
                table: "users".into(),
                column: "email".into(),
                nullable: false,
                fill_with: Some("'unknown'".into()),
            }],
        };

        let result = validate_migration_plan(&plan);
        assert!(result.is_ok());
    }

    #[test]
    fn validate_migration_plan_modify_non_nullable_to_nullable() {
        // Changing from non-nullable to nullable does NOT require fill_with
        let plan = MigrationPlan {
            comment: None,
            created_at: None,
            version: 1,
            actions: vec![MigrationAction::ModifyColumnNullable {
                table: "users".into(),
                column: "email".into(),
                nullable: true,
                fill_with: None,
            }],
        };

        let result = validate_migration_plan(&plan);
        assert!(result.is_ok());
    }

    #[test]
    fn validate_enum_add_column_invalid_default() {
        let plan = MigrationPlan {
            comment: None,
            created_at: None,
            version: 1,
            actions: vec![MigrationAction::AddColumn {
                table: "users".into(),
                column: Box::new(ColumnDef {
                    name: "status".into(),
                    r#type: ColumnType::Complex(ComplexColumnType::Enum {
                        name: "user_status".into(),
                        values: EnumValues::String(vec![
                            "active".into(),
                            "inactive".into(),
                            "pending".into(),
                        ]),
                    }),
                    nullable: false,
                    default: Some("invalid_value".into()),
                    comment: None,
                    primary_key: None,
                    unique: None,
                    index: None,
                    foreign_key: None,
                }),
                fill_with: None,
            }],
        };

        let result = validate_migration_plan(&plan);
        assert!(result.is_err());
        match result.unwrap_err() {
            PlannerError::InvalidEnumDefault(err) => {
                assert_eq!(err.enum_name, "user_status");
                assert_eq!(err.table_name, "users");
                assert_eq!(err.column_name, "status");
                assert_eq!(err.value_type, "default");
                assert_eq!(err.value, "invalid_value");
            }
            err => panic!("expected InvalidEnumDefault error, got {:?}", err),
        }
    }

    #[test]
    fn validate_enum_add_column_invalid_fill_with() {
        let plan = MigrationPlan {
            comment: None,
            created_at: None,
            version: 1,
            actions: vec![MigrationAction::AddColumn {
                table: "users".into(),
                column: Box::new(ColumnDef {
                    name: "status".into(),
                    r#type: ColumnType::Complex(ComplexColumnType::Enum {
                        name: "user_status".into(),
                        values: EnumValues::String(vec![
                            "active".into(),
                            "inactive".into(),
                            "pending".into(),
                        ]),
                    }),
                    nullable: false,
                    default: None,
                    comment: None,
                    primary_key: None,
                    unique: None,
                    index: None,
                    foreign_key: None,
                }),
                fill_with: Some("unknown_status".into()),
            }],
        };

        let result = validate_migration_plan(&plan);
        assert!(result.is_err());
        match result.unwrap_err() {
            PlannerError::InvalidEnumDefault(err) => {
                assert_eq!(err.enum_name, "user_status");
                assert_eq!(err.table_name, "users");
                assert_eq!(err.column_name, "status");
                assert_eq!(err.value_type, "fill_with");
                assert_eq!(err.value, "unknown_status");
            }
            err => panic!("expected InvalidEnumDefault error, got {:?}", err),
        }
    }

    #[test]
    fn validate_enum_add_column_valid_default_quoted() {
        let plan = MigrationPlan {
            comment: None,
            created_at: None,
            version: 1,
            actions: vec![MigrationAction::AddColumn {
                table: "users".into(),
                column: Box::new(ColumnDef {
                    name: "status".into(),
                    r#type: ColumnType::Complex(ComplexColumnType::Enum {
                        name: "user_status".into(),
                        values: EnumValues::String(vec![
                            "active".into(),
                            "inactive".into(),
                            "pending".into(),
                        ]),
                    }),
                    nullable: false,
                    default: Some("'active'".into()),
                    comment: None,
                    primary_key: None,
                    unique: None,
                    index: None,
                    foreign_key: None,
                }),
                fill_with: None,
            }],
        };

        let result = validate_migration_plan(&plan);
        assert!(result.is_ok());
    }

    #[test]
    fn validate_enum_add_column_valid_default_unquoted() {
        let plan = MigrationPlan {
            comment: None,
            created_at: None,
            version: 1,
            actions: vec![MigrationAction::AddColumn {
                table: "users".into(),
                column: Box::new(ColumnDef {
                    name: "status".into(),
                    r#type: ColumnType::Complex(ComplexColumnType::Enum {
                        name: "user_status".into(),
                        values: EnumValues::String(vec![
                            "active".into(),
                            "inactive".into(),
                            "pending".into(),
                        ]),
                    }),
                    nullable: false,
                    default: Some("active".into()),
                    comment: None,
                    primary_key: None,
                    unique: None,
                    index: None,
                    foreign_key: None,
                }),
                fill_with: None,
            }],
        };

        let result = validate_migration_plan(&plan);
        assert!(result.is_ok());
    }

    #[test]
    fn validate_enum_add_column_valid_fill_with() {
        let plan = MigrationPlan {
            comment: None,
            created_at: None,
            version: 1,
            actions: vec![MigrationAction::AddColumn {
                table: "users".into(),
                column: Box::new(ColumnDef {
                    name: "status".into(),
                    r#type: ColumnType::Complex(ComplexColumnType::Enum {
                        name: "user_status".into(),
                        values: EnumValues::String(vec![
                            "active".into(),
                            "inactive".into(),
                            "pending".into(),
                        ]),
                    }),
                    nullable: false,
                    default: None,
                    comment: None,
                    primary_key: None,
                    unique: None,
                    index: None,
                    foreign_key: None,
                }),
                fill_with: Some("'pending'".into()),
            }],
        };

        let result = validate_migration_plan(&plan);
        assert!(result.is_ok());
    }

    #[test]
    fn validate_enum_schema_invalid_default() {
        // Test that schema validation also catches invalid enum defaults
        let schema = vec![table(
            "users",
            vec![col("id", ColumnType::Simple(SimpleColumnType::Integer)), {
                let mut c = col(
                    "status",
                    ColumnType::Complex(ComplexColumnType::Enum {
                        name: "user_status".into(),
                        values: EnumValues::String(vec!["active".into(), "inactive".into()]),
                    }),
                );
                c.default = Some("invalid".into());
                c
            }],
            vec![pk(vec!["id"])],
        )];

        let result = validate_schema(&schema);
        assert!(result.is_err());
        match result.unwrap_err() {
            PlannerError::InvalidEnumDefault(err) => {
                assert_eq!(err.enum_name, "user_status");
                assert_eq!(err.table_name, "users");
                assert_eq!(err.column_name, "status");
                assert_eq!(err.value_type, "default");
                assert_eq!(err.value, "invalid");
            }
            err => panic!("expected InvalidEnumDefault error, got {:?}", err),
        }
    }

    #[test]
    fn validate_enum_schema_valid_default() {
        let schema = vec![table(
            "users",
            vec![col("id", ColumnType::Simple(SimpleColumnType::Integer)), {
                let mut c = col(
                    "status",
                    ColumnType::Complex(ComplexColumnType::Enum {
                        name: "user_status".into(),
                        values: EnumValues::String(vec!["active".into(), "inactive".into()]),
                    }),
                );
                c.default = Some("'active'".into());
                c
            }],
            vec![pk(vec!["id"])],
        )];

        let result = validate_schema(&schema);
        assert!(result.is_ok());
    }

    #[test]
    fn validate_enum_integer_add_column_valid() {
        let plan = MigrationPlan {
            comment: None,
            created_at: None,
            version: 1,
            actions: vec![MigrationAction::AddColumn {
                table: "tasks".into(),
                column: Box::new(ColumnDef {
                    name: "priority".into(),
                    r#type: ColumnType::Complex(ComplexColumnType::Enum {
                        name: "priority_level".into(),
                        values: EnumValues::Integer(vec![
                            NumValue {
                                name: "Low".into(),
                                value: 0,
                            },
                            NumValue {
                                name: "Medium".into(),
                                value: 50,
                            },
                            NumValue {
                                name: "High".into(),
                                value: 100,
                            },
                        ]),
                    }),
                    nullable: false,
                    default: None,
                    comment: None,
                    primary_key: None,
                    unique: None,
                    index: None,
                    foreign_key: None,
                }),
                fill_with: Some("Low".into()),
            }],
        };

        let result = validate_migration_plan(&plan);
        assert!(result.is_ok());
    }

    #[test]
    fn validate_enum_integer_add_column_invalid() {
        let plan = MigrationPlan {
            comment: None,
            created_at: None,
            version: 1,
            actions: vec![MigrationAction::AddColumn {
                table: "tasks".into(),
                column: Box::new(ColumnDef {
                    name: "priority".into(),
                    r#type: ColumnType::Complex(ComplexColumnType::Enum {
                        name: "priority_level".into(),
                        values: EnumValues::Integer(vec![
                            NumValue {
                                name: "Low".into(),
                                value: 0,
                            },
                            NumValue {
                                name: "Medium".into(),
                                value: 50,
                            },
                            NumValue {
                                name: "High".into(),
                                value: 100,
                            },
                        ]),
                    }),
                    nullable: false,
                    default: None,
                    comment: None,
                    primary_key: None,
                    unique: None,
                    index: None,
                    foreign_key: None,
                }),
                fill_with: Some("Critical".into()), // Not a valid enum name
            }],
        };

        let result = validate_migration_plan(&plan);
        assert!(result.is_err());
        match result.unwrap_err() {
            PlannerError::InvalidEnumDefault(err) => {
                assert_eq!(err.enum_name, "priority_level");
                assert_eq!(err.table_name, "tasks");
                assert_eq!(err.column_name, "priority");
                assert_eq!(err.value_type, "fill_with");
                assert_eq!(err.value, "Critical");
            }
            err => panic!("expected InvalidEnumDefault error, got {:?}", err),
        }
    }

    #[test]
    fn validate_enum_null_value_skipped() {
        // NULL values should be allowed and skipped during validation
        let plan = MigrationPlan {
            comment: None,
            created_at: None,
            version: 1,
            actions: vec![MigrationAction::AddColumn {
                table: "users".into(),
                column: Box::new(ColumnDef {
                    name: "status".into(),
                    r#type: ColumnType::Complex(ComplexColumnType::Enum {
                        name: "user_status".into(),
                        values: EnumValues::String(vec!["active".into(), "inactive".into()]),
                    }),
                    nullable: true,
                    default: Some("NULL".into()),
                    comment: None,
                    primary_key: None,
                    unique: None,
                    index: None,
                    foreign_key: None,
                }),
                fill_with: None,
            }],
        };

        let result = validate_migration_plan(&plan);
        assert!(result.is_ok());
    }

    #[test]
    fn validate_enum_sql_expression_skipped() {
        // SQL expressions like function calls should be skipped
        let plan = MigrationPlan {
            comment: None,
            created_at: None,
            version: 1,
            actions: vec![MigrationAction::AddColumn {
                table: "users".into(),
                column: Box::new(ColumnDef {
                    name: "status".into(),
                    r#type: ColumnType::Complex(ComplexColumnType::Enum {
                        name: "user_status".into(),
                        values: EnumValues::String(vec!["active".into(), "inactive".into()]),
                    }),
                    nullable: true,
                    default: None,
                    comment: None,
                    primary_key: None,
                    unique: None,
                    index: None,
                    foreign_key: None,
                }),
                fill_with: Some("COALESCE(old_status, 'active')".into()),
            }],
        };

        let result = validate_migration_plan(&plan);
        assert!(result.is_ok());
    }

    #[test]
    fn validate_enum_empty_string_fill_with_skipped() {
        // Empty string fill_with should be skipped during enum validation
        // (converted to '' by to_sql, which is empty after trimming)
        let plan = MigrationPlan {
            comment: None,
            created_at: None,
            version: 1,
            actions: vec![MigrationAction::AddColumn {
                table: "users".into(),
                column: Box::new(ColumnDef {
                    name: "status".into(),
                    r#type: ColumnType::Complex(ComplexColumnType::Enum {
                        name: "user_status".into(),
                        values: EnumValues::String(vec!["active".into(), "inactive".into()]),
                    }),
                    nullable: true,
                    default: None,
                    comment: None,
                    primary_key: None,
                    unique: None,
                    index: None,
                    foreign_key: None,
                }),
                // Empty string - extract_enum_value returns None for empty trimmed values
                fill_with: Some("   ".into()),
            }],
        };

        let result = validate_migration_plan(&plan);
        assert!(result.is_ok());
    }

    // Tests for find_missing_fill_with function
    #[test]
    fn find_missing_fill_with_add_column_not_null_no_default() {
        let plan = MigrationPlan {
            comment: None,
            created_at: None,
            version: 1,
            actions: vec![MigrationAction::AddColumn {
                table: "users".into(),
                column: Box::new(ColumnDef {
                    name: "email".into(),
                    r#type: ColumnType::Simple(SimpleColumnType::Text),
                    nullable: false,
                    default: None,
                    comment: None,
                    primary_key: None,
                    unique: None,
                    index: None,
                    foreign_key: None,
                }),
                fill_with: None,
            }],
        };

        let missing = find_missing_fill_with(&plan);
        assert_eq!(missing.len(), 1);
        assert_eq!(missing[0].table, "users");
        assert_eq!(missing[0].column, "email");
        assert_eq!(missing[0].action_type, "AddColumn");
        assert!(missing[0].column_type.is_some());
    }

    #[test]
    fn find_missing_fill_with_add_column_with_default() {
        let plan = MigrationPlan {
            comment: None,
            created_at: None,
            version: 1,
            actions: vec![MigrationAction::AddColumn {
                table: "users".into(),
                column: Box::new(ColumnDef {
                    name: "email".into(),
                    r#type: ColumnType::Simple(SimpleColumnType::Text),
                    nullable: false,
                    default: Some("'default@example.com'".into()),
                    comment: None,
                    primary_key: None,
                    unique: None,
                    index: None,
                    foreign_key: None,
                }),
                fill_with: None,
            }],
        };

        let missing = find_missing_fill_with(&plan);
        assert!(missing.is_empty());
    }

    #[test]
    fn find_missing_fill_with_add_column_nullable() {
        let plan = MigrationPlan {
            comment: None,
            created_at: None,
            version: 1,
            actions: vec![MigrationAction::AddColumn {
                table: "users".into(),
                column: Box::new(ColumnDef {
                    name: "email".into(),
                    r#type: ColumnType::Simple(SimpleColumnType::Text),
                    nullable: true,
                    default: None,
                    comment: None,
                    primary_key: None,
                    unique: None,
                    index: None,
                    foreign_key: None,
                }),
                fill_with: None,
            }],
        };

        let missing = find_missing_fill_with(&plan);
        assert!(missing.is_empty());
    }

    #[test]
    fn find_missing_fill_with_add_column_with_fill_with() {
        let plan = MigrationPlan {
            comment: None,
            created_at: None,
            version: 1,
            actions: vec![MigrationAction::AddColumn {
                table: "users".into(),
                column: Box::new(ColumnDef {
                    name: "email".into(),
                    r#type: ColumnType::Simple(SimpleColumnType::Text),
                    nullable: false,
                    default: None,
                    comment: None,
                    primary_key: None,
                    unique: None,
                    index: None,
                    foreign_key: None,
                }),
                fill_with: Some("'default@example.com'".into()),
            }],
        };

        let missing = find_missing_fill_with(&plan);
        assert!(missing.is_empty());
    }

    #[test]
    fn find_missing_fill_with_modify_nullable_to_not_null() {
        let plan = MigrationPlan {
            comment: None,
            created_at: None,
            version: 1,
            actions: vec![MigrationAction::ModifyColumnNullable {
                table: "users".into(),
                column: "email".into(),
                nullable: false,
                fill_with: None,
            }],
        };

        let missing = find_missing_fill_with(&plan);
        assert_eq!(missing.len(), 1);
        assert_eq!(missing[0].table, "users");
        assert_eq!(missing[0].column, "email");
        assert_eq!(missing[0].action_type, "ModifyColumnNullable");
        assert!(missing[0].column_type.is_none());
    }

    #[test]
    fn find_missing_fill_with_modify_to_nullable() {
        let plan = MigrationPlan {
            comment: None,
            created_at: None,
            version: 1,
            actions: vec![MigrationAction::ModifyColumnNullable {
                table: "users".into(),
                column: "email".into(),
                nullable: true,
                fill_with: None,
            }],
        };

        let missing = find_missing_fill_with(&plan);
        assert!(missing.is_empty());
    }

    #[test]
    fn find_missing_fill_with_modify_not_null_with_fill_with() {
        let plan = MigrationPlan {
            comment: None,
            created_at: None,
            version: 1,
            actions: vec![MigrationAction::ModifyColumnNullable {
                table: "users".into(),
                column: "email".into(),
                nullable: false,
                fill_with: Some("'default'".into()),
            }],
        };

        let missing = find_missing_fill_with(&plan);
        assert!(missing.is_empty());
    }

    #[test]
    fn find_missing_fill_with_multiple_actions() {
        let plan = MigrationPlan {
            comment: None,
            created_at: None,
            version: 1,
            actions: vec![
                MigrationAction::AddColumn {
                    table: "users".into(),
                    column: Box::new(ColumnDef {
                        name: "email".into(),
                        r#type: ColumnType::Simple(SimpleColumnType::Text),
                        nullable: false,
                        default: None,
                        comment: None,
                        primary_key: None,
                        unique: None,
                        index: None,
                        foreign_key: None,
                    }),
                    fill_with: None,
                },
                MigrationAction::ModifyColumnNullable {
                    table: "orders".into(),
                    column: "status".into(),
                    nullable: false,
                    fill_with: None,
                },
                MigrationAction::AddColumn {
                    table: "users".into(),
                    column: Box::new(ColumnDef {
                        name: "name".into(),
                        r#type: ColumnType::Simple(SimpleColumnType::Text),
                        nullable: true, // nullable, so not missing
                        default: None,
                        comment: None,
                        primary_key: None,
                        unique: None,
                        index: None,
                        foreign_key: None,
                    }),
                    fill_with: None,
                },
            ],
        };

        let missing = find_missing_fill_with(&plan);
        assert_eq!(missing.len(), 2);
        assert_eq!(missing[0].action_index, 0);
        assert_eq!(missing[0].table, "users");
        assert_eq!(missing[0].column, "email");
        assert_eq!(missing[1].action_index, 1);
        assert_eq!(missing[1].table, "orders");
        assert_eq!(missing[1].column, "status");
    }

    #[test]
    fn find_missing_fill_with_other_actions_ignored() {
        let plan = MigrationPlan {
            comment: None,
            created_at: None,
            version: 1,
            actions: vec![
                MigrationAction::CreateTable {
                    table: "users".into(),
                    columns: vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
                    constraints: vec![pk(vec!["id"])],
                },
                MigrationAction::DeleteColumn {
                    table: "orders".into(),
                    column: "old_column".into(),
                },
            ],
        };

        let missing = find_missing_fill_with(&plan);
        assert!(missing.is_empty());
    }

    #[test]
    fn validate_auto_increment_on_text_column_fails() {
        let table_def = table(
            "users",
            vec![col("id", ColumnType::Simple(SimpleColumnType::Text))],
            vec![TableConstraint::PrimaryKey {
                auto_increment: true,
                columns: vec!["id".into()],
            }],
        );

        let result = validate_table(&table_def, &std::collections::HashMap::new());
        assert!(result.is_err());
        match result {
            Err(PlannerError::InvalidAutoIncrement(table_name, col_name, _)) => {
                assert_eq!(table_name, "users");
                assert_eq!(col_name, "id");
            }
            _ => panic!("Expected InvalidAutoIncrement error"),
        }
    }

    #[test]
    fn validate_auto_increment_on_integer_column_succeeds() {
        let table_def = table(
            "users",
            vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
            vec![TableConstraint::PrimaryKey {
                auto_increment: true,
                columns: vec!["id".into()],
            }],
        );

        let result = validate_table(&table_def, &std::collections::HashMap::new());
        assert!(result.is_ok());
    }

    #[test]
    fn validate_inline_auto_increment_on_text_column_fails() {
        let mut col_def = col("id", ColumnType::Simple(SimpleColumnType::Text));
        col_def.primary_key = Some(PrimaryKeySyntax::Object(PrimaryKeyDef {
            auto_increment: true,
        }));

        let table_def = table("users", vec![col_def], vec![]);

        let result = validate_table(&table_def, &std::collections::HashMap::new());
        assert!(result.is_err());
        match result {
            Err(PlannerError::InvalidAutoIncrement(table_name, col_name, _)) => {
                assert_eq!(table_name, "users");
                assert_eq!(col_name, "id");
            }
            _ => panic!("Expected InvalidAutoIncrement error"),
        }
    }
}
