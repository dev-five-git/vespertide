use super::*;

#[test]
fn validate_schema_rejects_duplicate_column_names() {
    let schema = vec![table(
        "users",
        vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            col("id", ColumnType::Simple(SimpleColumnType::Text)),
        ],
        vec![pk(vec!["id"])],
    )];

    let err = validate_schema(&schema).unwrap_err();

    assert!(matches!(err, PlannerError::TableValidation(_)));
    assert_eq!(
        err.to_string(),
        "table validation error: table 'users' has duplicate column name 'id'"
    );
}

#[test]
fn validate_schema_fk_ref_column_not_found() {
    let schema = vec![table(
        "users",
        vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
        vec![
            pk(vec!["id"]),
            TableConstraint::ForeignKey {
                name: Some("fk_bad".into()),
                columns: vec!["id".into()],
                ref_table: "users".into(),
                ref_columns: vec!["nonexistent".into()],
                on_delete: None,
                on_update: None,
                orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
            },
        ],
    )];

    let result = validate_schema(&schema);

    assert!(
        matches!(
            result,
            Err(PlannerError::ForeignKeyColumnNotFound(_, _, _, _))
        ),
        "FK pointing to non-existent column should trigger ForeignKeyColumnNotFound, got: {result:?}"
    );
}

#[test]
fn validate_schema_duplicate_enum_variant_name() {
    let schema = vec![table(
        "orders",
        vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            col(
                "status",
                ColumnType::Complex(ComplexColumnType::Enum {
                    name: "status_enum".into(),
                    values: EnumValues::String(vec!["active".into(), "active".into()]),
                }),
            ),
        ],
        vec![pk(vec!["id"])],
    )];

    let result = validate_schema(&schema);

    assert!(
        matches!(
            result,
            Err(PlannerError::DuplicateEnumVariantName(_, _, _, _))
        ),
        "duplicate enum variant should trigger DuplicateEnumVariantName, got: {result:?}"
    );
}

#[test]
fn validate_schema_rejects_numeric_scale_greater_than_precision() {
    let schema = vec![table(
        "prices",
        vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            col(
                "amount",
                ColumnType::Complex(ComplexColumnType::Numeric {
                    precision: 5,
                    scale: 10,
                }),
            ),
        ],
        vec![pk(vec!["id"])],
    )];

    let err = validate_schema(&schema).unwrap_err();

    assert!(matches!(err, PlannerError::TableValidation(_)));
    assert!(
        err.to_string()
            .contains("scale (10) must be <= precision (5)")
    );
}

#[test]
fn validate_schema_accepts_numeric_scale_equal_to_precision() {
    let schema = vec![table(
        "prices",
        vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            col(
                "amount",
                ColumnType::Complex(ComplexColumnType::Numeric {
                    precision: 5,
                    scale: 5,
                }),
            ),
        ],
        vec![pk(vec!["id"])],
    )];

    assert!(validate_schema(&schema).is_ok());
}

#[test]
fn validate_schema_rejects_integer_enum_values_outside_i32_range() {
    let table = table(
        "tasks",
        vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            col(
                "priority",
                ColumnType::Complex(ComplexColumnType::Enum {
                    name: "task_priority".into(),
                    values: EnumValues::Integer(vec![
                        NumValue {
                            name: "low".into(),
                            value: 0,
                        },
                        NumValue {
                            name: "too_large".into(),
                            value: 9_999_999_999,
                        },
                    ]),
                }),
            ),
        ],
        vec![pk(vec!["id"])],
    );

    let err = validate_schema(&[table]).unwrap_err();

    assert!(matches!(err, PlannerError::TableValidation(_)));
    assert!(
        err.to_string()
            .contains("integer enum value 9999999999 is outside i32 range")
    );
}

