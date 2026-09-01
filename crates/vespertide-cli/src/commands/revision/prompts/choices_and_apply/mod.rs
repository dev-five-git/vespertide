use anyhow::{Context, Result};
use colored::Colorize;
use dialoguer::Select;
use vespertide_core::{
    CheckViolationStrategy, ForeignKeyOrphanStrategy, KeepPolicy, PrimaryKeyAdditionStrategy,
    UniqueConstraintStrategy,
};
use vespertide_planner::{
    CascadeReachWarning, CascadeRiskLevel, CheckAdditionWarning, CheckStrengtheningKind,
    CheckStrengtheningWarning, CheckTypeMismatchWarning, DefaultChangeWarning,
    FkOrphanAdditionWarning, PkKind, PrimaryKeyAdditionWarning, RiskLevel, SequenceExhaustionKind,
    SequenceExhaustionWarning, SequenceRiskLevel, UniqueAdditionWarning,
};

/// User's choice for a single F15 [`DefaultChangeWarning`].
///
/// `Cancel` is handled at the CLI layer (it aborts the whole `revision`
/// command), so this enum only carries the two outcomes that translate into
/// plan changes:
/// - [`DefaultChoice::Backfill`] → set the action's `backfill` field so the
///   SQL generator emits an `UPDATE` rewriting every existing row.
/// - [`DefaultChoice::Skip`] → keep the action unchanged; existing rows
///   stay as they are.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::commands::revision) enum DefaultChoice {
    /// UPDATE all existing rows to match the new default.
    Backfill,
    /// Schema-only change: existing rows keep their current values.
    Skip,
}

/// Interactive resolution for a single `DefaultChangeWarning`.
///
/// Renders a header (with classified risk level) plus a `Select` menu:
/// Backfill / Skip / Cancel. Returns `None` for Cancel, `Some(choice)`
/// otherwise. When the action *removes* a default (`new_default = None`),
/// the Backfill option is hidden because there is no value to write —
/// only Skip / Cancel remain.
#[cfg(not(tarpaulin_include))]
pub(in crate::commands::revision) fn prompt_default_change_resolution(
    warning: &DefaultChangeWarning,
) -> Result<Option<DefaultChoice>> {
    println!();
    super::print_section_rule();
    println!("{}", format_default_change_header(warning));
    super::print_section_rule();

    let backfill_available = warning.new_default.is_some();

    let mut labels: Vec<String> = Vec::new();
    let mut outcomes: Vec<Option<DefaultChoice>> = Vec::new();

    if backfill_available {
        let new_default = warning.new_default.as_deref().unwrap_or_default();
        labels.push(format!(
            "Backfill: UPDATE all rows to {}",
            new_default.bright_green()
        ));
        outcomes.push(Some(DefaultChoice::Backfill));
    }

    labels.push("Skip: existing rows keep current values".to_string());
    outcomes.push(Some(DefaultChoice::Skip));

    labels.push("Cancel migration".to_string());
    outcomes.push(None);

    let selection = Select::new()
        .with_prompt("  What should happen to existing rows?")
        .items(&labels)
        .default(0)
        .interact()
        .context("failed to read default-change choice")?;

    Ok(outcomes[selection])
}

fn format_default_change_header(warning: &DefaultChangeWarning) -> String {
    let risk = warning.kind.risk_level();
    let risk_label = match risk {
        RiskLevel::High => "HIGH RISK".bright_red().bold().to_string(),
        RiskLevel::Medium => "MEDIUM RISK".bright_yellow().bold().to_string(),
        RiskLevel::Low => "LOW RISK".bright_cyan().to_string(),
    };
    let kind_label = warning.kind.label();
    let old = warning.old_default.as_deref().unwrap_or("(none)");
    let new = warning.new_default.as_deref().unwrap_or("(none)");
    format!(
        "  {} Column DEFAULT change ({kind_label} \u{2014} {risk_label})\n\n  \
         {}.{}:  {}  \u{2192}  {}\n\n  \
         Existing rows are NOT automatically updated.",
        "\u{26a0}".bright_yellow(),
        warning.table.bright_white().bold(),
        warning.column.bright_green(),
        old.bright_white(),
        new.bright_white(),
    )
}

/// User's choice for a single F2 [`UniqueAdditionWarning`].
///
/// The CLI maps these into `TableConstraint::Unique.strategy`:
/// - `DeleteDuplicates(KeepPolicy)` → strategy set, SQL generator emits
///   the `DELETE ... NOT IN (SELECT MIN/MAX(pk) ...)` ahead of ADD.
/// - `ContinueWithoutCleanup` → strategy left at default; the SQL
///   generator falls back to bare ADD CONSTRAINT (no DELETE) for tables
///   where PK shape can't drive auto-cleanup. The user accepts that
///   apply may fail if duplicates exist.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::commands::revision) enum UniqueAdditionChoice {
    DeleteDuplicates(KeepPolicy),
    ContinueWithoutCleanup,
}

