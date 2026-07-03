use super::*;
use crate::schema::column::{ColumnType, SimpleColumnType};
use crate::schema::foreign_key::{ForeignKeyDef, ForeignKeySyntax};
use crate::schema::primary_key::PrimaryKeySyntax;
use crate::schema::reference::ReferenceAction;
use crate::schema::str_or_bool::StrOrBoolOrArray;

pub(super) fn col(name: &str, ty: ColumnType) -> ColumnDef {
    ColumnDef {
        name: name.into(),
        r#type: ty,
        nullable: true,
        default: None,
        comment: None,
        primary_key: None,
        unique: None,
        index: None,
        foreign_key: None,
    }
}

#[test]
fn validate_unique_column_names_rejects_duplicate_names() {
    let table = TableDef {
        name: "users".into(),
        description: None,
        columns: vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            col("id", ColumnType::Simple(SimpleColumnType::Text)),
        ],
        constraints: vec![],
    };

    let err = table.validate_unique_column_names().unwrap_err();

    assert_eq!(
        err,
        TableValidationError::DuplicateColumnName {
            table: "users".into(),
            column: "id".into()
        }
    );
    assert_eq!(
        err.to_string(),
        "table 'users' has duplicate column name 'id'"
    );
}

// Distinct column names must PASS. Pins the `!seen.insert(...)` negation: a
// `delete !` mutant would treat the first (successful) insert as a duplicate
// and reject every well-formed table.
#[test]
fn validate_unique_column_names_accepts_distinct_names() {
    let table = TableDef {
        name: "users".into(),
        description: None,
        columns: vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            col("email", ColumnType::Simple(SimpleColumnType::Text)),
        ],
        constraints: vec![],
    };

    assert!(table.validate_unique_column_names().is_ok());
}

#[test]
fn normalize_inline_primary_key() {
    let mut id_col = col("id", ColumnType::Simple(SimpleColumnType::Integer));
    id_col.primary_key = Some(PrimaryKeySyntax::Bool(true));

    let table = TableDef {
        name: "users".into(),
        description: None,
        columns: vec![
            id_col,
            col("name", ColumnType::Simple(SimpleColumnType::Text)),
        ],
        constraints: vec![],
    };

    let normalized = table.normalize().unwrap();
    assert_eq!(normalized.constraints.len(), 1);
    assert!(matches!(
        &normalized.constraints[0],
        TableConstraint::PrimaryKey { columns, .. } if columns == &["id".to_string()]
    ));
}

#[test]
fn normalize_multiple_inline_primary_keys() {
    let mut id_col = col("id", ColumnType::Simple(SimpleColumnType::Integer));
    id_col.primary_key = Some(PrimaryKeySyntax::Bool(true));

    let mut tenant_col = col("tenant_id", ColumnType::Simple(SimpleColumnType::Integer));
    tenant_col.primary_key = Some(PrimaryKeySyntax::Bool(true));

    let table = TableDef {
        name: "users".into(),
        description: None,
        columns: vec![id_col, tenant_col],
        constraints: vec![],
    };

    let normalized = table.normalize().unwrap();
    assert_eq!(normalized.constraints.len(), 1);
    assert!(matches!(
        &normalized.constraints[0],
        TableConstraint::PrimaryKey { columns, .. } if columns == &["id".to_string(), "tenant_id".to_string()]
    ));
}

#[test]
fn normalize_does_not_duplicate_existing_pk() {
    let mut id_col = col("id", ColumnType::Simple(SimpleColumnType::Integer));
    id_col.primary_key = Some(PrimaryKeySyntax::Bool(true));

    let table = TableDef {
        name: "users".into(),
        description: None,
        columns: vec![id_col],
        constraints: vec![TableConstraint::PrimaryKey {
            auto_increment: false,
            columns: vec!["id".into()],
            strategy: crate::PrimaryKeyAdditionStrategy::default(),
        }],
    };

    let normalized = table.normalize().unwrap();
    assert_eq!(normalized.constraints.len(), 1);
}

#[test]
fn normalize_ignores_primary_key_false() {
    let mut id_col = col("id", ColumnType::Simple(SimpleColumnType::Integer));
    id_col.primary_key = Some(PrimaryKeySyntax::Bool(false));

    let table = TableDef {
        name: "users".into(),
        description: None,
        columns: vec![
            id_col,
            col("name", ColumnType::Simple(SimpleColumnType::Text)),
        ],
        constraints: vec![],
    };

    let normalized = table.normalize().unwrap();
    // primary_key: false should be ignored, so no primary key constraint should be added
    assert_eq!(normalized.constraints.len(), 0);
}

