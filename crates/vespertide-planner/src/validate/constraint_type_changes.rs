//! Fault **F12** — PRIMARY KEY ↔ UNIQUE constraint transitions.
//!
//! Even though `PRIMARY KEY` and `UNIQUE` look syntactically similar, they
//! differ in three independent dimensions:
//!
//! 1. **NULL semantics** — PK columns are implicitly NOT NULL; UNIQUE
//!    columns allow NULL (with backend-specific composite NULL handling).
//! 2. **Row-identity / FK target** — PK is the canonical row identifier;
//!    UQ is one of potentially many alternate keys.
//! 3. **Cardinality** — at most one PK constraint per table; many UQ
//!    allowed.
//!
//! Swapping PK and UQ on the same column set in one migration crosses all
//! three boundaries silently. This module detects two failure shapes:
//!
//! - **A / B** — `find_constraint_type_changes` flags any plan that
//!   removes one type and adds the other on the same `(table, column
//!   set)`. The error surfaces the direction (`PkToUq` / `UqToPk`) and
//!   any foreign keys pointing at the affected column set so the user
//!   sees the downstream impact.
//! - **E** — `find_primary_key_removals` flags any plan that drops a
//!   table's PRIMARY KEY without adding a replacement PK (or dropping the
//!   table outright). Every Vespertide-managed table must have a PK.
//!
//! Both checks are **purely static** — they walk the plan and the
//! baseline schema, no DB access required. Callers should treat the
//! returned `PlannerError` values as hard errors that block migration
//! generation.

use std::collections::{BTreeMap, BTreeSet};

use vespertide_core::{
    ColumnName, MigrationAction, MigrationPlan, TableConstraint, TableDef,
    schema::names::join_column_names,
};

use crate::error::PlannerError;

/// Scan the plan for any PK ↔ UQ swap on the same column set.
///
/// Returns one [`PlannerError::ConstraintTypeChanged`] per detected swap,
/// in plan order. Empty when nothing offends.
#[must_use]
pub fn find_constraint_type_changes(
    plan: &MigrationPlan,
    baseline: &[TableDef],
) -> Vec<PlannerError> {
    // Key: (table, sorted column set). Value: list of (kind, action index).
    // We use a sorted column set so `["a","b"]` and `["b","a"]` collide.
    let mut removes: BTreeMap<ConstraintKey, ConstraintKind> = BTreeMap::new();
    let mut adds: BTreeMap<ConstraintKey, ConstraintKind> = BTreeMap::new();

    for action in &plan.actions {
        match action {
            MigrationAction::RemoveConstraint { table, constraint } => {
                if let Some((kind, columns)) = classify_constraint(constraint) {
                    let key = ConstraintKey::new(table.as_str(), columns);
                    removes.insert(key, kind);
                }
            }
            MigrationAction::AddConstraint { table, constraint } => {
                if let Some((kind, columns)) = classify_constraint(constraint) {
                    let key = ConstraintKey::new(table.as_str(), columns);
                    adds.insert(key, kind);
                }
            }
            // `ReplaceConstraint { from, to, .. }` carries `from` and `to` of
            // the *same* high-level type (the diff layer guarantees this).
            // Plans that explicitly Replace are not F12 candidates.
            _ => {}
        }
    }

    let mut out = Vec::new();
    for (key, remove_kind) in &removes {
        let Some(add_kind) = adds.get(key) else {
            continue;
        };
        if remove_kind == add_kind {
            // Same kind on both sides: re-issue of the same constraint
            // (e.g. rename). Not a type swap; ignore.
            continue;
        }
        let direction = match (remove_kind, add_kind) {
            (ConstraintKind::PrimaryKey, ConstraintKind::Unique) => "PK → UQ",
            (ConstraintKind::Unique, ConstraintKind::PrimaryKey) => "UQ → PK",
            _ => unreachable!("classify_constraint only returns PrimaryKey or Unique"),
        };
        out.push(PlannerError::ConstraintTypeChanged {
            kind: direction,
            table: key.table.clone(),
            columns: key.columns.join(", "),
            fk_hint: render_fk_hint(baseline, &key.table, &key.columns),
        });
    }
    out
}

