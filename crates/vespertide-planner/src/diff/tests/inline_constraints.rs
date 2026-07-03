use super::*;

// Tests for inline column constraints normalization
use vespertide_core::schema::foreign_key::ForeignKeyDef;
use vespertide_core::schema::foreign_key::ForeignKeySyntax;
use vespertide_core::schema::primary_key::PrimaryKeySyntax;
use vespertide_core::{StrOrBoolOrArray, TableConstraint};

fn col_with_pk(name: &str, ty: ColumnType) -> ColumnDef {
    ColumnDef {
        name: name.into(),
        r#type: ty,
        nullable: false,
        default: None,
        comment: None,
        primary_key: Some(PrimaryKeySyntax::Bool(true)),
        unique: None,
        index: None,
        foreign_key: None,
    }
}

fn col_with_unique(name: &str, ty: ColumnType) -> ColumnDef {
    ColumnDef {
        name: name.into(),
        r#type: ty,
        nullable: true,
        default: None,
        comment: None,
        primary_key: None,
        unique: Some(StrOrBoolOrArray::Bool(true)),
        index: None,
        foreign_key: None,
    }
}

fn col_with_index(name: &str, ty: ColumnType) -> ColumnDef {
    ColumnDef {
        name: name.into(),
        r#type: ty,
        nullable: true,
        default: None,
        comment: None,
        primary_key: None,
        unique: None,
        index: Some(StrOrBoolOrArray::Bool(true)),
        foreign_key: None,
    }
}

fn col_with_fk(name: &str, ty: ColumnType, ref_table: &str, ref_col: &str) -> ColumnDef {
    ColumnDef {
        name: name.into(),
        r#type: ty,
        nullable: true,
        default: None,
        comment: None,
        primary_key: None,
        unique: None,
        index: None,
        foreign_key: Some(ForeignKeySyntax::Object(ForeignKeyDef {
            ref_table: ref_table.into(),
            ref_columns: vec![ref_col.into()],
            on_delete: None,
            on_update: None,
            orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
        })),
    }
}

#[test]
fn create_table_with_inline_pk() {
    let plan = diff_schemas(
        &[],
        &[table(
            "users",
            vec![
                col_with_pk("id", ColumnType::Simple(SimpleColumnType::Integer)),
                col("name", ColumnType::Simple(SimpleColumnType::Text)),
            ],
            vec![],
        )],
    )
    .unwrap();

    // Inline PK should be preserved in column definition
    assert_eq!(plan.actions.len(), 1);
    if let MigrationAction::CreateTable {
        columns,
        constraints,
        ..
    } = &plan.actions[0]
    {
        // Constraints should be empty (inline PK not moved here)
        assert_eq!(constraints.len(), 0);
        // Check that the column has inline PK
        let id_col = columns.iter().find(|c| c.name == "id").unwrap();
        assert!(id_col.primary_key.is_some());
    } else {
        panic!("Expected CreateTable action");
    }
}

#[test]
fn create_table_with_inline_unique() {
    let plan = diff_schemas(
        &[],
        &[table(
            "users",
            vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                col_with_unique("email", ColumnType::Simple(SimpleColumnType::Text)),
            ],
            vec![],
        )],
    )
    .unwrap();

    // Inline unique should be preserved in column definition
    assert_eq!(plan.actions.len(), 1);
    if let MigrationAction::CreateTable {
        columns,
        constraints,
        ..
    } = &plan.actions[0]
    {
        // Constraints should be empty (inline unique not moved here)
        assert_eq!(constraints.len(), 0);
        // Check that the column has inline unique
        let email_col = columns.iter().find(|c| c.name == "email").unwrap();
        assert!(matches!(
            email_col.unique,
            Some(StrOrBoolOrArray::Bool(true))
        ));
    } else {
        panic!("Expected CreateTable action");
    }
}

#[test]
fn create_table_with_inline_index() {
    let plan = diff_schemas(
        &[],
        &[table(
            "users",
            vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                col_with_index("name", ColumnType::Simple(SimpleColumnType::Text)),
            ],
            vec![],
        )],
    )
    .unwrap();

    // Inline index should be preserved in column definition, not moved to constraints
    assert_eq!(plan.actions.len(), 1);
    if let MigrationAction::CreateTable {
        columns,
        constraints,
        ..
    } = &plan.actions[0]
    {
        // Constraints should be empty (inline index not moved here)
        assert_eq!(constraints.len(), 0);
        // Check that the column has inline index
        let name_col = columns.iter().find(|c| c.name == "name").unwrap();
        assert!(matches!(name_col.index, Some(StrOrBoolOrArray::Bool(true))));
    } else {
        panic!("Expected CreateTable action");
    }
}

