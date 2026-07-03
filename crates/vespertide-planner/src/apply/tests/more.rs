use super::*;

// Tests for CreateTable normalizing inline constraints
#[test]
fn create_table_normalizes_inline_unique() {
    let mut col_with_unique = col("email", ColumnType::Simple(SimpleColumnType::Text));
    col_with_unique.unique = Some(vespertide_core::StrOrBoolOrArray::Bool(true));

    let mut schema = vec![];
    apply_action(
        &mut schema,
        &MigrationAction::CreateTable {
            table: "users".into(),
            columns: vec![col_with_unique],
            constraints: vec![],
        },
    )
    .unwrap();

    // Inline unique: true should be normalized to a TableConstraint::Unique
    assert!(
        schema[0]
            .constraints
            .iter()
            .any(|c| matches!(c, TableConstraint::Unique { columns, .. } if columns == &["email"])),
        "Expected a Unique constraint on 'email', got: {:?}",
        schema[0].constraints
    );
}

#[test]
fn create_table_normalizes_inline_index() {
    let mut col_with_index = col("email", ColumnType::Simple(SimpleColumnType::Text));
    col_with_index.index = Some(vespertide_core::StrOrBoolOrArray::Bool(true));

    let mut schema = vec![];
    apply_action(
        &mut schema,
        &MigrationAction::CreateTable {
            table: "users".into(),
            columns: vec![col_with_index],
            constraints: vec![],
        },
    )
    .unwrap();

    // Inline index: true should be normalized to a TableConstraint::Index
    assert!(
        schema[0]
            .constraints
            .iter()
            .any(|c| matches!(c, TableConstraint::Index { columns, .. } if columns == &["email"])),
        "Expected an Index constraint on 'email', got: {:?}",
        schema[0].constraints
    );
}

#[test]
fn create_table_normalizes_inline_primary_key() {
    let mut col_with_pk = col("id", ColumnType::Simple(SimpleColumnType::Integer));
    col_with_pk.primary_key =
        Some(vespertide_core::schema::primary_key::PrimaryKeySyntax::Bool(true));

    let mut schema = vec![];
    apply_action(
        &mut schema,
        &MigrationAction::CreateTable {
            table: "users".into(),
            columns: vec![col_with_pk],
            constraints: vec![],
        },
    )
    .unwrap();

    assert!(
        schema[0].constraints.iter().any(
            |c| matches!(c, TableConstraint::PrimaryKey { columns, .. } if columns == &["id"])
        ),
        "Expected a PrimaryKey constraint on 'id', got: {:?}",
        schema[0].constraints
    );
}

// clear_inline_constraint_fields must clear the inline primary_key flag on
// the column NAMED by the constraint, not some other column. Two columns both
// carry an inline PK flag; removing the PK that names only "a" must clear "a"
// and leave "b" untouched. Pins the `&c.name == col_name` match (a `!=`
// mutant would clear the first NON-matching column instead).
#[test]
fn clear_inline_primary_key_targets_the_named_column() {
    use crate::apply::constraint_ops::clear_inline_constraint_fields;
    use vespertide_core::schema::primary_key::PrimaryKeySyntax;

    let mut a = col("a", ColumnType::Simple(SimpleColumnType::Integer));
    a.primary_key = Some(PrimaryKeySyntax::Bool(true));
    let mut b = col("b", ColumnType::Simple(SimpleColumnType::Integer));
    b.primary_key = Some(PrimaryKeySyntax::Bool(true));
    let mut tbl = table("t", vec![a, b], vec![]);

    clear_inline_constraint_fields(
        "t",
        &mut tbl,
        &TableConstraint::PrimaryKey {
            auto_increment: false,
            columns: vec!["a".into()],
            strategy: vespertide_core::PrimaryKeyAdditionStrategy::default(),
        },
    );

    assert!(
        tbl.columns[0].primary_key.is_none(),
        "named column `a` inline PK must be cleared"
    );
    assert!(
        tbl.columns[1].primary_key.is_some(),
        "unrelated column `b` inline PK must be left intact"
    );
}