/// Scan the plan for any PRIMARY KEY removal that is not paired with an
/// add (or a table drop). Each unpaired drop yields one
/// [`PlannerError::PrimaryKeyRemovedWithoutReplacement`].
///
/// Pure plan-only analysis: the baseline schema is never consulted because
/// every signal — PK adds, PK drops, and table drops — is already encoded
/// in `plan.actions`.
#[must_use]
pub fn find_primary_key_removals(plan: &MigrationPlan) -> Vec<PlannerError> {
    // Collect the tables that gain a PRIMARY KEY in this plan (either via
    // AddConstraint or as part of a CreateTable). These can absorb PK
    // removals on the same table.
    let mut tables_gaining_pk: BTreeSet<&str> = BTreeSet::new();
    let mut tables_dropped: BTreeSet<&str> = BTreeSet::new();
    for action in &plan.actions {
        match action {
            MigrationAction::AddConstraint {
                table,
                constraint: TableConstraint::PrimaryKey { .. },
            } => {
                tables_gaining_pk.insert(table.as_str());
            }
            MigrationAction::CreateTable {
                table, constraints, ..
            } if constraints
                .iter()
                .any(|c| matches!(c, TableConstraint::PrimaryKey { .. })) =>
            {
                tables_gaining_pk.insert(table.as_str());
            }
            MigrationAction::DeleteTable { table } => {
                tables_dropped.insert(table.as_str());
            }
            // `ReplaceConstraint { from: PK, to: PK }` keeps a PK, so it does
            // not count as a removal. The diff layer guarantees `from`/`to`
            // share a type for non-FK constraints, so a Replace involving PK
            // is always a PK-to-PK swap.
            _ => {}
        }
    }

    let mut out = Vec::new();
    for action in &plan.actions {
        let MigrationAction::RemoveConstraint {
            table,
            constraint: TableConstraint::PrimaryKey { columns, .. },
        } = action
        else {
            continue;
        };
        let table_str = table.as_str();
        if tables_dropped.contains(table_str) || tables_gaining_pk.contains(table_str) {
            continue;
        }
        out.push(PlannerError::PrimaryKeyRemovedWithoutReplacement {
            table: table_str.to_string(),
            columns: join_column_names(columns, ", "),
        });
    }
    out
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ConstraintKind {
    PrimaryKey,
    Unique,
}

/// Sort the columns alphabetically so `["a","b"]` and `["b","a"]` produce
/// the same key — the planner does not preserve column order for the
/// purpose of constraint identity.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct ConstraintKey {
    table: String,
    columns: Vec<String>,
}

impl ConstraintKey {
    fn new(table: &str, mut columns: Vec<String>) -> Self {
        columns.sort_unstable();
        Self {
            table: table.to_string(),
            columns,
        }
    }
}

fn classify_constraint(c: &TableConstraint) -> Option<(ConstraintKind, Vec<String>)> {
    match c {
        TableConstraint::PrimaryKey { columns, .. } => Some((
            ConstraintKind::PrimaryKey,
            columns.iter().map(ColumnName::to_string).collect(),
        )),
        TableConstraint::Unique { columns, .. } => Some((
            ConstraintKind::Unique,
            columns.iter().map(ColumnName::to_string).collect(),
        )),
        _ => None,
    }
}

