use super::*;
use crate::test_support::{col_nullable as col, idx, table};
use rstest::rstest;
use vespertide_core::{ColumnDef, ColumnType, SimpleColumnType, TableConstraint};

#[derive(Debug, Clone, Copy)]
enum ErrKind {
    TableExists,
    TableNotFound,
    ColumnExists,
    ColumnNotFound,
}

fn assert_err_kind(err: &crate::error::PlannerError, kind: ErrKind) {
    if matches!(
        (err, kind),
        (
            crate::error::PlannerError::TableExists(_),
            ErrKind::TableExists
        ) | (
            crate::error::PlannerError::TableNotFound(_),
            ErrKind::TableNotFound
        ) | (
            crate::error::PlannerError::ColumnExists(_, _),
            ErrKind::ColumnExists
        ) | (
            crate::error::PlannerError::ColumnNotFound(_, _),
            ErrKind::ColumnNotFound
        )
    ) {
        return;
    }

    panic!("unexpected error {err:?}, expected {kind:?}");
}

#[rstest]
#[case(
        vec![table("users", vec![], vec![])],
        MigrationAction::CreateTable {
            table: "users".into(),
            columns: vec![],
            constraints: vec![],
        },
        ErrKind::TableExists
    )]
#[case(
        vec![],
        MigrationAction::DeleteTable {
            table: "users".into()
        },
        ErrKind::TableNotFound
    )]
#[case(
        vec![table(
            "users",
            vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
            vec![]
        )],
        MigrationAction::AddColumn {
            table: "users".into(),
            column: Box::new(col("id", ColumnType::Simple(SimpleColumnType::Integer))),
            fill_with: None,
        },
        ErrKind::ColumnExists
    )]
#[case(
        vec![table(
            "users",
            vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
            vec![]
        )],
        MigrationAction::DeleteColumn {
            table: "users".into(),
            column: "missing".into()
        },
        ErrKind::ColumnNotFound
    )]
#[case(
        vec![
            table("old", vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))], vec![]),
            table("new", vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))], vec![]),
        ],
        MigrationAction::RenameTable {
            from: "old".into(),
            to: "new".into()
        },
        ErrKind::TableExists
    )]
fn apply_action_reports_errors(
    #[case] mut schema: Vec<TableDef>,
    #[case] action: MigrationAction,
    #[case] expected: ErrKind,
) {
    let err = apply_action(&mut schema, &action).unwrap_err();
    assert_err_kind(&err, expected);
}

#[derive(Clone)]
struct SuccessCase {
    initial: Vec<TableDef>,
    actions: Vec<MigrationAction>,
    expected: Vec<TableDef>,
}

#[rstest]
#[case(SuccessCase {
        initial: vec![],
        actions: vec![
            MigrationAction::CreateTable {
                table: "users".into(),
                columns: vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
                constraints: vec![],
            },
            MigrationAction::DeleteTable {
                table: "users".into(),
            },
        ],
        expected: vec![],
    })]
