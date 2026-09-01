use std::fmt::Write as _;

use colored::Colorize;
use vespertide_core::MigrationPlan;
use vespertide_planner::{RawSqlReplayHazard, find_raw_sql_replay_hazards};

/// Warn that `raw_sql` in the applied history makes baseline replay incomplete.
///
/// Shared by `vespertide diff` and `vespertide status`. Silent when the history
/// has no `raw_sql`, which is the common case.
pub(super) fn emit_raw_sql_replay_warning(plans: &[MigrationPlan]) {
    let hazards = find_raw_sql_replay_hazards(plans);
    if hazards.is_empty() {
        return;
    }

    println!();
    for line in format_raw_sql_replay_warning(&hazards).lines() {
        println!("{line}");
    }
}

/// Render the warning as a multi-line indented block.
/// Extracted so its output can be unit-tested without going through stdout.
fn format_raw_sql_replay_warning(hazards: &[RawSqlReplayHazard]) -> String {
    let versions = hazards
        .iter()
        .map(|hazard| {
            let plural = if hazard.count == 1 { "" } else { "s" };
            format!("{} ({} action{})", hazard.version, hazard.count, plural)
        })
        .collect::<Vec<_>>()
        .join(", ");

    let mut out = format!(
        "{} {}",
        "⚠".bright_yellow().bold(),
        format!(
            "{} applied migration(s) use raw_sql — baseline replay may be incomplete:",
            hazards.len()
        )
        .bright_yellow()
    );
    let _ = write!(
        out,
        "\n  {} {}",
        "versions:".bright_white(),
        versions.bright_cyan().bold()
    );
    let _ = write!(
        out,
        "\n  {} replay cannot interpret raw SQL, so any schema change those actions made \
         is missing from the reconstructed baseline — `vespertide diff` may report changes \
         that are already applied",
        "why:".bright_white()
    );
    let _ = write!(
        out,
        "\n  {} re-express schema changes as typed actions; use `data_migration` for \
         data-only SQL so replay can skip it safely",
        "fix:".bright_green()
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use vespertide_core::MigrationAction;

    fn plan_of(version: u32, actions: Vec<MigrationAction>) -> MigrationPlan {
        MigrationPlan {
            id: String::new(),
            comment: None,
            created_at: None,
            version,
            actions,
        }
    }

    fn raw() -> MigrationAction {
        MigrationAction::RawSql {
            sql: "ALTER TABLE users ADD c int".to_string(),
        }
    }

    #[test]
    fn warning_names_every_affected_version_with_its_action_count() {
        let rendered = format_raw_sql_replay_warning(&[
            RawSqlReplayHazard {
                version: 3,
                count: 1,
            },
            RawSqlReplayHazard {
                version: 7,
                count: 2,
            },
        ]);

        assert!(
            rendered.contains("2 applied migration(s) use raw_sql"),
            "{rendered}"
        );
        assert!(rendered.contains("3 (1 action)"), "{rendered}");
        assert!(rendered.contains("7 (2 actions)"), "{rendered}");
        assert!(rendered.contains("data_migration"), "{rendered}");
    }

    #[test]
    fn emit_is_silent_for_a_history_without_raw_sql() {
        emit_raw_sql_replay_warning(&[plan_of(1, vec![])]);
    }

    #[test]
    fn emit_prints_for_a_history_with_raw_sql() {
        emit_raw_sql_replay_warning(&[plan_of(1, vec![raw()])]);
    }
}
