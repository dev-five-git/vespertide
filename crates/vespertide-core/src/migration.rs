#[derive(Debug, Clone)]
pub struct MigrationOptions {
    pub version_table: String,
}

#[derive(thiserror::Error, Debug)]
pub enum MigrationError {
    #[error("migration execution is not yet implemented")]
    NotImplemented,
    #[error("database error: {0}")]
    DatabaseError(String),
    #[error(
        "migration id mismatch for version {version}: expected '{expected}', found '{found}' in database"
    )]
    IdMismatch {
        version: u32,
        expected: String,
        found: String,
    },
}