#[test]
fn normalize_inline_unique_bool() {
    let mut email_col = col("email", ColumnType::Simple(SimpleColumnType::Text));
    email_col.unique = Some(StrOrBoolOrArray::Bool(true));

    let table = TableDef {
        name: "users".into(),
        description: None,
        columns: vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            email_col,
        ],
        constraints: vec![],
    };

    let normalized = table.normalize().unwrap();
    assert_eq!(normalized.constraints.len(), 1);
    assert!(matches!(
        &normalized.constraints[0],
        TableConstraint::Unique { name: None, columns, .. } if columns == &["email".to_string()]
    ));
}

#[test]
fn normalize_inline_unique_with_name() {
    let mut email_col = col("email", ColumnType::Simple(SimpleColumnType::Text));
    email_col.unique = Some(StrOrBoolOrArray::Str("uq_users_email".into()));

    let table = TableDef {
        name: "users".into(),
        description: None,
        columns: vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            email_col,
        ],
        constraints: vec![],
    };

    let normalized = table.normalize().unwrap();
    assert_eq!(normalized.constraints.len(), 1);
    assert!(matches!(
        &normalized.constraints[0],
        TableConstraint::Unique { name: Some(n), columns, .. }
            if n == "uq_users_email" && columns == &["email".to_string()]
    ));
}

#[test]
fn normalize_composite_unique_from_string_name() {
    // Test that multiple columns with the same unique constraint name
    // are grouped into a single composite unique constraint
    let mut route_col = col("join_route", ColumnType::Simple(SimpleColumnType::Text));
    route_col.unique = Some(StrOrBoolOrArray::Str("route_provider_id".into()));

    let mut provider_col = col("provider_id", ColumnType::Simple(SimpleColumnType::Text));
    provider_col.unique = Some(StrOrBoolOrArray::Str("route_provider_id".into()));

    let table = TableDef {
        name: "user".into(),
        description: None,
        columns: vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            route_col,
            provider_col,
        ],
        constraints: vec![],
    };

    let normalized = table.normalize().unwrap();
    assert_eq!(normalized.constraints.len(), 1);
    assert!(matches!(
        &normalized.constraints[0],
        TableConstraint::Unique { name: Some(n), columns, .. }
            if n == "route_provider_id"
                && columns == &["join_route".to_string(), "provider_id".to_string()]
    ));
}

#[test]
fn normalize_unique_name_mismatch_creates_both_constraints() {
    // Test coverage for line 181: When an inline unique has a name but existing doesn't (or vice versa),
    // they should not match and both constraints should be created
    let mut email_col = col("email", ColumnType::Simple(SimpleColumnType::Text));
    email_col.unique = Some(StrOrBoolOrArray::Str("named_unique".into()));

    let table = TableDef {
        name: "user".into(),
        description: None,
        columns: vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            email_col,
        ],
        constraints: vec![
            // Existing unnamed unique constraint on same column
            TableConstraint::Unique {
                name: None,
                columns: vec!["email".into()],
                strategy: crate::schema::UniqueConstraintStrategy::DeleteDuplicates {
                    keep: crate::schema::KeepPolicy::First,
                },
            },
        ],
    };

    let normalized = table.normalize().unwrap();

    // Should have 2 unique constraints: one named, one unnamed
    let unique_constraints: Vec<_> = normalized
        .constraints
        .iter()
        .filter(|c| matches!(c, TableConstraint::Unique { .. }))
        .collect();
    assert_eq!(
        unique_constraints.len(),
        2,
        "Should keep both named and unnamed unique constraints as they don't match"
    );

    // Verify we have one named and one unnamed
    let has_named = unique_constraints
        .iter()
        .any(|c| matches!(c, TableConstraint::Unique { name: Some(n), .. } if n == "named_unique"));
    let has_unnamed = unique_constraints
        .iter()
        .any(|c| matches!(c, TableConstraint::Unique { name: None, .. }));

    assert!(has_named, "Should have named unique constraint");
    assert!(has_unnamed, "Should have unnamed unique constraint");
}

