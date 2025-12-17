use std::collections::HashSet;

use crate::orm::OrmExporter;
use vespertide_core::{
    ColumnDef, ColumnType, ComplexColumnType, IndexDef, TableConstraint, TableDef,
};

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

    // Generate Enum definitions first
    let mut processed_enums = HashSet::new();
    for column in &table.columns {
        if let ColumnType::Complex(ComplexColumnType::Enum { name, values }) = &column.r#type {
            // Avoid duplicate enum definitions if multiple columns use the same enum
            if !processed_enums.contains(name) {
                render_enum(&mut lines, name, values);
                processed_enums.insert(name.clone());
            }
        }
    }

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

    let ty = match &column.r#type {
        ColumnType::Complex(ComplexColumnType::Enum { name, .. }) => {
            let enum_type = to_pascal_case(name);
            if column.nullable {
                format!("Option<{}>", enum_type)
            } else {
                enum_type
            }
        }
        _ => column.r#type.to_rust_type(column.nullable),
    };

    lines.push(format!("    pub {}: {},", field_name, ty));
}

fn primary_key_columns(table: &TableDef) -> HashSet<String> {
    use vespertide_core::schema::primary_key::PrimaryKeySyntax;
    let mut keys = HashSet::new();

    // First, check table-level constraints
    for constraint in &table.constraints {
        if let TableConstraint::PrimaryKey { columns, .. } = constraint {
            for col in columns {
                keys.insert(col.clone());
            }
        }
    }

    // Then, check inline primary_key on columns
    // This handles cases where primary_key is defined inline but not yet normalized
    for column in &table.columns {
        match &column.primary_key {
            Some(PrimaryKeySyntax::Bool(true)) | Some(PrimaryKeySyntax::Object(_)) => {
                keys.insert(column.name.clone());
            }
            _ => {}
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

fn render_enum(lines: &mut Vec<String>, name: &str, values: &[String]) {
    let enum_name = to_pascal_case(name);

    lines.push("#[derive(Debug, Clone, PartialEq, Eq, EnumIter, DeriveActiveEnum)]".into());
    lines.push(format!(
        "#[sea_orm(rs_type = \"String\", db_type = \"Enum\", enum_name = \"{}\")]",
        name
    ));
    lines.push(format!("pub enum {} {{", enum_name));

    for value in values {
        let variant_name = enum_variant_name(value);
        lines.push(format!("    #[sea_orm(string_value = \"{}\")]", value));
        lines.push(format!("    {},", variant_name));
    }
    lines.push("}".into());
    lines.push(String::new());
}

/// Convert a string to a valid Rust enum variant name (PascalCase).
/// Handles edge cases like numeric prefixes, special characters, and reserved words.
fn enum_variant_name(s: &str) -> String {
    let pascal = to_pascal_case(s);

    // Handle empty string
    if pascal.is_empty() {
        return "Value".to_string();
    }

    // Handle numeric prefix: prefix with underscore or 'N'
    if pascal
        .chars()
        .next()
        .map(|c| c.is_ascii_digit())
        .unwrap_or(false)
    {
        return format!("N{}", pascal);
    }

    pascal
}

fn to_pascal_case(s: &str) -> String {
    let mut result = String::new();
    let mut capitalize = true;
    for c in s.chars() {
        if c == '_' || c == '-' {
            capitalize = true;
        } else if capitalize {
            result.push(c.to_ascii_uppercase());
            capitalize = false;
        } else {
            result.push(c);
        }
    }
    result
}

#[cfg(test)]
mod helper_tests {
    use super::*;
    use vespertide_core::IndexDef;

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
        assert!(!lines.is_empty());
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
        use vespertide_core::{ColumnType, ComplexColumnType, SimpleColumnType};
        // Numeric types
        assert_eq!(
            ColumnType::Simple(SimpleColumnType::SmallInt).to_rust_type(false),
            "i16"
        );
        assert_eq!(
            ColumnType::Simple(SimpleColumnType::SmallInt).to_rust_type(true),
            "Option<i16>"
        );
        assert_eq!(
            ColumnType::Simple(SimpleColumnType::Integer).to_rust_type(false),
            "i32"
        );
        assert_eq!(
            ColumnType::Simple(SimpleColumnType::Integer).to_rust_type(true),
            "Option<i32>"
        );
        assert_eq!(
            ColumnType::Simple(SimpleColumnType::BigInt).to_rust_type(false),
            "i64"
        );
        assert_eq!(
            ColumnType::Simple(SimpleColumnType::BigInt).to_rust_type(true),
            "Option<i64>"
        );
        assert_eq!(
            ColumnType::Simple(SimpleColumnType::Real).to_rust_type(false),
            "f32"
        );
        assert_eq!(
            ColumnType::Simple(SimpleColumnType::DoublePrecision).to_rust_type(false),
            "f64"
        );

        // Text type
        assert_eq!(
            ColumnType::Simple(SimpleColumnType::Text).to_rust_type(false),
            "String"
        );
        assert_eq!(
            ColumnType::Simple(SimpleColumnType::Text).to_rust_type(true),
            "Option<String>"
        );

        // Boolean type
        assert_eq!(
            ColumnType::Simple(SimpleColumnType::Boolean).to_rust_type(false),
            "bool"
        );
        assert_eq!(
            ColumnType::Simple(SimpleColumnType::Boolean).to_rust_type(true),
            "Option<bool>"
        );

        // Date/Time types
        assert_eq!(
            ColumnType::Simple(SimpleColumnType::Date).to_rust_type(false),
            "Date"
        );
        assert_eq!(
            ColumnType::Simple(SimpleColumnType::Time).to_rust_type(false),
            "Time"
        );
        assert_eq!(
            ColumnType::Simple(SimpleColumnType::Timestamp).to_rust_type(false),
            "DateTime"
        );
        assert_eq!(
            ColumnType::Simple(SimpleColumnType::Timestamp).to_rust_type(true),
            "Option<DateTime>"
        );
        assert_eq!(
            ColumnType::Simple(SimpleColumnType::Timestamptz).to_rust_type(false),
            "DateTimeWithTimeZone"
        );
        assert_eq!(
            ColumnType::Simple(SimpleColumnType::Timestamptz).to_rust_type(true),
            "Option<DateTimeWithTimeZone>"
        );

        // Binary type
        assert_eq!(
            ColumnType::Simple(SimpleColumnType::Bytea).to_rust_type(false),
            "Vec<u8>"
        );

        // UUID type
        assert_eq!(
            ColumnType::Simple(SimpleColumnType::Uuid).to_rust_type(false),
            "Uuid"
        );

        // JSON types
        assert_eq!(
            ColumnType::Simple(SimpleColumnType::Json).to_rust_type(false),
            "Json"
        );
        assert_eq!(
            ColumnType::Simple(SimpleColumnType::Jsonb).to_rust_type(false),
            "Json"
        );

        // Network types
        assert_eq!(
            ColumnType::Simple(SimpleColumnType::Inet).to_rust_type(false),
            "String"
        );
        assert_eq!(
            ColumnType::Simple(SimpleColumnType::Cidr).to_rust_type(false),
            "String"
        );
        assert_eq!(
            ColumnType::Simple(SimpleColumnType::Macaddr).to_rust_type(false),
            "String"
        );

        // Interval type
        assert_eq!(
            ColumnType::Simple(SimpleColumnType::Interval).to_rust_type(false),
            "String"
        );

        // XML type
        assert_eq!(
            ColumnType::Simple(SimpleColumnType::Xml).to_rust_type(false),
            "String"
        );

        // Complex types
        assert_eq!(
            ColumnType::Complex(ComplexColumnType::Numeric {
                precision: 10,
                scale: 2
            })
            .to_rust_type(false),
            "Decimal"
        );
        assert_eq!(
            ColumnType::Complex(ComplexColumnType::Char { length: 10 }).to_rust_type(false),
            "String"
        );
    }

    #[test]
    fn test_sanitize_field_name() {
        assert_eq!(sanitize_field_name("normal_name"), "normal_name");
        assert_eq!(sanitize_field_name("123name"), "_123name");
        assert_eq!(sanitize_field_name("name-with-dash"), "name_with_dash");
        assert_eq!(sanitize_field_name("name.with.dot"), "name_with_dot");
        assert_eq!(sanitize_field_name("name with space"), "name_with_space");
        assert_eq!(
            sanitize_field_name("name  with  multiple  spaces"),
            "name__with__multiple__spaces"
        );
        assert_eq!(
            sanitize_field_name(" name_with_leading_space"),
            "_name_with_leading_space"
        );
        assert_eq!(
            sanitize_field_name("name_with_trailing_space "),
            "name_with_trailing_space_"
        );
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

    #[test]
    fn test_to_pascal_case() {
        // Basic snake_case conversion
        assert_eq!(to_pascal_case("hello_world"), "HelloWorld");
        assert_eq!(to_pascal_case("order_status"), "OrderStatus");

        // Kebab-case conversion
        assert_eq!(to_pascal_case("hello-world"), "HelloWorld");
        assert_eq!(to_pascal_case("info-level"), "InfoLevel");

        // Already PascalCase
        assert_eq!(to_pascal_case("HelloWorld"), "HelloWorld");

        // Single word
        assert_eq!(to_pascal_case("hello"), "Hello");
        assert_eq!(to_pascal_case("pending"), "Pending");

        // Mixed delimiters
        assert_eq!(to_pascal_case("hello_world-test"), "HelloWorldTest");

        // Uppercase input
        assert_eq!(to_pascal_case("HELLO_WORLD"), "HELLOWORLD");
        assert_eq!(to_pascal_case("ERROR_LEVEL"), "ERRORLEVEL");

        // With numbers
        assert_eq!(to_pascal_case("level_1"), "Level1");
        assert_eq!(to_pascal_case("1_critical"), "1Critical");

        // Empty string
        assert_eq!(to_pascal_case(""), "");
    }

    #[test]
    fn test_enum_variant_name() {
        // Normal cases
        assert_eq!(enum_variant_name("pending"), "Pending");
        assert_eq!(enum_variant_name("in_stock"), "InStock");
        assert_eq!(enum_variant_name("info-level"), "InfoLevel");

        // Numeric prefix - should add 'N' prefix
        // Note: to_pascal_case("1critical") returns "1critical" (starts with digit, so no uppercase)
        // then enum_variant_name adds 'N' prefix
        assert_eq!(enum_variant_name("1critical"), "N1critical");
        assert_eq!(enum_variant_name("123abc"), "N123abc");
        // With underscore separator, the part after underscore gets capitalized
        assert_eq!(enum_variant_name("1_critical"), "N1Critical");

        // Empty string - should return default
        assert_eq!(enum_variant_name(""), "Value");
    }

    #[test]
    fn test_render_enum() {
        let mut lines = Vec::new();
        render_enum(
            &mut lines,
            "order_status",
            &["pending".into(), "shipped".into()],
        );

        assert!(lines.iter().any(|l| l.contains("pub enum OrderStatus")));
        assert!(lines.iter().any(|l| l.contains("Pending")));
        assert!(lines.iter().any(|l| l.contains("Shipped")));
        assert!(lines.iter().any(|l| l.contains("DeriveActiveEnum")));
        assert!(lines.iter().any(|l| l.contains("EnumIter")));
        assert!(
            lines
                .iter()
                .any(|l| l.contains("enum_name = \"order_status\""))
        );
    }

    #[test]
    fn test_render_enum_with_numeric_prefix_value() {
        let mut lines = Vec::new();
        render_enum(
            &mut lines,
            "priority",
            &["1_high".into(), "2_medium".into(), "3_low".into()],
        );

        // Numeric prefixed values should be prefixed with 'N'
        assert!(lines.iter().any(|l| l.contains("N1High")));
        assert!(lines.iter().any(|l| l.contains("N2Medium")));
        assert!(lines.iter().any(|l| l.contains("N3Low")));
        // But the string_value should remain original
        assert!(
            lines
                .iter()
                .any(|l| l.contains("string_value = \"1_high\""))
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use insta::{assert_snapshot, with_settings};
    use rstest::rstest;
    use vespertide_core::schema::primary_key::PrimaryKeySyntax;
    use vespertide_core::{ColumnType, SimpleColumnType};

    #[rstest]
    #[case("basic_single_pk", TableDef {
        name: "users".into(),
        columns: vec![
            ColumnDef { name: "id".into(), r#type: ColumnType::Simple(SimpleColumnType::Integer), nullable: false, default: None, comment: None, primary_key: None, unique: None, index: None, foreign_key: None },
            ColumnDef { name: "display_name".into(), r#type: ColumnType::Simple(SimpleColumnType::Text), nullable: true, default: None, comment: None, primary_key: None, unique: None, index: None, foreign_key: None },
        ],
        constraints: vec![TableConstraint::PrimaryKey { auto_increment: false, columns: vec!["id".into()] }],
        indexes: vec![],
    })]
    #[case("composite_pk", TableDef {
        name: "accounts".into(),
        columns: vec![
            ColumnDef { name: "id".into(), r#type: ColumnType::Simple(SimpleColumnType::Integer), nullable: false, default: None, comment: None, primary_key: None, unique: None, index: None, foreign_key: None },
            ColumnDef { name: "tenant_id".into(), r#type: ColumnType::Simple(SimpleColumnType::BigInt), nullable: false, default: None, comment: None, primary_key: None, unique: None, index: None, foreign_key: None },
        ],
        constraints: vec![TableConstraint::PrimaryKey { auto_increment: false, columns: vec!["id".into(), "tenant_id".into()] }],
        indexes: vec![],
    })]
    #[case("fk_single", TableDef {
        name: "posts".into(),
        columns: vec![
            ColumnDef { name: "id".into(), r#type: ColumnType::Simple(SimpleColumnType::Integer), nullable: false, default: None, comment: None, primary_key: None, unique: None, index: None, foreign_key: None },
            ColumnDef { name: "user_id".into(), r#type: ColumnType::Simple(SimpleColumnType::Integer), nullable: false, default: None, comment: None, primary_key: None, unique: None, index: None, foreign_key: None },
            ColumnDef { name: "title".into(), r#type: ColumnType::Simple(SimpleColumnType::Text), nullable: true, default: None, comment: None, primary_key: None, unique: None, index: None, foreign_key: None },
        ],
        constraints: vec![
            TableConstraint::PrimaryKey { auto_increment: false, columns: vec!["id".into()] },
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
            ColumnDef { name: "id".into(), r#type: ColumnType::Simple(SimpleColumnType::Integer), nullable: false, default: None, comment: None, primary_key: None, unique: None, index: None, foreign_key: None },
            ColumnDef { name: "customer_id".into(), r#type: ColumnType::Simple(SimpleColumnType::Integer), nullable: false, default: None, comment: None, primary_key: None, unique: None, index: None, foreign_key: None },
            ColumnDef { name: "customer_tenant_id".into(), r#type: ColumnType::Simple(SimpleColumnType::Integer), nullable: false, default: None, comment: None, primary_key: None, unique: None, index: None, foreign_key: None },
        ],
        constraints: vec![
            TableConstraint::PrimaryKey { auto_increment: false, columns: vec!["id".into()] },
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
    #[case("inline_pk", TableDef {
        name: "users".into(),
        columns: vec![
            ColumnDef { name: "id".into(), r#type: ColumnType::Simple(SimpleColumnType::Uuid), nullable: false, default: Some("gen_random_uuid()".into()), comment: None, primary_key: Some(PrimaryKeySyntax::Bool(true)), unique: None, index: None, foreign_key: None },
            ColumnDef { name: "email".into(), r#type: ColumnType::Simple(SimpleColumnType::Text), nullable: false, default: None, comment: None, primary_key: None, unique: Some(vespertide_core::StrOrBoolOrArray::Bool(true)), index: None, foreign_key: None },
        ],
        constraints: vec![],
        indexes: vec![],
    })]
    #[case("pk_and_fk_together", {
        use vespertide_core::schema::foreign_key::{ForeignKeyDef, ForeignKeySyntax};
        use vespertide_core::schema::reference::ReferenceAction;
        let mut table = TableDef {
            name: "article_user".into(),
            columns: vec![
                ColumnDef {
                    name: "article_id".into(),
                    r#type: ColumnType::Simple(SimpleColumnType::Uuid),
                    nullable: false,
                    default: None,
                    comment: None,
                    primary_key: Some(PrimaryKeySyntax::Bool(true)),
                    unique: None,
                    index: Some(vespertide_core::StrOrBoolOrArray::Bool(true)),
                    foreign_key: Some(ForeignKeySyntax::Object(ForeignKeyDef {
                        ref_table: "article".into(),
                        ref_columns: vec!["id".into()],
                        on_delete: Some(ReferenceAction::Cascade),
                        on_update: None,
                    })),
                },
                ColumnDef {
                    name: "user_id".into(),
                    r#type: ColumnType::Simple(SimpleColumnType::Uuid),
                    nullable: false,
                    default: None,
                    comment: None,
                    primary_key: Some(PrimaryKeySyntax::Bool(true)),
                    unique: None,
                    index: Some(vespertide_core::StrOrBoolOrArray::Bool(true)),
                    foreign_key: Some(ForeignKeySyntax::Object(ForeignKeyDef {
                        ref_table: "user".into(),
                        ref_columns: vec!["id".into()],
                        on_delete: Some(ReferenceAction::Cascade),
                        on_update: None,
                    })),
                },
                ColumnDef {
                    name: "author_order".into(),
                    r#type: ColumnType::Simple(SimpleColumnType::Integer),
                    nullable: false,
                    default: Some("1".into()),
                    comment: None,
                    primary_key: None,
                    unique: None,
                    index: None,
                    foreign_key: None,
                },
                ColumnDef {
                    name: "role".into(),
                    r#type: ColumnType::Complex(vespertide_core::ComplexColumnType::Varchar { length: 20 }),
                    nullable: false,
                    default: Some("'contributor'".into()),
                    comment: None,
                    primary_key: None,
                    unique: None,
                    index: None,
                    foreign_key: None,
                },
                ColumnDef {
                    name: "is_lead".into(),
                    r#type: ColumnType::Simple(SimpleColumnType::Boolean),
                    nullable: false,
                    default: Some("false".into()),
                    comment: None,
                    primary_key: None,
                    unique: None,
                    index: None,
                    foreign_key: None,
                },
                ColumnDef {
                    name: "created_at".into(),
                    r#type: ColumnType::Simple(SimpleColumnType::Timestamptz),
                    nullable: false,
                    default: Some("now()".into()),
                    comment: None,
                    primary_key: None,
                    unique: None,
                    index: None,
                    foreign_key: None,
                },
            ],
            constraints: vec![],
            indexes: vec![],
        };
        // Normalize to convert inline constraints to table-level
        table = table.normalize().unwrap();
        table
    })]
    #[case("enum_type", TableDef {
        name: "orders".into(),
        columns: vec![
            ColumnDef { name: "id".into(), r#type: ColumnType::Simple(SimpleColumnType::Integer), nullable: false, default: None, comment: None, primary_key: Some(PrimaryKeySyntax::Bool(true)), unique: None, index: None, foreign_key: None },
            ColumnDef {
                name: "status".into(),
                r#type: ColumnType::Complex(ComplexColumnType::Enum {
                    name: "order_status".into(),
                    values: vec!["pending".into(), "shipped".into(), "delivered".into()]
                }),
                nullable: false,
                default: None,
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            },
        ],
        constraints: vec![],
        indexes: vec![],
    })]
    #[case("enum_nullable", TableDef {
        name: "tasks".into(),
        columns: vec![
            ColumnDef { name: "id".into(), r#type: ColumnType::Simple(SimpleColumnType::Integer), nullable: false, default: None, comment: None, primary_key: Some(PrimaryKeySyntax::Bool(true)), unique: None, index: None, foreign_key: None },
            ColumnDef {
                name: "priority".into(),
                r#type: ColumnType::Complex(ComplexColumnType::Enum {
                    name: "task_priority".into(),
                    values: vec!["low".into(), "medium".into(), "high".into(), "critical".into()]
                }),
                nullable: true,
                default: None,
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            },
        ],
        constraints: vec![],
        indexes: vec![],
    })]
    #[case("enum_multiple_columns", TableDef {
        name: "products".into(),
        columns: vec![
            ColumnDef { name: "id".into(), r#type: ColumnType::Simple(SimpleColumnType::Integer), nullable: false, default: None, comment: None, primary_key: Some(PrimaryKeySyntax::Bool(true)), unique: None, index: None, foreign_key: None },
            ColumnDef {
                name: "category".into(),
                r#type: ColumnType::Complex(ComplexColumnType::Enum {
                    name: "product_category".into(),
                    values: vec!["electronics".into(), "clothing".into(), "food".into()]
                }),
                nullable: false,
                default: None,
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            },
            ColumnDef {
                name: "availability".into(),
                r#type: ColumnType::Complex(ComplexColumnType::Enum {
                    name: "availability_status".into(),
                    values: vec!["in_stock".into(), "out_of_stock".into(), "pre_order".into()]
                }),
                nullable: false,
                default: None,
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            },
        ],
        constraints: vec![],
        indexes: vec![],
    })]
    #[case("enum_shared", TableDef {
        name: "documents".into(),
        columns: vec![
            ColumnDef { name: "id".into(), r#type: ColumnType::Simple(SimpleColumnType::Integer), nullable: false, default: None, comment: None, primary_key: Some(PrimaryKeySyntax::Bool(true)), unique: None, index: None, foreign_key: None },
            ColumnDef {
                name: "status".into(),
                r#type: ColumnType::Complex(ComplexColumnType::Enum {
                    name: "doc_status".into(),
                    values: vec!["draft".into(), "published".into(), "archived".into()]
                }),
                nullable: false,
                default: None,
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            },
            ColumnDef {
                name: "review_status".into(),
                r#type: ColumnType::Complex(ComplexColumnType::Enum {
                    name: "doc_status".into(),
                    values: vec!["draft".into(), "published".into(), "archived".into()]
                }),
                nullable: true,
                default: None,
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            },
        ],
        constraints: vec![],
        indexes: vec![],
    })]
    #[case("enum_special_values", TableDef {
        name: "events".into(),
        columns: vec![
            ColumnDef { name: "id".into(), r#type: ColumnType::Simple(SimpleColumnType::Integer), nullable: false, default: None, comment: None, primary_key: Some(PrimaryKeySyntax::Bool(true)), unique: None, index: None, foreign_key: None },
            ColumnDef {
                name: "severity".into(),
                r#type: ColumnType::Complex(ComplexColumnType::Enum {
                    name: "event_severity".into(),
                    values: vec!["info-level".into(), "warning_level".into(), "ERROR_LEVEL".into(), "1critical".into()]
                }),
                nullable: false,
                default: None,
                comment: None,
                primary_key: None,
                unique: None,
                index: None,
                foreign_key: None,
            },
        ],
        constraints: vec![],
        indexes: vec![],
    })]
    fn render_entity_snapshots(#[case] name: &str, #[case] table: TableDef) {
        let rendered = render_entity(&table);
        with_settings!({ snapshot_suffix => format!("params_{}", name) }, {
            assert_snapshot!(rendered);
        });
    }
}
