//! Foreign keys carrying both referential actions.

use vespertide_core::schema::column::SimpleColumnType;
use vespertide_core::schema::constraint::TableConstraint;
use vespertide_core::{ReferenceAction, TableDef};

use super::{nullable_simple, pk, simple};

/// Foreign keys that set `ON UPDATE` as well as `ON DELETE`, one action each.
/// GORM, Prisma and Drizzle render both, Django renders `on_delete` alone
/// (its `ForeignKey` has no update action), and the remaining four drop them
/// — a spread this fixture pins. Between the two children every action
/// but `SET NULL` (pinned by `self_referencing_fk`) appears, each paired with a
/// different one so a backend that emits one in the other's place is visible;
/// `comments.post_id` is nullable so a relation on a nullable key carries
/// actions too. `SET DEFAULT` appears with a column default
/// (`comments.author_id`) and without one: Django accepts only the former.
pub(crate) fn reference_actions() -> Vec<TableDef> {
    let users = TableDef {
        name: "users".into(),
        description: None,
        columns: vec![simple("id", SimpleColumnType::Integer)],
        constraints: vec![pk(&["id"])],
    };
    let posts = TableDef {
        name: "posts".into(),
        description: None,
        columns: vec![
            simple("id", SimpleColumnType::Integer),
            simple("user_id", SimpleColumnType::Integer),
        ],
        constraints: vec![
            pk(&["id"]),
            fk_with_actions(
                "user_id",
                "users",
                ReferenceAction::Cascade,
                ReferenceAction::Restrict,
            ),
        ],
    };
    let comments = TableDef {
        name: "comments".into(),
        description: None,
        columns: vec![
            simple("id", SimpleColumnType::Integer),
            nullable_simple("post_id", SimpleColumnType::Integer),
            simple("author_id", SimpleColumnType::Integer).default("1".into()),
        ],
        constraints: vec![
            pk(&["id"]),
            fk_with_actions(
                "post_id",
                "posts",
                ReferenceAction::SetDefault,
                ReferenceAction::NoAction,
            ),
            fk_with_actions(
                "author_id",
                "users",
                ReferenceAction::SetDefault,
                ReferenceAction::Cascade,
            ),
        ],
    };
    [users, posts, comments]
        .into_iter()
        .map(|t| t.normalize().expect("reference_actions normalizes"))
        .collect()
}

fn fk_with_actions(
    column: &str,
    ref_table: &str,
    on_delete: ReferenceAction,
    on_update: ReferenceAction,
) -> TableConstraint {
    TableConstraint::ForeignKey {
        name: None,
        columns: vec![column.into()],
        ref_table: ref_table.into(),
        ref_columns: vec!["id".into()],
        on_delete: Some(on_delete),
        on_update: Some(on_update),
        orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
    }
}
