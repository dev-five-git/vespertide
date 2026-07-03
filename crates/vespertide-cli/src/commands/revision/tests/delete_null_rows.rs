use super::*;

#[test]
fn test_parse_delete_null_rows_args() {
    let args = vec!["users.email".to_string(), "orders.user_id".to_string()];
    let result = parse_delete_null_rows_args(&args);
    assert_eq!(result.len(), 2);
    assert!(result.contains(&("users".to_string(), "email".to_string())));
    assert!(result.contains(&("orders".to_string(), "user_id".to_string())));
}

#[test]
fn test_parse_delete_null_rows_args_invalid_format() {
    let args = vec!["invalid_no_dot".to_string(), "valid.column".to_string()];
    let result = parse_delete_null_rows_args(&args);
    assert_eq!(result.len(), 1);
    assert!(result.contains(&("valid".to_string(), "column".to_string())));
}

#[test]
fn test_parse_delete_null_rows_args_empty() {
    let result = parse_delete_null_rows_args(&[]);
    assert!(result.is_empty());
}

#[test]
fn test_apply_delete_null_rows_to_plan() {
    use vespertide_core::MigrationPlan;

    let mut plan = MigrationPlan {
        id: String::new(),
        comment: None,
        created_at: None,
        version: 1,
        actions: vec![MigrationAction::ModifyColumnNullable {
            table: "orders".into(),
            column: "user_id".into(),
            nullable: false,
            fill_with: None,
            delete_null_rows: None,
        }],
    };

    let mut delete_set = HashSet::new();
    delete_set.insert(("orders".to_string(), "user_id".to_string()));
    apply_delete_null_rows_to_plan(&mut plan, &delete_set);

    match &plan.actions[0] {
        MigrationAction::ModifyColumnNullable {
            delete_null_rows, ..
        } => {
            assert_eq!(delete_null_rows, &Some(true));
        }
        _ => panic!("Expected ModifyColumnNullable action"),
    }
}

#[test]
fn test_apply_delete_null_rows_to_plan_skips_nullable_true() {
    use vespertide_core::MigrationPlan;

    let mut plan = MigrationPlan {
        id: String::new(),
        comment: None,
        created_at: None,
        version: 1,
        actions: vec![MigrationAction::ModifyColumnNullable {
            table: "orders".into(),
            column: "user_id".into(),
            nullable: true,
            fill_with: None,
            delete_null_rows: None,
        }],
    };

    let mut delete_set = HashSet::new();
    delete_set.insert(("orders".to_string(), "user_id".to_string()));
    apply_delete_null_rows_to_plan(&mut plan, &delete_set);

    match &plan.actions[0] {
        MigrationAction::ModifyColumnNullable {
            delete_null_rows, ..
        } => {
            assert_eq!(delete_null_rows, &None);
        }
        _ => panic!("Expected ModifyColumnNullable action"),
    }
}

#[test]
fn test_apply_delete_null_rows_to_plan_skips_already_set() {
    use vespertide_core::MigrationPlan;

    let mut plan = MigrationPlan {
        id: String::new(),
        comment: None,
        created_at: None,
        version: 1,
        actions: vec![MigrationAction::ModifyColumnNullable {
            table: "orders".into(),
            column: "user_id".into(),
            nullable: false,
            fill_with: None,
            delete_null_rows: Some(false),
        }],
    };

    let mut delete_set = HashSet::new();
    delete_set.insert(("orders".to_string(), "user_id".to_string()));
    apply_delete_null_rows_to_plan(&mut plan, &delete_set);

    match &plan.actions[0] {
        MigrationAction::ModifyColumnNullable {
            delete_null_rows, ..
        } => {
            assert_eq!(delete_null_rows, &Some(false));
        }
        _ => panic!("Expected ModifyColumnNullable action"),
    }
}

