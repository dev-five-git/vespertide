use crate::schema::names::{ColumnName, TableName};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TableValidationError {
    DuplicateColumnName {
        table: TableName,
        column: ColumnName,
    },
    DuplicateIndexColumn {
        index_name: String,
        column_name: String,
    },
    InvalidForeignKeyFormat {
        column_name: String,
        value: String,
    },
    /// Internal invariant violation in normalization; valid user input should not trigger this.
    InvariantViolation {
        context: String,
    },
}

impl std::fmt::Display for TableValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TableValidationError::DuplicateColumnName { table, column } => {
                write!(f, "table '{table}' has duplicate column name '{column}'")
            }
            TableValidationError::DuplicateIndexColumn {
                index_name,
                column_name,
            } => {
                write!(
                    f,
                    "Duplicate index '{index_name}' on column '{column_name}': the same index name cannot be applied to the same column multiple times"
                )
            }
            TableValidationError::InvalidForeignKeyFormat { column_name, value } => {
                write!(
                    f,
                    "Invalid foreign key format '{value}' on column '{column_name}': expected 'table.column' format"
                )
            }
            TableValidationError::InvariantViolation { context } => {
                write!(
                    f,
                    "internal table normalization invariant violated: {context}"
                )
            }
        }
    }
}

impl std::error::Error for TableValidationError {}

#[cfg(test)]
mod tests {
    //! Coverage-closure tests for the `InvalidForeignKeyFormat` Display arm.
    //! Targets `uncovered-detail.json` lines 35, 36
    //! (the multi-line `write!` body for `DuplicateIndexColumn` / `InvalidForeignKeyFormat`).
    use super::*;
    use rstest::rstest;

    #[rstest]
    #[case::duplicate_index(
        TableValidationError::DuplicateIndexColumn {
            index_name: "ix_user__email".into(),
            column_name: "email".into(),
        },
        "Duplicate index 'ix_user__email' on column 'email'",
    )]
    #[case::invalid_fk(
        TableValidationError::InvalidForeignKeyFormat {
            column_name: "author_id".into(),
            value: "broken".into(),
        },
        "Invalid foreign key format 'broken' on column 'author_id'",
    )]
    fn display_emits_descriptive_messages(#[case] err: TableValidationError, #[case] needle: &str) {
        let rendered = err.to_string();
        assert!(
            rendered.contains(needle),
            "expected `{needle}` to appear in `{rendered}`"
        );
    }

    #[test]
    fn display_duplicate_column_name() {
        // Pre-existing covered arm; included so the file's `mod tests` also
        // documents the canonical case.
        let err = TableValidationError::DuplicateColumnName {
            table: "user".into(),
            column: "email".into(),
        };
        assert_eq!(
            err.to_string(),
            "table 'user' has duplicate column name 'email'",
        );
    }

    #[test]
    fn display_invariant_violation() {
        let err = TableValidationError::InvariantViolation {
            context: "normalize() collapsed two unique constraints".into(),
        };
        assert!(err.to_string().contains("normalize()"));
    }
}
