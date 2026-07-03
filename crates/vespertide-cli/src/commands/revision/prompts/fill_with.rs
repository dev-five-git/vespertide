use std::collections::{BTreeMap, HashMap, HashSet};

use anyhow::{Context, Result};
use colored::Colorize;
use dialoguer::{Confirm, Input, Select};
use vespertide_core::{MigrationAction, MigrationPlan, TableDef};
use vespertide_planner::{EnumFillWithRequired, FillWithRequired, find_missing_enum_fill_with};

use super::super::emit::apply_enum_fill_with_to_plan;
#[cfg(test)]
use super::super::emit::apply_fill_with_to_plan;

/// Format the type info string for display.
/// Includes column type and default value hint if available.
pub(in crate::commands::revision) fn format_type_info(
    column_type: &str,
    default_value: &str,
) -> String {
    format!(" ({column_type}, default: {default_value})")
}

/// Format a single `fill_with` item for display.
pub(in crate::commands::revision) fn format_fill_with_item(
    table: &str,
    column: &str,
    type_info: &str,
    action_type: &str,
) -> String {
    format!(
        "  {} {}.{}{}\n    {} {}",
        "•".bright_cyan(),
        table.bright_white(),
        column.bright_green(),
        type_info.bright_black(),
        "Action:".bright_black(),
        action_type.bright_magenta()
    )
}

/// Format the prompt string for interactive input.
pub(in crate::commands::revision) fn format_fill_with_prompt(table: &str, column: &str) -> String {
    format!(
        "  Enter fill value for {}.{}",
        table.bright_white(),
        column.bright_green()
    )
}

/// Print the header for `fill_with` prompts.
pub(in crate::commands::revision) fn print_fill_with_header() {
    println!(
        "\n{} {}",
        "⚠".bright_yellow(),
        "The following columns require fill_with values:".bright_yellow()
    );
    println!("{}", "─".repeat(60).bright_black());
}

/// Print the footer for `fill_with` prompts.
pub(in crate::commands::revision) fn print_fill_with_footer() {
    println!("{}", "─".repeat(60).bright_black());
}

/// Print a `fill_with` item and return the formatted prompt.
pub(in crate::commands::revision) fn print_fill_with_item_and_get_prompt(
    table: &str,
    column: &str,
    column_type: &str,
    default_value: &str,
    action_type: &str,
) -> String {
    let type_info = format_type_info(column_type, default_value);
    let item_display = format_fill_with_item(table, column, &type_info, action_type);
    println!("{item_display}");
    format_fill_with_prompt(table, column)
}

/// Wrap a value with single quotes if it contains spaces and isn't already quoted.
pub(in crate::commands::revision) fn wrap_if_spaces(value: String) -> String {
    if value.is_empty() {
        return value;
    }
    // Already wrapped with single quotes
    if value.starts_with('\'') && value.ends_with('\'') {
        return value;
    }
    // Contains spaces: wrap with single quotes
    if value.contains(' ') {
        return format!("'{value}'");
    }
    value
}

/// Prompt the user for a `fill_with` value using dialoguer.
/// This function wraps terminal I/O and cannot be unit tested without a real terminal.
#[cfg(not(tarpaulin_include))] // reason: interactive stdin/dialoguer prompt, not unit-testable
pub(in crate::commands::revision) fn prompt_fill_with_value(
    prompt: &str,
    default: &str,
) -> Result<String> {
    let value: String = Input::new()
        .with_prompt(prompt)
        .default(default.to_string())
        .interact_text()
        .context("failed to read input")?;
    Ok(wrap_if_spaces(value))
}

/// Prompt the user to select an enum value using dialoguer Select.
/// Returns the selected value wrapped in single quotes for SQL.
#[cfg(not(tarpaulin_include))] // reason: interactive stdin/dialoguer prompt, not unit-testable
pub(in crate::commands::revision) fn prompt_enum_value(
    prompt: &str,
    enum_values: &[String],
) -> Result<String> {
    let selection = Select::new()
        .with_prompt(prompt)
        .items(enum_values)
        .default(0)
        .interact()
        .context("failed to read selection")?;
    // Return the selected value with single quotes for SQL enum literal
    Ok(format!("'{}'", enum_values[selection]))
}

