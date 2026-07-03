use super::*;

// Tests for foreign key dependency ordering
use vespertide_core::{MigrationPlan, TableConstraint};

fn table_with_fk(name: &str, ref_table: &str, fk_column: &str, ref_column: &str) -> TableDef {
    TableDef {
        name: name.into(),
        description: None,
        columns: vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            col(fk_column, ColumnType::Simple(SimpleColumnType::Integer)),
        ],
        constraints: vec![TableConstraint::ForeignKey {
            name: None,
            columns: vec![fk_column.into()],
            ref_table: ref_table.into(),
            ref_columns: vec![ref_column.into()],
            on_delete: None,
            on_update: None,
            orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
        }],
    }
}

fn simple_table(name: &str) -> TableDef {
    TableDef {
        name: name.into(),
        description: None,
        columns: vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
        constraints: vec![],
    }
}

fn pk_col(name: &str) -> ColumnDef {
    use vespertide_core::schema::primary_key::PrimaryKeySyntax;

    ColumnDef {
        name: name.into(),
        r#type: ColumnType::Simple(SimpleColumnType::Integer),
        nullable: false,
        default: None,
        comment: None,
        primary_key: Some(PrimaryKeySyntax::Bool(true)),
        unique: None,
        index: None,
        foreign_key: None,
    }
}

fn inline_fk_col(name: &str, ref_table: &str) -> ColumnDef {
    use vespertide_core::schema::foreign_key::ForeignKeySyntax;

    ColumnDef {
        name: name.into(),
        r#type: ColumnType::Simple(SimpleColumnType::Integer),
        nullable: true,
        default: None,
        comment: None,
        primary_key: None,
        unique: None,
        index: None,
        foreign_key: Some(ForeignKeySyntax::String(format!("{ref_table}.id"))),
    }
}

fn inline_fk_table(name: &str, ref_tables: &[&str]) -> TableDef {
    let mut columns = vec![pk_col("id")];
    columns.extend(
        ref_tables
            .iter()
            .map(|ref_table| inline_fk_col(&format!("{ref_table}_id"), ref_table)),
    );

    TableDef {
        name: name.into(),
        description: None,
        columns,
        constraints: vec![],
    }
}

fn create_order(plan: &MigrationPlan) -> Vec<&str> {
    plan.actions
        .iter()
        .filter_map(|action| {
            if let MigrationAction::CreateTable { table, .. } = action {
                Some(table.as_str())
            } else {
                None
            }
        })
        .collect()
}

fn assert_before(order: &[&str], first: &str, second: &str) {
    let first_pos = order.iter().position(|&table| table == first).unwrap();
    let second_pos = order.iter().position(|&table| table == second).unwrap();
    assert!(first_pos < second_pos, "{first} must come before {second}");
}

fn text_col_with_comment(name: &str, comment: &str) -> ColumnDef {
    let mut column = col(name, ColumnType::Simple(SimpleColumnType::Text));
    column.comment = Some(comment.into());
    column
}

fn sort_all_branch_schemas() -> (Vec<TableDef>, Vec<TableDef>) {
    let users_from = table(
        "users",
        vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            text_col_with_comment("name", "Old comment"),
            col("role_id", ColumnType::Simple(SimpleColumnType::Integer)),
        ],
        vec![],
    );
    let posts_from = table(
        "posts",
        vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            col("title", ColumnType::Simple(SimpleColumnType::Text)),
        ],
        vec![],
    );
    let roles = table(
        "roles",
        vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
        vec![],
    );

    let users_to = table(
        "users",
        vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            text_col_with_comment("name", "New comment"),
            col("role_id", ColumnType::Simple(SimpleColumnType::Integer)),
        ],
        vec![fk_constraint("role_id", "roles")],
    );
    let posts_to = table(
        "posts",
        vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            col("title", ColumnType::Simple(SimpleColumnType::Text)),
        ],
        vec![TableConstraint::Index {
            name: Some("idx_title".into()),
            columns: vec!["title".into()],
        }],
    );

    (
        vec![users_from, posts_from],
        vec![users_to, posts_to, roles],
    )
}

fn fk_constraint(column: &str, ref_table: &str) -> TableConstraint {
    TableConstraint::ForeignKey {
        name: None,
        columns: vec![column.into()],
        ref_table: ref_table.into(),
        ref_columns: vec!["id".into()],
        on_delete: None,
        on_update: None,
        orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
    }
}