// Tests for AddColumn normalizing inline constraints
#[test]
fn add_column_normalizes_inline_unique() {
    let mut schema = vec![table(
        "users",
        vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
        vec![],
    )];

    let mut col_with_unique = col("email", ColumnType::Simple(SimpleColumnType::Text));
    col_with_unique.unique = Some(vespertide_core::StrOrBoolOrArray::Bool(true));

    apply_action(
        &mut schema,
        &MigrationAction::AddColumn {
            table: "users".into(),
            column: Box::new(col_with_unique),
            fill_with: None,
        },
    )
    .unwrap();

    assert!(
        schema[0]
            .constraints
            .iter()
            .any(|c| matches!(c, TableConstraint::Unique { columns, .. } if columns == &["email"])),
        "Expected a Unique constraint on 'email' after AddColumn, got: {:?}",
        schema[0].constraints
    );
}

#[test]
fn add_column_normalizes_inline_index() {
    let mut schema = vec![table(
        "users",
        vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
        vec![],
    )];

    let mut col_with_index = col("email", ColumnType::Simple(SimpleColumnType::Text));
    col_with_index.index = Some(vespertide_core::StrOrBoolOrArray::Bool(true));

    apply_action(
        &mut schema,
        &MigrationAction::AddColumn {
            table: "users".into(),
            column: Box::new(col_with_index),
            fill_with: None,
        },
    )
    .unwrap();

    assert!(
        schema[0]
            .constraints
            .iter()
            .any(|c| matches!(c, TableConstraint::Index { columns, .. } if columns == &["email"])),
        "Expected an Index constraint on 'email' after AddColumn, got: {:?}",
        schema[0].constraints
    );
}

// Tests for ModifyColumnNullable
#[test]
fn apply_modify_column_nullable_success() {
    let mut schema = vec![table(
        "users",
        vec![col("email", ColumnType::Simple(SimpleColumnType::Text))],
        vec![],
    )];

    // Initially nullable: true (from col helper)
    assert!(schema[0].columns[0].nullable);

    apply_action(
        &mut schema,
        &MigrationAction::ModifyColumnNullable {
            table: "users".into(),
            column: "email".into(),
            nullable: false,
            fill_with: None,
            delete_null_rows: None,
        },
    )
    .unwrap();

    assert!(!schema[0].columns[0].nullable);
}

#[test]
fn apply_modify_column_nullable_table_not_found() {
    let mut schema = vec![];

    let err = apply_action(
        &mut schema,
        &MigrationAction::ModifyColumnNullable {
            table: "users".into(),
            column: "email".into(),
            nullable: false,
            fill_with: None,
            delete_null_rows: None,
        },
    )
    .unwrap_err();

    assert_err_kind(&err, ErrKind::TableNotFound);
}

#[test]
fn apply_modify_column_nullable_column_not_found() {
    let mut schema = vec![table(
        "users",
        vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
        vec![],
    )];

    let err = apply_action(
        &mut schema,
        &MigrationAction::ModifyColumnNullable {
            table: "users".into(),
            column: "email".into(),
            nullable: false,
            fill_with: None,
            delete_null_rows: None,
        },
    )
    .unwrap_err();

    assert_err_kind(&err, ErrKind::ColumnNotFound);
}

// Tests for ModifyColumnDefault
#[test]
fn apply_modify_column_default_set() {
    let mut schema = vec![table(
        "users",
        vec![col("status", ColumnType::Simple(SimpleColumnType::Text))],
        vec![],
    )];

    // Initially no default
    assert!(schema[0].columns[0].default.is_none());

    apply_action(
        &mut schema,
        &MigrationAction::ModifyColumnDefault {
            table: "users".into(),
            column: "status".into(),
            new_default: Some("'active'".into()),
            backfill: None,
        },
    )
    .unwrap();

    assert_eq!(
        schema[0].columns[0].default,
        Some(vespertide_core::StringOrBool::String("'active'".into()))
    );
}

