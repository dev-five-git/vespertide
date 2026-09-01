use std::fmt::Write as _;

use anyhow::Result;
use colored::Colorize;
use vespertide_planner::{
    ConstraintDropWarning, FkPolicyChangeWarning, MissingFkSupportingIndex,
    TimezoneConversionWarning, TypeNarrowingWarning, find_constraint_drops_without_replacement,
    find_fk_policy_changes, find_missing_fk_supporting_indexes, find_timezone_conversions,
    find_type_narrowings, plan_next_migration, render_reference_action, schema_from_plans,
};

use super::raw_sql_warning::emit_raw_sql_replay_warning;
use crate::utils::{load_config, load_migrations, load_models};
use vespertide_core::action::{sql_preview, truncate_comment};
use vespertide_core::{MigrationAction, MigrationPlan, TableDef};

pub async fn cmd_diff() -> Result<()> {
    let config = load_config()?;
    let current_models = load_models(&config)?;
    let applied_plans = load_migrations(&config)?;

    let plan = plan_next_migration(&current_models, &applied_plans)
        .map_err(|e| anyhow::anyhow!("planning error: {e}"))?;

    // Emitted before the action list: when replay was incomplete the list
    // itself is untrustworthy, so the caveat has to arrive first.
    emit_raw_sql_replay_warning(&applied_plans);

    if plan.actions.is_empty() {
        println!(
            "{} {}",
            "No differences found.".bright_green(),
            "Schema is up to date.".bright_white()
        );
    } else {
        println!(
            "{} {} {}",
            "Found".bright_cyan(),
            plan.actions.len().to_string().bright_yellow().bold(),
            "change(s) to apply:".bright_cyan()
        );
        println!();

        for (i, action) in plan.actions.iter().enumerate() {
            println!(
                "{}. {}",
                (i + 1).to_string().bright_magenta().bold(),
                format_action(action)
            );
        }
    }

    // Static safety analyses that run on the current model regardless of
    // whether there are pending actions — these are warnings, not blockers.
    emit_fk_supporting_index_warnings(&current_models);
    emit_constraint_drop_warnings(&plan);
    emit_fk_policy_change_warnings(&plan);

    // Type narrowing + timezone conversion both need the *baseline* schema
    // (the type before this migration). Reconstruct once and reuse.
    // Failure here is non-fatal: we just skip both warnings rather than
    // shadowing the actual diff output.
    if let Ok(baseline) = schema_from_plans(&applied_plans) {
        emit_type_narrowing_warnings(&plan, &baseline);
        emit_timezone_conversion_warnings(&plan, &baseline);
    }

    Ok(())
}

/// Surface `ModifyColumnType` actions that flip a column between
/// `timestamp` and `timestamptz`. This is fault **F20**: without a
/// recorded timezone, the conversion silently shifts every row by the
/// server's (or session's) implicit timezone.
fn emit_timezone_conversion_warnings(plan: &MigrationPlan, baseline: &[TableDef]) {
    let warnings = find_timezone_conversions(plan, baseline);
    if warnings.is_empty() {
        return;
    }

    println!();
    println!(
        "{} {}",
        "⚠".bright_yellow().bold(),
        format!(
            "{} timestamp \u{21c4} timestamptz conversion(s) — a timezone is required:",
            warnings.len()
        )
        .bright_yellow()
    );
    for w in &warnings {
        println!();
        for line in format_timezone_conversion_warning(w).lines() {
            println!("{line}");
        }
    }
}

/// Format a single `TimezoneConversionWarning` as a multi-line indented block.
fn format_timezone_conversion_warning(w: &TimezoneConversionWarning) -> String {
    let direction_explainer = match w.direction {
        vespertide_planner::TimezoneConversionDirection::NaiveToAware => {
            "existing naive values will be read AS IF they are in <tz>"
        }
        vespertide_planner::TimezoneConversionDirection::AwareToNaive => {
            "existing aware values will be projected INTO <tz>, then dropped"
        }
    };
    let mut out = format!(
        "  {} {}\n  {} {}\n  {} {}",
        "on:".bright_white(),
        format!("{}.{}", w.table, w.column).bright_cyan(),
        "direction:".bright_white(),
        w.direction.label().bright_yellow().bold(),
        "why:".bright_white(),
        direction_explainer
    );
    if let Some(tz) = &w.current_timezone {
        let _ = write!(
            out,
            "\n  {} {} {}",
            "currently:".bright_white(),
            tz.bright_cyan(),
            "(revision will skip the prompt)".bright_black()
        );
    } else {
        let _ = write!(
            out,
            "\n  {} run `vespertide revision` and pick a timezone (UTC / IANA / ±HH:MM)",
            "fix:".bright_green()
        );
    }
    out
}

