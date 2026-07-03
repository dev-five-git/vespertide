use super::*;
use insta::assert_debug_snapshot;
use std::collections::BTreeMap;

#[test]
fn diff_created_tables_reports_missing_original_table_instead_of_panicking() {
    let mut actions = Vec::new();
    let from_map = BTreeMap::new();
    let ghost = table(
        "ghost",
        vec![ColumnDef {
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
        vec![],
    );
    let to_map = BTreeMap::from([("ghost", &ghost)]);
    let to_original_map = BTreeMap::new();

    let err = super::super::tables::diff_created_tables(
        &mut actions,
        &from_map,
        &to_map,
        &to_original_map,
    )
    .unwrap_err();

    assert!(matches!(err, PlannerError::TableValidation(_)));
    assert!(
        err.to_string()
            .contains("normalized table 'ghost' missing original table")
    );
    assert!(actions.is_empty());
}

#[test]
fn create_table_with_inline_index() {
    let base = [table(
        "users",
        vec![
            ColumnDef {
                name: "id".into(),
                r#type: ColumnType::Simple(SimpleColumnType::Integer),
                nullable: false,
                default: None,
                comment: None,
                primary_key: Some(PrimaryKeySyntax::Bool(true)),
                unique: None,
                index: Some(StrOrBoolOrArray::Bool(false)),
                foreign_key: None,
            },
            ColumnDef {
                name: "name".into(),
                r#type: ColumnType::Simple(SimpleColumnType::Text),
                nullable: true,
                default: None,
                comment: None,
                primary_key: None,
                unique: Some(StrOrBoolOrArray::Bool(true)),
                index: Some(StrOrBoolOrArray::Bool(true)),
                foreign_key: None,
            },
        ],
        vec![],
    )];
    let plan = diff_schemas(&[], &base).unwrap();

    assert_eq!(plan.actions.len(), 1);
    assert_debug_snapshot!(plan.actions);

    let plan = diff_schemas(
        &base,
        &[table(
            "users",
            vec![
                ColumnDef {
                    name: "id".into(),
                    r#type: ColumnType::Simple(SimpleColumnType::Integer),
                    nullable: false,
                    default: None,
                    comment: None,
                    primary_key: Some(PrimaryKeySyntax::Bool(true)),
                    unique: None,
                    index: Some(StrOrBoolOrArray::Bool(false)),
                    foreign_key: None,
                },
                ColumnDef {
                    name: "name".into(),
                    r#type: ColumnType::Simple(SimpleColumnType::Text),
                    nullable: true,
                    default: None,
                    comment: None,
                    primary_key: None,
                    unique: Some(StrOrBoolOrArray::Bool(true)),
                    index: Some(StrOrBoolOrArray::Bool(false)),
                    foreign_key: None,
                },
            ],
            vec![],
        )],
    )
    .unwrap();

    assert_eq!(plan.actions.len(), 1);
    assert_debug_snapshot!(plan.actions);
}

#[rstest]
#[case(
        "add_index",
        vec![table(
            "users",
            vec![
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
            ],
            vec![],
        )],
        vec![table(
            "users",
            vec![
                ColumnDef {
                    name: "id".into(),
                    r#type: ColumnType::Simple(SimpleColumnType::Integer),
                    nullable: false,
                    default: None,
                    comment: None,
                    primary_key: Some(PrimaryKeySyntax::Bool(true)),
                    unique: None,
                    index: Some(StrOrBoolOrArray::Bool(true)),
                    foreign_key: None,
                },
            ],
            vec![],
        )],
    )]
#[case(
        "remove_index",
        vec![table(
            "users",
            vec![
                ColumnDef {
                    name: "id".into(),
                    r#type: ColumnType::Simple(SimpleColumnType::Integer),
                    nullable: false,
                    default: None,
                    comment: None,
                    primary_key: Some(PrimaryKeySyntax::Bool(true)),
                    unique: None,
                    index: Some(StrOrBoolOrArray::Bool(true)),
                    foreign_key: None,
                },
            ],
            vec![],
        )],
        vec![table(
            "users",
            vec![
                ColumnDef {
                    name: "id".into(),
                    r#type: ColumnType::Simple(SimpleColumnType::Integer),
                    nullable: false,
                    default: None,
                    comment: None,
                    primary_key: Some(PrimaryKeySyntax::Bool(true)),
                    unique: None,
                    index: Some(StrOrBoolOrArray::Bool(false)),
                    foreign_key: None,
                },
            ],
            vec![],
        )],
    )]
#[case(
        "add_named_index",
        vec![table(
            "users",
            vec![
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
            ],
            vec![],
        )],
        vec![table(
            "users",
            vec![
                ColumnDef {
                    name: "id".into(),
                    r#type: ColumnType::Simple(SimpleColumnType::Integer),
                    nullable: false,
                    default: None,
                    comment: None,
                    primary_key: Some(PrimaryKeySyntax::Bool(true)),
                    unique: None,
                    index: Some(StrOrBoolOrArray::Str("hello".to_string())),
                    foreign_key: None,
                },
            ],
            vec![],
        )],
    )]
#[case(
        "remove_named_index",
        vec![table(
            "users",
            vec![
                ColumnDef {
                    name: "id".into(),
                    r#type: ColumnType::Simple(SimpleColumnType::Integer),
                    nullable: false,
                    default: None,
                    comment: None,
                    primary_key: Some(PrimaryKeySyntax::Bool(true)),
                    unique: None,
                    index: Some(StrOrBoolOrArray::Str("hello".to_string())),
                    foreign_key: None,
                },
            ],
            vec![],
        )],
        vec![table(
            "users",
            vec![
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
            ],
            vec![],
        )],
    )]
fn diff_tables(#[case] name: &str, #[case] base: Vec<TableDef>, #[case] to: Vec<TableDef>) {
    use insta::with_settings;

    let plan = diff_schemas(&base, &to).unwrap();
    with_settings!({ snapshot_suffix => name }, {
        assert_debug_snapshot!(plan.actions);
    });
}
