use super::*;

#[test]
fn test_reverse_relations_has_many() {
    use vespertide_core::{ColumnType, SimpleColumnType};

    // user table
    let user = TableDef {
        name: "user".into(),
        description: None,
        columns: vec![ColumnDef {
            name: "id".into(),
            r#type: ColumnType::Simple(SimpleColumnType::Uuid),
            nullable: false,
            default: None,
            comment: None,
            primary_key: None,
            unique: None,
            index: None,
            foreign_key: None,
        }],
        constraints: vec![TableConstraint::PrimaryKey {
            auto_increment: false,
            columns: vec!["id".into()],
            strategy: vespertide_core::PrimaryKeyAdditionStrategy::default(),
        }],
    };

    // post table with FK to user (not PK, so has_many)
    let post = TableDef {
        name: "post".into(),
        description: None,
        columns: vec![
            ColumnDef {
                name: "id".into(),
                r#type: ColumnType::Simple(SimpleColumnType::Uuid),
                nullable: false,
                default: None,
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            },
            ColumnDef {
                name: "user_id".into(),
                r#type: ColumnType::Simple(SimpleColumnType::Uuid),
                nullable: false,
                default: None,
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            },
        ],
        constraints: vec![
            TableConstraint::PrimaryKey {
                auto_increment: false,
                columns: vec!["id".into()],
                strategy: vespertide_core::PrimaryKeyAdditionStrategy::default(),
            },
            TableConstraint::ForeignKey {
                name: None,
                columns: vec!["user_id".into()],
                ref_table: "user".into(),
                ref_columns: vec!["id".into()],
                on_delete: None,
                on_update: None,
                orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
            },
        ],
    };

    let schema = vec![user.clone(), post];

    // Render user with schema context - should have has_many to posts
    let rendered = render_entity_with_schema(&user, &schema);

    assert!(rendered.contains("#[sea_orm(has_many)]"));
    assert!(rendered.contains("HasMany<super::post::Entity>"));
    assert!(rendered.contains("pub posts:")); // pluralized field name
    // has_many should NOT have from/to attributes
    assert!(!rendered.contains("has_many, from"));
}

#[test]
fn test_reverse_relations_has_one() {
    use vespertide_core::{ColumnType, SimpleColumnType};

    // user table
    let user = TableDef {
        name: "user".into(),
        description: None,
        columns: vec![ColumnDef {
            name: "id".into(),
            r#type: ColumnType::Simple(SimpleColumnType::Uuid),
            nullable: false,
            default: None,
            comment: None,
            primary_key: None,
            unique: None,
            index: None,
            foreign_key: None,
        }],
        constraints: vec![TableConstraint::PrimaryKey {
            auto_increment: false,
            columns: vec!["id".into()],
            strategy: vespertide_core::PrimaryKeyAdditionStrategy::default(),
        }],
    };

    // profile table with FK to user that is also the PK (one-to-one)
    let profile = TableDef {
        name: "profile".into(),
        description: None,
        columns: vec![
            ColumnDef {
                name: "user_id".into(),
                r#type: ColumnType::Simple(SimpleColumnType::Uuid),
                nullable: false,
                default: None,
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            },
            ColumnDef {
                name: "bio".into(),
                r#type: ColumnType::Simple(SimpleColumnType::Text),
                nullable: true,
                default: None,
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            },
        ],
        constraints: vec![
            TableConstraint::PrimaryKey {
                auto_increment: false,
                columns: vec!["user_id".into()],
                strategy: vespertide_core::PrimaryKeyAdditionStrategy::default(),
            },
            TableConstraint::ForeignKey {
                name: None,
                columns: vec!["user_id".into()],
                ref_table: "user".into(),
                ref_columns: vec!["id".into()],
                on_delete: None,
                on_update: None,
                orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
            },
        ],
    };

    let schema = vec![user.clone(), profile];

    // Render user with schema context - should have has_one to profile
    let rendered = render_entity_with_schema(&user, &schema);

    assert!(rendered.contains("#[sea_orm(has_one)]"));
    assert!(rendered.contains("HasOne<super::profile::Entity>"));
    assert!(rendered.contains("pub profile:")); // singular field name
    // has_one should NOT have from/to attributes
    assert!(!rendered.contains("has_one, from"));
}

#[test]
fn test_reverse_relations_unique_fk() {
    use vespertide_core::{ColumnType, SimpleColumnType};

    // user table
    let user = TableDef {
        name: "user".into(),
        description: None,
        columns: vec![ColumnDef {
            name: "id".into(),
            r#type: ColumnType::Simple(SimpleColumnType::Uuid),
            nullable: false,
            default: None,
            comment: None,
            primary_key: None,
            unique: None,
            index: None,
            foreign_key: None,
        }],
        constraints: vec![TableConstraint::PrimaryKey {
            auto_increment: false,
            columns: vec!["id".into()],
            strategy: vespertide_core::PrimaryKeyAdditionStrategy::default(),
        }],
    };

    // settings table with unique FK to user (one-to-one via UNIQUE constraint)
    let settings = TableDef {
        name: "settings".into(),
        description: None,
        columns: vec![
            ColumnDef {
                name: "id".into(),
                r#type: ColumnType::Simple(SimpleColumnType::Uuid),
                nullable: false,
                default: None,
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            },
            ColumnDef {
                name: "user_id".into(),
                r#type: ColumnType::Simple(SimpleColumnType::Uuid),
                nullable: false,
                default: None,
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            },
        ],
        constraints: vec![
            TableConstraint::PrimaryKey {
                auto_increment: false,
                columns: vec!["id".into()],
                strategy: vespertide_core::PrimaryKeyAdditionStrategy::default(),
            },
            TableConstraint::ForeignKey {
                name: None,
                columns: vec!["user_id".into()],
                ref_table: "user".into(),
                ref_columns: vec!["id".into()],
                on_delete: None,
                on_update: None,
                orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
            },
            TableConstraint::Unique {
                name: None,
                columns: vec!["user_id".into()],
                strategy: vespertide_core::UniqueConstraintStrategy::DeleteDuplicates {
                    keep: vespertide_core::KeepPolicy::First,
                },
            },
        ],
    };

    let schema = vec![user.clone(), settings];

    // Render user with schema context - should have has_one (because of UNIQUE)
    let rendered = render_entity_with_schema(&user, &schema);

    assert!(rendered.contains("#[sea_orm(has_one)]"));
    assert!(rendered.contains("HasOne<super::settings::Entity>"));
    assert!(rendered.contains("pub settings:")); // singular field name
    // has_one should NOT have from/to attributes
    assert!(!rendered.contains("has_one, from"));
}
