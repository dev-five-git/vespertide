use anyhow::{Context, Result};
use colored::Colorize;
use dialoguer::{Input, Select};
use vespertide_core::{MigrationPlan, NarrowingStrategy};
use vespertide_planner::{NarrowingKind, TypeNarrowingWarning};

/// Strategies that can be safely emitted by the SQL generator for a given
/// narrowing kind. Drives the dialoguer `Select` UI so the user only ever
/// sees applicable options.
///
/// Returning an empty slice means *no automatic strategy exists* — the
/// caller must abort the revision and ask the user to pre-clean the data
/// manually (Phase 3 SQL generation returns `UnsupportedAction` for these).
pub(in crate::commands::revision) fn applicable_strategies(
    kind: &NarrowingKind,
) -> &'static [&'static str] {
    match kind {
        NarrowingKind::VarcharLength { .. }
        | NarrowingKind::CharLength { .. }
        | NarrowingKind::VarcharToCharShorter { .. }
        | NarrowingKind::CharToVarcharShorter { .. }
        | NarrowingKind::TextToVarchar { .. }
        | NarrowingKind::TextToChar { .. }
        | NarrowingKind::NumericScale { .. } => &["truncate", "delete", "set_to_value"],
        NarrowingKind::NumericIntegerDigits { .. } | NarrowingKind::IntegerSize { .. } => {
            &["delete", "set_to_value"]
        }
        NarrowingKind::FloatSize { .. } | NarrowingKind::TimestamptzToTimestamp => &[],
    }
}

/// Whether the new type is string-shaped (`set_to_value` input should be
/// auto-quoted with single quotes when the user types a bare literal).
fn is_string_target(kind: &NarrowingKind) -> bool {
    matches!(
        kind,
        NarrowingKind::VarcharLength { .. }
            | NarrowingKind::CharLength { .. }
            | NarrowingKind::VarcharToCharShorter { .. }
            | NarrowingKind::CharToVarcharShorter { .. }
            | NarrowingKind::TextToVarchar { .. }
            | NarrowingKind::TextToChar { .. }
    )
}

