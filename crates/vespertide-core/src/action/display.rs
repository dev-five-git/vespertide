use super::MigrationAction;
use crate::schema::TableConstraint;
use std::fmt;

impl fmt::Display for MigrationAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write_migration_action(f, self)
    }
}

fn write_migration_action(f: &mut fmt::Formatter<'_>, action: &MigrationAction) -> fmt::Result {
    match action {
        MigrationAction::CreateTable { table, .. } => write!(f, "CreateTable: {table}"),
        MigrationAction::DeleteTable { table } => write!(f, "DeleteTable: {table}"),
        MigrationAction::AddColumn { table, column, .. } => {
            write!(f, "AddColumn: {}.{}", table, column.name)
        }
        MigrationAction::RenameColumn { table, from, to } => {
            write!(f, "RenameColumn: {table}.{from} -> {to}")
        }
        MigrationAction::DeleteColumn { table, column } => {
            write!(f, "DeleteColumn: {table}.{column}")
        }
        MigrationAction::ModifyColumnType { table, column, .. } => {
            write!(f, "ModifyColumnType: {table}.{column}")
        }
        MigrationAction::ModifyColumnNullable {
            table,
            column,
            nullable,
            ..
        } => write_nullable_action(f, table, column, *nullable),
        MigrationAction::ModifyColumnDefault {
            table,
            column,
            new_default,
            ..
        } => write_default_action(f, table, column, new_default.as_deref()),
        MigrationAction::ModifyColumnComment {
            table,
            column,
            new_comment,
        } => write_comment_action(f, table, column, new_comment.as_deref()),
        MigrationAction::AddConstraint { table, constraint } => {
            write_constraint_action(f, "AddConstraint", table, constraint)
        }
        MigrationAction::RemoveConstraint { table, constraint } => {
            write_constraint_action(f, "RemoveConstraint", table, constraint)
        }
        MigrationAction::ReplaceConstraint { table, to, .. } => {
            write_constraint_action(f, "ReplaceConstraint", table, to)
        }
        MigrationAction::RenameTable { from, to } => write!(f, "RenameTable: {from} -> {to}"),
        MigrationAction::RawSql { sql } => write_raw_sql_action(f, sql),
        MigrationAction::RemapEnumValues {
            table,
            column,
            mapping,
        } => {
            let summary = mapping
                .iter()
                .map(|(old, new)| format!("{old}->{new}"))
                .collect::<Vec<_>>()
                .join(", ");
            write!(f, "RemapEnumValues: {table}.{column} [{summary}]")
        }
    }
}

fn write_nullable_action(
    f: &mut fmt::Formatter<'_>,
    table: &str,
    column: &str,
    nullable: bool,
) -> fmt::Result {
    let nullability = if nullable { "NULL" } else { "NOT NULL" };
    write!(f, "ModifyColumnNullable: {table}.{column} -> {nullability}")
}

fn write_default_action(
    f: &mut fmt::Formatter<'_>,
    table: &str,
    column: &str,
    default: Option<&str>,
) -> fmt::Result {
    if let Some(default) = default {
        write!(f, "ModifyColumnDefault: {table}.{column} -> {default}")
    } else {
        write!(f, "ModifyColumnDefault: {table}.{column} -> (none)")
    }
}

fn write_comment_action(
    f: &mut fmt::Formatter<'_>,
    table: &str,
    column: &str,
    comment: Option<&str>,
) -> fmt::Result {
    if let Some(comment) = comment {
        let display = truncate_comment(comment);
        write!(f, "ModifyColumnComment: {table}.{column} -> '{display}'")
    } else {
        write!(f, "ModifyColumnComment: {table}.{column} -> (none)")
    }
}

fn truncate_comment(comment: &str) -> String {
    if comment.chars().count() > 30 {
        format!("{}...", truncate_chars(comment, 27))
    } else {
        comment.to_string()
    }
}

fn truncate_chars(s: &str, max_chars: usize) -> String {
    s.chars().take(max_chars).collect()
}

