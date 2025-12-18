use vespertide_core::{IndexDef, MigrationAction, TableConstraint, TableDef};

use crate::error::PlannerError;

/// Apply a single migration action to an in-memory schema snapshot.
pub fn apply_action(
    schema: &mut Vec<TableDef>,
    action: &MigrationAction,
) -> Result<(), PlannerError> {
    match action {
        MigrationAction::CreateTable {
            table,
            columns,
            constraints,
        } => {
            if schema.iter().any(|t| t.name == *table) {
                return Err(PlannerError::TableExists(table.clone()));
            }
            schema.push(TableDef {
                name: table.clone(),
                columns: columns.clone(),
                constraints: constraints.clone(),
                indexes: Vec::new(),
            });
            Ok(())
        }
        MigrationAction::DeleteTable { table } => {
            let before = schema.len();
            schema.retain(|t| t.name != *table);
            if schema.len() == before {
                Err(PlannerError::TableNotFound(table.clone()))
            } else {
                Ok(())
            }
        }
        MigrationAction::AddColumn {
            table,
            column,
            fill_with: _,
        } => {
            let tbl = schema
                .iter_mut()
                .find(|t| t.name == *table)
                .ok_or_else(|| PlannerError::TableNotFound(table.clone()))?;
            if tbl.columns.iter().any(|c| c.name == column.name) {
                Err(PlannerError::ColumnExists(
                    table.clone(),
                    column.name.clone(),
                ))
            } else {
                tbl.columns.push((**column).clone());
                Ok(())
            }
        }
        MigrationAction::RenameColumn { table, from, to } => {
            let tbl = schema
                .iter_mut()
                .find(|t| t.name == *table)
                .ok_or_else(|| PlannerError::TableNotFound(table.clone()))?;
            let col = tbl
                .columns
                .iter_mut()
                .find(|c| c.name == *from)
                .ok_or_else(|| PlannerError::ColumnNotFound(table.clone(), from.clone()))?;
            col.name = to.clone();
            rename_column_in_constraints(&mut tbl.constraints, from, to);
            rename_column_in_indexes(&mut tbl.indexes, from, to);
            Ok(())
        }
        MigrationAction::DeleteColumn { table, column } => {
            let tbl = schema
                .iter_mut()
                .find(|t| t.name == *table)
                .ok_or_else(|| PlannerError::TableNotFound(table.clone()))?;
            let before = tbl.columns.len();
            tbl.columns.retain(|c| c.name != *column);
            if tbl.columns.len() == before {
                Err(PlannerError::ColumnNotFound(table.clone(), column.clone()))
            } else {
                drop_column_from_constraints(&mut tbl.constraints, column);
                drop_column_from_indexes(&mut tbl.indexes, column);
                Ok(())
            }
        }
        MigrationAction::ModifyColumnType {
            table,
            column,
            new_type,
        } => {
            let tbl = schema
                .iter_mut()
                .find(|t| t.name == *table)
                .ok_or_else(|| PlannerError::TableNotFound(table.clone()))?;
            let col = tbl
                .columns
                .iter_mut()
                .find(|c| c.name == *column)
                .ok_or_else(|| PlannerError::ColumnNotFound(table.clone(), column.clone()))?;
            col.r#type = new_type.clone();
            Ok(())
        }
        MigrationAction::AddIndex { table, index } => {
            let tbl = schema
                .iter_mut()
                .find(|t| t.name == *table)
                .ok_or_else(|| PlannerError::TableNotFound(table.clone()))?;
            tbl.indexes.push(index.clone());
            Ok(())
        }
        MigrationAction::RemoveIndex { table, name } => {
            let tbl = schema
                .iter_mut()
                .find(|t| t.name == *table)
                .ok_or_else(|| PlannerError::TableNotFound(table.clone()))?;
            let before = tbl.indexes.len();
            tbl.indexes.retain(|i| i.name != *name);

            // Also clear inline index on column if index name matches the auto-generated pattern
            // Pattern: idx_{table}_{column} for Bool(true) or the name itself for Str(name)
            let prefix = format!("idx_{}_", table);
            if let Some(col_name) = name.strip_prefix(&prefix) {
                // This is an auto-generated index name - clear the inline index on that column
                if let Some(col) = tbl.columns.iter_mut().find(|c| c.name == col_name) {
                    col.index = None;
                }
            }
            // Also check if any column has a named index matching this name
            for col in &mut tbl.columns {
                if let Some(ref idx_val) = col.index {
                    match idx_val {
                        vespertide_core::StrOrBoolOrArray::Str(idx_name) if idx_name == name => {
                            col.index = None;
                        }
                        vespertide_core::StrOrBoolOrArray::Array(names) => {
                            let filtered: Vec<_> =
                                names.iter().filter(|n| *n != name).cloned().collect();
                            if filtered.is_empty() {
                                col.index = None;
                            } else if filtered.len() < names.len() {
                                col.index =
                                    Some(vespertide_core::StrOrBoolOrArray::Array(filtered));
                            }
                        }
                        _ => {}
                    }
                }
            }

            if tbl.indexes.len() == before {
                Err(PlannerError::IndexNotFound(table.clone(), name.clone()))
            } else {
                Ok(())
            }
        }
        MigrationAction::RenameTable { from, to } => {
            if schema.iter().any(|t| t.name == *to) {
                Err(PlannerError::TableExists(to.clone()))
            } else {
                let tbl = schema
                    .iter_mut()
                    .find(|t| t.name == *from)
                    .ok_or_else(|| PlannerError::TableNotFound(from.clone()))?;
                tbl.name = to.clone();
                Ok(())
            }
        }
        MigrationAction::RawSql { .. } => Ok(()), // Does not mutate in-memory schema; allowed as side-effect-only
        MigrationAction::AddConstraint { table, constraint } => {
            let tbl = schema
                .iter_mut()
                .find(|t| t.name == *table)
                .ok_or_else(|| PlannerError::TableNotFound(table.clone()))?;
            tbl.constraints.push(constraint.clone());
            Ok(())
        }
        MigrationAction::RemoveConstraint { table, constraint } => {
            let tbl = schema
                .iter_mut()
                .find(|t| t.name == *table)
                .ok_or_else(|| PlannerError::TableNotFound(table.clone()))?;
            tbl.constraints.retain(|c| c != constraint);

            // Also clear inline column fields that correspond to the removed constraint
            // This ensures normalize() won't re-add the constraint from inline fields
            match constraint {
                TableConstraint::Unique { name, columns } => {
                    // For unnamed single-column unique constraints, clear the column's inline unique
                    if name.is_none()
                        && columns.len() == 1
                        && let Some(col) = tbl.columns.iter_mut().find(|c| c.name == columns[0])
                    {
                        col.unique = None;
                    }
                    // For named constraints, clear inline unique references to this constraint name
                    if let Some(constraint_name) = name {
                        for col in &mut tbl.columns {
                            if let Some(vespertide_core::StrOrBoolOrArray::Array(names)) =
                                &mut col.unique
                            {
                                names.retain(|n| n != constraint_name);
                                if names.is_empty() {
                                    col.unique = None;
                                }
                            } else if let Some(vespertide_core::StrOrBoolOrArray::Str(n)) =
                                &col.unique
                                && n == constraint_name
                            {
                                col.unique = None;
                            }
                        }
                    }
                }
                TableConstraint::PrimaryKey { columns, .. } => {
                    // Clear inline primary_key for columns in this constraint
                    for col_name in columns {
                        if let Some(col) = tbl.columns.iter_mut().find(|c| &c.name == col_name) {
                            col.primary_key = None;
                        }
                    }
                }
                TableConstraint::ForeignKey { columns, .. } => {
                    // Clear inline foreign_key for columns in this constraint
                    for col_name in columns {
                        if let Some(col) = tbl.columns.iter_mut().find(|c| &c.name == col_name) {
                            col.foreign_key = None;
                        }
                    }
                }
                TableConstraint::Check { .. } => {
                    // Check constraints don't have inline representation
                }
            }
            Ok(())
        }
    }
}

