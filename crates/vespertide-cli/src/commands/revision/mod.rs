use anyhow::Result;
use chrono::Utc;
use colored::Colorize;
use vespertide_core::{MigrationAction, MigrationPlan, NarrowingStrategy, TableDef};
use vespertide_planner::{
    CascadeReachWarning, CheckAdditionWarning, CheckStrengtheningWarning, CheckTypeMismatchWarning,
    DanglingFkDrop, DefaultChangeWarning, DropChoice, DropResolution, FkOrphanAdditionWarning,
    FkPolicyChangeWarning, MultipleErrors, PlannerError, PrimaryKeyAdditionWarning,
    SequenceExhaustionWarning, TimezoneConversionWarning, TypeNarrowingWarning,
    UniqueAdditionWarning, apply_drop_resolution, find_addcolumn_fk_nullable_violations,
    find_cascade_reach_violations, find_check_additions, find_check_strengthenings,
    find_check_type_mismatches, find_constraint_type_changes, find_dangling_fk_drops,
    find_default_changes, find_drop_resolutions, find_fk_orphan_additions, find_fk_policy_changes,
    find_missing_fill_with, find_primary_key_additions, find_primary_key_removals,
    find_sequence_exhaustion_risks, find_timezone_conversions, find_type_narrowings,
    find_unique_additions, plan_next_migration, schema_from_plans,
};

use prompts::{
    CascadeReachChoice, CheckStrengtheningChoice, CheckTypeMismatchChoice, CheckViolationChoice,
    DefaultChoice, FkOrphanChoice, PrimaryKeyAdditionChoice, SequenceExhaustionChoice,
    UniqueAdditionChoice,
};

use crate::utils::{load_config, load_migrations, load_models};

mod emit;
mod parse;
mod prompts;
mod timezones;
mod write;

#[cfg(test)]
mod tests;

#[cfg(test)]
use emit::*;
#[cfg(test)]
use parse::*;
#[cfg(test)]
use prompts::*;

use emit::RecreateTableRequired;

/// Convert a list of dangling FK drops to a single [`PlannerError`] suitable
/// for surfacing to the user. Follows the same 0/1/N+ contract as
/// [`vespertide_planner::validate_schema`]:
/// - 0 drops → `None` (no error)
/// - 1 drop  → bare [`PlannerError::DanglingForeignKeyAfterDrop`]
/// - 2+      → [`PlannerError::Multiple`] so the user sees every dangling
///   reference in one shot.
fn dangling_drops_to_planner_error(drops: Vec<DanglingFkDrop>) -> Option<PlannerError> {
    if drops.is_empty() {
        return None;
    }
    let mut errors: Vec<PlannerError> = drops
        .into_iter()
        .map(|d| PlannerError::DanglingForeignKeyAfterDrop {
            dropped_table: d.dropped_table,
            dropped_column: d.dropped_column,
            referencing_table: d.referencing_table,
            referencing_constraint: d.referencing_constraint,
        })
        .collect();
    Some(match errors.len() {
        1 => errors.remove(0),
        _ => PlannerError::Multiple(Box::new(MultipleErrors(errors))),
    })
}

fn single_or_multiple_error(mut errors: Vec<PlannerError>) -> PlannerError {
    if errors.len() == 1 {
        errors.remove(0)
    } else {
        PlannerError::Multiple(Box::new(MultipleErrors(errors)))
    }
}

fn ensure_no_dangling_fk_drops(plan: &MigrationPlan, baseline_schema: &[TableDef]) -> Result<()> {
    if let Some(err) =
        dangling_drops_to_planner_error(find_dangling_fk_drops(plan, baseline_schema))
    {
        anyhow::bail!("{err}");
    }
    Ok(())
}