#[test]
fn create_tables_respects_fk_order() {
    // Create users and posts tables where posts references users
    // The order should be: users first, then posts
    let users = simple_table("users");
    let posts = table_with_fk("posts", "users", "user_id", "id");

    let plan = diff_schemas(&[], &[posts.clone(), users.clone()]).unwrap();

    // Extract CreateTable actions in order
    let create_order: Vec<&str> = plan
        .actions
        .iter()
        .filter_map(|a| {
            if let MigrationAction::CreateTable { table, .. } = a {
                Some(table.as_str())
            } else {
                None
            }
        })
        .collect();

    assert_eq!(create_order, vec!["users", "posts"]);
}

#[test]
fn create_tables_chain_dependency() {
    // Chain: users <- media <- articles
    // users has no FK
    // media references users
    // articles references media
    let users = simple_table("users");
    let media = table_with_fk("media", "users", "owner_id", "id");
    let articles = table_with_fk("articles", "media", "media_id", "id");

    // Pass in reverse order to ensure sorting works
    let plan = diff_schemas(&[], &[articles.clone(), media.clone(), users.clone()]).unwrap();

    let create_order: Vec<&str> = plan
        .actions
        .iter()
        .filter_map(|a| {
            if let MigrationAction::CreateTable { table, .. } = a {
                Some(table.as_str())
            } else {
                None
            }
        })
        .collect();

    assert_eq!(create_order, vec!["users", "media", "articles"]);
}

#[test]
fn create_tables_multiple_independent_branches() {
    // Two independent branches:
    // users <- posts
    // categories <- products
    let users = simple_table("users");
    let posts = table_with_fk("posts", "users", "user_id", "id");
    let categories = simple_table("categories");
    let products = table_with_fk("products", "categories", "category_id", "id");

    let plan = diff_schemas(
        &[],
        &[
            products.clone(),
            posts.clone(),
            categories.clone(),
            users.clone(),
        ],
    )
    .unwrap();

    let create_order: Vec<&str> = plan
        .actions
        .iter()
        .filter_map(|a| {
            if let MigrationAction::CreateTable { table, .. } = a {
                Some(table.as_str())
            } else {
                None
            }
        })
        .collect();

    // users must come before posts
    let users_pos = create_order.iter().position(|&t| t == "users").unwrap();
    let posts_pos = create_order.iter().position(|&t| t == "posts").unwrap();
    assert!(
        users_pos < posts_pos,
        "users should be created before posts"
    );

    // categories must come before products
    let categories_pos = create_order
        .iter()
        .position(|&t| t == "categories")
        .unwrap();
    let products_pos = create_order.iter().position(|&t| t == "products").unwrap();
    assert!(
        categories_pos < products_pos,
        "categories should be created before products"
    );
}

#[test]
fn delete_tables_respects_fk_order() {
    // When deleting users and posts where posts references users,
    // posts should be deleted first (reverse of creation order)
    let users = simple_table("users");
    let posts = table_with_fk("posts", "users", "user_id", "id");

    let plan = diff_schemas(&[users.clone(), posts.clone()], &[]).unwrap();

    let delete_order: Vec<&str> = plan
        .actions
        .iter()
        .filter_map(|a| {
            if let MigrationAction::DeleteTable { table } = a {
                Some(table.as_str())
            } else {
                None
            }
        })
        .collect();

    assert_eq!(delete_order, vec!["posts", "users"]);
}

#[test]
fn delete_tables_chain_dependency() {
    // Chain: users <- media <- articles
    // Delete order should be: articles, media, users
    let users = simple_table("users");
    let media = table_with_fk("media", "users", "owner_id", "id");
    let articles = table_with_fk("articles", "media", "media_id", "id");

    let plan = diff_schemas(&[users.clone(), media.clone(), articles.clone()], &[]).unwrap();

    let delete_order: Vec<&str> = plan
        .actions
        .iter()
        .filter_map(|a| {
            if let MigrationAction::DeleteTable { table } = a {
                Some(table.as_str())
            } else {
                None
            }
        })
        .collect();

    // articles must be deleted before media
    let articles_pos = delete_order.iter().position(|&t| t == "articles").unwrap();
    let media_pos = delete_order.iter().position(|&t| t == "media").unwrap();
    assert!(
        articles_pos < media_pos,
        "articles should be deleted before media"
    );

    // media must be deleted before users
    let users_pos = delete_order.iter().position(|&t| t == "users").unwrap();
    assert!(
        media_pos < users_pos,
        "media should be deleted before users"
    );
}

