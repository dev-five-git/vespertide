use crate::orm::OrmExporter;
use vespertide_core::TableDef;

pub struct SqlAlchemyExporter;

impl OrmExporter for SqlAlchemyExporter {
    fn render_entity(&self, table: &TableDef) -> Result<String, String> {
        // Placeholder: replace with real SQLAlchemy generation
        Ok(format!("# SQLAlchemy model placeholder for {}", table.name))
    }
}