/// Build a human-readable hint listing every FK in the baseline that
/// points at `(table, columns)`. Empty string when no FK references
/// match — the caller embeds this directly in the error message.
fn render_fk_hint(baseline: &[TableDef], target_table: &str, target_columns: &[String]) -> String {
    let target_set: BTreeSet<&str> = target_columns.iter().map(String::as_str).collect();
    let mut hits: Vec<String> = Vec::new();
    for table in baseline {
        for c in &table.constraints {
            if let TableConstraint::ForeignKey {
                name,
                columns,
                ref_table,
                ref_columns,
                ..
            } = c
            {
                if ref_table.as_str() != target_table {
                    continue;
                }
                let ref_set: BTreeSet<&str> = ref_columns.iter().map(ColumnName::as_str).collect();
                if ref_set != target_set {
                    continue;
                }
                let label = name
                    .clone()
                    .unwrap_or_else(|| format!("({})", join_column_names(columns, ", ")));
                hits.push(format!("{}.{}", table.name.as_str(), label));
            }
        }
    }
    if hits.is_empty() {
        String::new()
    } else {
        format!(
            "Foreign keys referencing this column set: {}.",
            hits.join(", ")
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{pk, plan, table};
    use vespertide_core::{ColumnDef, ColumnType, SimpleColumnType, TableName};

    fn col(name: &str) -> ColumnDef {
        ColumnDef::new(name, ColumnType::Simple(SimpleColumnType::Integer), false)
    }

    fn uq(name: Option<&str>, columns: Vec<&str>) -> TableConstraint {
        TableConstraint::Unique {
            name: name.map(ToString::to_string),
            columns: columns.into_iter().map(Into::into).collect(),
            strategy: vespertide_core::UniqueConstraintStrategy::DeleteDuplicates {
                keep: vespertide_core::KeepPolicy::First,
            },
        }
    }

    fn fk(
        name: Option<&str>,
        columns: Vec<&str>,
        ref_table: &str,
        ref_columns: Vec<&str>,
    ) -> TableConstraint {
        TableConstraint::ForeignKey {
            name: name.map(ToString::to_string),
            columns: columns.into_iter().map(Into::into).collect(),
            ref_table: ref_table.into(),
            ref_columns: ref_columns.into_iter().map(Into::into).collect(),
            on_delete: None,
            on_update: None,
            orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
        }
    }

    fn remove(table: &str, c: TableConstraint) -> MigrationAction {
        MigrationAction::RemoveConstraint {
            table: TableName::from(table),
            constraint: c,
        }
    }

    fn add(table: &str, c: TableConstraint) -> MigrationAction {
        MigrationAction::AddConstraint {
            table: TableName::from(table),
            constraint: c,
        }
    }

    // ── A: PK → UQ ──────────────────────────────────────────────────────

    /// Case 1: simple PK→UQ swap → ConstraintTypeChanged(PK → UQ).
    #[test]
    fn case_01_pk_to_uq_single_column() {
        let baseline = vec![table("user", vec![col("id")], vec![pk(vec!["id"])])];
        let p = plan(vec![
            remove("user", pk(vec!["id"])),
            add("user", uq(Some("uq_user_id"), vec!["id"])),
        ]);

        let errs = find_constraint_type_changes(&p, &baseline);
        assert_eq!(errs.len(), 1);
        assert!(matches!(
            &errs[0],
            PlannerError::ConstraintTypeChanged { kind, .. } if *kind == "PK → UQ"
        ));
    }

    /// Case 2: simple UQ→PK swap.
    #[test]
    fn case_02_uq_to_pk_single_column() {
        let baseline = vec![table(
            "user",
            vec![col("code")],
            vec![pk(vec!["code"]), uq(Some("uq_code"), vec!["code"])],
        )];
        let p = plan(vec![
            remove("user", uq(Some("uq_code"), vec!["code"])),
            add("user", pk(vec!["code"])),
        ]);

        let errs = find_constraint_type_changes(&p, &baseline);
        assert_eq!(errs.len(), 1);
        assert!(matches!(
            &errs[0],
            PlannerError::ConstraintTypeChanged { kind, .. } if *kind == "UQ → PK"
        ));
    }

    /// Case 3: composite PK → UQ — column-set match is order-insensitive.
    #[test]
    fn case_03_pk_to_uq_composite_order_insensitive() {
        let baseline = vec![table(
            "user_role",
            vec![col("user_id"), col("role_id")],
            vec![pk(vec!["user_id", "role_id"])],
        )];
        let p = plan(vec![
            remove("user_role", pk(vec!["user_id", "role_id"])),
            add(
                "user_role",
                uq(Some("uq_user_role"), vec!["role_id", "user_id"]), // swapped order
            ),
        ]);

        let errs = find_constraint_type_changes(&p, &baseline);
        assert_eq!(errs.len(), 1, "expected one PK→UQ error: {errs:?}");
    }

    /// Case 4: PK→UQ with FK pointing at the same column → `fk_hint` populated.
    #[test]
    fn case_04_pk_to_uq_with_fk_reference() {
        let baseline = vec![
            table("user", vec![col("id")], vec![pk(vec!["id"])]),
            table(
                "post",
                vec![col("id"), col("author_id")],
                vec![
                    pk(vec!["id"]),
                    fk(
                        Some("fk_post_author"),
                        vec!["author_id"],
                        "user",
                        vec!["id"],
                    ),
                ],
            ),
        ];
        let p = plan(vec![
            remove("user", pk(vec!["id"])),
            add("user", uq(Some("uq_user_id"), vec!["id"])),
        ]);

        let errs = find_constraint_type_changes(&p, &baseline);
        assert_eq!(errs.len(), 1);
        let PlannerError::ConstraintTypeChanged { fk_hint, .. } = &errs[0] else {
            panic!("wrong error variant");
        };
        assert!(
            fk_hint.contains("fk_post_author"),
            "fk hint missing reference: {fk_hint}"
        );
    }

    /// Case 5: PK→PK with different columns is `ReplaceConstraint`, not
    /// a type swap. F12 must NOT fire.
    #[test]
    fn case_05_pk_replace_same_type_is_safe() {
        let baseline = vec![table(
            "user",
            vec![col("id"), col("uuid")],
            vec![pk(vec!["id"])],
        )];
        let p = plan(vec![
            remove("user", pk(vec!["id"])),
            add("user", pk(vec!["uuid"])),
        ]);

        let errs = find_constraint_type_changes(&p, &baseline);
        assert!(errs.is_empty(), "PK→PK is safe, got: {errs:?}");
    }

    /// Case 6: UQ→UQ on different columns is also safe.
    #[test]
    fn case_06_uq_rename_same_type_is_safe() {
        let baseline = vec![table(
            "user",
            vec![col("id"), col("email")],
            vec![pk(vec!["id"]), uq(Some("uq_old"), vec!["email"])],
        )];
        let p = plan(vec![
            remove("user", uq(Some("uq_old"), vec!["email"])),
            add("user", uq(Some("uq_new"), vec!["email"])),
        ]);

        let errs = find_constraint_type_changes(&p, &baseline);
        assert!(errs.is_empty());
    }

    // ── E: PK removal without replacement ───────────────────────────────

    /// Case 7: Bare RemoveConstraint(PK) without any AddConstraint(PK).
    #[test]
    fn case_07_pk_removal_no_replacement() {
        let p = plan(vec![remove("user", pk(vec!["id"]))]);

        let errs = find_primary_key_removals(&p);
        assert_eq!(errs.len(), 1);
        assert!(matches!(
            &errs[0],
            PlannerError::PrimaryKeyRemovedWithoutReplacement { table, .. } if table == "user"
        ));
    }

    /// A CreateTable that carries NO PrimaryKey constraint must NOT be counted
    /// as "gaining a PK", so a PK removal on that same table still errors.
    /// Pins the `constraints.iter().any(matches!(PrimaryKey))` match guard on
    /// the CreateTable arm (a `-> true` mutant would absorb the removal).
    #[test]
    fn create_table_without_pk_does_not_absorb_pk_removal() {
        let p = plan(vec![
            MigrationAction::CreateTable {
                table: "user".into(),
                columns: vec![col("id")],
                constraints: vec![], // no PrimaryKey constraint
            },
            remove("user", pk(vec!["id"])),
        ]);

        let errs = find_primary_key_removals(&p);
        assert_eq!(
            errs.len(),
            1,
            "a PK-less CreateTable must not suppress the removal error"
        );
    }

    /// Case 8: PK removal + replacement on same table → no error
    /// (legitimate PK replacement).
    #[test]
    fn case_08_pk_removal_with_replacement() {
        let p = plan(vec![
            remove("user", pk(vec!["id"])),
            add("user", pk(vec!["uuid"])),
        ]);

        let errs = find_primary_key_removals(&p);
        assert!(errs.is_empty());
    }

    /// Case 9: PK removal + same table dropped → no error (whole table
    /// going away).
    #[test]
    fn case_09_pk_removal_with_table_drop() {
        let p = plan(vec![
            remove("user", pk(vec!["id"])),
            MigrationAction::DeleteTable {
                table: TableName::from("user"),
            },
        ]);

        let errs = find_primary_key_removals(&p);
        assert!(errs.is_empty());
    }

    /// Case 10: multiple unpaired PK removals → multiple errors.
    #[test]
    fn case_10_multiple_pk_removals_no_replacement() {
        let p = plan(vec![
            remove("a", pk(vec!["id"])),
            remove("b", pk(vec!["id"])),
        ]);

        let errs = find_primary_key_removals(&p);
        assert_eq!(errs.len(), 2);
    }

    /// Case 11: A/B suppression — PK→UQ on the same column set must NOT be
    /// flagged by `find_primary_key_removals` because Scenario A/B has its
    /// own dedicated detector. (The CLI calls both detectors; both should
    /// fire — the user gets the more specific A/B message AND the E
    /// message saying "no replacement PK exists".) This test pins down the
    /// expected behaviour so a future refactor doesn't accidentally collapse
    /// the two checks.
    #[test]
    fn case_11_pk_to_uq_also_triggers_pk_removal_check() {
        let baseline = vec![table("user", vec![col("id")], vec![pk(vec!["id"])])];
        let p = plan(vec![
            remove("user", pk(vec!["id"])),
            add("user", uq(Some("uq_user_id"), vec!["id"])),
        ]);

        let type_change_errs = find_constraint_type_changes(&p, &baseline);
        let pk_removal_errs = find_primary_key_removals(&p);

        assert_eq!(type_change_errs.len(), 1, "A/B detector must fire");
        assert_eq!(
            pk_removal_errs.len(),
            1,
            "E detector must also fire: PK is gone after the plan, replaced only by a UQ"
        );
    }

    /// Case 12: PK removal absorbed by `CreateTable` that includes a fresh PK
    /// (e.g. table recreation flow). No error.
    #[test]
    fn case_12_pk_removal_absorbed_by_create_table() {
        let p = plan(vec![
            remove("user", pk(vec!["id"])),
            MigrationAction::CreateTable {
                table: TableName::from("user"),
                columns: vec![col("uuid")],
                constraints: vec![pk(vec!["uuid"])],
            },
        ]);

        let errs = find_primary_key_removals(&p);
        assert!(errs.is_empty(), "expected absorbed PK, got: {errs:?}");
    }

    // ── Coverage-closure: ConstraintKey ordering + fk_hint empty paths ──

    /// `render_fk_hint` returns "" when no FK in baseline points at the
    /// affected `(table, columns)` set — exercises the early-return path
    /// at the end of the function via PK→UQ swap with no referencing FK.
    #[test]
    fn pk_to_uq_no_fk_reference_yields_empty_hint() {
        let baseline = vec![table("user", vec![col("id")], vec![pk(vec!["id"])])];
        let p = plan(vec![
            remove("user", pk(vec!["id"])),
            add("user", uq(Some("uq_user_id"), vec!["id"])),
        ]);

        let errs = find_constraint_type_changes(&p, &baseline);
        assert_eq!(errs.len(), 1);
        let PlannerError::ConstraintTypeChanged { fk_hint, .. } = &errs[0] else {
            panic!("wrong variant");
        };
        assert!(fk_hint.is_empty(), "fk_hint expected empty: {fk_hint:?}");
    }

    /// `render_fk_hint` skips FKs whose `ref_table` differs from the
    /// target — exercises the `if ref_table != target_table { continue; }`
    /// branch.
    #[test]
    fn pk_to_uq_with_fk_pointing_at_other_table_yields_empty_hint() {
        let baseline = vec![
            table("user", vec![col("id")], vec![pk(vec!["id"])]),
            table("other", vec![col("id")], vec![pk(vec!["id"])]),
            table(
                "log",
                vec![col("id"), col("other_id")],
                vec![
                    pk(vec!["id"]),
                    fk(Some("fk_log_other"), vec!["other_id"], "other", vec!["id"]),
                ],
            ),
        ];
        let p = plan(vec![
            remove("user", pk(vec!["id"])),
            add("user", uq(Some("uq"), vec!["id"])),
        ]);

        let errs = find_constraint_type_changes(&p, &baseline);
        let PlannerError::ConstraintTypeChanged { fk_hint, .. } = &errs[0] else {
            panic!("wrong variant");
        };
        // log's FK targets `other`, not `user` — hint must be empty.
        assert!(fk_hint.is_empty(), "fk_hint: {fk_hint}");
    }

    /// `render_fk_hint` skips FKs whose `ref_columns` set differs from the
    /// target column set — exercises the `if ref_set != target_set { continue; }`
    /// branch.
    #[test]
    fn pk_to_uq_with_fk_to_different_columns_yields_empty_hint() {
        // user has both `id` and `email`. The PK→UQ swap happens on `id`.
        // The log FK references `user.email`, a DIFFERENT column from the
        // affected set → ref_set != target_set → fk_hint stays empty.
        let baseline = vec![
            table("user", vec![col("id"), col("email")], vec![pk(vec!["id"])]),
            table(
                "log",
                vec![col("uemail")],
                vec![fk(
                    Some("fk_log_email"),
                    vec!["uemail"],
                    "user",
                    vec!["email"],
                )],
            ),
        ];
        let p = plan(vec![
            remove("user", pk(vec!["id"])),
            add("user", uq(Some("uq"), vec!["id"])),
        ]);

        let errs = find_constraint_type_changes(&p, &baseline);
        let PlannerError::ConstraintTypeChanged {
            fk_hint, columns, ..
        } = &errs[0]
        else {
            panic!("wrong variant");
        };
        // Affected unique column set is (id) but the FK references (email)
        // — ref_set != target_set → hint must be empty.
        assert_eq!(columns, "id");
        assert!(fk_hint.is_empty(), "fk_hint: {fk_hint}");
    }

    /// `render_fk_hint` falls back to the `(columns)` shape when the FK
    /// has no declared name.
    #[test]
    fn pk_to_uq_with_unnamed_fk_uses_columns_in_hint() {
        let baseline = vec![
            table("user", vec![col("id")], vec![pk(vec!["id"])]),
            table(
                "log",
                vec![col("user_id")],
                vec![fk(None, vec!["user_id"], "user", vec!["id"])],
            ),
        ];
        let p = plan(vec![
            remove("user", pk(vec!["id"])),
            add("user", uq(Some("uq"), vec!["id"])),
        ]);

        let errs = find_constraint_type_changes(&p, &baseline);
        let PlannerError::ConstraintTypeChanged { fk_hint, .. } = &errs[0] else {
            panic!("wrong variant");
        };
        assert!(fk_hint.contains("(user_id)"), "fk_hint: {fk_hint}");
    }

    #[test]
    fn classify_constraint_ignores_non_pk_unique_constraints() {
        let index = TableConstraint::Index {
            name: Some("ix_user__id".into()),
            columns: vec!["id".into()],
        };
        let check = TableConstraint::Check {
            name: "ck_id".into(),
            expr: "id > 0".into(),
            strategy: vespertide_core::CheckViolationStrategy::default(),
        };

        assert!(classify_constraint(&index).is_none());
        assert!(classify_constraint(&check).is_none());
    }
}
