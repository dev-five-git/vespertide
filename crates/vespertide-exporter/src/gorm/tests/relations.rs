use super::*;

// -----------------------------------------------------------------------
// Composite (multi-column) FK relation field
// -----------------------------------------------------------------------

fn composite_fk_table() -> TableDef {
    TableDef {
        name: "order_items".into(),
        description: None,
        columns: vec![
            col("order_id", ColumnType::Simple(SimpleColumnType::Integer)),
            col("region_id", ColumnType::Simple(SimpleColumnType::Integer)),
        ],
        constraints: vec![
            TableConstraint::PrimaryKey {
                auto_increment: false,
                columns: vec!["order_id".into(), "region_id".into()],
                strategy: vespertide_core::PrimaryKeyAdditionStrategy::default(),
            },
            TableConstraint::ForeignKey {
                name: None,
                columns: vec!["order_id".into(), "region_id".into()],
                ref_table: "order_regions".into(),
                ref_columns: vec!["order_id".into(), "region_id".into()],
                on_delete: Some(ReferenceAction::Cascade),
                on_update: Some(ReferenceAction::Restrict),
                orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
            },
        ],
    }
}

#[test]
fn test_composite_fk_relation_field() {
    let result = render_entity(&composite_fk_table()).unwrap();
    assert!(
        result.contains(
            "OrderRegions OrderRegions `gorm:\"foreignKey:OrderID,RegionID;references:OrderID,RegionID;constraint:OnDelete:CASCADE,OnUpdate:RESTRICT\" json:\"-\"`"
        ),
        "expected composite FK relation field in GORM output, got:\n{result}"
    );
}

#[test]
fn test_composite_fk_relation_field_name_collision_suffixed() {
    // A column already named "OrderRegions" (Go field name) collides with the
    // natural composite-FK relation field name, forcing a numeric suffix.
    let mut table = composite_fk_table();
    table.columns.push(col(
        "order_regions",
        ColumnType::Simple(SimpleColumnType::Text),
    ));
    let result = render_entity(&table).unwrap();
    assert!(
        result.contains("OrderRegions2 OrderRegions `gorm:\"foreignKey:OrderID,RegionID"),
        "expected suffixed relation field name on collision, got:\n{result}"
    );
}

#[test]
fn test_composite_fk_relation_field_name_double_collision_increments_suffix() {
    // Both "OrderRegions" and "OrderRegions2" are already taken by columns,
    // so the collision loop must advance past its first candidate too.
    let mut table = composite_fk_table();
    table.columns.push(col(
        "order_regions",
        ColumnType::Simple(SimpleColumnType::Text),
    ));
    table.columns.push(col(
        "order_regions2",
        ColumnType::Simple(SimpleColumnType::Text),
    ));
    let result = render_entity(&table).unwrap();
    assert!(
        result.contains("OrderRegions3 OrderRegions `gorm:\"foreignKey:OrderID,RegionID"),
        "expected double-suffixed relation field name on double collision, got:\n{result}"
    );
}

// -----------------------------------------------------------------------
// Self-referencing FK (single table referencing itself, e.g. a tree/
// hierarchy structure: categories.parent_id -> categories.id)
// -----------------------------------------------------------------------

fn self_referencing_table() -> TableDef {
    TableDef {
        name: "categories".into(),
        description: None,
        columns: vec![
            col("id", ColumnType::Simple(SimpleColumnType::Integer)),
            ColumnDef {
                name: "parent_id".into(),
                r#type: ColumnType::Simple(SimpleColumnType::Integer),
                nullable: true,
                default: None,
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            },
        ],
        constraints: vec![
            TableConstraint::PrimaryKey {
                auto_increment: true,
                columns: vec!["id".into()],
                strategy: vespertide_core::PrimaryKeyAdditionStrategy::default(),
            },
            TableConstraint::ForeignKey {
                name: None,
                columns: vec!["parent_id".into()],
                ref_table: "categories".into(),
                ref_columns: vec!["id".into()],
                on_delete: Some(ReferenceAction::SetNull),
                on_update: None,
                orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
            },
        ],
    }
}

#[test]
fn test_self_referencing_fk_forward_relation() {
    let table = self_referencing_table();
    let schema = vec![table.clone()];
    let result = render_entity_with_schema(&table, &schema).unwrap();
    assert!(
        result.contains("Parent *Categories `gorm:\"foreignKey:ParentID"),
        "expected forward self-ref relation field, got:\n{result}"
    );
}

#[test]
fn test_self_referencing_fk_reverse_relation() {
    let table = self_referencing_table();
    let schema = vec![table.clone()];
    let result = render_entity_with_schema(&table, &schema).unwrap();
    assert!(
        result.contains("Children []Categories `gorm:\"foreignKey:ParentID"),
        "expected reverse (has-many) self-ref relation field, got:\n{result}"
    );
}
