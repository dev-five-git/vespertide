use super::*;

#[test]
fn test_boolean_default_value_with_bool_type() {
    use vespertide_core::schema::primary_key::PrimaryKeySyntax;
    let table = TableDef {
        name: "settings".into(),
        description: None,
        columns: vec![
            ColumnDef {
                name: "id".into(),
                r#type: ColumnType::Simple(SimpleColumnType::Integer),
                nullable: false,
                default: None,
                comment: None,
                primary_key: Some(PrimaryKeySyntax::Bool(true)),
                unique: None,
                index: None,
                foreign_key: None,
            },
            ColumnDef {
                name: "is_active".into(),
                r#type: ColumnType::Simple(SimpleColumnType::Boolean),
                nullable: false,
                default: Some(StringOrBool::Bool(true)),
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            },
            ColumnDef {
                name: "is_deleted".into(),
                r#type: ColumnType::Simple(SimpleColumnType::Boolean),
                nullable: false,
                default: Some(StringOrBool::Bool(false)),
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            },
        ],
        constraints: vec![],
    };
    let rendered = render_entity(&table);
    assert!(rendered.contains("default_value = true"));
    assert!(rendered.contains("default_value = false"));
}

#[test]
fn test_exporter_with_config_render_entity() {
    use vespertide_core::schema::primary_key::PrimaryKeySyntax;

    let mut config = SeaOrmConfig::default();
    config.extra_enum_derives = vec!["CustomDerive".to_string()];
    config.extra_model_derives = vec!["ModelDerive".to_string()];
    let exporter = SeaOrmExporterWithConfig::new(&config, "");

    let table = TableDef {
        name: "items".into(),
        description: None,
        columns: vec![ColumnDef {
            name: "id".into(),
            r#type: ColumnType::Simple(SimpleColumnType::Integer),
            nullable: false,
            default: None,
            comment: None,
            primary_key: Some(PrimaryKeySyntax::Bool(true)),
            unique: None,
            index: None,
            foreign_key: None,
        }],
        constraints: vec![],
    };

    let result = exporter.render_entity(&table).unwrap();
    assert!(result.contains("ModelDerive"));
}

#[test]
fn test_exporter_with_config_render_entity_with_enum() {
    use vespertide_core::schema::primary_key::PrimaryKeySyntax;

    let mut config = SeaOrmConfig::default();
    config.extra_enum_derives = vec!["CustomEnumDerive".to_string()];
    config.extra_model_derives = vec![];
    let exporter = SeaOrmExporterWithConfig::new(&config, "");

    let table = TableDef {
        name: "orders".into(),
        description: None,
        columns: vec![
            ColumnDef {
                name: "id".into(),
                r#type: ColumnType::Simple(SimpleColumnType::Integer),
                nullable: false,
                default: None,
                comment: None,
                primary_key: Some(PrimaryKeySyntax::Bool(true)),
                unique: None,
                index: None,
                foreign_key: None,
            },
            ColumnDef {
                name: "status".into(),
                r#type: ColumnType::Complex(ComplexColumnType::Enum {
                    name: "order_status".into(),
                    values: EnumValues::String(vec!["pending".into(), "shipped".into()]),
                }),
                nullable: false,
                default: None,
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            },
        ],
        constraints: vec![],
    };

    let result = exporter.render_entity(&table).unwrap();
    assert!(result.contains("CustomEnumDerive"));
}

#[test]
fn test_exporter_with_config_render_entity_with_schema() {
    use vespertide_core::schema::primary_key::PrimaryKeySyntax;

    let mut config = SeaOrmConfig::default();
    config.extra_enum_derives = vec![];
    config.extra_model_derives = vec!["SchemaDerive".to_string()];
    let exporter = SeaOrmExporterWithConfig::new(&config, "");

    let table = TableDef {
        name: "users".into(),
        description: None,
        columns: vec![ColumnDef {
            name: "id".into(),
            r#type: ColumnType::Simple(SimpleColumnType::Integer),
            nullable: false,
            default: None,
            comment: None,
            primary_key: Some(PrimaryKeySyntax::Bool(true)),
            unique: None,
            index: None,
            foreign_key: None,
        }],
        constraints: vec![],
    };

    let schema = vec![table.clone()];
    let result = exporter.render_entity_with_schema(&table, &schema).unwrap();
    assert!(result.contains("SchemaDerive"));
}