/// Surface `ModifyColumnType` actions that shrink a column's storable value
/// range. This is fault **F6 / F19 / F33 / F87**: the migration SQL may
/// succeed silently on some backends (`MySQL` truncates, `SQLite` ignores)
/// and fail outright on others (`PostgreSQL` rejects with "value too long").
/// Vespertide cannot — and must not — silently apply destructive type
/// changes; the user must explicitly pick a strategy via `revision`.
fn emit_type_narrowing_warnings(plan: &MigrationPlan, baseline: &[TableDef]) {
    let warnings = find_type_narrowings(plan, baseline);
    if warnings.is_empty() {
        return;
    }

    println!();
    println!(
        "{} {}",
        "⚠".bright_yellow().bold(),
        format!(
            "{} type narrowing(s) — existing rows may be truncated, rejected, \
             or silently corrupted depending on backend:",
            warnings.len()
        )
        .bright_yellow()
    );
    for w in &warnings {
        println!();
        for line in format_type_narrowing_warning(w).lines() {
            println!("{line}");
        }
    }
}

/// Format a single `TypeNarrowingWarning` as a multi-line indented block.
/// Backend impacts are shown side by side so the user can see at a glance
/// that the *same migration* behaves differently per backend — which is
/// precisely the silent corruption surface Vespertide is closing.
fn format_type_narrowing_warning(w: &TypeNarrowingWarning) -> String {
    let mut out = format!(
        "  {} {}\n  {} {} {} {}",
        "on:".bright_white(),
        format!("{}.{}", w.table, w.column).bright_cyan(),
        "change:".bright_white(),
        w.from_display.bright_red(),
        "->".bright_white(),
        w.to_display.bright_yellow().bold()
    );
    let _ = write!(
        out,
        "\n  {} {}",
        "postgres:".bright_white(),
        w.kind.postgres_impact().bright_red()
    );
    let _ = write!(
        out,
        "\n  {} {}",
        "mysql:   ".bright_white(),
        w.kind.mysql_impact().bright_red()
    );
    let _ = write!(
        out,
        "\n  {} {}",
        "sqlite:  ".bright_white(),
        w.kind.sqlite_impact().bright_black()
    );
    let _ = write!(
        out,
        "\n  {} pick a `narrowing_strategy` in revision (truncate / delete / set_to_value) \
         so the migration succeeds on every backend",
        "fix:".bright_green()
    );
    out
}

/// Surface `ReplaceConstraint` actions that change FK `on_delete` /
/// `on_update` policy. This is fault **F30**: the migration SQL succeeds,
/// the data is untouched, but application code that assumed the previous
/// policy will silently break at the first DELETE / UPDATE trigger event.
fn emit_fk_policy_change_warnings(plan: &MigrationPlan) {
    let warnings = find_fk_policy_changes(plan);
    if warnings.is_empty() {
        return;
    }

    println!();
    println!(
        "{} {}",
        "⚠".bright_yellow().bold(),
        format!(
            "{} FK policy change(s) — application behavior will silently change:",
            warnings.len()
        )
        .bright_yellow()
    );
    for w in &warnings {
        println!();
        for line in format_fk_policy_change_warning(w).lines() {
            println!("{line}");
        }
    }
}