#[test]
fn normalize_inline_index_bool() {
    let mut name_col = col("name", ColumnType::Simple(SimpleColumnType::Text));
    name_col.index = Some(StrOrBoolOrArray::Bool(true));

    let table = TableDef {
        name: "users".into(),
        description: None,
        columns: vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            name_col,
        ],
        constraints: vec![],
    };

    let normalized = table.normalize().unwrap();
    // Count Index constraints
    let indexes: Vec<_> = normalized
        .constraints
        .iter()
        .filter(|c| matches!(c, TableConstraint::Index { .. }))
        .collect();
    assert_eq!(indexes.len(), 1);
    // Auto-generated indexes (from index: true) should have name: None
    // SQL generation will create the actual name based on naming conventions
    assert!(matches!(
        indexes[0],
        TableConstraint::Index { name: None, columns }
            if columns == &["name".to_string()]
    ));
}

#[test]
fn normalize_inline_index_with_name() {
    let mut name_col = col("name", ColumnType::Simple(SimpleColumnType::Text));
    name_col.index = Some(StrOrBoolOrArray::Str("custom_idx_name".into()));

    let table = TableDef {
        name: "users".into(),
        description: None,
        columns: vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            name_col,
        ],
        constraints: vec![],
    };

    let normalized = table.normalize().unwrap();
    let indexes: Vec<_> = normalized
        .constraints
        .iter()
        .filter(|c| matches!(c, TableConstraint::Index { .. }))
        .collect();
    assert_eq!(indexes.len(), 1);
    assert!(matches!(
        indexes[0],
        TableConstraint::Index { name: Some(n), .. }
            if n == "custom_idx_name"
    ));
}

#[test]
fn normalize_inline_foreign_key() {
    let mut user_id_col = col("user_id", ColumnType::Simple(SimpleColumnType::Integer));
    user_id_col.foreign_key = Some(ForeignKeySyntax::Object(ForeignKeyDef {
        ref_table: "users".into(),
        ref_columns: vec!["id".into()],
        on_delete: Some(ReferenceAction::Cascade),
        on_update: None,
        orphan_strategy: crate::ForeignKeyOrphanStrategy::default(),
    }));

    let table = TableDef {
        name: "posts".into(),
        description: None,
        columns: vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            user_id_col,
        ],
        constraints: vec![],
    };

    let normalized = table.normalize().unwrap();
    assert_eq!(normalized.constraints.len(), 1);
    assert!(matches!(
        &normalized.constraints[0],
        TableConstraint::ForeignKey {
            name: None,
            columns,
            ref_table,
            ref_columns,
            on_delete: Some(ReferenceAction::Cascade),
            on_update: None,
            ..
        } if columns == &["user_id".to_string()]
            && ref_table == "users"
            && ref_columns == &["id".to_string()]
    ));
}

// foreign_key_constraint_exists looks for a SINGLE-column table-level FK on the
// inline FK's column. A pre-existing COMPOSITE (2-column) FK whose first column
// is the same must NOT count as a match, so the inline single-column FK is
// still materialized. Pins the `columns.len() == 1 &&` guard: a `||` mutant
// would treat the composite FK as the match and drop the inline FK.
#[test]
fn normalize_keeps_inline_fk_despite_composite_fk_on_same_first_column() {
    let mut a_col = col("a", ColumnType::Simple(SimpleColumnType::Integer));
    a_col.foreign_key = Some(ForeignKeySyntax::Object(ForeignKeyDef {
        ref_table: "ref".into(),
        ref_columns: vec!["id".into()],
        on_delete: None,
        on_update: None,
        orphan_strategy: crate::ForeignKeyOrphanStrategy::default(),
    }));

    let table = TableDef {
        name: "t".into(),
        description: None,
        columns: vec![
            a_col,
            col("b", ColumnType::Simple(SimpleColumnType::Integer)),
        ],
        constraints: vec![TableConstraint::ForeignKey {
            name: Some("fk_composite".into()),
            columns: vec!["a".into(), "b".into()],
            ref_table: "other".into(),
            ref_columns: vec!["x".into(), "y".into()],
            on_delete: None,
            on_update: None,
            orphan_strategy: crate::ForeignKeyOrphanStrategy::default(),
        }],
    };

    let normalized = table.normalize().unwrap();
    let has_single_col_fk = normalized.constraints.iter().any(|c| {
        matches!(c, TableConstraint::ForeignKey { columns, .. }
            if columns.len() == 1 && columns[0] == "a")
    });
    assert!(
        has_single_col_fk,
        "inline single-column FK on `a` must be materialized: {:?}",
        normalized.constraints
    );
}

