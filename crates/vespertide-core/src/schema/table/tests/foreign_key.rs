use super::*;
use rstest::rstest;

#[test]
fn normalize_inline_foreign_key_string_syntax() {
    // Test ForeignKeySyntax::String with valid "table.column" format
    let mut user_id_col = col("user_id", ColumnType::Simple(SimpleColumnType::Integer));
    user_id_col.foreign_key = Some(ForeignKeySyntax::String("users.id".into()));

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
            on_delete: None,
            on_update: None,
            ..
        } if columns == &["user_id".to_string()]
            && ref_table == "users"
            && ref_columns == &["id".to_string()]
    ));
}

#[test]
fn normalize_inline_foreign_key_invalid_format_no_dot() {
    // Test ForeignKeySyntax::String with invalid format (no dot)
    let mut user_id_col = col("user_id", ColumnType::Simple(SimpleColumnType::Integer));
    user_id_col.foreign_key = Some(ForeignKeySyntax::String("usersid".into()));

    let table = TableDef {
        name: "posts".into(),
        description: None,
        columns: vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            user_id_col,
        ],
        constraints: vec![],
    };

    let result = table.normalize();
    assert!(result.is_err());
    if let Err(TableValidationError::InvalidForeignKeyFormat { column_name, value }) = result {
        assert_eq!(column_name, "user_id");
        assert_eq!(value, "usersid");
    } else {
        panic!("Expected InvalidForeignKeyFormat error");
    }
}

#[test]
fn normalize_inline_foreign_key_invalid_format_empty_table() {
    // Test ForeignKeySyntax::String with empty table part
    let mut user_id_col = col("user_id", ColumnType::Simple(SimpleColumnType::Integer));
    user_id_col.foreign_key = Some(ForeignKeySyntax::String(".id".into()));

    let table = TableDef {
        name: "posts".into(),
        description: None,
        columns: vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            user_id_col,
        ],
        constraints: vec![],
    };

    let result = table.normalize();
    assert!(result.is_err());
    if let Err(TableValidationError::InvalidForeignKeyFormat { column_name, value }) = result {
        assert_eq!(column_name, "user_id");
        assert_eq!(value, ".id");
    } else {
        panic!("Expected InvalidForeignKeyFormat error");
    }
}

#[test]
fn normalize_inline_foreign_key_invalid_format_empty_column() {
    // Test ForeignKeySyntax::String with empty column part
    let mut user_id_col = col("user_id", ColumnType::Simple(SimpleColumnType::Integer));
    user_id_col.foreign_key = Some(ForeignKeySyntax::String("users.".into()));

    let table = TableDef {
        name: "posts".into(),
        description: None,
        columns: vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            user_id_col,
        ],
        constraints: vec![],
    };

    let result = table.normalize();
    assert!(result.is_err());
    if let Err(TableValidationError::InvalidForeignKeyFormat { column_name, value }) = result {
        assert_eq!(column_name, "user_id");
        assert_eq!(value, "users.");
    } else {
        panic!("Expected InvalidForeignKeyFormat error");
    }
}

#[test]
fn normalize_inline_foreign_key_invalid_format_too_many_parts() {
    // Test ForeignKeySyntax::String with too many parts
    let mut user_id_col = col("user_id", ColumnType::Simple(SimpleColumnType::Integer));
    user_id_col.foreign_key = Some(ForeignKeySyntax::String("schema.users.id".into()));

    let table = TableDef {
        name: "posts".into(),
        description: None,
        columns: vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            user_id_col,
        ],
        constraints: vec![],
    };

    let result = table.normalize();
    assert!(result.is_err());
    if let Err(TableValidationError::InvalidForeignKeyFormat { column_name, value }) = result {
        assert_eq!(column_name, "user_id");
        assert_eq!(value, "schema.users.id");
    } else {
        panic!("Expected InvalidForeignKeyFormat error");
    }
}

#[rstest]
#[case(".")]
#[case(".col")]
#[case("tbl.")]
#[case("..")]
#[case("a.b.c")]
fn normalize_inline_foreign_key_rejects_empty_or_extra_reference_parts(#[case] reference: &str) {
    let mut user_id_col = col("user_id", ColumnType::Simple(SimpleColumnType::Integer));
    user_id_col.foreign_key = Some(ForeignKeySyntax::String(reference.into()));

    let table = TableDef {
        name: "posts".into(),
        description: None,
        columns: vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            user_id_col,
        ],
        constraints: vec![],
    };

    let result = table.normalize();

    assert!(matches!(
        result,
        Err(TableValidationError::InvalidForeignKeyFormat { .. })
    ));
}

