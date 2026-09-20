//! Shared enum-column scans for the renderers that put a whole schema into
//! one scope.
//!
//! Backends that write one file per table get enum scoping for free; Prisma,
//! Drizzle, GORM and Django do not, and all start from the same per-table
//! scan. What they do with it differs — Prisma deduplicates identifiers
//! globally (see `prisma::enums`), Drizzle table-prefixes every type, GORM and
//! Django claim them in the file's one scope (see `scope_names`).

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

/// An enum's variant names in declaration order: the values of a string
/// enum, the member names of an integer one.
pub(crate) fn variant_names(values: &EnumValues) -> Vec<&str> {
    match values {
        EnumValues::String(values) => values.iter().map(String::as_str).collect(),
        EnumValues::Integer(values) => values.iter().map(|v| v.name.as_str()).collect(),
    }
}
