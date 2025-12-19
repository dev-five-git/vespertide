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
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use insta::{assert_snapshot, with_settings};
    use rstest::rstest;
    use vespertide_core::{ColumnDef, ColumnType, SimpleColumnType, TableConstraint};

    fn simple_table() -> TableDef {
        TableDef {
            name: "test".into(),
            columns: vec![ColumnDef {
                name: "id".into(),
                r#type: ColumnType::Simple(SimpleColumnType::Integer),
                nullable: false,
                default: None,
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            }],
            constraints: vec![TableConstraint::PrimaryKey {
                auto_increment: false,
                columns: vec!["id".into()],
            }],
        }
    }

    #[rstest]
    #[case("seaorm", Orm::SeaOrm)]
    #[case("sqlalchemy", Orm::SqlAlchemy)]
    #[case("sqlmodel", Orm::SqlModel)]
    fn test_render_entity_snapshots(#[case] name: &str, #[case] orm: Orm) {
        let table = simple_table();
        let result = render_entity(orm, &table);
        assert!(result.is_ok());
        with_settings!({ snapshot_suffix => name }, {
            assert_snapshot!(result.unwrap());
        });
    }

    #[rstest]
    #[case("seaorm", Orm::SeaOrm)]
    #[case("sqlalchemy", Orm::SqlAlchemy)]
    #[case("sqlmodel", Orm::SqlModel)]
    fn test_render_entity_with_schema_snapshots(#[case] name: &str, #[case] orm: Orm) {
        let table = simple_table();
        let schema = vec![table.clone()];
        let result = render_entity_with_schema(orm, &table, &schema);
        assert!(result.is_ok());
        with_settings!({ snapshot_suffix => name }, {
            assert_snapshot!(result.unwrap());
        });
    }
}