#[case(SuccessCase {
        initial: vec![table(
            "users",
            vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                col("old", ColumnType::Simple(SimpleColumnType::Text)),
                col("ref_id", ColumnType::Simple(SimpleColumnType::Integer))
            ],
            vec![
                TableConstraint::PrimaryKey { auto_increment: false, columns: vec!["id".into()], strategy: vespertide_core::PrimaryKeyAdditionStrategy::default() },
                TableConstraint::Unique {
                    name: Some("u_old".into()),
                    columns: vec!["old".into()],
                    strategy: vespertide_core::UniqueConstraintStrategy::DeleteDuplicates { keep: vespertide_core::KeepPolicy::First },
                },
                TableConstraint::ForeignKey {
                    name: Some("fk_old".into()),
                    columns: vec!["old".into()],
                    ref_table: "ref_table".into(),
                    ref_columns: vec!["ref_id".into()],
                    on_delete: None,
                    on_update: None,
                    orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
                },
                TableConstraint::Check {
                    name: "ck_old".into(),
                    expr: "old IS NOT NULL".into(),
                    strategy: vespertide_core::CheckViolationStrategy::default(),
                },
                idx("idx_old", vec!["old"]),
                idx("idx_ref", vec!["ref_id"]),
            ],
        )],
        actions: vec![
            MigrationAction::AddColumn {
                table: "users".into(),
                column: Box::new(col("new_col", ColumnType::Simple(SimpleColumnType::Boolean))),
                fill_with: None,
            },
            MigrationAction::RenameColumn {
                table: "users".into(),
                from: "ref_id".into(),
                to: "renamed".into(),
            },
        ],
        expected: vec![table(
            "users",
            vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                col("old", ColumnType::Simple(SimpleColumnType::Text)),
                col("renamed", ColumnType::Simple(SimpleColumnType::Integer)),
                col("new_col", ColumnType::Simple(SimpleColumnType::Boolean))
            ],
            vec![
                TableConstraint::PrimaryKey { auto_increment: false, columns: vec!["id".into()], strategy: vespertide_core::PrimaryKeyAdditionStrategy::default() },
                TableConstraint::Unique {
                    name: Some("u_old".into()),
                    columns: vec!["old".into()],
                    strategy: vespertide_core::UniqueConstraintStrategy::DeleteDuplicates { keep: vespertide_core::KeepPolicy::First },
                },
                TableConstraint::ForeignKey {
                    name: Some("fk_old".into()),
                    columns: vec!["old".into()],
                    ref_table: "ref_table".into(),
                    ref_columns: vec!["renamed".into()],
                    on_delete: None,
                    on_update: None,
                    orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
                },
                TableConstraint::Check {
                    name: "ck_old".into(),
                    expr: "old IS NOT NULL".into(),
                    strategy: vespertide_core::CheckViolationStrategy::default(),
                },
                idx("idx_old", vec!["old"]),
                idx("idx_ref", vec!["renamed"]),
            ],
        )],
    })]
#[case(SuccessCase {
        initial: vec![table(
            "users",
            vec![col("id", ColumnType::Simple(SimpleColumnType::Integer)), col("old", ColumnType::Simple(SimpleColumnType::Text))],
            vec![
                TableConstraint::PrimaryKey { auto_increment: false, columns: vec!["id".into()], strategy: vespertide_core::PrimaryKeyAdditionStrategy::default() },
                TableConstraint::Unique {
                    name: Some("u_old".into()),
                    columns: vec!["old".into()],
                    strategy: vespertide_core::UniqueConstraintStrategy::DeleteDuplicates { keep: vespertide_core::KeepPolicy::First },
                },
                TableConstraint::ForeignKey {
                    name: Some("fk_old".into()),
                    columns: vec!["old".into()],
                    ref_table: "ref_table".into(),
                    ref_columns: vec!["old".into()],
                    on_delete: None,
                    on_update: None,
                    orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
                },
                TableConstraint::Check {
                    name: "ck_old".into(),
                    expr: "old IS NOT NULL".into(),
                    strategy: vespertide_core::CheckViolationStrategy::default(),
                },
                idx("idx_old", vec!["old"]),
            ],
        )],
        actions: vec![MigrationAction::DeleteColumn {
            table: "users".into(),
            column: "old".into(),
        }],
        expected: vec![table(
            "users",
            vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
            vec![
                TableConstraint::PrimaryKey { auto_increment: false, columns: vec!["id".into()], strategy: vespertide_core::PrimaryKeyAdditionStrategy::default() },
                TableConstraint::Check {
                    name: "ck_old".into(),
                    expr: "old IS NOT NULL".into(),
                    strategy: vespertide_core::CheckViolationStrategy::default(),
                },
            ],
        )],
    })]