#[test]
fn normalize_inline_primary_key_with_auto_increment() {
    use crate::schema::primary_key::PrimaryKeyDef;

    let mut id_col = col("id", ColumnType::Simple(SimpleColumnType::Integer));
    id_col.primary_key = Some(PrimaryKeySyntax::Object(PrimaryKeyDef {
        auto_increment: true,
    }));

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
        TableConstraint::PrimaryKey { auto_increment: true, columns, .. } if columns == &["id".to_string()]
    ));
}

#[test]
fn normalize_duplicate_inline_index_on_same_column() {
    // This test triggers the DuplicateIndexColumn error (lines 251-253)
    // by having the same column appear twice in the same named index group
    use crate::schema::str_or_bool::StrOrBoolOrArray;

    // Create a column that references the same index name twice (via Array)
    let mut email_col = col("email", ColumnType::Simple(SimpleColumnType::Text));
    email_col.index = Some(StrOrBoolOrArray::Array(vec![
        "idx_email".into(),
        "idx_email".into(), // Duplicate reference
    ]));

    let table = TableDef {
        name: "users".into(),
        description: None,
        columns: vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            email_col,
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
        assert_eq!(index_name, "idx_email");
        assert_eq!(column_name, "email");
    } else {
        panic!("Expected DuplicateIndexColumn error, got: {result:?}");
    }
}

#[test]
fn test_invalid_foreign_key_format_error_display() {
    let error = TableValidationError::InvalidForeignKeyFormat {
        column_name: "user_id".into(),
        value: "invalid".into(),
    };
    let error_msg = format!("{error}");
    assert!(error_msg.contains("user_id"));
    assert!(error_msg.contains("invalid"));
    assert!(error_msg.contains("table.column"));
}

#[test]
fn normalize_inline_foreign_key_reference_syntax() {
    // Test ForeignKeySyntax::Reference with { "references": "table.column", "on_delete": ... }
    use crate::schema::foreign_key::ReferenceSyntaxDef;

    let mut user_id_col = col("user_id", ColumnType::Simple(SimpleColumnType::Integer));
    user_id_col.foreign_key = Some(ForeignKeySyntax::Reference(ReferenceSyntaxDef {
        references: "users.id".into(),
        on_delete: Some(ReferenceAction::Cascade),
        on_update: None,
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

#[test]
fn normalize_inline_foreign_key_reference_syntax_invalid_format() {
    // Test ForeignKeySyntax::Reference with invalid format
    use crate::schema::foreign_key::ReferenceSyntaxDef;

    let mut user_id_col = col("user_id", ColumnType::Simple(SimpleColumnType::Integer));
    user_id_col.foreign_key = Some(ForeignKeySyntax::Reference(ReferenceSyntaxDef {
        references: "invalid_no_dot".into(),
        on_delete: None,
        on_update: None,
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

    let result = table.normalize();
    assert!(result.is_err());
    if let Err(TableValidationError::InvalidForeignKeyFormat { column_name, value }) = result {
        assert_eq!(column_name, "user_id");
        assert_eq!(value, "invalid_no_dot");
    } else {
        panic!("Expected InvalidForeignKeyFormat error");
    }
}

#[test]
fn deserialize_table_without_constraints() {
    // Test that constraints field is optional in JSON deserialization
    let json = r#"{
        "name": "users",
        "columns": [
            { "name": "id", "type": "integer", "nullable": false }
        ]
    }"#;

    let table: TableDef = serde_json::from_str(json).unwrap();
    assert_eq!(table.name.as_str(), "users");
    assert!(table.constraints.is_empty());
}

#[test]
fn deserialize_foreign_key_reference_syntax() {
    // Test JSON deserialization of new reference syntax
    let json = r#"{
        "name": "posts",
        "columns": [
            { "name": "id", "type": "integer", "nullable": false },
            {
                "name": "user_id",
                "type": "integer",
                "nullable": false,
                "foreign_key": { "references": "users.id", "on_delete": "cascade" }
            }
        ]
    }"#;

    let table: TableDef = serde_json::from_str(json).unwrap();
    assert_eq!(table.columns.len(), 2);

    let user_id_col = &table.columns[1];
    assert!(user_id_col.foreign_key.is_some());

    if let Some(ForeignKeySyntax::Reference(ref_syntax)) = &user_id_col.foreign_key {
        assert_eq!(ref_syntax.references, "users.id");
        assert_eq!(ref_syntax.on_delete, Some(ReferenceAction::Cascade));
    } else {
        panic!("Expected Reference syntax");
    }
}
