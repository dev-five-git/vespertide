use super::*;
use rstest::rstest;
use vespertide_core::{
    ColumnDef, ColumnType, MigrationAction, MigrationPlan, PrimaryKeyAdditionStrategy,
    SimpleColumnType, TableConstraint, TableDef, TableName,
};

fn int_col(name: &str, ty: SimpleColumnType) -> ColumnDef {
    ColumnDef {
        name: name.into(),
        r#type: ColumnType::Simple(ty),
        nullable: false,
        default: None,
        comment: None,
        primary_key: None,
        unique: None,
        index: None,
        foreign_key: None,
    }
}

fn table_with_pk(name: &str, cols: Vec<ColumnDef>, pk_col: &str, auto_increment: bool) -> TableDef {
    TableDef {
        name: name.into(),
        description: None,
        columns: cols,
        constraints: vec![TableConstraint::PrimaryKey {
            auto_increment,
            columns: vec![pk_col.into()],
            strategy: PrimaryKeyAdditionStrategy::default(),
        }],
    }
}

fn add_pk(table: &str, col: &str, auto_increment: bool) -> MigrationAction {
    MigrationAction::AddConstraint {
        table: TableName::from(table),
        constraint: TableConstraint::PrimaryKey {
            auto_increment,
            columns: vec![col.into()],
            strategy: PrimaryKeyAdditionStrategy::default(),
        },
    }
}

fn create_table_inline_pk(
    name: &str,
    cols: Vec<ColumnDef>,
    pk_col: &str,
    auto_increment: bool,
) -> MigrationAction {
    MigrationAction::CreateTable {
        table: name.into(),
        columns: cols,
        constraints: vec![TableConstraint::PrimaryKey {
            auto_increment,
            columns: vec![pk_col.into()],
            strategy: PrimaryKeyAdditionStrategy::default(),
        }],
    }
}

fn plan(actions: Vec<MigrationAction>) -> MigrationPlan {
    MigrationPlan {
        id: "test".into(),
        version: 1,
        comment: None,
        created_at: None,
        actions,
    }
}

#[rstest]
fn case_01_integer_pk_with_auto_increment_medium_warning() {
    let p = plan(vec![create_table_inline_pk(
        "users",
        vec![int_col("id", SimpleColumnType::Integer)],
        "id",
        true,
    )]);
    let ws = find_sequence_exhaustion_risks(&p, &[]);
    assert_eq!(ws.len(), 1);
    assert_eq!(ws[0].risk_level, SequenceRiskLevel::Medium);
    assert_eq!(ws[0].kind, SequenceExhaustionKind::Primary);
    assert_eq!(ws[0].recommended_type, SimpleColumnType::BigInt);
}

#[rstest]
fn case_02_smallint_pk_with_auto_increment_high_warning() {
    let p = plan(vec![create_table_inline_pk(
        "tiny",
        vec![int_col("id", SimpleColumnType::SmallInt)],
        "id",
        true,
    )]);
    let ws = find_sequence_exhaustion_risks(&p, &[]);
    assert_eq!(ws.len(), 1);
    assert_eq!(ws[0].risk_level, SequenceRiskLevel::High);
}

#[rstest]
fn case_03_bigint_pk_safe_no_warning() {
    let p = plan(vec![create_table_inline_pk(
        "users",
        vec![int_col("id", SimpleColumnType::BigInt)],
        "id",
        true,
    )]);
    assert!(find_sequence_exhaustion_risks(&p, &[]).is_empty());
}

// A COMPOSITE (2-column) auto-increment PK must NOT be flagged as a
// single-sequence exhaustion risk: only a lone `serial`-style column carries
// that risk. These pin the `columns.len() == 1` guard in both the
// AddConstraint path (find_sequence_exhaustion_risks) and the CreateTable
// path (single_pk_with_auto_increment); a `-> true` mutant would flag them.
fn two_col_auto_inc_pk_add(table: &str, a: &str, b: &str) -> MigrationAction {
    MigrationAction::AddConstraint {
        table: TableName::from(table),
        constraint: TableConstraint::PrimaryKey {
            auto_increment: true,
            columns: vec![a.into(), b.into()],
            strategy: PrimaryKeyAdditionStrategy::default(),
        },
    }
}

#[rstest]
fn composite_auto_increment_pk_add_constraint_is_not_flagged() {
    // Baseline PK is a SAFE bigint (so `baseline_existing_risky_pk` stays
    // empty and cannot suppress); the risky smallint columns a/b are NOT a
    // single PK. The 2-column auto-inc AddConstraint must not be flagged.
    // A `columns.len() == 1 -> true` mutant would treat column `a` as a
    // lone risky sequence and emit a warning.
    let baseline = vec![table_with_pk(
        "users",
        vec![
            int_col("id", SimpleColumnType::BigInt),
            int_col("a", SimpleColumnType::SmallInt),
            int_col("b", SimpleColumnType::SmallInt),
        ],
        "id",
        false,
    )];
    let p = plan(vec![two_col_auto_inc_pk_add("users", "a", "b")]);
    assert!(
        find_sequence_exhaustion_risks(&p, &baseline).is_empty(),
        "composite auto-inc PK must not be a single-sequence risk"
    );
}