#[test]
fn circular_fk_dependency_returns_error() {
    // Create circular dependency: A -> B -> A
    let table_a = TableDef {
        name: "table_a".into(),
        description: None,
        columns: vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            col("b_id", ColumnType::Simple(SimpleColumnType::Integer)),
        ],
        constraints: vec![TableConstraint::ForeignKey {
            name: None,
            columns: vec!["b_id".into()],
            ref_table: "table_b".into(),
            ref_columns: vec!["id".into()],
            on_delete: None,
            on_update: None,
            orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
        }],
    };

    let table_b = TableDef {
        name: "table_b".into(),
        description: None,
        columns: vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            col("a_id", ColumnType::Simple(SimpleColumnType::Integer)),
        ],
        constraints: vec![TableConstraint::ForeignKey {
            name: None,
            columns: vec!["a_id".into()],
            ref_table: "table_a".into(),
            ref_columns: vec!["id".into()],
            on_delete: None,
            on_update: None,
            orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
        }],
    };

    let result = diff_schemas(&[], &[table_a, table_b]);
    assert!(result.is_err());
    if let Err(PlannerError::TableValidation(msg)) = result {
        assert!(
            msg.contains("Circular foreign key dependency"),
            "Expected circular dependency error, got: {msg}"
        );
    } else {
        panic!("Expected TableValidation error, got {result:?}");
    }
}

#[test]
fn diff_schemas_detects_circular_fk_cycle() {
    let a = TableDef {
        name: "a".into(),
        description: None,
        columns: vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            col("b_id", ColumnType::Simple(SimpleColumnType::Integer)),
        ],
        constraints: vec![TableConstraint::ForeignKey {
            name: None,
            columns: vec!["b_id".into()],
            ref_table: "b".into(),
            ref_columns: vec!["id".into()],
            on_delete: None,
            on_update: None,
            orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
        }],
    };
    let b = TableDef {
        name: "b".into(),
        description: None,
        columns: vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            col("a_id", ColumnType::Simple(SimpleColumnType::Integer)),
        ],
        constraints: vec![TableConstraint::ForeignKey {
            name: None,
            columns: vec!["a_id".into()],
            ref_table: "a".into(),
            ref_columns: vec!["id".into()],
            on_delete: None,
            on_update: None,
            orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
        }],
    };

    let result = diff_schemas(&[], &[a, b]);

    assert!(
        matches!(
            result,
            Err(PlannerError::TableValidation(ref msg))
                if msg.contains("Circular foreign key dependency")
        ),
        "mutual FKs are currently rejected by Kahn cycle detection, got: {result:?}"
    );
}

#[test]
fn fk_to_external_table_is_ignored() {
    // FK referencing a table not in the migration should not affect ordering
    let posts = table_with_fk("posts", "users", "user_id", "id");
    let comments = table_with_fk("comments", "posts", "post_id", "id");

    // users is NOT being created in this migration
    let plan = diff_schemas(&[], &[comments.clone(), posts.clone()]).unwrap();

    let create_order: Vec<&str> = plan
        .actions
        .iter()
        .filter_map(|a| {
            if let MigrationAction::CreateTable { table, .. } = a {
                Some(table.as_str())
            } else {
                None
            }
        })
        .collect();

    // posts must come before comments (comments depends on posts)
    let posts_pos = create_order.iter().position(|&t| t == "posts").unwrap();
    let comments_pos = create_order.iter().position(|&t| t == "comments").unwrap();
    assert!(
        posts_pos < comments_pos,
        "posts should be created before comments"
    );
}

