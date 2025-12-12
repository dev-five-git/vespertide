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
    let relation_fields = relation_field_defs(table);

    let mut lines: Vec<String> = Vec::new();
    lines.push("use sea_orm::entity::prelude::*;".into());
    lines.push(String::new());
    lines.push("#[sea_orm::model]".into());
    lines.push("#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]".into());
    lines.push(format!("#[sea_orm(table_name = \"{}\")]", table.name));
    lines.push("pub struct Model {".into());

    for column in &table.columns {
        render_column(&mut lines, column, &primary_keys, composite_pk);
    }
    for field in relation_fields {
        lines.push(field);
    }

    lines.push("}".into());

    // Indexes (relations expressed as belongs_to fields above)
    lines.push(String::new());
    render_indexes(&mut lines, indexes);

    lines.push("impl ActiveModelBehavior for ActiveModel {}".into());

    lines.push(String::new());

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
        if let TableConstraint::PrimaryKey { columns } = constraint {
            for col in columns {
                keys.insert(col.clone());
            }
        }
    }
    keys
}

fn relation_field_defs(table: &TableDef) -> Vec<String> {
    let mut out = Vec::new();
    let mut used = HashSet::new();
    for constraint in &table.constraints {
        if let TableConstraint::ForeignKey {
            columns,
            ref_table,
            ref_columns,
            ..
        } = constraint
        {
            let base = sanitize_field_name(ref_table);
            let field_name = unique_name(&base, &mut used);
            let from = fk_attr_value(columns);
            let to = fk_attr_value(ref_columns);
            out.push(format!(
                "    #[sea_orm(belongs_to, from = \"{from}\", to = \"{to}\")]"
            ));
            out.push(format!(
                "    pub {field_name}: HasOne<super::{ref_table}::Entity>,"
            ));
        }
    }
    out
}