#[rstest]
fn composite_auto_increment_pk_create_table_is_not_flagged() {
    let p = plan(vec![MigrationAction::CreateTable {
        table: "users".into(),
        columns: vec![
            int_col("id", SimpleColumnType::SmallInt),
            int_col("tenant", SimpleColumnType::SmallInt),
        ],
        constraints: vec![TableConstraint::PrimaryKey {
            auto_increment: true,
            columns: vec!["id".into(), "tenant".into()],
            strategy: PrimaryKeyAdditionStrategy::default(),
        }],
    }]);
    assert!(
        find_sequence_exhaustion_risks(&p, &[]).is_empty(),
        "composite auto-inc PK must not be a single-sequence risk"
    );
}

#[rstest]
fn case_04_integer_pk_without_auto_increment_no_warning() {
    let p = plan(vec![create_table_inline_pk(
        "ref",
        vec![int_col("id", SimpleColumnType::Integer)],
        "id",
        false,
    )]);
    assert!(find_sequence_exhaustion_risks(&p, &[]).is_empty());
}

#[rstest]
fn case_05_baseline_already_exposes_no_duplicate_warning() {
    let baseline = vec![table_with_pk(
        "users",
        vec![int_col("id", SimpleColumnType::Integer)],
        "id",
        true,
    )];
    // Plan re-adds the same PK (defensive plan)
    let p = plan(vec![add_pk("users", "id", true)]);
    assert!(find_sequence_exhaustion_risks(&p, &baseline).is_empty());
}

#[rstest]
fn case_06_modify_column_type_pk_narrowing_warning() {
    let baseline = vec![table_with_pk(
        "users",
        vec![int_col("id", SimpleColumnType::BigInt)],
        "id",
        true,
    )];
    let p = plan(vec![MigrationAction::ModifyColumnType {
        table: "users".into(),
        column: "id".into(),
        new_type: ColumnType::Simple(SimpleColumnType::Integer),
        fill_with: None,
        narrowing_strategy: None,
        timezone: None,
    }]);
    let ws = find_sequence_exhaustion_risks(&p, &baseline);
    assert_eq!(ws.len(), 1);
    assert!(matches!(
        &ws[0].kind,
        SequenceExhaustionKind::PkTypeNarrowing {
            from: SimpleColumnType::BigInt
        }
    ));
}

#[rstest]
fn case_07_fk_parent_bigint_child_integer_mismatch_warning() {
    let baseline = vec![
        // parent has BigInt PK
        table_with_pk(
            "users",
            vec![int_col("id", SimpleColumnType::BigInt)],
            "id",
            true,
        ),
        // child has Integer FK column (PK irrelevant)
        TableDef {
            name: "posts".into(),
            description: None,
            columns: vec![
                int_col("id", SimpleColumnType::BigInt),
                int_col("user_id", SimpleColumnType::Integer),
            ],
            constraints: vec![TableConstraint::PrimaryKey {
                auto_increment: true,
                columns: vec!["id".into()],
                strategy: PrimaryKeyAdditionStrategy::default(),
            }],
        },
    ];
    let p = plan(vec![MigrationAction::AddConstraint {
        table: "posts".into(),
        constraint: TableConstraint::ForeignKey {
            name: None,
            columns: vec!["user_id".into()],
            ref_table: "users".into(),
            ref_columns: vec!["id".into()],
            on_delete: None,
            on_update: None,
            orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
        },
    }]);
    let ws = find_sequence_exhaustion_risks(&p, &baseline);
    assert_eq!(ws.len(), 1);
    assert!(matches!(
        &ws[0].kind,
        SequenceExhaustionKind::ForeignKeyMismatch { parent_table, parent_type: SimpleColumnType::BigInt }
            if parent_table == "users"
    ));
}

#[rstest]
fn case_08_composite_pk_not_flagged() {
    let p = plan(vec![MigrationAction::CreateTable {
        table: "join_table".into(),
        columns: vec![
            int_col("a_id", SimpleColumnType::Integer),
            int_col("b_id", SimpleColumnType::Integer),
        ],
        constraints: vec![TableConstraint::PrimaryKey {
            auto_increment: false,
            columns: vec!["a_id".into(), "b_id".into()],
            strategy: PrimaryKeyAdditionStrategy::default(),
        }],
    }]);
    assert!(find_sequence_exhaustion_risks(&p, &[]).is_empty());
}

