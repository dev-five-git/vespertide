use vespertide_core::MigrationPlan;

use crate::error::QueryError;
use crate::sql::{build_action_queries, BuiltQuery};

pub fn build_plan_queries(plan: &MigrationPlan) -> Result<Vec<BuiltQuery>, QueryError> {
    let mut queries: Vec<BuiltQuery> = Vec::new();
    for action in &plan.actions {
        queries.extend(build_action_queries(action)?);
    }
    Ok(queries)
}