/// Interactive resolution for a single F2 unique-addition.
///
/// Returns `Ok(None)` to cancel the whole revision. The set of offered
/// options is tailored to `warning.pk_kind`:
///
/// - `SingleAutoCleanupCapable` → `KeepFirst` (default) / `KeepLast` /
///   Continue / Cancel
/// - any other kind (composite PK, PK inside unique set, no PK) →
///   `ContinueWithoutCleanup` / Cancel (auto-cleanup is unavailable)
#[cfg(not(tarpaulin_include))]
pub(in crate::commands::revision) fn prompt_unique_additions(
    warning: &UniqueAdditionWarning,
) -> Result<Option<UniqueAdditionChoice>> {
    println!();
    super::print_section_rule();
    println!("{}", format_unique_addition_header(warning));
    super::print_section_rule();

    let auto_cleanup_available = matches!(warning.pk_kind, PkKind::SingleAutoCleanupCapable { .. });

    let mut labels: Vec<String> = Vec::new();
    let mut outcomes: Vec<Option<UniqueAdditionChoice>> = Vec::new();
    if auto_cleanup_available {
        labels.push("Delete duplicates, keep FIRST (smallest PK, recommended default)".to_string());
        outcomes.push(Some(UniqueAdditionChoice::DeleteDuplicates(
            KeepPolicy::First,
        )));
        labels.push("Delete duplicates, keep LAST (largest PK)".to_string());
        outcomes.push(Some(UniqueAdditionChoice::DeleteDuplicates(
            KeepPolicy::Last,
        )));
    }
    labels.push(
        "Continue without auto-cleanup (DB will reject the migration if duplicates exist)"
            .to_string(),
    );
    outcomes.push(Some(UniqueAdditionChoice::ContinueWithoutCleanup));
    labels.push("Cancel migration".to_string());
    outcomes.push(None);

    let selection = Select::new()
        .with_prompt("  How should pre-existing duplicates be handled?")
        .items(&labels)
        .default(0)
        .interact()
        .context("failed to read unique-addition choice")?;
    Ok(outcomes[selection])
}

fn format_unique_addition_header(warning: &UniqueAdditionWarning) -> String {
    let target = format!(
        "{}.({})",
        warning.table.bright_white().bold(),
        warning.columns.join(", ").bright_green()
    );
    let pk_hint = match &warning.pk_kind {
        PkKind::SingleAutoCleanupCapable { column } => format!(
            "Single-column PK: {} — auto-cleanup available.",
            column.bright_cyan()
        ),
        PkKind::SingleInsideUniqueSet { column } => format!(
            "PK column '{}' is INSIDE the unique set — auto-cleanup unavailable (tautology).",
            column.bright_yellow()
        ),
        PkKind::Composite { columns } => format!(
            "Composite PK ({}) — auto-cleanup unavailable in v0.2. Pre-clean manually.",
            columns.join(", ").bright_yellow()
        ),
        PkKind::None => "No PRIMARY KEY on table — auto-cleanup unavailable.".to_string(),
    };
    let fk_hint = if warning.fk_references.is_empty() {
        String::new()
    } else {
        let names: Vec<String> = warning
            .fk_references
            .iter()
            .map(|r| {
                let label = r
                    .constraint_name
                    .clone()
                    .unwrap_or_else(|| format!("({})", r.child_columns.join(", ")));
                format!("{}.{}", r.child_table, label)
            })
            .collect();
        format!(
            "\n  Foreign keys reference this column set: {}.",
            names.join(", ")
        )
    };
    format!(
        "  {} Adding UNIQUE on {target} (existing column)\n  {pk_hint}{fk_hint}",
        "\u{26a0}".bright_yellow()
    )
}

/// Apply a user's resolution to the plan. Mutates the matching
/// `AddConstraint(Unique)` action's `strategy` field.
pub(in crate::commands::revision) fn apply_unique_addition_choice(
    plan: &mut vespertide_core::MigrationPlan,
    warning: &UniqueAdditionWarning,
    choice: UniqueAdditionChoice,
) {
    let Some(action) = plan.actions.get_mut(warning.action_index) else {
        return;
    };
    let vespertide_core::MigrationAction::AddConstraint {
        constraint: vespertide_core::TableConstraint::Unique { strategy, .. },
        ..
    } = action
    else {
        return;
    };
    match choice {
        UniqueAdditionChoice::DeleteDuplicates(keep) => {
            *strategy = UniqueConstraintStrategy::DeleteDuplicates { keep };
        }
        UniqueAdditionChoice::ContinueWithoutCleanup => {
            // Strategy field stays at its default `DeleteDuplicates { First }`;
            // the SQL generator's PK-shape fallback emits no DELETE for
            // tables without a usable PK, so this records intent without
            // changing SQL output.
        }
    }
}