#[rstest]
fn case_09_uuid_pk_safe() {
    let mut col = int_col("id", SimpleColumnType::Uuid);
    col.r#type = ColumnType::Simple(SimpleColumnType::Uuid);
    let p = plan(vec![create_table_inline_pk(
        "users",
        vec![col],
        "id",
        false,
    )]);
    assert!(find_sequence_exhaustion_risks(&p, &[]).is_empty());
}

#[rstest]
fn case_10_fk_parent_safe_no_mismatch() {
    // parent and child both BigInt - no warning
    let baseline = vec![
        table_with_pk(
            "users",
            vec![int_col("id", SimpleColumnType::BigInt)],
            "id",
            true,
        ),
        TableDef {
            name: "posts".into(),
            description: None,
            columns: vec![
                int_col("id", SimpleColumnType::BigInt),
                int_col("user_id", SimpleColumnType::BigInt),
            ],
            constraints: vec![],
        },
    ];
    let p = plan(vec![MigrationAction::AddConstraint {
        table: "posts".into(),
        constraint: TableConstraint::ForeignKey {
            name: None,
            columns: vec!["user_id".into()],
            ref_table: "users".into(),
            ref_columns: vec!["id".into()],
            on_delete: None,
            on_update: None,
            orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
        },
    }]);
    assert!(find_sequence_exhaustion_risks(&p, &baseline).is_empty());
}

#[rstest]
fn case_11_modify_column_narrowing_non_pk_not_flagged() {
    let baseline = vec![TableDef {
        name: "users".into(),
        description: None,
        columns: vec![
            int_col("id", SimpleColumnType::BigInt),
            int_col("count", SimpleColumnType::BigInt),
        ],
        constraints: vec![TableConstraint::PrimaryKey {
            auto_increment: true,
            columns: vec!["id".into()],
            strategy: PrimaryKeyAdditionStrategy::default(),
        }],
    }];
    let p = plan(vec![MigrationAction::ModifyColumnType {
        table: "users".into(),
        column: "count".into(), // not the PK column
        new_type: ColumnType::Simple(SimpleColumnType::Integer),
        fill_with: None,
        narrowing_strategy: None,
        timezone: None,
    }]);
    assert!(find_sequence_exhaustion_risks(&p, &baseline).is_empty());
}

#[rstest]
fn case_12_multiple_warnings_in_one_plan() {
    let p = plan(vec![
        create_table_inline_pk(
            "users",
            vec![int_col("id", SimpleColumnType::Integer)],
            "id",
            true,
        ),
        create_table_inline_pk(
            "tiny",
            vec![int_col("id", SimpleColumnType::SmallInt)],
            "id",
            true,
        ),
    ]);
    let ws = find_sequence_exhaustion_risks(&p, &[]);
    assert_eq!(ws.len(), 2);
}

// ── Coverage-closure: defensive `continue` arms + helper branches ──

use vespertide_core::schema::foreign_key::{ForeignKeyDef, ForeignKeySyntax, ReferenceSyntaxDef};

fn int_col_with_inline_fk(name: &str, ty: SimpleColumnType, fk: ForeignKeySyntax) -> ColumnDef {
    let mut c = int_col(name, ty);
    c.foreign_key = Some(fk);
    c
}

/// CreateTable with inline FK to a baseline parent whose PK is wider →
/// flags the child column. Exercises the `for col in columns` inline-FK
/// scan inside the `CreateTable` arm (lines 184-204), `inline_fk_parent_table`
/// `Object` variant, and `is_narrower_than` rank-based comparison.
#[rstest]
fn case_13_create_table_inline_fk_object_to_bigint_parent_flags_child() {
    let baseline = vec![table_with_pk(
        "users",
        vec![int_col("id", SimpleColumnType::BigInt)],
        "id",
        true,
    )];
    let child_fk = ForeignKeySyntax::Object(ForeignKeyDef {
        ref_table: "users".into(),
        ref_columns: vec!["id".into()],
        on_delete: None,
        on_update: None,
        orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
    });
    let child = int_col_with_inline_fk("user_id", SimpleColumnType::Integer, child_fk);
    let p = plan(vec![MigrationAction::CreateTable {
        table: "posts".into(),
        columns: vec![int_col("id", SimpleColumnType::BigInt), child],
        constraints: vec![TableConstraint::PrimaryKey {
            auto_increment: true,
            columns: vec!["id".into()],
            strategy: PrimaryKeyAdditionStrategy::default(),
        }],
    }]);
    let ws = find_sequence_exhaustion_risks(&p, &baseline);
    // 1 warning: ForeignKeyMismatch on user_id.
    assert_eq!(ws.len(), 1);
    assert!(matches!(
        &ws[0].kind,
        SequenceExhaustionKind::ForeignKeyMismatch { parent_table, parent_type: SimpleColumnType::BigInt }
            if parent_table == "users"
    ));
}

