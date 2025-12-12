use thiserror::Error;

#[derive(Debug, Error)]
pub enum QueryError {
    #[error("unsupported table constraint")]
    UnsupportedConstraint,
    #[error("{0}")]
    Other(String),
}