#[test]
fn validate_schema_accepts_table_level_primary_key_without_inline_pk() {
    let schema = vec![table(
        "users",
        vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
        vec![TableConstraint::PrimaryKey {
            auto_increment: false,
            columns: vec!["id".into()],
            strategy: vespertide_core::PrimaryKeyAdditionStrategy::default(),
        }],
    )];

    assert!(validate_schema(&schema).is_ok());
}

#[test]
fn validate_schema_rejects_nullable_primary_key_column() {
    // A column named in a table-level PRIMARY KEY must be NOT NULL: SQL
    // defines PRIMARY KEY as UNIQUE + NOT NULL, so a nullable PK column is
    // rejected up front (schema::validate_table PrimaryKeyColumnNullable arm).
    let schema = vec![table(
        "users",
        vec![col_nullable(
            "id",
            ColumnType::Simple(SimpleColumnType::Integer),
        )],
        vec![pk(vec!["id"])],
    )];

    let err = validate_schema(&schema).unwrap_err();

    assert!(
        matches!(
            &err,
            PlannerError::PrimaryKeyColumnNullable { table, column }
                if table == "users" && column == "id"
        ),
        "expected PrimaryKeyColumnNullable, got: {err:?}"
    );
}

#[test]
fn validate_schema_returns_bare_missing_primary_key_for_single_table() {
    let schema = vec![table(
        "users",
        vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
        vec![],
    )];

    let err = validate_schema(&schema).unwrap_err();

    assert!(matches!(err, PlannerError::MissingPrimaryKey(table) if table == "users"));
}

#[rstest]
#[case::valid_schema(
        vec![table(
            "users",
            vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
            vec![TableConstraint::PrimaryKey { auto_increment: false, columns: vec!["id".into()], strategy: vespertide_core::PrimaryKeyAdditionStrategy::default() }],
        )],
        None
    )]
#[case::duplicate_table(
        vec![
            table("users", vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))], vec![]),
            table("users", vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))], vec![]),
        ],
        Some(is_duplicate as fn(&PlannerError) -> bool)
    )]
#[case::fk_missing_table(
        vec![table(
            "users",
            vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
            vec![pk(vec!["id"]), TableConstraint::ForeignKey {
                name: None,
                columns: vec!["id".into()],
                ref_table: "nonexistent".into(),
                ref_columns: vec!["id".into()],
                on_delete: None,
                on_update: None,
                orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
            }],
        )],
        Some(is_fk_table as fn(&PlannerError) -> bool)
    )]
#[case::fk_missing_column(
        vec![
            table("posts", vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))], vec![pk(vec!["id"])]),
            table(
                "users",
                vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
                vec![pk(vec!["id"]), TableConstraint::ForeignKey {
                    name: None,
                    columns: vec!["id".into()],
                    ref_table: "posts".into(),
                    ref_columns: vec!["nonexistent".into()],
                    on_delete: None,
                    on_update: None,
                    orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
                }],
            ),
        ],
        Some(is_fk_column as fn(&PlannerError) -> bool)
    )]
#[case::fk_local_missing_column(
        vec![
            table("posts", vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))], vec![pk(vec!["id"])]),
            table(
                "users",
                vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
                vec![pk(vec!["id"]), TableConstraint::ForeignKey {
                    name: None,
                    columns: vec!["missing".into()],
                    ref_table: "posts".into(),
                    ref_columns: vec!["id".into()],
                    on_delete: None,
                    on_update: None,
                    orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
                }],
            ),
        ],
        Some(is_constraint_column as fn(&PlannerError) -> bool)
    )]
#[case::fk_valid(
        vec![
            table(
                "posts",
                vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
                vec![pk(vec!["id"])],
            ),
            table(
                "users",
                vec![col("id", ColumnType::Simple(SimpleColumnType::Integer)), col("post_id", ColumnType::Simple(SimpleColumnType::Integer))],
                vec![pk(vec!["id"]), TableConstraint::ForeignKey {
                    name: None,
                    columns: vec!["post_id".into()],
                    ref_table: "posts".into(),
                    ref_columns: vec!["id".into()],
                    on_delete: None,
                    on_update: None,
                    orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
                }],
            ),
        ],
        None
    )]
