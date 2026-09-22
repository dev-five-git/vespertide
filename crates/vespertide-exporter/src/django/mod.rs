mod enums;
mod render;
mod types;

use crate::orm::OrmExporter;
use vespertide_config::DjangoConfig;
use vespertide_core::TableDef;

pub use render::{export, export_with_config, render_entity, render_entity_with_schema};

pub struct DjangoExporter;

impl OrmExporter for DjangoExporter {
    fn render_entity(&self, table: &TableDef) -> Result<String, String> {
        render_entity(table)
    }

    fn render_entity_with_schema(
        &self,
        table: &TableDef,
        schema: &[TableDef],
    ) -> Result<String, String> {
        render_entity_with_schema(table, schema)
    }
}

/// Django exporter that honors `vespertide.json`'s `django` config section
/// (currently an optional `app_label` written into every model's `Meta`
/// class). Mirrors `seaorm::SeaOrmExporterWithConfig`.
pub struct DjangoExporterWithConfig<'a> {
    config: &'a DjangoConfig,
}

impl<'a> DjangoExporterWithConfig<'a> {
    pub fn new(config: &'a DjangoConfig) -> Self {
        Self { config }
    }

    /// [`export`] with the configured `app_label`.
    pub fn export(&self, schema: &[TableDef]) -> Result<String, String> {
        export_with_config(schema, self.config.app_label())
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use vespertide_core::ReferenceAction;
    use vespertide_core::schema::column::{ColumnType, SimpleColumnType};

    use super::types::{UsedImports, build_default, django_field_type, on_delete_for};

    /// A SQL string default becomes the Python value it spells: the outer
    /// quotes go, a doubled `''` is one quote, and what Python's literal
    /// cannot hold is escaped.
    #[rstest]
    #[case::plain("'draft'", r#""draft""#)]
    #[case::doubled_sql_quote("'it''s'", r#""it's""#)]
    #[case::double_quote_inside(r#"'say "hi"'"#, r#""say \"hi\"""#)]
    #[case::backslash(r"'a\b'", r#""a\\b""#)]
    #[case::empty("''", r#""""#)]
    fn string_defaults_become_python_literals(#[case] sql: &str, #[case] expected: &str) {
        let text = ColumnType::Simple(SimpleColumnType::Text);
        let default = build_default(&text, sql, &mut UsedImports::default());
        assert_eq!(default.as_deref(), Some(expected));
    }

    #[test]
    fn a_json_default_is_left_to_the_database() {
        let json = ColumnType::Simple(SimpleColumnType::Json);
        let default = build_default(&json, r#"'{"a": 1}'"#, &mut UsedImports::default());
        assert_eq!(default, None);
    }

    #[rstest]
    #[case::small_int(SimpleColumnType::SmallInt, "models.SmallIntegerField")]
    #[case::integer(SimpleColumnType::Integer, "models.IntegerField")]
    #[case::big_int(SimpleColumnType::BigInt, "models.BigIntegerField")]
    #[case::real(SimpleColumnType::Real, "models.FloatField")]
    #[case::double_precision(SimpleColumnType::DoublePrecision, "models.FloatField")]
    #[case::text(SimpleColumnType::Text, "models.TextField")]
    #[case::xml(SimpleColumnType::Xml, "models.TextField")]
    #[case::boolean(SimpleColumnType::Boolean, "models.BooleanField")]
    #[case::date(SimpleColumnType::Date, "models.DateField")]
    #[case::time(SimpleColumnType::Time, "models.TimeField")]
    #[case::timestamp(SimpleColumnType::Timestamp, "models.DateTimeField")]
    #[case::timestamptz(SimpleColumnType::Timestamptz, "models.DateTimeField")]
    #[case::interval(SimpleColumnType::Interval, "models.DurationField")]
    #[case::bytea(SimpleColumnType::Bytea, "models.BinaryField")]
    #[case::uuid(SimpleColumnType::Uuid, "models.UUIDField")]
    #[case::json(SimpleColumnType::Json, "models.JSONField")]
    #[case::inet(SimpleColumnType::Inet, "models.GenericIPAddressField")]
    #[case::cidr(SimpleColumnType::Cidr, "models.GenericIPAddressField")]
    #[case::macaddr(SimpleColumnType::Macaddr, "models.CharField")]
    fn simple_types_map_to_field_classes(#[case] ty: SimpleColumnType, #[case] expected: &str) {
        assert_eq!(
            django_field_type(&ColumnType::Simple(ty), false, false),
            expected
        );
    }

    #[rstest]
    #[case::small_int(SimpleColumnType::SmallInt, "models.SmallAutoField")]
    #[case::integer(SimpleColumnType::Integer, "models.AutoField")]
    #[case::big_int(SimpleColumnType::BigInt, "models.BigAutoField")]
    fn auto_increment_primary_keys_map_to_auto_fields(
        #[case] ty: SimpleColumnType,
        #[case] expected: &str,
    ) {
        assert_eq!(
            django_field_type(&ColumnType::Simple(ty), true, true),
            expected
        );
    }

    #[rstest]
    #[case::cascade(Some(ReferenceAction::Cascade), true, true, "models.CASCADE")]
    #[case::restrict(Some(ReferenceAction::Restrict), true, true, "models.RESTRICT")]
    #[case::set_null(Some(ReferenceAction::SetNull), true, true, "models.SET_NULL")]
    #[case::set_default(Some(ReferenceAction::SetDefault), true, true, "models.SET_DEFAULT")]
    #[case::no_action(Some(ReferenceAction::NoAction), true, true, "models.DO_NOTHING")]
    #[case::no_action_given(None, true, true, "models.RESTRICT")]
    #[case::set_null_on_a_field_that_is_not_null(
        Some(ReferenceAction::SetNull),
        true,
        false,
        "models.DO_NOTHING"
    )]
    #[case::set_default_without_a_default(
        Some(ReferenceAction::SetDefault),
        false,
        true,
        "models.DO_NOTHING"
    )]
    fn reference_actions_map_to_on_delete(
        #[case] action: Option<ReferenceAction>,
        #[case] has_default: bool,
        #[case] null: bool,
        #[case] expected: &str,
    ) {
        assert_eq!(on_delete_for(action.as_ref(), has_default, null), expected);
    }
}
