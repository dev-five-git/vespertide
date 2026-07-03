use sea_query::{Alias, Query, Table};

use vespertide_core::{TableConstraint, TableDef};

use crate::error::QueryError;
use crate::sql::helpers::{build_sqlite_temp_table_create, recreate_indexes_after_rebuild};
use crate::sql::rename_table::build_rename_table;
use crate::sql::types::{BuiltQuery, DatabaseBackend};

pub fn requires_rebuild(constraint: &TableConstraint) -> bool {
    matches!(
        constraint,
        TableConstraint::PrimaryKey { .. }
            | TableConstraint::Unique { .. }
            | TableConstraint::ForeignKey { .. }
            | TableConstraint::Check { .. }
    )
}

pub fn build_remove_constraint(
    table: &str,
    constraint: &TableConstraint,
    current_schema: &[TableDef],
    pending_constraints: &[TableConstraint],
) -> Result<Vec<BuiltQuery>, QueryError> {
    let table_def = current_schema.iter().find(|t| t.name == table).ok_or_else(|| QueryError::SchemaError(format!("Table '{table}' not found in current schema. SQLite requires current schema information to remove constraints.")))?;

    let new_constraints = constraints_without(table_def, constraint);
    let constraints_to_recreate = if matches!(constraint, TableConstraint::Unique { .. }) {
        &new_constraints
    } else {
        &table_def.constraints
    };

    Ok(rebuild_table_without_constraint(
        table,
        table_def,
        &new_constraints,
        constraints_to_recreate,
        pending_constraints,
    ))
}

fn constraints_without(table_def: &TableDef, removed: &TableConstraint) -> Vec<TableConstraint> {
    table_def
        .constraints
        .iter()
        .filter(|candidate| !same_constraint(candidate, removed))
        .cloned()
        .collect()
}

fn same_constraint(candidate: &TableConstraint, removed: &TableConstraint) -> bool {
    match (candidate, removed) {
        (TableConstraint::PrimaryKey { .. }, TableConstraint::PrimaryKey { .. }) => true,
        (
            TableConstraint::Unique {
                name: candidate_name,
                columns: candidate_columns,
                ..
            },
            TableConstraint::Unique {
                name: removed_name,
                columns: removed_columns,
                ..
            },
        )
        | (
            TableConstraint::ForeignKey {
                name: candidate_name,
                columns: candidate_columns,
                ..
            },
            TableConstraint::ForeignKey {
                name: removed_name,
                columns: removed_columns,
                ..
            },
        ) => same_named_or_column_constraint(
            candidate_name.as_ref(),
            candidate_columns,
            removed_name.as_ref(),
            removed_columns,
        ),
        (
            TableConstraint::Check {
                name: candidate_name,
                ..
            },
            TableConstraint::Check {
                name: removed_name, ..
            },
        ) => candidate_name == removed_name,
        _ => false,
    }
}

fn same_named_or_column_constraint<T: AsRef<str>, U: AsRef<str>>(
    candidate_name: Option<&String>,
    candidate_columns: &[T],
    removed_name: Option<&String>,
    removed_columns: &[U],
) -> bool {
    if let (Some(candidate_name), Some(removed_name)) = (candidate_name, removed_name) {
        candidate_name == removed_name
    } else {
        candidate_columns.len() == removed_columns.len()
            && candidate_columns
                .iter()
                .zip(removed_columns)
                .all(|(candidate, removed)| candidate.as_ref() == removed.as_ref())
    }
}

fn rebuild_table_without_constraint(
    table: &str,
    table_def: &TableDef,
    new_constraints: &[TableConstraint],
    constraints_to_recreate: &[TableConstraint],
    pending_constraints: &[TableConstraint],
) -> Vec<BuiltQuery> {
    let temp_table = format!("{table}_temp");

    // SQLite has no native ALTER TABLE DROP CONSTRAINT. Keep the canonical rebuild sequence:
    // create temp table, copy rows, drop original, rename temp, then recreate indexes.
    let create_query = build_sqlite_temp_table_create(
        DatabaseBackend::Sqlite,
        &temp_table,
        table,
        &table_def.columns,
        new_constraints,
    );
    let insert_query = build_copy_into_temp_table(table, &temp_table, table_def);
    let drop_query =
        BuiltQuery::DropTable(Box::new(Table::drop().table(Alias::new(table)).to_owned()));
    let rename_query = build_rename_table(&temp_table, table);
    let index_queries =
        recreate_indexes_after_rebuild(table, constraints_to_recreate, pending_constraints);

    let mut queries = vec![create_query, insert_query, drop_query, rename_query];
    queries.extend(index_queries);
    queries
}

fn build_copy_into_temp_table(table: &str, temp_table: &str, table_def: &TableDef) -> BuiltQuery {
    let column_aliases: Vec<Alias> = table_def
        .columns
        .iter()
        .map(|column| Alias::new(&column.name))
        .collect();

    let mut select_query = Query::select();
    for column_alias in &column_aliases {
        select_query.column(column_alias.clone());
    }
    select_query.from(Alias::new(table));

    let insert_stmt = Query::insert()
        .into_table(Alias::new(temp_table))
        .columns(column_aliases)
        .select_from(select_query)
        .expect("SQLite temp table copy SELECT should be valid")
        .to_owned();

    BuiltQuery::Insert(Box::new(insert_stmt))
}
