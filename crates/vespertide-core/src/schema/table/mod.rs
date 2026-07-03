mod normalize;
#[cfg(all(test, feature = "arbitrary"))]
mod normalize_proptest;
#[cfg(test)]
mod tests;
mod validation;

use serde::{Deserialize, Serialize};

use crate::schema::{column::ColumnDef, constraint::TableConstraint, names::TableName};

pub use validation::TableValidationError;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct TableDef {
    pub name: TableName,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub columns: Vec<ColumnDef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub constraints: Vec<TableConstraint>,
}

impl TableDef {
    /// Normalizes inline column constraints (`primary_key`, unique, index, `foreign_key`)
    /// into table-level constraints.
    /// Returns a new `TableDef` with all inline constraints converted to table-level.
    ///
    /// # Errors
    ///
    /// Returns an error if the same index name is applied to the same column multiple times.
    pub fn normalize(&self) -> Result<Self, TableValidationError> {
        normalize::normalize(self)
    }

    /// Validate that no two columns share a name within this table.
    ///
    /// # Errors
    /// Returns `TableValidationError::DuplicateColumnName { table, column }`
    /// if the same column name appears more than once.
    pub fn validate_unique_column_names(&self) -> Result<(), TableValidationError> {
        let mut seen: std::collections::BTreeSet<&str> = std::collections::BTreeSet::new();
        for col in &self.columns {
            if !seen.insert(col.name.as_str()) {
                return Err(TableValidationError::DuplicateColumnName {
                    table: self.name.clone(),
                    column: col.name.clone(),
                });
            }
        }
        Ok(())
    }
}