#[test]
fn test_exporter_with_empty_extra_derives() {
    use vespertide_core::schema::primary_key::PrimaryKeySyntax;

    let mut config = SeaOrmConfig::default();
    config.extra_enum_derives = vec![];
    config.extra_model_derives = vec![];
    let exporter = SeaOrmExporterWithConfig::new(&config, "");

    let table = TableDef {
        name: "products".into(),
        description: None,
        columns: vec![
            ColumnDef {
                name: "id".into(),
                r#type: ColumnType::Simple(SimpleColumnType::Integer),
                nullable: false,
                default: None,
                comment: None,
                primary_key: Some(PrimaryKeySyntax::Bool(true)),
                unique: None,
                index: None,
                foreign_key: None,
            },
            ColumnDef {
                name: "category".into(),
                r#type: ColumnType::Complex(ComplexColumnType::Enum {
                    name: "category".into(),
                    values: EnumValues::String(vec!["electronics".into(), "clothing".into()]),
                }),
                nullable: false,
                default: None,
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            },
        ],
        constraints: vec![],
    };

    let result = exporter.render_entity(&table).unwrap();
    // Should have base derives but no extra ones
    assert!(result.contains("DeriveActiveEnum"));
    assert!(result.contains("DeriveEntityModel"));
    // Should NOT contain vespera::Schema since we explicitly set empty
    assert!(!result.contains("vespera::Schema"));
}

#[test]
fn test_doc_comments_from_description_and_comment() {
    use vespertide_core::schema::primary_key::PrimaryKeySyntax;

    let table = TableDef {
        name: "users".into(),
        description: Some("User account information table".into()),
        columns: vec![
            ColumnDef {
                name: "id".into(),
                r#type: ColumnType::Simple(SimpleColumnType::Integer),
                nullable: false,
                default: None,
                comment: Some("Unique user identifier".into()),
                primary_key: Some(PrimaryKeySyntax::Bool(true)),
                unique: None,
                index: None,
                foreign_key: None,
            },
            ColumnDef {
                name: "email".into(),
                r#type: ColumnType::Simple(SimpleColumnType::Text),
                nullable: false,
                default: None,
                comment: Some("User's email address for login".into()),
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            },
            ColumnDef {
                name: "name".into(),
                r#type: ColumnType::Simple(SimpleColumnType::Text),
                nullable: true,
                default: None,
                comment: None, // No comment
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            },
        ],
        constraints: vec![],
    };

    let rendered = render_entity(&table);

    // Check table description as doc comment
    assert!(rendered.contains("/// User account information table"));

    // Check column comments as doc comments
    assert!(rendered.contains("/// Unique user identifier"));
    assert!(rendered.contains("/// User's email address for login"));

    // name column has no comment, so no doc comment for it
    assert!(!rendered.contains("/// name"));
}

#[test]
fn test_multiline_doc_comments() {
    use vespertide_core::schema::primary_key::PrimaryKeySyntax;

    let table = TableDef {
        name: "posts".into(),
        description: Some("Blog posts table\nContains all user-submitted content".into()),
        columns: vec![ColumnDef {
            name: "content".into(),
            r#type: ColumnType::Simple(SimpleColumnType::Text),
            nullable: false,
            default: None,
            comment: Some("Post content body\nSupports markdown format".into()),
            primary_key: Some(PrimaryKeySyntax::Bool(true)),
            unique: None,
            index: None,
            foreign_key: None,
        }],
        constraints: vec![],
    };

    let rendered = render_entity(&table);

    // Check multiline table description
    assert!(rendered.contains("/// Blog posts table"));
    assert!(rendered.contains("/// Contains all user-submitted content"));

    // Check multiline column comment
    assert!(rendered.contains("/// Post content body"));
    assert!(rendered.contains("/// Supports markdown format"));
}

#[test]
fn test_exporter_with_prefix() {
    use vespertide_core::schema::primary_key::PrimaryKeySyntax;

    let config = SeaOrmConfig::default();
    let exporter = SeaOrmExporterWithConfig::new(&config, "myapp_");

    let table = TableDef {
        name: "users".into(),
        description: None,
        columns: vec![ColumnDef {
            name: "id".into(),
            r#type: ColumnType::Simple(SimpleColumnType::Integer),
            nullable: false,
            default: None,
            comment: None,
            primary_key: Some(PrimaryKeySyntax::Bool(true)),
            unique: None,
            index: None,
            foreign_key: None,
        }],
        constraints: vec![],
    };

    let result = exporter.render_entity(&table).unwrap();
    // Should have prefixed table name
    assert!(result.contains("#[sea_orm(table_name = \"myapp_users\")]"));
}