#[test]
fn delete_tables_mixed_with_other_actions() {
    // Test that sort_delete_actions correctly handles actions that are not DeleteTable
    // This tests lines 124, 193, 198 (the else branches)
    use crate::diff::diff_schemas;

    let from_schema = vec![
        table(
            "users",
            vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
            vec![],
        ),
        table(
            "posts",
            vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
            vec![],
        ),
    ];

    let to_schema = vec![
        // Drop posts table, but also add a new column to users
        table(
            "users",
            vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                col("name", ColumnType::Simple(SimpleColumnType::Text)),
            ],
            vec![],
        ),
    ];

    let plan = diff_schemas(&from_schema, &to_schema).unwrap();

    // Should have: AddColumn (for users.name) and DeleteTable (for posts)
    assert!(
        plan.actions
            .iter()
            .any(|a| matches!(a, MigrationAction::AddColumn { .. }))
    );
    assert!(
        plan.actions
            .iter()
            .any(|a| matches!(a, MigrationAction::DeleteTable { .. }))
    );

    // The else branches in sort_delete_actions should handle AddColumn gracefully
    // (returning empty string for table name, which sorts it to position 0)
}

#[test]
#[should_panic(expected = "Expected DeleteTable action")]
fn test_extract_delete_table_name_panics_on_non_delete_action() {
    // Test that extract_delete_table_name panics when called with non-DeleteTable action
    use crate::diff::ordering::extract_delete_table_name;

    let action = MigrationAction::AddColumn {
        table: "users".into(),
        column: Box::new(ColumnDef {
            name: "email".into(),
            r#type: ColumnType::Simple(SimpleColumnType::Text),
            nullable: true,
            default: None,
            comment: None,
            primary_key: None,
            unique: None,
            index: None,
            foreign_key: None,
        }),
        fill_with: None,
    };

    // This should panic
    extract_delete_table_name(&action);
}

/// Test that inline FK across multiple tables works correctly with topological sort
#[test]
fn create_tables_with_inline_fk_chain() {
    use super::*;

    // Reproduce the app example structure:
    // user -> (no deps)
    // product -> (no deps)
    // project -> user
    // code -> product, user, project
    // order -> user, project, product, code
    // payment -> order

    let user = inline_fk_table("user", &[]);
    let product = inline_fk_table("product", &[]);
    let project = inline_fk_table("project", &["user"]);
    let code = inline_fk_table("code", &["product", "user", "project"]);
    let order = inline_fk_table("order", &["user", "project", "product", "code"]);
    let payment = inline_fk_table("payment", &["order"]);

    // Pass in arbitrary order - should NOT return circular dependency error
    let result = diff_schemas(&[], &[payment, order, code, project, product, user]);
    assert!(result.is_ok(), "Expected Ok, got: {result:?}");

    let plan = result.unwrap();
    let order = create_order(&plan);

    // user and product have no deps, can be in any order
    // project depends on user
    assert_before(&order, "user", "project");
    // code depends on product, user, project
    assert_before(&order, "product", "code");
    assert_before(&order, "user", "code");
    assert_before(&order, "project", "code");
    // order depends on user, project, product, code
    assert_before(&order, "code", "order");
    // payment depends on order
    assert_before(&order, "order", "payment");
}

/// Test that `AddConstraint` FK to a new table comes AFTER `CreateTable` for that table
#[test]
fn add_constraint_fk_to_new_table_comes_after_create_table() {
    use super::*;

    // Existing table: notification (with broadcast_id column)
    let notification_from = table(
        "notification",
        vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            col(
                "broadcast_id",
                ColumnType::Simple(SimpleColumnType::Integer),
            ),
        ],
        vec![],
    );

    // New table: notification_broadcast
    let notification_broadcast = table(
        "notification_broadcast",
        vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
        vec![],
    );

    // Modified notification with FK constraint to the new table
    let notification_to = table(
        "notification",
        vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            col(
                "broadcast_id",
                ColumnType::Simple(SimpleColumnType::Integer),
            ),
        ],
        vec![TableConstraint::ForeignKey {
            name: None,
            columns: vec!["broadcast_id".into()],
            ref_table: "notification_broadcast".into(),
            ref_columns: vec!["id".into()],
            on_delete: None,
            on_update: None,
            orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
        }],
    );

    let from_schema = vec![notification_from];
    let to_schema = vec![notification_to, notification_broadcast];

    let plan = diff_schemas(&from_schema, &to_schema).unwrap();

    // Find positions
    let create_pos = plan.actions.iter().position(|a| matches!(a, MigrationAction::CreateTable { table, .. } if table == "notification_broadcast"));
    let add_constraint_pos = plan.actions.iter().position(|a| {
        matches!(a, MigrationAction::AddConstraint {
            constraint: TableConstraint::ForeignKey { ref_table, .. }, ..
        } if ref_table == "notification_broadcast")
    });

    assert!(
        create_pos.is_some(),
        "Should have CreateTable for notification_broadcast"
    );
    assert!(
        add_constraint_pos.is_some(),
        "Should have AddConstraint for FK to notification_broadcast"
    );
    assert!(
        create_pos.unwrap() < add_constraint_pos.unwrap(),
        "CreateTable must come BEFORE AddConstraint FK that references it. Got CreateTable at {}, AddConstraint at {}",
        create_pos.unwrap(),
        add_constraint_pos.unwrap()
    );
}

