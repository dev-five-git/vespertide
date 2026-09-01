//! Fault **F15**: column DEFAULT value changes that affect *new rows only*.
//!
//! `ALTER TABLE ... ALTER COLUMN ... SET DEFAULT ...` updates the catalog so
//! every *future* `INSERT` uses the new default. **Existing rows are not
//! touched.** This is correct SQL behaviour but a common source of silent
//! data-consistency bugs:
//!
//! - "I changed `default: 'pending'` to `default: 'active'` — why are old
//!   rows still 'pending'?"
//! - "I switched to `gen_random_uuid()` — why do legacy rows have a literal
//!   'manual' marker instead?"
//!
//! This module performs a **purely static** scan over the planned actions
//! and the baseline schema to classify every `ModifyColumnDefault` action
//! into one of six shapes, each with a risk level the CLI surfaces to the
//! user:
//!
//! | Kind                       | Risk    |
//! |----------------------------|---------|
//! | `AddedDefault`             | Medium  |
//! | `RemovedDefault`           | Medium  |
//! | `LiteralToLiteral`         | Low     |
//! | `LiteralToFunction`        | **High**|
//! | `FunctionToLiteral`        | Medium  |
//! | `FunctionToFunction`       | Low     |
//!
//! The CLI uses this output to drive an interactive Backfill / Skip / Cancel
//! prompt; planner stays purely library-level.

use std::collections::HashSet;

use vespertide_core::{DefaultValue, MigrationAction, MigrationPlan, TableDef};

/// One classified default-change warning, ready for prompt rendering.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DefaultChangeWarning {
    /// Index of the originating `ModifyColumnDefault` action in [`MigrationPlan::actions`].
    pub action_index: usize,
    pub table: String,
    pub column: String,
    /// Old default rendered to SQL (`None` if the column had no default in
    /// the baseline). Used by the prompt header and by the backfill SQL
    /// when the user picks "rewrite rows that currently match the old
    /// default" in a future enhancement.
    pub old_default: Option<String>,
    /// New default as it will be written by the migration (`None` if the
    /// action drops the default).
    pub new_default: Option<String>,
    /// Classification of the change shape.
    pub kind: DefaultChangeKind,
}

/// Shape of a default change. Drives the [`RiskLevel`] and the prompt copy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DefaultChangeKind {
    /// `None → Some(value)` — column gains a default for new rows. Existing
    /// rows still have whatever values were inserted explicitly (often `NULL`
    /// for nullable columns or the prior DB-assigned value otherwise).
    AddedDefault,
    /// `Some(value) → None` — column loses its default. New rows must now
    /// supply a value explicitly or rely on the column's `NULL`-ability.
    RemovedDefault,
    /// `'pending' → 'active'` — both sides are literal constants. New rows
    /// get the new constant; existing rows keep their stored values.
    LiteralToLiteral,
    /// `'manual' → gen_random_uuid()` — literal replaced by a function call.
    /// **High risk**: new rows get fresh function results (likely unique
    /// per row), existing rows keep the old literal. Two very different
    /// "categories" of data now live in the same column.
    LiteralToFunction,
    /// `NOW() → '2024-01-01'` — function replaced by a literal. New rows
    /// freeze to one value; existing rows keep their old function results.
    FunctionToLiteral,
    /// `NOW() → CURRENT_TIMESTAMP` — both sides are function calls. Often
    /// semantically equivalent, but treat as a soft warning so the user
    /// confirms intent.
    FunctionToFunction,
}

/// Risk level surfaced by the CLI prompt. Drives ordering (HIGH first,
/// LOW last) and visual emphasis (red vs yellow vs cyan).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RiskLevel {
    High,
    Medium,
    Low,
}

impl DefaultChangeKind {
    /// Risk grade for this change shape. See module docs for the rationale.
    #[must_use]
    pub fn risk_level(self) -> RiskLevel {
        match self {
            Self::LiteralToFunction => RiskLevel::High,
            Self::AddedDefault | Self::RemovedDefault | Self::FunctionToLiteral => {
                RiskLevel::Medium
            }
            Self::LiteralToLiteral | Self::FunctionToFunction => RiskLevel::Low,
        }
    }

    /// Short human label for prompt summaries (e.g. `"literal → function"`).
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::AddedDefault => "added default",
            Self::RemovedDefault => "removed default",
            Self::LiteralToLiteral => "literal → literal",
            Self::LiteralToFunction => "literal → function",
            Self::FunctionToLiteral => "function → literal",
            Self::FunctionToFunction => "function → function",
        }
    }
}

