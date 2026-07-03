use vespertide_core::{ColumnDef, SimpleColumnType, TableConstraint, TableDef};

use super::{fk, pk, simple, table};

pub(crate) fn schema_scenario(name: &str) -> (TableDef, Vec<TableDef>) {
    use SimpleColumnType::{BigInt, Integer, Text, Uuid};
    match name {
        "many_to_many_article" => article_user_schema("article"),
        "many_to_many_user" => article_user_schema("user"),
        "many_to_many_missing_target" => {
            let article = table_with_named_pk("article", vec![simple("id", BigInt)], &["id"]);
            let article_user = table_with_fk_constraints(
                "article_user",
                vec![simple("article_id", BigInt), simple("user_id", Uuid)],
                &["article_id", "user_id"],
                vec![
                    (vec!["article_id"], "article", vec!["id"]),
                    (vec!["user_id"], "user", vec!["id"]),
                ],
            );
            (article.clone(), vec![article, article_user])
        }
        "many_to_many_multiple_junctions" => {
            let user = table_with_named_pk("user", vec![simple("id", Uuid)], &["id"]);
            let media = table_with_named_pk("media", vec![simple("id", Uuid)], &["id"]);
            let role = user_media_junction("user_media_role");
            let favorite = user_media_junction("user_media_favorite");
            (user.clone(), vec![user, media, role, favorite])
        }
        "composite_fk_parent" => {
            let parent = table_with_named_pk(
                "parent",
                vec![simple("id1", Integer), simple("id2", Integer)],
                &["id1", "id2"],
            );
            let child_one = table_with_fk_constraints(
                "child_one",
                vec![simple("parent_id1", Integer), simple("parent_id2", Integer)],
                &["parent_id1", "parent_id2"],
                vec![(
                    vec!["parent_id1", "parent_id2"],
                    "parent",
                    vec!["id1", "id2"],
                )],
            );
            let child_many = table_with_fk_constraints(
                "child_many",
                vec![
                    simple("id", Integer),
                    simple("parent_id1", Integer),
                    simple("parent_id2", Integer),
                ],
                &["id"],
                vec![(
                    vec!["parent_id1", "parent_id2"],
                    "parent",
                    vec!["id1", "id2"],
                )],
            );
            (parent.clone(), vec![parent, child_one, child_many])
        }
        "not_junction_single_pk" => {
            let other = table_with_named_pk("other", vec![simple("id", Integer)], &["id"]);
            let regular = table_with_fk_constraints(
                "regular",
                vec![simple("id", Integer), simple("other_id", Integer)],
                &["id"],
                vec![(vec!["other_id"], "other", vec!["id"])],
            );
            (other.clone(), vec![other, regular])
        }
        "not_junction_fk_not_in_pk_other" | "not_junction_fk_not_in_pk_another" => {
            let other = table_with_named_pk("other", vec![simple("id", Integer)], &["id"]);
            let another = table_with_named_pk("another", vec![simple("id", Integer)], &["id"]);
            let junction = table_with_fk_constraints(
                "not_junction",
                vec![
                    simple("id", Integer),
                    simple("other_id", Integer),
                    simple("another_id", Integer),
                ],
                &["id", "other_id"],
                vec![
                    (vec!["other_id"], "other", vec!["id"]),
                    (vec!["another_id"], "another", vec!["id"]),
                ],
            );
            let target = if name.ends_with("another") {
                another.clone()
            } else {
                other.clone()
            };
            (target, vec![other, another, junction])
        }
        "multiple_fk_same_table" => {
            let user = table_with_named_pk("user", vec![simple("id", Uuid)], &["id"]);
            let post = table_with_fk_constraints(
                "post",
                vec![
                    simple("id", Uuid),
                    simple("creator_user_id", Uuid),
                    simple("used_by_user_id", Uuid),
                ],
                &["id"],
                vec![
                    (vec!["creator_user_id"], "user", vec!["id"]),
                    (vec!["used_by_user_id"], "user", vec!["id"]),
                ],
            );
            (post.clone(), vec![user, post])
        }
        "username_fk" => {
            let user = table_with_named_pk("user", vec![simple("username", Text)], &["username"]);
            let session = table_with_fk_constraints(
                "session",
                vec![simple("id", Uuid), simple("username", Text)],
                &["id"],
                vec![(vec!["username"], "user", vec!["username"])],
            );
            (session.clone(), vec![user, session])
        }
        "multiple_reverse_relations" => {
            reverse_user_schema("profile", &["preferred_user_id", "backup_user_id"], false)
        }
        "dual_reverse_relations" => reverse_dual_schema(&["username", "checker_username"]),
        "triple_reverse_relations" => {
            reverse_dual_schema(&["username", "checker_username", "other_username"])
        }
        "multiple_has_one_relations" => reverse_user_schema(
            "settings",
            &["created_by_user_id", "updated_by_user_id"],
            true,
        ),
        _ => panic!("unknown schema scenario {name}"),
    }
}

