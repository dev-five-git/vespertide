#![expect(
    unsafe_code,
    reason = "serial_test serializes Rust 2024 std::env var setters used to force deterministic schema-validation thresholds"
)]

use std::ffi::OsString;

use rayon::ThreadPoolBuilder;
use serial_test::serial;
use vespertide_core::{ColumnDef, ColumnType, SimpleColumnType, TableConstraint, TableDef};
use vespertide_planner::validate_schema;

const TEST_PAR_THRESHOLD: &str = "8";
const VALIDATE_SCHEMA_THRESHOLD_ENV: &str = "VESPERTIDE_VALIDATE_SCHEMA_PAR_THRESHOLD";

#[test]
#[serial]
fn validate_schema_error_order_matches_across_thread_counts() {
    let _threshold = EnvVarGuard::set(VALIDATE_SCHEMA_THRESHOLD_ENV, TEST_PAR_THRESHOLD);

    for table_count in [1_usize, 49, 50, 200] {
        let schema = schema_with_tables(table_count);
        let one_thread = validate_error_with_thread_count(&schema, 1);
        let four_threads = validate_error_with_thread_count(&schema, 4);

        assert_eq!(
            one_thread, four_threads,
            "schema validator should report the same first error for {table_count} tables"
        );
    }
}

#[test]
#[serial]
fn env_guard_restores_previous_validate_schema_threshold() {
    // SAFETY: this test is `#[serial]`, so the environment mutation cannot race
    // with another test in this binary.
    unsafe { std::env::set_var(VALIDATE_SCHEMA_THRESHOLD_ENV, "123") };
    {
        let _threshold = EnvVarGuard::set(VALIDATE_SCHEMA_THRESHOLD_ENV, TEST_PAR_THRESHOLD);
        assert_eq!(
            std::env::var(VALIDATE_SCHEMA_THRESHOLD_ENV).as_deref(),
            Ok(TEST_PAR_THRESHOLD)
        );
    }
    assert_eq!(
        std::env::var(VALIDATE_SCHEMA_THRESHOLD_ENV).as_deref(),
        Ok("123")
    );
    // SAFETY: this test is `#[serial]`, so the environment mutation cannot race
    // with another test in this binary.
    unsafe { std::env::remove_var(VALIDATE_SCHEMA_THRESHOLD_ENV) };
}

fn schema_with_tables(table_count: usize) -> Vec<TableDef> {
    (0..table_count).map(table_for_index).collect()
}

fn validate_error_with_thread_count(schema: &[TableDef], thread_count: usize) -> String {
    ThreadPoolBuilder::new()
        .num_threads(thread_count)
        .build()
        .expect("rayon thread pool should build")
        .install(|| validate_error_string(schema))
}

fn validate_error_string(schema: &[TableDef]) -> String {
    validate_schema(schema)
        .expect_err("schema should contain at least one invalid table")
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

fn table_for_index(index: usize) -> TableDef {
    let id_column = ColumnDef {
        name: "id".into(),
        r#type: ColumnType::Simple(SimpleColumnType::Integer),
        nullable: false,
        default: None,
        comment: None,
        primary_key: None,
        unique: None,
        index: None,
        foreign_key: None,
    };

    TableDef {
        name: format!("table_{index}").into(),
        description: None,
        columns: vec![id_column],
        constraints: constraints_for_index(index),
    }
}

fn constraints_for_index(index: usize) -> Vec<TableConstraint> {
    if index == 0 || index % 10 == 7 {
        Vec::new()
    } else {
        vec![TableConstraint::PrimaryKey {
            auto_increment: false,
            columns: vec!["id".into()],
            strategy: vespertide_core::PrimaryKeyAdditionStrategy::default(),
        }]
    }
}
