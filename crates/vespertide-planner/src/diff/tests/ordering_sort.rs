use super::*;

// Direct unit tests for sort_create_before_add_constraint and compare_actions_for_create_order
mod sort_create_before_add_constraint_tests {
    use super::*;
    use crate::diff::ordering::{
        compare_actions_for_create_order, sort_create_before_add_constraint,
    };
    use std::cmp::Ordering;

    fn make_add_column(table: &str, col: &str) -> MigrationAction {
        MigrationAction::AddColumn {
            table: table.into(),
            column: Box::new(ColumnDef {
                name: col.into(),
                r#type: ColumnType::Simple(SimpleColumnType::Text),
                nullable: true,
                default: None,
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            }),
            fill_with: None,
        }
    }

    fn make_create_table(name: &str) -> MigrationAction {
        MigrationAction::CreateTable {
            table: name.into(),
            columns: vec![],
            constraints: vec![],
        }
    }

    fn make_add_fk(table: &str, ref_table: &str) -> MigrationAction {
        MigrationAction::AddConstraint {
            table: table.into(),
            constraint: TableConstraint::ForeignKey {
                name: None,
                columns: vec!["fk_col".into()],
                ref_table: ref_table.into(),
                ref_columns: vec!["id".into()],
                on_delete: None,
                on_update: None,
                orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
            },
        }
    }

    /// Test line 218: (false, true, _, _) - a is NOT `CreateTable`, b IS `CreateTable`
    /// Direct test of comparison function
    #[test]
    fn test_compare_non_create_vs_create() {
        let created_tables: BTreeSet<String> = ["roles".to_string()].into_iter().collect();

        let add_col = make_add_column("users", "name");
        let create_table = make_create_table("roles");

        // a=AddColumn (non-create), b=CreateTable (create) -> Greater (b comes first)
        let result = compare_actions_for_create_order(&add_col, &create_table, &created_tables);
        assert_eq!(
            result,
            Ordering::Greater,
            "Non-CreateTable vs CreateTable should return Greater"
        );
    }

    /// Test line 216: (true, false, _, _) - a IS `CreateTable`, b is NOT `CreateTable`
    #[test]
    fn test_compare_create_vs_non_create() {
        let created_tables: BTreeSet<String> = ["roles".to_string()].into_iter().collect();

        let create_table = make_create_table("roles");
        let add_col = make_add_column("users", "name");

        // a=CreateTable (create), b=AddColumn (non-create) -> Less (a comes first)
        let result = compare_actions_for_create_order(&create_table, &add_col, &created_tables);
        assert_eq!(
            result,
            Ordering::Less,
            "CreateTable vs Non-CreateTable should return Less"
        );
    }

    /// Test line 214: (true, true, _, _) - both `CreateTable`
    #[test]
    fn test_compare_create_vs_create() {
        let created_tables: BTreeSet<String> = ["roles".to_string(), "categories".to_string()]
            .into_iter()
            .collect();

        let create1 = make_create_table("roles");
        let create2 = make_create_table("categories");

        // Both CreateTable -> Equal (maintain original order)
        let result = compare_actions_for_create_order(&create1, &create2, &created_tables);
        assert_eq!(
            result,
            Ordering::Equal,
            "CreateTable vs CreateTable should return Equal"
        );
    }

    /// Test line 221: (false, false, true, false) - neither `CreateTable`, a refs created, b doesn't
    #[test]
    fn test_compare_refs_vs_non_refs() {
        let created_tables: BTreeSet<String> = ["roles".to_string()].into_iter().collect();

        let add_fk = make_add_fk("users", "roles"); // refs created
        let add_col = make_add_column("posts", "title"); // doesn't ref

        // a refs created, b doesn't -> Greater (a comes after)
        let result = compare_actions_for_create_order(&add_fk, &add_col, &created_tables);
        assert_eq!(
            result,
            Ordering::Greater,
            "FK-ref vs non-ref should return Greater"
        );
    }