#[case(SuccessCase {
        initial: vec![table(
            "users",
            vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
            vec![],
        )],
        actions: vec![
            MigrationAction::ModifyColumnType {
                table: "users".into(),
                column: "id".into(),
                new_type: ColumnType::Simple(SimpleColumnType::Text),
                fill_with: None,
                narrowing_strategy: None,
                timezone: None,
            },
            MigrationAction::AddConstraint {
                table: "users".into(),
                constraint: idx("idx_id", vec!["id"]),
            },
            MigrationAction::RemoveConstraint {
                table: "users".into(),
                constraint: idx("idx_id", vec!["id"]),
            },
        ],
        expected: vec![table(
            "users",
            vec![col("id", ColumnType::Simple(SimpleColumnType::Text))],
            vec![],
        )],
    })]
#[case(SuccessCase {
        initial: vec![table(
            "old",
            vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
            vec![],
        )],
        actions: vec![MigrationAction::RenameTable {
            from: "old".into(),
            to: "new".into(),
        }],
        expected: vec![table(
            "new",
            vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
            vec![],
        )],
    })]
#[case(SuccessCase {
        initial: vec![table("users", vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))], vec![])],
        actions: vec![MigrationAction::AddConstraint {
            table: "users".into(),
            constraint: TableConstraint::PrimaryKey {
                auto_increment: false,
                columns: vec!["id".into()],
                strategy: vespertide_core::PrimaryKeyAdditionStrategy::default(),
            },
        }],
        expected: vec![table(
            "users",
            vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
            vec![TableConstraint::PrimaryKey {
                auto_increment: false,
                columns: vec!["id".into()],
                strategy: vespertide_core::PrimaryKeyAdditionStrategy::default(),
            }],
        )],
    })]
#[case(SuccessCase {
        initial: vec![table(
            "users",
            vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
            vec![TableConstraint::PrimaryKey {
                auto_increment: false,
                columns: vec!["id".into()],
                strategy: vespertide_core::PrimaryKeyAdditionStrategy::default(),
            }],
        )],
        actions: vec![MigrationAction::RemoveConstraint {
            table: "users".into(),
            constraint: TableConstraint::PrimaryKey {
                auto_increment: false,
                columns: vec!["id".into()],
                strategy: vespertide_core::PrimaryKeyAdditionStrategy::default(),
            },
        }],
        expected: vec![table("users", vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))], vec![])],
    })]
#[case(SuccessCase {
        initial: vec![table("users", vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))], vec![])],
        actions: vec![MigrationAction::RawSql {
            sql: "SELECT 1;".to_string(),
        }],
        expected: vec![table("users", vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))], vec![])],
    })]
fn apply_action_success_cases(#[case] case: SuccessCase) {
    let mut schema = case.initial;
    for action in case.actions {
        apply_action(&mut schema, &action).unwrap();
    }
    assert_eq!(schema, case.expected);
}

#[test]
fn apply_rename_table_rewrites_foreign_key_ref_table() {
    let mut schema = vec![
        table(
            "users",
            vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
            vec![],
        ),
        table(
            "posts",
            vec![col(
                "user_id",
                ColumnType::Simple(SimpleColumnType::Integer),
            )],
            vec![TableConstraint::ForeignKey {
                name: Some("fk_posts__user_id".into()),
                columns: vec!["user_id".into()],
                ref_table: "users".into(),
                ref_columns: vec!["id".into()],
                on_delete: None,
                on_update: None,
                orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
            }],
        ),
    ];

    apply_action(
        &mut schema,
        &MigrationAction::RenameTable {
            from: "users".into(),
            to: "account".into(),
        },
    )
    .unwrap();

    let posts = schema.iter().find(|table| table.name == "posts").unwrap();
    assert!(posts.constraints.iter().any(|constraint| matches!(
        constraint,
        TableConstraint::ForeignKey { ref_table, .. } if ref_table == "account"
    )));
}

