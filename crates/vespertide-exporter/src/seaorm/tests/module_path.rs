use super::*;
use vespertide_core::{ColumnDef, ColumnType, SimpleColumnType, TableConstraint, TableDef};

fn test_pk_column(name: &str) -> ColumnDef {
    ColumnDef {
        name: name.into(),
        r#type: ColumnType::Simple(SimpleColumnType::Text),
        nullable: false,
        default: None,
        comment: None,
        primary_key: Some(vespertide_core::schema::primary_key::PrimaryKeySyntax::Bool(true)),
        unique: None,
        index: None,
        foreign_key: None,
    }
}

#[test]
fn absolute_module_path_builds_correct_path() {
    let result = absolute_module_path("crate::models", &["admin".into(), "admin".into()]);
    assert_eq!(result, "crate::models::admin::admin");
}

#[test]
fn absolute_module_path_single_segment() {
    let result = absolute_module_path("crate::models", &["user".into()]);
    assert_eq!(result, "crate::models::user");
}

#[test]
fn absolute_module_path_deep_nesting() {
    let result = absolute_module_path(
        "crate::db::entities",
        &["company".into(), "division".into(), "department".into()],
    );
    assert_eq!(result, "crate::db::entities::company::division::department");
}

#[test]
fn resolve_relation_entity_module_path_with_crate_prefix() {
    let mut module_paths = HashMap::new();
    module_paths.insert(
        "estimate".into(),
        vec!["estimate".into(), "estimate".into()],
    );
    module_paths.insert("admin".into(), vec!["admin".into(), "admin".into()]);
    let result =
        resolve_relation_entity_module_path("estimate", "admin", &module_paths, "crate::models");
    assert_eq!(result, "crate::models::admin::admin");
}

#[test]
fn resolve_relation_entity_module_path_prefers_super_for_siblings() {
    let mut module_paths = HashMap::new();
    module_paths.insert("admin".into(), vec!["admin".into(), "admin".into()]);
    module_paths.insert(
        "admin_stamp".into(),
        vec!["admin".into(), "admin_stamp".into()],
    );

    let result =
        resolve_relation_entity_module_path("admin_stamp", "admin", &module_paths, "crate::models");
    assert_eq!(result, "super::admin");
}

#[test]
fn resolve_relation_entity_module_path_fallback_when_empty_prefix() {
    let mut module_paths = HashMap::new();
    module_paths.insert("admin".into(), vec!["admin".into(), "admin".into()]);
    let result = resolve_relation_entity_module_path("user", "admin", &module_paths, "");
    assert_eq!(result, "super::admin");
}

#[test]
fn resolve_relation_entity_module_path_uses_crate_for_cross_directory_nested_models() {
    let mut module_paths = HashMap::new();
    module_paths.insert("admin".into(), vec!["admin".into(), "admin".into()]);
    module_paths.insert(
        "estimate".into(),
        vec!["estimate".into(), "estimate".into()],
    );

    let result =
        resolve_relation_entity_module_path("admin", "estimate", &module_paths, "crate::models");
    assert_eq!(result, "crate::models::estimate::estimate");
}

#[test]
fn resolve_relation_entity_module_path_uses_super_for_same_directory() {
    let mut module_paths = HashMap::new();
    module_paths.insert("admin".into(), vec!["shared".into(), "admin".into()]);
    module_paths.insert(
        "admin_stamp".into(),
        vec!["shared".into(), "admin_stamp".into()],
    );
    let result =
        resolve_relation_entity_module_path("admin", "admin_stamp", &module_paths, "crate::models");
    assert_eq!(result, "super::admin_stamp");
}

#[test]
fn resolve_relation_entity_module_path_fallback_super_when_empty_prefix_cross_directory() {
    let mut module_paths = HashMap::new();
    module_paths.insert("admin".into(), vec!["admin".into(), "admin".into()]);
    module_paths.insert(
        "estimate".into(),
        vec!["estimate".into(), "estimate".into()],
    );
    let result = resolve_relation_entity_module_path("admin", "estimate", &module_paths, "");
    assert_eq!(result, "super::estimate");
}

#[test]
fn resolve_relation_entity_module_path_uses_crate_prefix_when_not_in_module_paths() {
    let module_paths = HashMap::new();
    let result =
        resolve_relation_entity_module_path("admin", "estimate", &module_paths, "crate::models");
    assert_eq!(result, "crate::models::estimate");
}

#[test]
fn resolve_self_ref_link_module_path_uses_super_for_same_directory() {
    let mut module_paths = HashMap::new();
    module_paths.insert("admin".into(), vec!["shared".into(), "admin".into()]);
    module_paths.insert(
        "admin_friendship".into(),
        vec!["shared".into(), "admin_friendship".into()],
    );
    let result = resolve_self_ref_link_module_path(
        "admin",
        "admin_friendship",
        &module_paths,
        "crate::models",
    );
    assert_eq!(result, "super::admin_friendship");
}

#[test]
fn resolve_self_ref_link_module_path_absolute_fallback_when_empty_prefix() {
    let mut module_paths = HashMap::new();
    module_paths.insert("admin".into(), vec!["admin".into(), "admin".into()]);
    module_paths.insert(
        "admin_friendship".into(),
        vec!["social".into(), "admin_friendship".into()],
    );
    let result = resolve_self_ref_link_module_path("admin", "admin_friendship", &module_paths, "");
    assert_eq!(result, "crate::models::social::admin_friendship");
}

#[test]
fn self_ref_link_helpers_use_crate_path_for_cross_directory_junctions() {
    let admin = TableDef {
        name: "admin".into(),
        description: None,
        columns: vec![test_pk_column("username")],
        constraints: vec![],
    };

    let estimate_user_checker_setting = TableDef {
        name: "estimate_user_checker_setting".into(),
        description: None,
        columns: vec![
            test_pk_column("username"),
            test_pk_column("checker_username"),
        ],
        constraints: vec![
            TableConstraint::ForeignKey {
                name: None,
                columns: vec!["username".into()],
                ref_table: "admin".into(),
                ref_columns: vec!["username".into()],
                on_delete: None,
                on_update: None,
                orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
            },
            TableConstraint::ForeignKey {
                name: None,
                columns: vec!["checker_username".into()],
                ref_table: "admin".into(),
                ref_columns: vec!["username".into()],
                on_delete: None,
                on_update: None,
                orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
            },
        ],
    };

    let schema = vec![admin.clone(), estimate_user_checker_setting];
    let mut module_paths = HashMap::new();
    module_paths.insert("admin".into(), vec!["admin".into(), "admin".into()]);
    module_paths.insert(
        "estimate_user_checker_setting".into(),
        vec!["estimate".into(), "estimate_user_checker_setting".into()],
    );

    let rendered = render_entity_with_config_and_paths(
        &admin,
        &schema,
        &SeaOrmConfig::default(),
        "",
        &module_paths,
        "crate::models",
    );

    assert!(rendered.contains(
        "crate::models::estimate::estimate_user_checker_setting::Relation::Username.def().rev()"
    ));
    assert!(rendered.contains(
        "crate::models::estimate::estimate_user_checker_setting::Relation::CheckerUsername.def()"
    ));
}
