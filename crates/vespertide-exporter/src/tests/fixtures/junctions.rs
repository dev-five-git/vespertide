//! Junction tables a backend's many-to-many support has to tell apart.

use vespertide_core::TableDef;
use vespertide_core::schema::column::SimpleColumnType;

use super::{fk, pk, simple};

/// A junction whose far end has a composite primary key, so it reaches that
/// end by a composite foreign key. Whether that still counts as a
/// many-to-many is up to the backend: Django cannot relate to a composite-key
/// model at all, and leaves all three as plain models.
pub(crate) fn junction_over_composite_key() -> Vec<TableDef> {
    let user = TableDef {
        name: "user".into(),
        description: None,
        columns: vec![simple("id", SimpleColumnType::Integer)],
        constraints: vec![pk(&["id"])],
    };
    let article = TableDef {
        name: "article".into(),
        description: None,
        columns: vec![
            simple("media_id", SimpleColumnType::Integer),
            simple("id", SimpleColumnType::Integer),
        ],
        constraints: vec![pk(&["media_id", "id"])],
    };
    let article_user = TableDef {
        name: "article_user".into(),
        description: None,
        columns: vec![
            simple("media_id", SimpleColumnType::Integer),
            simple("article_id", SimpleColumnType::Integer),
            simple("user_id", SimpleColumnType::Integer),
        ],
        constraints: vec![
            pk(&["media_id", "article_id", "user_id"]),
            fk(&["media_id", "article_id"], "article", &["media_id", "id"]),
            fk(&["user_id"], "user", &["id"]),
        ],
    };
    [user, article, article_user]
        .into_iter()
        .map(|t| {
            t.normalize()
                .expect("junction_over_composite_key normalizes")
        })
        .collect()
}