/// Test `sort_create_before_add_constraint` with multiple action types
/// Covers lines 218, 221, 223, 225 in `sort_create_before_add_constraint`
#[test]
fn sort_create_before_add_constraint_all_branches() {
    use super::*;
    // Existing tables get a comment change, a regular index, and an FK to a new table.
    let (from_schema, to_schema) = sort_all_branch_schemas();

    let plan = diff_schemas(&from_schema, &to_schema).unwrap();

    // Verify CreateTable comes first
    let create_pos = plan
        .actions
        .iter()
        .position(|a| matches!(a, MigrationAction::CreateTable { table, .. } if table == "roles"))
        .expect("Should have CreateTable for roles");

    // ModifyColumnComment should come after CreateTable (line 218: non-create vs create)
    let modify_pos = plan
        .actions
        .iter()
        .position(|a| matches!(a, MigrationAction::ModifyColumnComment { .. }))
        .expect("Should have ModifyColumnComment");

    // AddConstraint Index (not FK to created) should come after CreateTable (line 218)
    let add_index_pos = plan
        .actions
        .iter()
        .position(|a| {
            matches!(
                a,
                MigrationAction::AddConstraint {
                    constraint: TableConstraint::Index { .. },
                    ..
                }
            )
        })
        .expect("Should have AddConstraint Index");

    // AddConstraint FK to roles should come last (line 221: refs created, others don't)
    let add_fk_pos = plan
        .actions
        .iter()
        .position(|a| {
            matches!(
                a,
                MigrationAction::AddConstraint {
                    constraint: TableConstraint::ForeignKey { ref_table, .. },
                    ..
                } if ref_table == "roles"
            )
        })
        .expect("Should have AddConstraint FK to roles");

    assert!(
        create_pos < modify_pos,
        "CreateTable must come before ModifyColumnComment"
    );
    assert!(
        create_pos < add_index_pos,
        "CreateTable must come before AddConstraint Index"
    );
    assert!(
        create_pos < add_fk_pos,
        "CreateTable must come before AddConstraint FK"
    );
    // FK to created table should come after non-FK-to-created actions
    assert!(
        add_index_pos < add_fk_pos,
        "AddConstraint Index (not referencing created) should come before AddConstraint FK (referencing created)"
    );
}