#[test]
fn test_apply_delete_null_rows_to_plan_no_match() {
    use vespertide_core::MigrationPlan;

    let mut plan = MigrationPlan {
        id: String::new(),
        comment: None,
        created_at: None,
        version: 1,
        actions: vec![MigrationAction::ModifyColumnNullable {
            table: "orders".into(),
            column: "user_id".into(),
            nullable: false,
            fill_with: None,
            delete_null_rows: None,
        }],
    };

    let mut delete_set = HashSet::new();
    delete_set.insert(("other_table".to_string(), "other_col".to_string()));
    apply_delete_null_rows_to_plan(&mut plan, &delete_set);

    match &plan.actions[0] {
        MigrationAction::ModifyColumnNullable {
            delete_null_rows, ..
        } => {
            assert_eq!(delete_null_rows, &None);
        }
        _ => panic!("Expected ModifyColumnNullable action"),
    }
}

#[test]
fn test_handle_delete_null_rows_fk_accepted() {
    use vespertide_core::MigrationPlan;
    use vespertide_planner::FillWithRequired;

    let mut plan = MigrationPlan {
        id: String::new(),
        comment: None,
        created_at: None,
        version: 1,
        actions: vec![MigrationAction::ModifyColumnNullable {
            table: "orders".into(),
            column: "user_id".into(),
            nullable: false,
            fill_with: None,
            delete_null_rows: None,
        }],
    };

    let mut missing = vec![FillWithRequired {
        action_index: 0,
        table: "orders".to_string(),
        column: "user_id".to_string(),
        action_type: "ModifyColumnNullable",
        column_type: "integer".to_string(),
        default_value: "0".to_string(),
        enum_values: None,
        has_foreign_key: true,
    }];

    let delete_set = HashSet::new();

    let mock_prompt = |_table: &str, _column: &str| -> Result<bool> { Ok(true) };

    let result = handle_delete_null_rows(&mut plan, &mut missing, &delete_set, mock_prompt);
    assert!(result.is_ok());

    assert!(missing.is_empty());

    match &plan.actions[0] {
        MigrationAction::ModifyColumnNullable {
            delete_null_rows, ..
        } => {
            assert_eq!(delete_null_rows, &Some(true));
        }
        _ => panic!("Expected ModifyColumnNullable"),
    }
}

#[test]
fn test_handle_delete_null_rows_fk_declined() {
    use vespertide_core::MigrationPlan;
    use vespertide_planner::FillWithRequired;

    let mut plan = MigrationPlan {
        id: String::new(),
        comment: None,
        created_at: None,
        version: 1,
        actions: vec![MigrationAction::ModifyColumnNullable {
            table: "orders".into(),
            column: "user_id".into(),
            nullable: false,
            fill_with: None,
            delete_null_rows: None,
        }],
    };

    let mut missing = vec![FillWithRequired {
        action_index: 0,
        table: "orders".to_string(),
        column: "user_id".to_string(),
        action_type: "ModifyColumnNullable",
        column_type: "integer".to_string(),
        default_value: "0".to_string(),
        enum_values: None,
        has_foreign_key: true,
    }];

    let delete_set = HashSet::new();

    let mock_prompt = |_table: &str, _column: &str| -> Result<bool> { Ok(false) };

    let result = handle_delete_null_rows(&mut plan, &mut missing, &delete_set, mock_prompt);
    assert!(result.is_ok());

    assert_eq!(missing.len(), 1);
    assert_eq!(missing[0].table, "orders");

    match &plan.actions[0] {
        MigrationAction::ModifyColumnNullable {
            delete_null_rows, ..
        } => {
            assert_eq!(delete_null_rows, &None);
        }
        _ => panic!("Expected ModifyColumnNullable"),
    }
}

