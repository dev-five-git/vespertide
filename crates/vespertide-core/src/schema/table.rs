use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::schema::{
    StrOrBool, column::ColumnDef, constraint::TableConstraint, index::IndexDef, names::TableName,
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
                constraints.push(TableConstraint::PrimaryKey { columns: pk_columns });
            }
        }

        // Process inline unique and index for each column
        for col in &self.columns {
            // Handle inline unique
            if let Some(ref unique_val) = col.unique {
                let constraint_name = match unique_val {
                    StrOrBool::Str(name) => Some(name.clone()),
                    StrOrBool::Bool(true) => None,
                    StrOrBool::Bool(false) => continue,
                };

                // Check if this unique constraint already exists
                let exists = constraints.iter().any(|c| {
                    if let TableConstraint::Unique { columns, .. } = c {
                        columns.len() == 1 && columns[0] == col.name
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

            // Handle inline index
            if let Some(ref index_val) = col.index {
                let index_name = match index_val {
                    StrOrBool::Str(name) => name.clone(),
                    StrOrBool::Bool(true) => format!("idx_{}_{}", self.name, col.name),
                    StrOrBool::Bool(false) => continue,
                };

                // Check if this index already exists
                let exists = indexes.iter().any(|i| {
                    i.columns.len() == 1 && i.columns[0] == col.name
                });

                if !exists {
                    indexes.push(IndexDef {
                        name: index_name,
                        columns: vec![col.name.clone()],
                        unique: false,
                    });
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
        email_col.unique = Some(StrOrBool::Bool(true));

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
        email_col.unique = Some(StrOrBool::Str("uq_users_email".into()));

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
        name_col.index = Some(StrOrBool::Bool(true));

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
        name_col.index = Some(StrOrBool::Str("custom_idx_name".into()));

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
        email_col.unique = Some(StrOrBool::Bool(true));

        let mut name_col = col("name", ColumnType::Text);
        name_col.index = Some(StrOrBool::Bool(true));

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
    fn normalize_false_values_are_ignored() {
        let mut email_col = col("email", ColumnType::Text);
        email_col.unique = Some(StrOrBool::Bool(false));
        email_col.index = Some(StrOrBool::Bool(false));

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
}