/// Prompt for enum value selection and return bare (unquoted) value.
/// Used by `cmd_revision` for enum `fill_with` collection where `BTreeMap` stores bare names.
#[cfg(not(tarpaulin_include))] // reason: interactive stdin/dialoguer prompt, not unit-testable
pub(in crate::commands::revision) fn prompt_enum_value_bare(
    prompt: &str,
    values: &[String],
) -> Result<String> {
    let selected = prompt_enum_value(prompt, values)?;
    Ok(strip_enum_quotes(&selected))
}

/// Strip SQL single-quotes from an enum value string.
/// `BTreeMap` stores bare enum names; the SQL layer handles quoting via `Expr::val()`.
pub(in crate::commands::revision) fn strip_enum_quotes(value: &str) -> String {
    value
        .trim_start_matches('\'')
        .trim_end_matches('\'')
        .to_string()
}

/// Collect `fill_with` values interactively for missing columns.
/// The `prompt_fn` parameter allows injecting a mock for testing.
/// The `enum_prompt_fn` parameter handles enum type columns with selection UI.
pub(in crate::commands::revision) fn collect_fill_with_values<F, E>(
    missing: &[vespertide_planner::FillWithRequired],
    fill_values: &mut HashMap<(String, String), String>,
    prompt_fn: F,
    enum_prompt_fn: E,
) -> Result<()>
where
    F: Fn(&str, &str) -> Result<String>,
    E: Fn(&str, &[String]) -> Result<String>,
{
    print_fill_with_header();

    for item in missing {
        let prompt = print_fill_with_item_and_get_prompt(
            &item.table,
            &item.column,
            &item.column_type,
            &item.default_value,
            item.action_type,
        );

        let value = if let Some(enum_values) = &item.enum_values {
            // Use selection UI for enum types
            enum_prompt_fn(&prompt, enum_values)?
        } else {
            // Use text input with default pre-filled
            prompt_fn(&prompt, &item.default_value)?
        };
        fill_values.insert((item.table.clone(), item.column.clone()), value);
    }

    print_fill_with_footer();
    Ok(())
}

/// Handle interactive `fill_with` collection if there are missing values.
/// Returns the updated `fill_values` map after collecting from user.
#[cfg(test)]
pub(in crate::commands::revision) fn handle_missing_fill_with<F, E>(
    plan: &mut MigrationPlan,
    fill_values: &mut HashMap<(String, String), String>,
    current_schema: &[TableDef],
    prompt_fn: F,
    enum_prompt_fn: E,
) -> Result<()>
where
    F: Fn(&str, &str) -> Result<String>,
    E: Fn(&str, &[String]) -> Result<String>,
{
    let missing = vespertide_planner::find_missing_fill_with(plan, current_schema);

    if !missing.is_empty() {
        collect_fill_with_values(&missing, fill_values, prompt_fn, enum_prompt_fn)?;

        // Apply the collected fill_with values
        apply_fill_with_to_plan(plan, fill_values);
    }

    Ok(())
}

#[cfg(not(tarpaulin_include))] // reason: interactive stdin/dialoguer prompt, not unit-testable
pub(in crate::commands::revision) fn prompt_delete_null_rows(
    table: &str,
    column: &str,
) -> Result<bool> {
    let confirmed = Confirm::new()
        .with_prompt(format!("  Delete rows where {table}.{column} IS NULL?"))
        .default(false)
        .interact()
        .context("failed to read confirmation")?;
    Ok(confirmed)
}

