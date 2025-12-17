use schemars::JsonSchema;

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

use crate::schema::{
    StrOrBoolOrArray, column::ColumnDef, constraint::TableConstraint,
    foreign_key::ForeignKeySyntax, index::IndexDef, names::TableName,
    primary_key::PrimaryKeySyntax,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TableValidationError {
    DuplicateIndexColumn {
        index_name: String,
        column_name: String,
    },
    InvalidForeignKeyFormat {
        column_name: String,
        value: String,
    },
}

impl std::fmt::Display for TableValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TableValidationError::DuplicateIndexColumn {
                index_name,
                column_name,
            } => {
                write!(
                    f,
                    "Duplicate index '{}' on column '{}': the same index name cannot be applied to the same column multiple times",
                    index_name, column_name
                )
            }
            TableValidationError::InvalidForeignKeyFormat { column_name, value } => {
                write!(
                    f,
                    "Invalid foreign key format '{}' on column '{}': expected 'table.column' format",
                    value, column_name
                )
            }
        }
    }
}

impl std::error::Error for TableValidationError {}

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
    ///
    /// # Errors
    ///
    /// Returns an error if the same index name is applied to the same column multiple times.
    pub fn normalize(&self) -> Result<Self, TableValidationError> {
        let mut constraints = self.constraints.clone();
        let mut indexes = self.indexes.clone();

        // Collect columns with inline primary_key and check for auto_increment
        let mut pk_columns: Vec<String> = Vec::new();
        let mut pk_auto_increment = false;

        for col in &self.columns {
            if let Some(ref pk) = col.primary_key {
                match pk {
                    PrimaryKeySyntax::Bool(true) => {
                        pk_columns.push(col.name.clone());
                    }
                    PrimaryKeySyntax::Bool(false) => {}
                    PrimaryKeySyntax::Object(pk_def) => {
                        pk_columns.push(col.name.clone());
                        if pk_def.auto_increment {
                            pk_auto_increment = true;
                        }
                    }
                }
            }
        }

        // Add primary key constraint if any columns have inline pk and no existing pk constraint.
        if !pk_columns.is_empty() {
            let has_pk_constraint = constraints
                .iter()
                .any(|c| matches!(c, TableConstraint::PrimaryKey { .. }));

            if !has_pk_constraint {
                constraints.push(TableConstraint::PrimaryKey {
                    auto_increment: pk_auto_increment,
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
                            if let TableConstraint::Unique {
                                name: c_name,
                                columns,
                            } = c
                            {
                                c_name.as_ref() == Some(name)
                                    && columns.len() == 1
                                    && columns[0] == col.name
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
                            if let TableConstraint::Unique {
                                name: None,
                                columns,
                            } = c
                            {
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
                                if let TableConstraint::Unique { columns, .. } = existing
                                    && !columns.contains(&col.name)
                                {
                                    columns.push(col.name.clone());
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
            if let Some(ref fk_syntax) = col.foreign_key {
                // Convert ForeignKeySyntax to ForeignKeyDef
                let (ref_table, ref_columns, on_delete, on_update) = match fk_syntax {
                    ForeignKeySyntax::String(s) => {
                        // Parse "table.column" format
                        let parts: Vec<&str> = s.split('.').collect();
                        if parts.len() != 2 || parts[0].is_empty() || parts[1].is_empty() {
                            return Err(TableValidationError::InvalidForeignKeyFormat {
                                column_name: col.name.clone(),
                                value: s.clone(),
                            });
                        }
                        (parts[0].to_string(), vec![parts[1].to_string()], None, None)
                    }
                    ForeignKeySyntax::Object(fk_def) => (
                        fk_def.ref_table.clone(),
                        fk_def.ref_columns.clone(),
                        fk_def.on_delete.clone(),
                        fk_def.on_update.clone(),
                    ),
                };

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
                        ref_table,
                        ref_columns,
                        on_delete,
                        on_update,
                    });
                }
            }
        }

        // Group columns by index name to create composite indexes
        // Use a HashMap to group, but preserve column order by tracking first occurrence
        let mut index_groups: HashMap<String, Vec<String>> = HashMap::new();
        let mut index_order: Vec<String> = Vec::new(); // Preserve order of first occurrence
        // Track which columns are already in each index from inline definitions to detect duplicates
        // Only track inline definitions, not existing table-level indexes (they can be extended)
        let mut inline_index_column_tracker: HashMap<String, HashSet<String>> = HashMap::new();

        for col in &self.columns {
            if let Some(ref index_val) = col.index {
                match index_val {
                    StrOrBoolOrArray::Str(name) => {
                        // Named index - group by name
                        let index_name = name.clone();

                        // Check for duplicate - only check inline definitions, not existing table-level indexes
                        if let Some(columns) = inline_index_column_tracker.get(name.as_str())
                            && columns.contains(col.name.as_str())
                        {
                            return Err(TableValidationError::DuplicateIndexColumn {
                                index_name: name.clone(),
                                column_name: col.name.clone(),
                            });
                        }

                        if !index_groups.contains_key(&index_name) {
                            index_order.push(index_name.clone());
                        }

                        index_groups
                            .entry(index_name.clone())
                            .or_default()
                            .push(col.name.clone());

                        inline_index_column_tracker
                            .entry(index_name)
                            .or_default()
                            .insert(col.name.clone());
                    }
                    StrOrBoolOrArray::Bool(true) => {
                        // Auto-generated index name
                        let index_name = format!("idx_{}_{}", self.name, col.name);

                        // Check for duplicate (auto-generated names are unique per column, so this shouldn't happen)
                        // But we check anyway for consistency - only check inline definitions
                        if let Some(columns) = inline_index_column_tracker.get(index_name.as_str())
                            && columns.contains(col.name.as_str())
                        {
                            return Err(TableValidationError::DuplicateIndexColumn {
                                index_name: index_name.clone(),
                                column_name: col.name.clone(),
                            });
                        }

                        if !index_groups.contains_key(&index_name) {
                            index_order.push(index_name.clone());
                        }

                        index_groups
                            .entry(index_name.clone())
                            .or_default()
                            .push(col.name.clone());

                        inline_index_column_tracker
                            .entry(index_name)
                            .or_default()
                            .insert(col.name.clone());
                    }
                    StrOrBoolOrArray::Bool(false) => continue,
                    StrOrBoolOrArray::Array(names) => {
                        // Array format: each element is an index name
                        // This column will be part of all these named indexes
                        // Check for duplicates within the array
                        let mut seen_in_array = HashSet::new();
                        for index_name in names {
                            // Check for duplicate within the same array
                            if seen_in_array.contains(index_name.as_str()) {
                                return Err(TableValidationError::DuplicateIndexColumn {
                                    index_name: index_name.clone(),
                                    column_name: col.name.clone(),
                                });
                            }
                            seen_in_array.insert(index_name.clone());

                            // Check for duplicate across different inline definitions
                            // Only check inline definitions, not existing table-level indexes
                            if let Some(columns) =
                                inline_index_column_tracker.get(index_name.as_str())
                                && columns.contains(col.name.as_str())
                            {
                                return Err(TableValidationError::DuplicateIndexColumn {
                                    index_name: index_name.clone(),
                                    column_name: col.name.clone(),
                                });
                            }

                            if !index_groups.contains_key(index_name.as_str()) {
                                index_order.push(index_name.clone());
                            }

                            index_groups
                                .entry(index_name.clone())
                                .or_default()
                                .push(col.name.clone());

                            inline_index_column_tracker
                                .entry(index_name.clone())
                                .or_default()
                                .insert(col.name.clone());
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
            let exists = indexes.iter().any(|i| i.name == index_name);

            if !exists {
                indexes.push(IndexDef {
                    name: index_name,
                    columns,
                    unique: false,
                });
            }
        }

        Ok(TableDef {
            name: self.name.clone(),
            columns: self.columns.clone(),
            constraints,
            indexes,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::column::{ColumnType, SimpleColumnType};
    use crate::schema::foreign_key::{ForeignKeyDef, ForeignKeySyntax};
    use crate::schema::primary_key::PrimaryKeySyntax;
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
        let mut id_col = col("id", ColumnType::Simple(SimpleColumnType::Integer));
        id_col.primary_key = Some(PrimaryKeySyntax::Bool(true));

        let table = TableDef {
            name: "users".into(),
            columns: vec![
                id_col,
                col("name", ColumnType::Simple(SimpleColumnType::Text)),
            ],
            constraints: vec![],
            indexes: vec![],
        };

        let normalized = table.normalize().unwrap();
        assert_eq!(normalized.constraints.len(), 1);
        assert!(matches!(
            &normalized.constraints[0],
            TableConstraint::PrimaryKey { columns, .. } if columns == &["id".to_string()]
        ));
    }

    #[test]
    fn normalize_multiple_inline_primary_keys() {
        let mut id_col = col("id", ColumnType::Simple(SimpleColumnType::Integer));
        id_col.primary_key = Some(PrimaryKeySyntax::Bool(true));

        let mut tenant_col = col("tenant_id", ColumnType::Simple(SimpleColumnType::Integer));
        tenant_col.primary_key = Some(PrimaryKeySyntax::Bool(true));

        let table = TableDef {
            name: "users".into(),
            columns: vec![id_col, tenant_col],
            constraints: vec![],
            indexes: vec![],
        };

        let normalized = table.normalize().unwrap();
        assert_eq!(normalized.constraints.len(), 1);
        assert!(matches!(
            &normalized.constraints[0],
            TableConstraint::PrimaryKey { columns, .. } if columns == &["id".to_string(), "tenant_id".to_string()]
        ));
    }

    #[test]
    fn normalize_does_not_duplicate_existing_pk() {
        let mut id_col = col("id", ColumnType::Simple(SimpleColumnType::Integer));
        id_col.primary_key = Some(PrimaryKeySyntax::Bool(true));

        let table = TableDef {
            name: "users".into(),
            columns: vec![id_col],
            constraints: vec![TableConstraint::PrimaryKey {
                auto_increment: false,
                columns: vec!["id".into()],
            }],
            indexes: vec![],
        };

        let normalized = table.normalize().unwrap();
        assert_eq!(normalized.constraints.len(), 1);
    }

    #[test]
    fn normalize_ignores_primary_key_false() {
        let mut id_col = col("id", ColumnType::Simple(SimpleColumnType::Integer));
        id_col.primary_key = Some(PrimaryKeySyntax::Bool(false));

        let table = TableDef {
            name: "users".into(),
            columns: vec![
                id_col,
                col("name", ColumnType::Simple(SimpleColumnType::Text)),
            ],
            constraints: vec![],
            indexes: vec![],
        };

        let normalized = table.normalize().unwrap();
        // primary_key: false should be ignored, so no primary key constraint should be added
        assert_eq!(normalized.constraints.len(), 0);
    }

    #[test]
    fn normalize_inline_unique_bool() {
        let mut email_col = col("email", ColumnType::Simple(SimpleColumnType::Text));
        email_col.unique = Some(StrOrBoolOrArray::Bool(true));

        let table = TableDef {
            name: "users".into(),
            columns: vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                email_col,
            ],
            constraints: vec![],
            indexes: vec![],
        };

        let normalized = table.normalize().unwrap();
        assert_eq!(normalized.constraints.len(), 1);
        assert!(matches!(
            &normalized.constraints[0],
            TableConstraint::Unique { name: None, columns } if columns == &["email".to_string()]
        ));
    }

    #[test]
    fn normalize_inline_unique_with_name() {
        let mut email_col = col("email", ColumnType::Simple(SimpleColumnType::Text));
        email_col.unique = Some(StrOrBoolOrArray::Str("uq_users_email".into()));

        let table = TableDef {
            name: "users".into(),
            columns: vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                email_col,
            ],
            constraints: vec![],
            indexes: vec![],
        };

        let normalized = table.normalize().unwrap();
        assert_eq!(normalized.constraints.len(), 1);
        assert!(matches!(
            &normalized.constraints[0],
            TableConstraint::Unique { name: Some(n), columns }
                if n == "uq_users_email" && columns == &["email".to_string()]
        ));
    }

    #[test]
    fn normalize_inline_index_bool() {
        let mut name_col = col("name", ColumnType::Simple(SimpleColumnType::Text));
        name_col.index = Some(StrOrBoolOrArray::Bool(true));

        let table = TableDef {
            name: "users".into(),
            columns: vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                name_col,
            ],
            constraints: vec![],
            indexes: vec![],
        };

        let normalized = table.normalize().unwrap();
        assert_eq!(normalized.indexes.len(), 1);
        assert_eq!(normalized.indexes[0].name, "idx_users_name");
        assert_eq!(normalized.indexes[0].columns, vec!["name".to_string()]);
        assert!(!normalized.indexes[0].unique);
    }

    #[test]
    fn normalize_inline_index_with_name() {
        let mut name_col = col("name", ColumnType::Simple(SimpleColumnType::Text));
        name_col.index = Some(StrOrBoolOrArray::Str("custom_idx_name".into()));

        let table = TableDef {
            name: "users".into(),
            columns: vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                name_col,
            ],
            constraints: vec![],
            indexes: vec![],
        };

        let normalized = table.normalize().unwrap();
        assert_eq!(normalized.indexes.len(), 1);
        assert_eq!(normalized.indexes[0].name, "custom_idx_name");
    }

    #[test]
    fn normalize_inline_foreign_key() {
        let mut user_id_col = col("user_id", ColumnType::Simple(SimpleColumnType::Integer));
        user_id_col.foreign_key = Some(ForeignKeySyntax::Object(ForeignKeyDef {
            ref_table: "users".into(),
            ref_columns: vec!["id".into()],
            on_delete: Some(ReferenceAction::Cascade),
            on_update: None,
        }));

        let table = TableDef {
            name: "posts".into(),
            columns: vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                user_id_col,
            ],
            constraints: vec![],
            indexes: vec![],
        };

        let normalized = table.normalize().unwrap();
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
        let mut id_col = col("id", ColumnType::Simple(SimpleColumnType::Integer));
        id_col.primary_key = Some(PrimaryKeySyntax::Bool(true));

        let mut email_col = col("email", ColumnType::Simple(SimpleColumnType::Text));
        email_col.unique = Some(StrOrBoolOrArray::Bool(true));

        let mut name_col = col("name", ColumnType::Simple(SimpleColumnType::Text));
        name_col.index = Some(StrOrBoolOrArray::Bool(true));

        let mut user_id_col = col("org_id", ColumnType::Simple(SimpleColumnType::Integer));
        user_id_col.foreign_key = Some(ForeignKeySyntax::Object(ForeignKeyDef {
            ref_table: "orgs".into(),
            ref_columns: vec!["id".into()],
            on_delete: None,
            on_update: None,
        }));

        let table = TableDef {
            name: "users".into(),
            columns: vec![id_col, email_col, name_col, user_id_col],
            constraints: vec![],
            indexes: vec![],
        };

        let normalized = table.normalize().unwrap();
        // Should have: PrimaryKey, Unique, ForeignKey
        assert_eq!(normalized.constraints.len(), 3);
        // Should have: 1 index
        assert_eq!(normalized.indexes.len(), 1);
    }

    #[test]
    fn normalize_composite_index_from_string_name() {
        let mut updated_at_col = col(
            "updated_at",
            ColumnType::Simple(SimpleColumnType::Timestamp),
        );
        updated_at_col.index = Some(StrOrBoolOrArray::Str("tuple".into()));

        let mut user_id_col = col("user_id", ColumnType::Simple(SimpleColumnType::Integer));
        user_id_col.index = Some(StrOrBoolOrArray::Str("tuple".into()));

        let table = TableDef {
            name: "post".into(),
            columns: vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                updated_at_col,
                user_id_col,
            ],
            constraints: vec![],
            indexes: vec![],
        };

        let normalized = table.normalize().unwrap();
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
        let mut col1 = col("col1", ColumnType::Simple(SimpleColumnType::Text));
        col1.index = Some(StrOrBoolOrArray::Str("idx_a".into()));

        let mut col2 = col("col2", ColumnType::Simple(SimpleColumnType::Text));
        col2.index = Some(StrOrBoolOrArray::Str("idx_a".into()));

        let mut col3 = col("col3", ColumnType::Simple(SimpleColumnType::Text));
        col3.index = Some(StrOrBoolOrArray::Str("idx_b".into()));

        let mut col4 = col("col4", ColumnType::Simple(SimpleColumnType::Text));
        col4.index = Some(StrOrBoolOrArray::Bool(true));

        let table = TableDef {
            name: "test".into(),
            columns: vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                col1,
                col2,
                col3,
                col4,
            ],
            constraints: vec![],
            indexes: vec![],
        };

        let normalized = table.normalize().unwrap();
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
        let mut email_col = col("email", ColumnType::Simple(SimpleColumnType::Text));
        email_col.unique = Some(StrOrBoolOrArray::Bool(false));
        email_col.index = Some(StrOrBoolOrArray::Bool(false));

        let table = TableDef {
            name: "users".into(),
            columns: vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                email_col,
            ],
            constraints: vec![],
            indexes: vec![],
        };

        let normalized = table.normalize().unwrap();
        assert_eq!(normalized.constraints.len(), 0);
        assert_eq!(normalized.indexes.len(), 0);
    }

    #[test]
    fn normalize_table_without_primary_key() {
        // Test normalize with a table that has no primary key columns
        // This should cover lines 67-69, 72-73, and 93 (pk_columns.is_empty() branch)
        let table = TableDef {
            name: "users".into(),
            columns: vec![
                col("name", ColumnType::Simple(SimpleColumnType::Text)),
                col("email", ColumnType::Simple(SimpleColumnType::Text)),
            ],
            constraints: vec![],
            indexes: vec![],
        };

        let normalized = table.normalize().unwrap();
        // Should not add any primary key constraint
        assert_eq!(normalized.constraints.len(), 0);
        assert_eq!(normalized.indexes.len(), 0);
    }

    #[test]
    fn normalize_multiple_indexes_from_same_array() {
        // Multiple columns with same array of index names should create multiple composite indexes
        let mut updated_at_col = col(
            "updated_at",
            ColumnType::Simple(SimpleColumnType::Timestamp),
        );
        updated_at_col.index = Some(StrOrBoolOrArray::Array(vec![
            "tuple".into(),
            "tuple2".into(),
        ]));

        let mut user_id_col = col("user_id", ColumnType::Simple(SimpleColumnType::Integer));
        user_id_col.index = Some(StrOrBoolOrArray::Array(vec![
            "tuple".into(),
            "tuple2".into(),
        ]));

        let table = TableDef {
            name: "post".into(),
            columns: vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                updated_at_col,
                user_id_col,
            ],
            constraints: vec![],
            indexes: vec![],
        };

        let normalized = table.normalize().unwrap();
        // Should have: tuple (composite: updated_at, user_id), tuple2 (composite: updated_at, user_id)
        assert_eq!(normalized.indexes.len(), 2);

        let tuple_idx = normalized
            .indexes
            .iter()
            .find(|i| i.name == "tuple")
            .unwrap();
        let mut sorted_cols = tuple_idx.columns.clone();
        sorted_cols.sort();
        assert_eq!(
            sorted_cols,
            vec!["updated_at".to_string(), "user_id".to_string()]
        );

        let tuple2_idx = normalized
            .indexes
            .iter()
            .find(|i| i.name == "tuple2")
            .unwrap();
        let mut sorted_cols2 = tuple2_idx.columns.clone();
        sorted_cols2.sort();
        assert_eq!(
            sorted_cols2,
            vec!["updated_at".to_string(), "user_id".to_string()]
        );
    }

    #[test]
    fn normalize_inline_unique_with_array_existing_constraint() {
        // Test Array format where constraint already exists - should add column to existing
        let mut col1 = col("col1", ColumnType::Simple(SimpleColumnType::Text));
        col1.unique = Some(StrOrBoolOrArray::Array(vec!["uq_group".into()]));

        let mut col2 = col("col2", ColumnType::Simple(SimpleColumnType::Text));
        col2.unique = Some(StrOrBoolOrArray::Array(vec!["uq_group".into()]));

        let table = TableDef {
            name: "test".into(),
            columns: vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                col1,
                col2,
            ],
            constraints: vec![],
            indexes: vec![],
        };

        let normalized = table.normalize().unwrap();
        assert_eq!(normalized.constraints.len(), 1);
        let unique_constraint = &normalized.constraints[0];
        assert!(matches!(
            unique_constraint,
            TableConstraint::Unique { name: Some(n), columns: _ }
                if n == "uq_group"
        ));
        if let TableConstraint::Unique { columns, .. } = unique_constraint {
            let mut sorted_cols = columns.clone();
            sorted_cols.sort();
            assert_eq!(sorted_cols, vec!["col1".to_string(), "col2".to_string()]);
        }
    }

    #[test]
    fn normalize_inline_unique_with_array_column_already_in_constraint() {
        // Test Array format where column is already in constraint - should not duplicate
        let mut col1 = col("col1", ColumnType::Simple(SimpleColumnType::Text));
        col1.unique = Some(StrOrBoolOrArray::Array(vec!["uq_group".into()]));

        let table = TableDef {
            name: "test".into(),
            columns: vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                col1.clone(),
            ],
            constraints: vec![],
            indexes: vec![],
        };

        let normalized1 = table.normalize().unwrap();
        assert_eq!(normalized1.constraints.len(), 1);

        // Add same column again - should not create duplicate
        let table2 = TableDef {
            name: "test".into(),
            columns: vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                col1,
            ],
            constraints: normalized1.constraints.clone(),
            indexes: vec![],
        };

        let normalized2 = table2.normalize().unwrap();
        assert_eq!(normalized2.constraints.len(), 1);
        if let TableConstraint::Unique { columns, .. } = &normalized2.constraints[0] {
            assert_eq!(columns.len(), 1);
            assert_eq!(columns[0], "col1");
        }
    }

    #[test]
    fn normalize_inline_unique_str_already_exists() {
        // Test that existing unique constraint with same name and column is not duplicated
        let mut email_col = col("email", ColumnType::Simple(SimpleColumnType::Text));
        email_col.unique = Some(StrOrBoolOrArray::Str("uq_email".into()));

        let table = TableDef {
            name: "users".into(),
            columns: vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                email_col,
            ],
            constraints: vec![TableConstraint::Unique {
                name: Some("uq_email".into()),
                columns: vec!["email".into()],
            }],
            indexes: vec![],
        };

        let normalized = table.normalize().unwrap();
        // Should not duplicate the constraint
        let unique_constraints: Vec<_> = normalized
            .constraints
            .iter()
            .filter(|c| matches!(c, TableConstraint::Unique { .. }))
            .collect();
        assert_eq!(unique_constraints.len(), 1);
    }

    #[test]
    fn normalize_inline_unique_bool_already_exists() {
        // Test that existing unnamed unique constraint with same column is not duplicated
        let mut email_col = col("email", ColumnType::Simple(SimpleColumnType::Text));
        email_col.unique = Some(StrOrBoolOrArray::Bool(true));

        let table = TableDef {
            name: "users".into(),
            columns: vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                email_col,
            ],
            constraints: vec![TableConstraint::Unique {
                name: None,
                columns: vec!["email".into()],
            }],
            indexes: vec![],
        };

        let normalized = table.normalize().unwrap();
        // Should not duplicate the constraint
        let unique_constraints: Vec<_> = normalized
            .constraints
            .iter()
            .filter(|c| matches!(c, TableConstraint::Unique { .. }))
            .collect();
        assert_eq!(unique_constraints.len(), 1);
    }

    #[test]
    fn normalize_inline_foreign_key_already_exists() {
        // Test that existing foreign key constraint is not duplicated
        let mut user_id_col = col("user_id", ColumnType::Simple(SimpleColumnType::Integer));
        user_id_col.foreign_key = Some(ForeignKeySyntax::Object(ForeignKeyDef {
            ref_table: "users".into(),
            ref_columns: vec!["id".into()],
            on_delete: None,
            on_update: None,
        }));

        let table = TableDef {
            name: "posts".into(),
            columns: vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                user_id_col,
            ],
            constraints: vec![TableConstraint::ForeignKey {
                name: None,
                columns: vec!["user_id".into()],
                ref_table: "users".into(),
                ref_columns: vec!["id".into()],
                on_delete: None,
                on_update: None,
            }],
            indexes: vec![],
        };

        let normalized = table.normalize().unwrap();
        // Should not duplicate the foreign key
        let fk_constraints: Vec<_> = normalized
            .constraints
            .iter()
            .filter(|c| matches!(c, TableConstraint::ForeignKey { .. }))
            .collect();
        assert_eq!(fk_constraints.len(), 1);
    }

    #[test]
    fn normalize_duplicate_index_same_column_str() {
        // Same index name applied to the same column multiple times should error
        // This tests inline index duplicate, not table-level index
        let mut col1 = col("col1", ColumnType::Simple(SimpleColumnType::Text));
        col1.index = Some(StrOrBoolOrArray::Str("idx1".into()));

        let table = TableDef {
            name: "test".into(),
            columns: vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                col1.clone(),
                {
                    // Same column with same index name again
                    let mut c = col1.clone();
                    c.index = Some(StrOrBoolOrArray::Str("idx1".into()));
                    c
                },
            ],
            constraints: vec![],
            indexes: vec![],
        };

        let result = table.normalize();
        assert!(result.is_err());
        if let Err(TableValidationError::DuplicateIndexColumn {
            index_name,
            column_name,
        }) = result
        {
            assert_eq!(index_name, "idx1");
            assert_eq!(column_name, "col1");
        } else {
            panic!("Expected DuplicateIndexColumn error");
        }
    }

    #[test]
    fn normalize_duplicate_index_same_column_array() {
        // Same index name in array applied to the same column should error
        let mut col1 = col("col1", ColumnType::Simple(SimpleColumnType::Text));
        col1.index = Some(StrOrBoolOrArray::Array(vec!["idx1".into(), "idx1".into()]));

        let table = TableDef {
            name: "test".into(),
            columns: vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                col1,
            ],
            constraints: vec![],
            indexes: vec![],
        };

        let result = table.normalize();
        assert!(result.is_err());
        if let Err(TableValidationError::DuplicateIndexColumn {
            index_name,
            column_name,
        }) = result
        {
            assert_eq!(index_name, "idx1");
            assert_eq!(column_name, "col1");
        } else {
            panic!("Expected DuplicateIndexColumn error");
        }
    }

    #[test]
    fn normalize_duplicate_index_same_column_multiple_definitions() {
        // Same index name applied to the same column in different ways should error
        let mut col1 = col("col1", ColumnType::Simple(SimpleColumnType::Text));
        col1.index = Some(StrOrBoolOrArray::Str("idx1".into()));

        let table = TableDef {
            name: "test".into(),
            columns: vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                col1.clone(),
                {
                    let mut c = col1.clone();
                    c.index = Some(StrOrBoolOrArray::Array(vec!["idx1".into()]));
                    c
                },
            ],
            constraints: vec![],
            indexes: vec![],
        };

        let result = table.normalize();
        assert!(result.is_err());
        if let Err(TableValidationError::DuplicateIndexColumn {
            index_name,
            column_name,
        }) = result
        {
            assert_eq!(index_name, "idx1");
            assert_eq!(column_name, "col1");
        } else {
            panic!("Expected DuplicateIndexColumn error");
        }
    }

    #[test]
    fn test_table_validation_error_display() {
        let error = TableValidationError::DuplicateIndexColumn {
            index_name: "idx_test".into(),
            column_name: "col1".into(),
        };
        let error_msg = format!("{}", error);
        assert!(error_msg.contains("idx_test"));
        assert!(error_msg.contains("col1"));
        assert!(error_msg.contains("Duplicate index"));
    }

    #[test]
    fn normalize_inline_unique_str_with_different_constraint_type() {
        // Test that other constraint types don't match in the exists check
        let mut email_col = col("email", ColumnType::Simple(SimpleColumnType::Text));
        email_col.unique = Some(StrOrBoolOrArray::Str("uq_email".into()));

        let table = TableDef {
            name: "users".into(),
            columns: vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                email_col,
            ],
            constraints: vec![
                // Add a PrimaryKey constraint (different type) - should not match
                TableConstraint::PrimaryKey {
                    auto_increment: false,
                    columns: vec!["id".into()],
                },
            ],
            indexes: vec![],
        };

        let normalized = table.normalize().unwrap();
        // Should have: PrimaryKey (existing) + Unique (new)
        assert_eq!(normalized.constraints.len(), 2);
    }

    #[test]
    fn normalize_inline_unique_array_with_different_constraint_type() {
        // Test that other constraint types don't match in the exists check for Array case
        let mut col1 = col("col1", ColumnType::Simple(SimpleColumnType::Text));
        col1.unique = Some(StrOrBoolOrArray::Array(vec!["uq_group".into()]));

        let table = TableDef {
            name: "test".into(),
            columns: vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                col1,
            ],
            constraints: vec![
                // Add a PrimaryKey constraint (different type) - should not match
                TableConstraint::PrimaryKey {
                    auto_increment: false,
                    columns: vec!["id".into()],
                },
            ],
            indexes: vec![],
        };

        let normalized = table.normalize().unwrap();
        // Should have: PrimaryKey (existing) + Unique (new)
        assert_eq!(normalized.constraints.len(), 2);
    }

    #[test]
    fn normalize_duplicate_index_bool_true_same_column() {
        // Test that Bool(true) with duplicate on same column errors
        let mut col1 = col("col1", ColumnType::Simple(SimpleColumnType::Text));
        col1.index = Some(StrOrBoolOrArray::Bool(true));

        let table = TableDef {
            name: "test".into(),
            columns: vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                col1.clone(),
                {
                    // Same column with Bool(true) again
                    let mut c = col1.clone();
                    c.index = Some(StrOrBoolOrArray::Bool(true));
                    c
                },
            ],
            constraints: vec![],
            indexes: vec![],
        };

        let result = table.normalize();
        assert!(result.is_err());
        if let Err(TableValidationError::DuplicateIndexColumn {
            index_name,
            column_name,
        }) = result
        {
            assert!(index_name.contains("idx_test"));
            assert!(index_name.contains("col1"));
            assert_eq!(column_name, "col1");
        } else {
            panic!("Expected DuplicateIndexColumn error");
        }
    }

    #[test]
    fn normalize_inline_foreign_key_string_syntax() {
        // Test ForeignKeySyntax::String with valid "table.column" format
        let mut user_id_col = col("user_id", ColumnType::Simple(SimpleColumnType::Integer));
        user_id_col.foreign_key = Some(ForeignKeySyntax::String("users.id".into()));

        let table = TableDef {
            name: "posts".into(),
            columns: vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                user_id_col,
            ],
            constraints: vec![],
            indexes: vec![],
        };

        let normalized = table.normalize().unwrap();
        assert_eq!(normalized.constraints.len(), 1);
        assert!(matches!(
            &normalized.constraints[0],
            TableConstraint::ForeignKey {
                name: None,
                columns,
                ref_table,
                ref_columns,
                on_delete: None,
                on_update: None,
            } if columns == &["user_id".to_string()]
                && ref_table == "users"
                && ref_columns == &["id".to_string()]
        ));
    }

    #[test]
    fn normalize_inline_foreign_key_invalid_format_no_dot() {
        // Test ForeignKeySyntax::String with invalid format (no dot)
        let mut user_id_col = col("user_id", ColumnType::Simple(SimpleColumnType::Integer));
        user_id_col.foreign_key = Some(ForeignKeySyntax::String("usersid".into()));

        let table = TableDef {
            name: "posts".into(),
            columns: vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                user_id_col,
            ],
            constraints: vec![],
            indexes: vec![],
        };

        let result = table.normalize();
        assert!(result.is_err());
        if let Err(TableValidationError::InvalidForeignKeyFormat { column_name, value }) = result {
            assert_eq!(column_name, "user_id");
            assert_eq!(value, "usersid");
        } else {
            panic!("Expected InvalidForeignKeyFormat error");
        }
    }

    #[test]
    fn normalize_inline_foreign_key_invalid_format_empty_table() {
        // Test ForeignKeySyntax::String with empty table part
        let mut user_id_col = col("user_id", ColumnType::Simple(SimpleColumnType::Integer));
        user_id_col.foreign_key = Some(ForeignKeySyntax::String(".id".into()));

        let table = TableDef {
            name: "posts".into(),
            columns: vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                user_id_col,
            ],
            constraints: vec![],
            indexes: vec![],
        };

        let result = table.normalize();
        assert!(result.is_err());
        if let Err(TableValidationError::InvalidForeignKeyFormat { column_name, value }) = result {
            assert_eq!(column_name, "user_id");
            assert_eq!(value, ".id");
        } else {
            panic!("Expected InvalidForeignKeyFormat error");
        }
    }

    #[test]
    fn normalize_inline_foreign_key_invalid_format_empty_column() {
        // Test ForeignKeySyntax::String with empty column part
        let mut user_id_col = col("user_id", ColumnType::Simple(SimpleColumnType::Integer));
        user_id_col.foreign_key = Some(ForeignKeySyntax::String("users.".into()));

        let table = TableDef {
            name: "posts".into(),
            columns: vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                user_id_col,
            ],
            constraints: vec![],
            indexes: vec![],
        };

        let result = table.normalize();
        assert!(result.is_err());
        if let Err(TableValidationError::InvalidForeignKeyFormat { column_name, value }) = result {
            assert_eq!(column_name, "user_id");
            assert_eq!(value, "users.");
        } else {
            panic!("Expected InvalidForeignKeyFormat error");
        }
    }

    #[test]
    fn normalize_inline_foreign_key_invalid_format_too_many_parts() {
        // Test ForeignKeySyntax::String with too many parts
        let mut user_id_col = col("user_id", ColumnType::Simple(SimpleColumnType::Integer));
        user_id_col.foreign_key = Some(ForeignKeySyntax::String("schema.users.id".into()));

        let table = TableDef {
            name: "posts".into(),
            columns: vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                user_id_col,
            ],
            constraints: vec![],
            indexes: vec![],
        };

        let result = table.normalize();
        assert!(result.is_err());
        if let Err(TableValidationError::InvalidForeignKeyFormat { column_name, value }) = result {
            assert_eq!(column_name, "user_id");
            assert_eq!(value, "schema.users.id");
        } else {
            panic!("Expected InvalidForeignKeyFormat error");
        }
    }

    #[test]
    fn normalize_inline_primary_key_with_auto_increment() {
        use crate::schema::primary_key::PrimaryKeyDef;

        let mut id_col = col("id", ColumnType::Simple(SimpleColumnType::Integer));
        id_col.primary_key = Some(PrimaryKeySyntax::Object(PrimaryKeyDef {
            auto_increment: true,
            columns: vec![], // columns is ignored for inline definition
        }));

        let table = TableDef {
            name: "users".into(),
            columns: vec![
                id_col,
                col("name", ColumnType::Simple(SimpleColumnType::Text)),
            ],
            constraints: vec![],
            indexes: vec![],
        };

        let normalized = table.normalize().unwrap();
        assert_eq!(normalized.constraints.len(), 1);
        assert!(matches!(
            &normalized.constraints[0],
            TableConstraint::PrimaryKey { auto_increment: true, columns } if columns == &["id".to_string()]
        ));
    }

    #[test]
    fn normalize_duplicate_inline_index_on_same_column() {
        // This test triggers the DuplicateIndexColumn error (lines 251-253)
        // by having the same column appear twice in the same named index group
        use crate::schema::str_or_bool::StrOrBoolOrArray;

        // Create a column that references the same index name twice (via Array)
        let mut email_col = col("email", ColumnType::Simple(SimpleColumnType::Text));
        email_col.index = Some(StrOrBoolOrArray::Array(vec![
            "idx_email".into(),
            "idx_email".into(), // Duplicate reference
        ]));

        let table = TableDef {
            name: "users".into(),
            columns: vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                email_col,
            ],
            constraints: vec![],
            indexes: vec![],
        };

        let result = table.normalize();
        assert!(result.is_err());
        if let Err(TableValidationError::DuplicateIndexColumn {
            index_name,
            column_name,
        }) = result
        {
            assert_eq!(index_name, "idx_email");
            assert_eq!(column_name, "email");
        } else {
            panic!("Expected DuplicateIndexColumn error, got: {:?}", result);
        }
    }

    #[test]
    fn test_invalid_foreign_key_format_error_display() {
        let error = TableValidationError::InvalidForeignKeyFormat {
            column_name: "user_id".into(),
            value: "invalid".into(),
        };
        let error_msg = format!("{}", error);
        assert!(error_msg.contains("user_id"));
        assert!(error_msg.contains("invalid"));
        assert!(error_msg.contains("table.column"));
    }
}
