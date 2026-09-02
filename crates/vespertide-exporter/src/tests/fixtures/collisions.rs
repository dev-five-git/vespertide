//! Adversarial-name fixtures for the single-file backends' binding claims.

use vespertide_core::TableDef;
use vespertide_core::schema::column::{ColumnType, ComplexColumnType, SimpleColumnType};
use vespertide_core::schema::constraint::TableConstraint;

use super::{col, fk, pk, simple};

/// Adversarial file-scope binding names for the single-file backends: a table
/// whose const collides with another table's would-be `relations` const
/// (`user_relations` vs `user`), a table named after a drizzle-orm import
/// (`sql`), and a custom type named after a column constructor (`integer`).
/// Drizzle suffixes its way around each; the per-table backends are
/// unaffected.
pub(crate) fn binding_collisions() -> Vec<TableDef> {
    let user_relations = TableDef {
        name: "user_relations".into(),
        description: None,
        columns: vec![
            simple("id", SimpleColumnType::Integer),
            col(
                "kind",
                ColumnType::Complex(ComplexColumnType::Custom {
                    custom_type: "integer".to_string(),
                }),
            ),
        ],
        constraints: vec![pk(&["id"])],
    };
    let user = TableDef {
        name: "user".into(),
        description: None,
        columns: vec![simple("id", SimpleColumnType::Integer)],
        constraints: vec![pk(&["id"])],
    };
    let sql_table = TableDef {
        name: "sql".into(),
        description: None,
        columns: vec![
            simple("id", SimpleColumnType::Integer),
            simple("amount", SimpleColumnType::Integer),
        ],
        constraints: vec![
            pk(&["id"]),
            TableConstraint::Check {
                name: "chk_sql_amount".into(),
                expr: "amount > 0".into(),
                strategy: vespertide_core::CheckViolationStrategy::default(),
            },
        ],
    };
    let posts = TableDef {
        name: "posts".into(),
        description: None,
        columns: vec![
            simple("id", SimpleColumnType::Integer),
            simple("user_id", SimpleColumnType::Integer),
        ],
        constraints: vec![pk(&["id"]), fk(&["user_id"], "user", &["id"])],
    };
    [user_relations, user, sql_table, posts]
        .into_iter()
        .map(|t| t.normalize().expect("binding_collisions normalizes"))
        .collect()
}