// index_constraint_exists dedups an inline NAMED index against a pre-existing
// table-level Index of the SAME name. Pins the `(Some, Some) => n1 == n2` arm:
// deleting the arm (-> `_ => false`) or flipping `==` to `!=` fails to detect
// the duplicate and emits a SECOND index of the same name.
#[test]
fn normalize_dedups_inline_named_index_against_existing_named_index() {
    let mut name_col = col("name", ColumnType::Simple(SimpleColumnType::Text));
    name_col.index = Some(StrOrBoolOrArray::Str("ix_name".into()));

    let table = TableDef {
        name: "users".into(),
        description: None,
        columns: vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            name_col,
        ],
        constraints: vec![TableConstraint::Index {
            name: Some("ix_name".into()),
            columns: vec!["name".into()],
        }],
    };

    let normalized = table.normalize().unwrap();
    let ix_name_count = normalized
        .constraints
        .iter()
        .filter(|c| matches!(c, TableConstraint::Index { name: Some(n), .. } if n == "ix_name"))
        .count();
    assert_eq!(
        ix_name_count, 1,
        "duplicate named index must be deduped: {:?}",
        normalized.constraints
    );
}

// An UNNAMED inline index (`index: true` -> auto name -> constraint_name None)
// is deduped against an existing UNNAMED table-level index on the SAME columns.
// Pins the `(None, None) => cols.as_slice() == columns` arm: a `!=` mutant
// fails to detect the duplicate and emits a second identical index.
#[test]
fn normalize_dedups_inline_unnamed_index_against_existing_unnamed_index() {
    let mut x_col = col("x", ColumnType::Simple(SimpleColumnType::Integer));
    x_col.index = Some(StrOrBoolOrArray::Bool(true));

    let table = TableDef {
        name: "t".into(),
        description: None,
        columns: vec![x_col],
        constraints: vec![TableConstraint::Index {
            name: None,
            columns: vec!["x".into()],
        }],
    };

    let normalized = table.normalize().unwrap();
    let unnamed_x_index_count = normalized
        .constraints
        .iter()
        .filter(|c| {
            matches!(c, TableConstraint::Index { name: None, columns }
                if columns.len() == 1 && columns[0] == "x")
        })
        .count();
    assert_eq!(
        unnamed_x_index_count, 1,
        "duplicate unnamed index on [x] must be deduped: {:?}",
        normalized.constraints
    );
}

#[test]
fn normalize_all_inline_constraints() {
    let mut id_col = col("id", ColumnType::Simple(SimpleColumnType::Integer));
    id_col.primary_key = Some(PrimaryKeySyntax::Bool(true));

    let mut email_col = col("email", ColumnType::Simple(SimpleColumnType::Text));
    email_col.unique = Some(StrOrBoolOrArray::Bool(true));

    let mut name_col = col("name", ColumnType::Simple(SimpleColumnType::Text));
    name_col.index = Some(StrOrBoolOrArray::Bool(true));

    let mut user_id_col = col("org_id", ColumnType::Simple(SimpleColumnType::Integer));
    user_id_col.foreign_key = Some(ForeignKeySyntax::Object(ForeignKeyDef {
        ref_table: "orgs".into(),
        ref_columns: vec!["id".into()],
        on_delete: None,
        on_update: None,
        orphan_strategy: crate::ForeignKeyOrphanStrategy::default(),
    }));

    let table = TableDef {
        name: "users".into(),
        description: None,
        columns: vec![id_col, email_col, name_col, user_id_col],
        constraints: vec![],
    };

    let normalized = table.normalize().unwrap();
    // Should have: PrimaryKey, Unique, ForeignKey, Index
    // Count non-Index constraints
    let non_index_constraints: Vec<_> = normalized
        .constraints
        .iter()
        .filter(|c| !matches!(c, TableConstraint::Index { .. }))
        .collect();
    assert_eq!(non_index_constraints.len(), 3);
    // Should have: 1 index
    let indexes: Vec<_> = normalized
        .constraints
        .iter()
        .filter(|c| matches!(c, TableConstraint::Index { .. }))
        .collect();
    assert_eq!(indexes.len(), 1);
}

