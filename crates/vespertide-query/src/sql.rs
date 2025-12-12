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
        MigrationAction::RawSql { sql } => Ok(vec![BuiltQuery {
            sql: sql.to_string(),
            binds: Vec::new(),
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
        TableConstraint::PrimaryKey { columns } => {
            let placeholders = columns
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

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;
    use vespertide_core::{
        ColumnDef, ColumnType, IndexDef, MigrationAction, ReferenceAction, TableConstraint,
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

    #[rstest]
    #[case(
        vec!["test"],
        vec!["$1"],
        vec!["test".to_string()]
    )]
    #[case(
        vec!["test", "test2"],
        vec!["$1", "$2"],
        vec!["test".to_string(), "test2".to_string()]
    )]
    fn test_bind(
        #[case] inputs: Vec<&str>,
        #[case] expected_placeholders: Vec<&str>,
        #[case] expected_binds: Vec<String>,
    ) {
        let mut binds = Vec::new();
        for (i, input) in inputs.iter().enumerate() {
            let placeholder = bind(&mut binds, *input);
            assert_eq!(placeholder, expected_placeholders[i]);
        }
        assert_eq!(binds, expected_binds);
    }

    #[rstest]
    #[case::create_table(
        MigrationAction::CreateTable {
            table: "users".into(),
            columns: vec![
                col("id", ColumnType::Integer),
                col("name", ColumnType::Text),
            ],
            constraints: vec![TableConstraint::PrimaryKey{columns: vec!["id".into()] }],
        },
        vec![(
            "CREATE TABLE $1 ($2 INTEGER, $3 TEXT, PRIMARY KEY ($4));".to_string(),
            vec!["users".to_string(), "id".to_string(), "name".to_string(), "id".to_string()],
        )]
    )]
    #[case::delete_table(
        MigrationAction::DeleteTable {
            table: "users".into(),
        },
        vec![("DROP TABLE $1;".to_string(), vec!["users".to_string()])]
    )]
    #[case::add_column_nullable(
        MigrationAction::AddColumn {
            table: "users".into(),
            column: col("email", ColumnType::Text),
            fill_with: None,
        },
        vec![(
            "ALTER TABLE $1 ADD COLUMN $2 TEXT;".to_string(),
            vec!["users".to_string(), "email".to_string()],
        )]
    )]
    #[case::add_column_not_null_with_default(
        {
            let mut c = col("email", ColumnType::Text);
            c.nullable = false;
            c.default = Some("''".to_string());
            MigrationAction::AddColumn {
                table: "users".into(),
                column: c,
                fill_with: None,
            }
        },
        vec![(
            "ALTER TABLE $1 ADD COLUMN $2 TEXT NOT NULL DEFAULT $3;".to_string(),
            vec!["users".to_string(), "email".to_string(), "''".to_string()],
        )]
    )]
    #[case::add_column_not_null_with_fill(
        {
            let mut c = col("email", ColumnType::Text);
            c.nullable = false;
            MigrationAction::AddColumn {
                table: "users".into(),
                column: c,
                fill_with: Some("test@example.com".to_string()),
            }
        },
        vec![
            (
                "ALTER TABLE $1 ADD COLUMN $2 TEXT;".to_string(),
                vec!["users".to_string(), "email".to_string()],
            ),
            (
                "UPDATE $1 SET $2 = $3;".to_string(),
                vec!["users".to_string(), "email".to_string(), "test@example.com".to_string()],
            ),
            (
                "ALTER TABLE $1 ALTER COLUMN $2 SET NOT NULL;".to_string(),
                vec!["users".to_string(), "email".to_string()],
            ),
        ]
    )]
    #[case::add_column_not_null_without_default_without_fill(
        {
            let mut c = col("email", ColumnType::Text);
            c.nullable = false;
            MigrationAction::AddColumn {
                table: "users".into(),
                column: c,
                fill_with: None,
            }
        },
        vec![(
            "ALTER TABLE $1 ADD COLUMN $2 TEXT NOT NULL;".to_string(),
            vec!["users".to_string(), "email".to_string()],
        )]
    )]
    #[case::rename_column(
        MigrationAction::RenameColumn {
            table: "users".into(),
            from: "old_name".into(),
            to: "new_name".into(),
        },
        vec![(
            "ALTER TABLE $1 RENAME COLUMN $2 TO $3;".to_string(),
            vec!["users".to_string(), "old_name".to_string(), "new_name".to_string()],
        )]
    )]
    #[case::delete_column(
        MigrationAction::DeleteColumn {
            table: "users".into(),
            column: "email".into(),
        },
        vec![(
            "ALTER TABLE $1 DROP COLUMN $2;".to_string(),
            vec!["users".to_string(), "email".to_string()],
        )]
    )]
    #[case::modify_column_type(
        MigrationAction::ModifyColumnType {
            table: "users".into(),
            column: "age".into(),
            new_type: ColumnType::BigInt,
        },
        vec![(
            "ALTER TABLE $1 ALTER COLUMN $2 TYPE BIGINT;".to_string(),
            vec!["users".to_string(), "age".to_string()],
        )]
    )]
    #[case::add_index(
        MigrationAction::AddIndex {
            table: "users".into(),
            index: IndexDef {
                name: "idx_email".into(),
                columns: vec!["email".into()],
                unique: false,
            },
        },
        vec![(
            "CREATE INDEX $2 ON $1 ($3);".to_string(),
            vec!["users".to_string(), "idx_email".to_string(), "email".to_string()],
        )]
    )]
    #[case::add_unique_index(
        MigrationAction::AddIndex {
            table: "users".into(),
            index: IndexDef {
                name: "idx_email".into(),
                columns: vec!["email".into()],
                unique: true,
            },
        },
        vec![(
            "CREATE UNIQUE INDEX $2 ON $1 ($3);".to_string(),
            vec!["users".to_string(), "idx_email".to_string(), "email".to_string()],
        )]
    )]
    #[case::add_index_multiple_columns(
        MigrationAction::AddIndex {
            table: "users".into(),
            index: IndexDef {
                name: "idx_name_email".into(),
                columns: vec!["name".into(), "email".into()],
                unique: false,
            },
        },
        vec![(
            "CREATE INDEX $2 ON $1 ($3, $4);".to_string(),
            vec![
                "users".to_string(),
                "idx_name_email".to_string(),
                "name".to_string(),
                "email".to_string(),
            ],
        )]
    )]
    #[case::remove_index(
        MigrationAction::RemoveIndex {
            table: "users".into(),
            name: "idx_email".into(),
        },
        vec![(
            "DROP INDEX $1;".to_string(),
            vec!["idx_email".to_string()],
        )]
    )]
    #[case::rename_table(
        MigrationAction::RenameTable {
            from: "old_users".into(),
            to: "new_users".into(),
        },
        vec![(
            "ALTER TABLE $1 RENAME TO $2;".to_string(),
            vec!["old_users".to_string(), "new_users".to_string()],
        )]
    )]
    fn test_build_action_queries(
        #[case] action: MigrationAction,
        #[case] expected: Vec<(String, Vec<String>)>,
    ) {
        let result = build_action_queries(&action).unwrap();
        assert_eq!(
            result.len(),
            expected.len(),
            "Expected {} queries, got {}",
            expected.len(),
            result.len()
        );

        for (i, (expected_sql, expected_binds)) in expected.iter().enumerate() {
            assert_eq!(result[i].sql, *expected_sql, "Query {} mismatch sql", i);
            assert_eq!(
                result[i].binds, *expected_binds,
                "Query {} mismatch binds",
                i
            );
        }
    }

    #[rstest]
    #[case::simple(
        "users",
        vec![col("id", ColumnType::Integer), col("name", ColumnType::Text)],
        vec![TableConstraint::PrimaryKey{columns: vec!["id".into()] }],
        (
            "CREATE TABLE $1 ($2 INTEGER, $3 TEXT, PRIMARY KEY ($4));".to_string(),
            vec!["users".to_string(), "id".to_string(), "name".to_string(), "id".to_string()],
        )
    )]
    #[case::multiple_constraints(
        "users",
        vec![col("id", ColumnType::Integer), col("email", ColumnType::Text)],
        vec![
            TableConstraint::PrimaryKey{columns: vec!["id".into()] },
            TableConstraint::Unique {
                name: Some("unique_email".into()),
                columns: vec!["email".into()],
            },
        ],
        (
            "CREATE TABLE $1 ($2 INTEGER, $3 TEXT, PRIMARY KEY ($4), CONSTRAINT $5 UNIQUE ($6));".to_string(),
            vec![
                "users".to_string(),
                "id".to_string(),
                "email".to_string(),
                "id".to_string(),
                "unique_email".to_string(),
                "email".to_string(),
            ],
        )
    )]
    fn test_create_table_sql(
        #[case] table: &str,
        #[case] columns: Vec<ColumnDef>,
        #[case] constraints: Vec<TableConstraint>,
        #[case] expected: (String, Vec<String>),
    ) {
        let result = create_table_sql(table, &columns, &constraints).unwrap();
        assert_eq!(result.sql, expected.0);
        assert_eq!(result.binds, expected.1);
    }

    #[rstest]
    #[case::nullable(
        col("name", ColumnType::Text),
        ("$1 TEXT".to_string(), vec!["name".to_string()])
    )]
    #[case::not_null(
        {
            let mut c = col("name", ColumnType::Text);
            c.nullable = false;
            c
        },
        ("$1 TEXT NOT NULL".to_string(), vec!["name".to_string()])
    )]
    #[case::with_default(
        {
            let mut c = col("name", ColumnType::Text);
            c.default = Some("'default'".to_string());
            c
        },
        (
            "$1 TEXT DEFAULT $2".to_string(),
            vec!["name".to_string(), "'default'".to_string()],
        )
    )]
    fn test_column_def_sql(#[case] column: ColumnDef, #[case] expected: (String, Vec<String>)) {
        let mut binds = Vec::new();
        let result = column_def_sql(&column, &mut binds);
        assert_eq!(result, expected.0);
        assert_eq!(binds, expected.1);
    }

    #[rstest]
    #[case(ColumnType::Integer, "INTEGER")]
    #[case(ColumnType::BigInt, "BIGINT")]
    #[case(ColumnType::Text, "TEXT")]
    #[case(ColumnType::Boolean, "BOOLEAN")]
    #[case(ColumnType::Timestamp, "TIMESTAMP")]
    #[case(ColumnType::Custom("VARCHAR(255)".to_string()), "VARCHAR(255)")]
    fn test_column_type_sql(#[case] ty: ColumnType, #[case] expected: &str) {
        assert_eq!(column_type_sql(&ty), expected);
    }

    #[rstest]
    #[case::primary_key_single(
        TableConstraint::PrimaryKey{columns: vec!["id".into()] },
        ("PRIMARY KEY ($1)".to_string(), vec!["id".to_string()])
    )]
    #[case::primary_key_multiple(
        TableConstraint::PrimaryKey{columns: vec!["id".into(), "version".into()] },
        ("PRIMARY KEY ($1, $2)".to_string(), vec!["id".to_string(), "version".to_string()])
    )]
    #[case::unique_without_name(
        TableConstraint::Unique {
            name: None,
            columns: vec!["email".into()],
        },
        ("UNIQUE ($1)".to_string(), vec!["email".to_string()])
    )]
    #[case::unique_with_name(
        TableConstraint::Unique {
            name: Some("unique_email".into()),
            columns: vec!["email".into()],
        },
        (
            "CONSTRAINT $1 UNIQUE ($2)".to_string(),
            vec!["unique_email".to_string(), "email".to_string()],
        )
    )]
    #[case::foreign_key_without_name(
        TableConstraint::ForeignKey {
            name: None,
            columns: vec!["user_id".into()],
            ref_table: "users".into(),
            ref_columns: vec!["id".into()],
            on_delete: None,
            on_update: None,
        },
        (
            "FOREIGN KEY ($1) REFERENCES $3 ($2)".to_string(),
            vec!["user_id".to_string(), "id".to_string(), "users".to_string()],
        )
    )]
    #[case::foreign_key_with_name(
        TableConstraint::ForeignKey {
            name: Some("fk_user".into()),
            columns: vec!["user_id".into()],
            ref_table: "users".into(),
            ref_columns: vec!["id".into()],
            on_delete: None,
            on_update: None,
        },
        (
            "CONSTRAINT $1 FOREIGN KEY ($2) REFERENCES $4 ($3)".to_string(),
            vec![
                "fk_user".to_string(),
                "user_id".to_string(),
                "id".to_string(),
                "users".to_string(),
            ],
        )
    )]
    #[case::foreign_key_with_on_delete(
        TableConstraint::ForeignKey {
            name: None,
            columns: vec!["user_id".into()],
            ref_table: "users".into(),
            ref_columns: vec!["id".into()],
            on_delete: Some(ReferenceAction::Cascade),
            on_update: None,
        },
        (
            "FOREIGN KEY ($1) REFERENCES $3 ($2) ON DELETE CASCADE".to_string(),
            vec!["user_id".to_string(), "id".to_string(), "users".to_string()],
        )
    )]
    #[case::foreign_key_with_on_update(
        TableConstraint::ForeignKey {
            name: None,
            columns: vec!["user_id".into()],
            ref_table: "users".into(),
            ref_columns: vec!["id".into()],
            on_delete: None,
            on_update: Some(ReferenceAction::Restrict),
        },
        (
            "FOREIGN KEY ($1) REFERENCES $3 ($2) ON UPDATE RESTRICT".to_string(),
            vec!["user_id".to_string(), "id".to_string(), "users".to_string()],
        )
    )]
    #[case::foreign_key_with_both_actions(
        TableConstraint::ForeignKey {
            name: None,
            columns: vec!["user_id".into()],
            ref_table: "users".into(),
            ref_columns: vec!["id".into()],
            on_delete: Some(ReferenceAction::SetNull),
            on_update: Some(ReferenceAction::SetDefault),
        },
        (
            "FOREIGN KEY ($1) REFERENCES $3 ($2) ON DELETE SET NULL ON UPDATE SET DEFAULT".to_string(),
            vec!["user_id".to_string(), "id".to_string(), "users".to_string()],
        )
    )]
    #[case::foreign_key_multiple_columns(
        TableConstraint::ForeignKey {
            name: None,
            columns: vec!["user_id".into(), "tenant_id".into()],
            ref_table: "user_tenants".into(),
            ref_columns: vec!["user_id".into(), "tenant_id".into()],
            on_delete: None,
            on_update: None,
        },
        (
            "FOREIGN KEY ($1, $2) REFERENCES $5 ($3, $4)".to_string(),
            vec![
                "user_id".to_string(),
                "tenant_id".to_string(),
                "user_id".to_string(),
                "tenant_id".to_string(),
                "user_tenants".to_string(),
            ],
        )
    )]
    #[case::check_without_name(
        TableConstraint::Check {
            name: None,
            expr: "age > 0".to_string(),
        },
        ("CHECK ($1)".to_string(), vec!["age > 0".to_string()])
    )]
    #[case::check_with_name(
        TableConstraint::Check {
            name: Some("check_age".into()),
            expr: "age > 0".to_string(),
        },
        (
            "CONSTRAINT $1 CHECK ($2)".to_string(),
            vec!["check_age".to_string(), "age > 0".to_string()],
        )
    )]
    fn test_table_constraint_sql(
        #[case] constraint: TableConstraint,
        #[case] expected: (String, Vec<String>),
    ) {
        let mut binds = Vec::new();
        let result = table_constraint_sql(&constraint, &mut binds).unwrap();
        assert_eq!(result, expected.0);
        assert_eq!(binds, expected.1);
    }

    #[rstest]
    #[case(ReferenceAction::Cascade, "CASCADE")]
    #[case(ReferenceAction::Restrict, "RESTRICT")]
    #[case(ReferenceAction::SetNull, "SET NULL")]
    #[case(ReferenceAction::SetDefault, "SET DEFAULT")]
    #[case(ReferenceAction::NoAction, "NO ACTION")]
    fn test_reference_action_sql(#[case] action: ReferenceAction, #[case] expected: &str) {
        let mut binds = Vec::new();
        assert_eq!(reference_action_sql(&action, &mut binds), expected);
    }
}
