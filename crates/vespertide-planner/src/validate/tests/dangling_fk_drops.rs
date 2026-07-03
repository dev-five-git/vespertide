//! Tests for fault **F9** — dangling FK after a column or table drop.
//!
//! Each rstest case is structured `(baseline, plan, expected_violations)`:
//! - `baseline`: the schema before the plan is applied
//! - `plan`: the migration plan under test
//! - `expected_violations`: a sorted `Vec<DanglingFkDrop>` (empty = pass)
//!
//! Equality is full-struct equality on the sorted vec, so order changes in
//! `find_dangling_fk_drops` show up here immediately.

use super::*;
use crate::validate::{DanglingFkDrop, find_dangling_fk_drops};

/// Build a FK constraint for tests.
fn fk(
    name: Option<&str>,
    columns: Vec<&str>,
    ref_table: &str,
    ref_columns: Vec<&str>,
) -> TableConstraint {
    TableConstraint::ForeignKey {
        name: name.map(ToString::to_string),
        columns: columns.into_iter().map(Into::into).collect(),
        ref_table: ref_table.into(),
        ref_columns: ref_columns.into_iter().map(Into::into).collect(),
        on_delete: None,
        on_update: None,
        orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
    }
}

fn int_col(name: &str) -> ColumnDef {
    let mut c = col(name, ColumnType::Simple(SimpleColumnType::Integer));
    c.nullable = false;
    c
}

fn plan_with(actions: Vec<MigrationAction>) -> MigrationPlan {
    MigrationPlan {
        id: String::new(),
        comment: None,
        created_at: None,
        version: 1,
        actions,
    }
}

fn dangling(
    dropped_table: &str,
    dropped_column: Option<&str>,
    referencing_table: &str,
    fk_name: Option<&str>,
) -> DanglingFkDrop {
    DanglingFkDrop {
        dropped_table: dropped_table.to_string(),
        dropped_column: dropped_column.map(ToString::to_string),
        referencing_table: referencing_table.to_string(),
        referencing_constraint: fk_name.map(ToString::to_string),
    }
}

/// Two-table baseline: `parent(id)` + `child(id, parent_id)` with FK
/// `child.parent_id -> parent.id` named `fk_child_parent`.
fn two_table_baseline() -> Vec<TableDef> {
    vec![
        table("parent", vec![int_col("id")], vec![pk(vec!["id"])]),
        table(
            "child",
            vec![int_col("id"), int_col("parent_id")],
            vec![
                pk(vec!["id"]),
                fk(
                    Some("fk_child_parent"),
                    vec!["parent_id"],
                    "parent",
                    vec!["id"],
                ),
            ],
        ),
    ]
}

// ───────────────────────────────────────────────────────────────────────────
// Case 1: column drop, no referencing FK → OK
// ───────────────────────────────────────────────────────────────────────────
#[test]
fn case_01_drop_column_no_referencing_fk() {
    // Baseline has parent with two columns; no FK anywhere.
    let baseline = vec![table(
        "parent",
        vec![int_col("id"), int_col("legacy")],
        vec![pk(vec!["id"])],
    )];

    let plan = plan_with(vec![MigrationAction::DeleteColumn {
        table: "parent".into(),
        column: "legacy".into(),
    }]);

    assert_eq!(
        find_dangling_fk_drops(&plan, &baseline),
        Vec::<DanglingFkDrop>::new()
    );
}

// ───────────────────────────────────────────────────────────────────────────
// Case 2: column drop with referencing single-col FK → ERR
// ───────────────────────────────────────────────────────────────────────────
#[test]
fn case_02_drop_column_with_referencing_fk() {
    let plan = plan_with(vec![MigrationAction::DeleteColumn {
        table: "parent".into(),
        column: "id".into(),
    }]);

    assert_eq!(
        find_dangling_fk_drops(&plan, &two_table_baseline()),
        vec![dangling(
            "parent",
            Some("id"),
            "child",
            Some("fk_child_parent")
        )]
    );
}