/// F3 (FK with orphan rows) - user resolution for an
/// `AddConstraint(ForeignKey)` on a baseline-existing column.
///
/// `Nullify` and `Delete` map 1-to-1 to
/// [`ForeignKeyOrphanStrategy`] variants. `Nullify` is only offered
/// when [`FkOrphanAdditionWarning::all_columns_nullable`] is `true` -
/// the SQL `UPDATE child SET col = NULL` would otherwise violate the
/// NOT NULL constraint on the column being NULL-ed out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::commands::revision) enum FkOrphanChoice {
    /// Map to [`ForeignKeyOrphanStrategy::NullifyOrphans`].
    Nullify,
    /// Map to [`ForeignKeyOrphanStrategy::DeleteOrphans`].
    Delete,
}

/// Per-warning interactive prompt for F3. Returns `None` when the user
/// cancels (no migration written).
///
/// The user is **always required to choose explicitly** - there is no
/// silent default-apply path. The recommended option is highlighted
/// (`(recommended)` suffix) but the user must press Enter on it.
#[cfg(not(tarpaulin_include))]
pub(in crate::commands::revision) fn prompt_fk_orphan_additions(
    warning: &FkOrphanAdditionWarning,
) -> Result<Option<FkOrphanChoice>> {
    println!();
    super::print_section_rule();
    println!("{}", format_fk_orphan_addition_header(warning));
    super::print_section_rule();

    let mut labels: Vec<String> = Vec::new();
    let mut outcomes: Vec<Option<FkOrphanChoice>> = Vec::new();

    if warning.all_columns_nullable {
        labels
            .push("Nullify orphan references (less destructive, recommended default)".to_string());
        outcomes.push(Some(FkOrphanChoice::Nullify));
        labels.push("Delete orphan rows".to_string());
        outcomes.push(Some(FkOrphanChoice::Delete));
    } else {
        labels
            .push("Delete orphan rows (Nullify unavailable: FK columns are NOT NULL)".to_string());
        outcomes.push(Some(FkOrphanChoice::Delete));
    }
    labels.push("Cancel migration".to_string());
    outcomes.push(None);

    let selection = Select::new()
        .with_prompt("  How should pre-existing orphan rows be handled?")
        .items(&labels)
        .default(0)
        .interact()
        .context("failed to read fk-orphan-addition choice")?;
    Ok(outcomes[selection])
}

fn format_fk_orphan_addition_header(warning: &FkOrphanAdditionWarning) -> String {
    let child = format!(
        "{}.({})",
        warning.table.bright_white().bold(),
        warning.columns.join(", ").bright_green()
    );
    let parent = format!(
        "{}.({})",
        warning.ref_table.bright_white().bold(),
        warning.ref_columns.join(", ").bright_cyan()
    );
    let nullable_hint = if warning.all_columns_nullable {
        "FK columns are nullable - Nullify is available.".to_string()
    } else {
        "FK columns are NOT NULL - only Delete is available.".to_string()
    };
    let constraint_label = warning
        .constraint_name
        .as_deref()
        .map_or_else(String::new, |n| format!(" (constraint `{n}`)"));
    format!(
        "  {} Adding FOREIGN KEY{constraint_label} on existing column(s)\n  \
         {child} {arrow} {parent}\n  {nullable_hint}",
        "\u{26a0}".bright_yellow(),
        arrow = "\u{2192}".bright_black()
    )
}

/// Stamp the user's choice onto the matching `AddConstraint(ForeignKey)`
/// action's `orphan_strategy` field. Idempotent if the matching action
/// has already been mutated.
pub(in crate::commands::revision) fn apply_fk_orphan_addition_choice(
    plan: &mut vespertide_core::MigrationPlan,
    warning: &FkOrphanAdditionWarning,
    choice: FkOrphanChoice,
) {
    let Some(action) = plan.actions.get_mut(warning.action_index) else {
        return;
    };
    let vespertide_core::MigrationAction::AddConstraint {
        constraint:
            vespertide_core::TableConstraint::ForeignKey {
                orphan_strategy, ..
            },
        ..
    } = action
    else {
        return;
    };
    *orphan_strategy = match choice {
        FkOrphanChoice::Nullify => ForeignKeyOrphanStrategy::NullifyOrphans,
        FkOrphanChoice::Delete => ForeignKeyOrphanStrategy::DeleteOrphans,
    };
}