#[test]
fn normalize_composite_index_from_string_name() {
    let mut updated_at_col = col(
        "updated_at",
        ColumnType::Simple(SimpleColumnType::Timestamp),
    );
    updated_at_col.index = Some(StrOrBoolOrArray::Str("tuple".into()));

    let mut user_id_col = col("user_id", ColumnType::Simple(SimpleColumnType::Integer));
    user_id_col.index = Some(StrOrBoolOrArray::Str("tuple".into()));

    let table = TableDef {
        name: "post".into(),
        description: None,
        columns: vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            updated_at_col,
            user_id_col,
        ],
        constraints: vec![],
    };

    let normalized = table.normalize().unwrap();
    let indexes: Vec<_> = normalized
        .constraints
        .iter()
        .filter_map(|c| {
            if let TableConstraint::Index { name, columns } = c {
                Some((name.clone(), columns.clone()))
            } else {
                None
            }
        })
        .collect();
    assert_eq!(indexes.len(), 1);
    assert_eq!(indexes[0].0, Some("tuple".to_string()));
    assert_eq!(
        indexes[0].1,
        vec!["updated_at".to_string(), "user_id".to_string()]
    );
}

#[test]
fn normalize_multiple_different_indexes() {
    let mut col1 = col("col1", ColumnType::Simple(SimpleColumnType::Text));
    col1.index = Some(StrOrBoolOrArray::Str("idx_a".into()));

    let mut col2 = col("col2", ColumnType::Simple(SimpleColumnType::Text));
    col2.index = Some(StrOrBoolOrArray::Str("idx_a".into()));

    let mut col3 = col("col3", ColumnType::Simple(SimpleColumnType::Text));
    col3.index = Some(StrOrBoolOrArray::Str("idx_b".into()));

    let mut col4 = col("col4", ColumnType::Simple(SimpleColumnType::Text));
    col4.index = Some(StrOrBoolOrArray::Bool(true));

    let table = TableDef {
        name: "test".into(),
        description: None,
        columns: vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            col1,
            col2,
            col3,
            col4,
        ],
        constraints: vec![],
    };

    let normalized = table.normalize().unwrap();
    let indexes: Vec<_> = normalized
        .constraints
        .iter()
        .filter_map(|c| {
            if let TableConstraint::Index { name, columns } = c {
                Some((name.clone(), columns.clone()))
            } else {
                None
            }
        })
        .collect();
    assert_eq!(indexes.len(), 3);

    // Check idx_a composite index
    let idx_a = indexes
        .iter()
        .find(|(n, _)| n == &Some("idx_a".to_string()))
        .unwrap();
    assert_eq!(idx_a.1, vec!["col1".to_string(), "col2".to_string()]);

    // Check idx_b single column index
    let idx_b = indexes
        .iter()
        .find(|(n, _)| n == &Some("idx_b".to_string()))
        .unwrap();
    assert_eq!(idx_b.1, vec!["col3".to_string()]);

    // Check auto-generated index for col4 (should have name: None)
    let idx_col4 = indexes.iter().find(|(n, _)| n.is_none()).unwrap();
    assert_eq!(idx_col4.1, vec!["col4".to_string()]);
}

#[test]
fn normalize_false_values_are_ignored() {
    let mut email_col = col("email", ColumnType::Simple(SimpleColumnType::Text));
    email_col.unique = Some(StrOrBoolOrArray::Bool(false));
    email_col.index = Some(StrOrBoolOrArray::Bool(false));

    let table = TableDef {
        name: "users".into(),
        description: None,
        columns: vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            email_col,
        ],
        constraints: vec![],
    };

    let normalized = table.normalize().unwrap();
    assert_eq!(normalized.constraints.len(), 0);
}

#[test]
fn normalize_table_without_primary_key() {
    // Test normalize with a table that has no primary key columns
    // This should cover lines 67-69, 72-73, and 93 (pk_columns.is_empty() branch)
    let table = TableDef {
        name: "users".into(),
        description: None,
        columns: vec![
            col("name", ColumnType::Simple(SimpleColumnType::Text)),
            col("email", ColumnType::Simple(SimpleColumnType::Text)),
        ],
        constraints: vec![],
    };

    let normalized = table.normalize().unwrap();
    // Should not add any primary key constraint
    assert_eq!(normalized.constraints.len(), 0);
}