    /// Test line 223: (false, false, false, true) - neither `CreateTable`, a doesn't ref, b refs
    #[test]
    fn test_compare_non_refs_vs_refs() {
        let created_tables: BTreeSet<String> = ["roles".to_string()].into_iter().collect();

        let add_col = make_add_column("posts", "title"); // doesn't ref
        let add_fk = make_add_fk("users", "roles"); // refs created

        // a doesn't ref, b refs -> Less (b comes after, a comes first)
        let result = compare_actions_for_create_order(&add_col, &add_fk, &created_tables);
        assert_eq!(
            result,
            Ordering::Less,
            "Non-ref vs FK-ref should return Less"
        );
    }

    /// Test line 225: (false, false, _, _) - neither `CreateTable`, both don't ref
    #[test]
    fn test_compare_non_refs_vs_non_refs() {
        let created_tables: BTreeSet<String> = ["roles".to_string()].into_iter().collect();

        let add_col1 = make_add_column("users", "name");
        let add_col2 = make_add_column("posts", "title");

        // Both don't ref -> Equal
        let result = compare_actions_for_create_order(&add_col1, &add_col2, &created_tables);
        assert_eq!(
            result,
            Ordering::Equal,
            "Non-ref vs non-ref should return Equal"
        );
    }

    /// Test line 225: (false, false, _, _) - neither `CreateTable`, both ref created
    #[test]
    fn test_compare_refs_vs_refs() {
        let created_tables: BTreeSet<String> = ["roles".to_string(), "categories".to_string()]
            .into_iter()
            .collect();

        let add_fk1 = make_add_fk("users", "roles");
        let add_fk2 = make_add_fk("posts", "categories");

        // Both ref -> Equal
        let result = compare_actions_for_create_order(&add_fk1, &add_fk2, &created_tables);
        assert_eq!(
            result,
            Ordering::Equal,
            "FK-ref vs FK-ref should return Equal"
        );
    }

    /// Integration test: sort function works correctly
    #[test]
    fn test_sort_integration() {
        let mut actions = vec![
            make_add_column("t1", "c1"),
            make_add_fk("users", "roles"),
            make_create_table("roles"),
        ];

        sort_create_before_add_constraint(&mut actions);

        // CreateTable first, AddColumn second, AddConstraint FK last
        assert!(matches!(&actions[0], MigrationAction::CreateTable { .. }));
        assert!(matches!(&actions[1], MigrationAction::AddColumn { .. }));
        assert!(matches!(&actions[2], MigrationAction::AddConstraint { .. }));
    }
}

// Direct unit tests for topological_sort_tables (cycle reporting) and
// sort_delete_tables (Kahn's in-degree decrement / enqueue).
mod topo_and_delete_sort_tests {
    use super::*;
    use crate::diff::ordering::{
        extract_delete_table_name, sort_delete_tables, topological_sort_tables,
    };
    use std::collections::BTreeMap;

    fn pk_table(name: &str, fk_to: Option<&str>) -> TableDef {
        let mut constraints = vec![TableConstraint::PrimaryKey {
            auto_increment: false,
            columns: vec!["id".into()],
            strategy: vespertide_core::PrimaryKeyAdditionStrategy::default(),
        }];
        if let Some(target) = fk_to {
            constraints.push(TableConstraint::ForeignKey {
                name: None,
                columns: vec!["id".into()],
                ref_table: target.into(),
                ref_columns: vec!["id".into()],
                on_delete: None,
                on_update: None,
                orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
            });
        }
        table(
            name,
            vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
            constraints,
        )
    }

    fn delete(name: &str) -> MigrationAction {
        MigrationAction::DeleteTable { table: name.into() }
    }

