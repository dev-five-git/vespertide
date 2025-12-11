use crate::orm::OrmExporter;
use vespertide_core::TableDef;

pub struct SqlModelExporter;

impl OrmExporter for SqlModelExporter {
    fn render_entity(&self, table: &TableDef) -> Result<String, String> {
        // Placeholder: replace with real SQLModel generation
        Ok(format!("# SQLModel placeholder for {}", table.name))
    }
}