fn table_with_named_pk(name: &str, columns: Vec<ColumnDef>, pk_cols: &[&str]) -> TableDef {
    table(name, columns, vec![pk(pk_cols)])
}

fn table_with_fk_constraints(
    name: &str,
    columns: Vec<ColumnDef>,
    pk_cols: &[&str],
    fks: Vec<(Vec<&str>, &str, Vec<&str>)>,
) -> TableDef {
    let mut constraints = vec![pk(pk_cols)];
    constraints.extend(
        fks.into_iter()
            .map(|(cols, table, refs)| fk(&cols, table, &refs)),
    );
    table(name, columns, constraints)
}

fn article_user_schema(target: &str) -> (TableDef, Vec<TableDef>) {
    let article = table_with_named_pk(
        "article",
        vec![simple("id", SimpleColumnType::BigInt)],
        &["id"],
    );
    let user = table_with_named_pk("user", vec![simple("id", SimpleColumnType::Uuid)], &["id"]);
    let article_user = table_with_fk_constraints(
        "article_user",
        vec![
            simple("article_id", SimpleColumnType::BigInt),
            simple("user_id", SimpleColumnType::Uuid),
        ],
        &["article_id", "user_id"],
        vec![
            (vec!["article_id"], "article", vec!["id"]),
            (vec!["user_id"], "user", vec!["id"]),
        ],
    );
    let selected = if target == "article" {
        article.clone()
    } else {
        user.clone()
    };
    (selected, vec![article, user, article_user])
}

fn user_media_junction(name: &str) -> TableDef {
    table_with_fk_constraints(
        name,
        vec![
            simple("user_id", SimpleColumnType::Uuid),
            simple("media_id", SimpleColumnType::Uuid),
        ],
        &["user_id", "media_id"],
        vec![
            (vec!["user_id"], "user", vec!["id"]),
            (vec!["media_id"], "media", vec!["id"]),
        ],
    )
}

fn reverse_user_schema(name: &str, fk_cols: &[&str], unique: bool) -> (TableDef, Vec<TableDef>) {
    let user = table_with_named_pk("user", vec![simple("id", SimpleColumnType::Uuid)], &["id"]);
    let mut columns = vec![simple("id", SimpleColumnType::Uuid)];
    columns.extend(
        fk_cols
            .iter()
            .map(|col| simple(col, SimpleColumnType::Uuid)),
    );
    let mut table = table_with_fk_constraints(
        name,
        columns,
        &["id"],
        fk_cols
            .iter()
            .map(|col| (vec![*col], "user", vec!["id"]))
            .collect(),
    );
    if unique {
        table
            .constraints
            .extend(fk_cols.iter().map(|col| TableConstraint::Unique {
                name: None,
                columns: vec![(*col).into()],
                strategy: vespertide_core::UniqueConstraintStrategy::DeleteDuplicates {
                    keep: vespertide_core::KeepPolicy::First,
                },
            }));
    }
    (user.clone(), vec![user, table])
}

fn reverse_dual_schema(fk_cols: &[&str]) -> (TableDef, Vec<TableDef>) {
    let dual = table_with_named_pk(
        "dual",
        vec![simple("username", SimpleColumnType::Text)],
        &["username"],
    );
    let rel_name = if fk_cols.len() == 2 {
        "dual_rel"
    } else {
        "triple_rel"
    };
    let columns = fk_cols
        .iter()
        .map(|col| simple(col, SimpleColumnType::Text))
        .collect();
    let rel = table_with_fk_constraints(
        rel_name,
        columns,
        fk_cols,
        fk_cols
            .iter()
            .map(|col| (vec![*col], "dual", vec!["username"]))
            .collect(),
    );
    (dual.clone(), vec![dual, rel])
}
