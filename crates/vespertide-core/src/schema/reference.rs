use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReferenceAction {
    Cascade,
    Restrict,
    SetNull,
    SetDefault,
    NoAction,
}

