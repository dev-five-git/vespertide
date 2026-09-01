//! Core data structures for vespertide schema definition and migration planning.
//!
//! - [`TableDef`], [`ColumnDef`]: schema model
//! - [`MigrationAction`], [`MigrationPlan`]: typed migration operations
//! - [`MigrationError`]: runtime migration error type

pub mod action;
#[cfg(feature = "arbitrary")]
pub mod arbitrary;
pub mod migration;
pub mod schema;
pub mod sql_escape;

pub use action::{
    DataMigrationSql, MigrationAction, MigrationPlan, NarrowingStrategy, leading_ddl_keyword,
};
pub use migration::{MigrationError, MigrationOptions};
pub use schema::{
    CheckViolationStrategy, ColumnDef, ColumnName, ColumnType, ComplexColumnType, ConstraintKind,
    DefaultValue, EnumValues, ForeignKeyOrphanStrategy, IndexDef, IndexName, KeepPolicy, NumValue,
    PrimaryKeyAdditionStrategy, ReferenceAction, SimpleColumnType, StrOrBoolOrArray, StringOrBool,
    TableConstraint, TableDef, TableName, TableValidationError, UniqueConstraintStrategy,
};
pub use sql_escape::escape_sql_string_literal;