#[test]
fn apply_delete_column_preserves_foreign_key_ref_columns() {
    let mut schema = vec![table(
        "orders",
        vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            col("user_id", ColumnType::Simple(SimpleColumnType::Integer)),
        ],
        vec![TableConstraint::ForeignKey {
            name: Some("fk_orders__user_id".into()),
            columns: vec!["user_id".into()],
            ref_table: "users".into(),
            ref_columns: vec!["id".into()],
            on_delete: None,
            on_update: None,
            orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
        }],
    )];

    apply_action(
        &mut schema,
        &MigrationAction::DeleteColumn {
            table: "orders".into(),
            column: "id".into(),
        },
    )
    .unwrap();

    assert_eq!(schema[0].constraints.len(), 1);
    assert!(matches!(
        &schema[0].constraints[0],
        TableConstraint::ForeignKey { columns, ref_columns, .. }
            if columns == &["user_id"] && ref_columns == &["id"]
    ));
}

#[rstest]
#[case(
        vec![
            TableConstraint::PrimaryKey { auto_increment: false, columns: vec!["id".into(), "old".into()], strategy: vespertide_core::PrimaryKeyAdditionStrategy::default() },
            TableConstraint::Unique {
                name: None,
                columns: vec!["old".into(), "keep".into()],
                strategy: vespertide_core::UniqueConstraintStrategy::DeleteDuplicates { keep: vespertide_core::KeepPolicy::First },
            },
            TableConstraint::ForeignKey {
                name: None,
                columns: vec!["old".into()],
                ref_table: "ref".into(),
                ref_columns: vec!["old".into()],
                on_delete: None,
                on_update: None,
                orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
            },
            TableConstraint::Check {
                name: "ck_old".into(),
                expr: "old > 0".into(),
                strategy: vespertide_core::CheckViolationStrategy::default(),
            },
            idx("idx_old", vec!["old", "keep"]),
        ],
        "old",
        "new",
        vec![
            TableConstraint::PrimaryKey { auto_increment: false, columns: vec!["id".into(), "new".into()], strategy: vespertide_core::PrimaryKeyAdditionStrategy::default() },
            TableConstraint::Unique {
                name: None,
                columns: vec!["new".into(), "keep".into()],
                strategy: vespertide_core::UniqueConstraintStrategy::DeleteDuplicates { keep: vespertide_core::KeepPolicy::First },
            },
            TableConstraint::ForeignKey {
                name: None,
                columns: vec!["new".into()],
                ref_table: "ref".into(),
                ref_columns: vec!["new".into()],
                on_delete: None,
                on_update: None,
                orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
            },
            TableConstraint::Check {
                name: "ck_old".into(),
                expr: "old > 0".into(),
                strategy: vespertide_core::CheckViolationStrategy::default(),
            },
            idx("idx_old", vec!["new", "keep"]),
        ]
    )]
#[case(
        vec![
            TableConstraint::PrimaryKey { auto_increment: false, columns: vec!["id".into()], strategy: vespertide_core::PrimaryKeyAdditionStrategy::default() },
            TableConstraint::Check {
                name: "ck_id".into(),
                expr: "id > 0".into(),
                strategy: vespertide_core::CheckViolationStrategy::default(),
            },
            idx("idx_id", vec!["id"]),
        ],
        "missing",
        "new",
        vec![
            TableConstraint::PrimaryKey { auto_increment: false, columns: vec!["id".into()], strategy: vespertide_core::PrimaryKeyAdditionStrategy::default() },
            TableConstraint::Check {
                name: "ck_id".into(),
                expr: "id > 0".into(),
                strategy: vespertide_core::CheckViolationStrategy::default(),
            },
            idx("idx_id", vec!["id"]),
        ]
    )]
fn rename_helpers_update_constraints(
    #[case] mut constraints: Vec<TableConstraint>,
    #[case] from: &str,
    #[case] to: &str,
    #[case] expected_constraints: Vec<TableConstraint>,
) {
    super::column_ops::rename_column_in_constraints_for_test(&mut constraints, from, to);
    assert_eq!(constraints, expected_constraints);
}

