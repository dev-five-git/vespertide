use sea_query::{Alias, ForeignKey, Query, Table};

use vespertide_core::{TableConstraint, TableDef};

use super::helpers::{
    build_sqlite_temp_table_create, recreate_indexes_after_rebuild, to_sea_fk_action,
};
use super::rename_table::build_rename_table;
use super::types::{BuiltQuery, DatabaseBackend};
use crate::error::QueryError;

/// Build SQL queries to replace a constraint in-place.
///
/// For PostgreSQL/MySQL: DROP old FK + ADD new FK (two ALTER TABLE statements).
/// For SQLite: single temp table recreation with the new constraint swapped in.
///
/// This avoids the double table recreation that would occur with separate
/// RemoveConstraint + AddConstraint on SQLite.
pub fn build_replace_constraint(
    backend: &DatabaseBackend,
    table: &str,
    from: &TableConstraint,
    to: &TableConstraint,
    current_schema: &[TableDef],
    pending_constraints: &[TableConstraint],
) -> Result<Vec<BuiltQuery>, QueryError> {
    match (from, to) {
        (
            TableConstraint::ForeignKey {
                name: old_name,
                columns: old_columns,
                ..
            },
            TableConstraint::ForeignKey {
                name: new_name,
                columns: new_columns,
                ref_table,
                ref_columns,
                on_delete,
                on_update,
            },
        ) => {
            if *backend == DatabaseBackend::Sqlite {
                build_sqlite_constraint_replace(
                    backend,
                    table,
                    from,
                    to,
                    current_schema,
                    pending_constraints,
                )
            } else {
                // PostgreSQL/MySQL: DROP old FK + ADD new FK
                let old_fk_name = vespertide_naming::build_foreign_key_name(
                    table,
                    old_columns,
                    old_name.as_deref(),
                );
                let fk_drop = ForeignKey::drop()
                    .name(&old_fk_name)
                    .table(Alias::new(table))
                    .to_owned();

                let new_fk_name = vespertide_naming::build_foreign_key_name(
                    table,
                    new_columns,
                    new_name.as_deref(),
                );
                let mut fk_create = ForeignKey::create();
                fk_create = fk_create.name(&new_fk_name).to_owned();
                fk_create = fk_create.from_tbl(Alias::new(table)).to_owned();
                for col in new_columns {
                    fk_create = fk_create.from_col(Alias::new(col)).to_owned();
                }
                fk_create = fk_create.to_tbl(Alias::new(ref_table)).to_owned();
                for col in ref_columns {
                    fk_create = fk_create.to_col(Alias::new(col)).to_owned();
                }
                if let Some(action) = on_delete {
                    fk_create = fk_create.on_delete(to_sea_fk_action(action)).to_owned();
                }
                if let Some(action) = on_update {
                    fk_create = fk_create.on_update(to_sea_fk_action(action)).to_owned();
                }

                Ok(vec![
                    BuiltQuery::DropForeignKey(Box::new(fk_drop)),
                    BuiltQuery::CreateForeignKey(Box::new(fk_create)),
                ])
            }
        }
        // For non-FK constraints: SQLite uses single temp table, PG/MySQL uses remove + add
        _ => {
            if *backend == DatabaseBackend::Sqlite {
                build_sqlite_constraint_replace(
                    backend,
                    table,
                    from,
                    to,
                    current_schema,
                    pending_constraints,
                )
            } else {
                let mut queries = super::remove_constraint::build_remove_constraint(
                    backend,
                    table,
                    from,
                    current_schema,
                    pending_constraints,
                )?;

                // Build a modified schema with the old constraint removed and new one added
                let modified_schema: Vec<TableDef> = current_schema
                    .iter()
                    .map(|t| {
                        if t.name == table {
                            let mut modified = t.clone();
                            modified.constraints.retain(|c| c != from);
                            modified.constraints.push(to.clone());
                            modified
                        } else {
                            t.clone()
                        }
                    })
                    .collect();

                queries.extend(super::add_constraint::build_add_constraint(
                    backend,
                    table,
                    to,
                    &modified_schema,
                    pending_constraints,
                )?);
                Ok(queries)
            }
        }
    }
}