pub(in crate::commands::revision) fn handle_delete_null_rows<F>(
    plan: &mut MigrationPlan,
    missing: &mut Vec<FillWithRequired>,
    delete_set: &HashSet<(String, String)>,
    prompt_fn: F,
) -> Result<()>
where
    F: Fn(&str, &str) -> Result<bool>,
{
    let mut to_delete = Vec::new();
    let mut remaining = Vec::new();

    for item in missing.drain(..) {
        if item.has_foreign_key && !delete_set.contains(&(item.table.clone(), item.column.clone()))
        {
            // FK column without CLI arg — prompt user
            println!(
                "  {} {}.{} has a foreign key constraint — fill_with may not work.",
                "\u{2022}".bright_cyan(),
                item.table.bright_white(),
                item.column.bright_green()
            );
            if prompt_fn(&item.table, &item.column)? {
                to_delete.push((item.table.clone(), item.column.clone()));
            } else {
                remaining.push(item);
            }
        } else if delete_set.contains(&(item.table.clone(), item.column.clone())) {
            to_delete.push((item.table.clone(), item.column.clone()));
        } else {
            remaining.push(item);
        }
    }

    // Apply delete_null_rows to plan
    for (table, column) in &to_delete {
        for action in &mut plan.actions {
            if let MigrationAction::ModifyColumnNullable {
                table: t,
                column: c,
                delete_null_rows,
                ..
            } = action
                && t == table
                && c == column
            {
                *delete_null_rows = Some(true);
            }
        }
    }

    *missing = remaining;
    Ok(())
}

/// Collect enum `fill_with` values interactively for removed enum values.
/// The `enum_prompt_fn` parameter handles enum type columns with selection UI.
///
/// **F23 rename heuristic**: for each removed value, compute the most
/// string-similar surviving value via Levenshtein distance. When the best
/// match is "close enough" (see [`SIMILARITY_THRESHOLD`]) we:
/// 1. Print a "(suggested: 'X' is new — likely rename)" hint above the prompt.
/// 2. Reorder `remaining_values` so the suggestion appears at index 0, which
///    becomes the `Select::default(0)` choice — pressing Enter applies the
///    likely rename. The user can still arrow-down to pick any other value.
///
/// The original ordering of `remaining_values` is preserved for every entry
/// other than the suggestion (which is hoisted to the top), so non-suggested
/// options remain in a predictable order.
pub(in crate::commands::revision) fn collect_enum_fill_with_values<E>(
    missing: &[EnumFillWithRequired],
    enum_prompt_fn: E,
) -> Result<Vec<(usize, BTreeMap<String, String>)>>
where
    E: Fn(&str, &[String]) -> Result<String>,
{
    let mut results = Vec::new();

    println!(
        "\n{} {}",
        "\u{26a0}".bright_yellow(),
        "The following enum value removals require replacement mappings:".bright_yellow()
    );
    println!("{}", "\u{2500}".repeat(60).bright_black());

    for item in missing {
        println!(
            "  {} {}.{}: removing enum values",
            "\u{2022}".bright_cyan(),
            item.table.bright_white(),
            item.column.bright_green()
        );

        let mut mappings = BTreeMap::new();
        for removed in &item.removed_values {
            let suggestion = best_rename_candidate(removed, &item.remaining_values);
            let mut prompt = format!(
                "  Replace '{}' in {}.{} with",
                removed.bright_red(),
                item.table.bright_white(),
                item.column.bright_green()
            );
            if let Some(suggested) = &suggestion {
                prompt = format!(
                    "{prompt}\n    {} {} '{}' is new — likely rename",
                    "(suggested:".bright_cyan(),
                    suggested.bright_green(),
                    "press Enter to accept)".bright_cyan()
                );
            }
            let ordered = reorder_with_suggestion(&item.remaining_values, suggestion.as_deref());
            let value = enum_prompt_fn(&prompt, &ordered)?;
            mappings.insert(removed.clone(), value);
        }
        results.push((item.action_index, mappings));
    }

    println!("{}", "\u{2500}".repeat(60).bright_black());
    Ok(results)
}

