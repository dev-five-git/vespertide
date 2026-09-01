use anyhow::{Context, Result};
use colored::Colorize;
use dialoguer::{Confirm, Input, Select};
use vespertide_core::MigrationAction;
use vespertide_planner::TimezoneConversionWarning;

use super::super::timezones::{KNOWN_IANA, validate_timezone};

/// Sentinel labels appended after the IANA whitelist in the Select UI.
const CUSTOM_IANA_LABEL: &str = "Custom IANA name (validated against whitelist)";
const CUSTOM_OFFSET_LABEL: &str = "Custom UTC offset (±HH:MM)";

/// Prompt the user to pick a timezone for every `timestamp ⇄ timestamptz`
/// conversion queued in the current migration plan.
///
/// Returns `Ok(Some(choices))` with one timezone string per warning (in the
/// input order) on successful completion. Returns `Ok(None)` when the user
/// explicitly declines via the trailing Confirm or the validation loop fails
/// repeatedly (after 3 attempts).
#[cfg(not(tarpaulin_include))] // reason: interactive stdin/dialoguer prompt, not unit-testable
pub(in crate::commands::revision) fn prompt_timezone_conversions(
    warnings: &[TimezoneConversionWarning],
) -> Result<Option<Vec<String>>> {
    println!(
        "\n{} {}",
        "\u{26a0}".bright_yellow(),
        format!(
            "{} timestamp \u{21c4} timestamptz conversion(s) detected \
             \u{2014} a timezone is required for safe migration:",
            warnings.len()
        )
        .bright_yellow()
    );

    // Build the Select item list once: 30 IANA entries plus 2 custom slots.
    let mut items: Vec<String> = KNOWN_IANA.iter().map(|s| (*s).to_string()).collect();
    items.push(CUSTOM_IANA_LABEL.to_string());
    items.push(CUSTOM_OFFSET_LABEL.to_string());

    let mut choices = Vec::with_capacity(warnings.len());
    for (idx, w) in warnings.iter().enumerate() {
        super::print_section_rule();
        println!(
            "  {} {}/{}: {} ({})",
            "\u{25b6}".bright_cyan(),
            idx + 1,
            warnings.len(),
            format!("{}.{}", w.table, w.column).bright_white().bold(),
            w.direction.label().bright_yellow().bold()
        );
        match w.direction {
            vespertide_planner::TimezoneConversionDirection::NaiveToAware => println!(
                "    {} {}",
                "interpretation:".bright_white(),
                "existing naive values will be read AS IF they are in this timezone."
                    .bright_black()
            ),
            vespertide_planner::TimezoneConversionDirection::AwareToNaive => println!(
                "    {} {}",
                "projection:    ".bright_white(),
                "existing aware values will be projected INTO this timezone, then dropped."
                    .bright_black()
            ),
        }
        if let Some(prev) = &w.current_timezone {
            println!(
                "    {} {} {}",
                "currently:".bright_white(),
                prev.bright_cyan(),
                "(picking again will overwrite this)".bright_black()
            );
        }
        println!();

        let selection = Select::new()
            .with_prompt("  Select timezone")
            .items(&items)
            .default(0)
            .interact()
            .context("failed to read timezone selection")?;

        let tz = if selection < KNOWN_IANA.len() {
            KNOWN_IANA[selection].to_string()
        } else {
            // Custom path: ask for free-text and run validate_timezone with
            // up to 3 retries. After 3 failures the prompt cancels — the user
            // can re-run with `--timezone` later (future flag).
            let label = items[selection].as_str();
            match prompt_custom_timezone_with_retry(label, 3)? {
                Some(custom) => custom,
                None => return Ok(None),
            }
        };
        println!(
            "  {} {}",
            "selected:".bright_white(),
            tz.bright_green().bold()
        );
        choices.push(tz);
    }
    super::print_section_rule();
    Ok(Some(choices))
}

#[cfg(not(tarpaulin_include))] // reason: interactive stdin/dialoguer prompt, not unit-testable
fn prompt_custom_timezone_with_retry(label: &str, max_attempts: u8) -> Result<Option<String>> {
    for attempt in 1..=max_attempts {
        let raw: String = Input::new()
            .with_prompt(format!("  {label}"))
            .interact_text()
            .context("failed to read custom timezone")?;
        match validate_timezone(&raw) {
            Ok(tz) => return Ok(Some(tz)),
            Err(why) => {
                println!("  {} {}", "\u{2717}".bright_red(), why);
                if attempt < max_attempts {
                    println!(
                        "  {} {} attempts left",
                        "\u{21bb}".bright_yellow(),
                        max_attempts - attempt
                    );
                }
            }
        }
    }
    Ok(None)
}