/// F4 (CHECK with violating rows) - user resolution for an
/// `AddConstraint(Check)` whose expression matches the narrow shape
/// against a baseline-existing table.
///
/// `Nullify` and `Delete` map 1-to-1 to [`CheckViolationStrategy`]
/// variants. `Nullify` is only offered when the target column is
/// nullable in the baseline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::commands::revision) enum CheckViolationChoice {
    /// Map to [`CheckViolationStrategy::NullifyViolatingColumn { column }`].
    Nullify {
        /// Target column carried from the warning so the SQL emitter
        /// knows which column to `SET = NULL`.
        column: String,
    },
    /// Map to [`CheckViolationStrategy::DeleteViolatingRows`].
    Delete,
}

/// Per-warning interactive prompt for F4. Returns `None` when the user
/// cancels (no migration written).
///
/// The user is always required to choose explicitly; the recommended
/// option is highlighted but Enter on the default still counts as an
/// explicit selection.
#[cfg(not(tarpaulin_include))]
pub(in crate::commands::revision) fn prompt_check_additions(
    warning: &CheckAdditionWarning,
) -> Result<Option<CheckViolationChoice>> {
    println!();
    super::print_section_rule();
    println!("{}", format_check_addition_header(warning));
    super::print_section_rule();

    let mut labels: Vec<String> = Vec::new();
    let mut outcomes: Vec<Option<CheckViolationChoice>> = Vec::new();

    if warning.target_column_nullable {
        labels.push(
            "Nullify the violating column (less destructive, recommended default)".to_string(),
        );
        outcomes.push(Some(CheckViolationChoice::Nullify {
            column: warning.target_column.clone(),
        }));
        labels.push("Delete violating rows".to_string());
        outcomes.push(Some(CheckViolationChoice::Delete));
    } else {
        labels.push(
            "Delete violating rows (Nullify unavailable: target column is NOT NULL)".to_string(),
        );
        outcomes.push(Some(CheckViolationChoice::Delete));
    }
    labels.push("Cancel migration".to_string());
    outcomes.push(None);

    let selection = Select::new()
        .with_prompt("  How should pre-existing violating rows be handled?")
        .items(&labels)
        .default(0)
        .interact()
        .context("failed to read check-addition choice")?;
    Ok(outcomes[selection].clone())
}

fn format_check_addition_header(warning: &CheckAdditionWarning) -> String {
    let target = format!(
        "{}.{}",
        warning.table.bright_white().bold(),
        warning.target_column.bright_green()
    );
    let nullable_hint = if warning.target_column_nullable {
        "Column is nullable - Nullify is available.".to_string()
    } else {
        "Column is NOT NULL - only Delete is available.".to_string()
    };
    format!(
        "  {} Adding CHECK `{}` ({}) on existing rows\n  Target: {target}\n  {nullable_hint}",
        "\u{26a0}".bright_yellow(),
        warning.constraint_name.bright_cyan(),
        warning.check_expr.bright_white()
    )
}

/// Stamp the user's choice onto the matching `AddConstraint(Check)`
/// action's `strategy` field.
pub(in crate::commands::revision) fn apply_check_addition_choice(
    plan: &mut vespertide_core::MigrationPlan,
    warning: &CheckAdditionWarning,
    choice: CheckViolationChoice,
) {
    let Some(action) = plan.actions.get_mut(warning.action_index) else {
        return;
    };
    let vespertide_core::MigrationAction::AddConstraint {
        constraint: vespertide_core::TableConstraint::Check { strategy, .. },
        ..
    } = action
    else {
        return;
    };
    *strategy = match choice {
        CheckViolationChoice::Nullify { column } => {
            CheckViolationStrategy::NullifyViolatingColumn {
                column: column.into(),
            }
        }
        CheckViolationChoice::Delete => CheckViolationStrategy::DeleteViolatingRows,
    };
}

/// F5 (PK addition with duplicate / NULL violations) - user resolution
/// for an `AddConstraint(PrimaryKey)` on a baseline-existing table.
///
/// `DeleteDuplicates { keep }` maps to
/// [`PrimaryKeyAdditionStrategy::DeleteDuplicates`]. The duplicate
/// cleanup is offered only when the warning reports
/// `auto_cleanup_capable = true` (single-column PK with a usable
/// baseline single-column PK to drive the `DELETE ... NOT IN
/// (SELECT MIN(pk) ...)` query).
///
/// NULL violations are *not* handled by this enum — they are surfaced
/// via the standard F1 `fill_with` mechanism for each entry in
/// `warning.nullable_columns`, which fires later in the revision flow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::commands::revision) enum PrimaryKeyAdditionChoice {
    /// Map to [`PrimaryKeyAdditionStrategy::DeleteDuplicates { keep }`].
    DeleteDuplicates(KeepPolicy),
    /// Acknowledge the warning without auto-cleanup. Used when
    /// duplicates are already prevented (baseline UNIQUE) or the
    /// baseline shape can't drive auto-cleanup (composite / no PK).
    ContinueWithoutCleanup,
}

