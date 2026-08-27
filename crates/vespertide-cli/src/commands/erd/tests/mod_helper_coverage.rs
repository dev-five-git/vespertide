//! `erd/mod.rs` private-helper mutation-coverage tests, split out of
//! `tests/mod.rs` to keep that file under the 1200-line budget. `use
//! super::*;` reaches the shared fixtures (`table`, `primary_key`, `integer`,
//! `text`, `column`, `foreign_key`, `unique_foreign_key`, `normalize`,
//! `is_junction_table`, `are_columns_unique`, `foreign_key_column_groups`,
//! `parse_reference`, `ForeignKeySyntax`, …) defined in `tests/mod.rs`.
use super::*;

/// `is_junction_table` returns false when fewer than 2 distinct FK column
/// groups are present even though the table has 2+ PK columns → covers
/// mod.rs:259 (`return false;` after `foreign_key_groups.len() < 2`).
#[test]
fn is_junction_table_with_fewer_than_two_fk_groups_returns_false() {
    // 2 PK columns, only 1 inline FK → foreign_key_groups.len() == 1.
    let tbl = table(
        "link",
        vec![
            primary_key("a_id", integer()).foreign_key(ForeignKeySyntax::String("other.id".into())),
            primary_key("b_id", integer()),
        ],
    );
    assert!(!is_junction_table(&tbl));
}

/// `are_columns_unique` short-circuits to false when the FK column list is
/// empty → covers mod.rs:269 (`return false;`).
#[test]
fn are_columns_unique_empty_columns_returns_false() {
    let tbl = table("foo", vec![primary_key("id", integer())]);
    let empty: Vec<String> = Vec::new();
    assert!(!are_columns_unique(&tbl, &empty));
}

/// `are_columns_unique` returns true when the queried columns match the
/// table's primary key set → covers mod.rs:274 (`return true;`). Driven via
/// `collect_foreign_key_relations` so the OneToOne classification proves the
/// path executed end-to-end.
#[test]
fn are_columns_unique_pk_match_drives_one_to_one_via_collect_relations() {
    let users = normalize(&table("user", vec![primary_key("id", integer())]));
    // `profile` has a single PK column `user_id` that is ALSO an inline FK
    // to user.id → after normalize, primary_key_columns == FK columns →
    // are_columns_unique returns true via the PK-match branch.
    let profile = normalize(&table(
        "profile",
        vec![
            primary_key("user_id", integer())
                .foreign_key(ForeignKeySyntax::String("user.id".into())),
        ],
    ));
    let relations = collect_foreign_key_relations(&[users, profile]);
    let rel = relations
        .iter()
        .find(|r| r.child_table == "profile")
        .expect("profile relation");
    assert_eq!(rel.cardinality, Cardinality::OneToOne);
}

/// `foreign_key_column_groups` collects inline FK columns when not yet
/// normalized → covers mod.rs:305 (`if column.foreign_key.is_some()`) +
/// 308 (`groups.push(group)`). Drives through `is_junction_table` so the
/// branch executes on a real public path.
#[test]
fn foreign_key_column_groups_collects_inline_fk_for_unnormalized_junction() {
    // NOT normalized — inline FKs remain inline so foreign_key_column_groups'
    // inline-FK loop must process them.
    let junction = table(
        "user_tag",
        vec![
            primary_key("user_id", integer())
                .foreign_key(ForeignKeySyntax::String("user.id".into())),
            primary_key("tag_id", integer()).foreign_key(ForeignKeySyntax::String("tag.id".into())),
        ],
    );
    assert!(
        is_junction_table(&junction),
        "unnormalized junction should still classify via inline FK groups"
    );
}

/// `inline_unique_column_groups` handles `StrOrBoolOrArray::Bool(true)` by
/// inserting an auto-named group → covers mod.rs:332 (arm header) + 333
/// (`groups.insert(format!("__auto_{}", column.name), ...)`).
#[test]
fn inline_unique_column_groups_bool_true_creates_auto_group() {
    let users = normalize(&table("user", vec![primary_key("id", integer())]));
    // child has a single FK column declared `unique: true` (Bool variant).
    // `unique_foreign_key` builds exactly that shape.
    let child = table(
        "child",
        vec![
            primary_key("id", integer()),
            unique_foreign_key("user_id", "user.id"),
        ],
    );
    let relations = collect_foreign_key_relations(&[users, child]);
    let rel = relations
        .iter()
        .find(|r| r.child_table == "child")
        .expect("child relation");
    // OneToOne proves are_columns_unique returned true via the inline-unique
    // Bool(true) path (`__auto_{column}` group).
    assert_eq!(rel.cardinality, Cardinality::OneToOne);
}

/// Direct cover for `foreign_key_column_groups` line 305
/// (`if column.foreign_key.is_some()`). Calls the private helper with a
/// table whose columns carry inline FK syntax (un-normalized) so the
/// `column.foreign_key.is_some()` predicate evaluates true for each
/// inline-FK column and the `groups.push(group)` body executes.
#[test]
fn foreign_key_column_groups_inline_fk_column_executes_is_some_branch() {
    let tbl = table(
        "posts",
        vec![
            primary_key("id", integer()),
            foreign_key("user_id", "users.id"),
            foreign_key("author_id", "users.id"),
        ],
    );
    let groups = foreign_key_column_groups(&tbl);
    assert!(groups.iter().any(|g| g == &vec!["user_id".to_string()]));
    assert!(groups.iter().any(|g| g == &vec!["author_id".to_string()]));
}