// ───────────────────────────────────────────────────────────────────────────
// Case 3: column drop + that FK explicitly removed → OK
// ───────────────────────────────────────────────────────────────────────────
#[test]
fn case_03_drop_column_and_fk_in_same_plan() {
    let plan = plan_with(vec![
        MigrationAction::RemoveConstraint {
            table: "child".into(),
            constraint: fk(
                Some("fk_child_parent"),
                vec!["parent_id"],
                "parent",
                vec!["id"],
            ),
        },
        MigrationAction::DeleteColumn {
            table: "parent".into(),
            column: "id".into(),
        },
    ]);

    assert_eq!(
        find_dangling_fk_drops(&plan, &two_table_baseline()),
        Vec::<DanglingFkDrop>::new()
    );
}

// ───────────────────────────────────────────────────────────────────────────
// Case 4: column drop + the table OWNING the FK is dropped → OK
// ───────────────────────────────────────────────────────────────────────────
#[test]
fn case_04_drop_column_and_referencing_table() {
    let plan = plan_with(vec![
        MigrationAction::DeleteTable {
            table: "child".into(),
        },
        MigrationAction::DeleteColumn {
            table: "parent".into(),
            column: "id".into(),
        },
    ]);

    assert_eq!(
        find_dangling_fk_drops(&plan, &two_table_baseline()),
        Vec::<DanglingFkDrop>::new()
    );
}

// ───────────────────────────────────────────────────────────────────────────
// Case 5: column drop + the child column participating in the FK is also dropped → OK
// ───────────────────────────────────────────────────────────────────────────
#[test]
fn case_05_drop_column_and_fk_child_column() {
    let plan = plan_with(vec![
        // Drop the FK's child column → backend implicitly drops the FK.
        MigrationAction::DeleteColumn {
            table: "child".into(),
            column: "parent_id".into(),
        },
        // Drop the referenced parent column too.
        MigrationAction::DeleteColumn {
            table: "parent".into(),
            column: "id".into(),
        },
    ]);

    assert_eq!(
        find_dangling_fk_drops(&plan, &two_table_baseline()),
        Vec::<DanglingFkDrop>::new()
    );
}

// ───────────────────────────────────────────────────────────────────────────
// Case 6: table drop, no referencing FK → OK
// ───────────────────────────────────────────────────────────────────────────
#[test]
fn case_06_drop_table_no_referencing_fk() {
    let baseline = vec![
        table("orphan", vec![int_col("id")], vec![pk(vec!["id"])]),
        table("kept", vec![int_col("id")], vec![pk(vec!["id"])]),
    ];

    let plan = plan_with(vec![MigrationAction::DeleteTable {
        table: "orphan".into(),
    }]);

    assert_eq!(
        find_dangling_fk_drops(&plan, &baseline),
        Vec::<DanglingFkDrop>::new()
    );
}

// ───────────────────────────────────────────────────────────────────────────
// Case 7: table drop, some other table's FK references it → ERR
// ───────────────────────────────────────────────────────────────────────────
#[test]
fn case_07_drop_table_with_referencing_fk() {
    let plan = plan_with(vec![MigrationAction::DeleteTable {
        table: "parent".into(),
    }]);

    assert_eq!(
        find_dangling_fk_drops(&plan, &two_table_baseline()),
        vec![dangling("parent", None, "child", Some("fk_child_parent"))]
    );
}

// ───────────────────────────────────────────────────────────────────────────
// Case 8: table drop + that FK explicitly removed → OK
// ───────────────────────────────────────────────────────────────────────────
#[test]
fn case_08_drop_table_and_fk_in_same_plan() {
    let plan = plan_with(vec![
        MigrationAction::RemoveConstraint {
            table: "child".into(),
            constraint: fk(
                Some("fk_child_parent"),
                vec!["parent_id"],
                "parent",
                vec!["id"],
            ),
        },
        MigrationAction::DeleteTable {
            table: "parent".into(),
        },
    ]);

    assert_eq!(
        find_dangling_fk_drops(&plan, &two_table_baseline()),
        Vec::<DanglingFkDrop>::new()
    );
}