/// CreateTable with inline FK using `String` shorthand (`"users.id"`) -
/// exercises `inline_fk_parent_table` `String` variant + same-plan
/// pk_type_map merge (parent table created in same plan).
#[rstest]
fn case_14_create_table_inline_fk_string_same_plan_parent() {
    let parent = MigrationAction::CreateTable {
        table: "users".into(),
        columns: vec![int_col("id", SimpleColumnType::BigInt)],
        constraints: vec![TableConstraint::PrimaryKey {
            auto_increment: true,
            columns: vec!["id".into()],
            strategy: PrimaryKeyAdditionStrategy::default(),
        }],
    };
    let child_fk = ForeignKeySyntax::String("users.id".into());
    let child = int_col_with_inline_fk("user_id", SimpleColumnType::SmallInt, child_fk);
    let posts = MigrationAction::CreateTable {
        table: "posts".into(),
        columns: vec![int_col("id", SimpleColumnType::BigInt), child],
        constraints: vec![TableConstraint::PrimaryKey {
            auto_increment: true,
            columns: vec!["id".into()],
            strategy: PrimaryKeyAdditionStrategy::default(),
        }],
    };
    let p = plan(vec![parent, posts]);
    let ws = find_sequence_exhaustion_risks(&p, &[]);
    // Child SmallInt vs parent BigInt → 1 warning (High risk).
    assert!(ws.iter().any(|w| matches!(
        &w.kind,
        SequenceExhaustionKind::ForeignKeyMismatch {
            parent_type: SimpleColumnType::BigInt,
            ..
        }
    ) && w.risk_level == SequenceRiskLevel::High));
}

/// CreateTable inline FK using `Reference` shorthand
/// (`{"references": "users.id"}`) — exercises the
/// `inline_fk_parent_table` `Reference` arm.
#[rstest]
fn case_15_create_table_inline_fk_reference_variant() {
    let baseline = vec![table_with_pk(
        "users",
        vec![int_col("id", SimpleColumnType::BigInt)],
        "id",
        true,
    )];
    let child_fk = ForeignKeySyntax::Reference(ReferenceSyntaxDef {
        references: "users.id".into(),
        on_delete: None,
        on_update: None,
    });
    let child = int_col_with_inline_fk("user_id", SimpleColumnType::Integer, child_fk);
    let p = plan(vec![MigrationAction::CreateTable {
        table: "posts".into(),
        columns: vec![int_col("id", SimpleColumnType::BigInt), child],
        constraints: vec![TableConstraint::PrimaryKey {
            auto_increment: true,
            columns: vec!["id".into()],
            strategy: PrimaryKeyAdditionStrategy::default(),
        }],
    }]);
    let ws = find_sequence_exhaustion_risks(&p, &baseline);
    assert!(
        ws.iter()
            .any(|w| matches!(&w.kind, SequenceExhaustionKind::ForeignKeyMismatch { .. }))
    );
}

/// AddConstraint(ForeignKey) with composite FK → `columns.len() != 1`
/// continue (line 259-261).
#[rstest]
fn case_16_add_constraint_fk_composite_skipped() {
    let baseline = vec![
        table_with_pk(
            "users",
            vec![int_col("id", SimpleColumnType::BigInt)],
            "id",
            true,
        ),
        TableDef {
            name: "posts".into(),
            description: None,
            columns: vec![
                int_col("a", SimpleColumnType::Integer),
                int_col("b", SimpleColumnType::Integer),
            ],
            constraints: vec![],
        },
    ];
    let p = plan(vec![MigrationAction::AddConstraint {
        table: "posts".into(),
        constraint: TableConstraint::ForeignKey {
            name: None,
            columns: vec!["a".into(), "b".into()],
            ref_table: "users".into(),
            ref_columns: vec!["id".into(), "id".into()],
            on_delete: None,
            on_update: None,
            orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
        },
    }]);
    assert!(find_sequence_exhaustion_risks(&p, &baseline).is_empty());
}

/// AddConstraint(ForeignKey) referencing a parent NOT in pk_type_map →
/// `pk_type_map.get None` continue (line 263-265).
#[rstest]
fn case_17_add_constraint_fk_parent_not_in_pk_type_map() {
    let baseline = vec![TableDef {
        name: "posts".into(),
        description: None,
        columns: vec![int_col("user_id", SimpleColumnType::Integer)],
        constraints: vec![],
    }];
    let p = plan(vec![MigrationAction::AddConstraint {
        table: "posts".into(),
        constraint: TableConstraint::ForeignKey {
            name: None,
            columns: vec!["user_id".into()],
            ref_table: "unknown".into(), // not in baseline
            ref_columns: vec!["id".into()],
            on_delete: None,
            on_update: None,
            orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
        },
    }]);
    assert!(find_sequence_exhaustion_risks(&p, &baseline).is_empty());
}

