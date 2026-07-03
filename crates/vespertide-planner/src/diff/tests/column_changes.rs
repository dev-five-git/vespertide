use super::*;

mod default_changes {
    use super::*;

    fn col_with_default(name: &str, ty: ColumnType, default: Option<&str>) -> ColumnDef {
        ColumnDef {
            name: name.into(),
            r#type: ty,
            nullable: true,
            default: default.map(std::convert::Into::into),
            comment: None,
            primary_key: None,
            unique: None,
            index: None,
            foreign_key: None,
        }
    }

    #[test]
    fn add_default_value() {
        // Column: no default -> has default
        let from = vec![table(
            "users",
            vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                col_with_default("status", ColumnType::Simple(SimpleColumnType::Text), None),
            ],
            vec![],
        )];

        let to = vec![table(
            "users",
            vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                col_with_default(
                    "status",
                    ColumnType::Simple(SimpleColumnType::Text),
                    Some("'active'"),
                ),
            ],
            vec![],
        )];

        let plan = diff_schemas(&from, &to).unwrap();

        assert_eq!(plan.actions.len(), 1);
        assert!(matches!(
            &plan.actions[0],
            MigrationAction::ModifyColumnDefault {
                table,
                column,
                new_default: Some(default),
                ..
            } if table == "users" && column == "status" && default == "'active'"
        ));
    }

    #[test]
    fn remove_default_value() {
        // Column: has default -> no default
        let from = vec![table(
            "users",
            vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                col_with_default(
                    "status",
                    ColumnType::Simple(SimpleColumnType::Text),
                    Some("'active'"),
                ),
            ],
            vec![],
        )];

        let to = vec![table(
            "users",
            vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                col_with_default("status", ColumnType::Simple(SimpleColumnType::Text), None),
            ],
            vec![],
        )];

        let plan = diff_schemas(&from, &to).unwrap();

        assert_eq!(plan.actions.len(), 1);
        assert!(matches!(
            &plan.actions[0],
            MigrationAction::ModifyColumnDefault {
                table,
                column,
                new_default: None,
                ..
            } if table == "users" && column == "status"
        ));
    }

    #[test]
    fn change_default_value() {
        // Column: 'active' -> 'pending'
        let from = vec![table(
            "users",
            vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                col_with_default(
                    "status",
                    ColumnType::Simple(SimpleColumnType::Text),
                    Some("'active'"),
                ),
            ],
            vec![],
        )];

        let to = vec![table(
            "users",
            vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                col_with_default(
                    "status",
                    ColumnType::Simple(SimpleColumnType::Text),
                    Some("'pending'"),
                ),
            ],
            vec![],
        )];

        let plan = diff_schemas(&from, &to).unwrap();

        assert_eq!(plan.actions.len(), 1);
        assert!(matches!(
            &plan.actions[0],
            MigrationAction::ModifyColumnDefault {
                table,
                column,
                new_default: Some(default),
                ..
            } if table == "users" && column == "status" && default == "'pending'"
        ));
    }

    #[test]
    fn no_change_same_default() {
        // Column: same default -> no action
        let from = vec![table(
            "users",
            vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                col_with_default(
                    "status",
                    ColumnType::Simple(SimpleColumnType::Text),
                    Some("'active'"),
                ),
            ],
            vec![],
        )];

        let to = vec![table(
            "users",
            vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                col_with_default(
                    "status",
                    ColumnType::Simple(SimpleColumnType::Text),
                    Some("'active'"),
                ),
            ],
            vec![],
        )];

        let plan = diff_schemas(&from, &to).unwrap();

        assert!(plan.actions.is_empty());
    }

    #[test]
    fn multiple_columns_default_changes() {
        // Multiple columns with default changes
        let from = vec![table(
            "users",
            vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                col_with_default("status", ColumnType::Simple(SimpleColumnType::Text), None),
                col_with_default(
                    "role",
                    ColumnType::Simple(SimpleColumnType::Text),
                    Some("'user'"),
                ),
                col_with_default(
                    "active",
                    ColumnType::Simple(SimpleColumnType::Boolean),
                    Some("true"),
                ),
            ],
            vec![],
        )];

        let to = vec![table(
            "users",
            vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                col_with_default(
                    "status",
                    ColumnType::Simple(SimpleColumnType::Text),
                    Some("'pending'"),
                ), // None -> 'pending'
                col_with_default("role", ColumnType::Simple(SimpleColumnType::Text), None), // 'user' -> None
                col_with_default(
                    "active",
                    ColumnType::Simple(SimpleColumnType::Boolean),
                    Some("true"),
                ), // no change
            ],
            vec![],
        )];

        let plan = diff_schemas(&from, &to).unwrap();

        assert_eq!(plan.actions.len(), 2);

        let has_status_change = plan.actions.iter().any(|a| {
            matches!(
                a,
                MigrationAction::ModifyColumnDefault {
                    table,
                    column,
                    new_default: Some(default),
                    ..
                } if table == "users" && column == "status" && default == "'pending'"
            )
        });
        assert!(has_status_change, "Should detect status default added");

        let has_role_change = plan.actions.iter().any(|a| {
            matches!(
                a,
                MigrationAction::ModifyColumnDefault {
                    table,
                    column,
                    new_default: None,
                    ..
                } if table == "users" && column == "role"
            )
        });
        assert!(has_role_change, "Should detect role default removed");
    }

    #[test]
    fn default_change_with_type_change() {
        // Column changing both type and default
        let from = vec![table(
            "users",
            vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                col_with_default(
                    "count",
                    ColumnType::Simple(SimpleColumnType::Integer),
                    Some("0"),
                ),
            ],
            vec![],
        )];

        let to = vec![table(
            "users",
            vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                col_with_default(
                    "count",
                    ColumnType::Simple(SimpleColumnType::Text),
                    Some("'0'"),
                ),
            ],
            vec![],
        )];

        let plan = diff_schemas(&from, &to).unwrap();

        // Should generate both ModifyColumnType and ModifyColumnDefault
        assert_eq!(plan.actions.len(), 2);

        let has_type_change = plan.actions.iter().any(|a| {
            matches!(
                a,
                MigrationAction::ModifyColumnType { table, column, .. }
                if table == "users" && column == "count"
            )
        });
        assert!(has_type_change, "Should detect type change");

        let has_default_change = plan.actions.iter().any(|a| {
            matches!(
                a,
                MigrationAction::ModifyColumnDefault {
                    table,
                    column,
                    new_default: Some(default),
                    ..
                } if table == "users" && column == "count" && default == "'0'"
            )
        });
        assert!(has_default_change, "Should detect default change");
    }
}

