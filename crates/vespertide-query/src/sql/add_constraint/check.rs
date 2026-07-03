use vespertide_core::{CheckViolationStrategy, TableConstraint};

use super::super::helpers::quote_ident;
use super::super::types::{BuiltQuery, DatabaseBackend, RawSql};
use super::{QueryError, TableDef, rebuild_sqlite_table_with_added_constraint};

/// Build SQL for adding a CHECK constraint, plus the F4 pre-cleanup
/// statement dictated by `strategy`.
///
/// The cleanup is always emitted (on a table with no violating rows it
/// is a no-op). SQL pattern (PG/MySQL/SQLite uniform):
///
/// - **`NullifyViolatingColumn { column }`**: `UPDATE table SET <column> = NULL WHERE NOT (<expr>);`
/// - **`DeleteViolatingRows`**:               `DELETE FROM table WHERE NOT (<expr>);`
///
/// SQL standard treats `NULL` in the predicate as *not TRUE* (the
/// `WHERE NOT (<expr>)` clause naturally excludes rows whose column is
/// already NULL), so no explicit `IS NOT NULL` guard is needed - rows
/// that already conform to the new constraint via NULL are left
/// untouched on both `NullifyViolatingColumn` and `DeleteViolatingRows`.
///
/// **F11 (`PostgreSQL` only)** ? after the cleanup, PG emits the CHECK in
/// two statements: `ADD CONSTRAINT ... NOT VALID` (immediate, no
/// table-wide lock) followed by `VALIDATE CONSTRAINT` (uses only
/// `SHARE UPDATE EXCLUSIVE`, allows concurrent read/write). Both
/// statements run inside the migration transaction, so PG rollback
/// reverts both on failure ? no partial-apply zombie. `MySQL` emits the
/// CHECK in a single statement (it has no `NOT VALID` equivalent);
/// `SQLite` handles CHECK via the temp-table rebuild path above.
#[expect(
    clippy::too_many_arguments,
    reason = "CHECK builder mirrors action fields plus SQLite schema context plus the F4 cleanup strategy; ConstraintContext is a deferred refactor"
)]
pub(super) fn build_check(
    backend: DatabaseBackend,
    table: &str,
    name: &str,
    expr: &str,
    strategy: &CheckViolationStrategy,
    constraint: &TableConstraint,
    current_schema: &[TableDef],
    pending_constraints: &[TableConstraint],
) -> Result<Vec<BuiltQuery>, QueryError> {
    let cleanup = build_check_violation_cleanup(backend, table, expr, strategy)?;

    if backend == DatabaseBackend::Sqlite {
        let mut queries = cleanup;
        queries.extend(rebuild_sqlite_table_with_added_constraint(
            backend,
            table,
            constraint,
            current_schema,
            pending_constraints,
        )?);
        return Ok(queries);
    }

    let mut queries = cleanup;
    let quoted_table = quote_ident(table, backend);
    let quoted_name = quote_ident(name, backend);

    if backend == DatabaseBackend::Postgres {
        // F11: PG NOT VALID + VALIDATE 2-step. Both statements run
        // inside the migration transaction - rollback reverts both.
        let add_not_valid = format!(
            "ALTER TABLE {quoted_table} ADD CONSTRAINT {quoted_name} CHECK ({expr}) NOT VALID"
        );
        let validate = format!("ALTER TABLE {quoted_table} VALIDATE CONSTRAINT {quoted_name}");
        queries.push(BuiltQuery::Raw(RawSql::uniform(add_not_valid)));
        queries.push(BuiltQuery::Raw(RawSql::uniform(validate)));
    } else {
        // MySQL: single statement (no NOT VALID equivalent). SQLite was
        // handled by the rebuild branch above.
        let mysql_sql =
            format!("ALTER TABLE {quoted_table} ADD CONSTRAINT {quoted_name} CHECK ({expr})");
        queries.push(BuiltQuery::Raw(RawSql::uniform(mysql_sql)));
    }

    Ok(queries)
}

/// Emit the F4 pre-cleanup statement (UPDATE / DELETE) ahead of the
/// `ADD CONSTRAINT CHECK`. Returns an empty `Vec` only when the
/// strategy variant is unrecognised (future-proofing for
/// `non_exhaustive`).
fn build_check_violation_cleanup(
    backend: DatabaseBackend,
    table: &str,
    expr: &str,
    strategy: &CheckViolationStrategy,
) -> Result<Vec<BuiltQuery>, QueryError> {
    let quoted_table = quote_ident(table, backend);

    let sql = match strategy {
        CheckViolationStrategy::NullifyViolatingColumn { column } => {
            let quoted_col = quote_ident(column.as_str(), backend);
            format!("UPDATE {quoted_table} SET {quoted_col} = NULL WHERE NOT ({expr})")
        }
        CheckViolationStrategy::DeleteViolatingRows => {
            format!("DELETE FROM {quoted_table} WHERE NOT ({expr})")
        }
        // `#[non_exhaustive]` future-variant guard; unreachable today.
        #[cfg(not(tarpaulin_include))]
        _ => {
            return Err(QueryError::UnsupportedAction(format!(
                "AddConstraint(Check) on '{table}': unsupported strategy variant"
            )));
        }
    };

    Ok(vec![BuiltQuery::Raw(RawSql::uniform(sql))])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_check_violation_cleanup_nullify_emits_update_set_null() {
        // NullifyViolatingColumn body: format!("UPDATE ... SET <col> = NULL
        // WHERE NOT (<expr>)"). Hits the previously-uncovered Nullify arm.
        let queries = build_check_violation_cleanup(
            DatabaseBackend::Postgres,
            "orders",
            "qty > 0",
            &CheckViolationStrategy::NullifyViolatingColumn {
                column: "qty".into(),
            },
        )
        .expect("nullify arm should succeed");
        assert_eq!(queries.len(), 1);
        let sql = queries[0].build(DatabaseBackend::Postgres);
        assert!(sql.contains("UPDATE \"orders\""));
        assert!(sql.contains("SET \"qty\" = NULL"));
        assert!(sql.contains("WHERE NOT (qty > 0)"));
    }
}