/// AddConstraint(ForeignKey) on a table that's not in baseline → child
/// `baseline.iter().find None` continue (line 266-269).
#[rstest]
fn case_18_add_constraint_fk_child_table_missing_in_baseline() {
    let baseline = vec![table_with_pk(
        "users",
        vec![int_col("id", SimpleColumnType::BigInt)],
        "id",
        true,
    )];
    let p = plan(vec![MigrationAction::AddConstraint {
        table: "ghost_table".into(),
        constraint: TableConstraint::ForeignKey {
            name: None,
            columns: vec!["user_id".into()],
            ref_table: "users".into(),
            ref_columns: vec!["id".into()],
            on_delete: None,
            on_update: None,
            orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
        },
    }]);
    assert!(find_sequence_exhaustion_risks(&p, &baseline).is_empty());
}

/// AddConstraint(ForeignKey) with child column missing from baseline →
/// `col None` continue (line 270-275).
#[rstest]
fn case_19_add_constraint_fk_child_column_missing() {
    let baseline = vec![
        table_with_pk(
            "users",
            vec![int_col("id", SimpleColumnType::BigInt)],
            "id",
            true,
        ),
        TableDef {
            name: "posts".into(),
            description: None,
            columns: vec![int_col("id", SimpleColumnType::BigInt)],
            constraints: vec![],
        },
    ];
    let p = plan(vec![MigrationAction::AddConstraint {
        table: "posts".into(),
        constraint: TableConstraint::ForeignKey {
            name: None,
            columns: vec!["ghost_col".into()],
            ref_table: "users".into(),
            ref_columns: vec!["id".into()],
            on_delete: None,
            on_update: None,
            orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
        },
    }]);
    assert!(find_sequence_exhaustion_risks(&p, &baseline).is_empty());
}

/// AddConstraint(ForeignKey) with non-int child column → `simple_int_type_of
/// None` continue (line 277-279). Exercises `simple_int_type_of` `_ => None`.
#[rstest]
fn case_20_add_constraint_fk_child_column_non_int() {
    let mut text_col = int_col("user_ref", SimpleColumnType::Integer);
    text_col.r#type = ColumnType::Simple(SimpleColumnType::Text);
    let baseline = vec![
        table_with_pk(
            "users",
            vec![int_col("id", SimpleColumnType::BigInt)],
            "id",
            true,
        ),
        TableDef {
            name: "posts".into(),
            description: None,
            columns: vec![int_col("id", SimpleColumnType::BigInt), text_col],
            constraints: vec![],
        },
    ];
    let p = plan(vec![MigrationAction::AddConstraint {
        table: "posts".into(),
        constraint: TableConstraint::ForeignKey {
            name: None,
            columns: vec!["user_ref".into()],
            ref_table: "users".into(),
            ref_columns: vec!["id".into()],
            on_delete: None,
            on_update: None,
            orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
        },
    }]);
    assert!(find_sequence_exhaustion_risks(&p, &baseline).is_empty());
}

/// AddConstraint(ForeignKey) with child same width as parent (`!is_narrower_than`)
/// → continue (line 280-282).
#[rstest]
fn case_21_add_constraint_fk_child_same_width_skipped() {
    let baseline = vec![
        table_with_pk(
            "users",
            vec![int_col("id", SimpleColumnType::BigInt)],
            "id",
            true,
        ),
        TableDef {
            name: "posts".into(),
            description: None,
            columns: vec![
                int_col("id", SimpleColumnType::BigInt),
                int_col("user_id", SimpleColumnType::BigInt),
            ],
            constraints: vec![],
        },
    ];
    let p = plan(vec![MigrationAction::AddConstraint {
        table: "posts".into(),
        constraint: TableConstraint::ForeignKey {
            name: None,
            columns: vec!["user_id".into()],
            ref_table: "users".into(),
            ref_columns: vec!["id".into()],
            on_delete: None,
            on_update: None,
            orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
        },
    }]);
    assert!(find_sequence_exhaustion_risks(&p, &baseline).is_empty());
}

/// ModifyColumnType on a table not in baseline → `table_def None`
/// continue (line 307-310).
#[rstest]
fn case_22_modify_column_type_table_missing_in_baseline() {
    let baseline: Vec<TableDef> = vec![];
    let p = plan(vec![MigrationAction::ModifyColumnType {
        table: "ghost".into(),
        column: "id".into(),
        new_type: ColumnType::Simple(SimpleColumnType::Integer),
        fill_with: None,
        narrowing_strategy: None,
        timezone: None,
    }]);
    assert!(find_sequence_exhaustion_risks(&p, &baseline).is_empty());
}

/// ModifyColumnType on a column missing from the baseline table →
/// `col None` continue (line 311-317).
#[rstest]
fn case_23_modify_column_type_column_missing() {
    let baseline = vec![table_with_pk(
        "users",
        vec![int_col("id", SimpleColumnType::BigInt)],
        "id",
        true,
    )];
    let p = plan(vec![MigrationAction::ModifyColumnType {
        table: "users".into(),
        column: "ghost_col".into(),
        new_type: ColumnType::Simple(SimpleColumnType::Integer),
        fill_with: None,
        narrowing_strategy: None,
        timezone: None,
    }]);
    assert!(find_sequence_exhaustion_risks(&p, &baseline).is_empty());
}