fn write_raw_sql_action(f: &mut fmt::Formatter<'_>, sql: &str) -> fmt::Result {
    let display_sql = if sql.chars().count() > 50 {
        format!("{}...", truncate_chars(sql, 47))
    } else {
        sql.to_string()
    };
    write!(f, "RawSql: {display_sql}")
}

fn write_constraint_action(
    f: &mut fmt::Formatter<'_>,
    action: &str,
    table: &str,
    constraint: &TableConstraint,
) -> fmt::Result {
    match constraint {
        TableConstraint::PrimaryKey { .. } => write!(f, "{action}: {table}.PRIMARY KEY"),
        TableConstraint::Unique { name, .. } => {
            write_named_constraint(f, action, table, name.as_ref(), "UNIQUE")
        }
        TableConstraint::ForeignKey { name, .. } => {
            write_named_constraint(f, action, table, name.as_ref(), "FOREIGN KEY")
        }
        TableConstraint::Check { name, .. } => write!(f, "{action}: {table}.{name} (CHECK)"),
        TableConstraint::Index { name, .. } => {
            write_named_constraint(f, action, table, name.as_ref(), "INDEX")
        }
    }
}

fn write_named_constraint(
    f: &mut fmt::Formatter<'_>,
    action: &str,
    table: &str,
    name: Option<&String>,
    fallback: &str,
) -> fmt::Result {
    if let Some(name) = name {
        write!(f, "{action}: {table}.{name} ({fallback})")
    } else {
        write!(f, "{action}: {table}.{fallback}")
    }
}

#[cfg(test)]
mod tests {
    //! Coverage-closure tests for the `ModifyColumnDefault` Display match arm.
    //! Targets `uncovered-detail.json` lines 35, 36, 37 (the field bindings
    //! `column`, `new_default`, `..` inside the `ModifyColumnDefault` pattern).
    use super::*;
    use crate::action::MigrationAction;

    #[test]
    fn modify_column_default_some_format() {
        // Hits lines 33-38 (ModifyColumnDefault pattern with new_default = Some).
        let action = MigrationAction::ModifyColumnDefault {
            table: "user".into(),
            column: "status".into(),
            new_default: Some("'active'".to_string()),
            backfill: None,
        };
        assert_eq!(
            format!("{action}"),
            "ModifyColumnDefault: user.status -> 'active'"
        );
    }

    #[test]
    fn modify_column_default_none_format() {
        // Re-exercises the ModifyColumnDefault arm with new_default = None.
        let action = MigrationAction::ModifyColumnDefault {
            table: "user".into(),
            column: "status".into(),
            new_default: None,
            backfill: None,
        };
        assert_eq!(
            format!("{action}"),
            "ModifyColumnDefault: user.status -> (none)"
        );
    }

    #[test]
    fn remap_enum_values_format_single_mapping() {
        // Drives lines 35, 36, 37 — the RemapEnumValues match arm: the
        // pattern binding (`table`, `column`, `mapping`), the iterator that
        // joins `old->new` pairs into the summary, and the `write!` call
        // that emits the bracketed summary.
        let action = MigrationAction::RemapEnumValues {
            table: "user".into(),
            column: "status".into(),
            mapping: vec![(1_i64, 2_i64)].into_iter().collect(),
        };
        assert_eq!(format!("{action}"), "RemapEnumValues: user.status [1->2]");
    }

    #[test]
    fn remap_enum_values_format_multiple_mappings_joined() {
        // Re-exercises the same arm with a multi-entry BTreeMap so the
        // `.join(", ")` in line 36 produces a non-trivial summary. BTreeMap
        // iteration is sorted, so the resulting order is deterministic.
        let action = MigrationAction::RemapEnumValues {
            table: "order".into(),
            column: "state".into(),
            mapping: vec![(1_i64, 10_i64), (2_i64, 20_i64)].into_iter().collect(),
        };
        assert_eq!(
            format!("{action}"),
            "RemapEnumValues: order.state [1->10, 2->20]"
        );
    }