fn fk_attr_value(cols: &[String]) -> String {
    if cols.len() == 1 {
        cols[0].clone()
    } else {
        format!("({})", cols.join(", "))
    }
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
        if (ch.is_ascii_alphanumeric() && (idx > 0 || ch.is_ascii_alphabetic())) || ch == '_' {
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

fn unique_name(base: &str, used: &mut HashSet<String>) -> String {
    let mut name = base.to_string();
    let mut i = 1;
    while used.contains(&name) {
        name = format!("{base}_{i}");
        i += 1;
    }
    used.insert(name.clone());
    name
}

#[cfg(test)]
mod helper_tests {
    use super::*;
    use vespertide_core::{ColumnType, IndexDef};

    #[test]
    fn test_render_indexes() {
        let mut lines = Vec::new();
        let indexes = vec![
            IndexDef {
                name: "idx_users_email".into(),
                columns: vec!["email".into()],
                unique: false,
            },
            IndexDef {
                name: "idx_users_name_email".into(),
                columns: vec!["name".into(), "email".into()],
                unique: true,
            },
        ];
        render_indexes(&mut lines, &indexes);
        assert!(lines.len() > 0);
        assert!(lines.iter().any(|l| l.contains("idx_users_email")));
        assert!(lines.iter().any(|l| l.contains("idx_users_name_email")));
    }

    #[test]
    fn test_render_indexes_empty() {
        let mut lines = Vec::new();
        render_indexes(&mut lines, &[]);
        // Should not add anything when indexes are empty
        assert_eq!(lines.len(), 0);
    }

    #[test]
    fn test_rust_type() {
        assert_eq!(rust_type(&ColumnType::Integer, false), "i32");
        assert_eq!(rust_type(&ColumnType::Integer, true), "Option<i32>");
        assert_eq!(rust_type(&ColumnType::BigInt, false), "i64");
        assert_eq!(rust_type(&ColumnType::BigInt, true), "Option<i64>");
        assert_eq!(rust_type(&ColumnType::Text, false), "String");
        assert_eq!(rust_type(&ColumnType::Text, true), "Option<String>");
        assert_eq!(rust_type(&ColumnType::Boolean, false), "bool");
        assert_eq!(rust_type(&ColumnType::Boolean, true), "Option<bool>");
        assert_eq!(rust_type(&ColumnType::Timestamp, false), "DateTimeWithTimeZone");
        assert_eq!(rust_type(&ColumnType::Timestamp, true), "Option<DateTimeWithTimeZone>");
        assert_eq!(rust_type(&ColumnType::Custom("MyType".into()), false), "MyType");
        assert_eq!(rust_type(&ColumnType::Custom("MyType".into()), true), "Option<MyType>");
    }

    #[test]
    fn test_sanitize_field_name() {
        assert_eq!(sanitize_field_name("normal_name"), "normal_name");
        assert_eq!(sanitize_field_name("123name"), "_123name");
        assert_eq!(sanitize_field_name("name-with-dash"), "name_with_dash");
        assert_eq!(sanitize_field_name("name.with.dot"), "name_with_dot");
        assert_eq!(sanitize_field_name(""), "_col");
        assert_eq!(sanitize_field_name("a"), "a");
    }

    #[test]
    fn test_unique_name() {
        let mut used = std::collections::HashSet::new();
        assert_eq!(unique_name("test", &mut used), "test");
        assert_eq!(unique_name("test", &mut used), "test_1");
        assert_eq!(unique_name("test", &mut used), "test_2");
        assert_eq!(unique_name("other", &mut used), "other");
        assert_eq!(unique_name("other", &mut used), "other_1");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use insta::{assert_snapshot, with_settings};
    use rstest::rstest;

    #[rstest]
    #[case("basic_single_pk", TableDef {
        name: "users".into(),
        columns: vec![
            ColumnDef { name: "id".into(), r#type: ColumnType::Integer, nullable: false, default: None, comment: None, primary_key: None, unique: None, index: None, foreign_key: None },
            ColumnDef { name: "display_name".into(), r#type: ColumnType::Text, nullable: true, default: None, comment: None, primary_key: None, unique: None, index: None, foreign_key: None },
        ],
        constraints: vec![TableConstraint::PrimaryKey { columns: vec!["id".into()] }],
        indexes: vec![],
    })]
    #[case("composite_pk", TableDef {
        name: "accounts".into(),
        columns: vec![
            ColumnDef { name: "id".into(), r#type: ColumnType::Integer, nullable: false, default: None, comment: None, primary_key: None, unique: None, index: None, foreign_key: None },
            ColumnDef { name: "tenant_id".into(), r#type: ColumnType::BigInt, nullable: false, default: None, comment: None, primary_key: None, unique: None, index: None, foreign_key: None },
        ],
        constraints: vec![TableConstraint::PrimaryKey { columns: vec!["id".into(), "tenant_id".into()] }],
        indexes: vec![],
    })]
    #[case("fk_single", TableDef {
        name: "posts".into(),
        columns: vec![
            ColumnDef { name: "id".into(), r#type: ColumnType::Integer, nullable: false, default: None, comment: None, primary_key: None, unique: None, index: None, foreign_key: None },
            ColumnDef { name: "user_id".into(), r#type: ColumnType::Integer, nullable: false, default: None, comment: None, primary_key: None, unique: None, index: None, foreign_key: None },
            ColumnDef { name: "title".into(), r#type: ColumnType::Text, nullable: true, default: None, comment: None, primary_key: None, unique: None, index: None, foreign_key: None },
        ],
        constraints: vec![
            TableConstraint::PrimaryKey { columns: vec!["id".into()] },
            TableConstraint::ForeignKey {
                name: None,
                columns: vec!["user_id".into()],
                ref_table: "users".into(),
                ref_columns: vec!["id".into()],
                on_delete: None,
                on_update: None,
            },
        ],
        indexes: vec![],
    })]
    #[case("fk_composite", TableDef {
        name: "invoices".into(),
        columns: vec![
            ColumnDef { name: "id".into(), r#type: ColumnType::Integer, nullable: false, default: None, comment: None, primary_key: None, unique: None, index: None, foreign_key: None },
            ColumnDef { name: "customer_id".into(), r#type: ColumnType::Integer, nullable: false, default: None, comment: None, primary_key: None, unique: None, index: None, foreign_key: None },
            ColumnDef { name: "customer_tenant_id".into(), r#type: ColumnType::Integer, nullable: false, default: None, comment: None, primary_key: None, unique: None, index: None, foreign_key: None },
        ],
        constraints: vec![
            TableConstraint::PrimaryKey { columns: vec!["id".into()] },
            TableConstraint::ForeignKey {
                name: None,
                columns: vec!["customer_id".into(), "customer_tenant_id".into()],
                ref_table: "customers".into(),
                ref_columns: vec!["id".into(), "tenant_id".into()],
                on_delete: None,
                on_update: None,
            },
        ],
        indexes: vec![],
    })]
    fn render_entity_snapshots(#[case] name: &str, #[case] table: TableDef) {
        let rendered = render_entity(&table);
        with_settings!({ snapshot_suffix => format!("params_{}", name) }, {
            assert_snapshot!(rendered);
        });
    }
}
