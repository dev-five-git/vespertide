/// `RawSql` is an emergency escape hatch for side-effect-only SQL.
///
/// Baseline replay is intentionally typed and in-memory, so arbitrary SQL is
/// opaque here and cannot be reflected into the reconstructed schema snapshot.
pub(super) const fn apply_raw_sql() {}