#[test]
fn normalize_multiple_indexes_from_same_array() {
    // Multiple columns with same array of index names should create multiple composite indexes
    let mut updated_at_col = col(
        "updated_at",
        ColumnType::Simple(SimpleColumnType::Timestamp),
    );
    updated_at_col.index = Some(StrOrBoolOrArray::Array(vec![
        "tuple".into(),
        "tuple2".into(),
    ]));

    let mut user_id_col = col("user_id", ColumnType::Simple(SimpleColumnType::Integer));
    user_id_col.index = Some(StrOrBoolOrArray::Array(vec![
        "tuple".into(),
        "tuple2".into(),
    ]));

    let table = TableDef {
        name: "post".into(),
        description: None,
        columns: vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            updated_at_col,
            user_id_col,
        ],
        constraints: vec![],
    };

    let normalized = table.normalize().unwrap();
    // Should have: tuple (composite: updated_at, user_id), tuple2 (composite: updated_at, user_id)
    let indexes: Vec<_> = normalized
        .constraints
        .iter()
        .filter_map(|c| {
            if let TableConstraint::Index { name, columns } = c {
                Some((name.clone(), columns.clone()))
            } else {
                None
            }
        })
        .collect();
    assert_eq!(indexes.len(), 2);

    let tuple_idx = indexes
        .iter()
        .find(|(n, _)| n == &Some("tuple".to_string()))
        .unwrap();
    let mut sorted_cols = tuple_idx.1.clone();
    sorted_cols.sort();
    assert_eq!(
        sorted_cols,
        vec!["updated_at".to_string(), "user_id".to_string()]
    );

    let tuple2_idx = indexes
        .iter()
        .find(|(n, _)| n == &Some("tuple2".to_string()))
        .unwrap();
    let mut sorted_cols2 = tuple2_idx.1.clone();
    sorted_cols2.sort();
    assert_eq!(
        sorted_cols2,
        vec!["updated_at".to_string(), "user_id".to_string()]
    );
}

#[test]
fn normalize_inline_unique_with_array_existing_constraint() {
    // Test Array format where constraint already exists - should add column to existing
    let mut col1 = col("col1", ColumnType::Simple(SimpleColumnType::Text));
    col1.unique = Some(StrOrBoolOrArray::Array(vec!["uq_group".into()]));

    let mut col2 = col("col2", ColumnType::Simple(SimpleColumnType::Text));
    col2.unique = Some(StrOrBoolOrArray::Array(vec!["uq_group".into()]));

    let table = TableDef {
        name: "test".into(),
        description: None,
        columns: vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            col1,
            col2,
        ],
        constraints: vec![],
    };

    let normalized = table.normalize().unwrap();
    assert_eq!(normalized.constraints.len(), 1);
    let unique_constraint = &normalized.constraints[0];
    assert!(matches!(
        unique_constraint,
        TableConstraint::Unique { name: Some(n), columns: _, .. }
            if n == "uq_group"
    ));
    if let TableConstraint::Unique { columns, .. } = unique_constraint {
        let mut sorted_cols = columns.clone();
        sorted_cols.sort();
        assert_eq!(sorted_cols, vec!["col1".to_string(), "col2".to_string()]);
    }
}

#[test]
fn normalize_inline_unique_with_array_column_already_in_constraint() {
    // Test Array format where column is already in constraint - should not duplicate
    let mut col1 = col("col1", ColumnType::Simple(SimpleColumnType::Text));
    col1.unique = Some(StrOrBoolOrArray::Array(vec!["uq_group".into()]));

    let table = TableDef {
        name: "test".into(),
        description: None,
        columns: vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            col1.clone(),
        ],
        constraints: vec![],
    };

    let normalized1 = table.normalize().unwrap();
    assert_eq!(normalized1.constraints.len(), 1);

    // Add same column again - should not create duplicate
    let table2 = TableDef {
        name: "test".into(),
        description: None,
        columns: vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            col1,
        ],
        constraints: normalized1.constraints.clone(),
    };

    let normalized2 = table2.normalize().unwrap();
    assert_eq!(normalized2.constraints.len(), 1);
    if let TableConstraint::Unique { columns, .. } = &normalized2.constraints[0] {
        assert_eq!(columns.len(), 1);
        assert_eq!(columns[0], "col1");
    }
}