/// SQLite: single temp table recreation with the constraint replaced.
/// Works for all constraint types (FK, Check, Unique, Index, PK).
fn build_sqlite_constraint_replace(
    backend: &DatabaseBackend,
    table: &str,
    from: &TableConstraint,
    to: &TableConstraint,
    current_schema: &[TableDef],
    pending_constraints: &[TableConstraint],
) -> Result<Vec<BuiltQuery>, QueryError> {
    let table_def = current_schema
        .iter()
        .find(|t| t.name == table)
        .ok_or_else(|| {
            QueryError::Other(format!(
                "Table '{}' not found in current schema. SQLite requires current schema \
                 information to replace constraints.",
                table
            ))
        })?;

    // Build new constraints: replace old constraint with new one
    let new_constraints: Vec<TableConstraint> = table_def
        .constraints
        .iter()
        .map(|c| if c == from { to.clone() } else { c.clone() })
        .collect();

    let temp_table = format!("{}_temp", table);

    // 1. Create temporary table with replaced constraint
    let create_query = build_sqlite_temp_table_create(
        backend,
        &temp_table,
        table,
        &table_def.columns,
        &new_constraints,
    );

    // 2. Copy data (all columns)
    let column_aliases: Vec<Alias> = table_def
        .columns
        .iter()
        .map(|c| Alias::new(&c.name))
        .collect();
    let mut select_query = Query::select();
    for col_alias in &column_aliases {
        select_query = select_query.column(col_alias.clone()).to_owned();
    }
    select_query = select_query.from(Alias::new(table)).to_owned();

    let insert_stmt = Query::insert()
        .into_table(Alias::new(&temp_table))
        .columns(column_aliases.clone())
        .select_from(select_query)
        .unwrap()
        .to_owned();
    let insert_query = BuiltQuery::Insert(Box::new(insert_stmt));

    // 3. Drop original table
    let drop_table = Table::drop().table(Alias::new(table)).to_owned();
    let drop_query = BuiltQuery::DropTable(Box::new(drop_table));

    // 4. Rename temporary table to original name
    let rename_query = build_rename_table(&temp_table, table);

    // 5. Recreate indexes (both regular and UNIQUE)
    let index_queries =
        recreate_indexes_after_rebuild(table, &table_def.constraints, pending_constraints);

    let mut queries = vec![create_query, insert_query, drop_query, rename_query];
    queries.extend(index_queries);
    Ok(queries)
}

#[cfg(test)]
mod tests {
    use super::*;
    use insta::{assert_snapshot, with_settings};
    use rstest::rstest;
    use vespertide_core::{
        ColumnDef, ColumnType, ReferenceAction, SimpleColumnType, TableConstraint, TableDef,
    };

    fn test_schema() -> Vec<TableDef> {
        vec![
            TableDef {
                name: "users".into(),
                columns: vec![ColumnDef {
                    name: "id".into(),
                    r#type: ColumnType::Simple(SimpleColumnType::Integer),
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
                description: None,
            },
            TableDef {
                name: "posts".into(),
                columns: vec![
                    ColumnDef {
                        name: "id".into(),
                        r#type: ColumnType::Simple(SimpleColumnType::Integer),
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
                        r#type: ColumnType::Simple(SimpleColumnType::Integer),
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
                        name: Some("fk_user".into()),
                        columns: vec!["user_id".into()],
                        ref_table: "users".into(),
                        ref_columns: vec!["id".into()],
                        on_delete: None,
                        on_update: None,
                    },
                ],
                description: None,
            },
        ]
    }

    #[rstest]
    #[case::postgres(DatabaseBackend::Postgres)]
    #[case::mysql(DatabaseBackend::MySql)]
    #[case::sqlite(DatabaseBackend::Sqlite)]
    fn replace_fk_on_delete(#[case] backend: DatabaseBackend) {
        let schema = test_schema();
        let from = TableConstraint::ForeignKey {
            name: Some("fk_user".into()),
            columns: vec!["user_id".into()],
            ref_table: "users".into(),
            ref_columns: vec!["id".into()],
            on_delete: None,
            on_update: None,
        };
        let to = TableConstraint::ForeignKey {
            name: Some("fk_user".into()),
            columns: vec!["user_id".into()],
            ref_table: "users".into(),
            ref_columns: vec!["id".into()],
            on_delete: Some(ReferenceAction::Cascade),
            on_update: None,
        };

        let queries = build_replace_constraint(&backend, "posts", &from, &to, &schema, &[])
            .expect("should succeed");

        let sql: Vec<String> = queries.iter().map(|q| q.build(backend)).collect();
        let combined = sql.join(";\n");

        with_settings!({
            description => format!("replace FK on_delete for {:?}", backend),
            omit_expression => true,
            snapshot_suffix => format!("replace_fk_on_delete_{:?}", backend),
        }, {
            assert_snapshot!(combined);
        });
    }
}