/// ModifyColumnType on a non-int baseline column → `simple_int_type_of
/// None` for from_ty continue (line 318-320).
#[rstest]
fn case_24_modify_column_type_from_non_int_skipped() {
    let mut text_col = int_col("name", SimpleColumnType::Integer);
    text_col.r#type = ColumnType::Simple(SimpleColumnType::Text);
    let baseline = vec![TableDef {
        name: "users".into(),
        description: None,
        columns: vec![int_col("id", SimpleColumnType::BigInt), text_col],
        constraints: vec![TableConstraint::PrimaryKey {
            auto_increment: true,
            columns: vec!["id".into()],
            strategy: PrimaryKeyAdditionStrategy::default(),
        }],
    }];
    let p = plan(vec![MigrationAction::ModifyColumnType {
        table: "users".into(),
        column: "name".into(),
        new_type: ColumnType::Simple(SimpleColumnType::Integer),
        fill_with: None,
        narrowing_strategy: None,
        timezone: None,
    }]);
    assert!(find_sequence_exhaustion_risks(&p, &baseline).is_empty());
}

/// ModifyColumnType to a non-int target → `simple_int_type_of None`
/// for to_ty continue (line 321-323).
#[rstest]
fn case_25_modify_column_type_to_non_int_skipped() {
    let baseline = vec![table_with_pk(
        "users",
        vec![int_col("id", SimpleColumnType::BigInt)],
        "id",
        true,
    )];
    let p = plan(vec![MigrationAction::ModifyColumnType {
        table: "users".into(),
        column: "id".into(),
        new_type: ColumnType::Simple(SimpleColumnType::Text), // non-int
        fill_with: None,
        narrowing_strategy: None,
        timezone: None,
    }]);
    assert!(find_sequence_exhaustion_risks(&p, &baseline).is_empty());
}

/// ModifyColumnType from non-BigInt (already Integer) to risky → skip
/// (line 324: `from_ty != BigInt`).
#[rstest]
fn case_26_modify_column_type_from_integer_to_smallint_not_pk_narrowing() {
    // Baseline column is Integer; F76 narrowing is only flagged when
    // the column was BigInt → smaller. Integer→SmallInt does not
    // trigger PkTypeNarrowing (separate F6 prompt covers truncation).
    let baseline = vec![table_with_pk(
        "users",
        vec![int_col("id", SimpleColumnType::Integer)],
        "id",
        true,
    )];
    let p = plan(vec![MigrationAction::ModifyColumnType {
        table: "users".into(),
        column: "id".into(),
        new_type: ColumnType::Simple(SimpleColumnType::SmallInt),
        fill_with: None,
        narrowing_strategy: None,
        timezone: None,
    }]);
    // F76 doesn't fire because from_ty != BigInt.
    assert!(find_sequence_exhaustion_risks(&p, &baseline).is_empty());
}

/// ModifyColumnType BigInt → BigInt (no risky target) → skip
/// (line 324: `!is_risky_int_type(to_ty)`).
#[rstest]
fn case_27_modify_column_type_bigint_to_bigint_safe() {
    let baseline = vec![table_with_pk(
        "users",
        vec![int_col("id", SimpleColumnType::BigInt)],
        "id",
        true,
    )];
    let p = plan(vec![MigrationAction::ModifyColumnType {
        table: "users".into(),
        column: "id".into(),
        new_type: ColumnType::Simple(SimpleColumnType::BigInt),
        fill_with: None,
        narrowing_strategy: None,
        timezone: None,
    }]);
    assert!(find_sequence_exhaustion_risks(&p, &baseline).is_empty());
}

/// CreateTable with composite PK (`pk_columns.len() != 1`) and inline
/// auto_increment → `single_pk_type_from_create_table` returns None
/// (line 415-417) and the `pk_type_map` insert is skipped.
#[rstest]
fn case_28_create_table_composite_pk_skipped_for_pk_type_map() {
    // No FK to flag → simply asserts no warnings.
    let p = plan(vec![MigrationAction::CreateTable {
        table: "join".into(),
        columns: vec![
            int_col("a", SimpleColumnType::Integer),
            int_col("b", SimpleColumnType::Integer),
        ],
        constraints: vec![TableConstraint::PrimaryKey {
            auto_increment: false,
            columns: vec!["a".into(), "b".into()],
            strategy: PrimaryKeyAdditionStrategy::default(),
        }],
    }]);
    assert!(find_sequence_exhaustion_risks(&p, &[]).is_empty());
}