#[test]
fn create_table_with_inline_fk() {
    let plan = diff_schemas(
        &[],
        &[table(
            "posts",
            vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                col_with_fk(
                    "user_id",
                    ColumnType::Simple(SimpleColumnType::Integer),
                    "users",
                    "id",
                ),
            ],
            vec![],
        )],
    )
    .unwrap();

    // Inline FK should be preserved in column definition
    assert_eq!(plan.actions.len(), 1);
    if let MigrationAction::CreateTable {
        columns,
        constraints,
        ..
    } = &plan.actions[0]
    {
        // Constraints should be empty (inline FK not moved here)
        assert_eq!(constraints.len(), 0);
        // Check that the column has inline FK
        let user_id_col = columns.iter().find(|c| c.name == "user_id").unwrap();
        assert!(user_id_col.foreign_key.is_some());
    } else {
        panic!("Expected CreateTable action");
    }
}

#[test]
fn add_index_via_inline_constraint() {
    // Existing table without index -> table with inline index
    // Inline index (Bool(true)) is normalized to a named table-level constraint
    let plan = diff_schemas(
        &[table(
            "users",
            vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                col("name", ColumnType::Simple(SimpleColumnType::Text)),
            ],
            vec![],
        )],
        &[table(
            "users",
            vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                col_with_index("name", ColumnType::Simple(SimpleColumnType::Text)),
            ],
            vec![],
        )],
    )
    .unwrap();

    // Should generate AddConstraint with name: None (auto-generated indexes)
    assert_eq!(plan.actions.len(), 1);
    if let MigrationAction::AddConstraint { table, constraint } = &plan.actions[0] {
        assert_eq!(table, "users");
        if let TableConstraint::Index { name, columns } = constraint {
            assert_eq!(name, &None); // Auto-generated indexes use None
            assert_eq!(columns, &vec!["name".to_string()]);
        } else {
            panic!("Expected Index constraint, got {constraint:?}");
        }
    } else {
        panic!("Expected AddConstraint action, got {:?}", plan.actions[0]);
    }
}

#[test]
fn create_table_with_all_inline_constraints() {
    let mut id_col = col("id", ColumnType::Simple(SimpleColumnType::Integer));
    id_col.primary_key = Some(PrimaryKeySyntax::Bool(true));
    id_col.nullable = false;

    let mut email_col = col("email", ColumnType::Simple(SimpleColumnType::Text));
    email_col.unique = Some(StrOrBoolOrArray::Bool(true));

    let mut name_col = col("name", ColumnType::Simple(SimpleColumnType::Text));
    name_col.index = Some(StrOrBoolOrArray::Bool(true));

    let mut org_id_col = col("org_id", ColumnType::Simple(SimpleColumnType::Integer));
    org_id_col.foreign_key = Some(ForeignKeySyntax::Object(ForeignKeyDef {
        ref_table: "orgs".into(),
        ref_columns: vec!["id".into()],
        on_delete: None,
        on_update: None,
        orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
    }));

    let plan = diff_schemas(
        &[],
        &[table(
            "users",
            vec![id_col, email_col, name_col, org_id_col],
            vec![],
        )],
    )
    .unwrap();

    // All inline constraints should be preserved in column definitions
    assert_eq!(plan.actions.len(), 1);

    if let MigrationAction::CreateTable {
        columns,
        constraints,
        ..
    } = &plan.actions[0]
    {
        // Constraints should be empty (all inline)
        assert_eq!(constraints.len(), 0);

        // Check each column has its inline constraint
        let id_col = columns.iter().find(|c| c.name == "id").unwrap();
        assert!(id_col.primary_key.is_some());

        let email_col = columns.iter().find(|c| c.name == "email").unwrap();
        assert!(matches!(
            email_col.unique,
            Some(StrOrBoolOrArray::Bool(true))
        ));

        let name_col = columns.iter().find(|c| c.name == "name").unwrap();
        assert!(matches!(name_col.index, Some(StrOrBoolOrArray::Bool(true))));

        let org_id_col = columns.iter().find(|c| c.name == "org_id").unwrap();
        assert!(org_id_col.foreign_key.is_some());
    } else {
        panic!("Expected CreateTable action");
    }
}

