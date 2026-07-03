use super::*;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn int_col(name: &str) -> ColumnDef {
    let mut c = col(name, ColumnType::Simple(SimpleColumnType::Integer));
    c.nullable = false;
    c
}

fn pk_id() -> TableConstraint {
    pk(vec!["id"])
}

fn unique(name: &str, columns: Vec<&str>) -> TableConstraint {
    TableConstraint::Unique {
        name: Some(name.to_string()),
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

fn parent() -> TableDef {
    table("parent", vec![int_col("id")], vec![pk_id()])
}

// ---------------------------------------------------------------------------
// Detection: FK without any covering index
// ---------------------------------------------------------------------------

#[test]
fn fk_without_supporting_index_is_detected() {
    let child = table(
        "child",
        vec![int_col("id"), int_col("parent_id")],
        vec![
            pk_id(),
            fk(
                Some("fk_child_parent"),
                vec!["parent_id"],
                "parent",
                vec!["id"],
            ),
        ],
    );
    let schema = vec![parent(), child];

    let missing = find_missing_fk_supporting_indexes(&schema);

    assert_eq!(missing.len(), 1);
    let item = &missing[0];
    assert_eq!(item.table, "child");
    assert_eq!(item.constraint_name.as_deref(), Some("fk_child_parent"));
    assert_eq!(item.columns, vec!["parent_id"]);
    assert_eq!(item.ref_table, "parent");
    assert_eq!(item.ref_columns, vec!["id"]);
    assert_eq!(item.suggested_index_name, "ix_child__parent_id");
}

#[test]
fn unnamed_fk_without_supporting_index_is_detected_with_none_name() {
    let child = table(
        "child",
        vec![int_col("id"), int_col("parent_id")],
        vec![pk_id(), fk(None, vec!["parent_id"], "parent", vec!["id"])],
    );
    let schema = vec![parent(), child];

    let missing = find_missing_fk_supporting_indexes(&schema);

    assert_eq!(missing.len(), 1);
    assert!(missing[0].constraint_name.is_none());
}

// ---------------------------------------------------------------------------
// Covered: exact / leading-prefix / PK / Unique
// ---------------------------------------------------------------------------

#[test]
fn fk_covered_by_exact_index_is_not_detected() {
    let child = table(
        "child",
        vec![int_col("id"), int_col("parent_id")],
        vec![
            pk_id(),
            idx("ix_child__parent_id", vec!["parent_id"]),
            fk(None, vec!["parent_id"], "parent", vec!["id"]),
        ],
    );
    let schema = vec![parent(), child];

    let missing = find_missing_fk_supporting_indexes(&schema);
    assert!(missing.is_empty());
}

#[test]
fn fk_covered_by_leading_prefix_index_is_not_detected() {
    // FK on [parent_id]; existing composite index on [parent_id, created_at]
    let child = table(
        "child",
        vec![int_col("id"), int_col("parent_id"), int_col("created_at")],
        vec![
            pk_id(),
            idx(
                "ix_child__parent_id_created_at",
                vec!["parent_id", "created_at"],
            ),
            fk(None, vec!["parent_id"], "parent", vec!["id"]),
        ],
    );
    let schema = vec![parent(), child];

    let missing = find_missing_fk_supporting_indexes(&schema);
    assert!(missing.is_empty());
}

#[test]
fn fk_covered_by_primary_key_leading_prefix_is_not_detected() {
    // PK on [parent_id, sequence_no]; FK on [parent_id] — PK is a covering index.
    let child = table(
        "child",
        vec![int_col("parent_id"), int_col("sequence_no")],
        vec![
            pk(vec!["parent_id", "sequence_no"]),
            fk(None, vec!["parent_id"], "parent", vec!["id"]),
        ],
    );
    let schema = vec![parent(), child];

    let missing = find_missing_fk_supporting_indexes(&schema);
    assert!(missing.is_empty());
}

#[test]
fn fk_covered_by_unique_constraint_is_not_detected() {
    let child = table(
        "child",
        vec![int_col("id"), int_col("parent_id")],
        vec![
            pk_id(),
            unique("uq_child__parent_id", vec!["parent_id"]),
            fk(None, vec!["parent_id"], "parent", vec!["id"]),
        ],
    );
    let schema = vec![parent(), child];

    let missing = find_missing_fk_supporting_indexes(&schema);
    assert!(missing.is_empty());
}

// ---------------------------------------------------------------------------
// Not covered: wrong order / suffix only / insufficient prefix
// ---------------------------------------------------------------------------

#[test]
fn composite_fk_covered_when_pk_matches_even_with_other_unhelpful_indexes() {
    // PK [a, b] covers FK [a, b]; an extra wrong-order index [b, a] is
    // irrelevant — coverage comes from the PK alone.
    let child = table(
        "child",
        vec![int_col("a"), int_col("b")],
        vec![
            pk(vec!["a", "b"]),
            idx("ix_child__b_a", vec!["b", "a"]),
            fk(
                Some("fk_child__a_b"),
                vec!["a", "b"],
                "parent",
                vec!["id", "id"],
            ),
        ],
    );
    let schema = vec![parent(), child];

    let missing = find_missing_fk_supporting_indexes(&schema);
    assert!(missing.is_empty(), "PK [a,b] alone covers FK [a,b]");
}

#[test]
fn wrong_order_index_alone_does_not_cover_composite_fk() {
    // Detection is called independently from schema validation, so PK absence
    // is permitted here; we test the prefix-order invariant in isolation.
    // FK on [a, b] is NOT covered by an index on [b, a].
    let child = TableDef {
        name: "child".into(),
        description: None,
        columns: vec![int_col("a"), int_col("b")],
        constraints: vec![
            idx("ix_child__b_a", vec!["b", "a"]),
            fk(
                Some("fk_child__a_b"),
                vec!["a", "b"],
                "parent",
                vec!["id", "id"],
            ),
        ],
    };
    let schema = vec![parent(), child];

    let missing = find_missing_fk_supporting_indexes(&schema);

    assert_eq!(missing.len(), 1);
    assert_eq!(missing[0].columns, vec!["a", "b"]);
    assert_eq!(missing[0].suggested_index_name, "ix_child__a_b");
}

#[test]
fn fk_columns_must_be_leading_prefix_not_anywhere_in_index() {
    // FK on [parent_id]; only existing index is on [other_col, parent_id]
    // (parent_id appears, but not as the leading column).
    let child = table(
        "child",
        vec![int_col("id"), int_col("parent_id"), int_col("other_col")],
        vec![
            pk_id(),
            idx("ix_child__other_parent", vec!["other_col", "parent_id"]),
            fk(None, vec!["parent_id"], "parent", vec!["id"]),
        ],
    );
    let schema = vec![parent(), child];

    let missing = find_missing_fk_supporting_indexes(&schema);

    assert_eq!(missing.len(), 1);
    assert_eq!(missing[0].columns, vec!["parent_id"]);
}

#[test]
fn composite_fk_partially_covered_is_still_detected() {
    // FK on [a, b]; only existing index is on [a] — prefix is too short to
    // cover the full FK column list.
    let child = table(
        "child",
        vec![int_col("id"), int_col("a"), int_col("b")],
        vec![
            pk_id(),
            idx("ix_child__a", vec!["a"]),
            fk(
                Some("fk_child__ab"),
                vec!["a", "b"],
                "parent",
                vec!["id", "id"],
            ),
        ],
    );
    let schema = vec![parent(), child];

    let missing = find_missing_fk_supporting_indexes(&schema);

    assert_eq!(missing.len(), 1);
    assert_eq!(missing[0].columns, vec!["a", "b"]);
    assert_eq!(missing[0].suggested_index_name, "ix_child__a_b");
}

// ---------------------------------------------------------------------------
// Aggregation: multiple FKs / multiple tables
// ---------------------------------------------------------------------------

#[test]
fn only_uncovered_fks_are_returned() {
    // Two FKs on the same table: one covered, one not.
    let child = table(
        "child",
        vec![
            int_col("id"),
            int_col("parent_id"),
            int_col("other_parent_id"),
        ],
        vec![
            pk_id(),
            idx("ix_child__parent_id", vec!["parent_id"]),
            fk(Some("fk_a"), vec!["parent_id"], "parent", vec!["id"]),
            fk(Some("fk_b"), vec!["other_parent_id"], "parent", vec!["id"]),
        ],
    );
    let schema = vec![parent(), child];

    let missing = find_missing_fk_supporting_indexes(&schema);

    assert_eq!(missing.len(), 1);
    assert_eq!(missing[0].constraint_name.as_deref(), Some("fk_b"));
    assert_eq!(missing[0].columns, vec!["other_parent_id"]);
}

#[test]
fn multiple_tables_each_report_independently() {
    let a = table(
        "a",
        vec![int_col("id"), int_col("parent_id")],
        vec![
            pk_id(),
            // covered
            idx("ix_a__parent_id", vec!["parent_id"]),
            fk(Some("fk_a"), vec!["parent_id"], "parent", vec!["id"]),
        ],
    );
    let b = table(
        "b",
        vec![int_col("id"), int_col("parent_id")],
        vec![
            pk_id(),
            // NOT covered
            fk(Some("fk_b"), vec!["parent_id"], "parent", vec!["id"]),
        ],
    );
    let schema = vec![parent(), a, b];

    let missing = find_missing_fk_supporting_indexes(&schema);

    assert_eq!(missing.len(), 1);
    assert_eq!(missing[0].table, "b");
    assert_eq!(missing[0].constraint_name.as_deref(), Some("fk_b"));
}

// ---------------------------------------------------------------------------
// Edge cases
// ---------------------------------------------------------------------------

#[test]
fn schema_without_any_fk_returns_empty() {
    let t = table(
        "users",
        vec![int_col("id"), int_col("email")],
        vec![pk_id(), unique("uq_users__email", vec!["email"])],
    );
    let schema = vec![t];

    let missing = find_missing_fk_supporting_indexes(&schema);
    assert!(missing.is_empty());
}

#[test]
fn empty_schema_returns_empty() {
    let missing = find_missing_fk_supporting_indexes(&[]);
    assert!(missing.is_empty());
}

#[test]
fn self_referential_fk_without_index_is_detected() {
    // Common pattern: tree structures (parent_id → self.id) where the
    // self-FK lacks an explicit child-side index, causing recursive query
    // regressions.
    let users = table(
        "users",
        vec![int_col("id"), int_col("parent_id")],
        vec![
            pk_id(),
            fk(
                Some("fk_users__parent"),
                vec!["parent_id"],
                "users",
                vec!["id"],
            ),
        ],
    );
    let schema = vec![users];

    let missing = find_missing_fk_supporting_indexes(&schema);

    assert_eq!(missing.len(), 1);
    assert_eq!(missing[0].table, "users");
    assert_eq!(missing[0].ref_table, "users");
    assert_eq!(missing[0].suggested_index_name, "ix_users__parent_id");
}
