use super::*;

#[test]
fn test_format_type_info_with_type_and_default() {
    let result = format_type_info("integer", "0");
    assert_eq!(result, " (integer, default: 0)");
}

#[test]
fn test_format_type_info_with_type_only() {
    let result = format_type_info("text", "''");
    assert_eq!(result, " (text, default: '')");
}

#[test]
fn test_format_fill_with_item() {
    let result = format_fill_with_item("users", "email", " (Text)", "AddColumn");
    // The result should contain the table, column, type info, and action type
    // Colors make exact matching difficult, but we can check structure
    assert!(result.contains("users"));
    assert!(result.contains("email"));
    assert!(result.contains("(Text)"));
    assert!(result.contains("AddColumn"));
    assert!(result.contains("Action:"));
}

#[test]
fn test_format_fill_with_item_empty_type_info() {
    let result = format_fill_with_item("orders", "status", "", "ModifyColumnNullable");
    assert!(result.contains("orders"));
    assert!(result.contains("status"));
    assert!(result.contains("ModifyColumnNullable"));
}

#[test]
fn test_format_fill_with_prompt() {
    let result = format_fill_with_prompt("users", "email");
    assert!(result.contains("Enter fill value for"));
    assert!(result.contains("users"));
    assert!(result.contains("email"));
}

#[test]
fn test_print_fill_with_item_and_get_prompt() {
    // This function prints to stdout and returns the prompt string
    let prompt = print_fill_with_item_and_get_prompt("users", "email", "text", "''", "AddColumn");
    assert!(prompt.contains("Enter fill value for"));
    assert!(prompt.contains("users"));
    assert!(prompt.contains("email"));
}

#[test]
fn test_print_fill_with_item_and_get_prompt_no_default() {
    let prompt = print_fill_with_item_and_get_prompt(
        "orders",
        "status",
        "text",
        "''",
        "ModifyColumnNullable",
    );
    assert!(prompt.contains("Enter fill value for"));
    assert!(prompt.contains("orders"));
    assert!(prompt.contains("status"));
}

#[test]
fn test_print_fill_with_item_and_get_prompt_with_default() {
    let prompt = print_fill_with_item_and_get_prompt("users", "age", "integer", "0", "AddColumn");
    assert!(prompt.contains("Enter fill value for"));
    assert!(prompt.contains("users"));
    assert!(prompt.contains("age"));
}

#[test]
fn test_print_fill_with_header() {
    // Just verify it doesn't panic - output goes to stdout
    print_fill_with_header();
}

#[test]
fn test_print_fill_with_footer() {
    // Just verify it doesn't panic - output goes to stdout
    print_fill_with_footer();
}

// Mock enum prompt function for tests - returns first enum value quoted
fn mock_enum_prompt(_prompt: &str, values: &[String]) -> Result<String> {
    let first = values
        .first()
        .ok_or_else(|| anyhow::anyhow!("mock enum prompt requires at least one value"))?;
    Ok(format!("'{first}'"))
}

#[test]
fn test_collect_fill_with_values_single_item() {
    use vespertide_planner::FillWithRequired;

    let missing = vec![FillWithRequired {
        action_index: 0,
        table: "users".to_string(),
        column: "email".to_string(),
        action_type: "AddColumn",
        column_type: "text".to_string(),
        default_value: "''".to_string(),
        enum_values: None,
        has_foreign_key: false,
    }];

    let mut fill_values = HashMap::new();

    // Mock prompt function that returns a fixed value
    let mock_prompt =
        |_prompt: &str, _default: &str| -> Result<String> { Ok("'test@example.com'".to_string()) };

    let result =
        collect_fill_with_values(&missing, &mut fill_values, mock_prompt, mock_enum_prompt);
    assert!(result.is_ok());
    assert_eq!(fill_values.len(), 1);
    assert_eq!(
        fill_values.get(&("users".to_string(), "email".to_string())),
        Some(&"'test@example.com'".to_string())
    );
}

