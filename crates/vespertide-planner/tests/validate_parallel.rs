#![expect(
    unsafe_code,
    reason = "serial_test serializes Rust 2024 std::env var setters used to force deterministic plan-validation thresholds"
)]

use std::ffi::OsString;

use rayon::ThreadPoolBuilder;
use serial_test::serial;
use vespertide_core::{ColumnDef, ColumnType, MigrationAction, MigrationPlan, SimpleColumnType};
use vespertide_planner::validate_migration_plan;

const TEST_PAR_THRESHOLD: &str = "8";
const VALIDATE_PLAN_THRESHOLD_ENV: &str = "VESPERTIDE_VALIDATE_PLAN_PAR_THRESHOLD";

#[test]
#[serial]
fn validate_migration_plan_error_order_matches_across_thread_counts() {
    let _threshold = EnvVarGuard::set(VALIDATE_PLAN_THRESHOLD_ENV, TEST_PAR_THRESHOLD);

    for action_count in [1_usize, 49, 50, 200] {
        let plan = plan_with_actions(action_count);
        let one_thread = validate_error_with_thread_count(&plan, 1);
        let four_threads = validate_error_with_thread_count(&plan, 4);

        assert_eq!(
            one_thread, four_threads,
            "validator should report the same first error for {action_count} actions"
        );
    }
}

#[test]
#[serial]
fn env_guard_restores_previous_validate_plan_threshold() {
    // SAFETY: this test is `#[serial]`, so the environment mutation cannot race
    // with another test in this binary.
    unsafe { std::env::set_var(VALIDATE_PLAN_THRESHOLD_ENV, "123") };
    {
        let _threshold = EnvVarGuard::set(VALIDATE_PLAN_THRESHOLD_ENV, TEST_PAR_THRESHOLD);
        assert_eq!(
            std::env::var(VALIDATE_PLAN_THRESHOLD_ENV).as_deref(),
            Ok(TEST_PAR_THRESHOLD)
        );
    }
    assert_eq!(
        std::env::var(VALIDATE_PLAN_THRESHOLD_ENV).as_deref(),
        Ok("123")
    );
    // SAFETY: this test is `#[serial]`, so the environment mutation cannot race
    // with another test in this binary.
    unsafe { std::env::remove_var(VALIDATE_PLAN_THRESHOLD_ENV) };
}

fn plan_with_actions(action_count: usize) -> MigrationPlan {
    MigrationPlan {
        id: String::new(),
        comment: None,
        created_at: None,
        version: 1,
        actions: (0..action_count).map(action_for_index).collect(),
    }
}

fn validate_error_with_thread_count(plan: &MigrationPlan, thread_count: usize) -> String {
    ThreadPoolBuilder::new()
        .num_threads(thread_count)
        .build()
        .expect("rayon thread pool should build")
        .install(|| validate_error_string(plan))
}

fn validate_error_string(plan: &MigrationPlan) -> String {
    validate_migration_plan(plan)
        .expect_err("plan should contain at least one invalid action")
        .to_string()
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

fn action_for_index(index: usize) -> MigrationAction {
    let is_invalid = index == 0 || index % 10 == 7;
    MigrationAction::AddColumn {
        table: format!("table_{index}").into(),
        column: Box::new(ColumnDef {
            name: format!("column_{index}").into(),
            r#type: ColumnType::Simple(SimpleColumnType::Text),
            nullable: !is_invalid,
            default: None,
            comment: None,
            primary_key: None,
            unique: None,
            index: None,
            foreign_key: None,
        }),
        fill_with: None,
    }
}