// Tests for RemoveConstraint (Index) clearing inline index on columns
#[test]
fn remove_index_constraint_clears_inline_index_bool() {
    // Column with inline index: true creates ix_{table}__{column} pattern
    let mut col_with_index = col("email", ColumnType::Simple(SimpleColumnType::Text));
    col_with_index.index = Some(vespertide_core::StrOrBoolOrArray::Bool(true));

    let mut schema = vec![table(
        "users",
        vec![col_with_index],
        vec![idx("ix_users__email", vec!["email"])],
    )];

    apply_action(
        &mut schema,
        &MigrationAction::RemoveConstraint {
            table: "users".into(),
            constraint: idx("ix_users__email", vec!["email"]),
        },
    )
    .unwrap();

    // Index should be removed from constraints
    assert!(schema[0].constraints.is_empty());
    // Inline index on column should also be cleared
    assert!(schema[0].columns[0].index.is_none());
}

#[test]
fn remove_index_constraint_clears_inline_index_str() {
    // Column with inline index: "custom_idx_name"
    let mut col_with_index = col("email", ColumnType::Simple(SimpleColumnType::Text));
    col_with_index.index = Some(vespertide_core::StrOrBoolOrArray::Str(
        "custom_idx_name".into(),
    ));

    let mut schema = vec![table(
        "users",
        vec![col_with_index],
        vec![idx("custom_idx_name", vec!["email"])],
    )];

    apply_action(
        &mut schema,
        &MigrationAction::RemoveConstraint {
            table: "users".into(),
            constraint: idx("custom_idx_name", vec!["email"]),
        },
    )
    .unwrap();

    assert!(schema[0].constraints.is_empty());
    assert!(schema[0].columns[0].index.is_none());
}

#[test]
fn remove_index_constraint_clears_inline_index_array_partial() {
    // Column with inline index: ["idx_a", "idx_b"]
    let mut col_with_index = col("email", ColumnType::Simple(SimpleColumnType::Text));
    col_with_index.index = Some(vespertide_core::StrOrBoolOrArray::Array(vec![
        "idx_a".into(),
        "idx_b".into(),
    ]));

    let mut schema = vec![table(
        "users",
        vec![col_with_index],
        vec![idx("idx_a", vec!["email"]), idx("idx_b", vec!["email"])],
    )];

    // Remove only idx_a
    apply_action(
        &mut schema,
        &MigrationAction::RemoveConstraint {
            table: "users".into(),
            constraint: idx("idx_a", vec!["email"]),
        },
    )
    .unwrap();

    assert_eq!(schema[0].constraints.len(), 1);
    // inline index should only have idx_b remaining
    assert_eq!(
        schema[0].columns[0].index,
        Some(vespertide_core::StrOrBoolOrArray::Array(vec![
            "idx_b".into()
        ]))
    );
}

#[test]
fn remove_index_constraint_clears_inline_index_array_all() {
    // Column with inline index: ["idx_single"]
    let mut col_with_index = col("email", ColumnType::Simple(SimpleColumnType::Text));
    col_with_index.index = Some(vespertide_core::StrOrBoolOrArray::Array(vec![
        "idx_single".into(),
    ]));

    let mut schema = vec![table(
        "users",
        vec![col_with_index],
        vec![idx("idx_single", vec!["email"])],
    )];

    apply_action(
        &mut schema,
        &MigrationAction::RemoveConstraint {
            table: "users".into(),
            constraint: idx("idx_single", vec!["email"]),
        },
    )
    .unwrap();

    assert!(schema[0].constraints.is_empty());
    // When array becomes empty, inline index should be None
    assert!(schema[0].columns[0].index.is_none());
}

