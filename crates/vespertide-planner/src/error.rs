use thiserror::Error;

#[derive(Debug, Error)]
pub enum PlannerError {
    #[error("table already exists: {0}")]
    TableExists(String),
    #[error("table not found: {0}")]
    TableNotFound(String),
    #[error("column already exists: {0}.{1}")]
    ColumnExists(String, String),
    #[error("column not found: {0}.{1}")]
    ColumnNotFound(String, String),
    #[error("index not found: {0}.{1}")]
    IndexNotFound(String, String),
}

