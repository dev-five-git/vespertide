#![expect(
    unsafe_code,
    reason = "serial_test serializes Rust 2024 std::env var setters used to force deterministic parallel query thresholds"
)]

use std::ffi::OsString;

use rayon::ThreadPoolBuilder;
use serial_test::serial;
use vespertide_core::{ColumnDef, ColumnType, MigrationAction, MigrationPlan, SimpleColumnType};
use vespertide_planner::apply_action;
use vespertide_query::builder::PlanQueries;
use vespertide_query::sql::build_action_queries_with_pending;
use vespertide_query::{DatabaseBackend, build_plan_queries};

const PLAN_QUERY_THRESHOLD_ENV: &str = "VESPERTIDE_PLAN_QUERY_PAR_THRESHOLD";
const TEST_PAR_THRESHOLD: &str = "8";

fn col(name: &str, ty: ColumnType) -> ColumnDef {
    ColumnDef::new(name, ty, false)
}

fn create_table_action(i: usize) -> MigrationAction {
    MigrationAction::CreateTable {
        table: format!("parallel_table_{i}").into(),
        columns: vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
        constraints: vec![],
    }
}

fn plan_with_actions(count: usize) -> MigrationPlan {
    MigrationPlan {
        id: format!("parallel-build-{count}"),
        comment: None,
        created_at: None,
        version: 1,
        actions: (0..count).map(create_table_action).collect(),
    }
}

fn build_plan_queries_sequentially(plan: &MigrationPlan) -> Vec<PlanQueries> {
    let mut evolving_schema = Vec::new();
    let mut queries = Vec::with_capacity(plan.actions.len());

    for action in &plan.actions {
        let postgres = build_action_queries_with_pending(
            DatabaseBackend::Postgres,
            action,
            &evolving_schema,
            &[],
        )
        .unwrap();
        let mysql = build_action_queries_with_pending(
            DatabaseBackend::MySql,
            action,
            &evolving_schema,
            &[],
        )
        .unwrap();
        let sqlite = build_action_queries_with_pending(
            DatabaseBackend::Sqlite,
            action,
            &evolving_schema,
            &[],
        )
        .unwrap();

        queries.push(PlanQueries {
            action: action.clone(),
            postgres,
            mysql,
            sqlite,
        });

        let _ = apply_action(&mut evolving_schema, action);
    }

    queries
}

fn backend_sql(plan_queries: &[PlanQueries], backend: DatabaseBackend) -> Vec<Vec<String>> {
    plan_queries
        .iter()
        .map(|plan_query| {
            let queries = match backend {
                DatabaseBackend::Postgres => &plan_query.postgres,
                DatabaseBackend::MySql => &plan_query.mysql,
                DatabaseBackend::Sqlite => &plan_query.sqlite,
            };
            queries.iter().map(|query| query.build(backend)).collect()
        })
        .collect()
}

fn assert_byte_identical_to_sequential(plan: &MigrationPlan) {
    let expected = build_plan_queries_sequentially(plan);
    let actual = build_plan_queries(plan, &[]).unwrap();

    assert_plan_queries_match(&actual, &expected);
}

fn assert_plan_queries_match(actual: &[PlanQueries], expected: &[PlanQueries]) {
    assert_eq!(actual.len(), expected.len());
    for (actual_query, expected_query) in actual.iter().zip(expected) {
        assert_eq!(&actual_query.action, &expected_query.action);
    }

    for backend in [
        DatabaseBackend::Postgres,
        DatabaseBackend::MySql,
        DatabaseBackend::Sqlite,
    ] {
        assert_eq!(backend_sql(actual, backend), backend_sql(expected, backend));
    }
}

// Every test in this binary is `#[serial]` because `parallel_build_above_threshold_*`
// mutates the process-global environment (`VESPERTIDE_PLAN_QUERY_PAR_THRESHOLD`)
// via `unsafe { std::env::set_var }`. `serial_test` only serializes opt-in tests,
// so any non-serial test in this binary could race with the env mutation and
// trigger Rust 2024 undefined behavior. Marking every test serial preserves the
// "exclusive test execution within this binary during env mutation" invariant.
#[test]
#[serial]
fn sequential_build_below_parallel_threshold_matches_reference() {
    let plan = plan_with_actions(49);

    assert_byte_identical_to_sequential(&plan);
}

#[test]
#[serial]
fn parallel_build_above_threshold_preserves_order_for_all_backends() {
    let _threshold = EnvVarGuard::set(PLAN_QUERY_THRESHOLD_ENV, TEST_PAR_THRESHOLD);
    let plan = plan_with_actions(100);
    let expected = build_plan_queries_sequentially(&plan);
    let single_thread = build_plan_queries_with_thread_count(&plan, 1);
    let four_threads = build_plan_queries_with_thread_count(&plan, 4);

    assert_plan_queries_match(&single_thread, &expected);
    assert_plan_queries_match(&four_threads, &expected);
    assert_plan_queries_match(&four_threads, &single_thread);
}

fn build_plan_queries_with_thread_count(
    plan: &MigrationPlan,
    thread_count: usize,
) -> Vec<PlanQueries> {
    ThreadPoolBuilder::new()
        .num_threads(thread_count)
        .build()
        .expect("rayon thread pool should build")
        .install(|| build_plan_queries(plan, &[]).unwrap())
}

struct EnvVarGuard {
    key: &'static str,
    previous: Option<OsString>,
}

impl EnvVarGuard {
    fn set(key: &'static str, value: &'static str) -> Self {
        let previous = std::env::var_os(key);
        // SAFETY: every test in this binary carries `#[serial]`, guaranteeing
        // that no other test (env-mutating OR env-reading) runs concurrently
        // in this process while the override is installed. This satisfies
        // Rust 2024's exclusive-access precondition for `std::env::set_var`.
        unsafe { std::env::set_var(key, value) };
        Self { key, previous }
    }
}

impl Drop for EnvVarGuard {
    fn drop(&mut self) {
        if let Some(previous) = &self.previous {
            // SAFETY: every test in this binary carries `#[serial]`, so the
            // restore call cannot race with another thread reading or
            // mutating the environment in this process.
            unsafe { std::env::set_var(self.key, previous) };
        } else {
            // SAFETY: every test in this binary carries `#[serial]`, so the
            // remove call cannot race with another thread reading or
            // mutating the environment in this process.
            unsafe { std::env::remove_var(self.key) };
        }
    }
}