#[test]
fn remove_index_constraint_with_inline_bool_non_matching_name() {
    // Column with inline index: true, but index name doesn't match ix_{table}__{column} pattern
    let mut col_with_index = col("email", ColumnType::Simple(SimpleColumnType::Text));
    col_with_index.index = Some(vespertide_core::StrOrBoolOrArray::Bool(true));

    let mut schema = vec![table(
        "users",
        vec![col_with_index],
        vec![idx("custom_email_idx", vec!["email"])],
    )];

    apply_action(
        &mut schema,
        &MigrationAction::RemoveConstraint {
            table: "users".into(),
            constraint: idx("custom_email_idx", vec!["email"]),
        },
    )
    .unwrap();

    // Index removed from constraints
    assert!(schema[0].constraints.is_empty());
    // Inline index NOT cleared because name didn't match pattern
    assert_eq!(
        schema[0].columns[0].index,
        Some(vespertide_core::StrOrBoolOrArray::Bool(true))
    );
}

#[test]
fn remove_unique_constraint_clears_inline_unique_array() {
    // Column with inline unique: ["uq_email", "uq_users_email"]
    let mut col_with_unique = col("email", ColumnType::Simple(SimpleColumnType::Text));
    col_with_unique.unique = Some(vespertide_core::StrOrBoolOrArray::Array(vec![
        "uq_email".to_string(),
        "uq_users_email".to_string(),
    ]));

    let mut schema = vec![table(
        "users",
        vec![col_with_unique],
        vec![TableConstraint::Unique {
            name: Some("uq_email".into()),
            columns: vec!["email".into()],
            strategy: vespertide_core::UniqueConstraintStrategy::DeleteDuplicates {
                keep: vespertide_core::KeepPolicy::First,
            },
        }],
    )];

    apply_action(
        &mut schema,
        &MigrationAction::RemoveConstraint {
            table: "users".into(),
            constraint: TableConstraint::Unique {
                name: Some("uq_email".into()),
                columns: vec!["email".into()],
                strategy: vespertide_core::UniqueConstraintStrategy::DeleteDuplicates {
                    keep: vespertide_core::KeepPolicy::First,
                },
            },
        },
    )
    .unwrap();

    // Constraint removed
    assert!(schema[0].constraints.is_empty());
    // "uq_email" removed from array, "uq_users_email" remains
    assert_eq!(
        schema[0].columns[0].unique,
        Some(vespertide_core::StrOrBoolOrArray::Array(vec![
            "uq_users_email".to_string()
        ]))
    );
}

#[test]
fn remove_unique_constraint_clears_inline_unique_array_last_item() {
    // Column with inline unique: ["uq_email"] (only one item in array)
    let mut col_with_unique = col("email", ColumnType::Simple(SimpleColumnType::Text));
    col_with_unique.unique = Some(vespertide_core::StrOrBoolOrArray::Array(vec![
        "uq_email".to_string(),
    ]));

    let mut schema = vec![table(
        "users",
        vec![col_with_unique],
        vec![TableConstraint::Unique {
            name: Some("uq_email".into()),
            columns: vec!["email".into()],
            strategy: vespertide_core::UniqueConstraintStrategy::DeleteDuplicates {
                keep: vespertide_core::KeepPolicy::First,
            },
        }],
    )];

    apply_action(
        &mut schema,
        &MigrationAction::RemoveConstraint {
            table: "users".into(),
            constraint: TableConstraint::Unique {
                name: Some("uq_email".into()),
                columns: vec!["email".into()],
                strategy: vespertide_core::UniqueConstraintStrategy::DeleteDuplicates {
                    keep: vespertide_core::KeepPolicy::First,
                },
            },
        },
    )
    .unwrap();

    // Constraint removed
    assert!(schema[0].constraints.is_empty());
    // Array becomes empty, so unique should be None
    assert!(schema[0].columns[0].unique.is_none());
}

