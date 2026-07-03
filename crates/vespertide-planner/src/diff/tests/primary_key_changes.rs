use super::*;
use crate::test_support::pk;

#[test]
fn add_column_to_composite_pk() {
    // Primary key: [id] -> [id, tenant_id]
    let from = vec![table(
        "users",
        vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            col("tenant_id", ColumnType::Simple(SimpleColumnType::Integer)),
        ],
        vec![pk(vec!["id"])],
    )];

    let to = vec![table(
        "users",
        vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            col("tenant_id", ColumnType::Simple(SimpleColumnType::Integer)),
        ],
        vec![pk(vec!["id", "tenant_id"])],
    )];

    let plan = diff_schemas(&from, &to).unwrap();

    // Should replace PK with new composite PK
    assert_eq!(plan.actions.len(), 1);

    assert!(matches!(
        &plan.actions[0],
        MigrationAction::ReplaceConstraint {
            table,
            from: TableConstraint::PrimaryKey { columns: from_cols, .. },
            to: TableConstraint::PrimaryKey { columns: to_cols, .. },
        } if table == "users"
          && from_cols == &vec!["id".to_string()]
          && to_cols == &vec!["id".to_string(), "tenant_id".to_string()]
    ));
}

#[test]
fn remove_column_from_composite_pk() {
    // Primary key: [id, tenant_id] -> [id]
    let from = vec![table(
        "users",
        vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            col("tenant_id", ColumnType::Simple(SimpleColumnType::Integer)),
        ],
        vec![pk(vec!["id", "tenant_id"])],
    )];

    let to = vec![table(
        "users",
        vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            col("tenant_id", ColumnType::Simple(SimpleColumnType::Integer)),
        ],
        vec![pk(vec!["id"])],
    )];

    let plan = diff_schemas(&from, &to).unwrap();

    // Should replace composite PK with single-column PK
    assert_eq!(plan.actions.len(), 1);

    assert!(matches!(
        &plan.actions[0],
        MigrationAction::ReplaceConstraint {
            table,
            from: TableConstraint::PrimaryKey { columns: from_cols, .. },
            to: TableConstraint::PrimaryKey { columns: to_cols, .. },
        } if table == "users"
          && from_cols == &vec!["id".to_string(), "tenant_id".to_string()]
          && to_cols == &vec!["id".to_string()]
    ));
}

#[test]
fn change_pk_columns_entirely() {
    // Primary key: [id] -> [uuid]
    let from = vec![table(
        "users",
        vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            col("uuid", ColumnType::Simple(SimpleColumnType::Text)),
        ],
        vec![pk(vec!["id"])],
    )];

    let to = vec![table(
        "users",
        vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            col("uuid", ColumnType::Simple(SimpleColumnType::Text)),
        ],
        vec![pk(vec!["uuid"])],
    )];

    let plan = diff_schemas(&from, &to).unwrap();

    assert_eq!(plan.actions.len(), 1);

    assert!(matches!(
        &plan.actions[0],
        MigrationAction::ReplaceConstraint {
            table,
            from: TableConstraint::PrimaryKey { columns: from_cols, .. },
            to: TableConstraint::PrimaryKey { columns: to_cols, .. },
        } if table == "users"
          && from_cols == &vec!["id".to_string()]
          && to_cols == &vec!["uuid".to_string()]
    ));
}

#[test]
fn add_multiple_columns_to_composite_pk() {
    // Primary key: [id] -> [id, tenant_id, region_id]
    let from = vec![table(
        "users",
        vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            col("tenant_id", ColumnType::Simple(SimpleColumnType::Integer)),
            col("region_id", ColumnType::Simple(SimpleColumnType::Integer)),
        ],
        vec![pk(vec!["id"])],
    )];

    let to = vec![table(
        "users",
        vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            col("tenant_id", ColumnType::Simple(SimpleColumnType::Integer)),
            col("region_id", ColumnType::Simple(SimpleColumnType::Integer)),
        ],
        vec![pk(vec!["id", "tenant_id", "region_id"])],
    )];

    let plan = diff_schemas(&from, &to).unwrap();

    assert_eq!(plan.actions.len(), 1);

    assert!(matches!(
        &plan.actions[0],
        MigrationAction::ReplaceConstraint {
            table,
            from: TableConstraint::PrimaryKey { columns: from_cols, .. },
            to: TableConstraint::PrimaryKey { columns: to_cols, .. },
        } if table == "users"
          && from_cols == &vec!["id".to_string()]
          && to_cols == &vec![
              "id".to_string(),
              "tenant_id".to_string(),
              "region_id".to_string()
          ]
    ));
}

#[test]
fn remove_multiple_columns_from_composite_pk() {
    // Primary key: [id, tenant_id, region_id] -> [id]
    let from = vec![table(
        "users",
        vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            col("tenant_id", ColumnType::Simple(SimpleColumnType::Integer)),
            col("region_id", ColumnType::Simple(SimpleColumnType::Integer)),
        ],
        vec![pk(vec!["id", "tenant_id", "region_id"])],
    )];

    let to = vec![table(
        "users",
        vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            col("tenant_id", ColumnType::Simple(SimpleColumnType::Integer)),
            col("region_id", ColumnType::Simple(SimpleColumnType::Integer)),
        ],
        vec![pk(vec!["id"])],
    )];

    let plan = diff_schemas(&from, &to).unwrap();

    assert_eq!(plan.actions.len(), 1);

    assert!(matches!(
        &plan.actions[0],
        MigrationAction::ReplaceConstraint {
            table,
            from: TableConstraint::PrimaryKey { columns: from_cols, .. },
            to: TableConstraint::PrimaryKey { columns: to_cols, .. },
        } if table == "users"
          && from_cols == &vec![
              "id".to_string(),
              "tenant_id".to_string(),
              "region_id".to_string()
          ]
          && to_cols == &vec!["id".to_string()]
    ));
}

#[test]
fn change_composite_pk_columns_partially() {
    // Primary key: [id, tenant_id] -> [id, region_id]
    // One column kept, one removed, one added
    let from = vec![table(
        "users",
        vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            col("tenant_id", ColumnType::Simple(SimpleColumnType::Integer)),
            col("region_id", ColumnType::Simple(SimpleColumnType::Integer)),
        ],
        vec![pk(vec!["id", "tenant_id"])],
    )];

    let to = vec![table(
        "users",
        vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            col("tenant_id", ColumnType::Simple(SimpleColumnType::Integer)),
            col("region_id", ColumnType::Simple(SimpleColumnType::Integer)),
        ],
        vec![pk(vec!["id", "region_id"])],
    )];

    let plan = diff_schemas(&from, &to).unwrap();

    assert_eq!(plan.actions.len(), 1);

    assert!(matches!(
        &plan.actions[0],
        MigrationAction::ReplaceConstraint {
            table,
            from: TableConstraint::PrimaryKey { columns: from_cols, .. },
            to: TableConstraint::PrimaryKey { columns: to_cols, .. },
        } if table == "users"
          && from_cols == &vec!["id".to_string(), "tenant_id".to_string()]
          && to_cols == &vec!["id".to_string(), "region_id".to_string()]
    ));
}