fn ensure_no_f12_errors(plan: &MigrationPlan, baseline_schema: &[TableDef]) -> Result<()> {
    let mut f12_errors: Vec<PlannerError> = Vec::new();
    f12_errors.extend(find_constraint_type_changes(plan, baseline_schema));
    f12_errors.extend(find_primary_key_removals(plan, baseline_schema));
    if !f12_errors.is_empty() {
        let err = single_or_multiple_error(f12_errors);
        anyhow::bail!("{err}");
    }
    Ok(())
}

pub async fn cmd_revision(
    message: String,
    fill_with_args: Vec<String>,
    delete_null_rows_args: Vec<String>,
) -> Result<()> {
    cmd_revision_core(
        message,
        fill_with_args,
        delete_null_rows_args,
        RevisionPromptFns {
            recreate: prompts::prompt_recreate_tables,
            delete_null_rows: prompts::prompt_delete_null_rows,
            fill_with: prompts::prompt_fill_with_value,
            enum_quoted: prompts::prompt_enum_value,
            enum_bare: prompts::prompt_enum_value_bare,
            fk_policy_change: prompts::prompt_fk_policy_changes,
            type_narrowing: prompts::prompt_type_narrowings,
            timezone_conversion: prompts::prompt_timezone_conversions,
            remap_enum_values: prompts::prompt_remap_enum_values,
            drop_resolution: prompts::prompt_drop_resolution,
            default_change: prompts::prompt_default_change_resolution,
            unique_addition: prompts::prompt_unique_additions,
            fk_orphan_addition: prompts::prompt_fk_orphan_additions,
            check_addition: prompts::prompt_check_additions,
            pk_addition: prompts::prompt_pk_additions,
            cascade_reach: prompts::prompt_cascade_reach,
            sequence_exhaustion: prompts::prompt_sequence_exhaustion,
            check_strengthening: prompts::prompt_check_strengthening,
            check_type_mismatch: prompts::prompt_check_type_mismatch,
        },
    )
    .await
}

struct RevisionPromptFns<R, D, F, E, EB, P, N, TZ, RM, DR, DC, UN, FO, CK, PK, CR, SE, CS, CTM> {
    recreate: R,
    delete_null_rows: D,
    fill_with: F,
    enum_quoted: E,
    enum_bare: EB,
    fk_policy_change: P,
    type_narrowing: N,
    timezone_conversion: TZ,
    remap_enum_values: RM,
    drop_resolution: DR,
    default_change: DC,
    unique_addition: UN,
    fk_orphan_addition: FO,
    check_addition: CK,
    pk_addition: PK,
    cascade_reach: CR,
    sequence_exhaustion: SE,
    check_strengthening: CS,
    check_type_mismatch: CTM,
}