/// Format a single `FkPolicyChangeWarning` as a multi-line indented block.
/// Extracted so its output can be unit-tested without going through stdout.
fn format_fk_policy_change_warning(w: &FkPolicyChangeWarning) -> String {
    let fk_label = w.constraint_name.as_deref().unwrap_or("(unnamed)");
    let from = format!("{}({})", w.table, w.columns.join(", "));
    let to = format!("{}({})", w.ref_table, w.ref_columns.join(", "));

    let mut out = format!(
        "  {} {}\n  {} {} {} {}",
        "on:".bright_white(),
        w.table.bright_cyan(),
        "fk:".bright_white(),
        format!("{fk_label} {from}").bright_cyan().bold(),
        "->".bright_white(),
        to.bright_cyan()
    );

    if let Some(delta) = &w.on_delete_change {
        let before = render_reference_action(delta.before.as_ref());
        let after = render_reference_action(delta.after.as_ref());
        let _ = write!(
            out,
            "\n  {} {} {} {}",
            "ON DELETE:".bright_white(),
            before.bright_red(),
            "->".bright_white(),
            after.bright_yellow().bold()
        );
    }
    if let Some(delta) = &w.on_update_change {
        let before = render_reference_action(delta.before.as_ref());
        let after = render_reference_action(delta.after.as_ref());
        let _ = write!(
            out,
            "\n  {} {} {} {}",
            "ON UPDATE:".bright_white(),
            before.bright_red(),
            "->".bright_white(),
            after.bright_yellow().bold()
        );
    }

    let _ = write!(
        out,
        "\n  {} application code that assumed the previous policy will behave differently",
        "why:".bright_white()
    );
    let _ = write!(
        out,
        "\n  {} review backend code BEFORE applying this migration",
        "fix:".bright_green()
    );
    out
}

/// Surface `RemoveConstraint` actions that drop integrity-preserving
/// constraints (PK / UQ / FK / CHECK) with no explicit replacement.
///
/// This is fault **F50** in the data-dependent migration fault taxonomy:
/// the migration succeeds, but every subsequent write that the dropped
/// constraint would have rejected is now silently accepted.
fn emit_constraint_drop_warnings(plan: &MigrationPlan) {
    let warnings = find_constraint_drops_without_replacement(plan);
    if warnings.is_empty() {
        return;
    }

    println!();
    println!(
        "{} {}",
        "⚠".bright_yellow().bold(),
        format!(
            "{} constraint drop(s) without explicit replacement \
             (silent integrity loss risk):",
            warnings.len()
        )
        .bright_yellow()
    );
    for w in &warnings {
        println!();
        for line in format_constraint_drop_warning(w).lines() {
            println!("{line}");
        }
    }
}

/// Format a single `ConstraintDropWarning` as a multi-line indented block.
/// Extracted so its output can be unit-tested without going through stdout.
fn format_constraint_drop_warning(w: &ConstraintDropWarning) -> String {
    let kind_label = match w.kind {
        vespertide_core::ConstraintKind::PrimaryKey => "PRIMARY KEY",
        vespertide_core::ConstraintKind::Unique => "UNIQUE",
        vespertide_core::ConstraintKind::ForeignKey => "FOREIGN KEY",
        vespertide_core::ConstraintKind::Check => "CHECK",
        // Index is filtered out by the detector; this arm exists only to
        // satisfy the `#[non_exhaustive]` enum.
        _ => "(unknown)",
    };
    format!(
        "  {} {}\n  {} {}\n  {} future writes can silently violate this invariant\n  {} use `ReplaceConstraint(from, to)` to swap atomically, or keep the constraint",
        "on:".bright_white(),
        w.table.bright_cyan(),
        "drop:".bright_white(),
        format!("{} — {}", kind_label, w.label).bright_cyan().bold(),
        "why:".bright_white(),
        "fix:".bright_green()
    )
}

/// Normalise the current model set and surface FK constraints that lack a
/// supporting index on the child table. Each FK is reported individually
/// with a concrete suggested index name.
///
/// This is fault **F51** in the data-dependent migration fault taxonomy:
/// it never produces a SQL error, but degrades cascade/lookup performance
/// silently as the child table grows.
fn emit_fk_supporting_index_warnings(current_models: &[vespertide_core::TableDef]) {
    // Normalise per-table; skip tables that fail to normalise (they will
    // have surfaced as planner errors above).
    let normalized: Vec<vespertide_core::TableDef> = current_models
        .iter()
        .filter_map(|t| t.normalize().ok())
        .collect();
    let missing = find_missing_fk_supporting_indexes(&normalized);
    if missing.is_empty() {
        return;
    }

    println!();
    println!(
        "{} {}",
        "⚠".bright_yellow().bold(),
        format!(
            "{} foreign key(s) lack a supporting index \
             (silent performance regression risk):",
            missing.len()
        )
        .bright_yellow()
    );
    for m in &missing {
        println!();
        for line in format_missing_fk_warning(m).lines() {
            println!("{line}");
        }
    }
}