/// Scan a plan for `ModifyColumnDefault` actions and classify each.
///
/// Returned warnings are sorted by `action_index` so the CLI processes them
/// in the same order the planner emitted them.
///
/// **Suppression rule** — columns that also carry a `ModifyColumnType` or
/// `RemapEnumValues` action in the *same* plan are skipped. The user has
/// already acknowledged the underlying value/type change through the
/// type-narrowing or enum-remap prompts (F6/F19/F33/F87, F7-(b)), so
/// re-prompting for the resulting default change would be redundant and
/// confusing (the integer-enum remap already rewrites every existing row;
/// asking the user again to "backfill" the default would either repeat or,
/// worse, *overwrite* the row-specific remap targets).
///
/// Pass the **baseline** schema (state *before* the plan is applied), not
/// the current model state — the warning needs to know what the *previous*
/// default looked like to render a sensible "X → Y" message.
#[must_use]
pub fn find_default_changes(
    plan: &MigrationPlan,
    baseline: &[TableDef],
) -> Vec<DefaultChangeWarning> {
    let suppressed = collect_columns_with_value_change(plan);

    let mut out = Vec::new();
    for (idx, action) in plan.actions.iter().enumerate() {
        let MigrationAction::ModifyColumnDefault {
            table,
            column,
            new_default,
            ..
        } = action
        else {
            continue;
        };

        if suppressed.contains(&(table.as_str(), column.as_str())) {
            continue;
        }

        let old_default = lookup_old_default(baseline, table.as_str(), column.as_str());
        let kind = classify(old_default.as_deref(), new_default.as_deref());

        out.push(DefaultChangeWarning {
            action_index: idx,
            table: table.to_string(),
            column: column.to_string(),
            old_default,
            new_default: new_default.clone(),
            kind,
        });
    }
    out
}

/// Collect `(table, column)` pairs that have a value-rewriting action in the
/// plan (`ModifyColumnType` or `RemapEnumValues`). These columns are
/// suppressed from F15 prompting because the user has already provided
/// intent through the corresponding action's own prompt.
fn collect_columns_with_value_change(plan: &MigrationPlan) -> HashSet<(&str, &str)> {
    plan.actions
        .iter()
        .filter_map(|action| match action {
            MigrationAction::ModifyColumnType { table, column, .. }
            | MigrationAction::RemapEnumValues { table, column, .. } => {
                Some((table.as_str(), column.as_str()))
            }
            _ => None,
        })
        .collect()
}

fn lookup_old_default(baseline: &[TableDef], table: &str, column: &str) -> Option<String> {
    baseline
        .iter()
        .find(|t| t.name == table)?
        .columns
        .iter()
        .find(|c| c.name == column)?
        .default
        .as_ref()
        .map(DefaultValue::to_sql)
}

/// Classify a default change given the rendered SQL of the old and new
/// defaults. Both sides are `None` when the corresponding side is absent.
fn classify(old: Option<&str>, new: Option<&str>) -> DefaultChangeKind {
    match (old, new) {
        (None, Some(_)) => DefaultChangeKind::AddedDefault,
        (Some(_), None) => DefaultChangeKind::RemovedDefault,
        (Some(o), Some(n)) => {
            let o_fn = is_function_expr(o);
            let n_fn = is_function_expr(n);
            match (o_fn, n_fn) {
                (false, true) => DefaultChangeKind::LiteralToFunction,
                (true, false) => DefaultChangeKind::FunctionToLiteral,
                (false, false) => DefaultChangeKind::LiteralToLiteral,
                (true, true) => DefaultChangeKind::FunctionToFunction,
            }
        }
        // (None, None) cannot happen: the action would not exist if both
        // defaults were absent. Treat as a no-op literal change to stay
        // defensive against malformed plans.
        (None, None) => DefaultChangeKind::LiteralToLiteral,
    }
}

