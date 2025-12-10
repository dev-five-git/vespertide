use std::fmt::Write;

use vespertide_core::{ColumnDef, ColumnType, MigrationAction, TableConstraint};

use crate::error::QueryError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuiltQuery {
    pub sql: String,
    pub binds: Vec<String>,
}

pub(crate) fn bind(binds: &mut Vec<String>, value: impl Into<String>) -> String {
    binds.push(value.into());
    format!("${}", binds.len())
}

pub fn build_action_queries(action: &MigrationAction) -> Result<Vec<BuiltQuery>, QueryError> {
    match action {
        MigrationAction::CreateTable {
            table,
            columns,
            constraints,
        } => Ok(vec![create_table_sql(table, columns, constraints)?]),
        MigrationAction::DeleteTable { table } => {
            let mut binds = Vec::new();
            let t = bind(&mut binds, table);
            Ok(vec![BuiltQuery {
                sql: format!("DROP TABLE {t};"),
                binds,
            }])
        }
        MigrationAction::AddColumn {
            table,
            column,
            fill_with,
        } => {
            // If adding NOT NULL without default, optionally backfill then enforce NOT NULL.
            let mut stmts: Vec<BuiltQuery> = Vec::new();
            let mut binds_add = Vec::new();
            let t = bind(&mut binds_add, table);
            let add_col_sql = if column.nullable || column.default.is_some() || fill_with.is_none()
            {
                format!(
                    "ALTER TABLE {t} ADD COLUMN {};",
                    column_def_sql(column, &mut binds_add)
                )
            } else {
                // Add as nullable to allow backfill.
                let mut c = column.clone();
                c.nullable = true;
                format!(
                    "ALTER TABLE {t} ADD COLUMN {};",
                    column_def_sql(&c, &mut binds_add)
                )
            };
            stmts.push(BuiltQuery {
                sql: add_col_sql,
                binds: binds_add,
            });

            if let Some(fill) = fill_with {
                let mut binds = Vec::new();
                let t = bind(&mut binds, table);
                let col = bind(&mut binds, &column.name);
                let val = bind(&mut binds, fill);
                stmts.push(BuiltQuery {
                    sql: format!("UPDATE {t} SET {col} = {val};"),
                    binds,
                });
            }

            if !column.nullable && column.default.is_none() && fill_with.is_some() {
                let mut binds = Vec::new();
                let t = bind(&mut binds, table);
                let col = bind(&mut binds, &column.name);
                stmts.push(BuiltQuery {
                    sql: format!("ALTER TABLE {t} ALTER COLUMN {col} SET NOT NULL;"),
                    binds,
                });
            }

            Ok(stmts)
        }
        MigrationAction::RenameColumn { table, from, to } => Ok(vec![BuiltQuery {
            sql: {
                let mut binds = Vec::new();
                let t = bind(&mut binds, table);
                let f = bind(&mut binds, from);
                let tt = bind(&mut binds, to);
                format!("ALTER TABLE {t} RENAME COLUMN {f} TO {tt};")
            },
            binds: {
                let mut b = Vec::new();
                bind(&mut b, table);
                bind(&mut b, from);
                bind(&mut b, to);
                b
            },
        }]),
        MigrationAction::DeleteColumn { table, column } => Ok(vec![BuiltQuery {
            sql: {
                let mut binds = Vec::new();
                let t = bind(&mut binds, table);
                let c = bind(&mut binds, column);
                format!("ALTER TABLE {t} DROP COLUMN {c};")
            },
            binds: {
                let mut b = Vec::new();
                bind(&mut b, table);
                bind(&mut b, column);
                b
            },
        }]),
        MigrationAction::ModifyColumnType {
            table,
            column,
            new_type,
        } => Ok(vec![BuiltQuery {
            sql: {
                let mut binds = Vec::new();
                let t = bind(&mut binds, table);
                let c = bind(&mut binds, column);
                format!(
                    "ALTER TABLE {t} ALTER COLUMN {c} TYPE {};",
                    column_type_sql(new_type)
                )
            },
            binds: {
                let mut b = Vec::new();
                bind(&mut b, table);
                bind(&mut b, column);
                b
            },
        }]),
        MigrationAction::AddIndex { table, index } => Ok(vec![BuiltQuery {
            sql: {
                let mut binds = Vec::new();
                let t = bind(&mut binds, table);
                let idx = bind(&mut binds, &index.name);
                let cols = index
                    .columns
                    .iter()
                    .map(|c| bind(&mut binds, c))
                    .collect::<Vec<_>>()
                    .join(", ");
                let unique = if index.unique { "UNIQUE " } else { "" };
                format!("CREATE {unique}INDEX {idx} ON {t} ({cols});")
            },
            binds: {
                let mut b = Vec::new();
                bind(&mut b, table);
                bind(&mut b, &index.name);
                for c in &index.columns {
                    bind(&mut b, c);
                }
                b
            },
        }]),
        MigrationAction::RemoveIndex { name, .. } => Ok(vec![BuiltQuery {
            sql: {
                let mut binds = Vec::new();
                let n = bind(&mut binds, name);
                format!("DROP INDEX {n};")
            },
            binds: {
                let mut b = Vec::new();
                bind(&mut b, name);
                b
            },
        }]),
        MigrationAction::RenameTable { from, to } => Ok(vec![BuiltQuery {
            sql: {
                let mut binds = Vec::new();
                let f = bind(&mut binds, from);
                let t = bind(&mut binds, to);
                format!("ALTER TABLE {f} RENAME TO {t};")
            },
            binds: {
                let mut b = Vec::new();
                bind(&mut b, from);
                bind(&mut b, to);
                b
            },
        }]),
    }
}

