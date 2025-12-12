use schemars::JsonSchema;

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::schema::{
    StrOrBoolOrArray, column::ColumnDef, constraint::TableConstraint, index::IndexDef, names::TableName,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct TableDef {
    pub name: TableName,
    pub columns: Vec<ColumnDef>,
    pub constraints: Vec<TableConstraint>,
    pub indexes: Vec<IndexDef>,
}

impl TableDef {
    /// Normalizes inline column constraints (primary_key, unique, index, foreign_key)
    /// into table-level constraints and indexes.
    /// Returns a new TableDef with all inline constraints converted to table-level.
    pub fn normalize(&self) -> Self {
        let mut constraints = self.constraints.clone();
        let mut indexes = self.indexes.clone();

        // Collect columns with inline primary_key
        let pk_columns: Vec<String> = self
            .columns
            .iter()
            .filter(|c| c.primary_key == Some(true))
            .map(|c| c.name.clone())
            .collect();

        // Add primary key constraint if any columns have inline pk and no existing pk constraint
        if !pk_columns.is_empty() {
            let has_pk = constraints
                .iter()
                .any(|c| matches!(c, TableConstraint::PrimaryKey { .. }));
            if !has_pk {
                constraints.push(TableConstraint::PrimaryKey {
                    columns: pk_columns,
                });
            }
        }

        // Process inline unique and index for each column
        for col in &self.columns {
            // Handle inline unique
            if let Some(ref unique_val) = col.unique {
                match unique_val {
                    StrOrBoolOrArray::Str(name) => {
                        let constraint_name = Some(name.clone());
                        
                        // Check if this unique constraint already exists
                        let exists = constraints.iter().any(|c| {
                            if let TableConstraint::Unique { name: c_name, columns } = c {
                                c_name.as_ref() == Some(name) && columns.len() == 1 && columns[0] == col.name
                            } else {
                                false
                            }
                        });

                        if !exists {
                            constraints.push(TableConstraint::Unique {
                                name: constraint_name,
                                columns: vec![col.name.clone()],
                            });
                        }
                    }
                    StrOrBoolOrArray::Bool(true) => {
                        let exists = constraints.iter().any(|c| {
                            if let TableConstraint::Unique { name: None, columns } = c {
                                columns.len() == 1 && columns[0] == col.name
                            } else {
                                false
                            }
                        });

                        if !exists {
                            constraints.push(TableConstraint::Unique {
                                name: None,
                                columns: vec![col.name.clone()],
                            });
                        }
                    }
                    StrOrBoolOrArray::Bool(false) => continue,
                    StrOrBoolOrArray::Array(names) => {
                        // Array format: each element is a constraint name
                        // This column will be part of all these named constraints
                        for constraint_name in names {
                            // Check if constraint with this name already exists
                            if let Some(existing) = constraints.iter_mut().find(|c| {
                                if let TableConstraint::Unique { name: Some(n), .. } = c {
                                    n == constraint_name
                                } else {
                                    false
                                }
                            }) {
                                // Add this column to existing composite constraint
                                if let TableConstraint::Unique { columns, .. } = existing {
                                    if !columns.contains(&col.name) {
                                        columns.push(col.name.clone());
                                    }
                                }
                            } else {
                                // Create new constraint with this column
                                constraints.push(TableConstraint::Unique {
                                    name: Some(constraint_name.clone()),
                                    columns: vec![col.name.clone()],
                                });
                            }
                        }
                    }
                }
            }

            // Handle inline foreign_key
            if let Some(ref fk) = col.foreign_key {
                // Check if this foreign key already exists
                let exists = constraints.iter().any(|c| {
                    if let TableConstraint::ForeignKey { columns, .. } = c {
                        columns.len() == 1 && columns[0] == col.name
                    } else {
                        false
                    }
                });

                if !exists {
                    constraints.push(TableConstraint::ForeignKey {
                        name: None,
                        columns: vec![col.name.clone()],
                        ref_table: fk.ref_table.clone(),
                        ref_columns: fk.ref_columns.clone(),
                        on_delete: fk.on_delete.clone(),
                        on_update: fk.on_update.clone(),
                    });
                }
            }
        }

        // Group columns by index name to create composite indexes
        // Use a HashMap to group, but preserve column order by tracking first occurrence
        let mut index_groups: HashMap<String, Vec<String>> = HashMap::new();
        let mut index_order: Vec<String> = Vec::new(); // Preserve order of first occurrence

        for col in &self.columns {
            if let Some(ref index_val) = col.index {
                match index_val {
                    StrOrBoolOrArray::Str(name) => {
                        // Named index - group by name
                        let index_name = name.clone();
                        
                        if !index_groups.contains_key(&index_name) {
                            index_order.push(index_name.clone());
                        }
                        
                        index_groups
                            .entry(index_name)
                            .or_default()
                            .push(col.name.clone());
                    }
                    StrOrBoolOrArray::Bool(true) => {
                        // Auto-generated index name
                        let index_name = format!("idx_{}_{}", self.name, col.name);
                        
                        if !index_groups.contains_key(&index_name) {
                            index_order.push(index_name.clone());
                        }
                        
                        index_groups
                            .entry(index_name)
                            .or_default()
                            .push(col.name.clone());
                    }
                    StrOrBoolOrArray::Bool(false) => continue,
                    StrOrBoolOrArray::Array(names) => {
                        // Array format: each element is an index name
                        // This column will be part of all these named indexes
                        for index_name in names {
                            if !index_groups.contains_key(index_name.as_str()) {
                                index_order.push(index_name.clone());
                            }
                            
                            index_groups
                                .entry(index_name.clone())
                                .or_default()
                                .push(col.name.clone());
                        }
                    }
                }
            }
        }

        // Create indexes from grouped columns in order
        for index_name in index_order {
            let columns = index_groups.get(&index_name).unwrap().clone();

            // Check if this index already exists (by name only, not by column match)
            // Multiple indexes can have the same columns but different names
            let exists = indexes
                .iter()
                .any(|i| i.name == index_name);

            if !exists {
                indexes.push(IndexDef {
                    name: index_name,
                    columns,
                    unique: false,
                });
            }
        }

        TableDef {
            name: self.name.clone(),
            columns: self.columns.clone(),
            constraints,
            indexes,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::column::ColumnType;
    use crate::schema::foreign_key::ForeignKeyDef;
    use crate::schema::reference::ReferenceAction;
    use crate::schema::str_or_bool::StrOrBoolOrArray;

    fn col(name: &str, ty: ColumnType) -> ColumnDef {
        ColumnDef {
            name: name.to_string(),
            r#type: ty,
            nullable: true,
            default: None,
            comment: None,
            primary_key: None,
            unique: None,
            index: None,
            foreign_key: None,
        }
    }

    #[test]
    fn normalize_inline_primary_key() {
        let mut id_col = col("id", ColumnType::Integer);
        id_col.primary_key = Some(true);

        let table = TableDef {
            name: "users".into(),
            columns: vec![id_col, col("name", ColumnType::Text)],
            constraints: vec![],
            indexes: vec![],
        };

        let normalized = table.normalize();
        assert_eq!(normalized.constraints.len(), 1);
        assert!(matches!(
            &normalized.constraints[0],
            TableConstraint::PrimaryKey { columns } if columns == &["id".to_string()]
        ));
    }

    #[test]
    fn normalize_multiple_inline_primary_keys() {
        let mut id_col = col("id", ColumnType::Integer);
        id_col.primary_key = Some(true);

        let mut tenant_col = col("tenant_id", ColumnType::Integer);
        tenant_col.primary_key = Some(true);

        let table = TableDef {
            name: "users".into(),
            columns: vec![id_col, tenant_col],
            constraints: vec![],
            indexes: vec![],
        };

        let normalized = table.normalize();
        assert_eq!(normalized.constraints.len(), 1);
        assert!(matches!(
            &normalized.constraints[0],
            TableConstraint::PrimaryKey { columns } if columns == &["id".to_string(), "tenant_id".to_string()]
        ));
    }

    #[test]
    fn normalize_does_not_duplicate_existing_pk() {
        let mut id_col = col("id", ColumnType::Integer);
        id_col.primary_key = Some(true);

        let table = TableDef {
            name: "users".into(),
            columns: vec![id_col],
            constraints: vec![TableConstraint::PrimaryKey {
                columns: vec!["id".into()],
            }],
            indexes: vec![],
        };

        let normalized = table.normalize();
        assert_eq!(normalized.constraints.len(), 1);
    }

    #[test]
    fn normalize_inline_unique_bool() {
        let mut email_col = col("email", ColumnType::Text);
        email_col.unique = Some(StrOrBoolOrArray::Bool(true));

        let table = TableDef {
            name: "users".into(),
            columns: vec![col("id", ColumnType::Integer), email_col],
            constraints: vec![],
            indexes: vec![],
        };

        let normalized = table.normalize();
        assert_eq!(normalized.constraints.len(), 1);
        assert!(matches!(
            &normalized.constraints[0],
            TableConstraint::Unique { name: None, columns } if columns == &["email".to_string()]
        ));
    }

    #[test]
    fn normalize_inline_unique_with_name() {
        let mut email_col = col("email", ColumnType::Text);
        email_col.unique = Some(StrOrBoolOrArray::Str("uq_users_email".into()));

        let table = TableDef {
            name: "users".into(),
            columns: vec![col("id", ColumnType::Integer), email_col],
            constraints: vec![],
            indexes: vec![],
        };

        let normalized = table.normalize();
        assert_eq!(normalized.constraints.len(), 1);
        assert!(matches!(
            &normalized.constraints[0],
            TableConstraint::Unique { name: Some(n), columns }
                if n == "uq_users_email" && columns == &["email".to_string()]
        ));
    }

    #[test]
    fn normalize_inline_index_bool() {
        let mut name_col = col("name", ColumnType::Text);
        name_col.index = Some(StrOrBoolOrArray::Bool(true));

        let table = TableDef {
            name: "users".into(),
            columns: vec![col("id", ColumnType::Integer), name_col],
            constraints: vec![],
            indexes: vec![],
        };

        let normalized = table.normalize();
        assert_eq!(normalized.indexes.len(), 1);
        assert_eq!(normalized.indexes[0].name, "idx_users_name");
        assert_eq!(normalized.indexes[0].columns, vec!["name".to_string()]);
        assert!(!normalized.indexes[0].unique);
    }

    #[test]
    fn normalize_inline_index_with_name() {
        let mut name_col = col("name", ColumnType::Text);
        name_col.index = Some(StrOrBoolOrArray::Str("custom_idx_name".into()));

        let table = TableDef {
            name: "users".into(),
            columns: vec![col("id", ColumnType::Integer), name_col],
            constraints: vec![],
            indexes: vec![],
        };

        let normalized = table.normalize();
        assert_eq!(normalized.indexes.len(), 1);
        assert_eq!(normalized.indexes[0].name, "custom_idx_name");
    }

    #[test]
    fn normalize_inline_foreign_key() {
        let mut user_id_col = col("user_id", ColumnType::Integer);
        user_id_col.foreign_key = Some(ForeignKeyDef {
            ref_table: "users".into(),
            ref_columns: vec!["id".into()],
            on_delete: Some(ReferenceAction::Cascade),
            on_update: None,
        });

        let table = TableDef {
            name: "posts".into(),
            columns: vec![col("id", ColumnType::Integer), user_id_col],
            constraints: vec![],
            indexes: vec![],
        };

        let normalized = table.normalize();
        assert_eq!(normalized.constraints.len(), 1);
        assert!(matches!(
            &normalized.constraints[0],
            TableConstraint::ForeignKey {
                name: None,
                columns,
                ref_table,
                ref_columns,
                on_delete: Some(ReferenceAction::Cascade),
                on_update: None,
            } if columns == &["user_id".to_string()]
                && ref_table == "users"
                && ref_columns == &["id".to_string()]
        ));
    }

    #[test]
    fn normalize_all_inline_constraints() {
        let mut id_col = col("id", ColumnType::Integer);
        id_col.primary_key = Some(true);

        let mut email_col = col("email", ColumnType::Text);
        email_col.unique = Some(StrOrBoolOrArray::Bool(true));

        let mut name_col = col("name", ColumnType::Text);
        name_col.index = Some(StrOrBoolOrArray::Bool(true));

        let mut user_id_col = col("org_id", ColumnType::Integer);
        user_id_col.foreign_key = Some(ForeignKeyDef {
            ref_table: "orgs".into(),
            ref_columns: vec!["id".into()],
            on_delete: None,
            on_update: None,
        });

        let table = TableDef {
            name: "users".into(),
            columns: vec![id_col, email_col, name_col, user_id_col],
            constraints: vec![],
            indexes: vec![],
        };

        let normalized = table.normalize();
        // Should have: PrimaryKey, Unique, ForeignKey
        assert_eq!(normalized.constraints.len(), 3);
        // Should have: 1 index
        assert_eq!(normalized.indexes.len(), 1);
    }

    #[test]
    fn normalize_composite_index_from_string_name() {
        let mut updated_at_col = col("updated_at", ColumnType::Timestamp);
        updated_at_col.index = Some(StrOrBoolOrArray::Str("tuple".into()));

        let mut user_id_col = col("user_id", ColumnType::Integer);
        user_id_col.index = Some(StrOrBoolOrArray::Str("tuple".into()));

        let table = TableDef {
            name: "post".into(),
            columns: vec![col("id", ColumnType::Integer), updated_at_col, user_id_col],
            constraints: vec![],
            indexes: vec![],
        };

        let normalized = table.normalize();
        assert_eq!(normalized.indexes.len(), 1);
        assert_eq!(normalized.indexes[0].name, "tuple");
        assert_eq!(
            normalized.indexes[0].columns,
            vec!["updated_at".to_string(), "user_id".to_string()]
        );
        assert!(!normalized.indexes[0].unique);
    }

    #[test]
    fn normalize_multiple_different_indexes() {
        let mut col1 = col("col1", ColumnType::Text);
        col1.index = Some(StrOrBoolOrArray::Str("idx_a".into()));

        let mut col2 = col("col2", ColumnType::Text);
        col2.index = Some(StrOrBoolOrArray::Str("idx_a".into()));

        let mut col3 = col("col3", ColumnType::Text);
        col3.index = Some(StrOrBoolOrArray::Str("idx_b".into()));

        let mut col4 = col("col4", ColumnType::Text);
        col4.index = Some(StrOrBoolOrArray::Bool(true));

        let table = TableDef {
            name: "test".into(),
            columns: vec![col("id", ColumnType::Integer), col1, col2, col3, col4],
            constraints: vec![],
            indexes: vec![],
        };

        let normalized = table.normalize();
        assert_eq!(normalized.indexes.len(), 3);

        // Check idx_a composite index
        let idx_a = normalized
            .indexes
            .iter()
            .find(|i| i.name == "idx_a")
            .unwrap();
        assert_eq!(idx_a.columns, vec!["col1".to_string(), "col2".to_string()]);

        // Check idx_b single column index
        let idx_b = normalized
            .indexes
            .iter()
            .find(|i| i.name == "idx_b")
            .unwrap();
        assert_eq!(idx_b.columns, vec!["col3".to_string()]);

        // Check auto-generated index for col4
        let idx_col4 = normalized
            .indexes
            .iter()
            .find(|i| i.name == "idx_test_col4")
            .unwrap();
        assert_eq!(idx_col4.columns, vec!["col4".to_string()]);
    }

    #[test]
    fn normalize_false_values_are_ignored() {
        let mut email_col = col("email", ColumnType::Text);
        email_col.unique = Some(StrOrBoolOrArray::Bool(false));
        email_col.index = Some(StrOrBoolOrArray::Bool(false));

        let table = TableDef {
            name: "users".into(),
            columns: vec![col("id", ColumnType::Integer), email_col],
            constraints: vec![],
            indexes: vec![],
        };

        let normalized = table.normalize();
        assert_eq!(normalized.constraints.len(), 0);
        assert_eq!(normalized.indexes.len(), 0);
    }

    #[test]
    fn normalize_multiple_indexes_from_same_array() {
        // Multiple columns with same array of index names should create multiple composite indexes
        let mut updated_at_col = col("updated_at", ColumnType::Timestamp);
        updated_at_col.index = Some(StrOrBoolOrArray::Array(vec!["tuple".into(), "tuple2".into()]));

        let mut user_id_col = col("user_id", ColumnType::Integer);
        user_id_col.index = Some(StrOrBoolOrArray::Array(vec!["tuple".into(), "tuple2".into()]));

        let table = TableDef {
            name: "post".into(),
            columns: vec![col("id", ColumnType::Integer), updated_at_col, user_id_col],
            constraints: vec![],
            indexes: vec![],
        };

        let normalized = table.normalize();
        // Should have: tuple (composite: updated_at, user_id), tuple2 (composite: updated_at, user_id)
        assert_eq!(normalized.indexes.len(), 2);
        
        let tuple_idx = normalized.indexes.iter().find(|i| i.name == "tuple").unwrap();
        let mut sorted_cols = tuple_idx.columns.clone();
        sorted_cols.sort();
        assert_eq!(sorted_cols, vec!["updated_at".to_string(), "user_id".to_string()]);
        
        let tuple2_idx = normalized.indexes.iter().find(|i| i.name == "tuple2").unwrap();
        let mut sorted_cols2 = tuple2_idx.columns.clone();
        sorted_cols2.sort();
        assert_eq!(sorted_cols2, vec!["updated_at".to_string(), "user_id".to_string()]);
    }
}
