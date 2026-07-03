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
/// For `SQLite`: single temp table recreation with the new constraint swapped in.
///
/// This avoids the double table recreation that would occur with separate
/// `RemoveConstraint` + `AddConstraint` on `SQLite`.
pub fn build_replace_constraint(
    backend: DatabaseBackend,
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
                ..
            },
        ) => {
            if backend == DatabaseBackend::Sqlite {
                build_sqlite_constraint_replace(
                    backend,
                    table,
                    from,
                    to,
                    current_schema,
                    pending_constraints,
                )
            } else {
                Ok(build_direct_foreign_key_replace(
                    table,
                    old_name.as_deref(),
                    old_columns,
                    new_name.as_deref(),
                    new_columns,
                    ref_table,
                    ref_columns,
                    on_delete.as_ref(),
                    on_update.as_ref(),
                ))
            }
        }
        // For non-FK constraints: SQLite uses single temp table, PG/MySQL uses remove + add
        _ => {
            if backend == DatabaseBackend::Sqlite {
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

#[expect(
    clippy::too_many_arguments,
    reason = "mirrors foreign key action fields"
)]
fn build_direct_foreign_key_replace<T: AsRef<str>, U: AsRef<str>, V: AsRef<str>>(
    table: &str,
    old_name: Option<&str>,
    old_columns: &[T],
    new_name: Option<&str>,
    new_columns: &[U],
    ref_table: &str,
    ref_columns: &[V],
    on_delete: Option<&vespertide_core::ReferenceAction>,
    on_update: Option<&vespertide_core::ReferenceAction>,
) -> Vec<BuiltQuery> {
    let old_fk_name = vespertide_naming::build_foreign_key_name(table, old_columns, old_name);
    let fk_drop = ForeignKey::drop()
        .name(&old_fk_name)
        .table(Alias::new(table))
        .to_owned();
    let fk_create = build_replacement_foreign_key(
        table,
        new_name,
        new_columns,
        ref_table,
        ref_columns,
        on_delete,
        on_update,
    );

    vec![
        BuiltQuery::DropForeignKey(Box::new(fk_drop)),
        BuiltQuery::CreateForeignKey(Box::new(fk_create)),
    ]
}

fn build_replacement_foreign_key<T: AsRef<str>, U: AsRef<str>>(
    table: &str,
    new_name: Option<&str>,
    new_columns: &[T],
    ref_table: &str,
    ref_columns: &[U],
    on_delete: Option<&vespertide_core::ReferenceAction>,
    on_update: Option<&vespertide_core::ReferenceAction>,
) -> sea_query::ForeignKeyCreateStatement {
    let new_fk_name = vespertide_naming::build_foreign_key_name(table, new_columns, new_name);
    let mut fk_create = ForeignKey::create();
    fk_create.name(&new_fk_name);
    fk_create.from_tbl(Alias::new(table));
    for col in new_columns {
        fk_create.from_col(Alias::new(col.as_ref()));
    }
    fk_create.to_tbl(Alias::new(ref_table));
    for col in ref_columns {
        fk_create.to_col(Alias::new(col.as_ref()));
    }
    if let Some(action) = on_delete {
        fk_create.on_delete(to_sea_fk_action(action));
    }
    if let Some(action) = on_update {
        fk_create.on_update(to_sea_fk_action(action));
    }
    fk_create
}