fn create_table_sql(
    table: &str,
    columns: &[ColumnDef],
    constraints: &[TableConstraint],
) -> Result<BuiltQuery, QueryError> {
    let mut binds = Vec::new();
    let t = bind(&mut binds, table);
    let mut parts: Vec<String> = columns
        .iter()
        .map(|c| column_def_sql(c, &mut binds))
        .collect();
    for constraint in constraints {
        parts.push(table_constraint_sql(constraint, &mut binds)?);
    }
    let mut sql = String::new();
    write!(&mut sql, "CREATE TABLE {t} ({});", parts.join(", ")).unwrap();
    Ok(BuiltQuery { sql, binds })
}

fn column_def_sql(column: &ColumnDef, binds: &mut Vec<String>) -> String {
    let name = bind(binds, &column.name);
    let mut parts = vec![format!("{name} {}", column_type_sql(&column.r#type))];
    if !column.nullable {
        parts.push("NOT NULL".into());
    }
    if let Some(default) = &column.default {
        let p = bind(binds, default);
        parts.push(format!("DEFAULT {p}"));
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

fn table_constraint_sql(
    constraint: &TableConstraint,
    binds: &mut Vec<String>,
) -> Result<String, QueryError> {
    Ok(match constraint {
        TableConstraint::PrimaryKey(cols) => {
            let placeholders = cols
                .iter()
                .map(|c| bind(binds, c))
                .collect::<Vec<_>>()
                .join(", ");
            format!("PRIMARY KEY ({placeholders})")
        }
        TableConstraint::Unique { name, columns } => match name {
            Some(n) => {
                let nm = bind(binds, n);
                let placeholders = columns
                    .iter()
                    .map(|c| bind(binds, c))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("CONSTRAINT {nm} UNIQUE ({placeholders})")
            }
            None => {
                let placeholders = columns
                    .iter()
                    .map(|c| bind(binds, c))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("UNIQUE ({placeholders})")
            }
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
                let nm = bind(binds, n);
                write!(&mut sql, "CONSTRAINT {nm} ").unwrap();
            }
            let cols = columns
                .iter()
                .map(|c| bind(binds, c))
                .collect::<Vec<_>>()
                .join(", ");
            let ref_cols = ref_columns
                .iter()
                .map(|c| bind(binds, c))
                .collect::<Vec<_>>()
                .join(", ");
            let ref_tbl = bind(binds, ref_table);
            write!(
                &mut sql,
                "FOREIGN KEY ({cols}) REFERENCES {ref_tbl} ({ref_cols})"
            )
            .unwrap();
            if let Some(action) = on_delete {
                write!(
                    &mut sql,
                    " ON DELETE {}",
                    reference_action_sql(action, binds)
                )
                .unwrap();
            }
            if let Some(action) = on_update {
                write!(
                    &mut sql,
                    " ON UPDATE {}",
                    reference_action_sql(action, binds)
                )
                .unwrap();
            }
            sql
        }
        TableConstraint::Check { name, expr } => match name {
            Some(n) => {
                let nm = bind(binds, n);
                let e = bind(binds, expr);
                format!("CONSTRAINT {nm} CHECK ({e})")
            }
            None => {
                let e = bind(binds, expr);
                format!("CHECK ({e})")
            }
        },
    })
}

fn reference_action_sql(
    action: &vespertide_core::ReferenceAction,
    _binds: &mut Vec<String>,
) -> &'static str {
    match action {
        vespertide_core::ReferenceAction::Cascade => "CASCADE",
        vespertide_core::ReferenceAction::Restrict => "RESTRICT",
        vespertide_core::ReferenceAction::SetNull => "SET NULL",
        vespertide_core::ReferenceAction::SetDefault => "SET DEFAULT",
        vespertide_core::ReferenceAction::NoAction => "NO ACTION",
    }
}