/// F7-(b) — surface every `RemapEnumValues` action that the planner emit
/// and force the user to acknowledge the *automatic data rewrite*. We do
/// not provide an "edit" option here because the mapping is fully
/// determined by the model diff; the user's only choice is proceed /
/// cancel. Cancelling lets them revisit the model (e.g. revert the value
/// change, or coordinate with downstream consumers first).
#[cfg(not(tarpaulin_include))] // reason: interactive stdin/dialoguer prompt, not unit-testable
pub(in crate::commands::revision) fn prompt_remap_enum_values(
    plan: &vespertide_core::MigrationPlan,
) -> Result<bool> {
    let remaps: Vec<&MigrationAction> = plan
        .actions
        .iter()
        .filter(|a| matches!(a, MigrationAction::RemapEnumValues { .. }))
        .collect();
    if remaps.is_empty() {
        return Ok(true);
    }

    println!(
        "\n{} {}",
        "\u{26a0}".bright_yellow(),
        format!(
            "{} integer enum value remap(s) detected \u{2014} existing rows will be \
             AUTOMATICALLY rewritten by UPDATE ... CASE WHEN:",
            remaps.len()
        )
        .bright_yellow()
    );
    super::print_section_rule();
    for action in &remaps {
        if let MigrationAction::RemapEnumValues {
            table,
            column,
            mapping,
        } = action
        {
            let summary = mapping
                .iter()
                .map(|(old, new)| format!("{old}\u{2192}{new}"))
                .collect::<Vec<_>>()
                .join(", ");
            println!(
                "  {} {}.{} [{}]",
                "\u{2022}".bright_cyan(),
                table.as_str().bright_white(),
                column.as_str().bright_green(),
                summary.bright_white()
            );
        }
    }
    super::print_section_rule();
    println!(
        "  {} {}",
        "\u{26a0}".bright_red(),
        "This rewrite runs the moment the migration is applied. \
         Coordinate with all running ORM consumers BEFORE proceeding."
            .bright_red()
    );

    let confirmed = Confirm::new()
        .with_prompt("  I have coordinated downstream consumers. Apply remap?")
        .default(false)
        .interact()
        .context("failed to read confirmation")?;
    Ok(confirmed)
}

/// Apply user-supplied timezones onto the plan in place. Each warning's
/// `action_index` points at the `ModifyColumnType` action it came from.
pub(in crate::commands::revision) fn apply_timezone_choices_to_plan(
    plan: &mut vespertide_core::MigrationPlan,
    warnings: &[TimezoneConversionWarning],
    choices: &[String],
) {
    for (warning, choice) in warnings.iter().zip(choices) {
        if let Some(MigrationAction::ModifyColumnType { timezone, .. }) =
            plan.actions.get_mut(warning.action_index)
        {
            *timezone = Some(choice.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vespertide_core::{ColumnName, ColumnType, MigrationPlan, SimpleColumnType, TableName};
    use vespertide_planner::TimezoneConversionDirection;

    fn warning(idx: usize) -> TimezoneConversionWarning {
        TimezoneConversionWarning {
            action_index: idx,
            table: "events".into(),
            column: "at".into(),
            direction: TimezoneConversionDirection::NaiveToAware,
            current_timezone: None,
        }
    }

    fn plan_with_modify() -> MigrationPlan {
        MigrationPlan {
            id: String::new(),
            comment: None,
            created_at: None,
            version: 1,
            actions: vec![MigrationAction::ModifyColumnType {
                table: TableName::from("events"),
                column: ColumnName::from("at"),
                new_type: ColumnType::Simple(SimpleColumnType::Timestamptz),
                fill_with: None,
                narrowing_strategy: None,
                timezone: None,
            }],
        }
    }

    #[test]
    fn apply_timezone_choices_writes_chosen_tz_to_matching_action() {
        let mut plan = plan_with_modify();
        apply_timezone_choices_to_plan(&mut plan, &[warning(0)], &["America/New_York".to_string()]);
        let MigrationAction::ModifyColumnType { timezone, .. } = &plan.actions[0] else {
            panic!()
        };
        assert_eq!(timezone.as_deref(), Some("America/New_York"));
    }

    #[test]
    fn apply_timezone_choices_ignores_out_of_range_and_wrong_variant() {
        let mut plan = plan_with_modify();
        apply_timezone_choices_to_plan(&mut plan, &[warning(99)], &["UTC".into()]);
        let MigrationAction::ModifyColumnType { timezone, .. } = &plan.actions[0] else {
            panic!()
        };
        assert_eq!(*timezone, None);

        let mut plan2 = MigrationPlan {
            id: String::new(),
            comment: None,
            created_at: None,
            version: 1,
            actions: vec![MigrationAction::RawSql { sql: "x".into() }],
        };
        apply_timezone_choices_to_plan(&mut plan2, &[warning(0)], &["UTC".into()]);
        assert!(matches!(plan2.actions[0], MigrationAction::RawSql { .. }));
    }

    #[test]
    fn apply_timezone_choices_zip_stops_at_shorter_slice() {
        let mut plan = plan_with_modify();
        // Two warnings, one choice → only first applied.
        apply_timezone_choices_to_plan(&mut plan, &[warning(0), warning(0)], &["UTC".into()]);
    }
}
