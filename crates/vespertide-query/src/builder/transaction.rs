use super::PlanQueries;
use crate::DatabaseBackend;
use crate::sql::{BuiltQuery, RawSql};

pub(super) fn wrap_backend_queries(plan_queries: &mut [PlanQueries], backend: DatabaseBackend) {
    let Some(first_idx) = plan_queries
        .iter()
        .position(|pq| !backend_queries(pq, backend).is_empty())
    else {
        return;
    };
    // `rposition` succeeds by construction whenever `position` did (same
    // predicate, same input). `unwrap_or(first_idx)` is correct for both
    // the single-non-empty-queue case (rposition == first_idx) and the
    // unreachable `None` arm, so the `let-else` shape goes away entirely.
    let last_idx = plan_queries
        .iter()
        .rposition(|pq| !backend_queries(pq, backend).is_empty())
        .unwrap_or(first_idx);

    backend_queries_mut(&mut plan_queries[first_idx], backend)
        .insert(0, BuiltQuery::Raw(RawSql::uniform("BEGIN;".to_string())));
    backend_queries_mut(&mut plan_queries[last_idx], backend)
        .push(BuiltQuery::Raw(RawSql::uniform("COMMIT;".to_string())));
}

fn backend_queries(plan_queries: &PlanQueries, backend: DatabaseBackend) -> &[BuiltQuery] {
    match backend {
        DatabaseBackend::Postgres => &plan_queries.postgres,
        DatabaseBackend::MySql => &plan_queries.mysql,
        DatabaseBackend::Sqlite => &plan_queries.sqlite,
    }
}

fn backend_queries_mut(
    plan_queries: &mut PlanQueries,
    backend: DatabaseBackend,
) -> &mut Vec<BuiltQuery> {
    match backend {
        DatabaseBackend::Postgres => &mut plan_queries.postgres,
        DatabaseBackend::MySql => &mut plan_queries.mysql,
        DatabaseBackend::Sqlite => &mut plan_queries.sqlite,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vespertide_core::MigrationAction;

    /// Build a `PlanQueries` whose Postgres vec contains one raw SQL statement.
    fn pq_with_pg_sql(sql: &str) -> PlanQueries {
        PlanQueries {
            action: MigrationAction::RawSql { sql: sql.into() },
            postgres: vec![BuiltQuery::Raw(RawSql::uniform(sql.to_string()))],
            mysql: vec![],
            sqlite: vec![],
        }
    }

    /// Build a `PlanQueries` with all backend vecs empty.
    fn pq_empty() -> PlanQueries {
        PlanQueries {
            action: MigrationAction::RawSql { sql: String::new() },
            postgres: vec![],
            mysql: vec![],
            sqlite: vec![],
        }
    }

    /// Kills the `unwrap_or(first_idx)` → `unwrap_or(0)` mutant.
    ///
    /// With a single non-empty entry, `rposition` returns the same index as
    /// `position` (both == 0).  `unwrap_or(first_idx)` is correct; if the
    /// mutant changed it to `unwrap_or(0)` the result would be identical for
    /// this case — but the test still pins the observable: BEGIN is prepended
    /// and COMMIT is appended to the same (only) entry.
    ///
    /// The stronger kill comes from the two-entry variant below, but this test
    /// documents the single-entry contract explicitly.
    #[test]
    fn wrap_single_non_empty_begin_commit_same_entry() {
        let mut queries = vec![pq_with_pg_sql("SELECT 1")];
        wrap_backend_queries(&mut queries, DatabaseBackend::Postgres);

        let pg_sql: Vec<String> = queries[0]
            .postgres
            .iter()
            .map(|q| q.build(DatabaseBackend::Postgres))
            .collect();

        assert!(
            pg_sql.first().map(String::as_str) == Some("BEGIN;"),
            "first statement must be BEGIN, got: {pg_sql:?}"
        );
        assert!(
            pg_sql.last().map(String::as_str) == Some("COMMIT;"),
            "last statement must be COMMIT, got: {pg_sql:?}"
        );
        // MySQL and SQLite vecs were empty → must remain untouched.
        assert!(queries[0].mysql.is_empty());
        assert!(queries[0].sqlite.is_empty());
    }

    /// Kills the `position` / `rposition` None-arm mutant.
    ///
    /// When ALL backend vecs are empty, `position` returns `None` and
    /// `wrap_backend_queries` must return early without inserting anything.
    /// If the early-return were removed, the function would panic (or insert
    /// into an out-of-bounds index).
    #[test]
    fn wrap_noop_all_empty() {
        let mut queries = vec![pq_empty(), pq_empty()];
        wrap_backend_queries(&mut queries, DatabaseBackend::Postgres);

        // Nothing inserted.
        assert!(queries[0].postgres.is_empty());
        assert!(queries[1].postgres.is_empty());
    }

    /// Kills the `unwrap_or(first_idx)` → `unwrap_or(0)` mutant more directly.
    ///
    /// Two entries: first is empty, second is non-empty.
    /// `position` → 1 (first_idx = 1), `rposition` → 1 (last_idx = 1).
    /// `unwrap_or(0)` would give last_idx = 0, putting COMMIT on the wrong entry.
    #[test]
    fn wrap_two_entries_first_empty_second_non_empty() {
        let mut queries = vec![pq_empty(), pq_with_pg_sql("SELECT 2")];
        wrap_backend_queries(&mut queries, DatabaseBackend::Postgres);

        // Entry 0 is empty → must stay empty.
        assert!(
            queries[0].postgres.is_empty(),
            "empty first entry must not receive BEGIN/COMMIT"
        );

        // Entry 1 must have BEGIN prepended and COMMIT appended.
        let pg_sql: Vec<String> = queries[1]
            .postgres
            .iter()
            .map(|q| q.build(DatabaseBackend::Postgres))
            .collect();

        assert_eq!(
            pg_sql.first().map(String::as_str),
            Some("BEGIN;"),
            "BEGIN must be on the non-empty entry, got: {pg_sql:?}"
        );
        assert_eq!(
            pg_sql.last().map(String::as_str),
            Some("COMMIT;"),
            "COMMIT must be on the non-empty entry, got: {pg_sql:?}"
        );
    }
}