#[test]
fn apply_modify_column_default_drop() {
    let mut col_with_default = col("status", ColumnType::Simple(SimpleColumnType::Text));
    col_with_default.default = Some(vespertide_core::StringOrBool::String("'active'".into()));

    let mut schema = vec![table("users", vec![col_with_default], vec![])];

    apply_action(
        &mut schema,
        &MigrationAction::ModifyColumnDefault {
            table: "users".into(),
            column: "status".into(),
            new_default: None,
            backfill: None,
        },
    )
    .unwrap();

    assert!(schema[0].columns[0].default.is_none());
}

#[test]
fn apply_modify_column_default_table_not_found() {
    let mut schema = vec![];

    let err = apply_action(
        &mut schema,
        &MigrationAction::ModifyColumnDefault {
            table: "users".into(),
            column: "status".into(),
            new_default: Some("'active'".into()),
            backfill: None,
        },
    )
    .unwrap_err();

    assert_err_kind(&err, ErrKind::TableNotFound);
}

#[test]
fn apply_modify_column_default_column_not_found() {
    let mut schema = vec![table(
        "users",
        vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
        vec![],
    )];

    let err = apply_action(
        &mut schema,
        &MigrationAction::ModifyColumnDefault {
            table: "users".into(),
            column: "status".into(),
            new_default: Some("'active'".into()),
            backfill: None,
        },
    )
    .unwrap_err();

    assert_err_kind(&err, ErrKind::ColumnNotFound);
}

// Tests for ModifyColumnComment
#[test]
fn apply_modify_column_comment_set() {
    let mut schema = vec![table(
        "users",
        vec![col("email", ColumnType::Simple(SimpleColumnType::Text))],
        vec![],
    )];

    // Initially no comment
    assert!(schema[0].columns[0].comment.is_none());

    apply_action(
        &mut schema,
        &MigrationAction::ModifyColumnComment {
            table: "users".into(),
            column: "email".into(),
            new_comment: Some("User email address".into()),
        },
    )
    .unwrap();

    assert_eq!(
        schema[0].columns[0].comment,
        Some("User email address".into())
    );
}

#[test]
fn apply_modify_column_comment_drop() {
    let mut col_with_comment = col("email", ColumnType::Simple(SimpleColumnType::Text));
    col_with_comment.comment = Some("User email address".into());

    let mut schema = vec![table("users", vec![col_with_comment], vec![])];

    apply_action(
        &mut schema,
        &MigrationAction::ModifyColumnComment {
            table: "users".into(),
            column: "email".into(),
            new_comment: None,
        },
    )
    .unwrap();

    assert!(schema[0].columns[0].comment.is_none());
}

#[test]
fn apply_modify_column_comment_table_not_found() {
    let mut schema = vec![];

    let err = apply_action(
        &mut schema,
        &MigrationAction::ModifyColumnComment {
            table: "users".into(),
            column: "email".into(),
            new_comment: Some("User email".into()),
        },
    )
    .unwrap_err();

    assert_err_kind(&err, ErrKind::TableNotFound);
}

#[test]
fn apply_modify_column_comment_column_not_found() {
    let mut schema = vec![table(
        "users",
        vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
        vec![],
    )];

    let err = apply_action(
        &mut schema,
        &MigrationAction::ModifyColumnComment {
            table: "users".into(),
            column: "email".into(),
            new_comment: Some("User email".into()),
        },
    )
    .unwrap_err();

    assert_err_kind(&err, ErrKind::ColumnNotFound);
}

#[test]
fn apply_replace_constraint_fk() {
    let mut schema = vec![table(
        "posts",
        vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            col("user_id", ColumnType::Simple(SimpleColumnType::Integer)),
        ],
        vec![TableConstraint::ForeignKey {
            name: Some("fk_user".into()),
            columns: vec!["user_id".into()],
            ref_table: "users".into(),
            ref_columns: vec!["id".into()],
            on_delete: None,
            on_update: None,
            orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
        }],
    )];

    let from = TableConstraint::ForeignKey {
        name: Some("fk_user".into()),
        columns: vec!["user_id".into()],
        ref_table: "users".into(),
        ref_columns: vec!["id".into()],
        on_delete: None,
        on_update: None,
        orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
    };
    let to = TableConstraint::ForeignKey {
        name: Some("fk_user".into()),
        columns: vec!["user_id".into()],
        ref_table: "users".into(),
        ref_columns: vec!["id".into()],
        on_delete: Some(vespertide_core::ReferenceAction::Cascade),
        on_update: None,
        orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
    };

    apply_action(
        &mut schema,
        &MigrationAction::ReplaceConstraint {
            table: "posts".into(),
            from,
            to: to.clone(),
        },
    )
    .unwrap();
    assert_eq!(schema[0].constraints.len(), 1);
    assert_eq!(schema[0].constraints[0], to);
}

