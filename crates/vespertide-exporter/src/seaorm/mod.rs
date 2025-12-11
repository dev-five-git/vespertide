use std::collections::HashSet;

use crate::orm::OrmExporter;
use vespertide_core::{ColumnDef, ColumnType, IndexDef, TableConstraint, TableDef};

pub struct SeaOrmExporter;

impl OrmExporter for SeaOrmExporter {
    fn render_entity(&self, table: &TableDef) -> Result<String, String> {
        Ok(render_entity(table))
    }
}

/// Render a single table into SeaORM entity code.
///
/// Follows the official entity format:
/// <https://www.sea-ql.org/SeaORM/docs/generate-entity/entity-format/>
pub fn render_entity(table: &TableDef) -> String {
    let primary_keys = primary_key_columns(table);
    let composite_pk = primary_keys.len() > 1;
    let indexes = &table.indexes;

    let mut lines: Vec<String> = Vec::new();
    lines.push("use sea_orm::entity::prelude::*;".into());
    lines.push(String::new());
    lines.push("#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]".into());
    lines.push(format!("#[sea_orm(table_name = \"{}\")]", table.name));
    lines.push("pub struct Model {".into());

    for column in &table.columns {
        render_column(&mut lines, column, &primary_keys, composite_pk);
    }

    lines.push("}".into());
    lines.push(String::new());
    lines.push("impl ActiveModelBehavior for ActiveModel {}".into());

    // Relations and indexes
    lines.push(String::new());
    render_relations(&mut lines, table);
    render_indexes(&mut lines, indexes);

    lines.join("\n")
}