#[test]
fn add_constraint_to_existing_table() {
    // Add a unique constraint to an existing table
    let from_schema = vec![table(
        "users",
        vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            col("email", ColumnType::Simple(SimpleColumnType::Text)),
        ],
        vec![],
    )];

    let to_schema = vec![table(
        "users",
        vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            col("email", ColumnType::Simple(SimpleColumnType::Text)),
        ],
        vec![vespertide_core::TableConstraint::Unique {
            name: Some("uq_users_email".into()),
            columns: vec!["email".into()],
            strategy: vespertide_core::UniqueConstraintStrategy::DeleteDuplicates {
                keep: vespertide_core::KeepPolicy::First,
            },
        }],
    )];

    let plan = diff_schemas(&from_schema, &to_schema).unwrap();
    assert_eq!(plan.actions.len(), 1);
    if let MigrationAction::AddConstraint { table, constraint } = &plan.actions[0] {
        assert_eq!(table, "users");
        assert!(matches!(
            constraint,
            vespertide_core::TableConstraint::Unique { name: Some(n), columns, .. }
                if n == "uq_users_email" && columns == &vec!["email".to_string()]
        ));
    } else {
        panic!("Expected AddConstraint action, got {:?}", plan.actions[0]);
    }
}

#[test]
fn remove_constraint_from_existing_table() {
    // Remove a unique constraint from an existing table
    let from_schema = vec![table(
        "users",
        vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            col("email", ColumnType::Simple(SimpleColumnType::Text)),
        ],
        vec![vespertide_core::TableConstraint::Unique {
            name: Some("uq_users_email".into()),
            columns: vec!["email".into()],
            strategy: vespertide_core::UniqueConstraintStrategy::DeleteDuplicates {
                keep: vespertide_core::KeepPolicy::First,
            },
        }],
    )];

    let to_schema = vec![table(
        "users",
        vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            col("email", ColumnType::Simple(SimpleColumnType::Text)),
        ],
        vec![],
    )];

    let plan = diff_schemas(&from_schema, &to_schema).unwrap();
    assert_eq!(plan.actions.len(), 1);
    if let MigrationAction::RemoveConstraint { table, constraint } = &plan.actions[0] {
        assert_eq!(table, "users");
        assert!(matches!(
            constraint,
            vespertide_core::TableConstraint::Unique { name: Some(n), columns, .. }
                if n == "uq_users_email" && columns == &vec!["email".to_string()]
        ));
    } else {
        panic!(
            "Expected RemoveConstraint action, got {:?}",
            plan.actions[0]
        );
    }
}

#[test]
fn diff_schemas_with_normalize_error() {
    // Test that normalize errors are properly propagated
    let mut col1 = col("col1", ColumnType::Simple(SimpleColumnType::Text));
    col1.index = Some(StrOrBoolOrArray::Str("idx1".into()));

    let table = TableDef {
        name: "test".into(),
        description: None,
        columns: vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            col1.clone(),
            {
                // Same column with same index name - should error
                let mut c = col1.clone();
                c.index = Some(StrOrBoolOrArray::Str("idx1".into()));
                c
            },
        ],
        constraints: vec![],
    };

    let result = diff_schemas(&[], &[table]);
    assert!(result.is_err());
    if let Err(PlannerError::TableValidation(msg)) = result {
        // Audit C3-P1: duplicate column names are now rejected before normalize().
        // The lowercased message must mention "duplicate" — either the column-name
        // check (preferred, earlier) or the legacy normalize-time index check.
        let lower = msg.to_lowercase();
        assert!(
            lower.contains("duplicate"),
            "expected a duplicate-column or duplicate-index error, got: {msg}"
        );
    } else {
        panic!("Expected TableValidation error, got {result:?}");
    }
}

#[test]
fn diff_schemas_with_normalize_error_in_from_schema() {
    // Test that normalize errors in 'from' schema are properly propagated
    let mut col1 = col("col1", ColumnType::Simple(SimpleColumnType::Text));
    col1.index = Some(StrOrBoolOrArray::Str("idx1".into()));

    let table = TableDef {
        name: "test".into(),
        description: None,
        columns: vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            col1.clone(),
            {
                // Same column with same index name - should error
                let mut c = col1.clone();
                c.index = Some(StrOrBoolOrArray::Str("idx1".into()));
                c
            },
        ],
        constraints: vec![],
    };

    // 'from' schema has the invalid table
    let result = diff_schemas(&[table], &[]);
    assert!(result.is_err());
    if let Err(PlannerError::TableValidation(msg)) = result {
        // Audit C3-P1: duplicate column names rejected before normalize().
        let lower = msg.to_lowercase();
        assert!(
            lower.contains("duplicate"),
            "expected a duplicate-column or duplicate-index error, got: {msg}"
        );
    } else {
        panic!("Expected TableValidation error, got {result:?}");
    }
}
