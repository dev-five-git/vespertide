//! Shared enum-column scan for the single-file ORM renderers.
//!
//! Backends that write one file per table get enum scoping for free; Prisma
//! and Drizzle emit one file for the whole schema and both start from the same
//! per-table scan. What they do with it differs — Prisma deduplicates
//! identifiers globally (see `prisma::enums`), Drizzle table-prefixes every
//! type — so only the scan itself lives here.

use vespertide_core::TableDef;
use vespertide_core::schema::column::{ColumnType, ComplexColumnType, EnumValues};

use std::collections::HashSet;

/// Enum columns of a table, first declaration winning per name.
pub(crate) fn collect_table_enums(table: &TableDef) -> Vec<(&str, &EnumValues)> {
    let mut seen = HashSet::new();
    let mut result = Vec::new();
    for col in &table.columns {
        if let ColumnType::Complex(ComplexColumnType::Enum { name, values }) = &col.r#type
            && seen.insert(name.as_str())
        {
            result.push((name.as_str(), values));
        }
    }
    result
}