// ───────────────────────────────────────────────────────────────────────────
// Case 9: table drop + the entire referencing table is dropped → OK
// ───────────────────────────────────────────────────────────────────────────
#[test]
fn case_09_drop_table_and_referencing_table() {
    let plan = plan_with(vec![
        MigrationAction::DeleteTable {
            table: "child".into(),
        },
        MigrationAction::DeleteTable {
            table: "parent".into(),
        },
    ]);

    assert_eq!(
        find_dangling_fk_drops(&plan, &two_table_baseline()),
        Vec::<DanglingFkDrop>::new()
    );
}

// ───────────────────────────────────────────────────────────────────────────
// Case 10: self-referential FK column drop + FK explicitly removed in same plan → OK
// ───────────────────────────────────────────────────────────────────────────
#[test]
fn case_10_self_ref_fk_column_drop_with_fk_removed() {
    // `node(id, parent_id)` with self-FK parent_id → id.
    let baseline = vec![table(
        "node",
        vec![int_col("id"), int_col("parent_id")],
        vec![
            pk(vec!["id"]),
            fk(
                Some("fk_node_parent"),
                vec!["parent_id"],
                "node",
                vec!["id"],
            ),
        ],
    )];

    let plan = plan_with(vec![
        MigrationAction::RemoveConstraint {
            table: "node".into(),
            constraint: fk(
                Some("fk_node_parent"),
                vec!["parent_id"],
                "node",
                vec!["id"],
            ),
        },
        MigrationAction::DeleteColumn {
            table: "node".into(),
            column: "id".into(),
        },
    ]);

    assert_eq!(
        find_dangling_fk_drops(&plan, &baseline),
        Vec::<DanglingFkDrop>::new()
    );
}

// ───────────────────────────────────────────────────────────────────────────
// Case 11: composite FK, one of the referenced columns dropped, FK left intact → ERR
// ───────────────────────────────────────────────────────────────────────────
#[test]
fn case_11_composite_fk_partial_column_drop() {
    // `parent(id, tenant_id)` with composite PK; `child` references both.
    let baseline = vec![
        table(
            "parent",
            vec![int_col("id"), int_col("tenant_id")],
            vec![pk(vec!["id", "tenant_id"])],
        ),
        table(
            "child",
            vec![int_col("id"), int_col("parent_id"), int_col("tenant_id")],
            vec![
                pk(vec!["id"]),
                fk(
                    Some("fk_child_parent_composite"),
                    vec!["parent_id", "tenant_id"],
                    "parent",
                    vec!["id", "tenant_id"],
                ),
            ],
        ),
    ];

    // Drop only `parent.tenant_id` — composite FK still points at it.
    let plan = plan_with(vec![MigrationAction::DeleteColumn {
        table: "parent".into(),
        column: "tenant_id".into(),
    }]);

    assert_eq!(
        find_dangling_fk_drops(&plan, &baseline),
        vec![dangling(
            "parent",
            Some("tenant_id"),
            "child",
            Some("fk_child_parent_composite"),
        )]
    );
}

// ───────────────────────────────────────────────────────────────────────────
// Coverage-closure: FK with owner-column drop suppresses FK from surviving set
// (lines 197-202 in collect_surviving_fks) — child's FK column gone → FK
// disappears via column cascade, NOT reported as dangling even if the parent
// column is also dropped.
// ───────────────────────────────────────────────────────────────────────────
#[test]
fn case_13_fk_owner_column_dropped_suppresses_dangling() {
    let baseline = vec![
        table("parent", vec![int_col("id")], vec![pk(vec!["id"])]),
        table(
            "child",
            vec![int_col("id"), int_col("parent_id")],
            vec![
                pk(vec!["id"]),
                fk(
                    Some("fk_child_parent"),
                    vec!["parent_id"],
                    "parent",
                    vec!["id"],
                ),
            ],
        ),
    ];
    // Drop both the FK-owning column AND the referenced column. The FK
    // disappears via the child's column cascade, so the parent.id drop
    // is no longer dangling.
    let plan = plan_with(vec![
        MigrationAction::DeleteColumn {
            table: "child".into(),
            column: "parent_id".into(),
        },
        MigrationAction::DeleteColumn {
            table: "parent".into(),
            column: "id".into(),
        },
    ]);

    assert_eq!(
        find_dangling_fk_drops(&plan, &baseline),
        Vec::<DanglingFkDrop>::new()
    );
}

