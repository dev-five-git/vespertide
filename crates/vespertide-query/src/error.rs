use thiserror::Error;

#[derive(Debug, Error)]
pub enum QueryError {
    #[error("unsupported table constraint")]
    UnsupportedConstraint,
}