/// Per-warning interactive prompt for F5. Returns `None` when the user
/// cancels (no migration written).
#[cfg(not(tarpaulin_include))]
pub(in crate::commands::revision) fn prompt_pk_additions(
    warning: &PrimaryKeyAdditionWarning,
) -> Result<Option<PrimaryKeyAdditionChoice>> {
    println!();
    super::print_section_rule();
    println!("{}", format_pk_addition_header(warning));
    super::print_section_rule();

    let mut labels: Vec<String> = Vec::new();
    let mut outcomes: Vec<Option<PrimaryKeyAdditionChoice>> = Vec::new();

    if warning.auto_cleanup_capable {
        labels.push(
            "Delete duplicates, keep FIRST (smallest baseline PK, recommended default)".to_string(),
        );
        outcomes.push(Some(PrimaryKeyAdditionChoice::DeleteDuplicates(
            KeepPolicy::First,
        )));
        labels.push("Delete duplicates, keep LAST (largest baseline PK)".to_string());
        outcomes.push(Some(PrimaryKeyAdditionChoice::DeleteDuplicates(
            KeepPolicy::Last,
        )));
    }
    labels.push(
        "Continue without auto-cleanup (NULL fill-with prompts will follow if needed)".to_string(),
    );
    outcomes.push(Some(PrimaryKeyAdditionChoice::ContinueWithoutCleanup));
    labels.push("Cancel migration".to_string());
    outcomes.push(None);

    let selection = Select::new()
        .with_prompt("  How should the PRIMARY KEY addition handle existing data?")
        .items(&labels)
        .default(0)
        .interact()
        .context("failed to read pk-addition choice")?;
    Ok(outcomes[selection])
}

fn format_pk_addition_header(warning: &PrimaryKeyAdditionWarning) -> String {
    let target = format!(
        "{}.({})",
        warning.table.bright_white().bold(),
        warning.columns.join(", ").bright_green()
    );
    let nullable_hint = if warning.nullable_columns.is_empty() {
        String::new()
    } else {
        format!(
            "\n  Nullable PK columns: {} ? fill_with prompt(s) will follow.",
            warning.nullable_columns.join(", ").bright_yellow()
        )
    };
    let dedup_hint = if warning.duplicate_possible {
        if warning.auto_cleanup_capable {
            "Auto-cleanup available (single-column PK).".to_string()
        } else {
            "Composite PK / no baseline PK ? user must pre-clean duplicates manually.".to_string()
        }
    } else {
        "Baseline UNIQUE already prevents duplicates.".to_string()
    };
    format!(
        "  {} Adding PRIMARY KEY on {target}\n  {dedup_hint}{nullable_hint}",
        "\u{26a0}".bright_yellow()
    )
}

/// Stamp the user's choice onto the matching `AddConstraint(PrimaryKey)`
/// action's `strategy` field.
pub(in crate::commands::revision) fn apply_pk_addition_choice(
    plan: &mut vespertide_core::MigrationPlan,
    warning: &PrimaryKeyAdditionWarning,
    choice: PrimaryKeyAdditionChoice,
) {
    let Some(action) = plan.actions.get_mut(warning.action_index) else {
        return;
    };
    let vespertide_core::MigrationAction::AddConstraint {
        constraint: vespertide_core::TableConstraint::PrimaryKey { strategy, .. },
        ..
    } = action
    else {
        return;
    };
    match choice {
        PrimaryKeyAdditionChoice::DeleteDuplicates(keep) => {
            *strategy = PrimaryKeyAdditionStrategy::DeleteDuplicates { keep };
        }
        PrimaryKeyAdditionChoice::ContinueWithoutCleanup => {
            // Strategy stays at default; the SQL emit will fall back
            // to no-op cleanup when the baseline shape isn't usable.
        }
    }
}

/// F96 (cascade reach analysis) - per-warning user confirmation for a
/// newly added `ON DELETE CASCADE` foreign key that extends a deep or
/// high-fanout cascade chain. No SQL is emitted from the choice;
/// vespertide cannot auto-shrink a user-declared cascade chain. The
/// user either acknowledges the chain (`Proceed`) or cancels the
/// migration to re-examine the model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::commands::revision) enum CascadeReachChoice {
    /// User confirmed the chain is intentional - proceed with the
    /// migration unchanged.
    Proceed,
}