#[test]
fn apply_replace_constraint_table_not_found() {
    let mut schema = vec![];
    let from = idx("ix_old", vec!["col"]);
    let to = idx("ix_new", vec!["col"]);
    let err = apply_action(
        &mut schema,
        &MigrationAction::ReplaceConstraint {
            table: "missing".into(),
            from,
            to,
        },
    )
    .unwrap_err();
    assert_err_kind(&err, ErrKind::TableNotFound);
}

#[test]
fn apply_replace_constraint_no_match_errors() {
    let existing = idx("ix_existing", vec!["col"]);
    let mut schema = vec![table(
        "users",
        vec![col("col", ColumnType::Simple(SimpleColumnType::Integer))],
        vec![existing.clone()],
    )];

    let from = idx("ix_nonexistent", vec!["other"]);
    let to = idx("ix_new", vec!["other"]);
    let err = apply_action(
        &mut schema,
        &MigrationAction::ReplaceConstraint {
            table: "users".into(),
            from,
            to,
        },
    )
    .unwrap_err();

    assert!(matches!(err, PlannerError::TableValidation(_)));
    assert_eq!(schema[0].constraints, vec![existing]);
}

/// L33 of apply/mod.rs: dispatch arm for
/// `MigrationAction::RemapEnumValues { table, column, mapping }`.
/// Existing `remap_enum_values_*` tests in apply/column_ops.rs hit
/// the helper directly; this test drives the public `apply_action`
/// match so the dispatch arm itself is exercised.
#[test]
fn apply_action_dispatches_remap_enum_values_arm() {
    use std::collections::BTreeMap;
    use vespertide_core::{ComplexColumnType, EnumValues, NumValue};

    let int_enum = ColumnType::Complex(ComplexColumnType::Enum {
        name: "priority".into(),
        values: EnumValues::Integer(vec![
            NumValue {
                name: "Low".into(),
                value: 0,
            },
            NumValue {
                name: "High".into(),
                value: 10,
            },
        ]),
    });
    let priority_col = ColumnDef {
        name: "priority".into(),
        r#type: int_enum,
        nullable: false,
        default: None,
        comment: None,
        primary_key: None,
        unique: None,
        index: None,
        foreign_key: None,
    };
    let mut schema = vec![table("orders", vec![priority_col], vec![])];

    let mut mapping: BTreeMap<i64, i64> = BTreeMap::new();
    mapping.insert(0, 5);
    mapping.insert(10, 50);

    let action = MigrationAction::RemapEnumValues {
        table: "orders".into(),
        column: "priority".into(),
        mapping,
    };
    apply_action(&mut schema, &action)
        .expect("RemapEnumValues dispatch arm returns Ok for an integer enum");

    // Schema column is still an integer enum with the same names but
    // remapped integer values — confirms the dispatch landed on the
    // RemapEnumValues path (other arms wouldn't touch enum values).
    let ColumnType::Complex(ComplexColumnType::Enum {
        values: EnumValues::Integer(ref new_values),
        ..
    }) = schema[0].columns[0].r#type
    else {
        panic!(
            "expected updated integer enum, got: {:?}",
            schema[0].columns[0].r#type
        );
    };
    let by_name: std::collections::HashMap<&str, i64> = new_values
        .iter()
        .map(|v| (v.name.as_str(), v.value))
        .collect();
    assert_eq!(by_name["Low"], 5);
    assert_eq!(by_name["High"], 50);
}