/// Format a single `MissingFkSupportingIndex` as a multi-line indented block.
/// Extracted from `emit_fk_supporting_index_warnings` so its output can be
/// unit-tested without going through stdout.
fn format_missing_fk_warning(m: &MissingFkSupportingIndex) -> String {
    let fk_label = m.constraint_name.as_deref().unwrap_or("(unnamed)");
    let from = format!("{}({})", m.table, m.columns.join(", "));
    let to = format!("{}({})", m.ref_table, m.ref_columns.join(", "));
    format!(
        "  {} {}\n  {} {} {} {}\n  {} cascade/lookup scans the entire `{}` table\n  {} add index `{}`",
        "fk:".bright_white(),
        fk_label.bright_cyan(),
        "ref:".bright_white(),
        from.bright_cyan().bold(),
        "->".bright_white(),
        to.bright_cyan(),
        "why:".bright_white(),
        m.table,
        "fix:".bright_green(),
        m.suggested_index_name.bright_green().bold()
    )
}

fn format_action(action: &MigrationAction) -> String {
    let table = action.table_name().map(Colorize::bright_cyan);
    match action {
        MigrationAction::CreateTable { .. } => {
            format!(
                "{} {}",
                "Create table:".bright_green(),
                table.expect("CreateTable has a table").bold()
            )
        }
        MigrationAction::DeleteTable { .. } => {
            format!(
                "{} {}",
                "Delete table:".bright_red(),
                table.expect("DeleteTable has a table").bold()
            )
        }
        MigrationAction::AddColumn { column, .. } => {
            format!(
                "{} {}.{}",
                "Add column:".bright_green(),
                table.expect("AddColumn has a table"),
                column.name.bright_cyan().bold()
            )
        }
        MigrationAction::RenameColumn { from, to, .. } => {
            format!(
                "{} {}.{} {} {}",
                "Rename column:".bright_yellow(),
                table.expect("RenameColumn has a table"),
                from.bright_white(),
                "->".bright_white(),
                to.bright_cyan().bold()
            )
        }
        MigrationAction::DeleteColumn { column, .. } => {
            format!(
                "{} {}.{}",
                "Delete column:".bright_red(),
                table.expect("DeleteColumn has a table"),
                column.bright_cyan().bold()
            )
        }
        MigrationAction::ModifyColumnType {
            column, new_type, ..
        } => {
            format!(
                "{} {}.{} {} {}",
                "Modify column type:".bright_yellow(),
                table.expect("ModifyColumnType has a table"),
                column.bright_cyan().bold(),
                "->".bright_white(),
                new_type.to_display_string().bright_cyan().bold()
            )
        }
        MigrationAction::ModifyColumnNullable {
            column, nullable, ..
        } => {
            let nullability = if *nullable { "NULL" } else { "NOT NULL" };
            format!(
                "{} {}.{} {} {}",
                "Modify column nullability:".bright_yellow(),
                table.expect("ModifyColumnNullable has a table"),
                column.bright_cyan().bold(),
                "->".bright_white(),
                nullability.bright_cyan().bold()
            )
        }
        MigrationAction::ModifyColumnDefault {
            column,
            new_default,
            ..
        } => {
            let default_display = new_default.as_deref().unwrap_or("(none)");
            format!(
                "{} {}.{} {} {}",
                "Modify column default:".bright_yellow(),
                table.expect("ModifyColumnDefault has a table"),
                column.bright_cyan().bold(),
                "->".bright_white(),
                default_display.bright_cyan().bold()
            )
        }
        MigrationAction::ModifyColumnComment {
            column,
            new_comment,
            ..
        } => {
            let comment_display = new_comment.as_deref().unwrap_or("(none)");
            // Shared 30-char display budget — single source of truth in
            // `vespertide_core::action::truncate_comment`.
            let truncated = truncate_comment(comment_display);
            format!(
                "{} {}.{} {} '{}'",
                "Modify column comment:".bright_yellow(),
                table.expect("ModifyColumnComment has a table"),
                column.bright_cyan().bold(),
                "->".bright_white(),
                truncated.bright_cyan().bold()
            )
        }
        MigrationAction::RenameTable { from, to } => {
            format!(
                "{} {} {} {}",
                "Rename table:".bright_yellow(),
                from.bright_cyan(),
                "->".bright_white(),
                to.bright_cyan().bold()
            )
        }
        MigrationAction::RawSql { sql } => {
            format!(
                "{} {}",
                "Execute raw SQL:".bright_yellow(),
                sql.bright_cyan()
            )
        }
        MigrationAction::DataMigration { sql, description } => {
            let summary = description
                .clone()
                .unwrap_or_else(|| sql_preview(sql.postgres()));
            format!(
                "{} {}",
                "Data migration:".bright_yellow(),
                summary.bright_cyan()
            )
        }
        MigrationAction::AddConstraint { constraint, .. } => {
            format!(
                "{} {} {} {}",
                "Add constraint:".bright_green(),
                format_constraint_type(constraint).bright_cyan().bold(),
                "on".bright_white(),
                table.expect("AddConstraint has a table")
            )
        }
        MigrationAction::RemoveConstraint { constraint, .. } => {
            format!(
                "{} {} {} {}",
                "Remove constraint:".bright_red(),
                format_constraint_type(constraint).bright_cyan().bold(),
                "from".bright_white(),
                table.expect("RemoveConstraint has a table")
            )
        }
        MigrationAction::ReplaceConstraint { from, to, .. } => {
            format!(
                "{} {} {} {} {} {}",
                "Replace constraint:".bright_yellow(),
                format_constraint_type(from).bright_cyan().bold(),
                "->".bright_white(),
                format_constraint_type(to).bright_cyan().bold(),
                "on".bright_white(),
                table.expect("ReplaceConstraint has a table")
            )
        }
        MigrationAction::RemapEnumValues {
            column, mapping, ..
        } => {
            let summary = mapping
                .iter()
                .map(|(old, new)| format!("{old}->{new}"))
                .collect::<Vec<_>>()
                .join(", ");
            format!(
                "{} {}.{} [{}]",
                "Remap enum values:".bright_yellow(),
                table.expect("RemapEnumValues has a table"),
                column.bright_cyan().bold(),
                summary.bright_white()
            )
        }
        _ => unreachable!("MigrationAction is #[non_exhaustive]; all variants are matched above"),
    }
}

