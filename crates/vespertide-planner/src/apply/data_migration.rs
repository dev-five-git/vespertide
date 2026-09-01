/// `DataMigration` changes rows, never schema, so replay has nothing to apply.
///
/// This is a *different* reason from the sibling [`super::raw_sql`] no-op.
/// `RawSql` is skipped because its effect on the schema is **unknown** — any
/// DDL it performed is silently lost from the reconstructed baseline.
/// `DataMigration` is skipped because **changing no schema is its contract**,
/// enforced by the DDL guard in
/// [`crate::validate::validate_migration_plan`]. Replaying a history that
/// contains one therefore yields a schema identical to the one before it.
pub(super) const fn apply_data_migration() {}