/// True when the rendered SQL looks like a function call or a SQL keyword
/// that the DB evaluates per-row. False for quoted strings, numeric / boolean
/// literals, and bare identifiers that aren't reserved keywords.
///
/// Conservative classification: when in doubt (e.g. an unrecognised bare
/// identifier), we treat the value as a literal so we *don't* over-warn.
fn is_function_expr(s: &str) -> bool {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return false;
    }
    if trimmed.contains('(') {
        return true;
    }
    // ASCII-case-insensitive compares against the fixed keyword set avoid the
    // per-call `String` allocation that `to_uppercase()` would incur (mirrors
    // the allocation-free convention in `helpers.rs::needs_quoting`). All
    // keywords are pure ASCII, so the result is byte-identical.
    trimmed.eq_ignore_ascii_case("CURRENT_TIMESTAMP")
        || trimmed.eq_ignore_ascii_case("CURRENT_DATE")
        || trimmed.eq_ignore_ascii_case("CURRENT_TIME")
        || trimmed.eq_ignore_ascii_case("LOCALTIMESTAMP")
        || trimmed.eq_ignore_ascii_case("LOCALTIME")
        || trimmed.eq_ignore_ascii_case("CURRENT_USER")
        || trimmed.eq_ignore_ascii_case("SESSION_USER")
}

#[cfg(test)]
mod tests {
    use super::*;
    use vespertide_core::{
        ColumnDef, ColumnName, ColumnType, MigrationPlan, SimpleColumnType, TableConstraint,
        TableName,
    };

    fn col_with_default(name: &str, default: Option<&str>) -> ColumnDef {
        let mut c = ColumnDef::new(name, ColumnType::Simple(SimpleColumnType::Text), true);
        c.default = default.map(DefaultValue::from);
        c
    }

    fn table_with(name: &str, columns: Vec<ColumnDef>) -> TableDef {
        TableDef {
            name: TableName::from(name),
            description: None,
            columns,
            constraints: vec![TableConstraint::PrimaryKey {
                auto_increment: false,
                columns: vec![ColumnName::from("id")],
                strategy: vespertide_core::PrimaryKeyAdditionStrategy::default(),
            }],
        }
    }

    fn modify_default(table: &str, column: &str, new_default: Option<&str>) -> MigrationAction {
        MigrationAction::ModifyColumnDefault {
            table: TableName::from(table),
            column: ColumnName::from(column),
            new_default: new_default.map(ToString::to_string),
            backfill: None,
        }
    }

    fn plan_with(actions: Vec<MigrationAction>) -> MigrationPlan {
        MigrationPlan {
            id: String::new(),
            comment: None,
            created_at: None,
            version: 1,
            actions,
        }
    }

    // ── classify() unit tests ────────────────────────────────────────────

    #[test]
    fn classify_literal_to_literal_is_low() {
        let kind = classify(Some("'pending'"), Some("'active'"));
        assert_eq!(kind, DefaultChangeKind::LiteralToLiteral);
        assert_eq!(kind.risk_level(), RiskLevel::Low);
    }

    #[test]
    fn classify_literal_to_function_is_high() {
        let kind = classify(Some("'manual'"), Some("gen_random_uuid()"));
        assert_eq!(kind, DefaultChangeKind::LiteralToFunction);
        assert_eq!(kind.risk_level(), RiskLevel::High);
    }

    #[test]
    fn classify_function_to_literal_is_medium() {
        let kind = classify(Some("NOW()"), Some("'2024-01-01'"));
        assert_eq!(kind, DefaultChangeKind::FunctionToLiteral);
        assert_eq!(kind.risk_level(), RiskLevel::Medium);
    }

    #[test]
    fn classify_function_to_function_is_low() {
        let kind = classify(Some("NOW()"), Some("CURRENT_TIMESTAMP"));
        assert_eq!(kind, DefaultChangeKind::FunctionToFunction);
        assert_eq!(kind.risk_level(), RiskLevel::Low);
    }

    #[test]
    fn classify_added_default_is_medium() {
        let kind = classify(None, Some("'active'"));
        assert_eq!(kind, DefaultChangeKind::AddedDefault);
        assert_eq!(kind.risk_level(), RiskLevel::Medium);
    }

    #[test]
    fn classify_removed_default_is_medium() {
        let kind = classify(Some("'pending'"), None);
        assert_eq!(kind, DefaultChangeKind::RemovedDefault);
        assert_eq!(kind.risk_level(), RiskLevel::Medium);
    }

    #[test]
    fn is_function_expr_recognises_parens() {
        assert!(is_function_expr("NOW()"));
        assert!(is_function_expr("gen_random_uuid()"));
        assert!(is_function_expr("date_trunc('day', col)"));
    }