#[test]
fn test_exporter_without_prefix() {
    use vespertide_core::schema::primary_key::PrimaryKeySyntax;

    let config = SeaOrmConfig::default();
    let exporter = SeaOrmExporterWithConfig::new(&config, "");

    let table = TableDef {
        name: "users".into(),
        description: None,
        columns: vec![ColumnDef {
            name: "id".into(),
            r#type: ColumnType::Simple(SimpleColumnType::Integer),
            nullable: false,
            default: None,
            comment: None,
            primary_key: Some(PrimaryKeySyntax::Bool(true)),
            unique: None,
            index: None,
            foreign_key: None,
        }],
        constraints: vec![],
    };

    let result = exporter.render_entity(&table).unwrap();
    // Should have original table name without prefix
    assert!(result.contains("#[sea_orm(table_name = \"users\")]"));
}

#[test]
fn test_junction_relation_enum_without_via_when_entity_appears_multiple_times() {
    use vespertide_core::schema::primary_key::PrimaryKeySyntax;

    // user has a forward FK to user_tag (composite FK), making user_tag appear
    // in both forward and reverse targets => entity_count > 1 for user_tag.
    // The junction table entry from collect_many_to_many_relations has via=None, via_rel=None,
    // so when needs_relation_enum is true, it hits the branch with only relation_enum (no via/via_rel).
    let user = TableDef {
        name: "user".into(),
        description: None,
        columns: vec![
            ColumnDef {
                name: "id".into(),
                r#type: ColumnType::Simple(SimpleColumnType::Integer),
                nullable: false,
                default: None,
                comment: None,
                primary_key: Some(PrimaryKeySyntax::Bool(true)),
                unique: None,
                index: None,
                foreign_key: None,
            },
            ColumnDef {
                name: "pinned_user_id".into(),
                r#type: ColumnType::Simple(SimpleColumnType::Integer),
                nullable: true,
                default: None,
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            },
            ColumnDef {
                name: "pinned_tag_id".into(),
                r#type: ColumnType::Simple(SimpleColumnType::Integer),
                nullable: true,
                default: None,
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            },
        ],
        constraints: vec![TableConstraint::ForeignKey {
            name: None,
            columns: vec!["pinned_user_id".into(), "pinned_tag_id".into()],
            ref_table: "user_tag".into(),
            ref_columns: vec!["user_id".into(), "tag_id".into()],
            on_delete: None,
            on_update: None,
            orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
        }],
    };

    let user_tag = TableDef {
        name: "user_tag".into(),
        description: None,
        columns: vec![
            ColumnDef {
                name: "user_id".into(),
                r#type: ColumnType::Simple(SimpleColumnType::Integer),
                nullable: false,
                default: None,
                comment: None,
                primary_key: Some(PrimaryKeySyntax::Bool(true)),
                unique: None,
                index: None,
                foreign_key: None,
            },
            ColumnDef {
                name: "tag_id".into(),
                r#type: ColumnType::Simple(SimpleColumnType::Integer),
                nullable: false,
                default: None,
                comment: None,
                primary_key: Some(PrimaryKeySyntax::Bool(true)),
                unique: None,
                index: None,
                foreign_key: None,
            },
        ],
        constraints: vec![
            TableConstraint::ForeignKey {
                name: None,
                columns: vec!["user_id".into()],
                ref_table: "user".into(),
                ref_columns: vec!["id".into()],
                on_delete: None,
                on_update: None,
                orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
            },
            TableConstraint::ForeignKey {
                name: None,
                columns: vec!["tag_id".into()],
                ref_table: "tag".into(),
                ref_columns: vec!["id".into()],
                on_delete: None,
                on_update: None,
                orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
            },
        ],
    };

    let tag = TableDef {
        name: "tag".into(),
        description: None,
        columns: vec![ColumnDef {
            name: "id".into(),
            r#type: ColumnType::Simple(SimpleColumnType::Integer),
            nullable: false,
            default: None,
            comment: None,
            primary_key: Some(PrimaryKeySyntax::Bool(true)),
            unique: None,
            index: None,
            foreign_key: None,
        }],
        constraints: vec![],
    };

    let schema = vec![user.clone(), user_tag, tag];
    let rendered = render_entity_with_schema(&user, &schema);

    // The junction table "user_tag" appears in both forward (composite FK) and reverse (M2M junction),
    // so it gets relation_enum without via/via_rel
    assert!(rendered.contains("relation_enum"));
    // Verify we have a has_many to user_tag with relation_enum but no via
    let has_user_tag_relation_enum_without_via = rendered.lines().any(|line| {
        line.contains("has_many") && line.contains("relation_enum") && !line.contains("via")
    });
    assert!(
        has_user_tag_relation_enum_without_via,
        "Expected has_many with relation_enum but no via for junction table entity, got:\n{rendered}"
    );
}
