use std::collections::HashSet;

use vespertide_core::{
    ColumnDef, ColumnType, ComplexColumnType, EnumValues, MigrationAction, MigrationPlan,
    TableConstraint, TableDef,
};

use crate::error::PlannerError;

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

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;
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
}