mod comment_changes {
    use super::*;

    fn col_with_comment(name: &str, ty: ColumnType, comment: Option<&str>) -> ColumnDef {
        ColumnDef {
            name: name.into(),
            r#type: ty,
            nullable: true,
            default: None,
            comment: comment.map(std::string::ToString::to_string),
            primary_key: None,
            unique: None,
            index: None,
            foreign_key: None,
        }
    }

    #[test]
    fn add_comment() {
        // Column: no comment -> has comment
        let from = vec![table(
            "users",
            vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                col_with_comment("email", ColumnType::Simple(SimpleColumnType::Text), None),
            ],
            vec![],
        )];

        let to = vec![table(
            "users",
            vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                col_with_comment(
                    "email",
                    ColumnType::Simple(SimpleColumnType::Text),
                    Some("User's email address"),
                ),
            ],
            vec![],
        )];

        let plan = diff_schemas(&from, &to).unwrap();

        assert_eq!(plan.actions.len(), 1);
        assert!(matches!(
            &plan.actions[0],
            MigrationAction::ModifyColumnComment {
                table,
                column,
                new_comment: Some(comment),
            } if table == "users" && column == "email" && comment == "User's email address"
        ));
    }

    #[test]
    fn remove_comment() {
        // Column: has comment -> no comment
        let from = vec![table(
            "users",
            vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                col_with_comment(
                    "email",
                    ColumnType::Simple(SimpleColumnType::Text),
                    Some("User's email address"),
                ),
            ],
            vec![],
        )];

        let to = vec![table(
            "users",
            vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                col_with_comment("email", ColumnType::Simple(SimpleColumnType::Text), None),
            ],
            vec![],
        )];

        let plan = diff_schemas(&from, &to).unwrap();

        assert_eq!(plan.actions.len(), 1);
        assert!(matches!(
            &plan.actions[0],
            MigrationAction::ModifyColumnComment {
                table,
                column,
                new_comment: None,
            } if table == "users" && column == "email"
        ));
    }

    #[test]
    fn change_comment() {
        // Column: 'old comment' -> 'new comment'
        let from = vec![table(
            "users",
            vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                col_with_comment(
                    "email",
                    ColumnType::Simple(SimpleColumnType::Text),
                    Some("Old comment"),
                ),
            ],
            vec![],
        )];

        let to = vec![table(
            "users",
            vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                col_with_comment(
                    "email",
                    ColumnType::Simple(SimpleColumnType::Text),
                    Some("New comment"),
                ),
            ],
            vec![],
        )];

        let plan = diff_schemas(&from, &to).unwrap();

        assert_eq!(plan.actions.len(), 1);
        assert!(matches!(
            &plan.actions[0],
            MigrationAction::ModifyColumnComment {
                table,
                column,
                new_comment: Some(comment),
            } if table == "users" && column == "email" && comment == "New comment"
        ));
    }

    #[test]
    fn no_change_same_comment() {
        // Column: same comment -> no action
        let from = vec![table(
            "users",
            vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                col_with_comment(
                    "email",
                    ColumnType::Simple(SimpleColumnType::Text),
                    Some("Same comment"),
                ),
            ],
            vec![],
        )];

        let to = vec![table(
            "users",
            vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                col_with_comment(
                    "email",
                    ColumnType::Simple(SimpleColumnType::Text),
                    Some("Same comment"),
                ),
            ],
            vec![],
        )];

        let plan = diff_schemas(&from, &to).unwrap();

        assert!(plan.actions.is_empty());
    }

    #[test]
    fn multiple_columns_comment_changes() {
        // Multiple columns with comment changes
        let from = vec![table(
            "users",
            vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                col_with_comment("email", ColumnType::Simple(SimpleColumnType::Text), None),
                col_with_comment(
                    "name",
                    ColumnType::Simple(SimpleColumnType::Text),
                    Some("User name"),
                ),
                col_with_comment(
                    "phone",
                    ColumnType::Simple(SimpleColumnType::Text),
                    Some("Phone number"),
                ),
            ],
            vec![],
        )];

        let to = vec![table(
            "users",
            vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                col_with_comment(
                    "email",
                    ColumnType::Simple(SimpleColumnType::Text),
                    Some("Email address"),
                ), // None -> "Email address"
                col_with_comment("name", ColumnType::Simple(SimpleColumnType::Text), None), // "User name" -> None
                col_with_comment(
                    "phone",
                    ColumnType::Simple(SimpleColumnType::Text),
                    Some("Phone number"),
                ), // no change
            ],
            vec![],
        )];

        let plan = diff_schemas(&from, &to).unwrap();

        assert_eq!(plan.actions.len(), 2);

        let has_email_change = plan.actions.iter().any(|a| {
            matches!(
                a,
                MigrationAction::ModifyColumnComment {
                    table,
                    column,
                    new_comment: Some(comment),
                } if table == "users" && column == "email" && comment == "Email address"
            )
        });
        assert!(has_email_change, "Should detect email comment added");

        let has_name_change = plan.actions.iter().any(|a| {
            matches!(
                a,
                MigrationAction::ModifyColumnComment {
                    table,
                    column,
                    new_comment: None,
                } if table == "users" && column == "name"
            )
        });
        assert!(has_name_change, "Should detect name comment removed");
    }

    #[test]
    fn comment_change_with_nullable_change() {
        // Column changing both nullable and comment
        let from = vec![table(
            "users",
            vec![col("id", ColumnType::Simple(SimpleColumnType::Integer)), {
                let mut c =
                    col_with_comment("email", ColumnType::Simple(SimpleColumnType::Text), None);
                c.nullable = true;
                c
            }],
            vec![],
        )];

        let to = vec![table(
            "users",
            vec![col("id", ColumnType::Simple(SimpleColumnType::Integer)), {
                let mut c = col_with_comment(
                    "email",
                    ColumnType::Simple(SimpleColumnType::Text),
                    Some("Required email"),
                );
                c.nullable = false;
                c
            }],
            vec![],
        )];

        let plan = diff_schemas(&from, &to).unwrap();

        // Should generate both ModifyColumnNullable and ModifyColumnComment
        assert_eq!(plan.actions.len(), 2);

        let has_nullable_change = plan.actions.iter().any(|a| {
            matches!(
                a,
                MigrationAction::ModifyColumnNullable {
                    table,
                    column,
                    nullable: false,
                    ..
                } if table == "users" && column == "email"
            )
        });
        assert!(has_nullable_change, "Should detect nullable change");

        let has_comment_change = plan.actions.iter().any(|a| {
            matches!(
                a,
                MigrationAction::ModifyColumnComment {
                    table,
                    column,
                    new_comment: Some(comment),
                } if table == "users" && column == "email" && comment == "Required email"
            )
        });
        assert!(has_comment_change, "Should detect comment change");
    }
}