fn rename_column_in_constraints(constraints: &mut [TableConstraint], from: &str, to: &str) {
    for constraint in constraints {
        match constraint {
            TableConstraint::PrimaryKey { columns, .. } => {
                for c in columns.iter_mut() {
                    if c == from {
                        *c = to.to_string();
                    }
                }
            }
            TableConstraint::Unique { columns, .. } => {
                for c in columns.iter_mut() {
                    if c == from {
                        *c = to.to_string();
                    }
                }
            }
            TableConstraint::ForeignKey {
                columns,
                ref_columns,
                ..
            } => {
                for c in columns.iter_mut() {
                    if c == from {
                        *c = to.to_string();
                    }
                }
                for c in ref_columns.iter_mut() {
                    if c == from {
                        *c = to.to_string();
                    }
                }
            }
            TableConstraint::Check { .. } => {}
        }
    }
}

fn rename_column_in_indexes(indexes: &mut [IndexDef], from: &str, to: &str) {
    for idx in indexes {
        for c in idx.columns.iter_mut() {
            if c == from {
                *c = to.to_string();
            }
        }
    }
}

fn drop_column_from_constraints(constraints: &mut Vec<TableConstraint>, column: &str) {
    constraints.retain_mut(|c| match c {
        TableConstraint::PrimaryKey { columns, .. } => {
            columns.retain(|c| c != column);
            !columns.is_empty()
        }
        TableConstraint::Unique { columns, .. } => {
            columns.retain(|c| c != column);
            !columns.is_empty()
        }
        TableConstraint::ForeignKey {
            columns,
            ref_columns,
            ..
        } => {
            columns.retain(|c| c != column);
            ref_columns.retain(|c| c != column);
            !columns.is_empty() && !ref_columns.is_empty()
        }
        TableConstraint::Check { .. } => true,
    });
}