/// Baseline with single-column BigInt PK → `risky_single_pk_columns`
/// returns `vec![]` (line 361-363, `!is_risky_int_type` arm).
#[rstest]
fn case_29_bigint_pk_baseline_does_not_seed_existing_risky_set() {
    // Plan AddConstraint(PrimaryKey) on the same BigInt column. The
    // baseline_existing_risky_pk set is empty (BigInt isn't risky), so
    // the new PK addition is not suppressed; but BigInt PK is still
    // safe so no warning emitted.
    let baseline = vec![table_with_pk(
        "users",
        vec![int_col("id", SimpleColumnType::BigInt)],
        "id",
        true,
    )];
    let p = plan(vec![add_pk("users", "id", true)]);
    // No warning: BigInt is safe.
    assert!(find_sequence_exhaustion_risks(&p, &baseline).is_empty());
}

/// CreateTable using an INLINE column-level PK (not a table-level
/// `PrimaryKey` constraint). Exercises
/// `single_pk_type_from_create_table`'s inline-fallback path
/// (lines 406-413: when `table_level` is None) and similarly
/// `single_pk_column_with_type`'s inline fallback.
#[rstest]
fn case_30_create_table_inline_column_pk_seeds_pk_type_map() {
    use vespertide_core::schema::primary_key::PrimaryKeySyntax;
    let mut parent_col = int_col("id", SimpleColumnType::BigInt);
    parent_col.primary_key = Some(PrimaryKeySyntax::Bool(true));
    // Child has inline FK referencing parent — only path to assert the
    // pk_type_map was populated correctly via inline PK.
    let parent = MigrationAction::CreateTable {
        table: "users".into(),
        columns: vec![parent_col],
        constraints: vec![],
    };
    let mut child_id = int_col("id", SimpleColumnType::BigInt);
    child_id.primary_key = Some(PrimaryKeySyntax::Bool(true));
    let child_fk = ForeignKeySyntax::String("users.id".into());
    let child_col = int_col_with_inline_fk("user_id", SimpleColumnType::Integer, child_fk);
    let posts = MigrationAction::CreateTable {
        table: "posts".into(),
        columns: vec![child_id, child_col],
        constraints: vec![],
    };
    let p = plan(vec![parent, posts]);
    let ws = find_sequence_exhaustion_risks(&p, &[]);
    // The FK mismatch (Integer vs BigInt parent) should be flagged,
    // which means pk_type_map was seeded from the inline-PK CreateTable.
    assert!(ws.iter().any(|w| matches!(
        &w.kind,
        SequenceExhaustionKind::ForeignKeyMismatch {
            parent_type: SimpleColumnType::BigInt,
            ..
        }
    )));
}

/// Direct unit tests for private helpers
/// (`simple_int_type_of`, `is_risky_int_type`, `classify_risky_int_type`,
/// `is_narrower_than`) — locks every reachable arm.
#[rstest]
fn case_31_helper_functions_cover_all_reachable_arms() {
    assert_eq!(
        simple_int_type_of(&ColumnType::Simple(SimpleColumnType::SmallInt)),
        Some(SimpleColumnType::SmallInt)
    );
    assert_eq!(
        simple_int_type_of(&ColumnType::Simple(SimpleColumnType::Integer)),
        Some(SimpleColumnType::Integer)
    );
    assert_eq!(
        simple_int_type_of(&ColumnType::Simple(SimpleColumnType::BigInt)),
        Some(SimpleColumnType::BigInt)
    );
    // Non-int Simple → None.
    assert!(simple_int_type_of(&ColumnType::Simple(SimpleColumnType::Text)).is_none());

    assert!(is_risky_int_type(SimpleColumnType::SmallInt));
    assert!(is_risky_int_type(SimpleColumnType::Integer));
    assert!(!is_risky_int_type(SimpleColumnType::BigInt));

    assert_eq!(
        classify_risky_int_type(SimpleColumnType::SmallInt),
        Some((SimpleColumnType::SmallInt, SequenceRiskLevel::High))
    );
    assert_eq!(
        classify_risky_int_type(SimpleColumnType::Integer),
        Some((SimpleColumnType::Integer, SequenceRiskLevel::Medium))
    );
    assert!(classify_risky_int_type(SimpleColumnType::BigInt).is_none());

    // is_narrower_than: SmallInt(0) < Integer(1) < BigInt(2); others rank 3.
    assert!(is_narrower_than(
        SimpleColumnType::SmallInt,
        SimpleColumnType::Integer
    ));
    assert!(is_narrower_than(
        SimpleColumnType::Integer,
        SimpleColumnType::BigInt
    ));
    assert!(!is_narrower_than(
        SimpleColumnType::BigInt,
        SimpleColumnType::Integer
    ));
    // Non-int (rank 3) is never narrower than rank-3 itself.
    assert!(!is_narrower_than(
        SimpleColumnType::Text,
        SimpleColumnType::Text
    ));
    // BigInt(2) < Text(3) → BigInt IS narrower than Text per the rank.
    assert!(is_narrower_than(
        SimpleColumnType::BigInt,
        SimpleColumnType::Text
    ));
}

