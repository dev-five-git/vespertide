mod enums;
mod render;
mod types;

use std::collections::HashSet;

use crate::orm::OrmExporter;
use vespertide_core::TableDef;

pub struct PrismaExporter;

impl OrmExporter for PrismaExporter {
    fn render_entity(&self, table: &TableDef) -> Result<String, String> {
        // A lone table is its own schema: Prisma requires both ends of a
        // relation in the file, including self-referential ones.
        Ok(render_entity_with_schema(
            table,
            std::slice::from_ref(table),
        ))
    }

    fn render_entity_with_schema(
        &self,
        table: &TableDef,
        schema: &[TableDef],
    ) -> Result<String, String> {
        Ok(render_entity_with_schema(table, schema))
    }
}

/// Render every table into one Prisma schema file.
///
/// Output order: (globally deduped) enum blocks → model blocks.
///
/// No `datasource` or `generator` block is emitted: those describe the user's
/// project rather than their schema, and pinning a `provider` would make the
/// output backend-specific. Users pair this file with their own via Prisma's
/// multi-file schema directory, exactly as the other backends emit models only.
pub fn render_schema(tables: &[TableDef]) -> String {
    let ambiguous = enums::ambiguous_enum_identifiers(tables);
    let mut seen_enums: HashSet<String> = HashSet::new();
    let mut parts: Vec<String> = Vec::new();
    for table in tables {
        for (name, values) in enums::collect_table_enums(table) {
            let identifier = enums::enum_identifier(table.name.as_str(), name, &ambiguous);
            if seen_enums.insert(identifier.clone()) {
                parts.push(enums::render_enum(&identifier, values));
            }
        }
    }

    for table in tables {
        parts.push(render::render_model(table, tables, &ambiguous));
    }

    parts.join("\n\n") + "\n"
}

/// Render enum blocks + model block with full schema context (includes back-relations).
pub fn render_entity_with_schema(table: &TableDef, schema: &[TableDef]) -> String {
    let ambiguous = enums::ambiguous_enum_identifiers(schema);
    let mut parts: Vec<String> = Vec::new();
    for (name, values) in enums::collect_table_enums(table) {
        let identifier = enums::enum_identifier(table.name.as_str(), name, &ambiguous);
        parts.push(enums::render_enum(&identifier, values));
    }
    parts.push(render::render_model(table, schema, &ambiguous));
    parts.join("\n\n")
}

/// Multi-table entry point: render every table (enum + model blocks) with full
/// schema context and join them. Mirrors the other ORMs' `export` so the
/// cross-ORM test harness can dispatch Prisma through a single call. Unlike
/// [`render_schema`], enums are deduplicated per table rather than globally.
pub fn export(schema: &[TableDef]) -> Result<String, String> {
    Ok(schema
        .iter()
        .map(|table| render_entity_with_schema(table, schema))
        .collect::<Vec<_>>()
        .join("\n\n"))
}

#[cfg(test)]
mod tests {
    use insta::{assert_snapshot, with_settings};
    use rstest::rstest;

    use super::*;
    use crate::tests::fixtures::basic_single_pk;

    /// The file must stay usable under any provider, so it carries neither a
    /// `datasource`/`generator` block nor a backend-specific `@db.*` attribute.
    #[test]
    fn render_schema_emits_models_only() {
        let tables = vec![basic_single_pk()];
        let schema = render_schema(&tables);
        with_settings!({ snapshot_path => "../tests/snapshots" }, {
            assert_snapshot!(schema);
        });
    }

    #[test]
    fn render_schema_emits_shared_enum_block_once() {
        let t1 = crate::tests::fixtures::enum_shared();
        let mut t2 = crate::tests::fixtures::enum_shared();
        t2.name = "archived_documents".into();
        let schema = render_schema(&[t1, t2]);
        with_settings!({ snapshot_path => "../tests/snapshots" }, {
            assert_snapshot!(schema);
        });
    }

    /// Two tables may declare the same enum name with different values; the SQL
    /// layer keeps them apart as `{table}_{enum}` types, and a single Prisma file
    /// must do the same or both columns silently get the first table's values.
    /// The clash is judged after `PascalCase` conversion, so declared names that
    /// only collapse onto one identifier are split the same way.
    #[rstest]
    #[case::same_declared_name("status", "status")]
    #[case::names_collapse_after_conversion("doc_status", "docStatus")]
    fn render_schema_qualifies_ambiguous_enums(
        #[case] orders_enum: &str,
        #[case] tickets_enum: &str,
    ) {
        let orders = table_with_enum("orders", orders_enum, &["new", "paid"]);
        let tickets = table_with_enum("tickets", tickets_enum, &["open", "closed"]);
        let schema = render_schema(&[orders, tickets]);
        with_settings!(
            { snapshot_path => "../tests/snapshots", snapshot_suffix => format!("{orders_enum}_{tickets_enum}") },
            { assert_snapshot!(schema); }
        );
    }

    fn table_with_enum(name: &str, enum_name: &str, values: &[&str]) -> TableDef {
        use vespertide_core::schema::column::{ColumnType, ComplexColumnType, EnumValues};
        use vespertide_core::schema::primary_key::PrimaryKeySyntax;
        use vespertide_core::{ColumnDef, SimpleColumnType};

        TableDef {
            name: name.into(),
            description: None,
            columns: vec![
                ColumnDef::new("id", ColumnType::Simple(SimpleColumnType::Integer), false)
                    .primary_key(PrimaryKeySyntax::Bool(true)),
                ColumnDef::new(
                    "st",
                    ColumnType::Complex(ComplexColumnType::Enum {
                        name: enum_name.into(),
                        values: EnumValues::String(
                            values.iter().copied().map(Into::into).collect(),
                        ),
                    }),
                    false,
                ),
            ],
            constraints: vec![],
        }
        .normalize()
        .expect("fixture table normalizes")
    }
}