    #[test]
    fn is_function_expr_recognises_bare_keywords_case_insensitive() {
        assert!(is_function_expr("CURRENT_TIMESTAMP"));
        assert!(is_function_expr("current_timestamp"));
        assert!(is_function_expr("CURRENT_DATE"));
        assert!(is_function_expr("LocalTimestamp"));
        // The keyword list is one `||` chain, so only an input that reaches the
        // FINAL alternatives proves those `||`s are disjunctions: an earlier
        // match short-circuits and leaves the tail untested.
        assert!(is_function_expr("CURRENT_TIME"));
        assert!(is_function_expr("LOCALTIME"));
        assert!(is_function_expr("CURRENT_USER"));
        assert!(is_function_expr("session_user"));
    }

    #[test]
    fn is_function_expr_rejects_literals() {
        assert!(!is_function_expr("'pending'"));
        assert!(!is_function_expr("42"));
        assert!(!is_function_expr("true"));
        assert!(!is_function_expr("'NOW'")); // quoted string with the word
    }

    // ── find_default_changes() end-to-end ────────────────────────────────

    #[test]
    fn find_default_changes_empty_plan_returns_empty() {
        let baseline = vec![table_with("users", vec![col_with_default("email", None)])];
        let plan = plan_with(vec![]);
        assert!(find_default_changes(&plan, &baseline).is_empty());
    }

    #[test]
    fn find_default_changes_high_risk_literal_to_function() {
        let baseline = vec![table_with(
            "orders",
            vec![col_with_default("tracking_id", Some("'manual'"))],
        )];
        let plan = plan_with(vec![modify_default(
            "orders",
            "tracking_id",
            Some("gen_random_uuid()"),
        )]);

        let warnings = find_default_changes(&plan, &baseline);
        assert_eq!(warnings.len(), 1);
        let w = &warnings[0];
        assert_eq!(w.table, "orders");
        assert_eq!(w.column, "tracking_id");
        assert_eq!(w.old_default.as_deref(), Some("'manual'"));
        assert_eq!(w.new_default.as_deref(), Some("gen_random_uuid()"));
        assert_eq!(w.kind, DefaultChangeKind::LiteralToFunction);
        assert_eq!(w.kind.risk_level(), RiskLevel::High);
    }

    #[test]
    fn find_default_changes_preserves_action_index_order() {
        let baseline = vec![
            table_with("a", vec![col_with_default("col", Some("'old'"))]),
            table_with("b", vec![col_with_default("col", Some("'old'"))]),
            table_with("c", vec![col_with_default("col", Some("'old'"))]),
        ];
        // Out-of-order tables in the plan; preserved order is action index.
        let plan = plan_with(vec![
            modify_default("c", "col", Some("'new'")),
            modify_default("a", "col", Some("'new'")),
            modify_default("b", "col", Some("'new'")),
        ]);

        let warnings = find_default_changes(&plan, &baseline);
        assert_eq!(warnings.len(), 3);
        assert_eq!(warnings[0].action_index, 0);
        assert_eq!(warnings[0].table, "c");
        assert_eq!(warnings[1].action_index, 1);
        assert_eq!(warnings[1].table, "a");
        assert_eq!(warnings[2].action_index, 2);
        assert_eq!(warnings[2].table, "b");
    }

    /// Suppression: `RemapEnumValues` on the same `(table, column)` causes
    /// the `ModifyColumnDefault` warning to be skipped. The integer-enum
    /// remap already rewrites every existing row to the new value, so
    /// asking the user to "backfill" the default would duplicate intent
    /// (or, with a literal-to-literal default change, overwrite the
    /// row-specific remap targets).
    #[test]
    fn find_default_changes_suppressed_by_remap_enum_values() {
        let baseline = vec![table_with(
            "user",
            vec![col_with_default("priority", Some("0"))],
        )];
        let plan = plan_with(vec![
            MigrationAction::RemapEnumValues {
                table: TableName::from("user"),
                column: ColumnName::from("priority"),
                mapping: std::collections::BTreeMap::from([(0_i64, 100_i64), (100_i64, 101_i64)]),
            },
            modify_default("user", "priority", Some("100")),
        ]);

        let warnings = find_default_changes(&plan, &baseline);
        assert!(
            warnings.is_empty(),
            "default change on column with RemapEnumValues must be suppressed; got: {warnings:?}"
        );
    }

