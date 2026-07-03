#![expect(
    unsafe_code,
    reason = "serial_test serializes Rust 2024 std::env var setters used to force deterministic diff parallelism thresholds"
)]

use std::ffi::OsString;

use proptest::prelude::*;
use rayon::ThreadPoolBuilder;
use serial_test::serial;
use vespertide_core::{
    ColumnDef, ColumnType, MigrationPlan, SimpleColumnType, TableConstraint, TableDef,
};
use vespertide_planner::diff_schemas;

const DIFF_THRESHOLD_ENV: &str = "VESPERTIDE_DIFF_PAR_THRESHOLD";
const TABLE_COUNT: usize = 50;
const TEST_PAR_THRESHOLD: &str = "8";

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    #[serial]
    fn diff_schemas_byte_identical_across_thread_counts(
        (from, to) in arb_schema_pair_acyclic_with_50_tables()
    ) {
        let _threshold = EnvVarGuard::set(DIFF_THRESHOLD_ENV, TEST_PAR_THRESHOLD);

        let one_thread = run_diff_with_thread_count(&from, &to, 1);
        let four_threads = run_diff_with_thread_count(&from, &to, 4);

        prop_assert_eq!(one_thread, four_threads);
    }
}

#[test]
#[serial]
fn env_guard_restores_previous_diff_threshold() {
    // SAFETY: this test is `#[serial]`, so the environment mutation cannot race
    // with another test in this binary.
    unsafe { std::env::set_var(DIFF_THRESHOLD_ENV, "123") };
    {
        let _threshold = EnvVarGuard::set(DIFF_THRESHOLD_ENV, TEST_PAR_THRESHOLD);
        assert_eq!(
            std::env::var(DIFF_THRESHOLD_ENV).as_deref(),
            Ok(TEST_PAR_THRESHOLD)
        );
    }
    assert_eq!(std::env::var(DIFF_THRESHOLD_ENV).as_deref(), Ok("123"));
    // SAFETY: this test is `#[serial]`, so the environment mutation cannot race
    // with another test in this binary.
    unsafe { std::env::remove_var(DIFF_THRESHOLD_ENV) };
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

fn run_diff_with_thread_count(
    from: &[TableDef],
    to: &[TableDef],
    thread_count: usize,
) -> MigrationPlan {
    ThreadPoolBuilder::new()
        .num_threads(thread_count)
        .build()
        .expect("rayon thread pool should build")
        .install(|| diff_schemas(from, to).expect("acyclic schema pair should diff"))
}

fn arb_schema_pair_acyclic_with_50_tables() -> impl Strategy<Value = (Vec<TableDef>, Vec<TableDef>)>
{
    prop::collection::vec(0_u8..8, TABLE_COUNT).prop_map(|mutations| {
        let from: Vec<TableDef> = (0..TABLE_COUNT).map(table_for_index).collect();
        let mut to = from.clone();

        for (index, mutation) in mutations.into_iter().enumerate() {
            mutate_table(&mut to[index], index, mutation);
        }

        (from, to)
    })
}

fn table_for_index(index: usize) -> TableDef {
    let table_name = format!("table_{index:02}");
    TableDef {
        name: table_name.clone().into(),
        description: None,
        columns: vec![
            column("id", SimpleColumnType::Integer, false),
            column("code", SimpleColumnType::Text, false),
            column("label", SimpleColumnType::Text, true),
        ],
        constraints: vec![
            TableConstraint::PrimaryKey {
                auto_increment: false,
                columns: vec!["id".into()],
                strategy: vespertide_core::PrimaryKeyAdditionStrategy::default(),
            },
            TableConstraint::Index {
                name: Some(format!("ix_{table_name}__code")),
                columns: vec!["code".into()],
            },
            TableConstraint::Check {
                name: format!("check_{table_name}_label"),
                expr: "label IS NULL OR label <> ''".to_string(),
                strategy: vespertide_core::CheckViolationStrategy::default(),
            },
        ],
    }
}

fn column(name: &str, column_type: SimpleColumnType, nullable: bool) -> ColumnDef {
    ColumnDef::new(name, ColumnType::Simple(column_type), nullable)
}

fn mutate_table(table: &mut TableDef, index: usize, mutation: u8) {
    match mutation {
        0 => {}
        1 => table.columns[1].nullable = true,
        2 => table.columns[1].default = Some("'updated'".into()),
        3 => table.columns.push(column(
            &format!("extra_{index:02}"),
            SimpleColumnType::Text,
            true,
        )),
        4 => table.constraints.push(TableConstraint::Unique {
            name: Some(format!("uq_{}__code", table.name)),
            columns: vec!["code".into()],
            strategy: vespertide_core::UniqueConstraintStrategy::DeleteDuplicates {
                keep: vespertide_core::KeepPolicy::First,
            },
        }),
        5 => {
            if let Some(TableConstraint::Check { expr, .. }) = table.constraints.get_mut(2) {
                *expr = "label IS NULL OR length(label) > 1".to_string();
            }
        }
        6 => {
            table.constraints.retain(|constraint| {
                !matches!(
                    constraint,
                    TableConstraint::Index { columns, .. } if columns.len() == 1 && columns[0] == "code"
                )
            });
        }
        _ => table.constraints.push(TableConstraint::Check {
            name: format!("check_{}_code", table.name),
            expr: "code <> ''".to_string(),
            strategy: vespertide_core::CheckViolationStrategy::default(),
        }),
    }
}
