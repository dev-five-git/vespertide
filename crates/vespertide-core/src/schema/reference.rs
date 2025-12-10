use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum ReferenceAction {
    Cascade,
    Restrict,
    SetNull,
    SetDefault,
    NoAction,
}