/// `inline_fk_parent_table` Reference variant whose `references` is
/// not in `"table.column"` form → returns `None` (no dot to split on).
#[rstest]
fn case_32_inline_fk_parent_table_reference_no_dot_returns_none() {
    let mut col = int_col("ref", SimpleColumnType::Integer);
    col.foreign_key = Some(ForeignKeySyntax::Reference(ReferenceSyntaxDef {
        references: "no_dot_here".into(),
        on_delete: None,
        on_update: None,
    }));
    assert!(inline_fk_parent_table(&col).is_none());
}

/// AddConstraint(PrimaryKey) targeting a column whose baseline type is
/// **non-integer** (e.g. Text) → `simple_int_type_of` returns None →
/// the let-else `continue` arm fires and no warning is emitted.
/// Exercises the defensive guard at the "col_ty None" branch of the
/// AddConstraint(PrimaryKey) arm.
#[rstest]
fn case_34_add_constraint_pk_non_int_column_skipped() {
    let mut text_col = int_col("name", SimpleColumnType::Integer);
    text_col.r#type = ColumnType::Simple(SimpleColumnType::Text);
    let baseline = vec![TableDef {
        name: "users".into(),
        description: None,
        columns: vec![text_col],
        constraints: vec![],
    }];
    let p = plan(vec![add_pk("users", "name", true)]);
    assert!(find_sequence_exhaustion_risks(&p, &baseline).is_empty());
}

/// AddConstraint(PrimaryKey) where the target table is missing from the
/// baseline → the let-else `table_def` continue fires. Exercises the
/// "table_def None" branch.
#[rstest]
fn case_35_add_constraint_pk_table_missing_in_baseline() {
    let baseline: Vec<TableDef> = vec![];
    let p = plan(vec![add_pk("ghost", "id", true)]);
    assert!(find_sequence_exhaustion_risks(&p, &baseline).is_empty());
}

/// AddConstraint(PrimaryKey) where the target column is missing from
/// the baseline table → the let-else `col` continue fires. Exercises
/// the "col None" branch.
#[rstest]
fn case_36_add_constraint_pk_column_missing_in_baseline() {
    let baseline = vec![table_with_pk(
        "users",
        vec![int_col("id", SimpleColumnType::Integer)],
        "id",
        false,
    )];
    let p = plan(vec![add_pk("users", "ghost_col", true)]);
    assert!(find_sequence_exhaustion_risks(&p, &baseline).is_empty());
}

/// `single_pk_with_auto_increment` inline-fallback path. Currently
/// returns `None` because v0.2 only resolves table-level inline-PK
/// auto_increment. This locks that behaviour against regression.
#[rstest]
fn case_33_single_pk_with_auto_increment_inline_fallback_returns_none() {
    use vespertide_core::schema::primary_key::PrimaryKeySyntax;
    let mut col = int_col("id", SimpleColumnType::Integer);
    col.primary_key = Some(PrimaryKeySyntax::Bool(true));
    // No table-level PrimaryKey constraint, only inline.
    let got = single_pk_with_auto_increment(&[col], &[]);
    assert!(got.is_none());
}

#[rstest]
fn case_37_add_constraint_pk_new_integer_column_warns() {
    let baseline = vec![TableDef {
        name: "users".into(),
        description: None,
        columns: vec![int_col("id", SimpleColumnType::Integer)],
        constraints: vec![],
    }];
    let p = plan(vec![add_pk("users", "id", true)]);
    let ws = find_sequence_exhaustion_risks(&p, &baseline);
    assert_eq!(ws.len(), 1);
    assert_eq!(ws[0].kind, SequenceExhaustionKind::Primary);
    assert_eq!(ws[0].risk_level, SequenceRiskLevel::Medium);
}

#[rstest]
fn case_38_single_pk_column_with_type_skips_non_pk_constraint() {
    let table = TableDef {
        name: "users".into(),
        description: None,
        columns: vec![int_col("id", SimpleColumnType::Integer)],
        constraints: vec![TableConstraint::Index {
            name: None,
            columns: vec!["id".into()],
        }],
    };
    assert!(single_pk_column_with_type(&table).is_none());
}

#[rstest]
fn case_39_is_single_pk_column_skips_non_pk_constraint() {
    let table = TableDef {
        name: "users".into(),
        description: None,
        columns: vec![int_col("id", SimpleColumnType::Integer)],
        constraints: vec![TableConstraint::Unique {
            name: None,
            columns: vec!["id".into()],
            strategy: vespertide_core::UniqueConstraintStrategy::default(),
        }],
    };
    assert!(!is_single_pk_column(&table, "id"));
}