#[test]
fn remove_unique_constraint_clears_inline_unique_str() {
    // Column with inline unique: "uq_email"
    let mut col_with_unique = col("email", ColumnType::Simple(SimpleColumnType::Text));
    col_with_unique.unique = Some(vespertide_core::StrOrBoolOrArray::Str(
        "uq_email".to_string(),
    ));

    let mut schema = vec![table(
        "users",
        vec![col_with_unique],
        vec![TableConstraint::Unique {
            name: Some("uq_email".into()),
            columns: vec!["email".into()],
            strategy: vespertide_core::UniqueConstraintStrategy::DeleteDuplicates {
                keep: vespertide_core::KeepPolicy::First,
            },
        }],
    )];

    apply_action(
        &mut schema,
        &MigrationAction::RemoveConstraint {
            table: "users".into(),
            constraint: TableConstraint::Unique {
                name: Some("uq_email".into()),
                columns: vec!["email".into()],
                strategy: vespertide_core::UniqueConstraintStrategy::DeleteDuplicates {
                    keep: vespertide_core::KeepPolicy::First,
                },
            },
        },
    )
    .unwrap();

    // Constraint removed
    assert!(schema[0].constraints.is_empty());
    // Inline unique cleared
    assert!(schema[0].columns[0].unique.is_none());
}

#[test]
fn remove_foreign_key_constraint_clears_inline_fk() {
    use vespertide_core::schema::foreign_key::{ForeignKeyDef, ForeignKeySyntax};
    // Column with inline foreign_key
    let mut col_with_fk = col("user_id", ColumnType::Simple(SimpleColumnType::Integer));
    col_with_fk.foreign_key = Some(ForeignKeySyntax::Object(ForeignKeyDef {
        ref_table: "users".into(),
        ref_columns: vec!["id".into()],
        on_delete: None,
        on_update: None,
        orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
    }));

    let mut schema = vec![table(
        "posts",
        vec![col_with_fk],
        vec![TableConstraint::ForeignKey {
            name: Some("fk_posts_user".into()),
            columns: vec!["user_id".into()],
            ref_table: "users".into(),
            ref_columns: vec!["id".into()],
            on_delete: None,
            on_update: None,
            orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
        }],
    )];

    apply_action(
        &mut schema,
        &MigrationAction::RemoveConstraint {
            table: "posts".into(),
            constraint: TableConstraint::ForeignKey {
                name: Some("fk_posts_user".into()),
                columns: vec!["user_id".into()],
                ref_table: "users".into(),
                ref_columns: vec!["id".into()],
                on_delete: None,
                on_update: None,
                orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
            },
        },
    )
    .unwrap();

    // Constraint removed
    assert!(schema[0].constraints.is_empty());
    // Inline foreign_key cleared
    assert!(schema[0].columns[0].foreign_key.is_none());
}

#[test]
fn remove_check_constraint() {
    let mut schema = vec![table(
        "users",
        vec![col("age", ColumnType::Simple(SimpleColumnType::Integer))],
        vec![TableConstraint::Check {
            name: "check_age".into(),
            expr: "age >= 18".into(),
            strategy: vespertide_core::CheckViolationStrategy::default(),
        }],
    )];

    apply_action(
        &mut schema,
        &MigrationAction::RemoveConstraint {
            table: "users".into(),
            constraint: TableConstraint::Check {
                name: "check_age".into(),
                expr: "age >= 18".into(),
                strategy: vespertide_core::CheckViolationStrategy::default(),
            },
        },
    )
    .unwrap();

    // Constraint removed
    assert!(schema[0].constraints.is_empty());
}

#[test]
fn remove_unnamed_index_single_column() {
    // Column with inline index: true
    let mut col_with_index = col("email", ColumnType::Simple(SimpleColumnType::Text));
    col_with_index.index = Some(vespertide_core::StrOrBoolOrArray::Bool(true));

    let mut schema = vec![table(
        "users",
        vec![col_with_index],
        vec![TableConstraint::Index {
            name: None,
            columns: vec!["email".into()],
        }],
    )];

    apply_action(
        &mut schema,
        &MigrationAction::RemoveConstraint {
            table: "users".into(),
            constraint: TableConstraint::Index {
                name: None,
                columns: vec!["email".into()],
            },
        },
    )
    .unwrap();

    // Constraint removed
    assert!(schema[0].constraints.is_empty());
    // Inline index cleared
    assert!(schema[0].columns[0].index.is_none());
}

mod more;