/// Companion: column without `foreign_key` does NOT push a group. Locks
/// the false-branch of line 305 so a future refactor that reverses the
/// predicate is caught.
#[test]
fn foreign_key_column_groups_skips_columns_without_inline_fk() {
    let tbl = table(
        "plain",
        vec![primary_key("id", integer()), column("body", text())],
    );
    let groups = foreign_key_column_groups(&tbl);
    assert!(
        groups.is_empty(),
        "no inline FK → no groups; got {groups:?}"
    );
}

#[test]
fn foreign_key_column_groups_single_inline_fk_returns_single_column_group() {
    let tbl = table(
        "posts",
        vec![
            primary_key("id", integer()),
            foreign_key("user_id", "users.id"),
        ],
    );
    let groups = foreign_key_column_groups(&tbl);
    assert_eq!(groups, vec![vec!["user_id".to_string()]]);
}

#[test]
fn foreign_key_column_groups_pushes_object_inline_fk_without_table_constraint() {
    let inline_fk_column = ColumnDef::new("user_id", integer(), false).foreign_key(
        ForeignKeySyntax::Object(ForeignKeyDef {
            ref_table: "users".into(),
            ref_columns: vec!["id".into()],
            on_delete: None,
            on_update: None,
            orphan_strategy: Default::default(),
        }),
    );
    let tbl = TableDef {
        name: "posts".into(),
        description: None,
        columns: vec![primary_key("id", integer()), inline_fk_column],
        constraints: vec![],
    };

    let groups = foreign_key_column_groups(&tbl);

    let expected_group = vec!["user_id".to_string()];
    assert!(
        groups.iter().any(|group| group == &expected_group),
        "inline FK column group was not pushed: {groups:?}"
    );
    assert_eq!(groups, vec![expected_group]);
}

/// Inline FK whose parent table is not in the schema is silently ignored.
/// Covers the `table_lookup.get(parent_table)?` `None` branch inside
/// `inline_foreign_key_relation` (the early-return path when the referenced
/// table is absent from the provided schema).
#[test]
fn inline_fk_to_absent_table_is_ignored() {
    let article = table(
        "article",
        vec![
            primary_key("id", integer()),
            foreign_key("author_id", "user.id"),
        ],
    );
    // "user" table is deliberately absent — FK reference cannot resolve.
    assert!(collect_foreign_key_relations(&[article]).is_empty());
}

/// Mirrors `inline_fk_to_absent_table_is_ignored`, but for a table-level
/// `TableConstraint::ForeignKey` (the `let ... else { continue }` early-exit
/// path in `collect_foreign_key_relations` when the referenced table is
/// absent from the provided schema).
#[test]
fn table_level_fk_to_absent_table_is_ignored() {
    let article = TableDef {
        name: "article".into(),
        description: None,
        columns: vec![primary_key("id", integer()), column("author_id", integer())],
        constraints: vec![TableConstraint::ForeignKey {
            name: Some("fk_article__author_id".into()),
            columns: vec!["author_id".into()],
            ref_table: "user".into(),
            ref_columns: vec!["id".into()],
            on_delete: None,
            on_update: None,
            orphan_strategy: Default::default(),
        }],
    };
    // "user" table is deliberately absent — FK reference cannot resolve.
    assert!(collect_foreign_key_relations(&[article]).is_empty());
}

#[test]
fn foreign_key_column_groups_pushes_new_inline_group_after_table_constraint() {
    let tbl = TableDef {
        name: "posts".into(),
        description: None,
        columns: vec![
            primary_key("id", integer()),
            column("author_id", integer()),
            foreign_key("reviewer_id", "users.id"),
        ],
        constraints: vec![TableConstraint::ForeignKey {
            name: Some("fk_posts__author_id".into()),
            columns: vec!["author_id".into()],
            ref_table: "users".into(),
            ref_columns: vec!["id".into()],
            on_delete: None,
            on_update: None,
            orphan_strategy: Default::default(),
        }],
    };

    let groups = foreign_key_column_groups(&tbl);

    assert_eq!(
        groups,
        vec![
            vec!["author_id".to_string()],
            vec!["reviewer_id".to_string()]
        ]
    );
}

/// `parse_reference` is only reached indirectly (through
/// `collect_foreign_key_relations`), which leaves its accept/reject arms
/// attributed to a region the workspace-wide and single-package tarpaulin runs
/// disagree about. Calling it directly pins every branch to its own region.
#[rstest::rstest]
#[case::table_and_column("users.id", Some(("users", "id")))]
#[case::three_parts("a.b.c", None)]
#[case::empty_table(".id", None)]
#[case::empty_column("users.", None)]
#[case::no_separator("users", None)]
#[case::empty_input("", None)]
fn parse_reference_accepts_only_table_dot_column(
    #[case] input: &str,
    #[case] expected: Option<(&str, &str)>,
) {
    let expected = expected.map(|(table, column)| (table.to_string(), vec![column.to_string()]));
    assert_eq!(parse_reference(input), expected);
}
