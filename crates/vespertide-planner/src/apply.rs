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
                return Err(PlannerError::TableNotFound(table.clone()));
            }
            Ok(())
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
                return Err(PlannerError::ColumnExists(
                    table.clone(),
                    column.name.clone(),
                ));
            }
            tbl.columns.push(column.clone());
            Ok(())
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
                return Err(PlannerError::ColumnNotFound(table.clone(), column.clone()));
            }
            drop_column_from_constraints(&mut tbl.constraints, column);
            drop_column_from_indexes(&mut tbl.indexes, column);
            Ok(())
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
                return Err(PlannerError::IndexNotFound(table.clone(), name.clone()));
            }
            Ok(())
        }
        MigrationAction::RenameTable { from, to } => {
            if schema.iter().any(|t| t.name == *to) {
                return Err(PlannerError::TableExists(to.clone()));
            }
            let tbl = schema
                .iter_mut()
                .find(|t| t.name == *from)
                .ok_or_else(|| PlannerError::TableNotFound(from.clone()))?;
            tbl.name = to.clone();
            Ok(())
        }
    }
}

fn rename_column_in_constraints(constraints: &mut [TableConstraint], from: &str, to: &str) {
    for constraint in constraints {
        match constraint {
            TableConstraint::PrimaryKey(cols) => {
                for c in cols.iter_mut() {
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
        TableConstraint::PrimaryKey(cols) => {
            cols.retain(|c| c != column);
            !cols.is_empty()
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
    use vespertide_core::{ColumnDef, ColumnType};

    fn col(name: &str, ty: ColumnType) -> ColumnDef {
        ColumnDef {
            name: name.to_string(),
            r#type: ty,
            nullable: true,
            default: None,
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
            vec![col("id", ColumnType::Integer)],
            vec![],
            vec![]
        )],
        MigrationAction::AddColumn {
            table: "users".into(),
            column: col("id", ColumnType::Integer),
            fill_with: None,
        },
        ErrKind::ColumnExists
    )]
    #[case(
        vec![table(
            "users",
            vec![col("id", ColumnType::Integer)],
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
            vec![col("id", ColumnType::Integer)],
            vec![],
            vec![]
        )],
        MigrationAction::RemoveIndex {
            table: "users".into(),
            name: "idx".into()
        },
        ErrKind::IndexNotFound
    )]
    fn apply_action_reports_errors(
        #[case] mut schema: Vec<TableDef>,
        #[case] action: MigrationAction,
        #[case] expected: ErrKind,
    ) {
        let err = apply_action(&mut schema, &action).unwrap_err();
        assert_err_kind(err, expected);
    }
}