fn format_constraint_type(constraint: &vespertide_core::TableConstraint) -> String {
    match constraint {
        vespertide_core::TableConstraint::PrimaryKey { columns, .. } => {
            format!("PRIMARY KEY ({})", columns.join(", "))
        }
        vespertide_core::TableConstraint::Unique { name, columns, .. } => {
            if let Some(n) = name {
                format!("{} UNIQUE ({})", n, columns.join(", "))
            } else {
                format!("UNIQUE ({})", columns.join(", "))
            }
        }
        vespertide_core::TableConstraint::ForeignKey {
            name,
            columns,
            ref_table,
            ..
        } => {
            if let Some(n) = name {
                format!("{} FK ({}) -> {}", n, columns.join(", "), ref_table)
            } else {
                format!("FK ({}) -> {}", columns.join(", "), ref_table)
            }
        }
        vespertide_core::TableConstraint::Check { name, expr, .. } => {
            format!("{name} CHECK ({expr})")
        }
        vespertide_core::TableConstraint::Index { name, columns } => {
            if let Some(n) = name {
                format!("{} INDEX ({})", n, columns.join(", "))
            } else {
                format!("INDEX ({})", columns.join(", "))
            }
        }
        _ => unreachable!("TableConstraint is #[non_exhaustive]; all variants are matched above"),
    }
}

#[cfg(test)]
mod tests;
