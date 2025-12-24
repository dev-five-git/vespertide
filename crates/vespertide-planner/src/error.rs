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
    #[error("duplicate table name: {0}")]
    DuplicateTableName(String),
    #[error("foreign key references non-existent table: {0}.{1} -> {2}")]
    ForeignKeyTableNotFound(String, String, String),
    #[error("foreign key references non-existent column: {0}.{1} -> {2}.{3}")]
    ForeignKeyColumnNotFound(String, String, String, String),
    #[error("index references non-existent column: {0}.{1} -> {2}")]
    IndexColumnNotFound(String, String, String),
    #[error("constraint references non-existent column: {0}.{1} -> {2}")]
    ConstraintColumnNotFound(String, String, String),
    #[error("constraint has empty column list: {0}.{1}")]
    EmptyConstraintColumns(String, String),
    #[error("AddColumn requires fill_with when column is NOT NULL without default: {0}.{1}")]
    MissingFillWith(String, String),
    #[error("table validation error: {0}")]
    TableValidation(String),
    #[error("table '{0}' must have a primary key")]
    MissingPrimaryKey(String),
    #[error("enum '{0}' in column '{1}.{2}' has duplicate variant name: '{3}'")]
    DuplicateEnumVariantName(String, String, String, String),
    #[error("enum '{0}' in column '{1}.{2}' has duplicate value: {3}")]
    DuplicateEnumValue(String, String, String, i32),
    #[error("{0}")]
    InvalidEnumDefault(#[from] Box<InvalidEnumDefaultError>),
}

#[derive(Debug, Error)]
#[error("enum '{enum_name}' in column '{table_name}.{column_name}' has invalid {value_type} value '{value}': not in allowed values [{allowed}]")]
pub struct InvalidEnumDefaultError {
    pub enum_name: String,
    pub table_name: String,
    pub column_name: String,
    pub value_type: String,
    pub value: String,
    pub allowed: String,
}
