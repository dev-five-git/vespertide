use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::schema::{
    names::{ColumnName, TableName},
    ReferenceAction,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum TableConstraint {
    PrimaryKey {
        #[serde(default)]
        auto_increment: bool,
        columns: Vec<ColumnName>,
    },
    Unique {
        #[serde(skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        columns: Vec<ColumnName>,
    },
    ForeignKey {
        #[serde(skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        columns: Vec<ColumnName>,
        ref_table: TableName,
        ref_columns: Vec<ColumnName>,
        on_delete: Option<ReferenceAction>,
        on_update: Option<ReferenceAction>,
    },
    Check {
        name: String,
        expr: String,
    },
    Index {
        #[serde(skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        columns: Vec<ColumnName>,
    },
}

impl TableConstraint {
    /// Returns the columns referenced by this constraint.
    /// For Check constraints, returns an empty slice (expression-based, not column-based).
    pub fn columns(&self) -> &[ColumnName] {
        match self {
            TableConstraint::PrimaryKey { columns, .. } => columns,
            TableConstraint::Unique { columns, .. } => columns,
            TableConstraint::ForeignKey { columns, .. } => columns,
            TableConstraint::Index { columns, .. } => columns,
            TableConstraint::Check { .. } => &[],
        }
    }

    /// Apply a prefix to referenced table names in this constraint.
    /// Only affects ForeignKey constraints (which reference other tables).
    pub fn with_prefix(self, prefix: &str) -> Self {
        if prefix.is_empty() {
            return self;
        }
        match self {
            TableConstraint::ForeignKey {
                name,
                columns,
                ref_table,
                ref_columns,
                on_delete,
                on_update,
            } => TableConstraint::ForeignKey {
                name,
                columns,
                ref_table: format!("{}{}", prefix, ref_table),
                ref_columns,
                on_delete,
                on_update,
            },
            // Other constraints don't reference external tables
            other => other,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_columns_primary_key() {
        let pk = TableConstraint::PrimaryKey {
            auto_increment: false,
            columns: vec!["id".into(), "tenant_id".into()],
        };
        assert_eq!(pk.columns().len(), 2);
        assert_eq!(pk.columns()[0], "id");
        assert_eq!(pk.columns()[1], "tenant_id");
    }

    #[test]
    fn test_columns_unique() {
        let unique = TableConstraint::Unique {
            name: Some("uq_email".into()),
            columns: vec!["email".into()],
        };
        assert_eq!(unique.columns().len(), 1);
        assert_eq!(unique.columns()[0], "email");
    }

    #[test]
    fn test_columns_foreign_key() {
        let fk = TableConstraint::ForeignKey {
            name: Some("fk_user".into()),
            columns: vec!["user_id".into()],
            ref_table: "users".into(),
            ref_columns: vec!["id".into()],
            on_delete: None,
            on_update: None,
        };
        assert_eq!(fk.columns().len(), 1);
        assert_eq!(fk.columns()[0], "user_id");
    }

    #[test]
    fn test_columns_index() {
        let idx = TableConstraint::Index {
            name: Some("ix_created_at".into()),
            columns: vec!["created_at".into()],
        };
        assert_eq!(idx.columns().len(), 1);
        assert_eq!(idx.columns()[0], "created_at");
    }

    #[test]
    fn test_columns_check_returns_empty() {
        let check = TableConstraint::Check {
            name: "check_positive".into(),
            expr: "amount > 0".into(),
        };
        assert!(check.columns().is_empty());
    }

    #[test]
    fn test_with_prefix_foreign_key() {
        let fk = TableConstraint::ForeignKey {
            name: Some("fk_user".into()),
            columns: vec!["user_id".into()],
            ref_table: "users".into(),
            ref_columns: vec!["id".into()],
            on_delete: None,
            on_update: None,
        };
        let prefixed = fk.with_prefix("myapp_");
        if let TableConstraint::ForeignKey { ref_table, .. } = prefixed {
            assert_eq!(ref_table.as_str(), "myapp_users");
        } else {
            panic!("Expected ForeignKey");
        }
    }

    #[test]
    fn test_with_prefix_non_fk_unchanged() {
        let pk = TableConstraint::PrimaryKey {
            auto_increment: false,
            columns: vec!["id".into()],
        };
        let prefixed = pk.clone().with_prefix("myapp_");
        assert_eq!(pk, prefixed);

        let unique = TableConstraint::Unique {
            name: Some("uq_email".into()),
            columns: vec!["email".into()],
        };
        let prefixed = unique.clone().with_prefix("myapp_");
        assert_eq!(unique, prefixed);

        let idx = TableConstraint::Index {
            name: Some("ix_created_at".into()),
            columns: vec!["created_at".into()],
        };
        let prefixed = idx.clone().with_prefix("myapp_");
        assert_eq!(idx, prefixed);

        let check = TableConstraint::Check {
            name: "check_positive".into(),
            expr: "amount > 0".into(),
        };
        let prefixed = check.clone().with_prefix("myapp_");
        assert_eq!(check, prefixed);
    }

    #[test]
    fn test_with_prefix_empty_prefix() {
        let fk = TableConstraint::ForeignKey {
            name: Some("fk_user".into()),
            columns: vec!["user_id".into()],
            ref_table: "users".into(),
            ref_columns: vec!["id".into()],
            on_delete: None,
            on_update: None,
        };
        let prefixed = fk.clone().with_prefix("");
        assert_eq!(fk, prefixed);
    }
}