#[test]
fn normalize_inline_unique_str_already_exists() {
    // Test that existing unique constraint with same name and column is not duplicated
    let mut email_col = col("email", ColumnType::Simple(SimpleColumnType::Text));
    email_col.unique = Some(StrOrBoolOrArray::Str("uq_email".into()));

    let table = TableDef {
        name: "users".into(),
        description: None,
        columns: vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            email_col,
        ],
        constraints: vec![TableConstraint::Unique {
            name: Some("uq_email".into()),
            columns: vec!["email".into()],
            strategy: crate::schema::UniqueConstraintStrategy::DeleteDuplicates {
                keep: crate::schema::KeepPolicy::First,
            },
        }],
    };

    let normalized = table.normalize().unwrap();
    // Should not duplicate the constraint
    let unique_constraints: Vec<_> = normalized
        .constraints
        .iter()
        .filter(|c| matches!(c, TableConstraint::Unique { .. }))
        .collect();
    assert_eq!(unique_constraints.len(), 1);
}

#[test]
fn normalize_inline_unique_bool_already_exists() {
    // Test that existing unnamed unique constraint with same column is not duplicated
    let mut email_col = col("email", ColumnType::Simple(SimpleColumnType::Text));
    email_col.unique = Some(StrOrBoolOrArray::Bool(true));

    let table = TableDef {
        name: "users".into(),
        description: None,
        columns: vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            email_col,
        ],
        constraints: vec![TableConstraint::Unique {
            name: None,
            columns: vec!["email".into()],
            strategy: crate::schema::UniqueConstraintStrategy::DeleteDuplicates {
                keep: crate::schema::KeepPolicy::First,
            },
        }],
    };

    let normalized = table.normalize().unwrap();
    // Should not duplicate the constraint
    let unique_constraints: Vec<_> = normalized
        .constraints
        .iter()
        .filter(|c| matches!(c, TableConstraint::Unique { .. }))
        .collect();
    assert_eq!(unique_constraints.len(), 1);
}

#[test]
fn normalize_inline_foreign_key_already_exists() {
    // Test that existing foreign key constraint is not duplicated
    let mut user_id_col = col("user_id", ColumnType::Simple(SimpleColumnType::Integer));
    user_id_col.foreign_key = Some(ForeignKeySyntax::Object(ForeignKeyDef {
        ref_table: "users".into(),
        ref_columns: vec!["id".into()],
        on_delete: None,
        on_update: None,
        orphan_strategy: crate::ForeignKeyOrphanStrategy::default(),
    }));

    let table = TableDef {
        name: "posts".into(),
        description: None,
        columns: vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            user_id_col,
        ],
        constraints: vec![TableConstraint::ForeignKey {
            name: None,
            columns: vec!["user_id".into()],
            ref_table: "users".into(),
            ref_columns: vec!["id".into()],
            on_delete: None,
            on_update: None,
            orphan_strategy: crate::ForeignKeyOrphanStrategy::default(),
        }],
    };

    let normalized = table.normalize().unwrap();
    // Should not duplicate the foreign key
    let fk_constraints: Vec<_> = normalized
        .constraints
        .iter()
        .filter(|c| matches!(c, TableConstraint::ForeignKey { .. }))
        .collect();
    assert_eq!(fk_constraints.len(), 1);
}

#[test]
fn normalize_duplicate_index_same_column_str() {
    // Same index name applied to the same column multiple times should error
    // This tests inline index duplicate, not table-level index
    let mut col1 = col("col1", ColumnType::Simple(SimpleColumnType::Text));
    col1.index = Some(StrOrBoolOrArray::Str("idx1".into()));

    let table = TableDef {
        name: "test".into(),
        description: None,
        columns: vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            col1.clone(),
            {
                // Same column with same index name again
                let mut c = col1.clone();
                c.index = Some(StrOrBoolOrArray::Str("idx1".into()));
                c
            },
        ],
        constraints: vec![],
    };

    let result = table.normalize();
    assert!(result.is_err());
    if let Err(TableValidationError::DuplicateIndexColumn {
        index_name,
        column_name,
    }) = result
    {
        assert_eq!(index_name, "idx1");
        assert_eq!(column_name, "col1");
    } else {
        panic!("Expected DuplicateIndexColumn error");
    }
}

#[test]
fn normalize_duplicate_index_same_column_array() {
    // Same index name in array applied to the same column should error
    let mut col1 = col("col1", ColumnType::Simple(SimpleColumnType::Text));
    col1.index = Some(StrOrBoolOrArray::Array(vec!["idx1".into(), "idx1".into()]));

    let table = TableDef {
        name: "test".into(),
        description: None,
        columns: vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            col1,
        ],
        constraints: vec![],
    };

    let result = table.normalize();
    assert!(result.is_err());
    if let Err(TableValidationError::DuplicateIndexColumn {
        index_name,
        column_name,
    }) = result
    {
        assert_eq!(index_name, "idx1");
        assert_eq!(column_name, "col1");
    } else {
        panic!("Expected DuplicateIndexColumn error");
    }
}

