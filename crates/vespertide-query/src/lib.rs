use std::fmt::Write;

use thiserror::Error;
use vespertide_core::{
    ColumnDef, ColumnType, IndexDef, MigrationAction, MigrationPlan, TableConstraint,
};

#[derive(Debug, Error)]
pub enum QueryError {
    #[error("unsupported table constraint")]
    UnsupportedConstraint,
}

pub fn build_plan_queries(plan: &MigrationPlan) -> Result<Vec<String>, QueryError> {
    let mut queries = Vec::new();
    for action in &plan.actions {
        queries.extend(build_action_queries(action)?);
    }
    Ok(queries)
}

pub fn build_action_queries(action: &MigrationAction) -> Result<Vec<String>, QueryError> {
    match action {
        MigrationAction::CreateTable {
            table,
            columns,
            constraints,
        } => Ok(vec![create_table_sql(table, columns, constraints)?]),
        MigrationAction::DeleteTable { table } => {
            Ok(vec![format!("DROP TABLE {table};")])
        }
        MigrationAction::AddColumn { table, column } => Ok(vec![format!(
            "ALTER TABLE {table} ADD COLUMN {};",
            column_def_sql(column)
        )]),
        MigrationAction::RenameColumn { table, from, to } => Ok(vec![format!(
            "ALTER TABLE {table} RENAME COLUMN {from} TO {to};"
        )]),
        MigrationAction::DeleteColumn { table, column } => {
            Ok(vec![format!("ALTER TABLE {table} DROP COLUMN {column};")])
        }
        MigrationAction::ModifyColumnType {
            table,
            column,
            new_type,
        } => Ok(vec![format!(
            "ALTER TABLE {table} ALTER COLUMN {column} TYPE {};",
            column_type_sql(new_type)
        )]),
        MigrationAction::AddIndex { table, index } => Ok(vec![create_index_sql(table, index)]),
        MigrationAction::RemoveIndex { name, .. } => {
            Ok(vec![format!("DROP INDEX {name};")])
        }
        MigrationAction::RenameTable { from, to } => {
            Ok(vec![format!("ALTER TABLE {from} RENAME TO {to};")])
        }
    }
}

fn create_table_sql(
    table: &str,
    columns: &[ColumnDef],
    constraints: &[TableConstraint],
) -> Result<String, QueryError> {
    let mut parts: Vec<String> = columns.iter().map(column_def_sql).collect();
    for constraint in constraints {
        parts.push(table_constraint_sql(constraint)?);
    }
    let mut sql = String::new();
    write!(&mut sql, "CREATE TABLE {table} ({});", parts.join(", ")).unwrap();
    Ok(sql)
}

fn column_def_sql(column: &ColumnDef) -> String {
    let mut parts = vec![format!("{} {}", column.name, column_type_sql(&column.data_type))];
    if !column.nullable {
        parts.push("NOT NULL".into());
    }
    if let Some(default) = &column.default {
        parts.push(format!("DEFAULT {default}"));
    }
    parts.join(" ")
}

fn column_type_sql(ty: &ColumnType) -> String {
    match ty {
        ColumnType::Integer => "INTEGER".into(),
        ColumnType::BigInt => "BIGINT".into(),
        ColumnType::Text => "TEXT".into(),
        ColumnType::Boolean => "BOOLEAN".into(),
        ColumnType::Timestamp => "TIMESTAMP".into(),
        ColumnType::Custom(s) => s.clone(),
    }
}

fn table_constraint_sql(constraint: &TableConstraint) -> Result<String, QueryError> {
    Ok(match constraint {
        TableConstraint::PrimaryKey(cols) => {
            format!("PRIMARY KEY ({})", cols.join(", "))
        }
        TableConstraint::Unique { name, columns } => match name {
            Some(n) => format!("CONSTRAINT {n} UNIQUE ({})", columns.join(", ")),
            None => format!("UNIQUE ({})", columns.join(", ")),
        },
        TableConstraint::ForeignKey {
            name,
            columns,
            ref_table,
            ref_columns,
            on_delete,
            on_update,
        } => {
            let mut sql = String::new();
            if let Some(n) = name {
                write!(&mut sql, "CONSTRAINT {n} ").unwrap();
            }
            write!(
                &mut sql,
                "FOREIGN KEY ({}) REFERENCES {} ({})",
                columns.join(", "),
                ref_table,
                ref_columns.join(", ")
            )
            .unwrap();
            if let Some(action) = on_delete {
                write!(&mut sql, " ON DELETE {}", reference_action_sql(action)).unwrap();
            }
            if let Some(action) = on_update {
                write!(&mut sql, " ON UPDATE {}", reference_action_sql(action)).unwrap();
            }
            sql
        }
        TableConstraint::Check { name, expr } => match name {
            Some(n) => format!("CONSTRAINT {n} CHECK ({expr})"),
            None => format!("CHECK ({expr})"),
        },
    })
}

fn reference_action_sql(action: &vespertide_core::ReferenceAction) -> &'static str {
    match action {
        vespertide_core::ReferenceAction::Cascade => "CASCADE",
        vespertide_core::ReferenceAction::Restrict => "RESTRICT",
        vespertide_core::ReferenceAction::SetNull => "SET NULL",
        vespertide_core::ReferenceAction::SetDefault => "SET DEFAULT",
        vespertide_core::ReferenceAction::NoAction => "NO ACTION",
    }
}

fn create_index_sql(table: &str, index: &IndexDef) -> String {
    let unique = if index.unique { "UNIQUE " } else { "" };
    format!(
        "CREATE {unique}INDEX {} ON {} ({});",
        index.name,
        table,
        index.columns.join(", ")
    )
}