fn drop_column_from_indexes(indexes: &mut Vec<IndexDef>, column: &str) {
    indexes.retain(|idx| !idx.columns.iter().any(|c| c == column));
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;
    use vespertide_core::{ColumnDef, ColumnType, SimpleColumnType};

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

    #[derive(Debug, Clone, Copy)]
    enum ErrKind {
        TableExists,
        TableNotFound,
        ColumnExists,
        ColumnNotFound,
        IndexNotFound,
    }

    fn assert_err_kind(err: crate::error::PlannerError, kind: ErrKind) {
        match (err, kind) {
            (crate::error::PlannerError::TableExists(_), ErrKind::TableExists) => {}
            (crate::error::PlannerError::TableNotFound(_), ErrKind::TableNotFound) => {}
            (crate::error::PlannerError::ColumnExists(_, _), ErrKind::ColumnExists) => {}
            (crate::error::PlannerError::ColumnNotFound(_, _), ErrKind::ColumnNotFound) => {}
            (crate::error::PlannerError::IndexNotFound(_, _), ErrKind::IndexNotFound) => {}
            (other, expected) => panic!("unexpected error {other:?}, expected {:?}", expected),
        }
    }

    #[rstest]
    #[case(
        vec![table("users", vec![], vec![], vec![])],
        MigrationAction::CreateTable {
            table: "users".into(),
            columns: vec![],
            constraints: vec![],
        },
        ErrKind::TableExists
    )]
    #[case(
        vec![],
        MigrationAction::DeleteTable {
            table: "users".into()
        },
        ErrKind::TableNotFound
    )]
    #[case(
        vec![table(
            "users",
            vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
            vec![],
            vec![]
        )],
        MigrationAction::AddColumn {
            table: "users".into(),
            column: Box::new(col("id", ColumnType::Simple(SimpleColumnType::Integer))),
            fill_with: None,
        },
        ErrKind::ColumnExists
    )]
    #[case(
        vec![table(
            "users",
            vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
            vec![],
            vec![]
        )],
        MigrationAction::DeleteColumn {
            table: "users".into(),
            column: "missing".into()
        },
        ErrKind::ColumnNotFound
    )]
    #[case(
        vec![table(
            "users",
            vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
            vec![],
            vec![]
        )],
        MigrationAction::RemoveIndex {
            table: "users".into(),
            name: "idx".into()
        },
        ErrKind::IndexNotFound
    )]
    #[case(
        vec![
            table("old", vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))], vec![], vec![]),
            table("new", vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))], vec![], vec![]),
        ],
        MigrationAction::RenameTable {
            from: "old".into(),
            to: "new".into()
        },
        ErrKind::TableExists
    )]
    fn apply_action_reports_errors(
        #[case] mut schema: Vec<TableDef>,
        #[case] action: MigrationAction,
        #[case] expected: ErrKind,
    ) {
        let err = apply_action(&mut schema, &action).unwrap_err();
        assert_err_kind(err, expected);
    }

    fn idx(name: &str, columns: Vec<&str>, unique: bool) -> IndexDef {
        IndexDef {
            name: name.to_string(),
            columns: columns.into_iter().map(|s| s.to_string()).collect(),
            unique,
        }
    }

    #[derive(Clone)]
    struct SuccessCase {
        initial: Vec<TableDef>,
        actions: Vec<MigrationAction>,
        expected: Vec<TableDef>,
    }

    #[rstest]
    #[case(SuccessCase {
        initial: vec![],
        actions: vec![
            MigrationAction::CreateTable {
                table: "users".into(),
                columns: vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
                constraints: vec![],
            },
            MigrationAction::DeleteTable {
                table: "users".into(),
            },
        ],
        expected: vec![],
    })]
    #[case(SuccessCase {
        initial: vec![table(
            "users",
            vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                col("old", ColumnType::Simple(SimpleColumnType::Text)),
                col("ref_id", ColumnType::Simple(SimpleColumnType::Integer))
            ],
            vec![
                TableConstraint::PrimaryKey{ auto_increment: false, columns: vec!["id".into()] },
                TableConstraint::Unique {
                    name: Some("u_old".into()),
                    columns: vec!["old".into()],
                },
                TableConstraint::ForeignKey {
                    name: Some("fk_old".into()),
                    columns: vec!["old".into()],
                    ref_table: "ref_table".into(),
                    ref_columns: vec!["ref_id".into()],
                    on_delete: None,
                    on_update: None,
                },
                TableConstraint::Check {
                    name: "ck_old".into(),
                    expr: "old IS NOT NULL".into(),
                },
            ],
            vec![
                idx("idx_old", vec!["old"], false),
                idx("idx_ref", vec!["ref_id"], false),
            ],
        )],
        actions: vec![
            MigrationAction::AddColumn {
                table: "users".into(),
                column: Box::new(col("new_col", ColumnType::Simple(SimpleColumnType::Boolean))),
                fill_with: None,
            },
            MigrationAction::RenameColumn {
                table: "users".into(),
                from: "ref_id".into(),
                to: "renamed".into(),
            },
        ],
        expected: vec![table(
            "users",
            vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                col("old", ColumnType::Simple(SimpleColumnType::Text)),
                col("renamed", ColumnType::Simple(SimpleColumnType::Integer)),
                col("new_col", ColumnType::Simple(SimpleColumnType::Boolean))
            ],
            vec![
                TableConstraint::PrimaryKey{ auto_increment: false, columns: vec!["id".into()] },
                TableConstraint::Unique {
                    name: Some("u_old".into()),
                    columns: vec!["old".into()],
                },
                TableConstraint::ForeignKey {
                    name: Some("fk_old".into()),
                    columns: vec!["old".into()],
                    ref_table: "ref_table".into(),
                    ref_columns: vec!["renamed".into()],
                    on_delete: None,
                    on_update: None,
                },
                TableConstraint::Check {
                    name: "ck_old".into(),
                    expr: "old IS NOT NULL".into(),
                },
            ],
            vec![
                idx("idx_old", vec!["old"], false),
                idx("idx_ref", vec!["renamed"], false),
            ],
        )],
    })]
    #[case(SuccessCase {
        initial: vec![table(
            "users",
            vec![col("id", ColumnType::Simple(SimpleColumnType::Integer)), col("old", ColumnType::Simple(SimpleColumnType::Text))],
            vec![
                TableConstraint::PrimaryKey{ auto_increment: false, columns: vec!["id".into()] },
                TableConstraint::Unique {
                    name: Some("u_old".into()),
                    columns: vec!["old".into()],
                },
                TableConstraint::ForeignKey {
                    name: Some("fk_old".into()),
                    columns: vec!["old".into()],
                    ref_table: "ref_table".into(),
                    ref_columns: vec!["old".into()],
                    on_delete: None,
                    on_update: None,
                },
                TableConstraint::Check {
                    name: "ck_old".into(),
                    expr: "old IS NOT NULL".into(),
                },
            ],
            vec![idx("idx_old", vec!["old"], false)],
        )],
        actions: vec![MigrationAction::DeleteColumn {
            table: "users".into(),
            column: "old".into(),
        }],
        expected: vec![table(
            "users",
            vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
            vec![
                TableConstraint::PrimaryKey{ auto_increment: false, columns: vec!["id".into()] },
                TableConstraint::Check {
                    name: "ck_old".into(),
                    expr: "old IS NOT NULL".into(),
                },
            ],
            vec![],
        )],
    })]
    #[case(SuccessCase {
        initial: vec![table(
            "users",
            vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
            vec![],
            vec![],
        )],
        actions: vec![
            MigrationAction::ModifyColumnType {
                table: "users".into(),
                column: "id".into(),
                new_type: ColumnType::Simple(SimpleColumnType::Text),
            },
            MigrationAction::AddIndex {
                table: "users".into(),
                index: idx("idx_id", vec!["id"], true),
            },
            MigrationAction::RemoveIndex {
                table: "users".into(),
                name: "idx_id".into(),
            },
        ],
        expected: vec![table(
            "users",
            vec![col("id", ColumnType::Simple(SimpleColumnType::Text))],
            vec![],
            vec![],
        )],
    })]
    #[case(SuccessCase {
        initial: vec![table(
            "old",
            vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
            vec![],
            vec![],
        )],
        actions: vec![MigrationAction::RenameTable {
            from: "old".into(),
            to: "new".into(),
        }],
        expected: vec![table(
            "new",
            vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
            vec![],
            vec![],
        )],
    })]
    #[case(SuccessCase {
        initial: vec![table("users", vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))], vec![], vec![])],
        actions: vec![MigrationAction::AddConstraint {
            table: "users".into(),
            constraint: TableConstraint::PrimaryKey {
                auto_increment: false,
                columns: vec!["id".into()],
            },
        }],
        expected: vec![table(
            "users",
            vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
            vec![TableConstraint::PrimaryKey {
                auto_increment: false,
                columns: vec!["id".into()],
            }],
            vec![],
        )],
    })]
    #[case(SuccessCase {
        initial: vec![table(
            "users",
            vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
            vec![TableConstraint::PrimaryKey {
                auto_increment: false,
                columns: vec!["id".into()],
            }],
            vec![],
        )],
        actions: vec![MigrationAction::RemoveConstraint {
            table: "users".into(),
            constraint: TableConstraint::PrimaryKey {
                auto_increment: false,
                columns: vec!["id".into()],
            },
        }],
        expected: vec![table("users", vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))], vec![], vec![])],
    })]
    #[case(SuccessCase {
        initial: vec![table("users", vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))], vec![], vec![])],
        actions: vec![MigrationAction::RawSql {
            sql: "SELECT 1;".to_string(),
        }],
        expected: vec![table("users", vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))], vec![], vec![])],
    })]
    fn apply_action_success_cases(#[case] case: SuccessCase) {
        let mut schema = case.initial;
        for action in case.actions {
            apply_action(&mut schema, &action).unwrap();
        }
        assert_eq!(schema, case.expected);
    }

    #[rstest]
    #[case(
        vec![
            TableConstraint::PrimaryKey{ auto_increment: false, columns: vec!["id".into(), "old".into()] },
            TableConstraint::Unique {
                name: None,
                columns: vec!["old".into(), "keep".into()],
            },
            TableConstraint::ForeignKey {
                name: None,
                columns: vec!["old".into()],
                ref_table: "ref".into(),
                ref_columns: vec!["old".into()],
                on_delete: None,
                on_update: None,
            },
            TableConstraint::Check {
                name: "ck_old".into(),
                expr: "old > 0".into(),
            },
        ],
        vec![idx("idx_old", vec!["old", "keep"], false)],
        "old",
        "new",
        vec![
            TableConstraint::PrimaryKey{ auto_increment: false, columns: vec!["id".into(), "new".into()] },
            TableConstraint::Unique {
                name: None,
                columns: vec!["new".into(), "keep".into()],
            },
            TableConstraint::ForeignKey {
                name: None,
                columns: vec!["new".into()],
                ref_table: "ref".into(),
                ref_columns: vec!["new".into()],
                on_delete: None,
                on_update: None,
            },
            TableConstraint::Check {
                name: "ck_old".into(),
                expr: "old > 0".into(),
            },
        ],
        vec![idx("idx_old", vec!["new", "keep"], false)]
    )]
    #[case(
        vec![
            TableConstraint::PrimaryKey{ auto_increment: false, columns: vec!["id".into()] },
            TableConstraint::Check {
                name: "ck_id".into(),
                expr: "id > 0".into(),
            },
        ],
        vec![idx("idx_id", vec!["id"], false)],
        "missing",
        "new",
        vec![
            TableConstraint::PrimaryKey{ auto_increment: false, columns: vec!["id".into()] },
            TableConstraint::Check {
                name: "ck_id".into(),
                expr: "id > 0".into(),
            },
        ],
        vec![idx("idx_id", vec!["id"], false)]
    )]
    fn rename_helpers_update_constraints_and_indexes(
        #[case] mut constraints: Vec<TableConstraint>,
        #[case] mut indexes: Vec<IndexDef>,
        #[case] from: &str,
        #[case] to: &str,
        #[case] expected_constraints: Vec<TableConstraint>,
        #[case] expected_indexes: Vec<IndexDef>,
    ) {
        rename_column_in_constraints(&mut constraints, from, to);
        rename_column_in_indexes(&mut indexes, from, to);
        assert_eq!(constraints, expected_constraints);
        assert_eq!(indexes, expected_indexes);
    }

    // Tests for RemoveIndex clearing inline index on columns
    #[test]
    fn remove_index_clears_inline_index_bool() {
        // Column with inline index: true creates idx_{table}_{column} pattern
        let mut col_with_index = col("email", ColumnType::Simple(SimpleColumnType::Text));
        col_with_index.index = Some(vespertide_core::StrOrBoolOrArray::Bool(true));

        let mut schema = vec![table(
            "users",
            vec![col_with_index],
            vec![],
            vec![idx("idx_users_email", vec!["email"], false)],
        )];

        apply_action(
            &mut schema,
            &MigrationAction::RemoveIndex {
                table: "users".into(),
                name: "idx_users_email".into(),
            },
        )
        .unwrap();

        // Index should be removed from indexes array
        assert!(schema[0].indexes.is_empty());
        // Inline index on column should also be cleared
        assert!(schema[0].columns[0].index.is_none());
    }

    #[test]
    fn remove_index_clears_inline_index_str() {
        // Column with inline index: "custom_idx_name"
        let mut col_with_index = col("email", ColumnType::Simple(SimpleColumnType::Text));
        col_with_index.index = Some(vespertide_core::StrOrBoolOrArray::Str(
            "custom_idx_name".into(),
        ));

        let mut schema = vec![table(
            "users",
            vec![col_with_index],
            vec![],
            vec![idx("custom_idx_name", vec!["email"], false)],
        )];

        apply_action(
            &mut schema,
            &MigrationAction::RemoveIndex {
                table: "users".into(),
                name: "custom_idx_name".into(),
            },
        )
        .unwrap();

        assert!(schema[0].indexes.is_empty());
        assert!(schema[0].columns[0].index.is_none());
    }

    #[test]
    fn remove_index_clears_inline_index_array_partial() {
        // Column with inline index: ["idx_a", "idx_b"]
        let mut col_with_index = col("email", ColumnType::Simple(SimpleColumnType::Text));
        col_with_index.index = Some(vespertide_core::StrOrBoolOrArray::Array(vec![
            "idx_a".into(),
            "idx_b".into(),
        ]));

        let mut schema = vec![table(
            "users",
            vec![col_with_index],
            vec![],
            vec![
                idx("idx_a", vec!["email"], false),
                idx("idx_b", vec!["email"], false),
            ],
        )];

        // Remove only idx_a
        apply_action(
            &mut schema,
            &MigrationAction::RemoveIndex {
                table: "users".into(),
                name: "idx_a".into(),
            },
        )
        .unwrap();

        assert_eq!(schema[0].indexes.len(), 1);
        assert_eq!(schema[0].indexes[0].name, "idx_b");
        // inline index should only have idx_b remaining
        assert_eq!(
            schema[0].columns[0].index,
            Some(vespertide_core::StrOrBoolOrArray::Array(vec![
                "idx_b".into()
            ]))
        );
    }

    #[test]
    fn remove_index_clears_inline_index_array_all() {
        // Column with inline index: ["idx_single"]
        let mut col_with_index = col("email", ColumnType::Simple(SimpleColumnType::Text));
        col_with_index.index = Some(vespertide_core::StrOrBoolOrArray::Array(vec![
            "idx_single".into(),
        ]));

        let mut schema = vec![table(
            "users",
            vec![col_with_index],
            vec![],
            vec![idx("idx_single", vec!["email"], false)],
        )];

        apply_action(
            &mut schema,
            &MigrationAction::RemoveIndex {
                table: "users".into(),
                name: "idx_single".into(),
            },
        )
        .unwrap();

        assert!(schema[0].indexes.is_empty());
        // When array becomes empty, inline index should be None
        assert!(schema[0].columns[0].index.is_none());
    }

    #[test]
    fn remove_index_with_inline_bool_non_matching_name() {
        // Column with inline index: true, but index name doesn't match idx_{table}_{column} pattern
        // This tests the `_ => {}` branch (line 144) where Bool(true) doesn't match Str or Array
        let mut col_with_index = col("email", ColumnType::Simple(SimpleColumnType::Text));
        col_with_index.index = Some(vespertide_core::StrOrBoolOrArray::Bool(true));

        let mut schema = vec![table(
            "users",
            vec![col_with_index],
            vec![],
            vec![idx("custom_email_idx", vec!["email"], false)], // not idx_users_email
        )];

        apply_action(
            &mut schema,
            &MigrationAction::RemoveIndex {
                table: "users".into(),
                name: "custom_email_idx".into(),
            },
        )
        .unwrap();

        // Index removed from array
        assert!(schema[0].indexes.is_empty());
        // Inline index NOT cleared because name didn't match pattern and Bool(true) hits _ branch
        assert_eq!(
            schema[0].columns[0].index,
            Some(vespertide_core::StrOrBoolOrArray::Bool(true))
        );
    }
}