fn render_column(
    lines: &mut Vec<String>,
    column: &ColumnDef,
    primary_keys: &HashSet<String>,
    composite_pk: bool,
) {
    if primary_keys.contains(&column.name) {
        if composite_pk {
            lines.push("    #[sea_orm(primary_key, auto_increment = false)]".into());
        } else {
            lines.push("    #[sea_orm(primary_key)]".into());
        }
    }

    let field_name = sanitize_field_name(&column.name);
    let ty = rust_type(&column.r#type, column.nullable);
    lines.push(format!("    pub {}: {},", field_name, ty));
}

fn primary_key_columns(table: &TableDef) -> HashSet<String> {
    let mut keys = HashSet::new();
    for constraint in &table.constraints {
        if let TableConstraint::PrimaryKey(cols) = constraint {
            for col in cols {
                keys.insert(col.clone());
            }
        }
    }
    keys
}

fn render_relations(lines: &mut Vec<String>, table: &TableDef) {
    let foreign_keys: Vec<_> = table
        .constraints
        .iter()
        .filter_map(|c| {
            if let TableConstraint::ForeignKey {
                columns,
                ref_table,
                ref_columns,
                ..
            } = c
            {
                Some((columns.clone(), ref_table.clone(), ref_columns.clone()))
            } else {
                None
            }
        })
        .collect();

    if foreign_keys.is_empty() {
        return;
    }

    lines.push("#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]".into());
    lines.push("pub enum Relation {".into());
    for (idx, (_, ref_table, _)) in foreign_keys.iter().enumerate() {
        let variant = format!("    Ref{idx}, // to {ref_table}");
        lines.push(variant);
    }
    lines.push("}".into());

    lines.push(String::new());
    lines.push("impl Related<Entity> for Relation {}".into());

    // RelationDef builder
    lines.push(String::new());
    lines.push("impl RelationTrait for Relation {".into());
    lines.push("    fn def(&self) -> RelationDef {".into());
    lines.push("        match self {".into());

    for (idx, (columns, ref_table, ref_columns)) in foreign_keys.iter().enumerate() {
        let from_cols = columns.join(", ");
        let to_cols = ref_columns.join(", ");
        lines.push(format!(
            "            Relation::Ref{idx} => Entity::has_many(super::{ref_table}::Entity).from(Column::{}).to(super::{ref_table}::Column::{}),",
            columns.first().cloned().unwrap_or_default(),
            ref_columns.first().cloned().unwrap_or_default()
        ));
        if columns.len() > 1 || ref_columns.len() > 1 {
            lines.push(format!(
                "            // composite FK from [{from_cols}] to {ref_table}.[{to_cols}]"
            ));
        }
    }

    lines.push("        }".into());
    lines.push("    }".into());
    lines.push("}".into());
}

fn render_indexes(lines: &mut Vec<String>, indexes: &[IndexDef]) {
    if indexes.is_empty() {
        return;
    }
    lines.push(String::new());
    lines.push("// Index definitions (SeaORM uses Statement builders externally)".into());
    for idx in indexes {
        let cols = idx.columns.join(", ");
        lines.push(format!(
            "// {} on [{}] unique={}",
            idx.name, cols, idx.unique
        ));
    }
}

fn rust_type(column_type: &ColumnType, nullable: bool) -> String {
    let base = match column_type {
        ColumnType::Integer => "i32".to_string(),
        ColumnType::BigInt => "i64".to_string(),
        ColumnType::Text => "String".to_string(),
        ColumnType::Boolean => "bool".to_string(),
        ColumnType::Timestamp => "DateTimeWithTimeZone".to_string(),
        ColumnType::Custom(custom) => custom.clone(),
    };

    if nullable {
        format!("Option<{}>", base)
    } else {
        base
    }
}

fn sanitize_field_name(name: &str) -> String {
    let mut result = String::new();

    for (idx, ch) in name.chars().enumerate() {
        if (ch.is_ascii_alphanumeric() && (idx > 0 || ch.is_ascii_alphabetic()))
            || ch == '_'
        {
            result.push(ch);
        } else if idx == 0 && ch.is_ascii_digit() {
            result.push('_');
            result.push(ch);
        } else {
            result.push('_');
        }
    }

    if result.is_empty() {
        "_col".into()
    } else {
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_table() -> TableDef {
        TableDef {
            name: "users".into(),
            columns: vec![
                ColumnDef {
                    name: "id".into(),
                    r#type: ColumnType::Integer,
                    nullable: false,
                    default: None,
                },
                ColumnDef {
                    name: "display_name".into(),
                    r#type: ColumnType::Text,
                    nullable: true,
                    default: None,
                },
            ],
            constraints: vec![TableConstraint::PrimaryKey(vec!["id".into()])],
            indexes: vec![],
        }
    }

    #[test]
    fn render_entity_outputs_basic_model() {
        let table = base_table();
        let rendered = render_entity(&table);

        assert!(rendered.contains("#[sea_orm(table_name = \"users\")]"));
        assert!(rendered.contains("#[sea_orm(primary_key)]"));
        assert!(rendered.contains("pub id: i32,"));
        assert!(rendered.contains("pub display_name: Option<String>,"));
        assert!(rendered.contains("impl ActiveModelBehavior for ActiveModel"));
    }

    #[test]
    fn render_entity_marks_composite_primary_keys() {
        let mut table = base_table();
        table.columns.push(ColumnDef {
            name: "tenant_id".into(),
            r#type: ColumnType::BigInt,
            nullable: false,
            default: None,
        });
        table.constraints = vec![TableConstraint::PrimaryKey(vec![
            "id".into(),
            "tenant_id".into(),
        ])];

        let rendered = render_entity(&table);
        let pk_lines: Vec<_> = rendered.lines().filter(|line| line.contains("primary_key")).collect();

        // composite PK should disable auto increment
        assert!(pk_lines
            .iter()
            .all(|line| line.contains("auto_increment = false")));
        assert!(rendered.contains("pub tenant_id: i64,"));
    }

    #[test]
    fn render_entity_handles_multiple_tables_individually() {
        let tables = vec![
            base_table(),
            TableDef {
                name: "posts".into(),
                columns: vec![ColumnDef {
                    name: "id".into(),
                    r#type: ColumnType::Integer,
                    nullable: false,
                    default: None,
                }],
                constraints: vec![TableConstraint::PrimaryKey(vec!["id".into()])],
                indexes: vec![],
            },
        ];

        let rendered: Vec<_> = tables.iter().map(render_entity).collect();
        assert_eq!(rendered.len(), 2);
        assert!(rendered
            .iter()
            .any(|code| code.contains("#[sea_orm(table_name = \"users\")]")));
        assert!(rendered
            .iter()
            .any(|code| code.contains("#[sea_orm(table_name = \"posts\")]")));
    }
}