#[test]
fn test_handle_delete_null_rows_cli_provided() {
    use vespertide_core::MigrationPlan;
    use vespertide_planner::FillWithRequired;

    let mut plan = MigrationPlan {
        id: String::new(),
        comment: None,
        created_at: None,
        version: 1,
        actions: vec![MigrationAction::ModifyColumnNullable {
            table: "orders".into(),
            column: "user_id".into(),
            nullable: false,
            fill_with: None,
            delete_null_rows: None,
        }],
    };

    let mut missing = vec![FillWithRequired {
        action_index: 0,
        table: "orders".to_string(),
        column: "user_id".to_string(),
        action_type: "ModifyColumnNullable",
        column_type: "integer".to_string(),
        default_value: "0".to_string(),
        enum_values: None,
        has_foreign_key: false,
    }];

    let mut delete_set = HashSet::new();
    delete_set.insert(("orders".to_string(), "user_id".to_string()));

    let mock_prompt = |_table: &str, _column: &str| -> Result<bool> {
        panic!("Should not be called for CLI-provided items");
    };

    let result = handle_delete_null_rows(&mut plan, &mut missing, &delete_set, mock_prompt);
    assert!(result.is_ok());

    assert!(missing.is_empty());

    match &plan.actions[0] {
        MigrationAction::ModifyColumnNullable {
            delete_null_rows, ..
        } => {
            assert_eq!(delete_null_rows, &Some(true));
        }
        _ => panic!("Expected ModifyColumnNullable"),
    }
}

#[test]
fn test_handle_delete_null_rows_non_fk_passthrough() {
    use vespertide_core::MigrationPlan;
    use vespertide_planner::FillWithRequired;

    let mut plan = MigrationPlan {
        id: String::new(),
        comment: None,
        created_at: None,
        version: 1,
        actions: vec![MigrationAction::ModifyColumnNullable {
            table: "users".into(),
            column: "email".into(),
            nullable: false,
            fill_with: None,
            delete_null_rows: None,
        }],
    };

    let mut missing = vec![FillWithRequired {
        action_index: 0,
        table: "users".to_string(),
        column: "email".to_string(),
        action_type: "ModifyColumnNullable",
        column_type: "text".to_string(),
        default_value: "''".to_string(),
        enum_values: None,
        has_foreign_key: false,
    }];

    let delete_set = HashSet::new();

    let mock_prompt = |_table: &str, _column: &str| -> Result<bool> {
        panic!("Should not be called for non-FK items");
    };

    let result = handle_delete_null_rows(&mut plan, &mut missing, &delete_set, mock_prompt);
    assert!(result.is_ok());

    assert_eq!(missing.len(), 1);
    assert_eq!(missing[0].column, "email");

    match &plan.actions[0] {
        MigrationAction::ModifyColumnNullable {
            delete_null_rows, ..
        } => {
            assert_eq!(delete_null_rows, &None);
        }
        _ => panic!("Expected ModifyColumnNullable"),
    }
}

#[test]
fn test_handle_delete_null_rows_prompt_error() {
    use vespertide_core::MigrationPlan;
    use vespertide_planner::FillWithRequired;

    let mut plan = MigrationPlan {
        id: String::new(),
        comment: None,
        created_at: None,
        version: 1,
        actions: vec![MigrationAction::ModifyColumnNullable {
            table: "orders".into(),
            column: "user_id".into(),
            nullable: false,
            fill_with: None,
            delete_null_rows: None,
        }],
    };

    let mut missing = vec![FillWithRequired {
        action_index: 0,
        table: "orders".to_string(),
        column: "user_id".to_string(),
        action_type: "ModifyColumnNullable",
        column_type: "integer".to_string(),
        default_value: "0".to_string(),
        enum_values: None,
        has_foreign_key: true,
    }];

    let delete_set = HashSet::new();

    let mock_prompt =
        |_table: &str, _column: &str| -> Result<bool> { Err(anyhow::anyhow!("user cancelled")) };

    let result = handle_delete_null_rows(&mut plan, &mut missing, &delete_set, mock_prompt);
    assert!(result.is_err());
}