/// Per-warning interactive prompt for F96. Returns `None` when the
/// user cancels.
#[cfg(not(tarpaulin_include))]
pub(in crate::commands::revision) fn prompt_cascade_reach(
    warning: &CascadeReachWarning,
) -> Result<Option<CascadeReachChoice>> {
    println!();
    super::print_section_rule();
    println!("{}", format_cascade_reach_header(warning));
    super::print_section_rule();

    let labels = [
        "Proceed (cascade chain is intentional)".to_string(),
        "Cancel (review schema first)".to_string(),
    ];
    let outcomes = [Some(CascadeReachChoice::Proceed), None];

    let selection = Select::new()
        .with_prompt("  Confirm the cascade chain")
        .items(&labels)
        .default(0)
        .interact()
        .context("failed to read cascade-reach choice")?;
    Ok(outcomes[selection])
}

/// F76 (sequence/identity exhaustion) - user resolution for a new
/// risky single-column auto-increment PK, PK type narrowing, or
/// FK-mismatch.
///
/// `ChangeToBigInt` is offered for `CreateTable` and
/// `ModifyColumnType` cases where vespertide can directly mutate the
/// plan action to widen the column. `AddConstraint(PrimaryKey)` and
/// `AddConstraint(ForeignKey)` cases offer only `Proceed` since
/// widening the baseline column requires the user to add a separate
/// `ModifyColumnType` action explicitly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::commands::revision) enum SequenceExhaustionChoice {
    /// Mutate the matching plan action so the risky column becomes
    /// `BigInt`. Only valid when the warning's action is a
    /// `CreateTable` or a `ModifyColumnType` (vespertide can rewrite
    /// those in place).
    ChangeToBigInt,
    /// Acknowledge the risk and keep the original type.
    Proceed,
}

/// Per-warning interactive prompt for F76. Returns `None` when the
/// user cancels.
#[cfg(not(tarpaulin_include))]
pub(in crate::commands::revision) fn prompt_sequence_exhaustion(
    warning: &SequenceExhaustionWarning,
) -> Result<Option<SequenceExhaustionChoice>> {
    println!();
    super::print_section_rule();
    println!("{}", format_sequence_exhaustion_header(warning));
    super::print_section_rule();

    let mutable = warning_is_mutable(warning);
    let mut labels: Vec<String> = Vec::new();
    let mut outcomes: Vec<Option<SequenceExhaustionChoice>> = Vec::new();

    if mutable {
        labels.push("Rewrite the column to big_int (recommended)".to_string());
        outcomes.push(Some(SequenceExhaustionChoice::ChangeToBigInt));
    }
    labels.push("Proceed (overflow is intentional / known-small table)".to_string());
    outcomes.push(Some(SequenceExhaustionChoice::Proceed));
    labels.push("Cancel (edit model to big_int)".to_string());
    outcomes.push(None);

    let selection = Select::new()
        .with_prompt("  How should this overflow risk be handled?")
        .items(&labels)
        .default(0)
        .interact()
        .context("failed to read sequence-exhaustion choice")?;
    Ok(outcomes[selection])
}

/// True when [`apply_sequence_exhaustion_choice`] can directly rewrite
/// the matching plan action. `CreateTable` and `ModifyColumnType`
/// carry the column type inline, so vespertide can widen them in
/// place; constraint-add actions reference an existing baseline
/// column that the user must widen via a separate plan edit.
fn warning_is_mutable(warning: &SequenceExhaustionWarning) -> bool {
    matches!(
        warning.kind,
        SequenceExhaustionKind::Primary | SequenceExhaustionKind::PkTypeNarrowing { .. }
    )
}

fn format_sequence_exhaustion_header(warning: &SequenceExhaustionWarning) -> String {
    let target = format!(
        "{}.{}",
        warning.table.bright_white().bold(),
        warning.column.bright_green()
    );
    let current = simple_int_label(warning.current_type);
    let risk_label = match warning.risk_level {
        SequenceRiskLevel::Medium => "Medium".bright_yellow(),
        SequenceRiskLevel::High => "High".bright_red().bold(),
    };
    let scenario = match &warning.kind {
        SequenceExhaustionKind::Primary => "single-column auto-increment PRIMARY KEY".to_string(),
        SequenceExhaustionKind::PkTypeNarrowing { from } => format!(
            "PRIMARY KEY type narrowing from {} to {}",
            simple_int_label(*from).bright_red(),
            current.bright_yellow()
        ),
        SequenceExhaustionKind::ForeignKeyMismatch {
            parent_table,
            parent_type,
        } => format!(
            "FOREIGN KEY mismatch: child {} vs parent {}.id ({})",
            current.bright_yellow(),
            parent_table.bright_white(),
            simple_int_label(*parent_type).bright_cyan()
        ),
    };
    let estimate = match warning.current_type {
        vespertide_core::SimpleColumnType::SmallInt => {
            "At realistic write rates: overflow in hours to days.".to_string()
        }
        vespertide_core::SimpleColumnType::Integer => {
            "At 1M new rows/day: overflow in ~5.9 years. At 10M/day: ~7 months.".to_string()
        }
        _ => String::new(),
    };
    format!(
        "  {} INT identity overflow risk\n  Target: {target} ({current})\n  Scenario: {scenario}\n  Risk: {risk_label}\n  {estimate}\n  Recommended: rewrite to big_int.",
        "\u{26a0}".bright_yellow()
    )
}

