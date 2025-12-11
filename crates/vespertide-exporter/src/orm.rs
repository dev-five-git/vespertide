use vespertide_core::TableDef;

use crate::{seaorm::SeaOrmExporter, sqlalchemy::SqlAlchemyExporter, sqlmodel::SqlModelExporter};

/// Supported ORM targets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Orm {
    SeaOrm,
    SqlAlchemy,
    SqlModel,
}

/// Standardized exporter interface for all supported ORMs.
pub trait OrmExporter {
    fn render_entity(&self, table: &TableDef) -> Result<String, String>;
}

/// Render a single table definition for the selected ORM.
pub fn render_entity(orm: Orm, table: &TableDef) -> Result<String, String> {
    match orm {
        Orm::SeaOrm => SeaOrmExporter.render_entity(table),
        Orm::SqlAlchemy => SqlAlchemyExporter.render_entity(table),
        Orm::SqlModel => SqlModelExporter.render_entity(table),
    }
}
