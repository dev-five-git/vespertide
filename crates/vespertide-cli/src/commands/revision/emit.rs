use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use anyhow::Result;
use vespertide_core::{MigrationAction, MigrationPlan, TableConstraint, TableDef};

/// Apply `fill_with` values to a migration plan.
pub(super) fn apply_fill_with_to_plan(
    plan: &mut MigrationPlan,
    fill_values: &HashMap<(String, String), String>,
) {
    for action in &mut plan.actions {
        match action {
            MigrationAction::AddColumn {
                table,
                column,
                fill_with,
            } => {
                if fill_with.is_none()
                    && let Some(value) =
                        fill_values.get(&(table.to_string(), column.name.to_string()))
                {
                    *fill_with = Some(value.clone());
                }
            }
            MigrationAction::ModifyColumnNullable {
                table,
                column,
                fill_with,
                ..
            } => {
                if fill_with.is_none()
                    && let Some(value) = fill_values.get(&(table.to_string(), column.to_string()))
                {
                    *fill_with = Some(value.clone());
                }
            }
            _ => {}
        }
    }
}

/// Apply `delete_null_rows` flags to matching `ModifyColumnNullable` actions.
pub(super) fn apply_delete_null_rows_to_plan(
    plan: &mut MigrationPlan,
    delete_set: &HashSet<(String, String)>,
) {
    for action in &mut plan.actions {
        if let MigrationAction::ModifyColumnNullable {
            table,
            column,
            nullable,
            delete_null_rows,
            ..
        } = action
            && !*nullable
            && delete_null_rows.is_none()
            && delete_set.contains(&(table.to_string(), column.to_string()))
        {
            *delete_null_rows = Some(true);
        }
    }
}
/// Apply collected enum `fill_with` mappings to the migration plan.
pub(super) fn apply_enum_fill_with_to_plan(
    plan: &mut MigrationPlan,
    collected: &[(usize, BTreeMap<String, String>)],
) {
    for (action_index, mappings) in collected {
        if let Some(MigrationAction::ModifyColumnType { fill_with, .. }) =
            plan.actions.get_mut(*action_index)
        {
            match fill_with {
                Some(existing) => {
                    existing.extend(mappings.clone());
                }
                None => {
                    *fill_with = Some(mappings.clone());
                }
            }
        }
    }
}
/// Reason why a table needs to be recreated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum RecreateReason {
    /// A new non-nullable FK column is being added.
    AddColumnWithFk,
    /// A FK constraint is being added to an existing non-nullable column.
    AddFkToExistingColumn,
}

/// A table that needs to be recreated because of a non-nullable FK constraint issue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct RecreateTableRequired {
    pub(super) table: String,
    pub(super) column: String,
    pub(super) reason: RecreateReason,
}

/// Find actions that require table recreation due to non-nullable FK constraints.
///
/// Two cases are detected:
/// 1. **`AddColumn` with FK**: A new non-nullable FK column is being added (no default).
/// 2. **AddConstraint(FK) on existing column**: A FK constraint is being added to an
///    existing non-nullable column without a default.
///
/// In both cases, existing rows cannot satisfy the foreign key constraint,
/// so the table must be recreated (`DeleteTable` + `CreateTable`).
pub(super) fn find_non_nullable_fk_add_columns(
    plan: &MigrationPlan,
    current_models: &[TableDef],
) -> Vec<RecreateTableRequired> {
    // Collect FK columns from AddConstraint actions; lookup-only, ordering unused.
    // Keys borrow from `plan` so construction and probes stay allocation-free.
    let mut fk_columns: HashSet<(&str, &str)> = HashSet::new();
    for action in &plan.actions {
        if let MigrationAction::AddConstraint {
            table,
            constraint: TableConstraint::ForeignKey { columns, .. },
        } = action
        {
            for col in columns {
                fk_columns.insert((table.as_str(), col.as_str()));
            }
        }
    }

    // Collect columns being added in this migration (to distinguish new vs existing); lookup-only, ordering unused.
    let mut added_columns: HashSet<(&str, &str)> = HashSet::new();
    for action in &plan.actions {
        if let MigrationAction::AddColumn { table, column, .. } = action {
            added_columns.insert((table.as_str(), column.name.as_str()));
        }
    }

    let mut result = Vec::new();

    // Case 1: AddColumn with FK (new non-nullable FK column)
    for action in &plan.actions {
        if let MigrationAction::AddColumn { table, column, .. } = action {
            let has_fk = column.foreign_key.is_some()
                || fk_columns.contains(&(table.as_str(), column.name.as_str()));
            if has_fk && !column.nullable && column.default.is_none() {
                result.push(RecreateTableRequired {
                    table: table.to_string(),
                    column: column.name.to_string(),
                    reason: RecreateReason::AddColumnWithFk,
                });
            }
        }
    }

    // Case 2: AddConstraint(FK) on existing non-nullable column
    for action in &plan.actions {
        if let MigrationAction::AddConstraint {
            table,
            constraint: TableConstraint::ForeignKey { columns, .. },
        } = action
        {
            for col_name in columns {
                // Skip if this column is being added in this migration (handled by Case 1)
                if added_columns.contains(&(table.as_str(), col_name.as_str())) {
                    continue;
                }
                // Look up column in current models to check nullability
                if let Some(model) = current_models
                    .iter()
                    .find(|m| m.name.as_str() == table.as_str())
                    && let Some(col_def) = model
                        .columns
                        .iter()
                        .find(|c| c.name.as_str() == col_name.as_str())
                    && !col_def.nullable
                    && col_def.default.is_none()
                {
                    result.push(RecreateTableRequired {
                        table: table.to_string(),
                        column: col_name.to_string(),
                        reason: RecreateReason::AddFkToExistingColumn,
                    });
                }
            }
        }
    }

    result
}