/// Levenshtein-distance threshold under which a surviving value is treated as
/// a likely rename of the removed value. Empirically picked: `≤ 3` catches
/// common rename patterns (`pending` → `awaiting`, `cancelled` → `canceled`,
/// `inprogress` → `in_progress`) without false-positive recommending unrelated
/// values like `active` → `banned`.
const SIMILARITY_THRESHOLD: usize = 3;

/// Pick the surviving value most string-similar to `removed`, or `None` when
/// nothing is within [`SIMILARITY_THRESHOLD`]. Ties are broken by `remaining`'s
/// declaration order so the suggestion is deterministic for snapshots and
/// repeated runs.
pub(in crate::commands::revision) fn best_rename_candidate(
    removed: &str,
    remaining: &[String],
) -> Option<String> {
    let mut best: Option<(usize, &String)> = None;
    for candidate in remaining {
        let d = strsim::levenshtein(removed, candidate);
        if d > SIMILARITY_THRESHOLD {
            continue;
        }
        match best {
            None => best = Some((d, candidate)),
            Some((current_d, _)) if d < current_d => best = Some((d, candidate)),
            _ => {}
        }
    }
    best.map(|(_, c)| c.clone())
}

/// Hoist `suggestion` to index 0 of `values` while preserving the relative
/// order of every other entry. When `suggestion` is `None` or not present in
/// `values`, returns `values` unchanged.
fn reorder_with_suggestion(values: &[String], suggestion: Option<&str>) -> Vec<String> {
    let Some(s) = suggestion else {
        return values.to_vec();
    };
    let mut out: Vec<String> = Vec::with_capacity(values.len());
    out.push(s.to_string());
    for v in values {
        if v != s {
            out.push(v.clone());
        }
    }
    out
}

/// Handle interactive enum `fill_with` collection if there are missing values.
pub(in crate::commands::revision) fn handle_missing_enum_fill_with<E>(
    plan: &mut MigrationPlan,
    current_schema: &[TableDef],
    enum_prompt_fn: E,
) -> Result<()>
where
    E: Fn(&str, &[String]) -> Result<String>,
{
    let missing = find_missing_enum_fill_with(plan, current_schema);

    if !missing.is_empty() {
        let collected = collect_enum_fill_with_values(&missing, enum_prompt_fn)?;
        apply_enum_fill_with_to_plan(plan, &collected);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reorder_with_suggestion_hoists_when_present() {
        let values = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        let out = reorder_with_suggestion(&values, Some("b"));
        assert_eq!(out, vec!["b".to_string(), "a".to_string(), "c".to_string()]);
    }

    #[test]
    fn reorder_with_suggestion_none_passthrough() {
        let values = vec!["x".to_string(), "y".to_string()];
        assert_eq!(reorder_with_suggestion(&values, None), values);
    }

    // "already-wrapped" requires BOTH a leading AND trailing quote. A value
    // that only starts with a quote but contains a space must still be wrapped.
    // Pins `starts_with('\'') && ends_with('\'')`: a `||` mutant would treat
    // the half-quoted value as already wrapped and skip the space-wrapping.
    #[test]
    fn wrap_if_spaces_wraps_half_quoted_value_with_spaces() {
        assert_eq!(wrap_if_spaces("'a b".to_string()), "''a b'");
    }

    #[test]
    fn reorder_with_suggestion_missing_still_hoists_inserts_suggestion() {
        let values = vec!["a".to_string(), "b".to_string()];
        // Suggestion not in list -> still hoisted to front, original order preserved.
        let out = reorder_with_suggestion(&values, Some("missing"));
        assert_eq!(
            out,
            vec!["missing".to_string(), "a".to_string(), "b".to_string()]
        );
    }

    #[test]
    fn reorder_with_suggestion_already_first_stays_at_front() {
        let values = vec!["x".to_string(), "y".to_string()];
        assert_eq!(reorder_with_suggestion(&values, Some("x")), values);
    }
}