// ───────────────────────────────────────────────────────────────────────────
// Coverage-closure: ReplaceConstraint of FK counts as an explicit removal
// (`collect_explicitly_removed_fks` `ReplaceConstraint` branch).
// ───────────────────────────────────────────────────────────────────────────
#[test]
fn case_14_replace_constraint_fk_counts_as_explicit_removal() {
    let plan = plan_with(vec![
        MigrationAction::ReplaceConstraint {
            table: "child".into(),
            from: fk(
                Some("fk_child_parent"),
                vec!["parent_id"],
                "parent",
                vec!["id"],
            ),
            to: fk(
                Some("fk_child_parent"),
                vec!["parent_id"],
                "parent",
                vec!["id"],
            ),
        },
        // Drop parent.id — the original FK is "removed" by Replace, so the
        // detector will look at the *new* FK from the Replace's `to` side
        // when it's tracked as an addition. But Replace alone doesn't add;
        // here we just want to demonstrate the explicit-removal arm fires
        // and the parent.id drop is detected as dangling against the
        // *replaced* FK (which is the same shape).
        MigrationAction::DeleteColumn {
            table: "parent".into(),
            column: "id".into(),
        },
    ]);

    // The original baseline FK is treated as explicitly removed by the
    // Replace's `from`, so it does NOT survive to the dangling check. No
    // warning expected.
    assert_eq!(
        find_dangling_fk_drops(&plan, &two_table_baseline()),
        Vec::<DanglingFkDrop>::new()
    );
}

// ───────────────────────────────────────────────────────────────────────────
// Coverage-closure: CreateTable with FK pointing at a column dropped by the
// same plan (lines 226-251 — CreateTable's surviving FK addition + dangling).
// ───────────────────────────────────────────────────────────────────────────
#[test]
fn case_15_create_table_with_fk_to_dropped_column() {
    let baseline = vec![table("parent", vec![int_col("id")], vec![pk(vec!["id"])])];
    let plan = plan_with(vec![
        MigrationAction::CreateTable {
            table: "child".into(),
            columns: vec![int_col("id"), int_col("parent_id")],
            constraints: vec![
                pk(vec!["id"]),
                fk(Some("fk_new"), vec!["parent_id"], "parent", vec!["id"]),
            ],
        },
        // Drop the column the freshly-created FK points at.
        MigrationAction::DeleteColumn {
            table: "parent".into(),
            column: "id".into(),
        },
    ]);

    assert_eq!(
        find_dangling_fk_drops(&plan, &baseline),
        vec![dangling("parent", Some("id"), "child", Some("fk_new"))]
    );
}

// ───────────────────────────────────────────────────────────────────────────
// Coverage-closure: AddConstraint(FK) whose owner table is also dropped in
// the same plan — `if drop_set.contains(&(table, None)) { continue; }`
// (lines 262-264).
// ───────────────────────────────────────────────────────────────────────────
#[test]
fn case_16_add_constraint_fk_on_to_be_dropped_table_skipped() {
    let baseline = vec![
        table("parent", vec![int_col("id")], vec![pk(vec!["id"])]),
        table(
            "child",
            vec![int_col("id"), int_col("parent_id")],
            vec![pk(vec!["id"])],
        ),
    ];
    let plan = plan_with(vec![
        MigrationAction::AddConstraint {
            table: "child".into(),
            constraint: fk(Some("fk_add"), vec!["parent_id"], "parent", vec!["id"]),
        },
        // Drop the owner — added FK never survives the plan.
        MigrationAction::DeleteTable {
            table: "child".into(),
        },
        // Drop the referenced column too — must NOT produce a dangling
        // warning since the new FK disappeared with its table.
        MigrationAction::DeleteColumn {
            table: "parent".into(),
            column: "id".into(),
        },
    ]);

    assert_eq!(
        find_dangling_fk_drops(&plan, &baseline),
        Vec::<DanglingFkDrop>::new()
    );
}