#[test]
fn normalize_duplicate_index_same_column_multiple_definitions() {
    // Same index name applied to the same column in different ways should error
    let mut col1 = col("col1", ColumnType::Simple(SimpleColumnType::Text));
    col1.index = Some(StrOrBoolOrArray::Str("idx1".into()));

    let table = TableDef {
        name: "test".into(),
        description: None,
        columns: vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            col1.clone(),
            {
                let mut c = col1.clone();
                c.index = Some(StrOrBoolOrArray::Array(vec!["idx1".into()]));
                c
            },
        ],
        constraints: vec![],
    };

    let result = table.normalize();
    assert!(result.is_err());
    if let Err(TableValidationError::DuplicateIndexColumn {
        index_name,
        column_name,
    }) = result
    {
        assert_eq!(index_name, "idx1");
        assert_eq!(column_name, "col1");
    } else {
        panic!("Expected DuplicateIndexColumn error");
    }
}

#[test]
fn test_table_validation_error_display() {
    let error = TableValidationError::DuplicateIndexColumn {
        index_name: "idx_test".into(),
        column_name: "col1".into(),
    };
    let error_msg = format!("{error}");
    assert!(error_msg.contains("idx_test"));
    assert!(error_msg.contains("col1"));
    assert!(error_msg.contains("Duplicate index"));
}

#[test]
fn normalize_inline_unique_str_with_different_constraint_type() {
    // Test that other constraint types don't match in the exists check
    let mut email_col = col("email", ColumnType::Simple(SimpleColumnType::Text));
    email_col.unique = Some(StrOrBoolOrArray::Str("uq_email".into()));

    let table = TableDef {
        name: "users".into(),
        description: None,
        columns: vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            email_col,
        ],
        constraints: vec![
            // Add a PrimaryKey constraint (different type) - should not match
            TableConstraint::PrimaryKey {
                auto_increment: false,
                columns: vec!["id".into()],
                strategy: crate::PrimaryKeyAdditionStrategy::default(),
            },
        ],
    };

    let normalized = table.normalize().unwrap();
    // Should have: PrimaryKey (existing) + Unique (new)
    assert_eq!(normalized.constraints.len(), 2);
}

#[test]
fn normalize_inline_unique_array_with_different_constraint_type() {
    // Test that other constraint types don't match in the exists check for Array case
    let mut col1 = col("col1", ColumnType::Simple(SimpleColumnType::Text));
    col1.unique = Some(StrOrBoolOrArray::Array(vec!["uq_group".into()]));

    let table = TableDef {
        name: "test".into(),
        description: None,
        columns: vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            col1,
        ],
        constraints: vec![
            // Add a PrimaryKey constraint (different type) - should not match
            TableConstraint::PrimaryKey {
                auto_increment: false,
                columns: vec!["id".into()],
                strategy: crate::PrimaryKeyAdditionStrategy::default(),
            },
        ],
    };

    let normalized = table.normalize().unwrap();
    // Should have: PrimaryKey (existing) + Unique (new)
    assert_eq!(normalized.constraints.len(), 2);
}

#[test]
fn normalize_duplicate_index_bool_true_same_column() {
    // Test that Bool(true) with duplicate on same column errors
    let mut col1 = col("col1", ColumnType::Simple(SimpleColumnType::Text));
    col1.index = Some(StrOrBoolOrArray::Bool(true));

    let table = TableDef {
        name: "test".into(),
        description: None,
        columns: vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            col1.clone(),
            {
                // Same column with Bool(true) again
                let mut c = col1.clone();
                c.index = Some(StrOrBoolOrArray::Bool(true));
                c
            },
        ],
        constraints: vec![],
    };

    let result = table.normalize();
    assert!(result.is_err());
    if let Err(TableValidationError::DuplicateIndexColumn {
        index_name,
        column_name,
    }) = result
    {
        // The group key for auto-generated indexes is "__auto_{column}"
        assert!(index_name.contains("__auto_"));
        assert!(index_name.contains("col1"));
        assert_eq!(column_name, "col1");
    } else {
        panic!("Expected DuplicateIndexColumn error");
    }
}

mod foreign_key;