#[case::index_missing_column(
        vec![table(
            "users",
            vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
            vec![pk(vec!["id"]), idx("idx_name", vec!["nonexistent"])],
        )],
        Some(is_index_column as fn(&PlannerError) -> bool)
    )]
#[case::constraint_missing_column(
        vec![table(
            "users",
            vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
            vec![TableConstraint::PrimaryKey { auto_increment: false, columns: vec!["nonexistent".into()], strategy: vespertide_core::PrimaryKeyAdditionStrategy::default() }],
        )],
        Some(is_constraint_column as fn(&PlannerError) -> bool)
    )]
#[case::unique_empty_columns(
        vec![table(
            "users",
            vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
            vec![pk(vec!["id"]), TableConstraint::Unique {
                name: Some("u".into()),
                columns: vec![],
                strategy: vespertide_core::UniqueConstraintStrategy::DeleteDuplicates { keep: vespertide_core::KeepPolicy::First },
            }],
        )],
        Some(is_empty_columns as fn(&PlannerError) -> bool)
    )]
#[case::unique_missing_column(
        vec![table(
            "users",
            vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
            vec![pk(vec!["id"]), TableConstraint::Unique {
                name: None,
                columns: vec!["missing".into()],
                strategy: vespertide_core::UniqueConstraintStrategy::DeleteDuplicates { keep: vespertide_core::KeepPolicy::First },
            }],
        )],
        Some(is_constraint_column as fn(&PlannerError) -> bool)
    )]
#[case::empty_primary_key(
        vec![table(
            "users",
            vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
            vec![TableConstraint::PrimaryKey { auto_increment: false, columns: vec![], strategy: vespertide_core::PrimaryKeyAdditionStrategy::default() }],
        )],
        Some(is_empty_columns as fn(&PlannerError) -> bool)
    )]
#[case::fk_column_count_mismatch(
        vec![
            table(
                "posts",
                vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
                vec![pk(vec!["id"])],
            ),
            table(
                "users",
                vec![col("id", ColumnType::Simple(SimpleColumnType::Integer)), col("post_id", ColumnType::Simple(SimpleColumnType::Integer))],
                vec![pk(vec!["id"]), TableConstraint::ForeignKey {
                    name: None,
                    columns: vec!["id".into(), "post_id".into()],
                    ref_table: "posts".into(),
                    ref_columns: vec!["id".into()],
                    on_delete: None,
                    on_update: None,
                    orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
                }],
            ),
        ],
        Some(is_fk_column as fn(&PlannerError) -> bool)
    )]
#[case::fk_empty_columns(
        vec![table(
            "users",
            vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
            vec![pk(vec!["id"]), TableConstraint::ForeignKey {
                name: None,
                columns: vec![],
                ref_table: "posts".into(),
                ref_columns: vec!["id".into()],
                on_delete: None,
                on_update: None,
                orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
            }],
        )],
        Some(is_empty_columns as fn(&PlannerError) -> bool)
    )]
#[case::fk_empty_ref_columns(
        vec![
            table(
                "posts",
                vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
                vec![pk(vec!["id"])],
            ),
            table(
                "users",
                vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
                vec![pk(vec!["id"]), TableConstraint::ForeignKey {
                    name: None,
                    columns: vec!["id".into()],
                    ref_table: "posts".into(),
                    ref_columns: vec![],
                    on_delete: None,
                    on_update: None,
                    orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
                }],
            ),
        ],
        Some(is_empty_columns as fn(&PlannerError) -> bool)
    )]
#[case::index_empty_columns(
        vec![table(
            "users",
            vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
            vec![pk(vec!["id"]), TableConstraint::Index {
                name: Some("idx".into()),
                columns: vec![],
            }],
        )],
        Some(is_empty_columns as fn(&PlannerError) -> bool)
    )]
