mod enums;
mod render;
mod types;

use crate::orm::OrmExporter;
use vespertide_core::TableDef;

pub use render::{render_entities, render_entity};

pub struct SqlModelExporter;

impl OrmExporter for SqlModelExporter {
    fn render_entity(&self, table: &TableDef) -> Result<String, String> {
        render_entity(table)
    }
}

/// Test-only accessor for the `SQLModel` `to_pascal_case` helper.
///
/// Allows the cross-ORM consolidation test in `crate::tests` to exercise
/// the `SQLModel` naming helper without making it `pub(crate)` for the
/// entire crate.
#[cfg(test)]
pub(crate) fn to_pascal_case_for_tests(s: &str) -> String {
    enums::to_pascal_case(s)
}

#[cfg(test)]
mod tests {
    use super::types::UsedTypes;
    use vespertide_core::{ColumnType, ComplexColumnType, SimpleColumnType};

    #[test]
    fn test_used_types_tracks_import_needs() {
        let mut used = UsedTypes::default();
        used.add_column_type(&ColumnType::Simple(SimpleColumnType::Date), false);
        used.add_column_type(&ColumnType::Simple(SimpleColumnType::Time), false);
        used.add_column_type(&ColumnType::Simple(SimpleColumnType::Timestamp), false);
        used.add_column_type(&ColumnType::Simple(SimpleColumnType::Uuid), false);
        used.add_column_type(
            &ColumnType::Complex(ComplexColumnType::Numeric {
                precision: 10,
                scale: 2,
            }),
            false,
        );
        used.add_column_type(&ColumnType::Simple(SimpleColumnType::Integer), true);

        assert!(used.datetime_types.contains("date"));
        assert!(used.datetime_types.contains("time"));
        assert!(used.datetime_types.contains("datetime"));
        assert!(used.needs_uuid);
        assert!(used.needs_decimal);
        assert!(used.needs_optional);
    }

    #[test]
    fn test_used_types_other_simple_types_fallthrough() {
        let mut used = UsedTypes::default();
        used.add_column_type(&ColumnType::Simple(SimpleColumnType::Integer), false);
        assert!(used.datetime_types.is_empty());
        assert!(!used.needs_uuid);
        assert!(!used.needs_decimal);
    }
}