/// Rewrite the migration plan to recreate tables instead of adding columns.
/// Removes all column/constraint actions targeting the recreated tables and replaces
/// them with `DeleteTable` + `CreateTable` using the full target model.
pub(super) fn rewrite_plan_for_recreation(
    plan: &mut MigrationPlan,
    recreate_tables: &[RecreateTableRequired],
    current_models: &[TableDef],
) {
    let tables_to_recreate: BTreeSet<&str> =
        recreate_tables.iter().map(|r| r.table.as_str()).collect();

    // Remove all column/constraint actions targeting recreated tables
    plan.actions.retain(|action| {
        let table = match action {
            MigrationAction::AddColumn { table, .. }
            | MigrationAction::DeleteColumn { table, .. }
            | MigrationAction::RenameColumn { table, .. }
            | MigrationAction::ModifyColumnType { table, .. }
            | MigrationAction::ModifyColumnNullable { table, .. }
            | MigrationAction::ModifyColumnDefault { table, .. }
            | MigrationAction::ModifyColumnComment { table, .. }
            | MigrationAction::AddConstraint { table, .. }
            | MigrationAction::RemoveConstraint { table, .. }
            | MigrationAction::ReplaceConstraint { table, .. } => Some(table.as_str()),
            _ => None,
        };
        table.is_none_or(|t| !tables_to_recreate.contains(t))
    });

    // Add DeleteTable + CreateTable for each recreated table
    for table_name in &tables_to_recreate {
        if let Some(model) = current_models
            .iter()
            .find(|m| m.name.as_str() == *table_name)
        {
            plan.actions.push(MigrationAction::DeleteTable {
                table: (*table_name).into(),
            });
            plan.actions.push(MigrationAction::CreateTable {
                table: model.name.clone(),
                columns: model.columns.clone(),
                constraints: model.constraints.clone(),
            });
        }
    }
}