#[test]
fn test_collect_fill_with_values_multiple_items() {
    use vespertide_planner::FillWithRequired;

    let missing = vec![
        FillWithRequired {
            action_index: 0,
            table: "users".to_string(),
            column: "email".to_string(),
            action_type: "AddColumn",
            column_type: "text".to_string(),
            default_value: "''".to_string(),
            enum_values: None,
            has_foreign_key: false,
        },
        FillWithRequired {
            action_index: 1,
            table: "orders".to_string(),
            column: "status".to_string(),
            action_type: "ModifyColumnNullable",
            column_type: "text".to_string(),
            default_value: "''".to_string(),
            enum_values: None,
            has_foreign_key: false,
        },
    ];

    let mut fill_values = HashMap::new();

    // Mock prompt function that returns different values based on call count
    let call_count = std::cell::RefCell::new(0);
    let mock_prompt = |_prompt: &str, _default: &str| -> Result<String> {
        let mut count = call_count.borrow_mut();
        *count += 1;
        match *count {
            1 => Ok("'user@example.com'".to_string()),
            2 => Ok("'pending'".to_string()),
            _ => Ok("'default'".to_string()),
        }
    };

    let result =
        collect_fill_with_values(&missing, &mut fill_values, mock_prompt, mock_enum_prompt);
    assert!(result.is_ok());
    assert_eq!(fill_values.len(), 2);
    assert_eq!(
        fill_values.get(&("users".to_string(), "email".to_string())),
        Some(&"'user@example.com'".to_string())
    );
    assert_eq!(
        fill_values.get(&("orders".to_string(), "status".to_string())),
        Some(&"'pending'".to_string())
    );
}

#[test]
fn test_collect_fill_with_values_empty() {
    let missing: Vec<vespertide_planner::FillWithRequired> = vec![];
    let mut fill_values = HashMap::new();

    // This function should handle empty list gracefully (though it won't be called in practice)
    // But we can't test the header/footer without items since the function still prints them
    // So we test with a mock that would fail if called
    let mock_prompt = |_prompt: &str, _default: &str| -> Result<String> {
        panic!("Should not be called for empty list");
    };

    // Note: The function still prints header/footer even for empty list
    // This is a design choice - in practice, cmd_revision won't call this with empty list
    let result =
        collect_fill_with_values(&missing, &mut fill_values, mock_prompt, mock_enum_prompt);
    assert!(result.is_ok());
    assert!(fill_values.is_empty());
}

#[test]
fn test_collect_fill_with_values_prompt_error() {
    use vespertide_planner::FillWithRequired;

    let missing = vec![FillWithRequired {
        action_index: 0,
        table: "users".to_string(),
        column: "email".to_string(),
        action_type: "AddColumn",
        column_type: "text".to_string(),
        default_value: "''".to_string(),
        enum_values: None,
        has_foreign_key: false,
    }];

    let mut fill_values = HashMap::new();

    // Mock prompt function that returns an error
    let mock_prompt = |_prompt: &str, _default: &str| -> Result<String> {
        Err(anyhow::anyhow!("input cancelled"))
    };

    let result =
        collect_fill_with_values(&missing, &mut fill_values, mock_prompt, mock_enum_prompt);
    assert!(result.is_err());
    assert!(fill_values.is_empty());
}

#[test]
fn test_prompt_fill_with_value_function_exists() {
    // This test verifies that prompt_fill_with_value has the correct signature.
    // We cannot actually call it in tests because dialoguer::Input blocks waiting for terminal input.
    // The function is excluded from coverage with #[cfg(not(tarpaulin_include))].
    let _: fn(&str, &str) -> Result<String> = prompt_fill_with_value;
}

#[test]
fn test_handle_missing_fill_with_collects_and_applies() {
    use vespertide_core::MigrationPlan;

    let mut plan = MigrationPlan {
        id: String::new(),
        comment: None,
        created_at: None,
        version: 1,
        actions: vec![MigrationAction::AddColumn {
            table: "users".into(),
            column: Box::new(ColumnDef {
                name: "email".into(),
                r#type: ColumnType::Simple(SimpleColumnType::Text),
                nullable: false,
                default: None,
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            }),
            fill_with: None,
        }],
    };

    let mut fill_values = HashMap::new();

    // Mock prompt function
    let mock_prompt =
        |_prompt: &str, _default: &str| -> Result<String> { Ok("'test@example.com'".to_string()) };

    let result = handle_missing_fill_with(
        &mut plan,
        &mut fill_values,
        &[],
        mock_prompt,
        mock_enum_prompt,
    );
    assert!(result.is_ok());

    // Verify fill_with was applied to the plan
    match &plan.actions[0] {
        MigrationAction::AddColumn { fill_with, .. } => {
            assert_eq!(fill_with, &Some("'test@example.com'".to_string()));
        }
        _ => panic!("Expected AddColumn action"),
    }

    // Verify fill_values map was updated
    assert_eq!(
        fill_values.get(&("users".to_string(), "email".to_string())),
        Some(&"'test@example.com'".to_string())
    );
}