/// `SQLite`: single temp table recreation with the constraint replaced.
/// Works for all constraint types (FK, Check, Unique, Index, PK).
fn build_sqlite_constraint_replace(
    backend: DatabaseBackend,
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
            QueryError::SchemaError(format!(
                "Table '{table}' not found in current schema. SQLite requires current schema \
                 information to replace constraints."
            ))
        })?;

    // Build new constraints: replace old constraint with new one
    let new_constraints: Vec<TableConstraint> = table_def
        .constraints
        .iter()
        .map(|c| if c == from { to.clone() } else { c.clone() })
        .collect();

    let temp_table = format!("{table}_temp");

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
        select_query.column(col_alias.clone());
    }
    select_query.from(Alias::new(table));

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
                    strategy: vespertide_core::PrimaryKeyAdditionStrategy::default(),
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
                        strategy: vespertide_core::PrimaryKeyAdditionStrategy::default(),
                    },
                    TableConstraint::ForeignKey {
                        name: Some("fk_user".into()),
                        columns: vec!["user_id".into()],
                        ref_table: "users".into(),
                        ref_columns: vec!["id".into()],
                        on_delete: None,
                        on_update: None,
                        orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
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
            orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
        };
        let to = TableConstraint::ForeignKey {
            name: Some("fk_user".into()),
            columns: vec!["user_id".into()],
            ref_table: "users".into(),
            ref_columns: vec!["id".into()],
            on_delete: Some(ReferenceAction::Cascade),
            on_update: None,
            orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
        };

        let queries = build_replace_constraint(backend, "posts", &from, &to, &schema, &[])
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

    #[rstest]
    #[case::postgres(DatabaseBackend::Postgres)]
    #[case::mysql(DatabaseBackend::MySql)]
    #[case::sqlite(DatabaseBackend::Sqlite)]
    fn replace_fk_on_update(#[case] backend: DatabaseBackend) {
        let schema = test_schema();
        let from = TableConstraint::ForeignKey {
            name: Some("fk_user".into()),
            columns: vec!["user_id".into()],
            ref_table: "users".into(),
            ref_columns: vec!["id".into()],
            on_delete: None,
            on_update: None,
            orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
        };
        let to = TableConstraint::ForeignKey {
            name: Some("fk_user".into()),
            columns: vec!["user_id".into()],
            ref_table: "users".into(),
            ref_columns: vec!["id".into()],
            on_delete: None,
            on_update: Some(ReferenceAction::Cascade),
            orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
        };

        let queries = build_replace_constraint(backend, "posts", &from, &to, &schema, &[])
            .expect("should succeed");
        let sql: Vec<String> = queries.iter().map(|q| q.build(backend)).collect();
        let combined = sql.join(";\n");

        with_settings!({
            description => format!("replace FK on_update for {:?}", backend),
            omit_expression => true,
            snapshot_suffix => format!("replace_fk_on_update_{:?}", backend),
        }, {
            assert_snapshot!(combined);
        });
    }

    #[rstest]
    #[case::postgres(DatabaseBackend::Postgres)]
    #[case::mysql(DatabaseBackend::MySql)]
    #[case::sqlite(DatabaseBackend::Sqlite)]
    fn replace_unique_constraint(#[case] backend: DatabaseBackend) {
        // Non-FK constraint: PG/MySQL uses remove+add, SQLite uses temp table
        // Multi-table schema so the non-target table hits the else branch (t.clone())
        let schema = vec![
            TableDef {
                name: "other".into(),
                description: None,
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
                constraints: vec![],
            },
            TableDef {
                name: "users".into(),
                description: None,
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
                ],
                constraints: vec![
                    TableConstraint::PrimaryKey {
                        auto_increment: false,
                        columns: vec!["id".into()],
                        strategy: vespertide_core::PrimaryKeyAdditionStrategy::default(),
                    },
                    TableConstraint::Unique {
                        name: Some("uq_email".into()),
                        columns: vec!["email".into()],
                        strategy: vespertide_core::UniqueConstraintStrategy::DeleteDuplicates {
                            keep: vespertide_core::KeepPolicy::First,
                        },
                    },
                ],
            },
        ];
        let from = TableConstraint::Unique {
            name: Some("uq_email".into()),
            columns: vec!["email".into()],
            strategy: vespertide_core::UniqueConstraintStrategy::DeleteDuplicates {
                keep: vespertide_core::KeepPolicy::First,
            },
        };
        let to = TableConstraint::Unique {
            name: Some("uq_email_new".into()),
            columns: vec!["email".into()],
            strategy: vespertide_core::UniqueConstraintStrategy::DeleteDuplicates {
                keep: vespertide_core::KeepPolicy::First,
            },
        };

        let queries = build_replace_constraint(backend, "users", &from, &to, &schema, &[])
            .expect("should succeed");
        let sql: Vec<String> = queries.iter().map(|q| q.build(backend)).collect();
        let combined = sql.join(";\n");

        with_settings!({
            description => format!("replace unique constraint for {:?}", backend),
            omit_expression => true,
            snapshot_suffix => format!("replace_unique_{:?}", backend),
        }, {
            assert_snapshot!(combined);
        });
    }

    #[test]
    fn replace_constraint_table_not_found_sqlite() {
        let from = TableConstraint::Unique {
            name: Some("uq_old".into()),
            columns: vec!["col".into()],
            strategy: vespertide_core::UniqueConstraintStrategy::DeleteDuplicates {
                keep: vespertide_core::KeepPolicy::First,
            },
        };
        let to = TableConstraint::Unique {
            name: Some("uq_new".into()),
            columns: vec!["col".into()],
            strategy: vespertide_core::UniqueConstraintStrategy::DeleteDuplicates {
                keep: vespertide_core::KeepPolicy::First,
            },
        };
        let err =
            build_replace_constraint(DatabaseBackend::Sqlite, "missing", &from, &to, &[], &[])
                .unwrap_err();
        assert!(format!("{err}").contains("missing"));
    }
}