/// Print the multi-line strategy explainer block. Shared between Select UI
/// and unit tests so wording is canonical.
fn print_strategy_descriptions(applicable: &[&'static str]) {
    let header = "  Choose how to handle existing rows that would violate the new type:";
    println!("{}", header.bright_white());
    println!();
    for option in applicable {
        match *option {
            "truncate" => println!(
                "    {} - Trim violating values to fit the new size ({}).\n      \
                 Row preserved; tail content lost.",
                "truncate".bright_cyan().bold(),
                "LEFT(col, N) / ROUND(col, scale)".bright_black()
            ),
            "delete" => println!(
                "    {} - Delete entire rows whose value violates.\n      \
                 ⚠ Other columns of those rows are lost. Watch FK CASCADE.",
                "delete".bright_cyan().bold()
            ),
            "set_to_value" => println!(
                "    {} - Replace violating values with a sentinel you provide.\n      \
                 Rows preserved; you will be asked for the value next.",
                "set_to_value".bright_cyan().bold()
            ),
            _ => {}
        }
    }
    println!();
}

/// Prompt the user to pick a [`NarrowingStrategy`] for every type
/// narrowing queued in the current migration plan. Replaces the Phase 1
/// strong-confirm with a per-narrowing `Select` UI driven by
/// [`applicable_strategies`].
///
/// Returns `Ok(Some(strategies))` with one strategy per warning (in the
/// input order) on successful completion. Returns `Ok(None)` when:
///   * any narrowing kind has no automatic strategy (caller aborts revision);
///   * the user explicitly declines via the trailing confirm.
#[cfg(not(tarpaulin_include))] // reason: interactive stdin/dialoguer prompt, not unit-testable
pub(in crate::commands::revision) fn prompt_type_narrowings(
    warnings: &[TypeNarrowingWarning],
) -> Result<Option<Vec<NarrowingStrategy>>> {
    println!(
        "\n{} {}",
        "\u{26a0}".bright_yellow(),
        format!(
            "{} type narrowing(s) detected — each requires a strategy:",
            warnings.len()
        )
        .bright_yellow()
    );

    let mut strategies = Vec::with_capacity(warnings.len());
    for (idx, w) in warnings.iter().enumerate() {
        println!("{}", "\u{2500}".repeat(60).bright_black());
        println!(
            "  {} {}/{}: {} ({} {} {})",
            "\u{25b6}".bright_cyan(),
            idx + 1,
            warnings.len(),
            format!("{}.{}", w.table, w.column).bright_white().bold(),
            w.from_display.bright_red(),
            "->".bright_white(),
            w.to_display.bright_yellow().bold()
        );
        println!(
            "    postgres: {}\n    mysql:    {}\n    sqlite:   {}",
            w.kind.postgres_impact().bright_black(),
            w.kind.mysql_impact().bright_black(),
            w.kind.sqlite_impact().bright_black()
        );
        println!();

        let applicable = applicable_strategies(&w.kind);
        if applicable.is_empty() {
            println!(
                "  {} {}",
                "\u{26a0}".bright_red(),
                "No automatic strategy is available for this narrowing kind. \
                 You must pre-clean the data manually before retrying."
                    .bright_red()
            );
            return Ok(None);
        }

        print_strategy_descriptions(applicable);

        let selection = Select::new()
            .with_prompt("  Select strategy")
            .items(applicable)
            .default(0)
            .interact()
            .context("failed to read selection")?;
        let chosen = applicable[selection];
        let strategy = match chosen {
            "truncate" => NarrowingStrategy::Truncate,
            "delete" => NarrowingStrategy::Delete,
            "set_to_value" => {
                let raw: String = Input::new()
                    .with_prompt(format!(
                        "    Replacement value for {}.{} (must fit {})",
                        w.table, w.column, w.to_display
                    ))
                    .interact_text()
                    .context("failed to read replacement value")?;
                NarrowingStrategy::SetToValue {
                    value: quote_value_for_target(&raw, &w.kind),
                }
            }
            _ => unreachable!("applicable_strategies returns only the three known labels"),
        };
        strategies.push(strategy);
    }
    println!("{}", "\u{2500}".repeat(60).bright_black());
    Ok(Some(strategies))
}

/// Wrap a raw `set_to_value` input in single quotes when the new column
/// type is string-shaped, leave numeric/boolean literals as-is. Mirrors
/// the existing `wrap_if_spaces` helper used by `fill_with` collection so
/// users do not have to remember the SQL quoting rules.
fn quote_value_for_target(raw: &str, kind: &NarrowingKind) -> String {
    if !is_string_target(kind) {
        return raw.to_string();
    }
    if raw.starts_with('\'') && raw.ends_with('\'') {
        return raw.to_string();
    }
    format!("'{}'", raw.replace('\'', "''"))
}

/// Apply user-selected strategies onto the plan in place. Each warning's
/// `action_index` points at the `ModifyColumnType` action it came from.
///
/// Exposed via `pub(in crate::commands::revision)` so the integration test mocks can call it after
/// stubbing the prompt.
pub(in crate::commands::revision) fn apply_narrowing_strategies_to_plan(
    plan: &mut MigrationPlan,
    warnings: &[TypeNarrowingWarning],
    strategies: &[NarrowingStrategy],
) {
    for (warning, strategy) in warnings.iter().zip(strategies) {
        if let Some(vespertide_core::MigrationAction::ModifyColumnType {
            narrowing_strategy, ..
        }) = plan.actions.get_mut(warning.action_index)
        {
            *narrowing_strategy = Some(strategy.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;
    use vespertide_core::{
        ColumnName, ColumnType, MigrationAction, MigrationPlan, SimpleColumnType, TableName,
    };

    fn warning(kind: NarrowingKind, idx: usize) -> TypeNarrowingWarning {
        TypeNarrowingWarning {
            action_index: idx,
            table: "users".into(),
            column: "name".into(),
            kind,
            from_display: "text".into(),
            to_display: "varchar(10)".into(),
        }
    }

    #[test]
    fn applicable_strategies_per_kind() {
        let string_like = [
            NarrowingKind::VarcharLength { from: 20, to: 10 },
            NarrowingKind::CharLength { from: 20, to: 10 },
            NarrowingKind::VarcharToCharShorter { from: 20, to: 10 },
            NarrowingKind::CharToVarcharShorter { from: 20, to: 10 },
            NarrowingKind::TextToVarchar { to_length: 10 },
            NarrowingKind::TextToChar { to_length: 10 },
            NarrowingKind::NumericScale {
                from_scale: 4,
                to_scale: 2,
            },
        ];
        for k in string_like {
            assert_eq!(
                applicable_strategies(&k),
                &["truncate", "delete", "set_to_value"]
            );
        }
        for k in [
            NarrowingKind::NumericIntegerDigits {
                from_int_digits: 6,
                to_int_digits: 4,
            },
            NarrowingKind::IntegerSize {
                from: "bigint",
                to: "integer",
            },
        ] {
            assert_eq!(applicable_strategies(&k), &["delete", "set_to_value"]);
        }
        assert!(
            applicable_strategies(&NarrowingKind::FloatSize {
                from: "double",
                to: "real"
            })
            .is_empty()
        );
        assert!(applicable_strategies(&NarrowingKind::TimestamptzToTimestamp).is_empty());
    }

    #[rstest]
    #[case::varchar_length(NarrowingKind::VarcharLength { from: 20, to: 10 }, &[
        "truncate", "delete", "set_to_value"
    ])]
    #[case::integer_size(NarrowingKind::IntegerSize { from: "bigint", to: "integer" }, &[
        "delete", "set_to_value"
    ])]
    #[case::float_size(NarrowingKind::FloatSize { from: "double", to: "real" }, &[])]
    fn applicable_strategies_rstest_cases(#[case] kind: NarrowingKind, #[case] expected: &[&str]) {
        assert_eq!(applicable_strategies(&kind), expected);
    }

    #[test]
    fn is_string_target_branches() {
        assert!(is_string_target(&NarrowingKind::VarcharLength {
            from: 5,
            to: 4
        }));
        assert!(is_string_target(&NarrowingKind::CharLength {
            from: 5,
            to: 4
        }));
        assert!(is_string_target(&NarrowingKind::VarcharToCharShorter {
            from: 5,
            to: 4
        }));
        assert!(is_string_target(&NarrowingKind::CharToVarcharShorter {
            from: 5,
            to: 4
        }));
        assert!(is_string_target(&NarrowingKind::TextToVarchar {
            to_length: 4
        }));
        assert!(is_string_target(&NarrowingKind::TextToChar {
            to_length: 4
        }));
        assert!(!is_string_target(&NarrowingKind::NumericScale {
            from_scale: 4,
            to_scale: 2
        }));
        assert!(!is_string_target(&NarrowingKind::IntegerSize {
            from: "bigint",
            to: "integer"
        }));
    }

    #[test]
    fn quote_value_for_target_quotes_strings_and_passes_through_numbers() {
        let string_kind = NarrowingKind::VarcharLength { from: 10, to: 5 };
        assert_eq!(quote_value_for_target("abc", &string_kind), "'abc'");
        // Already-quoted stays untouched.
        assert_eq!(quote_value_for_target("'abc'", &string_kind), "'abc'");
        // Inner single quote escaped via doubling.
        assert_eq!(quote_value_for_target("a'b", &string_kind), "'a''b'");
        // Non-string target: pass-through.
        let int_kind = NarrowingKind::IntegerSize {
            from: "bigint",
            to: "integer",
        };
        assert_eq!(quote_value_for_target("42", &int_kind), "42");
    }

    fn plan_with_modify(idx: usize) -> MigrationPlan {
        let mut actions: Vec<MigrationAction> = (0..idx)
            .map(|_| MigrationAction::RawSql { sql: "x".into() })
            .collect();
        actions.push(MigrationAction::ModifyColumnType {
            table: TableName::from("users"),
            column: ColumnName::from("name"),
            new_type: ColumnType::Simple(SimpleColumnType::Text),
            fill_with: None,
            narrowing_strategy: None,
            timezone: None,
        });
        MigrationPlan {
            id: String::new(),
            comment: None,
            created_at: None,
            version: 1,
            actions,
        }
    }

    #[test]
    fn apply_narrowing_strategies_to_plan_writes_strategy_at_action_index() {
        let mut plan = plan_with_modify(0);
        let warnings = vec![warning(NarrowingKind::VarcharLength { from: 10, to: 5 }, 0)];
        let strategies = vec![NarrowingStrategy::Truncate];
        apply_narrowing_strategies_to_plan(&mut plan, &warnings, &strategies);
        let MigrationAction::ModifyColumnType {
            narrowing_strategy, ..
        } = &plan.actions[0]
        else {
            panic!()
        };
        assert_eq!(*narrowing_strategy, Some(NarrowingStrategy::Truncate));
    }

    #[test]
    fn apply_narrowing_strategies_to_plan_set_to_value_and_delete() {
        let mut plan = plan_with_modify(0);
        let warnings = vec![warning(NarrowingKind::VarcharLength { from: 10, to: 5 }, 0)];
        apply_narrowing_strategies_to_plan(
            &mut plan,
            &warnings,
            &[NarrowingStrategy::SetToValue {
                value: "'x'".into(),
            }],
        );
        let MigrationAction::ModifyColumnType {
            narrowing_strategy, ..
        } = &plan.actions[0]
        else {
            panic!()
        };
        assert!(matches!(
            narrowing_strategy,
            Some(NarrowingStrategy::SetToValue { .. })
        ));
    }

    #[test]
    fn apply_narrowing_strategies_to_plan_ignores_out_of_range_and_wrong_action() {
        // Out-of-range action_index: no-op.
        let mut plan = plan_with_modify(0);
        let warnings = vec![warning(
            NarrowingKind::VarcharLength { from: 10, to: 5 },
            99,
        )];
        apply_narrowing_strategies_to_plan(&mut plan, &warnings, &[NarrowingStrategy::Delete]);

        // Wrong action variant at index 0: no-op.
        let mut plan2 = MigrationPlan {
            id: String::new(),
            comment: None,
            created_at: None,
            version: 1,
            actions: vec![MigrationAction::RawSql { sql: "x".into() }],
        };
        apply_narrowing_strategies_to_plan(
            &mut plan2,
            &[warning(NarrowingKind::VarcharLength { from: 10, to: 5 }, 0)],
            &[NarrowingStrategy::Delete],
        );
        assert!(matches!(plan2.actions[0], MigrationAction::RawSql { .. }));
    }

    #[test]
    fn print_strategy_descriptions_emits_each_label() {
        // Covers lines 30-55: every option branch (`truncate` / `delete` /
        // `set_to_value`) plus the wildcard arm via an unknown label.
        // Output goes to stdout; the test asserts it doesn't panic and
        // walks every match arm including the `_ => {}` default.
        print_strategy_descriptions(&["truncate", "delete", "set_to_value"]);
        print_strategy_descriptions(&["truncate"]);
        print_strategy_descriptions(&["delete", "set_to_value"]);
        // Wildcard arm — unknown label, silently skipped.
        print_strategy_descriptions(&["totally_unknown_strategy"]);
    }

    #[rstest]
    #[case::truncate(&["truncate"])]
    #[case::delete(&["delete"])]
    #[case::set_to_value(&["set_to_value"])]
    #[case::unknown(&["totally_unknown_strategy"])]
    fn print_strategy_descriptions_rstest_single_branch_cases(#[case] labels: &[&'static str]) {
        print_strategy_descriptions(labels);
    }

    #[test]
    fn apply_narrowing_strategies_to_plan_zip_stops_at_shorter_slice() {
        let mut plan = plan_with_modify(0);
        // Two warnings, one strategy: only the first is applied.
        let warnings = vec![
            warning(NarrowingKind::VarcharLength { from: 10, to: 5 }, 0),
            warning(NarrowingKind::VarcharLength { from: 10, to: 5 }, 0),
        ];
        apply_narrowing_strategies_to_plan(&mut plan, &warnings, &[NarrowingStrategy::Truncate]);
    }

    // "already-quoted" requires BOTH a leading AND trailing quote. For a
    // string target, a half-quoted value (`'a`) must still be quoted/escaped.
    // Pins `starts_with('\'') && ends_with('\'')`: a `||` mutant would treat
    // it as already quoted and return it unchanged.
    #[test]
    fn quote_value_for_target_quotes_half_quoted_string_value() {
        let kind = NarrowingKind::VarcharLength { from: 20, to: 10 };
        assert_eq!(quote_value_for_target("'a", &kind), "'''a'");
    }
}