#[test]
fn test_handle_missing_fill_with_no_missing() {
    use vespertide_core::MigrationPlan;

    // Plan with no missing fill_with values (nullable column)
    let mut plan = MigrationPlan {
        id: String::new(),
        comment: None,
        created_at: None,
        version: 1,
        actions: vec![MigrationAction::AddColumn {
            table: "users".into(),
            column: Box::new(ColumnDef {
                name: "email".into(),
                r#type: ColumnType::Simple(SimpleColumnType::Text),
                nullable: true, // nullable, so no fill_with required
                default: None,
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            }),
            fill_with: None,
        }],
    };

    let mut fill_values = HashMap::new();

    // Mock prompt that should never be called
    let mock_prompt = |_prompt: &str, _default: &str| -> Result<String> {
        panic!("Should not be called when no missing fill_with values");
    };

    let result = handle_missing_fill_with(
        &mut plan,
        &mut fill_values,
        &[],
        mock_prompt,
        mock_enum_prompt,
    );
    assert!(result.is_ok());
    assert!(fill_values.is_empty());
}

#[test]
fn test_handle_missing_fill_with_prompt_error() {
    use vespertide_core::MigrationPlan;

    let mut plan = MigrationPlan {
        id: String::new(),
        comment: None,
        created_at: None,
        version: 1,
        actions: vec![MigrationAction::AddColumn {
            table: "users".into(),
            column: Box::new(ColumnDef {
                name: "email".into(),
                r#type: ColumnType::Simple(SimpleColumnType::Text),
                nullable: false,
                default: None,
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            }),
            fill_with: None,
        }],
    };

    let mut fill_values = HashMap::new();

    // Mock prompt that returns an error
    let mock_prompt = |_prompt: &str, _default: &str| -> Result<String> {
        Err(anyhow::anyhow!("user cancelled"))
    };

    let result = handle_missing_fill_with(
        &mut plan,
        &mut fill_values,
        &[],
        mock_prompt,
        mock_enum_prompt,
    );
    assert!(result.is_err());

    // Plan should not be modified on error
    match &plan.actions[0] {
        MigrationAction::AddColumn { fill_with, .. } => {
            assert_eq!(fill_with, &None);
        }
        _ => panic!("Expected AddColumn action"),
    }
}

#[test]
fn test_handle_missing_fill_with_multiple_columns() {
    use vespertide_core::MigrationPlan;

    let mut plan = MigrationPlan {
        id: String::new(),
        comment: None,
        created_at: None,
        version: 1,
        actions: vec![
            MigrationAction::AddColumn {
                table: "users".into(),
                column: Box::new(ColumnDef {
                    name: "email".into(),
                    r#type: ColumnType::Simple(SimpleColumnType::Text),
                    nullable: false,
                    default: None,
                    comment: None,
                    primary_key: None,
                    unique: None,
                    index: None,
                    foreign_key: None,
                }),
                fill_with: None,
            },
            MigrationAction::ModifyColumnNullable {
                table: "orders".into(),
                column: "status".into(),
                nullable: false,
                fill_with: None,
                delete_null_rows: None,
            },
        ],
    };

    let mut fill_values = HashMap::new();

    // Mock prompt that returns different values based on call count
    let call_count = std::cell::RefCell::new(0);
    let mock_prompt = |_prompt: &str, _default: &str| -> Result<String> {
        let mut count = call_count.borrow_mut();
        *count += 1;
        match *count {
            1 => Ok("'user@example.com'".to_string()),
            2 => Ok("'pending'".to_string()),
            _ => Ok("'default'".to_string()),
        }
    };

    let result = handle_missing_fill_with(
        &mut plan,
        &mut fill_values,
        &[],
        mock_prompt,
        mock_enum_prompt,
    );
    assert!(result.is_ok());

    // Verify both actions were updated
    match &plan.actions[0] {
        MigrationAction::AddColumn { fill_with, .. } => {
            assert_eq!(fill_with, &Some("'user@example.com'".to_string()));
        }
        _ => panic!("Expected AddColumn action"),
    }

    match &plan.actions[1] {
        MigrationAction::ModifyColumnNullable { fill_with, .. } => {
            assert_eq!(fill_with, &Some("'pending'".to_string()));
        }
        _ => panic!("Expected ModifyColumnNullable action"),
    }
}