    /// Suppression: `ModifyColumnType` on the same `(table, column)` causes
    /// the `ModifyColumnDefault` warning to be skipped. The user has
    /// already acknowledged the type/value change via the narrowing /
    /// enum-fill-with prompt; the default reshuffle is a natural follow-on.
    #[test]
    fn find_default_changes_suppressed_by_modify_column_type() {
        let baseline = vec![table_with(
            "user",
            vec![col_with_default("status", Some("'pending'"))],
        )];
        let plan = plan_with(vec![
            MigrationAction::ModifyColumnType {
                table: TableName::from("user"),
                column: ColumnName::from("status"),
                new_type: ColumnType::Simple(SimpleColumnType::Text),
                fill_with: None,
                narrowing_strategy: None,
                timezone: None,
            },
            modify_default("user", "status", Some("'active'")),
        ]);

        assert!(find_default_changes(&plan, &baseline).is_empty());
    }

    /// Suppression is column-scoped: a `ModifyColumnType` on *another*
    /// column in the same plan does not silence the warning for the
    /// affected column.
    #[test]
    fn find_default_changes_not_suppressed_for_unrelated_column() {
        let baseline = vec![table_with(
            "user",
            vec![
                col_with_default("status", Some("'pending'")),
                col_with_default("note", None),
            ],
        )];
        let plan = plan_with(vec![
            MigrationAction::ModifyColumnType {
                table: TableName::from("user"),
                column: ColumnName::from("note"),
                new_type: ColumnType::Simple(SimpleColumnType::Text),
                fill_with: None,
                narrowing_strategy: None,
                timezone: None,
            },
            modify_default("user", "status", Some("'active'")),
        ]);

        let warnings = find_default_changes(&plan, &baseline);
        assert_eq!(warnings.len(), 1, "expected unrelated-column warning");
        assert_eq!(warnings[0].column, "status");
    }

    #[test]
    fn find_default_changes_missing_baseline_column_yields_added_default() {
        // No baseline column → lookup_old_default returns None → treated
        // as AddedDefault even though the plan says "modify".
        let baseline: Vec<TableDef> = vec![];
        let plan = plan_with(vec![modify_default("unknown", "col", Some("'fresh'"))]);

        let warnings = find_default_changes(&plan, &baseline);
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].kind, DefaultChangeKind::AddedDefault);
    }

    // ── Coverage-closure: DefaultChangeKind::label() across every variant ──

    /// Direct `.label()` call covering each match arm (lines 102-109).
    #[test]
    fn label_returns_short_human_label_per_variant() {
        assert_eq!(DefaultChangeKind::AddedDefault.label(), "added default");
        assert_eq!(DefaultChangeKind::RemovedDefault.label(), "removed default");
        assert_eq!(
            DefaultChangeKind::LiteralToLiteral.label(),
            "literal → literal"
        );
        assert_eq!(
            DefaultChangeKind::LiteralToFunction.label(),
            "literal → function"
        );
        assert_eq!(
            DefaultChangeKind::FunctionToLiteral.label(),
            "function → literal"
        );
        assert_eq!(
            DefaultChangeKind::FunctionToFunction.label(),
            "function → function"
        );
    }

    /// `lookup_old_default` returns `None` when the baseline table is
    /// present but the column is not — exercises the second `find(...)?`
    /// chain (line 190 area).
    #[test]
    fn lookup_old_default_missing_column_yields_added_default() {
        let baseline = vec![table_with(
            "users",
            vec![col_with_default("id", None)], // no `email` column
        )];
        let plan = plan_with(vec![modify_default("users", "email", Some("'x'"))]);

        let warnings = find_default_changes(&plan, &baseline);
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].kind, DefaultChangeKind::AddedDefault);
    }

    /// `classify` `(None, None)` defensive fall-through (line 219 area).
    #[test]
    fn classify_none_to_none_defensive_returns_literal_to_literal() {
        // Direct unit-call: defensive arm cannot be hit through the
        // public pipeline (the action implies at least one side is set),
        // so we cover it explicitly here.
        assert_eq!(classify(None, None), DefaultChangeKind::LiteralToLiteral);
    }

    /// `is_function_expr` empty / whitespace input (defensive guard line
    /// 231-233).
    #[test]
    fn is_function_expr_empty_returns_false() {
        assert!(!is_function_expr(""));
        assert!(!is_function_expr("   "));
    }
}