fn simple_int_label(ty: vespertide_core::SimpleColumnType) -> &'static str {
    match ty {
        vespertide_core::SimpleColumnType::SmallInt => "small_int",
        vespertide_core::SimpleColumnType::Integer => "integer",
        vespertide_core::SimpleColumnType::BigInt => "big_int",
        _ => "?",
    }
}

/// Stamp the user's choice onto the matching plan action. Mutation
/// happens in place for `CreateTable` (rewrite the column type inline)
/// and `ModifyColumnType` (replace `new_type`). Non-mutable cases
/// (`AddConstraint(...)`) are no-ops; the user is expected to have
/// declined the prompt or to be choosing `Proceed`.
pub(in crate::commands::revision) fn apply_sequence_exhaustion_choice(
    plan: &mut vespertide_core::MigrationPlan,
    warning: &SequenceExhaustionWarning,
    choice: SequenceExhaustionChoice,
) {
    if !matches!(choice, SequenceExhaustionChoice::ChangeToBigInt) {
        return;
    }
    let Some(action) = plan.actions.get_mut(warning.action_index) else {
        return;
    };
    match action {
        vespertide_core::MigrationAction::CreateTable { columns, .. } => {
            for col in columns.iter_mut() {
                if col.name.as_str() == warning.column {
                    col.r#type = vespertide_core::ColumnType::Simple(
                        vespertide_core::SimpleColumnType::BigInt,
                    );
                    break;
                }
            }
        }
        vespertide_core::MigrationAction::ModifyColumnType { new_type, .. } => {
            *new_type =
                vespertide_core::ColumnType::Simple(vespertide_core::SimpleColumnType::BigInt);
        }
        _ => {
            // AddConstraint(...) - vespertide does not rewrite the
            // baseline column from here; the prompt has already
            // hidden the `ChangeToBigInt` option for these cases.
        }
    }
}

fn format_cascade_reach_header(warning: &CascadeReachWarning) -> String {
    let arrow = "\u{2192}".bright_black();
    let origin = format!(
        "{}.({})",
        warning.origin_child_table.bright_white().bold(),
        warning.origin_columns.join(", ").bright_green()
    );
    let parent = warning.parent_table.bright_white().bold();
    let chain = std::iter::once(warning.parent_table.clone())
        .chain(warning.reached_tables.iter().cloned())
        .collect::<Vec<_>>()
        .join(" \u{2192} ");
    let risk_label = match warning.risk_level {
        CascadeRiskLevel::Deep => "Deep".bright_yellow(),
        CascadeRiskLevel::HighFanout => "HighFanout".bright_yellow(),
        CascadeRiskLevel::Critical => "Critical".bright_red().bold(),
    };
    format!(
        "  {} ON DELETE CASCADE chain warning\n  \
         {origin} {arrow} {parent} (ON DELETE CASCADE)\n  \
         Cascade reach: {} hops\n    {chain}\n  \
         Risk: {risk_label} (depth={}, max fanout={})\n  \
         Deleting from {parent} may cascade to many downstream rows. \
         Verify this is intentional.",
        "\u{26a0}".bright_yellow(),
        warning.depth,
        warning.depth,
        warning.max_fanout,
    )
}

/// F29 (CHECK expression strengthening) — user resolution for a CHECK
/// constraint whose new predicate is *demonstrably* stricter than the
/// old one. vespertide cannot transparently widen a user-declared
/// CHECK from inside the revision flow (the new predicate is exactly
/// what the user authored), so the only choices are to acknowledge
/// the strengthening (`Proceed`) — the migration will succeed only
/// if every existing row satisfies the new predicate — or cancel
/// and adjust the model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::commands::revision) enum CheckStrengtheningChoice {
    /// User confirmed the stricter predicate is intentional and
    /// has verified (out of band) that existing rows satisfy it.
    Proceed,
}