#[test]
fn test_collect_fill_with_values_enum_column() {
    use vespertide_planner::FillWithRequired;

    let missing = vec![FillWithRequired {
        action_index: 0,
        table: "orders".to_string(),
        column: "status".to_string(),
        action_type: "AddColumn",
        column_type: "enum<order_status>".to_string(),
        default_value: "''".to_string(),
        enum_values: Some(vec![
            "pending".to_string(),
            "confirmed".to_string(),
            "shipped".to_string(),
        ]),
        has_foreign_key: false,
    }];

    let mut fill_values = HashMap::new();

    // Mock prompt function that should NOT be called for enum columns
    let mock_prompt = |_prompt: &str, _default: &str| -> Result<String> {
        panic!("Should not be called for enum columns");
    };

    // Mock enum prompt that selects the second value
    let mock_enum = |_prompt: &str, values: &[String]| -> Result<String> {
        // Select "confirmed" (index 1)
        Ok(format!("'{}'", values[1]))
    };

    let result = collect_fill_with_values(&missing, &mut fill_values, mock_prompt, mock_enum);
    assert!(result.is_ok());
    assert_eq!(fill_values.len(), 1);
    assert_eq!(
        fill_values.get(&("orders".to_string(), "status".to_string())),
        Some(&"'confirmed'".to_string())
    );
}

#[test]
fn test_wrap_if_spaces_empty() {
    assert_eq!(wrap_if_spaces(String::new()), "");
}

#[test]
fn test_wrap_if_spaces_no_spaces() {
    assert_eq!(wrap_if_spaces("value".to_string()), "value");
}

#[test]
fn test_wrap_if_spaces_with_spaces() {
    assert_eq!(wrap_if_spaces("my value".to_string()), "'my value'");
}

#[test]
fn test_wrap_if_spaces_already_quoted() {
    assert_eq!(
        wrap_if_spaces("'already quoted'".to_string()),
        "'already quoted'"
    );
}

#[test]
fn test_wrap_if_spaces_multiple_spaces() {
    assert_eq!(wrap_if_spaces("a b c".to_string()), "'a b c'");
}

// ── enum fill_with tests ───────────────────────────────────────────

#[test]
fn test_collect_enum_fill_with_values_single_removal() {
    use vespertide_planner::EnumFillWithRequired;

    let missing = vec![EnumFillWithRequired {
        action_index: 0,
        table: "orders".to_string(),
        column: "status".to_string(),
        removed_values: vec!["cancelled".to_string()],
        remaining_values: vec!["pending".to_string(), "shipped".to_string()],
    }];

    // Mock prompt: always select first remaining value
    let mock_enum = |_prompt: &str, values: &[String]| -> Result<String> { Ok(values[0].clone()) };

    let result = collect_enum_fill_with_values(&missing, mock_enum);
    assert!(result.is_ok());
    let collected = result.unwrap();
    assert_eq!(collected.len(), 1);
    assert_eq!(collected[0].0, 0); // action_index
    assert_eq!(
        collected[0].1.get("cancelled"),
        Some(&"pending".to_string())
    );
}

#[test]
fn test_collect_enum_fill_with_values_strips_quotes_from_prompt_result() {
    use vespertide_planner::EnumFillWithRequired;

    let missing = vec![EnumFillWithRequired {
        action_index: 0,
        table: "plan".to_string(),
        column: "sheet_policy".to_string(),
        removed_values: vec!["OVER_500".to_string()],
        remaining_values: vec!["FIXED".to_string(), "NEGOTIATION".to_string()],
    }];

    let quoting_enum =
        |_prompt: &str, values: &[String]| -> Result<String> { Ok(format!("'{}'", values[0])) };

    let collected = collect_enum_fill_with_values(&missing, quoting_enum).unwrap();
    assert_eq!(collected[0].1.get("OVER_500"), Some(&"FIXED".to_string()));
}

#[test]
fn test_collect_enum_fill_with_values_multiple_removals() {
    use vespertide_planner::EnumFillWithRequired;

    let missing = vec![EnumFillWithRequired {
        action_index: 0,
        table: "orders".to_string(),
        column: "status".to_string(),
        removed_values: vec!["cancelled".to_string(), "draft".to_string()],
        remaining_values: vec!["pending".to_string(), "shipped".to_string()],
    }];

    // Mock prompt: always select second remaining value
    let mock_enum = |_prompt: &str, values: &[String]| -> Result<String> { Ok(values[1].clone()) };

    let result = collect_enum_fill_with_values(&missing, mock_enum);
    assert!(result.is_ok());
    let collected = result.unwrap();
    assert_eq!(collected[0].1.len(), 2);
    assert_eq!(
        collected[0].1.get("cancelled"),
        Some(&"shipped".to_string())
    );
    assert_eq!(collected[0].1.get("draft"), Some(&"shipped".to_string()));
}