/// Test that two `AddConstraint` FKs both referencing created tables maintain stable order
/// Covers line 225: both ref created tables
#[test]
fn sort_multiple_fks_to_created_tables() {
    use super::*;

    // Two existing tables, each getting FK to a different new table
    let users_from = table(
        "users",
        vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            col("role_id", ColumnType::Simple(SimpleColumnType::Integer)),
        ],
        vec![],
    );

    let posts_from = table(
        "posts",
        vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            col("category_id", ColumnType::Simple(SimpleColumnType::Integer)),
        ],
        vec![],
    );

    // Two new tables
    let roles = table(
        "roles",
        vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
        vec![],
    );
    let categories = table(
        "categories",
        vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
        vec![],
    );

    let users_to = table(
        "users",
        vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            col("role_id", ColumnType::Simple(SimpleColumnType::Integer)),
        ],
        vec![TableConstraint::ForeignKey {
            name: None,
            columns: vec!["role_id".into()],
            ref_table: "roles".into(),
            ref_columns: vec!["id".into()],
            on_delete: None,
            on_update: None,
            orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
        }],
    );

    let posts_to = table(
        "posts",
        vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            col("category_id", ColumnType::Simple(SimpleColumnType::Integer)),
        ],
        vec![TableConstraint::ForeignKey {
            name: None,
            columns: vec!["category_id".into()],
            ref_table: "categories".into(),
            ref_columns: vec!["id".into()],
            on_delete: None,
            on_update: None,
            orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
        }],
    );

    let from_schema = vec![users_from, posts_from];
    let to_schema = vec![users_to, posts_to, roles, categories];

    let plan = diff_schemas(&from_schema, &to_schema).unwrap();

    // Both CreateTable should come before both AddConstraint FK
    let create_roles_pos = plan
        .actions
        .iter()
        .position(|a| matches!(a, MigrationAction::CreateTable { table, .. } if table == "roles"));
    let create_categories_pos = plan.actions.iter().position(
        |a| matches!(a, MigrationAction::CreateTable { table, .. } if table == "categories"),
    );
    let add_fk_roles_pos = plan.actions.iter().position(|a| {
        matches!(
            a,
            MigrationAction::AddConstraint {
                constraint: TableConstraint::ForeignKey { ref_table, .. },
                ..
            } if ref_table == "roles"
        )
    });
    let add_fk_categories_pos = plan.actions.iter().position(|a| {
        matches!(
            a,
            MigrationAction::AddConstraint {
                constraint: TableConstraint::ForeignKey { ref_table, .. },
                ..
            } if ref_table == "categories"
        )
    });

    assert!(create_roles_pos.is_some());
    assert!(create_categories_pos.is_some());
    assert!(add_fk_roles_pos.is_some());
    assert!(add_fk_categories_pos.is_some());

    // All CreateTable before all AddConstraint FK
    let max_create = create_roles_pos
        .unwrap()
        .max(create_categories_pos.unwrap());
    let min_add_fk = add_fk_roles_pos
        .unwrap()
        .min(add_fk_categories_pos.unwrap());
    assert!(
        max_create < min_add_fk,
        "All CreateTable actions must come before all AddConstraint FK actions"
    );
}

/// Test that multiple FKs to the same table are deduplicated correctly
#[test]
fn create_tables_with_duplicate_fk_references() {
    use super::*;
    use vespertide_core::schema::foreign_key::ForeignKeySyntax;
    use vespertide_core::schema::primary_key::PrimaryKeySyntax;

    fn col_pk(name: &str) -> ColumnDef {
        ColumnDef {
            name: name.into(),
            r#type: ColumnType::Simple(SimpleColumnType::Integer),
            nullable: false,
            default: None,
            comment: None,
            primary_key: Some(PrimaryKeySyntax::Bool(true)),
            unique: None,
            index: None,
            foreign_key: None,
        }
    }

    fn col_inline_fk(name: &str, ref_table: &str) -> ColumnDef {
        ColumnDef {
            name: name.into(),
            r#type: ColumnType::Simple(SimpleColumnType::Integer),
            nullable: true,
            default: None,
            comment: None,
            primary_key: None,
            unique: None,
            index: None,
            foreign_key: Some(ForeignKeySyntax::String(format!("{ref_table}.id"))),
        }
    }

    // Table with multiple FKs referencing the same table (like code.creator_user_id and code.used_by_user_id)
    let user = TableDef {
        name: "user".into(),
        description: None,
        columns: vec![col_pk("id")],
        constraints: vec![],
    };

    let code = TableDef {
        name: "code".into(),
        description: None,
        columns: vec![
            col_pk("id"),
            col_inline_fk("creator_user_id", "user"),
            col_inline_fk("used_by_user_id", "user"), // Second FK to same table
        ],
        constraints: vec![],
    };

    // This should NOT return circular dependency error even with duplicate FK refs
    let result = diff_schemas(&[], &[code, user]);
    assert!(result.is_ok(), "Expected Ok, got: {result:?}");

    let plan = result.unwrap();
    let create_order: Vec<&str> = plan
        .actions
        .iter()
        .filter_map(|a| {
            if let MigrationAction::CreateTable { table, .. } = a {
                Some(table.as_str())
            } else {
                None
            }
        })
        .collect();

    // user must come before code
    let user_pos = create_order.iter().position(|&t| t == "user").unwrap();
    let code_pos = create_order.iter().position(|&t| t == "code").unwrap();
    assert!(user_pos < code_pos, "user must come before code");
}