    // A standalone table `a` plus a `b <-> c` FK cycle. The error must list
    // ONLY the cyclic tables (b, c), proving the `!result.iter().any(name ==
    // t.name)` "not-yet-placed" filter. The `delete !` mutant and the `== ->
    // !=` mutant both invert the filter and would list `a` instead.
    #[test]
    fn topological_sort_reports_only_cyclic_tables() {
        let a = pk_table("a", None);
        let b = pk_table("b", Some("c"));
        let c = pk_table("c", Some("b"));
        let err = topological_sort_tables(&[&a, &b, &c]).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("\"b\""), "cyclic b must be listed: {msg}");
        assert!(msg.contains("\"c\""), "cyclic c must be listed: {msg}");
        assert!(
            !msg.contains("\"a\""),
            "standalone a must NOT be listed as cyclic: {msg}"
        );
    }

    // Two FK-dependent deletes given in the WRONG order: parent `p` before
    // child `c` (c FK-> p). sort_delete_tables must reorder so the child is
    // deleted first. Pins:
    //  - `delete_indices.len() <= 1` (a `> 1` mutant returns early, no sort)
    //  - `*degree -= 1` (a `+=`/`/=` mutant never reaches 0, child unsorted)
    //  - `*degree == 0` (a `!=` mutant never enqueues the child)
    #[test]
    fn sort_delete_orders_child_before_parent() {
        let parent = pk_table("p", None);
        let child = pk_table("c", Some("p"));
        let mut all: BTreeMap<&str, &TableDef> = BTreeMap::new();
        all.insert("p", &parent);
        all.insert("c", &child);

        let mut actions = vec![delete("p"), delete("c")];
        sort_delete_tables(&mut actions, &all);

        assert_eq!(
            extract_delete_table_name(&actions[0]),
            "c",
            "child (FK holder) must be deleted first"
        );
        assert_eq!(extract_delete_table_name(&actions[1]), "p");
    }

    // sort_enum_default_dependencies only swaps a ModifyColumnType ahead of a
    // ModifyColumnDefault when the OLD default is one of the REMOVED enum
    // values AND the type change precedes the default change. Here the old
    // default ('a') is NOT removed (only 'c' is), so no swap must happen even
    // though type_idx < default_idx. Pins the `&&` (a `||` mutant would swap
    // on the index condition alone, reordering the actions).
    #[test]
    fn enum_default_reorder_skips_when_old_default_not_removed() {
        use crate::diff::ordering::sort_enum_default_dependencies;
        use vespertide_core::{
            ComplexColumnType, DefaultValue, EnumValues, schema::names::ColumnName,
        };

        let old_enum = ColumnType::Complex(ComplexColumnType::Enum {
            name: "e".into(),
            values: EnumValues::String(vec!["a".into(), "b".into(), "c".into()]),
        });
        let new_enum = ColumnType::Complex(ComplexColumnType::Enum {
            name: "e".into(),
            values: EnumValues::String(vec!["a".into(), "b".into()]),
        });

        let mut from_col = col("status", old_enum);
        from_col.default = Some(DefaultValue::String("'a'".into()));
        let from_table = table("t", vec![from_col], vec![]);
        let mut from_map: BTreeMap<&str, &TableDef> = BTreeMap::new();
        from_map.insert("t", &from_table);

        let mut actions = vec![
            MigrationAction::ModifyColumnType {
                table: "t".into(),
                column: "status".into(),
                new_type: new_enum,
                fill_with: None,
                narrowing_strategy: None,
                timezone: None,
            },
            MigrationAction::ModifyColumnDefault {
                table: "t".into(),
                column: ColumnName::from("status"),
                new_default: Some("'a'".into()),
                backfill: None,
            },
        ];

        sort_enum_default_dependencies(&mut actions, &from_map);

        // Old default 'a' is not a removed value -> order is unchanged.
        assert!(
            matches!(&actions[0], MigrationAction::ModifyColumnType { .. }),
            "type change must stay first when old default is not removed"
        );
        assert!(matches!(
            &actions[1],
            MigrationAction::ModifyColumnDefault { .. }
        ));
    }
}
