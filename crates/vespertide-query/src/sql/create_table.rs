use sea_query::{Alias, ForeignKey, Index, Table, TableCreateStatement};

use vespertide_core::{ColumnDef, ColumnType, ComplexColumnType, TableConstraint};

use super::helpers::{
    build_create_enum_type_sql, build_create_with_checks, build_sea_column_def_with_table,
    collect_sqlite_enum_check_clauses, to_sea_fk_action,
};
use super::types::{BuiltQuery, DatabaseBackend};
use crate::error::QueryError;

pub(crate) fn build_create_table_for_backend(
    backend: DatabaseBackend,
    table: &str,
    columns: &[ColumnDef],
    constraints: &[TableConstraint],
) -> TableCreateStatement {
    let mut stmt = Table::create().table(Alias::new(table)).to_owned();
    add_create_table_columns(&mut stmt, backend, table, columns, constraints);
    add_create_table_constraints(&mut stmt, backend, table, columns, constraints);
    stmt
}

fn add_create_table_columns(
    stmt: &mut TableCreateStatement,
    backend: DatabaseBackend,
    table: &str,
    columns: &[ColumnDef],
    constraints: &[TableConstraint],
) {
    let has_table_primary_key = constraints
        .iter()
        .any(|c| matches!(c, TableConstraint::PrimaryKey { .. }));
    // The vast majority of tables have no `PrimaryKey { auto_increment: true }`
    // constraint, so skip the `HashSet` allocation + constraint scan entirely in
    // that common case; an empty set produces byte-identical output.
    let auto_increment_columns = if constraints.iter().any(|c| {
        matches!(
            c,
            TableConstraint::PrimaryKey {
                auto_increment: true,
                ..
            }
        )
    }) {
        collect_auto_increment_columns(constraints)
    } else {
        std::collections::HashSet::new()
    };

    for column in columns {
        let mut col = build_sea_column_def_with_table(backend, table, column);
        if column.primary_key.is_some() && !has_table_primary_key {
            col.primary_key();
        }
        if auto_increment_columns.contains(column.name.as_str())
            && column.r#type.supports_auto_increment()
        {
            if matches!(backend, DatabaseBackend::Sqlite) {
                col.primary_key();
            }
            col.auto_increment();
        }
        stmt.col(col);
    }
}

fn collect_auto_increment_columns(
    constraints: &[TableConstraint],
) -> std::collections::HashSet<&str> {
    constraints
        .iter()
        .filter_map(|c| {
            if let TableConstraint::PrimaryKey {
                columns: pk_cols,
                auto_increment: true,
                ..
            } = c
            {
                Some(pk_cols.iter().map(AsRef::as_ref))
            } else {
                None
            }
        })
        .flatten()
        .collect()
}

fn add_create_table_constraints(
    stmt: &mut TableCreateStatement,
    backend: DatabaseBackend,
    table: &str,
    columns: &[ColumnDef],
    constraints: &[TableConstraint],
) {
    for constraint in constraints {
        match constraint {
            TableConstraint::PrimaryKey {
                columns: pk_cols,
                auto_increment,
                ..
            } => add_primary_key_constraint(stmt, backend, columns, pk_cols, *auto_increment),
            TableConstraint::Unique {
                name,
                columns: unique_cols,
                ..
            } => add_mysql_unique_constraint(stmt, backend, table, unique_cols, name.as_deref()),
            TableConstraint::ForeignKey { .. } => {
                add_foreign_key_constraint(stmt, table, constraint);
            }
            // CHECK is emitted elsewhere; indexes are created separately. Both
            // are no-ops during CREATE TABLE constraint assembly.
            TableConstraint::Check { .. } | TableConstraint::Index { .. } => {}
            _ => {
                unreachable!("TableConstraint is #[non_exhaustive]; all variants are matched above")
            }
        }
    }
}

