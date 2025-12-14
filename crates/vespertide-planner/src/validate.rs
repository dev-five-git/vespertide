use std::collections::HashSet;

use vespertide_core::{IndexDef, MigrationAction, MigrationPlan, TableConstraint, TableDef};

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

    // Validate constraints
    for constraint in &table.constraints {
        validate_constraint(constraint, &table.name, &table_columns, table_map)?;
    }

    // Validate indexes
    for index in &table.indexes {
        validate_index(index, &table.name, &table_columns)?;
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
    }

    Ok(())
}

fn validate_index(
    index: &IndexDef,
    table_name: &str,
    table_columns: &HashSet<&str>,
) -> Result<(), PlannerError> {
    if index.columns.is_empty() {
        return Err(PlannerError::EmptyConstraintColumns(
            table_name.to_string(),
            format!("Index({})", index.name),
        ));
    }

    for col in &index.columns {
        if !table_columns.contains(col.as_str()) {
            return Err(PlannerError::IndexColumnNotFound(
                table_name.to_string(),
                index.name.clone(),
                col.clone(),
            ));
        }
    }

    Ok(())
}

/// Validate a migration plan for correctness.
/// Checks for:
/// - AddColumn actions with NOT NULL columns without default must have fill_with
pub fn validate_migration_plan(plan: &MigrationPlan) -> Result<(), PlannerError> {
    for action in &plan.actions {
        if let MigrationAction::AddColumn {
            table,
            column,
            fill_with,
        } = action
        {
            // If column is NOT NULL and has no default, fill_with is required
            if !column.nullable && column.default.is_none() && fill_with.is_none() {
                return Err(PlannerError::MissingFillWith(
                    table.clone(),
                    column.name.clone(),
                ));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;
    use vespertide_core::{ColumnDef, ColumnType, IndexDef, SimpleColumnType, TableConstraint};

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

    fn table(
        name: &str,
        columns: Vec<ColumnDef>,
        constraints: Vec<TableConstraint>,
        indexes: Vec<IndexDef>,
    ) -> TableDef {
        TableDef {
            name: name.to_string(),
            columns,
            constraints,
            indexes,
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

    #[rstest]
    #[case::valid_schema(
        vec![table(
            "users",
            vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
            vec![TableConstraint::PrimaryKey{ auto_increment: false, columns: vec!["id".into()] }],
            vec![],
        )],
        None
    )]
    #[case::duplicate_table(
        vec![
            table("users", vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))], vec![], vec![]),
            table("users", vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))], vec![], vec![]),
        ],
        Some(is_duplicate as fn(&PlannerError) -> bool)
    )]
    #[case::fk_missing_table(
        vec![table(
            "users",
            vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
            vec![TableConstraint::ForeignKey {
                name: None,
                columns: vec!["id".into()],
                ref_table: "nonexistent".into(),
                ref_columns: vec!["id".into()],
                on_delete: None,
                on_update: None,
            }],
            vec![],
        )],
        Some(is_fk_table as fn(&PlannerError) -> bool)
    )]
    #[case::fk_missing_column(
        vec![
            table("posts", vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))], vec![], vec![]),
            table(
                "users",
                vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
                vec![TableConstraint::ForeignKey {
                    name: None,
                    columns: vec!["id".into()],
                    ref_table: "posts".into(),
                    ref_columns: vec!["nonexistent".into()],
                    on_delete: None,
                    on_update: None,
                }],
                vec![],
            ),
        ],
        Some(is_fk_column as fn(&PlannerError) -> bool)
    )]
    #[case::fk_local_missing_column(
        vec![
            table("posts", vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))], vec![], vec![]),
            table(
                "users",
                vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
                vec![TableConstraint::ForeignKey {
                    name: None,
                    columns: vec!["missing".into()],
                    ref_table: "posts".into(),
                    ref_columns: vec!["id".into()],
                    on_delete: None,
                    on_update: None,
                }],
                vec![],
            ),
        ],
        Some(is_constraint_column as fn(&PlannerError) -> bool)
    )]
    #[case::fk_valid(
        vec![
            table(
                "posts",
                vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
                vec![TableConstraint::PrimaryKey{ auto_increment: false, columns: vec!["id".into()] }],
                vec![],
            ),
            table(
                "users",
                vec![col("id", ColumnType::Simple(SimpleColumnType::Integer)), col("post_id", ColumnType::Simple(SimpleColumnType::Integer))],
                vec![TableConstraint::ForeignKey {
                    name: None,
                    columns: vec!["post_id".into()],
                    ref_table: "posts".into(),
                    ref_columns: vec!["id".into()],
                    on_delete: None,
                    on_update: None,
                }],
                vec![],
            ),
        ],
        None
    )]
    #[case::index_missing_column(
        vec![table(
            "users",
            vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
            vec![],
            vec![IndexDef {
                name: "idx_name".into(),
                columns: vec!["nonexistent".into()],
                unique: false,
            }],
        )],
        Some(is_index_column as fn(&PlannerError) -> bool)
    )]
    #[case::constraint_missing_column(
        vec![table(
            "users",
            vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
            vec![TableConstraint::PrimaryKey{ auto_increment: false, columns: vec!["nonexistent".into()] }],
            vec![],
        )],
        Some(is_constraint_column as fn(&PlannerError) -> bool)
    )]
    #[case::unique_empty_columns(
        vec![table(
            "users",
            vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
            vec![TableConstraint::Unique {
                name: Some("u".into()),
                columns: vec![],
            }],
            vec![],
        )],
        Some(is_empty_columns as fn(&PlannerError) -> bool)
    )]
    #[case::unique_missing_column(
        vec![table(
            "users",
            vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
            vec![TableConstraint::Unique {
                name: None,
                columns: vec!["missing".into()],
            }],
            vec![],
        )],
        Some(is_constraint_column as fn(&PlannerError) -> bool)
    )]
    #[case::empty_primary_key(
        vec![table(
            "users",
            vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
            vec![TableConstraint::PrimaryKey{ auto_increment: false, columns: vec![] }],
            vec![],
        )],
        Some(is_empty_columns as fn(&PlannerError) -> bool)
    )]
    #[case::fk_column_count_mismatch(
        vec![
            table(
                "posts",
                vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
                vec![],
                vec![],
            ),
            table(
                "users",
                vec![col("id", ColumnType::Simple(SimpleColumnType::Integer)), col("post_id", ColumnType::Simple(SimpleColumnType::Integer))],
                vec![TableConstraint::ForeignKey {
                    name: None,
                    columns: vec!["id".into(), "post_id".into()],
                    ref_table: "posts".into(),
                    ref_columns: vec!["id".into()],
                    on_delete: None,
                    on_update: None,
                }],
                vec![],
            ),
        ],
        Some(is_fk_column as fn(&PlannerError) -> bool)
    )]
    #[case::fk_empty_columns(
        vec![table(
            "users",
            vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
            vec![TableConstraint::ForeignKey {
                name: None,
                columns: vec![],
                ref_table: "posts".into(),
                ref_columns: vec!["id".into()],
                on_delete: None,
                on_update: None,
            }],
            vec![],
        )],
        Some(is_empty_columns as fn(&PlannerError) -> bool)
    )]
    #[case::fk_empty_ref_columns(
        vec![
            table(
                "posts",
                vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
                vec![],
                vec![],
            ),
            table(
                "users",
                vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
                vec![TableConstraint::ForeignKey {
                    name: None,
                    columns: vec!["id".into()],
                    ref_table: "posts".into(),
                    ref_columns: vec![],
                    on_delete: None,
                    on_update: None,
                }],
                vec![],
            ),
        ],
        Some(is_empty_columns as fn(&PlannerError) -> bool)
    )]
    #[case::index_empty_columns(
        vec![table(
            "users",
            vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
            vec![],
            vec![IndexDef {
                name: "idx".into(),
                columns: vec![],
                unique: false,
            }],
        )],
        Some(is_empty_columns as fn(&PlannerError) -> bool)
    )]
    #[case::index_valid(
        vec![table(
            "users",
            vec![col("id", ColumnType::Simple(SimpleColumnType::Integer)), col("name", ColumnType::Simple(SimpleColumnType::Text))],
            vec![],
            vec![IndexDef {
                name: "idx_name".into(),
                columns: vec!["name".into()],
                unique: false,
            }],
        )],
        None
    )]
    #[case::check_constraint_ok(
        vec![table(
            "users",
            vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
            vec![TableConstraint::Check {
                name: Some("ck".into()),
                expr: "id > 0".into(),
            }],
            vec![],
        )],
        None
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
                column: ColumnDef {
                    name: "email".into(),
                    r#type: ColumnType::Simple(SimpleColumnType::Text),
                    nullable: false,
                    default: None,
                    comment: None,
                    primary_key: None,
                    unique: None,
                    index: None,
                    foreign_key: None,
                },
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
                column: ColumnDef {
                    name: "email".into(),
                    r#type: ColumnType::Simple(SimpleColumnType::Text),
                    nullable: false,
                    default: None,
                    comment: None,
                    primary_key: None,
                    unique: None,
                    index: None,
                    foreign_key: None,
                },
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
                column: ColumnDef {
                    name: "email".into(),
                    r#type: ColumnType::Simple(SimpleColumnType::Text),
                    nullable: true,
                    default: None,
                    comment: None,
                    primary_key: None,
                    unique: None,
                    index: None,
                    foreign_key: None,
                },
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
                column: ColumnDef {
                    name: "email".into(),
                    r#type: ColumnType::Simple(SimpleColumnType::Text),
                    nullable: false,
                    default: Some("default@example.com".into()),
                    comment: None,
                    primary_key: None,
                    unique: None,
                    index: None,
                    foreign_key: None,
                },
                fill_with: None,
            }],
        };

        let result = validate_migration_plan(&plan);
        assert!(result.is_ok());
    }
}
