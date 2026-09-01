mod render;
mod types;

use crate::orm::OrmExporter;
use crate::parallel_config::{JPA_EXPORT_PAR_TABLE_MIN_LEN, JPA_EXPORT_PAR_TABLE_THRESHOLD};
use rayon::prelude::*;
use vespertide_core::TableDef;

pub struct JpaExporter;

impl OrmExporter for JpaExporter {
    fn render_entity(&self, table: &TableDef) -> Result<String, String> {
        render_entity(table)
    }
}

/// Render a JPA entity for the given table definition.
pub fn render_entity(table: &TableDef) -> Result<String, String> {
    Ok(render::render_entity_inner(table))
}

/// Test-only accessor for the JPA `to_pascal_case` helper.
///
/// Allows the cross-ORM consolidation test in `crate::tests` to exercise
/// the JPA naming helper without making it `pub(crate)` for the entire
/// crate.
#[cfg(test)]
pub(crate) fn to_pascal_case_for_tests(s: &str) -> String {
    render::to_pascal_case(s)
}

/// Render JPA entities for a schema, using parallel rendering for larger schemas.
pub fn render_entities(schema: &[TableDef]) -> Result<Vec<String>, String> {
    let rendered = if schema.len() < JPA_EXPORT_PAR_TABLE_THRESHOLD {
        schema
            .iter()
            .map(render::render_entity_inner)
            .collect::<Vec<_>>()
    } else {
        schema
            .par_iter()
            .with_min_len(JPA_EXPORT_PAR_TABLE_MIN_LEN)
            .map(render::render_entity_inner)
            .collect::<Vec<_>>()
    };

    Ok(rendered)
}

#[cfg(test)]
mod tests {
    use super::render::{infer_fk_field_name, to_camel_case};
    use super::types::column_type_to_java;
    use vespertide_core::{ColumnType, ComplexColumnType, EnumValues};

    #[test]
    fn test_column_type_to_java_string_enum() {
        let ty = ColumnType::Complex(ComplexColumnType::Enum {
            name: "status".into(),
            values: EnumValues::String(vec!["a".into()]),
        });
        assert_eq!(column_type_to_java(&ty), "String");
    }

    /// Direct unit cover of every `column_type_to_java` `SimpleColumnType`
    /// match arm. The all-simple-types fixture exercises these via the
    /// real JPA render pipeline; this rstest pins each arm in isolation so
    /// the coverage tool credits the individual `=> "Foo"` lines (LLVM
    /// source-map attribution can otherwise miss tightly-packed match arms).
    #[rstest::rstest]
    #[case(vespertide_core::SimpleColumnType::SmallInt, "Short")]
    #[case(vespertide_core::SimpleColumnType::Integer, "Integer")]
    #[case(vespertide_core::SimpleColumnType::BigInt, "Long")]
    #[case(vespertide_core::SimpleColumnType::Real, "Float")]
    #[case(vespertide_core::SimpleColumnType::DoublePrecision, "Double")]
    #[case(vespertide_core::SimpleColumnType::Boolean, "Boolean")]
    #[case(vespertide_core::SimpleColumnType::Text, "String")]
    #[case(vespertide_core::SimpleColumnType::Xml, "String")]
    #[case(vespertide_core::SimpleColumnType::Interval, "String")]
    #[case(vespertide_core::SimpleColumnType::Json, "String")]
    #[case(vespertide_core::SimpleColumnType::Inet, "String")]
    #[case(vespertide_core::SimpleColumnType::Cidr, "String")]
    #[case(vespertide_core::SimpleColumnType::Macaddr, "String")]
    #[case(vespertide_core::SimpleColumnType::Date, "LocalDate")]
    #[case(vespertide_core::SimpleColumnType::Time, "LocalTime")]
    #[case(vespertide_core::SimpleColumnType::Timestamp, "LocalDateTime")]
    #[case(vespertide_core::SimpleColumnType::Timestamptz, "OffsetDateTime")]
    #[case(vespertide_core::SimpleColumnType::Bytea, "byte[]")]
    #[case(vespertide_core::SimpleColumnType::Uuid, "UUID")]
    fn column_type_to_java_simple_arm_returns_expected_java_type(
        #[case] ty: vespertide_core::SimpleColumnType,
        #[case] expected: &str,
    ) {
        assert_eq!(column_type_to_java(&ColumnType::Simple(ty)), expected);
    }

    #[rstest::rstest]
    #[case("created_at", "createdAt")]
    #[case("user_id", "userId")]
    #[case("id", "id")]
    #[case("user_profile_image", "userProfileImage")]
    #[case("", "")]
    fn test_to_camel_case(#[case] input: &str, #[case] expected: &str) {
        assert_eq!(to_camel_case(input), expected);
    }

    #[rstest::rstest]
    #[case("customer_id", "customer")]
    #[case("author_user_id", "authorUser")]
    #[case("parent", "parent")]
    fn test_infer_fk_field_name(#[case] input: &str, #[case] expected: &str) {
        assert_eq!(infer_fk_field_name(input), expected);
    }
}
