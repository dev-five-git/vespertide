//! SQL generation for `PostgreSQL`, `MySQL`, and `SQLite`.
//!
//! - [`build_plan_queries`]: per-backend SQL for a migration plan
//! - [`build_action_queries`]: per-action SQL with schema context
//! - Backend abstractions live in [`sql::types`]

pub mod builder;
pub mod error;
mod parallel_config;
pub mod sql;
#[cfg(test)]
mod test_support;

pub use builder::{
    PlanQueries, PlanQueriesOptions, build_plan_queries, build_plan_queries_with_options,
};
pub use error::QueryError;
pub use sql::{BuiltQuery, DatabaseBackend, build_action_queries};
