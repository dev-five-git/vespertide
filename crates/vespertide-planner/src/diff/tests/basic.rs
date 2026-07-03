use super::*;

#[rstest]
#[case::add_column_and_index(
    vec![table(
        "users",
        vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
        vec![],
    )],
    vec![table(
        "users",
        vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            col("name", ColumnType::Simple(SimpleColumnType::Text)),
        ],
        vec![idx("ix_users__name", vec!["name"])],
    )],
    vec![
        MigrationAction::AddColumn {
            table: "users".into(),
            column: Box::new(col("name", ColumnType::Simple(SimpleColumnType::Text))),
            fill_with: None,
        },
        MigrationAction::AddConstraint {
            table: "users".into(),
            constraint: idx("ix_users__name", vec!["name"]),
        },
    ]
)]
#[case::drop_table(
    vec![table(
        "users",
        vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
        vec![],
    )],
    vec![],
    vec![MigrationAction::DeleteTable {
        table: "users".into()
    }]
)]
#[case::add_table_with_index(
    vec![],
    vec![table(
        "users",
        vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
        vec![idx("idx_users_id", vec!["id"])],
    )],
    vec![
        MigrationAction::CreateTable {
            table: "users".into(),
            columns: vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
            constraints: vec![idx("idx_users_id", vec!["id"])],
        },
    ]
)]
#[case::delete_column(
    vec![table(
        "users",
        vec![col("id", ColumnType::Simple(SimpleColumnType::Integer)), col("name", ColumnType::Simple(SimpleColumnType::Text))],
        vec![],
    )],
    vec![table(
        "users",
        vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
        vec![],
    )],
    vec![MigrationAction::DeleteColumn {
        table: "users".into(),
        column: "name".into(),
    }]
)]
#[case::modify_column_type(
    vec![table(
        "users",
        vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
        vec![],
    )],
    vec![table(
        "users",
        vec![col("id", ColumnType::Simple(SimpleColumnType::Text))],
        vec![],
    )],
    vec![MigrationAction::ModifyColumnType {
        table: "users".into(),
        column: "id".into(),
        new_type: ColumnType::Simple(SimpleColumnType::Text),
        fill_with: None,
        narrowing_strategy: None,
        timezone: None,
    }]
)]
#[case::remove_index(
    vec![table(
        "users",
        vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
        vec![idx("idx_users_id", vec!["id"])],
    )],
    vec![table(
        "users",
        vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
        vec![],
    )],
    vec![MigrationAction::RemoveConstraint {
        table: "users".into(),
        constraint: idx("idx_users_id", vec!["id"]),
    }]
)]
#[case::add_index_existing_table(
    vec![table(
        "users",
        vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
        vec![],
    )],
    vec![table(
        "users",
        vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
        vec![idx("idx_users_id", vec!["id"])],
    )],
    vec![MigrationAction::AddConstraint {
        table: "users".into(),
        constraint: idx("idx_users_id", vec!["id"]),
    }]
)]
fn diff_schemas_detects_additions(
    #[case] from_schema: Vec<TableDef>,
    #[case] to_schema: Vec<TableDef>,
    #[case] expected_actions: Vec<MigrationAction>,
) {
    let plan = diff_schemas(&from_schema, &to_schema).unwrap();
    assert_eq!(plan.actions, expected_actions);
}
