use crate::MigrationOptions;

#[derive(thiserror::Error, Debug)]
pub enum MigrationError {
    #[error("migration execution is not yet implemented")]
    NotImplemented,
}

pub async fn run_migrations<P>(_pool: P, _options: MigrationOptions) -> Result<(), MigrationError> {
    // TODO: Generate and execute migration SQL from JSON/YAML plans.
    Err(MigrationError::NotImplemented)
}
