use std::collections::HashSet;

use crate::orm::OrmExporter;
use vespertide_core::{ColumnDef, IndexDef, TableConstraint, TableDef};

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
    let ty = column.r#type.to_rust_type(column.nullable);
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
    fn render_entity_snapshots(#[case] name: &str, #[case] table: TableDef) {
        let rendered = render_entity(&table);
        with_settings!({ snapshot_suffix => format!("params_{}", name) }, {
            assert_snapshot!(rendered);
        });
    }
}