/// Per-warning interactive prompt for F29. Returns `None` when the
/// user cancels (typical fix: pre-clean violating rows in a prior
/// migration, then re-run `vespertide revision`).
#[cfg(not(tarpaulin_include))]
pub(in crate::commands::revision) fn prompt_check_strengthening(
    warning: &CheckStrengtheningWarning,
) -> Result<Option<CheckStrengtheningChoice>> {
    println!();
    super::print_section_rule();
    println!("{}", format_check_strengthening_header(warning));
    super::print_section_rule();

    let labels = [
        "Proceed (existing rows already satisfy the stricter predicate)".to_string(),
        "Cancel (pre-clean violating rows in a separate migration first)".to_string(),
    ];
    let outcomes = [Some(CheckStrengtheningChoice::Proceed), None];

    let selection = Select::new()
        .with_prompt("  How should this CHECK strengthening be handled?")
        .items(&labels)
        .default(1)
        .interact()
        .context("failed to read check-strengthening choice")?;
    Ok(outcomes[selection])
}

fn format_check_strengthening_header(warning: &CheckStrengtheningWarning) -> String {
    let table = warning.table.bright_white().bold();
    let name = warning.constraint_name.bright_cyan();
    let kind_label = match warning.kind {
        CheckStrengtheningKind::BoundaryTightened => "boundary tightened (literal moved tighter)",
        CheckStrengtheningKind::OperatorTightened => "operator tightened (>= -> >, or <= -> <)",
        CheckStrengtheningKind::InListShrunk => "IN list shrunk (allowed set narrowed)",
        CheckStrengtheningKind::BetweenNarrowed => "BETWEEN range narrowed",
        CheckStrengtheningKind::ConjunctAdded => "extra AND conjunct added",
        CheckStrengtheningKind::DisjunctRemoved => "OR disjunct removed",
    };
    format!(
        "  {warn} CHECK expression strengthened\n  \
         Table:      {table}\n  \
         Constraint: {name}\n  \
         Old:        {old}\n  \
         New:        {new}\n  \
         Change:     {kind_label}\n  \
         Risk: any existing row that satisfied the old predicate but \
         not the new one will cause the migration to fail at the \
         CHECK validation step. Verify your data first.",
        warn = "\u{26a0}".bright_yellow(),
        old = warning.old_expr.bright_red(),
        new = warning.new_expr.bright_green(),
    )
}

/// F-novel-4 (CHECK literal type-mismatch) — user resolution for a
/// CHECK constraint that compares a column to a literal of a
/// *demonstrably* incompatible type (e.g. `int_col = 'abc'`,
/// `bool_col = 'x'`, `uuid_col > 0`). `PostgreSQL` rejects these at
/// `ADD CONSTRAINT` time; `MySQL` / `SQLite` may silently coerce. Since
/// the literal is exactly what the user authored, vespertide cannot
/// auto-correct it — the only choices are to acknowledge the
/// mismatch (`Proceed`) or cancel and fix the model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::commands::revision) enum CheckTypeMismatchChoice {
    /// User confirmed the literal type is intentional (or the target
    /// backend coerces) and wants the migration written as authored.
    Proceed,
}

/// Per-warning interactive prompt for F-novel-4. Returns `None` when
/// the user cancels (typical fix: correct the literal or the column
/// type in the model, then re-run `vespertide revision`).
#[cfg(not(tarpaulin_include))]
pub(in crate::commands::revision) fn prompt_check_type_mismatch(
    warning: &CheckTypeMismatchWarning,
) -> Result<Option<CheckTypeMismatchChoice>> {
    println!();
    super::print_section_rule();
    println!("{}", format_check_type_mismatch_header(warning));
    super::print_section_rule();

    let labels = [
        "Proceed (literal type is intentional / backend coerces it)".to_string(),
        "Cancel (fix the literal or column type in the model first)".to_string(),
    ];
    let outcomes = [Some(CheckTypeMismatchChoice::Proceed), None];

    let selection = Select::new()
        .with_prompt("  How should this CHECK type mismatch be handled?")
        .items(&labels)
        .default(1)
        .interact()
        .context("failed to read check-type-mismatch choice")?;
    Ok(outcomes[selection])
}

fn format_check_type_mismatch_header(warning: &CheckTypeMismatchWarning) -> String {
    let table = warning.table.bright_white().bold();
    let name = warning.constraint_name.bright_cyan();
    let column = warning.column.bright_white().bold();
    format!(
        "  {warn} CHECK literal type mismatch\n  \
         Table:      {table}\n  \
         Constraint: {name}\n  \
         Column:     {column} ({column_type})\n  \
         Literal:    {literal} ({literal_kind})\n  \
         Expr:       {expr}\n  \
         Risk: the literal type is incompatible with the column type. \
         PostgreSQL rejects this at ADD CONSTRAINT time; MySQL and \
         SQLite may coerce silently. Verify the comparison is intended.",
        warn = "\u{26a0}".bright_yellow(),
        column_type = warning.column_type_label.bright_blue(),
        literal = warning.literal_text.bright_red(),
        literal_kind = warning.literal_kind.bright_magenta(),
        expr = warning.expr.bright_black(),
    )
}

#[cfg(test)]
mod tests;