#[test]
fn test_apply_enum_fill_with_to_plan() {
    use vespertide_core::{ColumnType, ComplexColumnType, EnumValues};

    let mut plan = MigrationPlan {
        id: String::new(),
        comment: None,
        created_at: None,
        version: 2,
        actions: vec![MigrationAction::ModifyColumnType {
            table: "orders".into(),
            column: "status".into(),
            new_type: ColumnType::Complex(ComplexColumnType::Enum {
                name: "order_status".into(),
                values: EnumValues::String(vec!["pending".into(), "shipped".into()]),
            }),
            fill_with: None,
            narrowing_strategy: None,
            timezone: None,
        }],
    };

    let mut mappings = BTreeMap::new();
    mappings.insert("cancelled".to_string(), "pending".to_string());
    let collected = vec![(0usize, mappings)];

    apply_enum_fill_with_to_plan(&mut plan, &collected);

    if let MigrationAction::ModifyColumnType { fill_with, .. } = &plan.actions[0] {
        let fw = fill_with.as_ref().expect("fill_with should be set");
        assert_eq!(fw.get("cancelled"), Some(&"pending".to_string()));
    } else {
        panic!("Expected ModifyColumnType");
    }
}

#[test]
fn test_handle_missing_enum_fill_with_collects_and_applies() {
    use vespertide_core::{ColumnDef, ColumnType, ComplexColumnType, EnumValues};

    let mut plan = MigrationPlan {
        id: String::new(),
        comment: None,
        created_at: None,
        version: 2,
        actions: vec![MigrationAction::ModifyColumnType {
            table: "orders".into(),
            column: "status".into(),
            new_type: ColumnType::Complex(ComplexColumnType::Enum {
                name: "order_status".into(),
                values: EnumValues::String(vec!["pending".into(), "shipped".into()]),
            }),
            fill_with: None,
            narrowing_strategy: None,
            timezone: None,
        }],
    };

    let baseline = vec![TableDef {
        name: "orders".into(),
        description: None,
        columns: vec![ColumnDef {
            name: "status".into(),
            r#type: ColumnType::Complex(ComplexColumnType::Enum {
                name: "order_status".into(),
                values: EnumValues::String(vec![
                    "pending".into(),
                    "shipped".into(),
                    "cancelled".into(),
                ]),
            }),
            nullable: false,
            default: None,
            comment: None,
            primary_key: None,
            unique: None,
            index: None,
            foreign_key: None,
        }],
        constraints: vec![],
    }];

    // Mock: always select first remaining value
    let mock_enum = |_prompt: &str, values: &[String]| -> Result<String> { Ok(values[0].clone()) };

    let result = handle_missing_enum_fill_with(&mut plan, &baseline, mock_enum);
    assert!(result.is_ok());

    if let MigrationAction::ModifyColumnType { fill_with, .. } = &plan.actions[0] {
        let fw = fill_with.as_ref().expect("fill_with should be populated");
        assert_eq!(fw.get("cancelled"), Some(&"pending".to_string()));
    } else {
        panic!("Expected ModifyColumnType");
    }
}

#[test]
fn test_handle_missing_enum_fill_with_no_missing() {
    let mut plan = MigrationPlan {
        id: String::new(),
        comment: None,
        created_at: None,
        version: 2,
        actions: vec![],
    };

    let mock_enum = |_prompt: &str, _values: &[String]| -> Result<String> {
        panic!("Should not be called when nothing is missing");
    };

    let result = handle_missing_enum_fill_with(&mut plan, &[], mock_enum);
    assert!(result.is_ok());
}

