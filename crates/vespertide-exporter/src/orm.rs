use vespertide_core::TableDef;

use crate::{
    jpa::JpaExporter, seaorm::SeaOrmExporter, sqlalchemy::SqlAlchemyExporter,
    sqlmodel::SqlModelExporter,
};

/// Supported ORM targets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Orm {
    SeaOrm,
    SqlAlchemy,
    SqlModel,
    Jpa,
}

/// Standardized exporter interface for all supported ORMs.
pub trait OrmExporter {
    fn render_entity(&self, table: &TableDef) -> Result<String, String>;

    /// Render entity with schema context for FK chain resolution.
    /// Default implementation ignores schema context.
    fn render_entity_with_schema(
        &self,
        table: &TableDef,
        _schema: &[TableDef],
    ) -> Result<String, String> {
        self.render_entity(table)
    }
}

/// Render a single table definition for the selected ORM.
pub fn render_entity(orm: Orm, table: &TableDef) -> Result<String, String> {
    match orm {
        Orm::SeaOrm => SeaOrmExporter.render_entity(table),
        Orm::SqlAlchemy => SqlAlchemyExporter.render_entity(table),
        Orm::SqlModel => SqlModelExporter.render_entity(table),
        Orm::Jpa => JpaExporter.render_entity(table),
    }
}

/// Render a single table definition with full schema context for FK chain resolution.
pub fn render_entity_with_schema(
    orm: Orm,
    table: &TableDef,
    schema: &[TableDef],
) -> Result<String, String> {
    match orm {
        Orm::SeaOrm => SeaOrmExporter.render_entity_with_schema(table, schema),
        Orm::SqlAlchemy => SqlAlchemyExporter.render_entity_with_schema(table, schema),
        Orm::SqlModel => SqlModelExporter.render_entity_with_schema(table, schema),
        Orm::Jpa => JpaExporter.render_entity_with_schema(table, schema),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::fixtures::basic_single_pk;
    use rstest::rstest;

    #[rstest]
    #[case::seaorm(Orm::SeaOrm)]
    #[case::sqlalchemy(Orm::SqlAlchemy)]
    #[case::sqlmodel(Orm::SqlModel)]
    #[case::jpa(Orm::Jpa)]
    fn dispatch_render_entity_succeeds(#[case] orm: Orm) {
        let table = basic_single_pk();
        assert!(render_entity(orm, &table).is_ok());
    }

    #[rstest]
    #[case::seaorm(Orm::SeaOrm)]
    #[case::sqlalchemy(Orm::SqlAlchemy)]
    #[case::sqlmodel(Orm::SqlModel)]
    #[case::jpa(Orm::Jpa)]
    fn dispatch_render_entity_with_schema_succeeds(#[case] orm: Orm) {
        let table = basic_single_pk();
        let schema = vec![table.clone()];
        assert!(render_entity_with_schema(orm, &table, &schema).is_ok());
    }
}