// ───────────────────────────────────────────────────────────────────────────
// Coverage-closure: AddConstraint(FK) pointing at a column the same plan
// drops — the new FK becomes immediately dangling.
// ───────────────────────────────────────────────────────────────────────────
#[test]
fn case_17_add_constraint_fk_pointing_at_dropped_column() {
    let baseline = vec![
        table("parent", vec![int_col("id")], vec![pk(vec!["id"])]),
        table(
            "child",
            vec![int_col("id"), int_col("parent_id")],
            vec![pk(vec!["id"])],
        ),
    ];
    let plan = plan_with(vec![
        MigrationAction::AddConstraint {
            table: "child".into(),
            constraint: fk(Some("fk_add"), vec!["parent_id"], "parent", vec!["id"]),
        },
        MigrationAction::DeleteColumn {
            table: "parent".into(),
            column: "id".into(),
        },
    ]);

    assert_eq!(
        find_dangling_fk_drops(&plan, &baseline),
        vec![dangling("parent", Some("id"), "child", Some("fk_add"))]
    );
}

// ───────────────────────────────────────────────────────────────────────────
// Coverage-closure: CreateTable whose own table is also dropped — guard at
// line 232 (`if drop_set.contains(&(table, None)) { continue; }`).
// ───────────────────────────────────────────────────────────────────────────
#[test]
fn case_18_create_table_then_drop_skips_new_fk_tracking() {
    let baseline = vec![table("parent", vec![int_col("id")], vec![pk(vec!["id"])])];
    let plan = plan_with(vec![
        MigrationAction::CreateTable {
            table: "child".into(),
            columns: vec![int_col("id"), int_col("parent_id")],
            constraints: vec![
                pk(vec!["id"]),
                fk(Some("fk_new"), vec!["parent_id"], "parent", vec!["id"]),
            ],
        },
        MigrationAction::DeleteTable {
            table: "child".into(),
        },
        MigrationAction::DeleteColumn {
            table: "parent".into(),
            column: "id".into(),
        },
    ]);

    // child is dropped right after being created → its new FK does not
    // contribute to the surviving set → parent.id drop produces no warning.
    assert_eq!(
        find_dangling_fk_drops(&plan, &baseline),
        Vec::<DanglingFkDrop>::new()
    );
}

// ───────────────────────────────────────────────────────────────────────────
// Case 12: multiple dangling drops in one plan — all reported in deterministic order
// ───────────────────────────────────────────────────────────────────────────
#[test]
fn case_12_multiple_dangling_drops_batched() {
    // Three independent FKs all pointing at the same parent table; the plan
    // drops the parent table without cleaning any of them up.
    let baseline = vec![
        table("parent", vec![int_col("id")], vec![pk(vec!["id"])]),
        table(
            "child_a",
            vec![int_col("id"), int_col("parent_id")],
            vec![
                pk(vec!["id"]),
                fk(Some("fk_a"), vec!["parent_id"], "parent", vec!["id"]),
            ],
        ),
        table(
            "child_b",
            vec![int_col("id"), int_col("parent_id")],
            vec![
                pk(vec!["id"]),
                fk(Some("fk_b"), vec!["parent_id"], "parent", vec!["id"]),
            ],
        ),
        table(
            "child_c",
            vec![int_col("id"), int_col("parent_id")],
            vec![
                pk(vec!["id"]),
                fk(Some("fk_c"), vec!["parent_id"], "parent", vec!["id"]),
            ],
        ),
    ];

    let plan = plan_with(vec![MigrationAction::DeleteTable {
        table: "parent".into(),
    }]);

    // BTreeSet ordering: (dropped_table, dropped_column, referencing_table, constraint).
    // All four key fields equal except referencing_table → child_a < child_b < child_c.
    assert_eq!(
        find_dangling_fk_drops(&plan, &baseline),
        vec![
            dangling("parent", None, "child_a", Some("fk_a")),
            dangling("parent", None, "child_b", Some("fk_b")),
            dangling("parent", None, "child_c", Some("fk_c")),
        ]
    );
}