mod nullable_changes {
    use super::*;

    fn col_nullable(name: &str, ty: ColumnType, nullable: bool) -> ColumnDef {
        ColumnDef::new(name, ty, nullable)
    }

    #[test]
    fn column_nullable_to_non_nullable() {
        // Column: nullable -> non-nullable
        let from = vec![table(
            "users",
            vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                col_nullable("email", ColumnType::Simple(SimpleColumnType::Text), true),
            ],
            vec![],
        )];

        let to = vec![table(
            "users",
            vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                col_nullable("email", ColumnType::Simple(SimpleColumnType::Text), false),
            ],
            vec![],
        )];

        let plan = diff_schemas(&from, &to).unwrap();

        assert_eq!(plan.actions.len(), 1);
        assert!(matches!(
            &plan.actions[0],
            MigrationAction::ModifyColumnNullable {
                table,
                column,
                nullable: false,
                fill_with: None,
                delete_null_rows: None,
            } if table == "users" && column == "email"
        ));
    }

    #[test]
    fn column_non_nullable_to_nullable() {
        // Column: non-nullable -> nullable
        let from = vec![table(
            "users",
            vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                col_nullable("email", ColumnType::Simple(SimpleColumnType::Text), false),
            ],
            vec![],
        )];

        let to = vec![table(
            "users",
            vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                col_nullable("email", ColumnType::Simple(SimpleColumnType::Text), true),
            ],
            vec![],
        )];

        let plan = diff_schemas(&from, &to).unwrap();

        assert_eq!(plan.actions.len(), 1);
        assert!(matches!(
            &plan.actions[0],
            MigrationAction::ModifyColumnNullable {
                table,
                column,
                nullable: true,
                fill_with: None,
                delete_null_rows: None,
            } if table == "users" && column == "email"
        ));
    }

    #[test]
    fn multiple_columns_nullable_changes() {
        // Multiple columns changing nullability at once
        let from = vec![table(
            "users",
            vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                col_nullable("email", ColumnType::Simple(SimpleColumnType::Text), true),
                col_nullable("name", ColumnType::Simple(SimpleColumnType::Text), false),
                col_nullable("phone", ColumnType::Simple(SimpleColumnType::Text), true),
            ],
            vec![],
        )];

        let to = vec![table(
            "users",
            vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                col_nullable("email", ColumnType::Simple(SimpleColumnType::Text), false), // nullable -> non-nullable
                col_nullable("name", ColumnType::Simple(SimpleColumnType::Text), true), // non-nullable -> nullable
                col_nullable("phone", ColumnType::Simple(SimpleColumnType::Text), true), // no change
            ],
            vec![],
        )];

        let plan = diff_schemas(&from, &to).unwrap();

        assert_eq!(plan.actions.len(), 2);

        let has_email_change = plan.actions.iter().any(|a| {
            matches!(
                a,
                MigrationAction::ModifyColumnNullable {
                    table,
                    column,
                    nullable: false,
                    ..
                } if table == "users" && column == "email"
            )
        });
        assert!(
            has_email_change,
            "Should detect email nullable -> non-nullable"
        );

        let has_name_change = plan.actions.iter().any(|a| {
            matches!(
                a,
                MigrationAction::ModifyColumnNullable {
                    table,
                    column,
                    nullable: true,
                    ..
                } if table == "users" && column == "name"
            )
        });
        assert!(
            has_name_change,
            "Should detect name non-nullable -> nullable"
        );
    }

    #[test]
    fn nullable_change_with_type_change() {
        // Column changing both type and nullability
        let from = vec![table(
            "users",
            vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                col_nullable("age", ColumnType::Simple(SimpleColumnType::Integer), true),
            ],
            vec![],
        )];

        let to = vec![table(
            "users",
            vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                col_nullable("age", ColumnType::Simple(SimpleColumnType::Text), false),
            ],
            vec![],
        )];

        let plan = diff_schemas(&from, &to).unwrap();

        // Should generate both ModifyColumnType and ModifyColumnNullable
        assert_eq!(plan.actions.len(), 2);

        let has_type_change = plan.actions.iter().any(|a| {
            matches!(
                a,
                MigrationAction::ModifyColumnType { table, column, .. }
                if table == "users" && column == "age"
            )
        });
        assert!(has_type_change, "Should detect type change");

        let has_nullable_change = plan.actions.iter().any(|a| {
            matches!(
                a,
                MigrationAction::ModifyColumnNullable {
                    table,
                    column,
                    nullable: false,
                    ..
                } if table == "users" && column == "age"
            )
        });
        assert!(has_nullable_change, "Should detect nullable change");
    }
}