#[case::index_valid(
        vec![table(
            "users",
            vec![col("id", ColumnType::Simple(SimpleColumnType::Integer)), col("name", ColumnType::Simple(SimpleColumnType::Text))],
            vec![pk(vec!["id"]), idx("idx_name", vec!["name"])],
        )],
        None
    )]
#[case::check_constraint_ok(
        vec![table(
            "users",
            vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
            vec![pk(vec!["id"]), TableConstraint::Check {
                name: "ck".into(),
                expr: "id > 0".into(),
                strategy: vespertide_core::CheckViolationStrategy::default(),
            }],
        )],
        None
    )]
#[case::missing_primary_key(
        vec![table(
            "users",
            vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
            vec![],
        )],
        Some(is_missing_pk as fn(&PlannerError) -> bool)
    )]
fn validate_schema_cases(
    #[case] schema: Vec<TableDef>,
    #[case] expected_err: Option<fn(&PlannerError) -> bool>,
) {
    let result = validate_schema(&schema);
    match expected_err {
        None => assert!(result.is_ok()),
        Some(pred) => {
            let err = result.unwrap_err();
            assert!(matches_in_error(&err, pred), "unexpected error: {err:?}");
        }
    }
}

/// True if `pred` matches `err` itself, or — when `err` is a batched
/// [`PlannerError::Multiple`] — matches at least one of its nested errors.
///
/// Schema validation may emit several independent violations in one pass
/// (e.g. duplicate-table case yields both `DuplicateTableName` and
/// follow-on `MissingPrimaryKey`s from the affected tables). Tests that
/// assert "this specific variant must be present" stay meaningful as long
/// as the variant appears *somewhere* in the batch.
fn matches_in_error(err: &PlannerError, pred: fn(&PlannerError) -> bool) -> bool {
    if pred(err) {
        return true;
    }
    if let PlannerError::Multiple(inner) = err {
        return inner.0.iter().any(pred);
    }
    false
}

/// Batch-reporting contract for [`crate::validate::validate_schema`]:
/// a schema with multiple independent problems collapses into a single
/// [`PlannerError::Multiple`] that preserves every nested error in the
/// documented order (duplicate-name errors first, then per-table issues
/// in table-index order). The `Display` impl renders a numbered list so
/// CLI/loader callers surface every violation in one shot instead of
/// forcing the user to fix-and-rerun for each problem.
#[test]
fn validate_schema_batches_multiple_violations() {
    let schema = vec![
        // Table 0: missing PK → 1 violation.
        table(
            "users",
            vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
            vec![],
        ),
        // Table 1: FK target table missing → 1 violation.
        table(
            "posts",
            vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                col("author_id", ColumnType::Simple(SimpleColumnType::Integer)),
            ],
            vec![
                pk(vec!["id"]),
                TableConstraint::ForeignKey {
                    name: None,
                    columns: vec!["author_id".into()],
                    ref_table: "nonexistent".into(),
                    ref_columns: vec!["id".into()],
                    on_delete: None,
                    on_update: None,
                    orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
                },
            ],
        ),
    ];

    let err = validate_schema(&schema).unwrap_err();

    let PlannerError::Multiple(batch) = &err else {
        panic!("expected PlannerError::Multiple, got: {err:?}");
    };

    assert_eq!(
        batch.0.len(),
        2,
        "expected exactly 2 violations, got: {:?}",
        batch.0
    );
    assert!(
        batch.0.iter().any(is_missing_pk),
        "missing PK violation absent: {:?}",
        batch.0
    );
    assert!(
        batch.0.iter().any(is_fk_table),
        "FK target violation absent: {:?}",
        batch.0
    );

    // Display contract — numbered list with a fix-all footer.
    let rendered = format!("{err}");
    assert!(rendered.starts_with("2 validation violation(s):"));
    assert!(rendered.contains("\n  1. "));
    assert!(rendered.contains("\n  2. "));
    assert!(rendered.ends_with("Fix all of the above before re-running this command."));
}