#[expect(
    clippy::too_many_lines,
    reason = "linear revision flow: load → plan → recreate → drop resolution → dangling FK → F12 → F3 Edge#1 → unique → fk_orphan → fill_with → enum fill_with → fk policy → narrowing → timezone → remap → write. Extracting helpers scatters the ordering"
)]
#[expect(
    clippy::type_complexity,
    reason = "RevisionPromptFns gathers 19 closure types parameterised by the warning struct each prompt receives; extracting them to type aliases would scatter the signature across the file without aiding readability"
)]
async fn cmd_revision_core<R, D, F, E, EB, P, N, TZ, RM, DR, DC, UN, FO, CK, PK, CR, SE, CS, CTM>(
    message: String,
    fill_with_args: Vec<String>,
    delete_null_rows_args: Vec<String>,
    prompt_fns: RevisionPromptFns<
        R,
        D,
        F,
        E,
        EB,
        P,
        N,
        TZ,
        RM,
        DR,
        DC,
        UN,
        FO,
        CK,
        PK,
        CR,
        SE,
        CS,
        CTM,
    >,
) -> Result<()>
where
    R: Fn(&[RecreateTableRequired]) -> Result<bool>,
    D: Fn(&str, &str) -> Result<bool>,
    F: Fn(&str, &str) -> Result<String>,
    E: Fn(&str, &[String]) -> Result<String>,
    EB: Fn(&str, &[String]) -> Result<String>,
    P: Fn(&[FkPolicyChangeWarning]) -> Result<bool>,
    N: Fn(&[TypeNarrowingWarning]) -> Result<Option<Vec<NarrowingStrategy>>>,
    TZ: Fn(&[TimezoneConversionWarning]) -> Result<Option<Vec<String>>>,
    RM: Fn(&MigrationPlan) -> Result<bool>,
    DR: Fn(&DropResolution) -> Result<Option<DropChoice>>,
    DC: Fn(&DefaultChangeWarning) -> Result<Option<DefaultChoice>>,
    UN: Fn(&UniqueAdditionWarning) -> Result<Option<UniqueAdditionChoice>>,
    FO: Fn(&FkOrphanAdditionWarning) -> Result<Option<FkOrphanChoice>>,
    CK: Fn(&CheckAdditionWarning) -> Result<Option<CheckViolationChoice>>,
    PK: Fn(&PrimaryKeyAdditionWarning) -> Result<Option<PrimaryKeyAdditionChoice>>,
    CR: Fn(&CascadeReachWarning) -> Result<Option<CascadeReachChoice>>,
    SE: Fn(&SequenceExhaustionWarning) -> Result<Option<SequenceExhaustionChoice>>,
    CS: Fn(&CheckStrengtheningWarning) -> Result<Option<CheckStrengtheningChoice>>,
    CTM: Fn(&CheckTypeMismatchWarning) -> Result<Option<CheckTypeMismatchChoice>>,
{
    let RevisionPromptFns {
        recreate: recreate_prompt_fn,
        delete_null_rows: delete_null_rows_prompt_fn,
        fill_with: fill_with_prompt_fn,
        enum_quoted: enum_prompt_fn,
        enum_bare: enum_bare_prompt_fn,
        fk_policy_change: fk_policy_change_prompt_fn,
        type_narrowing: type_narrowing_prompt_fn,
        timezone_conversion: timezone_conversion_prompt_fn,
        remap_enum_values: remap_enum_values_prompt_fn,
        drop_resolution: drop_resolution_prompt_fn,
        default_change: default_change_prompt_fn,
        unique_addition: unique_addition_prompt_fn,
        fk_orphan_addition: fk_orphan_addition_prompt_fn,
        check_addition: check_addition_prompt_fn,
        pk_addition: pk_addition_prompt_fn,
        cascade_reach: cascade_reach_prompt_fn,
        sequence_exhaustion: sequence_exhaustion_prompt_fn,
        check_strengthening: check_strengthening_prompt_fn,
        check_type_mismatch: check_type_mismatch_prompt_fn,
    } = prompt_fns;

    let config = load_config()?;
    let current_models = load_models(&config)?;
    let applied_plans = load_migrations(&config)?;

    let mut plan = plan_next_migration(&current_models, &applied_plans)
        .map_err(|e| anyhow::anyhow!("planning error: {e}"))?;

    // Check for non-nullable FK changes that require table recreation.
    emit::handle_recreate_requirements(&mut plan, &current_models, recreate_prompt_fn)?;

    if plan.actions.is_empty() {
        println!(
            "{} {}",
            "No changes detected.".bright_yellow(),
            "Nothing to migrate.".bright_white()
        );
        return Ok(());
    }

    // Reconstruct baseline schema for column type lookups
    let baseline_schema = schema_from_plans(&applied_plans)
        .map_err(|e| anyhow::anyhow!("schema reconstruction error: {e}"))?;

    // F10 + F8 + F22 — Interactive drop resolution. Each DeleteColumn /
    // DeleteTable is presented to the user with the same-plan rename
    // candidates (option B). The user picks Drop / RenameTo / Cancel; on
    // RenameTo the plan is rewritten in place so a single migration
    // captures the full intent. Run BEFORE the dangling-FK check so a
    // rename choice removes the underlying DeleteX action and the F9
    // check sees the corrected plan.
    let resolutions = find_drop_resolutions(&plan, &baseline_schema);
    if !resolutions.is_empty() {
        let mut chosen: Vec<(DropResolution, DropChoice)> = Vec::new();
        for r in resolutions {
            if let Some(choice) = drop_resolution_prompt_fn(&r)? {
                chosen.push((r, choice));
            } else {
                println!(
                    "{} {}",
                    "Cancelled.".bright_yellow().bold(),
                    "Drop resolution declined; no migration written.".bright_white()
                );
                return Ok(());
            }
        }
        // Apply in descending action-index order so earlier indices stay
        // valid as the plan shrinks under each rewrite.
        chosen.sort_by_key(|(r, _)| std::cmp::Reverse(r.action_index));
        for (r, choice) in &chosen {
            apply_drop_resolution(&mut plan, &baseline_schema, r, choice)
                .map_err(|e| anyhow::anyhow!("apply drop resolution: {e}"))?;
        }
    }

    // F9 — Dangling foreign key after a column or table drop. Hard error
    // (no prompt): dropping a column or table while another table's FK
    // still references it would silently leave a stale FK pointing at
    // nothing. The plan must clean up the offending FK (or its owning
    // table/column) in the same revision. Surfaced *before* any other
    // interactive prompt so the user is not asked for fill_with values on
    // a plan that is going to be rejected anyway.
    ensure_no_dangling_fk_drops(&plan, &baseline_schema)?;

    // F12 — PK ↔ UQ constraint swaps and PRIMARY KEY removal without a
    // replacement. Both are hard errors (per user policy: every F12
    // scenario blocks at revision time). Combine the two detector outputs
    // into the standard 0/1/N+ contract so multi-table violations are
    // reported in one shot.
    ensure_no_f12_errors(&plan, &baseline_schema)?;

    // F2 — Adding UNIQUE on an existing column risks DB rejection if
    // production data has duplicates. Prompt for a deduplication strategy
    // one warning at a time; the choice is stamped back onto the matching
    // `TableConstraint::Unique.strategy` so re-running the revision
    // produces the same SQL.
    let unique_additions = find_unique_additions(&plan, &baseline_schema);
    for warning in &unique_additions {
        let Some(choice) = unique_addition_prompt_fn(warning)? else {
            println!(
                "{} {}",
                "Cancelled.".bright_yellow().bold(),
                "Unique addition declined; no migration written.".bright_white()
            );
            return Ok(());
        };
        prompts::apply_unique_addition_choice(&mut plan, warning, choice);
    }

    // F3 Edge #1 — `AddColumn` participating in a FK with `nullable: false`
    // plus `fill_with`/`default` is rejected up-front as a hard error.
    // The F3 emit pipeline (fill → nullify orphans → add FK) requires the
    // column to be nullable; vespertide never lifts `nullable` silently.
    let edge1_errors = find_addcolumn_fk_nullable_violations(&plan);
    if let Some(err) = match edge1_errors.len() {
        0 => None,
        1 => Some(edge1_errors.into_iter().next().expect("len == 1")),
        _ => Some(PlannerError::Multiple(Box::new(MultipleErrors(
            edge1_errors,
        )))),
    } {
        return Err(anyhow::anyhow!("{err}"));
    }

    // F3 — Adding FOREIGN KEY on an existing column may reference parent
    // rows that no longer exist. Prompt for a per-warning orphan strategy
    // (Nullify / Delete / Cancel); the choice is stamped back onto the
    // matching `TableConstraint::ForeignKey.orphan_strategy` so the SQL
    // generator emits the correct pre-cleanup statement ahead of the
    // ADD CONSTRAINT.
    let fk_orphan_additions = find_fk_orphan_additions(&plan, &baseline_schema);
    for warning in &fk_orphan_additions {
        let Some(choice) = fk_orphan_addition_prompt_fn(warning)? else {
            println!(
                "{} {}",
                "Cancelled.".bright_yellow().bold(),
                "FK orphan resolution declined; no migration written.".bright_white()
            );
            return Ok(());
        };
        prompts::apply_fk_orphan_addition_choice(&mut plan, warning, choice);
    }

    // F4 — Adding CHECK on a baseline-existing table whose narrow-shape
    // expression flags violating rows. Prompt for a per-warning
    // violation strategy (Nullify / Delete / Cancel); the choice is
    // stamped back onto the matching `TableConstraint::Check.strategy`
    // so the SQL generator emits the correct pre-cleanup statement
    // ahead of the ADD CONSTRAINT.
    let check_additions = find_check_additions(&plan, &baseline_schema);
    for warning in &check_additions {
        let Some(choice) = check_addition_prompt_fn(warning)? else {
            println!(
                "{} {}",
                "Cancelled.".bright_yellow().bold(),
                "CHECK violation resolution declined; no migration written.".bright_white()
            );
            return Ok(());
        };
        prompts::apply_check_addition_choice(&mut plan, warning, choice);
    }

    // F5 — Adding PRIMARY KEY on a baseline-existing table with
    // potential duplicate / NULL violations. Prompt for a per-warning
    // duplicate strategy (DeleteDuplicates / ContinueWithoutCleanup /
    // Cancel); NULL handling is delegated to the F1 fill_with prompt
    // that fires later in the flow. The choice is stamped back onto
    // `TableConstraint::PrimaryKey.strategy`.
    let pk_additions = find_primary_key_additions(&plan, &baseline_schema);
    for warning in &pk_additions {
        let Some(choice) = pk_addition_prompt_fn(warning)? else {
            println!(
                "{} {}",
                "Cancelled.".bright_yellow().bold(),
                "PRIMARY KEY resolution declined; no migration written.".bright_white()
            );
            return Ok(());
        };
        prompts::apply_pk_addition_choice(&mut plan, warning, choice);
    }

    // F96 — Adding ON DELETE CASCADE foreign keys that extend a deep
    // or high-fanout cascade chain. Pure static analysis; no plan
    // mutation. The user either acknowledges (`Proceed`) or cancels
    // to re-examine the model — vespertide cannot auto-shrink a
    // user-declared cascade chain.
    let cascade_warnings = find_cascade_reach_violations(&plan, &baseline_schema);
    for warning in &cascade_warnings {
        if cascade_reach_prompt_fn(warning)?.is_none() {
            println!(
                "{} {}",
                "Cancelled.".bright_yellow().bold(),
                "Cascade chain declined; no migration written.".bright_white()
            );
            return Ok(());
        }
    }

    // F76 — Sequence / identity overflow risk on PK columns and FK
    // mismatches against safe parent types. For mutable cases
    // (`CreateTable`, `ModifyColumnType`) the prompt offers a
    // single-click "rewrite to big_int" that stamps a wider type
    // back onto the matching plan action; `AddConstraint(...)` cases
    // surface as warnings only. Run AFTER F5 (which may have already
    // rewritten the PK shape) so the analysis sees the final plan.
    let sequence_warnings = find_sequence_exhaustion_risks(&plan, &baseline_schema);
    for warning in &sequence_warnings {
        let Some(choice) = sequence_exhaustion_prompt_fn(warning)? else {
            println!(
                "{} {}",
                "Cancelled.".bright_yellow().bold(),
                "INT overflow resolution declined; no migration written.".bright_white()
            );
            return Ok(());
        };
        prompts::apply_sequence_exhaustion_choice(&mut plan, warning, choice);
    }

    // F29 — CHECK expression strengthening. A migration replaces a
    // CHECK predicate with a *demonstrably* stricter one (literal
    // tightened, operator boundary tightened, IN list shrunk,
    // BETWEEN narrowed, AND conjunct added, or OR disjunct removed).
    // Any existing row that satisfied the old predicate but fails
    // the new one will fail the migration at `VALIDATE CONSTRAINT`
    // / `ADD CONSTRAINT` time. No mutation: vespertide cannot widen
    // a user-authored predicate — the user pre-cleans violating
    // rows in a separate migration or acknowledges the risk and
    // proceeds. Run AFTER F76 so the analysis sees the final plan.
    let check_strengthenings = find_check_strengthenings(&plan, &baseline_schema);
    for warning in &check_strengthenings {
        let Some(_choice) = check_strengthening_prompt_fn(warning)? else {
            println!(
                "{} {}",
                "Cancelled.".bright_yellow().bold(),
                "CHECK strengthening declined; no migration written.".bright_white()
            );
            return Ok(());
        };
    }

    // F-novel-4 — CHECK literal type-mismatch. A CHECK constraint
    // compares a column to a literal of a demonstrably incompatible
    // type (e.g. `int_col = 'abc'`, `bool_col = 'x'`, `uuid_col > 0`).
    // PostgreSQL rejects these at `ADD CONSTRAINT` time; MySQL and
    // SQLite may coerce silently. vespertide cannot auto-correct the
    // user-authored literal — the user fixes the model or acknowledges
    // and proceeds. Run AFTER F29 so the analysis sees the final plan.
    let check_type_mismatches = find_check_type_mismatches(&plan, &baseline_schema);
    for warning in &check_type_mismatches {
        let Some(_choice) = check_type_mismatch_prompt_fn(warning)? else {
            println!(
                "{} {}",
                "Cancelled.".bright_yellow().bold(),
                "CHECK type mismatch declined; no migration written.".bright_white()
            );
            return Ok(());
        };
    }

    // Parse CLI fill_with arguments
    let mut fill_values = parse::parse_fill_with_args(&fill_with_args);
    let delete_set = parse::parse_delete_null_rows_args(&delete_null_rows_args);

    // Apply any CLI-provided fill_with values first
    emit::apply_fill_with_to_plan(&mut plan, &fill_values);
    emit::apply_delete_null_rows_to_plan(&mut plan, &delete_set);

    // Find all missing fill_with values
    let mut missing = find_missing_fill_with(&plan, &baseline_schema);

    // Handle FK columns with delete_null_rows option first
    if !missing.is_empty() {
        prompts::handle_delete_null_rows(
            &mut plan,
            &mut missing,
            &delete_set,
            delete_null_rows_prompt_fn,
        )?;
    }

    // Handle remaining missing fill_with values interactively
    if !missing.is_empty() {
        prompts::collect_fill_with_values(
            &missing,
            &mut fill_values,
            fill_with_prompt_fn,
            enum_prompt_fn,
        )?;
        emit::apply_fill_with_to_plan(&mut plan, &fill_values);
    }

    // Handle any missing enum fill_with values (for removed enum values) interactively
    prompts::handle_missing_enum_fill_with(&mut plan, &baseline_schema, enum_bare_prompt_fn)?;

    // F30 — FK referential-action policy changes silently alter application
    // behavior. Surface them and require explicit double-confirmation before
    // the migration file is written.
    let fk_policy_warnings = find_fk_policy_changes(&plan);
    if !fk_policy_warnings.is_empty() && !fk_policy_change_prompt_fn(&fk_policy_warnings)? {
        println!(
            "{} {}",
            "Cancelled.".bright_yellow().bold(),
            "Review backend code before retrying revision.".bright_white()
        );
        return Ok(());
    }

    // F7-(b) — integer enum value remap. The planner already inserted
    // RemapEnumValues actions; surface them so the user explicitly
    // acknowledges the automatic data rewrite before the migration ships.
    if !remap_enum_values_prompt_fn(&plan)? {
        println!(
            "{} {}",
            "Cancelled.".bright_yellow().bold(),
            "Coordinate with ORM consumers before retrying revision.".bright_white()
        );
        return Ok(());
    }

    // F6/F19/F33/F87 — type narrowings can truncate, reject, or silently
    // corrupt existing rows depending on backend. Surface every narrowing
    // via per-narrowing Select UI; the chosen strategy is stamped onto the
    // plan so the SQL generator can emit safe pre-processing alongside
    // the ALTER. Returns None when the user declines or when the kind has
    // no automatic strategy.
    let narrowing_warnings = find_type_narrowings(&plan, &baseline_schema);
    if !narrowing_warnings.is_empty() {
        let Some(strategies) = type_narrowing_prompt_fn(&narrowing_warnings)? else {
            println!(
                "{} {}",
                "Cancelled.".bright_yellow().bold(),
                "Pre-clean the data manually before retrying revision.".bright_white()
            );
            return Ok(());
        };
        prompts::apply_narrowing_strategies_to_plan(&mut plan, &narrowing_warnings, &strategies);
    }

    // F20 — timestamp ↔ timestamptz conversions need an explicit timezone
    // so the SQL generator can emit `... AT TIME ZONE '<tz>'` on PG. Mute
    // for already-resolved conversions: only ask when timezone is missing.
    let timezone_warnings: Vec<TimezoneConversionWarning> =
        find_timezone_conversions(&plan, &baseline_schema)
            .into_iter()
            .filter(|w| w.current_timezone.is_none())
            .collect();
    if !timezone_warnings.is_empty() {
        let Some(choices) = timezone_conversion_prompt_fn(&timezone_warnings)? else {
            println!("{} {}", "Cancelled.".bright_yellow().bold(), "A timezone is required for safe timestamp \u{2194} timestamptz conversion. Re-run when you've decided which timezone to use.".bright_white());
            return Ok(());
        };
        prompts::apply_timezone_choices_to_plan(&mut plan, &timezone_warnings, &choices);
    }

    // F15 — DEFAULT value changes only affect new rows; existing rows keep
    // their stored values by default. Surface every change with a risk
    // classification and let the user pick Backfill (UPDATE all rows) /
    // Skip (schema-only) / Cancel. Run last among the elective prompts so
    // the user sees default changes in the context of any type / timezone
    // narrowings that may have just been resolved on the same column.
    let default_changes = find_default_changes(&plan, &baseline_schema);
    for warning in &default_changes {
        let Some(choice) = default_change_prompt_fn(warning)? else {
            println!(
                "{} {}",
                "Cancelled.".bright_yellow().bold(),
                "Default-change resolution declined; no migration written.".bright_white()
            );
            return Ok(());
        };
        if choice == DefaultChoice::Backfill
            && let Some(MigrationAction::ModifyColumnDefault {
                backfill,
                new_default,
                ..
            }) = plan.actions.get_mut(warning.action_index)
        {
            backfill.clone_from(new_default);
        }
    }

    plan.id = uuid::Uuid::new_v4().to_string();
    plan.comment = Some(message);
    if plan.created_at.is_none() {
        // Record creation time in RFC3339 (UTC).
        plan.created_at = Some(Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true));
    }

    let path = write::write_migration_file(&config, &plan).await?;

    println!(
        "{} {}",
        "Created migration:".bright_green().bold(),
        format!("{}", path.display()).bright_white()
    );
    println!(
        "  {} {}",
        "Version:".bright_cyan(),
        plan.version.to_string().bright_magenta().bold()
    );
    println!(
        "  {} {}",
        "Actions:".bright_cyan(),
        plan.actions.len().to_string().bright_yellow()
    );
    if let Some(comment) = &plan.comment {
        println!("  {} {}", "Comment:".bright_cyan(), comment.bright_white());
    }

    Ok(())
}