pub(super) fn handle_recreate_requirements<F>(
    plan: &mut MigrationPlan,
    current_models: &[TableDef],
    prompt_fn: F,
) -> Result<()>
where
    F: Fn(&[RecreateTableRequired]) -> Result<bool>,
{
    let recreate_tables = find_non_nullable_fk_add_columns(plan, current_models);
    if recreate_tables.is_empty() {
        return Ok(());
    }

    if !prompt_fn(&recreate_tables)? {
        anyhow::bail!(
            "Migration cancelled. To proceed without recreation, make the column nullable or add it with a default value that references an existing row."
        );
    }

    rewrite_plan_for_recreation(plan, &recreate_tables, current_models);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{BTreeMap, HashMap, HashSet};
    use vespertide_core::{
        ColumnDef, ColumnType, MigrationAction, MigrationPlan, SimpleColumnType, TableConstraint,
        TableDef,
    };

    fn empty_plan(actions: Vec<MigrationAction>) -> MigrationPlan {
        MigrationPlan {
            id: String::new(),
            comment: None,
            created_at: None,
            version: 1,
            actions,
        }
    }

    fn col(name: &str, nullable: bool) -> ColumnDef {
        ColumnDef {
            name: name.into(),
            r#type: ColumnType::Simple(SimpleColumnType::Integer),
            nullable,
            default: None,
            comment: None,
            primary_key: None,
            unique: None,
            index: None,
            foreign_key: None,
        }
    }

    // ── 1. apply_fill_with_to_plan: only overwrites None, preserves Some ──────

    #[test]
    fn apply_fill_with_to_plan_only_fills_none() {
        let mut plan = empty_plan(vec![
            MigrationAction::AddColumn {
                table: "t".into(),
                column: Box::new(col("a", false)),
                fill_with: None,
            },
            MigrationAction::AddColumn {
                table: "t".into(),
                column: Box::new(col("b", false)),
                fill_with: Some("kept".to_string()),
            },
        ]);
        let mut map = HashMap::new();
        map.insert(("t".to_string(), "a".to_string()), "filled".to_string());
        map.insert(
            ("t".to_string(), "b".to_string()),
            "should_not_replace".to_string(),
        );

        apply_fill_with_to_plan(&mut plan, &map);

        match &plan.actions[0] {
            MigrationAction::AddColumn { fill_with, .. } => {
                assert_eq!(
                    fill_with,
                    &Some("filled".to_string()),
                    "None should be filled"
                );
            }
            _ => panic!("expected AddColumn"),
        }
        match &plan.actions[1] {
            MigrationAction::AddColumn { fill_with, .. } => {
                assert_eq!(
                    fill_with,
                    &Some("kept".to_string()),
                    "Some should be preserved"
                );
            }
            _ => panic!("expected AddColumn"),
        }
    }

    // ── 2. apply_fill_with_to_plan: ModifyColumnNullable arm is filled ────────

    #[test]
    fn apply_fill_with_to_plan_modifies_nullable_action() {
        let mut plan = empty_plan(vec![MigrationAction::ModifyColumnNullable {
            table: "u".into(),
            column: "email".into(),
            nullable: false,
            fill_with: None,
            delete_null_rows: None,
        }]);
        let mut map = HashMap::new();
        map.insert(
            ("u".to_string(), "email".to_string()),
            "'x@y.z'".to_string(),
        );

        apply_fill_with_to_plan(&mut plan, &map);

        match &plan.actions[0] {
            MigrationAction::ModifyColumnNullable { fill_with, .. } => {
                assert_eq!(fill_with, &Some("'x@y.z'".to_string()));
            }
            _ => panic!("expected ModifyColumnNullable"),
        }
    }

    // ── 3. apply_delete_null_rows: only sets flag when nullable=false ─────────

    #[test]
    fn apply_delete_null_rows_only_when_not_nullable() {
        let mut plan = empty_plan(vec![
            MigrationAction::ModifyColumnNullable {
                table: "t".into(),
                column: "c1".into(),
                nullable: true,
                fill_with: None,
                delete_null_rows: None,
            },
            MigrationAction::ModifyColumnNullable {
                table: "t".into(),
                column: "c2".into(),
                nullable: false,
                fill_with: None,
                delete_null_rows: None,
            },
        ]);
        let mut set = HashSet::new();
        set.insert(("t".to_string(), "c1".to_string()));
        set.insert(("t".to_string(), "c2".to_string()));

        apply_delete_null_rows_to_plan(&mut plan, &set);

        match &plan.actions[0] {
            MigrationAction::ModifyColumnNullable {
                delete_null_rows, ..
            } => {
                assert_eq!(
                    delete_null_rows, &None,
                    "nullable=true must NOT get delete_null_rows"
                );
            }
            _ => panic!("expected ModifyColumnNullable"),
        }
        match &plan.actions[1] {
            MigrationAction::ModifyColumnNullable {
                delete_null_rows, ..
            } => {
                assert_eq!(
                    delete_null_rows,
                    &Some(true),
                    "nullable=false must get delete_null_rows"
                );
            }
            _ => panic!("expected ModifyColumnNullable"),
        }
    }

    // ── 4. apply_enum_fill_with: Some arm extends, does not replace ───────────

    #[test]
    fn apply_enum_fill_with_extends_existing_some() {
        let mut existing_map = BTreeMap::new();
        existing_map.insert("0".to_string(), "10".to_string());

        let mut plan = empty_plan(vec![MigrationAction::ModifyColumnType {
            table: "t".into(),
            column: "status".into(),
            new_type: ColumnType::Simple(SimpleColumnType::Integer),
            fill_with: Some(existing_map),
            narrowing_strategy: None,
            timezone: None,
        }]);

        let mut new_map = BTreeMap::new();
        new_map.insert("1".to_string(), "20".to_string());
        let collected = vec![(0usize, new_map)];

        apply_enum_fill_with_to_plan(&mut plan, &collected);

        match &plan.actions[0] {
            MigrationAction::ModifyColumnType { fill_with, .. } => {
                let m = fill_with
                    .as_ref()
                    .expect("fill_with must be Some after extend");
                assert_eq!(
                    m.get("0"),
                    Some(&"10".to_string()),
                    "original entry preserved"
                );
                assert_eq!(m.get("1"), Some(&"20".to_string()), "new entry added");
                assert_eq!(m.len(), 2);
            }
            _ => panic!("expected ModifyColumnType"),
        }
    }

    // ── 5. find_non_nullable_fk_add_columns case 1: AddColumn with FK ─────────

    #[test]
    fn find_non_nullable_fk_add_columns_case1() {
        use vespertide_core::ReferenceAction;
        use vespertide_core::schema::foreign_key::{ForeignKeyDef, ForeignKeySyntax};

        let plan = empty_plan(vec![MigrationAction::AddColumn {
            table: "post".into(),
            column: Box::new(ColumnDef {
                name: "author_id".into(),
                r#type: ColumnType::Simple(SimpleColumnType::Integer),
                nullable: false,
                default: None,
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: Some(ForeignKeySyntax::Object(ForeignKeyDef {
                    ref_table: "user".into(),
                    ref_columns: vec!["id".into()],
                    on_delete: Some(ReferenceAction::Cascade),
                    on_update: None,
                    orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
                })),
            }),
            fill_with: None,
        }]);

        let result = find_non_nullable_fk_add_columns(&plan, &[]);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].table, "post");
        assert_eq!(result[0].column, "author_id");
        assert_eq!(result[0].reason, RecreateReason::AddColumnWithFk);
    }

    // ── 6. find_non_nullable_fk_add_columns case 2: AddConstraint on existing ─

    #[test]
    fn find_non_nullable_fk_add_columns_case2() {
        let plan = empty_plan(vec![MigrationAction::AddConstraint {
            table: "t".into(),
            constraint: TableConstraint::ForeignKey {
                name: None,
                columns: vec!["email".into()],
                ref_table: "other".into(),
                ref_columns: vec!["id".into()],
                on_delete: None,
                on_update: None,
                orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
            },
        }]);

        let models = vec![TableDef {
            name: "t".into(),
            description: None,
            columns: vec![col("email", false)],
            constraints: vec![],
        }];

        let result = find_non_nullable_fk_add_columns(&plan, &models);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].table, "t");
        assert_eq!(result[0].column, "email");
        assert_eq!(result[0].reason, RecreateReason::AddFkToExistingColumn);
    }

    // ── 7. rewrite_plan_for_recreation: removes column actions, appends Delete+Create ──

    #[test]
    fn rewrite_plan_for_recreation_replaces() {
        let mut plan = empty_plan(vec![
            MigrationAction::AddColumn {
                table: "u".into(),
                column: Box::new(col("x", false)),
                fill_with: None,
            },
            MigrationAction::DeleteColumn {
                table: "u".into(),
                column: "y".into(),
            },
            MigrationAction::ModifyColumnNullable {
                table: "u".into(),
                column: "z".into(),
                nullable: false,
                fill_with: None,
                delete_null_rows: None,
            },
        ]);

        let recreate = vec![RecreateTableRequired {
            table: "u".to_string(),
            column: "x".to_string(),
            reason: RecreateReason::AddColumnWithFk,
        }];

        let models = vec![TableDef {
            name: "u".into(),
            description: None,
            columns: vec![col("id", false), col("x", false)],
            constraints: vec![],
        }];

        rewrite_plan_for_recreation(&mut plan, &recreate, &models);

        assert_eq!(
            plan.actions.len(),
            2,
            "expected exactly DeleteTable + CreateTable"
        );
        assert!(
            matches!(&plan.actions[0], MigrationAction::DeleteTable { table } if table == "u"),
            "first action must be DeleteTable for u"
        );
        assert!(
            matches!(&plan.actions[1], MigrationAction::CreateTable { table, .. } if table == "u"),
            "second action must be CreateTable for u"
        );
    }

    // ── 8. no FK → empty even if non-nullable (kills && → || at first &&) ─────

    #[test]
    fn find_non_nullable_fk_add_columns_no_fk_returns_empty() {
        let plan = empty_plan(vec![MigrationAction::AddColumn {
            table: "t".into(),
            column: Box::new(ColumnDef {
                name: "score".into(),
                r#type: ColumnType::Simple(SimpleColumnType::Integer),
                nullable: false,
                default: None,
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            }),
            fill_with: None,
        }]);
        assert!(
            find_non_nullable_fk_add_columns(&plan, &[]).is_empty(),
            "non-nullable column without FK must not trigger recreation"
        );
    }

    // ── 9. FK + non-nullable + has default → empty (kills && → || at second &&) ─

    #[test]
    fn find_non_nullable_fk_add_columns_with_default_returns_empty() {
        use vespertide_core::ReferenceAction;
        use vespertide_core::schema::foreign_key::{ForeignKeyDef, ForeignKeySyntax};

        let plan = empty_plan(vec![MigrationAction::AddColumn {
            table: "t".into(),
            column: Box::new(ColumnDef {
                name: "ref_id".into(),
                r#type: ColumnType::Simple(SimpleColumnType::Integer),
                nullable: false,
                default: Some(true.into()),
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: Some(ForeignKeySyntax::Object(ForeignKeyDef {
                    ref_table: "other".into(),
                    ref_columns: vec!["id".into()],
                    on_delete: Some(ReferenceAction::Cascade),
                    on_update: None,
                    orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
                })),
            }),
            fill_with: None,
        }]);
        assert!(
            find_non_nullable_fk_add_columns(&plan, &[]).is_empty(),
            "FK column with a default must not trigger recreation"
        );
    }

    // ── 10. prompt=true → rewrites plan (kills Ok(()) noop + kills delete !) ──

    #[test]
    fn handle_recreate_requirements_prompt_true_rewrites_plan() {
        use vespertide_core::ReferenceAction;
        use vespertide_core::schema::foreign_key::{ForeignKeyDef, ForeignKeySyntax};

        let mut plan = empty_plan(vec![MigrationAction::AddColumn {
            table: "post".into(),
            column: Box::new(ColumnDef {
                name: "user_id".into(),
                r#type: ColumnType::Simple(SimpleColumnType::Integer),
                nullable: false,
                default: None,
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: Some(ForeignKeySyntax::Object(ForeignKeyDef {
                    ref_table: "user".into(),
                    ref_columns: vec!["id".into()],
                    on_delete: Some(ReferenceAction::Cascade),
                    on_update: None,
                    orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
                })),
            }),
            fill_with: None,
        }]);

        let models = vec![TableDef {
            name: "post".into(),
            description: None,
            columns: vec![col("id", false), col("user_id", false)],
            constraints: vec![],
        }];

        handle_recreate_requirements(&mut plan, &models, |_| Ok(true)).unwrap();

        assert_eq!(
            plan.actions.len(),
            2,
            "plan must be rewritten to Delete+Create"
        );
        assert!(
            matches!(&plan.actions[0], MigrationAction::DeleteTable { table } if table == "post"),
            "first action must be DeleteTable"
        );
        assert!(
            matches!(&plan.actions[1], MigrationAction::CreateTable { table, .. } if table == "post"),
            "second action must be CreateTable"
        );
    }

    // ── 11. prompt=false → bails (kills delete ! from the other direction) ────

    #[test]
    fn handle_recreate_requirements_prompt_false_bails() {
        use vespertide_core::ReferenceAction;
        use vespertide_core::schema::foreign_key::{ForeignKeyDef, ForeignKeySyntax};

        let mut plan = empty_plan(vec![MigrationAction::AddColumn {
            table: "post".into(),
            column: Box::new(ColumnDef {
                name: "user_id".into(),
                r#type: ColumnType::Simple(SimpleColumnType::Integer),
                nullable: false,
                default: None,
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: Some(ForeignKeySyntax::Object(ForeignKeyDef {
                    ref_table: "user".into(),
                    ref_columns: vec!["id".into()],
                    on_delete: Some(ReferenceAction::Cascade),
                    on_update: None,
                    orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
                })),
            }),
            fill_with: None,
        }]);

        let err = handle_recreate_requirements(&mut plan, &[], |_| Ok(false)).unwrap_err();
        assert!(
            err.to_string().contains("Migration cancelled"),
            "must bail with cancellation message"
        );
    }
}