    // ── truncate_comment / truncate_chars unit tests ─────────────────────

    /// Kills the `> 30` → `>= 30` boundary mutant.
    ///
    /// A 30-char string must be returned unchanged; a 31-char string must be
    /// truncated to 27 chars + "...".
    #[test]
    fn truncate_comment_30_char_boundary() {
        let s30 = "a".repeat(30);
        assert_eq!(
            truncate_comment(&s30),
            s30,
            "30-char string must be returned unchanged"
        );

        let s31 = "a".repeat(31);
        let expected = format!("{}...", "a".repeat(27));
        assert_eq!(
            truncate_comment(&s31),
            expected,
            "31-char string must be truncated to 27 chars + '...'"
        );
    }

    /// Kills the `.chars().count()` → `.len()` mutant.
    ///
    /// "é" is 2 bytes but 1 char.  30 × "é" = 30 chars / 60 bytes → unchanged.
    /// 31 × "é" = 31 chars / 62 bytes → truncated by char count, not byte count.
    #[test]
    fn truncate_comment_multibyte_char_count() {
        let s30 = "é".repeat(30);
        assert_eq!(
            truncate_comment(&s30),
            s30,
            "30 multibyte chars must be returned unchanged"
        );

        let s31 = "é".repeat(31);
        let expected = format!("{}...", "é".repeat(27));
        assert_eq!(
            truncate_comment(&s31),
            expected,
            "31 multibyte chars must be truncated by char count"
        );
    }

    /// Kills the `take(n)` → `take(MAX)` mutant.
    ///
    /// `truncate_chars("hi", 0)` must return an empty string.
    #[test]
    fn truncate_chars_zero_returns_empty() {
        assert_eq!(
            truncate_chars("hi", 0),
            "",
            "truncate_chars with max_chars=0 must return empty string"
        );
    }

    /// Kills the `.chars()` → `.bytes()` mutant.
    ///
    /// "héllo" has 5 chars; taking 3 must yield "hél" (not a byte-split panic).
    #[test]
    fn truncate_chars_no_grapheme_panic() {
        assert_eq!(
            truncate_chars("héllo", 3),
            "hél",
            "truncate_chars must split by chars, not bytes"
        );
    }

    // write_raw_sql_action truncates the RawSql preview only when
    // chars().count() > 50. Pins the `> 50` boundary so a mutation to
    // `>= 50` (which would truncate an exactly-50-char SQL) is caught.
    #[test]
    fn raw_sql_display_50_char_boundary_not_truncated() {
        let sql50: String = "0123456789".repeat(5); // exactly 50 chars
        let out = format!("{}", crate::MigrationAction::RawSql { sql: sql50.clone() });
        assert_eq!(out, format!("RawSql: {sql50}"));
        assert!(
            !out.contains("..."),
            "50-char SQL must NOT be truncated: {out}"
        );

        let sql51 = format!("{sql50}X"); // 51 chars → truncated to 47 + "..."
        let out51 = format!("{}", crate::MigrationAction::RawSql { sql: sql51.clone() });
        let head: String = sql51.chars().take(47).collect();
        assert_eq!(out51, format!("RawSql: {head}..."));
    }

    /// Migrated INLINE from `crates/vespertide-core/tests/utf8_safety.rs`:
    /// the `MigrationAction::RawSql` Display+Debug impls must never panic on
    /// arbitrary Unicode input. This is a single-module proptest of the
    /// Display impl that lives in this file — it belongs inline.
    mod utf8 {
        use crate::MigrationAction;
        use proptest::prelude::*;

        proptest! {
            #[test]
            fn action_display_does_not_panic_on_unicode(
                s in proptest::collection::vec(any::<char>(), 0..100)
                    .prop_map(|v| v.into_iter().collect::<String>())
            ) {
                let action = MigrationAction::RawSql { sql: s };
                let _ = format!("{action:?}");
                let _ = format!("{action}");
            }
        }
    }
}
