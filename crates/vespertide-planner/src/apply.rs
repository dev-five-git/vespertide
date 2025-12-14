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
                tbl.columns.push(column.clone());
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
            column: col("id", ColumnType::Simple(SimpleColumnType::Integer)),
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
                    name: None,
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
                column: col("new_col", ColumnType::Simple(SimpleColumnType::Boolean)),
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
                    name: None,
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
                    name: None,
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
                    name: None,
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
                name: None,
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
                name: None,
                expr: "old > 0".into(),
            },
        ],
        vec![idx("idx_old", vec!["new", "keep"], false)]
    )]
    #[case(
        vec![
            TableConstraint::PrimaryKey{ auto_increment: false, columns: vec!["id".into()] },
            TableConstraint::Check {
                name: None,
                expr: "id > 0".into(),
            },
        ],
        vec![idx("idx_id", vec!["id"], false)],
        "missing",
        "new",
        vec![
            TableConstraint::PrimaryKey{ auto_increment: false, columns: vec!["id".into()] },
            TableConstraint::Check {
                name: None,
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
}