#[test]
fn test_apply_enum_fill_with_to_plan_extends_existing() {
    use vespertide_core::{ColumnType, ComplexColumnType, EnumValues};

    // Start with a fill_with that already has one entry
    let mut existing_fw = BTreeMap::new();
    existing_fw.insert("draft".to_string(), "pending".to_string());

    let mut plan = MigrationPlan {
        id: String::new(),
        comment: None,
        created_at: None,
        version: 2,
        actions: vec![MigrationAction::ModifyColumnType {
            table: "orders".into(),
            column: "status".into(),
            new_type: ColumnType::Complex(ComplexColumnType::Enum {
                name: "order_status".into(),
                values: EnumValues::String(vec!["pending".into(), "shipped".into()]),
            }),
            fill_with: Some(existing_fw),
            narrowing_strategy: None,
            timezone: None,
        }],
    };

    // Collect additional mappings
    let mut new_mappings = BTreeMap::new();
    new_mappings.insert("cancelled".to_string(), "shipped".to_string());
    let collected = vec![(0usize, new_mappings)];

    apply_enum_fill_with_to_plan(&mut plan, &collected);

    if let MigrationAction::ModifyColumnType { fill_with, .. } = &plan.actions[0] {
        let fw = fill_with.as_ref().expect("fill_with should be set");
        // Original entry preserved
        assert_eq!(fw.get("draft"), Some(&"pending".to_string()));
        // New entry added
        assert_eq!(fw.get("cancelled"), Some(&"shipped".to_string()));
        // Total 2 entries
        assert_eq!(fw.len(), 2);
    } else {
        panic!("Expected ModifyColumnType");
    }
}

#[test]
fn test_strip_enum_quotes_with_quotes() {
    assert_eq!(strip_enum_quotes("'active'"), "active");
}

#[test]
fn test_strip_enum_quotes_bare_value() {
    assert_eq!(strip_enum_quotes("active"), "active");
}

#[test]
fn test_strip_enum_quotes_empty() {
    assert_eq!(strip_enum_quotes(""), "");
}

#[test]
fn test_strip_enum_quotes_only_leading() {
    assert_eq!(strip_enum_quotes("'active"), "active");
}

#[test]
fn test_strip_enum_quotes_only_trailing() {
    assert_eq!(strip_enum_quotes("active'"), "active");
}

// ── F23 rename heuristic (best_rename_candidate) ─────────────────────────

/// Common rename: `pending` → `awaiting`. Distance = 6 (over threshold) so
/// the suggestion is None — this case must be selected manually.
#[test]
fn test_best_rename_no_suggestion_when_distance_too_large() {
    let remaining = vec!["awaiting".to_string(), "shipped".to_string()];
    assert_eq!(best_rename_candidate("pending", &remaining), None);
}

/// British/American spelling: `cancelled` → `canceled`. Distance = 1, well
/// within threshold; suggestion should fire.
#[test]
fn test_best_rename_picks_spelling_variant() {
    let remaining = vec!["canceled".to_string(), "active".to_string()];
    assert_eq!(
        best_rename_candidate("cancelled", &remaining),
        Some("canceled".to_string())
    );
}

/// Snake-case conversion: `inprogress` → `in_progress`. Distance = 1 (one
/// inserted underscore). Suggestion should fire.
#[test]
fn test_best_rename_picks_snake_case() {
    let remaining = vec!["in_progress".to_string(), "done".to_string()];
    assert_eq!(
        best_rename_candidate("inprogress", &remaining),
        Some("in_progress".to_string())
    );
}

/// When two candidates are equally distant, the FIRST one in declaration
/// order wins (deterministic for snapshots).
#[test]
fn test_best_rename_ties_break_by_declaration_order() {
    let remaining = vec!["test1".to_string(), "test2".to_string()];
    // Both are distance 1 from "test"; first one wins.
    assert_eq!(
        best_rename_candidate("test", &remaining),
        Some("test1".to_string())
    );
}

/// Identical strings shouldn't appear here (validation filters them) but
/// guard anyway: distance 0 still produces a suggestion.
#[test]
fn test_best_rename_handles_exact_match() {
    let remaining = vec!["active".to_string(), "inactive".to_string()];
    assert_eq!(
        best_rename_candidate("active", &remaining),
        Some("active".to_string())
    );
}

/// Empty remaining list: nothing to suggest.
#[test]
fn test_best_rename_empty_remaining() {
    assert_eq!(best_rename_candidate("anything", &[]), None);
}

/// Threshold boundary: distance 3 is accepted, distance 4 is rejected.
#[test]
fn test_best_rename_threshold_boundary() {
    // "abcd" vs "abcdEFG" — distance 3 (three insertions), at threshold.
    let in_range = vec!["abcdEFG".to_string()];
    assert_eq!(
        best_rename_candidate("abcd", &in_range),
        Some("abcdEFG".to_string())
    );

    // "abcd" vs "abcdEFGH" — distance 4, over threshold.
    let out_of_range = vec!["abcdEFGH".to_string()];
    assert_eq!(best_rename_candidate("abcd", &out_of_range), None);
}