fn add_primary_key_constraint(
    stmt: &mut TableCreateStatement,
    backend: DatabaseBackend,
    columns: &[ColumnDef],
    pk_cols: &[impl AsRef<str>],
    auto_increment: bool,
) {
    if should_skip_sqlite_auto_increment_pk(backend, columns, pk_cols, auto_increment) {
        return;
    }

    let mut pk_idx = Index::create();
    for c in pk_cols {
        pk_idx.col(Alias::new(c.as_ref()));
    }
    stmt.primary_key(&mut pk_idx);
}

fn should_skip_sqlite_auto_increment_pk(
    backend: DatabaseBackend,
    columns: &[ColumnDef],
    pk_cols: &[impl AsRef<str>],
    auto_increment: bool,
) -> bool {
    matches!(backend, DatabaseBackend::Sqlite)
        && auto_increment
        && pk_cols.iter().all(|col_name| {
            columns
                .iter()
                .any(|c| c.name == col_name.as_ref() && c.r#type.supports_auto_increment())
        })
}

fn add_mysql_unique_constraint(
    stmt: &mut TableCreateStatement,
    backend: DatabaseBackend,
    table: &str,
    unique_cols: &[impl AsRef<str>],
    name: Option<&str>,
) {
    if !matches!(backend, DatabaseBackend::MySql) {
        return;
    }

    let index_name = super::helpers::build_unique_constraint_name(table, unique_cols, name);
    let mut idx = Index::create().name(&index_name).unique().to_owned();
    for col in unique_cols {
        idx.col(Alias::new(col.as_ref()));
    }
    stmt.index(&mut idx);
}

fn add_foreign_key_constraint(
    stmt: &mut TableCreateStatement,
    table: &str,
    constraint: &TableConstraint,
) {
    if let TableConstraint::ForeignKey {
        name,
        columns: fk_cols,
        ref_table,
        ref_columns,
        on_delete,
        on_update,
        ..
    } = constraint
    {
        let fk_name = super::helpers::build_foreign_key_name(table, fk_cols, name.as_deref());
        let mut fk = ForeignKey::create().name(&fk_name).to_owned();
        fk.from_tbl(Alias::new(table));
        for col in fk_cols {
            fk.from_col(Alias::new(col));
        }
        fk.to_tbl(Alias::new(ref_table));
        for col in ref_columns {
            fk.to_col(Alias::new(col));
        }
        if let Some(action) = on_delete {
            fk.on_delete(to_sea_fk_action(action));
        }
        if let Some(action) = on_update {
            fk.on_update(to_sea_fk_action(action));
        }
        stmt.foreign_key(&mut fk);
    }
}

pub fn build_create_table(
    backend: DatabaseBackend,
    table: &str,
    columns: &[ColumnDef],
    constraints: &[TableConstraint],
) -> Result<Vec<BuiltQuery>, QueryError> {
    // Normalize the table to convert inline constraints to table-level
    // This ensures we don't have duplicate constraints if both inline and table-level are defined
    let table_def = vespertide_core::TableDef {
        description: None,
        name: table.into(),
        columns: columns.to_vec(),
        constraints: constraints.to_vec(),
    };
    let normalized = table_def.normalize().map_err(|e| {
        QueryError::SchemaError(format!("Failed to normalize table '{table}': {e}"))
    })?;

    // Use normalized columns and constraints for SQL generation
    let columns = &normalized.columns;
    let constraints = &normalized.constraints;

    // Pre-size for the CREATE TABLE statement plus up to one CREATE INDEX per
    // constraint (uniques + indexes); enum-type `Raw`s are rare and bounded by
    // the enum-column count, so a small over/under-estimate costs at most one
    // realloc, never correctness.
    let mut queries = Vec::with_capacity(1 + constraints.len());

    // Create enum types first (PostgreSQL only)
    // Collect unique enum types to avoid duplicates.
    // The vast majority of tables have no enum columns, so skip the `HashSet`
    // allocation + column scan entirely in that common case; the loop pushes
    // nothing when there are no enum columns, so the output is byte-identical.
    let has_enum_columns = columns.iter().any(|c| {
        matches!(
            c.r#type,
            ColumnType::Complex(ComplexColumnType::Enum { .. })
        )
    });
    if has_enum_columns {
        let mut created_enums: std::collections::HashSet<&str> = std::collections::HashSet::new();
        for column in columns {
            if let ColumnType::Complex(ComplexColumnType::Enum { name, .. }) = &column.r#type
                && created_enums.insert(name.as_str())
                && let Some(create_type_sql) = build_create_enum_type_sql(table, &column.r#type)
            {
                queries.push(BuiltQuery::Raw(create_type_sql));
            }
        }
    }

    // Build CREATE TABLE
    // For MySQL, include unique constraints in CREATE TABLE (no partition needed).
    // For Postgres and SQLite, exclude them here — they are emitted below as
    // separate CREATE UNIQUE INDEX statements, so partition off the uniques only
    // in that branch (the MySQL path discards both halves, so allocating them
    // there is pure waste).
    let has_unique_constraints = constraints
        .iter()
        .any(|c| matches!(c, TableConstraint::Unique { .. }));
    let create_table_stmt = if matches!(backend, DatabaseBackend::MySql) || !has_unique_constraints
    {
        // MySQL keeps uniques inline; the PG/SQLite path emits them as separate
        // CREATE UNIQUE INDEX statements below, so when there are no uniques to
        // strip the constraint slice can pass straight through with no clone.
        build_create_table_for_backend(backend, table, columns, constraints)
    } else {
        // PG/SQLite with at least one unique: partition off the uniques into an
        // owned Vec so they are excluded from the inline CREATE TABLE.
        let table_constraints_owned: Vec<TableConstraint> = constraints
            .iter()
            .filter(|c| !matches!(c, TableConstraint::Unique { .. }))
            .cloned()
            .collect();
        build_create_table_for_backend(backend, table, columns, &table_constraints_owned)
    };

    // For SQLite, add CHECK constraints for enum columns
    if matches!(backend, DatabaseBackend::Sqlite) {
        let enum_check_clauses = collect_sqlite_enum_check_clauses(table, columns);
        if enum_check_clauses.is_empty() {
            queries.push(BuiltQuery::CreateTable(Box::new(create_table_stmt)));
        } else {
            // Embed CHECK constraints into the CREATE TABLE statement via the
            // single source of truth for CHECK injection.
            queries.push(build_create_with_checks(
                backend,
                &create_table_stmt,
                &enum_check_clauses,
            ));
        }
    } else {
        queries.push(BuiltQuery::CreateTable(Box::new(create_table_stmt)));
    }

    // For Postgres and SQLite, add unique constraints as separate CREATE UNIQUE INDEX statements
    if matches!(backend, DatabaseBackend::Postgres | DatabaseBackend::Sqlite) {
        for constraint in constraints {
            if let TableConstraint::Unique {
                name,
                columns: unique_cols,
                ..
            } = constraint
            {
                // Always generate a proper name: uq_{table}_{key} or uq_{table}_{columns}
                let index_name = super::helpers::build_unique_constraint_name(
                    table,
                    unique_cols,
                    name.as_deref(),
                );
                let mut idx = Index::create()
                    .table(Alias::new(table))
                    .name(&index_name)
                    .unique()
                    .to_owned();
                for col in unique_cols {
                    idx.col(Alias::new(col));
                }
                queries.push(BuiltQuery::CreateIndex(Box::new(idx)));
            }
        }
    }

    // Add Index constraints as CREATE INDEX statements (for all backends)
    for constraint in constraints {
        if let TableConstraint::Index {
            name,
            columns: index_cols,
        } = constraint
        {
            // Always generate a proper name: ix_{table}_{key} or ix_{table}_{columns}
            let index_name = super::helpers::build_index_name(table, index_cols, name.as_deref());
            let mut idx = Index::create()
                .table(Alias::new(table))
                .name(&index_name)
                .to_owned();
            for col in index_cols {
                idx.col(Alias::new(col));
            }
            queries.push(BuiltQuery::CreateIndex(Box::new(idx)));
        }
    }

    Ok(queries)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{col, joined_sql};
    use insta::{assert_snapshot, with_settings};
    use rstest::rstest;
    use vespertide_core::{ColumnType, EnumValues, SimpleColumnType};

    fn join_queries(queries: &[BuiltQuery], backend: DatabaseBackend, separator: &str) -> String {
        let mut sql = String::new();
        for (index, query) in queries.iter().enumerate() {
            if index > 0 {
                sql.push_str(separator);
            }
            sql.push_str(&query.build(backend));
        }
        sql
    }

    #[rstest]
    #[case::create_table_postgres(
        "create_table_postgres",
        DatabaseBackend::Postgres,
        &["CREATE TABLE \"users\" ( \"id\" integer )"]
    )]
    #[case::create_table_mysql(
        "create_table_mysql",
        DatabaseBackend::MySql,
        &["CREATE TABLE `users` ( `id` int )"]
    )]
    #[case::create_table_sqlite(
        "create_table_sqlite",
        DatabaseBackend::Sqlite,
        &["CREATE TABLE \"users\" ( \"id\" integer )"]
    )]
    fn test_create_table(
        #[case] title: &str,
        #[case] backend: DatabaseBackend,
        #[case] expected: &[&str],
    ) {
        let result = build_create_table(
            backend,
            "users",
            &[col("id", ColumnType::Simple(SimpleColumnType::Integer))],
            &[],
        )
        .unwrap();
        let sql = join_queries(&result, backend, "\n");
        for exp in expected {
            assert!(
                sql.contains(exp),
                "Expected SQL to contain '{exp}', got: {sql}"
            );
        }

        with_settings!({ snapshot_suffix => format!("create_table_{}", title) }, {
            assert_snapshot!(sql);
        });
    }

    #[rstest]
    #[case::inline_unique_postgres(DatabaseBackend::Postgres)]
    #[case::inline_unique_mysql(DatabaseBackend::MySql)]
    #[case::inline_unique_sqlite(DatabaseBackend::Sqlite)]
    fn test_create_table_with_inline_unique(#[case] backend: DatabaseBackend) {
        // Test that inline unique constraint is converted to table-level during normalization.
        // build_create_table now normalizes the table, so inline unique becomes a CREATE UNIQUE INDEX.
        use vespertide_core::schema::str_or_bool::StrOrBoolOrArray;

        let mut email_col = col("email", ColumnType::Simple(SimpleColumnType::Text));
        email_col.unique = Some(StrOrBoolOrArray::Bool(true));

        let result = build_create_table(
            backend,
            "users",
            &[
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                email_col,
            ],
            // No explicit table-level unique constraint passed, but normalize will create one from inline
            &[],
        )
        .unwrap();
        let sql = join_queries(&result, backend, "\n");

        // After normalization, inline unique should produce UNIQUE constraint in SQL
        assert!(
            sql.contains("UNIQUE") || sql.to_uppercase().contains("UNIQUE"),
            "Normalized unique constraint should be in SQL, but not found: {sql}"
        );
        with_settings!({ snapshot_suffix => format!("create_table_with_inline_unique_{:?}", backend) }, {
            assert_snapshot!(sql);
        });
    }

    #[rstest]
    #[case::table_level_unique_postgres(DatabaseBackend::Postgres)]
    #[case::table_level_unique_mysql(DatabaseBackend::MySql)]
    #[case::table_level_unique_sqlite(DatabaseBackend::Sqlite)]
    fn test_create_table_with_table_level_unique(#[case] backend: DatabaseBackend) {
        // Test table-level unique constraint (lines 53-54, 56-58, 60-61)
        let result = build_create_table(
            backend,
            "users",
            &[
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                col("email", ColumnType::Simple(SimpleColumnType::Text)),
            ],
            &[TableConstraint::Unique {
                name: Some("uq_email".into()),
                columns: vec!["email".into()],
                strategy: vespertide_core::UniqueConstraintStrategy::DeleteDuplicates {
                    keep: vespertide_core::KeepPolicy::First,
                },
            }],
        )
        .unwrap();
        let sql = join_queries(&result, backend, "\n");
        assert!(sql.contains("CREATE TABLE"));
        // Verify unique constraint is present
        match backend {
            DatabaseBackend::MySql => {
                assert!(
                    sql.contains("UNIQUE"),
                    "MySQL should have UNIQUE in CREATE TABLE: {sql}"
                );
            }
            _ => {
                // For Postgres and SQLite, unique constraint should be in a separate CREATE UNIQUE INDEX statement
                assert!(
                    sql.contains("CREATE UNIQUE INDEX"),
                    "Postgres/SQLite should have CREATE UNIQUE INDEX: {sql}"
                );
            }
        }
        with_settings!({ snapshot_suffix => format!("create_table_with_table_level_unique_{:?}", backend) }, {
            assert_snapshot!(sql);
        });
    }

    /// The unique-only fixture above cannot see whether the PG/SQLite branch
    /// *strips* uniques or *keeps only* uniques - both leave the same effective
    /// constraint set when the unique is the sole entry. Pairing the unique with
    /// a PRIMARY KEY makes the two behaviours diverge: keeping only uniques
    /// would drop the PK from the emitted CREATE TABLE.
    #[rstest]
    #[case::unique_beside_pk_postgres(DatabaseBackend::Postgres)]
    #[case::unique_beside_pk_mysql(DatabaseBackend::MySql)]
    #[case::unique_beside_pk_sqlite(DatabaseBackend::Sqlite)]
    fn test_create_table_unique_alongside_primary_key(#[case] backend: DatabaseBackend) {
        let result = build_create_table(
            backend,
            "users",
            &[
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                col("email", ColumnType::Simple(SimpleColumnType::Text)),
            ],
            &[
                TableConstraint::PrimaryKey {
                    columns: vec!["id".into()],
                    auto_increment: false,
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
        )
        .unwrap();
        let sql = join_queries(&result, backend, "\n");

        assert!(
            sql.contains("PRIMARY KEY"),
            "the PK must survive the unique partitioning: {sql}"
        );
        with_settings!({ snapshot_suffix => format!("create_table_unique_alongside_primary_key_{:?}", backend) }, {
            assert_snapshot!(sql);
        });
    }

    #[rstest]
    #[case::table_level_unique_no_name_postgres(DatabaseBackend::Postgres)]
    #[case::table_level_unique_no_name_mysql(DatabaseBackend::MySql)]
    #[case::table_level_unique_no_name_sqlite(DatabaseBackend::Sqlite)]
    fn test_create_table_with_table_level_unique_no_name(#[case] backend: DatabaseBackend) {
        // Test table-level unique constraint without name (lines 53-54, 56-58, 60-61)
        let result = build_create_table(
            backend,
            "users",
            &[
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                col("email", ColumnType::Simple(SimpleColumnType::Text)),
            ],
            &[TableConstraint::Unique {
                name: None,
                columns: vec!["email".into()],
                strategy: vespertide_core::UniqueConstraintStrategy::DeleteDuplicates {
                    keep: vespertide_core::KeepPolicy::First,
                },
            }],
        )
        .unwrap();
        let sql = join_queries(&result, backend, "\n");
        assert!(sql.contains("CREATE TABLE"));
        // Verify unique constraint is present
        match backend {
            DatabaseBackend::MySql => {
                assert!(
                    sql.contains("UNIQUE"),
                    "MySQL should have UNIQUE in CREATE TABLE: {sql}"
                );
            }
            _ => {
                // For Postgres and SQLite, unique constraint should be in a separate CREATE UNIQUE INDEX statement
                assert!(
                    sql.contains("CREATE UNIQUE INDEX"),
                    "Postgres/SQLite should have CREATE UNIQUE INDEX: {sql}"
                );
            }
        }
        with_settings!({ snapshot_suffix => format!("create_table_with_table_level_unique_no_name_{:?}", backend) }, {
            assert_snapshot!(sql);
        });
    }

    #[rstest]
    #[case::postgres(DatabaseBackend::Postgres)]
    #[case::mysql(DatabaseBackend::MySql)]
    #[case::sqlite(DatabaseBackend::Sqlite)]
    fn test_create_table_with_enum_column(#[case] backend: DatabaseBackend) {
        // Test creating a table with an enum column (should create enum type first for PostgreSQL)
        let columns = vec![
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
                name: "status".into(),
                r#type: ColumnType::Complex(ComplexColumnType::Enum {
                    name: "user_status".into(),
                    values: EnumValues::String(vec![
                        "active".into(),
                        "inactive".into(),
                        "pending".into(),
                    ]),
                }),
                nullable: false,
                default: Some("'active'".into()),
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            },
        ];
        let constraints = vec![TableConstraint::PrimaryKey {
            auto_increment: false,
            columns: vec!["id".into()],
            strategy: vespertide_core::PrimaryKeyAdditionStrategy::default(),
        }];

        let result = build_create_table(backend, "users", &columns, &constraints);
        assert!(result.is_ok());
        let queries = result.unwrap();
        let sql = join_queries(&queries, backend, ";\n");

        with_settings!({ snapshot_suffix => format!("create_table_with_enum_column_{:?}", backend) }, {
            assert_snapshot!(sql);
        });
    }

    #[rstest]
    #[case::auto_increment_postgres(DatabaseBackend::Postgres)]
    #[case::auto_increment_mysql(DatabaseBackend::MySql)]
    #[case::auto_increment_sqlite(DatabaseBackend::Sqlite)]
    fn test_create_table_with_auto_increment_primary_key(#[case] backend: DatabaseBackend) {
        // Test that auto_increment on primary key generates SERIAL/AUTO_INCREMENT/AUTOINCREMENT
        let columns = vec![ColumnDef {
            name: "id".into(),
            r#type: ColumnType::Simple(SimpleColumnType::Integer),
            nullable: false,
            default: None,
            comment: None,
            primary_key: None,
            unique: None,
            index: None,
            foreign_key: None,
        }];
        let constraints = vec![TableConstraint::PrimaryKey {
            auto_increment: true,
            columns: vec!["id".into()],
            strategy: vespertide_core::PrimaryKeyAdditionStrategy::default(),
        }];

        let result = build_create_table(backend, "users", &columns, &constraints);
        assert!(result.is_ok());
        let queries = result.unwrap();
        let sql = join_queries(&queries, backend, ";\n");

        // Verify auto_increment is applied correctly for each backend
        match backend {
            DatabaseBackend::Postgres => {
                // sea-query 1.0.0-rc.33 renders PG auto-increment as the
                // SQL-standard `GENERATED BY DEFAULT AS IDENTITY` (PG 10+),
                // whereas the 1.0.0 final line used `SERIAL`. Both express an
                // auto-incrementing PK; accept either form.
                assert!(
                    sql.contains("SERIAL")
                        || sql.contains("serial")
                        || sql.contains("IDENTITY")
                        || sql.contains("identity"),
                    "PostgreSQL should use SERIAL or IDENTITY for auto_increment, got: {sql}"
                );
            }
            DatabaseBackend::MySql => {
                assert!(
                    sql.contains("AUTO_INCREMENT") || sql.contains("auto_increment"),
                    "MySQL should use AUTO_INCREMENT for auto_increment, got: {sql}"
                );
            }
            DatabaseBackend::Sqlite => {
                assert!(
                    sql.contains("AUTOINCREMENT") || sql.contains("autoincrement"),
                    "SQLite should use AUTOINCREMENT for auto_increment, got: {sql}"
                );
            }
        }

        with_settings!({ snapshot_suffix => format!("create_table_with_auto_increment_{:?}", backend) }, {
            assert_snapshot!(sql);
        });
    }

    #[rstest]
    #[case::auto_increment_unsupported_type_postgres(DatabaseBackend::Postgres)]
    #[case::auto_increment_unsupported_type_mysql(DatabaseBackend::MySql)]
    #[case::auto_increment_unsupported_type_sqlite(DatabaseBackend::Sqlite)]
    fn test_create_table_auto_increment_pk_on_non_integer_column(#[case] backend: DatabaseBackend) {
        // SQLite folds an auto-increment PK into the column definition
        // (`INTEGER PRIMARY KEY AUTOINCREMENT`) and therefore SKIPS the separate
        // PRIMARY KEY clause — but only when the PK column's type can actually
        // carry AUTOINCREMENT. TEXT cannot, so nothing is folded into the column
        // and the explicit PRIMARY KEY clause must survive on every backend.
        let columns = vec![col("id", ColumnType::Simple(SimpleColumnType::Text))];
        let constraints = vec![TableConstraint::PrimaryKey {
            auto_increment: true,
            columns: vec!["id".into()],
            strategy: vespertide_core::PrimaryKeyAdditionStrategy::default(),
        }];

        let queries = build_create_table(backend, "users", &columns, &constraints).unwrap();
        let sql = join_queries(&queries, backend, ";\n");

        assert!(
            sql.to_uppercase().contains("PRIMARY KEY"),
            "auto-increment PK on a type without auto-increment support must still \
             emit an explicit PRIMARY KEY, got: {sql}"
        );

        with_settings!({ snapshot_suffix => format!("create_table_auto_increment_pk_on_non_integer_column_{:?}", backend) }, {
            assert_snapshot!(sql);
        });
    }

    #[rstest]
    #[case::inline_auto_increment_postgres(DatabaseBackend::Postgres)]
    #[case::inline_auto_increment_mysql(DatabaseBackend::MySql)]
    #[case::inline_auto_increment_sqlite(DatabaseBackend::Sqlite)]
    fn test_create_table_with_inline_auto_increment_primary_key(#[case] backend: DatabaseBackend) {
        // Test that inline primary_key with auto_increment generates correct SQL
        use vespertide_core::schema::primary_key::{PrimaryKeyDef, PrimaryKeySyntax};

        let columns = vec![ColumnDef {
            name: "id".into(),
            r#type: ColumnType::Simple(SimpleColumnType::Integer),
            nullable: false,
            default: None,
            comment: None,
            primary_key: Some(PrimaryKeySyntax::Object(PrimaryKeyDef {
                auto_increment: true,
            })),
            unique: None,
            index: None,
            foreign_key: None,
        }];

        let result = build_create_table(backend, "users", &columns, &[]);
        assert!(result.is_ok());
        let queries = result.unwrap();
        let sql = join_queries(&queries, backend, ";\n");

        // Verify auto_increment is applied correctly for each backend
        match backend {
            DatabaseBackend::Postgres => {
                // sea-query 1.0.0-rc.33 renders PG auto-increment as the
                // SQL-standard `GENERATED BY DEFAULT AS IDENTITY` (PG 10+),
                // whereas the 1.0.0 final line used `SERIAL`. Both express an
                // auto-incrementing PK; accept either form.
                assert!(
                    sql.contains("SERIAL")
                        || sql.contains("serial")
                        || sql.contains("IDENTITY")
                        || sql.contains("identity"),
                    "PostgreSQL should use SERIAL or IDENTITY for auto_increment, got: {sql}"
                );
            }
            DatabaseBackend::MySql => {
                assert!(
                    sql.contains("AUTO_INCREMENT") || sql.contains("auto_increment"),
                    "MySQL should use AUTO_INCREMENT for auto_increment, got: {sql}"
                );
            }
            DatabaseBackend::Sqlite => {
                assert!(
                    sql.contains("AUTOINCREMENT") || sql.contains("autoincrement"),
                    "SQLite should use AUTOINCREMENT for auto_increment, got: {sql}"
                );
            }
        }

        with_settings!({ snapshot_suffix => format!("create_table_with_inline_auto_increment_{:?}", backend) }, {
            assert_snapshot!(sql);
        });
    }

    /// Test creating a table with timestamp column and `NOW()` default
    /// `SQLite` should convert `NOW()` to `CURRENT_TIMESTAMP`
    #[rstest]
    #[case::timestamp_now_default_postgres(DatabaseBackend::Postgres)]
    #[case::timestamp_now_default_mysql(DatabaseBackend::MySql)]
    #[case::timestamp_now_default_sqlite(DatabaseBackend::Sqlite)]
    /// CREATE TABLE with `Check` + `Index` constraints exercises the
    /// `TableConstraint::Check` and `TableConstraint::Index` arms of
    /// `add_create_table_constraints` (both silently skip - the
    /// constraints are emitted in separate ALTER statements outside
    /// this builder, but the match arms still execute on every call).
    #[rstest]
    #[case::postgres(DatabaseBackend::Postgres)]
    #[case::mysql(DatabaseBackend::MySql)]
    #[case::sqlite(DatabaseBackend::Sqlite)]
    fn test_create_table_check_and_index_constraints_are_skipped(#[case] backend: DatabaseBackend) {
        // Mix CHECK + INDEX + PrimaryKey so the match dispatcher walks the
        // Check and Index arms (currently no-op) alongside a real PK arm.
        let constraints = vec![
            TableConstraint::PrimaryKey {
                columns: vec!["id".into()],
                auto_increment: false,
                strategy: vespertide_core::PrimaryKeyAdditionStrategy::default(),
            },
            TableConstraint::Check {
                name: "chk_age".into(),
                expr: "age > 0".into(),
                strategy: vespertide_core::CheckViolationStrategy::default(),
            },
            TableConstraint::Index {
                name: Some("idx_age".into()),
                columns: vec!["age".into()],
            },
        ];
        let columns = vec![
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
                name: "age".into(),
                r#type: ColumnType::Simple(SimpleColumnType::Integer),
                nullable: false,
                default: None,
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            },
        ];
        let queries = build_create_table(backend, "users", &columns, &constraints).unwrap();
        let sql = joined_sql(backend, &queries);
        // The CHECK arm in `add_create_table_constraints` is a documented
        // no-op (CHECK appears in a separate ALTER ADD CONSTRAINT statement,
        // not in CREATE TABLE itself). We assert the dispatch ran (PK lands)
        // and the CHECK predicate is NOT inlined in the CREATE TABLE.
        assert!(sql.contains("PRIMARY KEY"));
        assert!(!sql.contains("CHECK (age > 0)"));
        // Index constraints fan out as separate CREATE INDEX statements,
        // which is the documented behaviour of build_create_table.
        assert!(sql.contains("CREATE INDEX") || sql.contains("idx_age"));
    }

    #[rstest]
    #[case::postgres(DatabaseBackend::Postgres)]
    #[case::mysql(DatabaseBackend::MySql)]
    #[case::sqlite(DatabaseBackend::Sqlite)]
    fn test_create_table_with_timestamp_now_default(#[case] backend: DatabaseBackend) {
        let columns = vec![
            ColumnDef {
                name: "id".into(),
                r#type: ColumnType::Simple(SimpleColumnType::BigInt),
                nullable: false,
                default: None,
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            },
            ColumnDef {
                name: "created_at".into(),
                r#type: ColumnType::Simple(SimpleColumnType::Timestamptz),
                nullable: false,
                default: Some("NOW()".into()), // uppercase NOW()
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            },
        ];

        let result = build_create_table(backend, "events", &columns, &[]);
        assert!(result.is_ok(), "build_create_table failed: {result:?}");
        let queries = result.unwrap();
        let sql = join_queries(&queries, backend, "\n");

        // SQLite should NOT have NOW() - it should be converted to CURRENT_TIMESTAMP
        if matches!(backend, DatabaseBackend::Sqlite) {
            assert!(
                !sql.contains("NOW()"),
                "SQLite should not contain NOW(), got: {sql}"
            );
            assert!(
                sql.contains("CURRENT_TIMESTAMP"),
                "SQLite should use CURRENT_TIMESTAMP, got: {sql}"
            );
        }

        with_settings!({ snapshot_suffix => format!("create_table_with_timestamp_now_default_{:?}", backend) }, {
            assert_snapshot!(sql);
        });
    }
}
