//! SQL generator for `MigrationAction::RemapEnumValues`.
//!
//! Integer-backed enums in vespertide are stored as plain `INTEGER` on
//! every supported backend. When the user changes a variant's numeric
//! `value` (e.g. `medium: 5 → 10`), the DB does not detect the drift and
//! the next ORM-side deserialisation silently re-interprets every
//! existing row. This module emits a single atomic `UPDATE ... SET col =
//! CASE WHEN ... END WHERE col IN (...)` that re-stamps affected rows
//! before the new ORM mapping takes effect.
//!
//! The SQL is portable across `PostgreSQL`, `MySQL` and `SQLite` —
//! `CASE WHEN` and `IN (...)` are universal.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use super::helpers::quote_ident;
use super::types::{BuiltQuery, DatabaseBackend, RawSql};
use crate::error::QueryError;

/// Build a single atomic UPDATE that re-stamps every row whose stored
/// integer value is being remapped. Empty `mapping` yields `Ok(vec![])`
/// so callers can blindly forward zero-change actions without special
/// casing them.
pub fn build_remap_enum_values(
    backend: DatabaseBackend,
    table: &str,
    column: &str,
    mapping: &BTreeMap<i64, i64>,
) -> Result<Vec<BuiltQuery>, QueryError> {
    if mapping.is_empty() {
        return Ok(vec![]);
    }
    let qt = quote_ident(table, backend);
    let qc = quote_ident(column, backend);
    // Write the CASE arms and the IN list into two reusable buffers in a
    // single pass over `mapping` — no intermediate `Vec<String>` per pair.
    let mut arms = String::new();
    let mut in_list = String::new();
    for (old, new) in mapping {
        if !arms.is_empty() {
            arms.push(' ');
        }
        let _ = write!(arms, "WHEN {qc} = {old} THEN {new}");
        if !in_list.is_empty() {
            in_list.push_str(", ");
        }
        let _ = write!(in_list, "{old}");
    }
    let sql = format!("UPDATE {qt} SET {qc} = CASE {arms} END WHERE {qc} IN ({in_list})");
    Ok(vec![BuiltQuery::Raw(RawSql::uniform(sql))])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::backend_tag;
    use insta::{assert_snapshot, with_settings};

    fn run(backend: DatabaseBackend, mapping: &[(i64, i64)]) -> String {
        let map: BTreeMap<i64, i64> = mapping.iter().copied().collect();
        build_remap_enum_values(backend, "tickets", "priority", &map)
            .expect("supported")
            .iter()
            .map(|q| q.build(backend))
            .collect::<Vec<_>>()
            .join(";\n")
    }

    fn snap(name: &str, sql: &str) {
        // This module lives at `sql/remap_enum_values.rs`, one level above
        // the snapshot directory (unlike `modify_column_type/*` which is
        // two levels deep). Use `./snapshots` instead of `../snapshots`
        // so insta resolves the shared `sql/snapshots/` location.
        with_settings!(
            { snapshot_path => "./snapshots", snapshot_suffix => format!("remap_{}", name) },
            { assert_snapshot!(sql); }
        );
    }

    /// Each test case fans out across `PG` / `MySQL` / `SQLite` so snapshots
    /// always come in 3-of-a-kind triples (same convention as F20).
    macro_rules! remap_all_backends {
        ($name:ident, $mapping:expr) => {
            #[test]
            fn $name() {
                for backend in [
                    DatabaseBackend::Postgres,
                    DatabaseBackend::MySql,
                    DatabaseBackend::Sqlite,
                ] {
                    let tag = backend_tag(backend);
                    let sql = run(backend, $mapping);
                    snap(&format!("{}_{}", stringify!($name), tag), &sql);
                }
            }
        };
    }

    // Single pair — the simplest case (medium: 5 -> 100).
    remap_all_backends!(single_pair, &[(5_i64, 100_i64)]);

    // Multi pair — medium + high shifted to new values.
    remap_all_backends!(multi_pair, &[(5_i64, 100_i64), (10_i64, 200_i64)]);

    // Atomic value swap — high/medium exchange. CASE WHEN reads pre-swap
    // values for every row, so this is safe in a single statement.
    remap_all_backends!(value_swap, &[(5_i64, 10_i64), (10_i64, 5_i64)]);

    // Negative-value remap (e.g. status codes from -1 to 0).
    remap_all_backends!(negative_values, &[(-1_i64, 0_i64), (0_i64, 1_i64)]);

    #[test]
    fn empty_mapping_emits_no_query() {
        let queries = build_remap_enum_values(
            DatabaseBackend::Postgres,
            "tickets",
            "priority",
            &BTreeMap::new(),
        )
        .expect("empty mapping is a valid no-op");
        assert!(queries.is_empty());
    }
}
