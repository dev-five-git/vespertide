use std::collections::HashSet;

use crate::orm::OrmExporter;
use vespertide_core::schema::column::{
    ColumnType, ComplexColumnType, EnumValues, SimpleColumnType,
};
use vespertide_core::schema::constraint::TableConstraint;
use vespertide_core::{ColumnDef, TableDef};

/// Track which types are actually used to generate minimal imports
#[derive(Default)]
struct UsedTypes<'a> {
    sa_types: HashSet<&'a str>,
    datetime_types: HashSet<&'a str>,
    needs_optional: bool,
    needs_uuid: bool,
    needs_decimal: bool,
}

impl<'a> UsedTypes<'a> {
    fn add_column_type(&mut self, col_type: &ColumnType, nullable: bool) {
        if nullable {
            self.needs_optional = true;
        }

        match col_type {
            ColumnType::Simple(ty) => match ty {
                SimpleColumnType::SmallInt => {
                    self.sa_types.insert("SmallInteger");
                }
                SimpleColumnType::Integer => {
                    self.sa_types.insert("Integer");
                }
                SimpleColumnType::BigInt => {
                    self.sa_types.insert("BigInteger");
                }
                SimpleColumnType::Real | SimpleColumnType::DoublePrecision => {
                    self.sa_types.insert("Float");
                }
                SimpleColumnType::Text => {
                    self.sa_types.insert("Text");
                }
                SimpleColumnType::Boolean => {
                    self.sa_types.insert("Boolean");
                }
                SimpleColumnType::Date => {
                    self.sa_types.insert("Date");
                    self.datetime_types.insert("date");
                }
                SimpleColumnType::Time => {
                    self.sa_types.insert("Time");
                    self.datetime_types.insert("time");
                }
                SimpleColumnType::Timestamp | SimpleColumnType::Timestamptz => {
                    self.sa_types.insert("DateTime");
                    self.datetime_types.insert("datetime");
                }
                SimpleColumnType::Interval => {
                    self.sa_types.insert("Interval");
                }
                SimpleColumnType::Bytea => {
                    self.sa_types.insert("LargeBinary");
                }
                SimpleColumnType::Uuid => {
                    self.sa_types.insert("Uuid");
                    self.needs_uuid = true;
                }
                SimpleColumnType::Json | SimpleColumnType::Jsonb => {
                    self.sa_types.insert("JSON");
                }
                SimpleColumnType::Inet | SimpleColumnType::Cidr | SimpleColumnType::Macaddr => {
                    self.sa_types.insert("String");
                }
                SimpleColumnType::Xml => {
                    self.sa_types.insert("Text");
                }
            },
            ColumnType::Complex(ty) => match ty {
                ComplexColumnType::Varchar { .. } | ComplexColumnType::Char { .. } => {
                    self.sa_types.insert("String");
                }
                ComplexColumnType::Numeric { .. } => {
                    self.sa_types.insert("Numeric");
                    self.needs_decimal = true;
                }
                ComplexColumnType::Custom { .. } => {}
                ComplexColumnType::Enum { values, .. } => match values {
                    EnumValues::String(_) => {
                        self.sa_types.insert("Enum");
                    }
                    EnumValues::Integer(_) => {
                        self.sa_types.insert("Integer");
                    }
                },
            },
        }
    }
}

pub struct SqlAlchemyExporter;

impl OrmExporter for SqlAlchemyExporter {
    fn render_entity(&self, table: &TableDef) -> Result<String, String> {
        render_entity(table)
    }
}

/// Render a SQLAlchemy model for the given table definition.
pub fn render_entity(table: &TableDef) -> Result<String, String> {
    let mut lines: Vec<String> = Vec::new();

    // Collect enums for this table
    let enums: Vec<(&str, &EnumValues)> = table
        .columns
        .iter()
        .filter_map(|col| {
            if let ColumnType::Complex(ComplexColumnType::Enum { name, values }) = &col.r#type {
                Some((name.as_str(), values))
            } else {
                None
            }
        })
        .collect();

    // Collect used types
    let mut used_types = UsedTypes::default();
    for col in &table.columns {
        used_types.add_column_type(&col.r#type, col.nullable);
    }

    // Check for foreign keys
    let has_fk = table
        .constraints
        .iter()
        .any(|c| matches!(c, TableConstraint::ForeignKey { .. }));
    if has_fk {
        used_types.sa_types.insert("ForeignKey");
    }

    // Check for indexes
    let has_index = table
        .constraints
        .iter()
        .any(|c| matches!(c, TableConstraint::Index { .. }));
    if has_index {
        used_types.sa_types.insert("Index");
    }

    // Check for composite unique constraints
    let has_unique = table
        .constraints
        .iter()
        .any(|c| matches!(c, TableConstraint::Unique { columns, .. } if columns.len() > 1));
    if has_unique {
        used_types.sa_types.insert("UniqueConstraint");
    }

    // Check for server defaults
    let has_server_default = table
        .columns
        .iter()
        .any(|c| c.default.as_ref().is_some_and(|d| d.to_sql().contains('(')));
    if has_server_default {
        used_types.sa_types.insert("text");
    }

    // Generate imports
    lines.push("from __future__ import annotations".into());
    lines.push("".into());
    if !enums.is_empty() {
        lines.push("import enum".into());
    }

    // datetime imports
    let datetime_imports: Vec<&str> = used_types.datetime_types.iter().copied().collect();
    if !datetime_imports.is_empty() {
        lines.push(format!(
            "from datetime import {}",
            datetime_imports.join(", ")
        ));
    }

    if used_types.needs_decimal {
        lines.push("from decimal import Decimal".into());
    }

    if used_types.needs_optional {
        lines.push("from typing import Optional".into());
    }

    if used_types.needs_uuid {
        lines.push("from uuid import UUID".into());
    }

    lines.push("".into());

    // SQLAlchemy imports
    let mut sa_imports: Vec<&str> = used_types.sa_types.iter().copied().collect();
    sa_imports.sort();
    lines.push(format!("from sqlalchemy import {}", sa_imports.join(", ")));
    lines.push("from sqlalchemy.orm import DeclarativeBase, Mapped, mapped_column".into());
    lines.push("".into());
    lines.push("".into());

    // Render enum classes
    for (enum_name, values) in &enums {
        render_enum(&mut lines, enum_name, values);
        lines.push("".into());
    }

    // Class definition
    let class_name = to_pascal_case(&table.name);

    // Add table description as docstring
    if let Some(ref desc) = table.description {
        lines.push(format!("class {}(DeclarativeBase):", class_name));
        lines.push(format!("    \"\"\"{}\"\"\"", desc.replace('\n', " ")));
    } else {
        lines.push(format!("class {}(DeclarativeBase):", class_name));
    }

    lines.push(format!("    __tablename__ = \"{}\"", table.name));
    lines.push("".into());

    // Collect primary key columns
    let pk_columns: std::collections::HashSet<String> = table
        .constraints
        .iter()
        .filter_map(|c| {
            if let TableConstraint::PrimaryKey { columns, .. } = c {
                Some(columns.clone())
            } else {
                None
            }
        })
        .flatten()
        .collect();

    // Collect unique columns (single-column unique constraints)
    let unique_columns: std::collections::HashSet<String> = table
        .constraints
        .iter()
        .filter_map(|c| {
            if let TableConstraint::Unique { columns, .. } = c {
                if columns.len() == 1 {
                    Some(columns[0].clone())
                } else {
                    None
                }
            } else {
                None
            }
        })
        .collect();

    // Collect foreign key info
    let fk_info: std::collections::HashMap<String, (String, String)> = table
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
                if columns.len() == 1 && ref_columns.len() == 1 {
                    Some((
                        columns[0].clone(),
                        (ref_table.clone(), ref_columns[0].clone()),
                    ))
                } else {
                    None
                }
            } else {
                None
            }
        })
        .collect();

    // Render columns
    for col in &table.columns {
        render_column(
            &mut lines,
            col,
            pk_columns.contains(&col.name),
            unique_columns.contains(&col.name),
            fk_info.get(&col.name),
        );
    }

    // Render indexes as __table_args__
    let indexes: Vec<_> = table
        .constraints
        .iter()
        .filter_map(|c| {
            if let TableConstraint::Index { name, columns } = c {
                Some((name.clone(), columns.clone()))
            } else {
                None
            }
        })
        .collect();

    // Render composite unique constraints
    let composite_uniques: Vec<_> = table
        .constraints
        .iter()
        .filter_map(|c| {
            if let TableConstraint::Unique { name, columns } = c {
                if columns.len() > 1 {
                    Some((name.clone(), columns.clone()))
                } else {
                    None
                }
            } else {
                None
            }
        })
        .collect();

    if !indexes.is_empty() || !composite_uniques.is_empty() {
        lines.push("".into());
        lines.push("    __table_args__ = (".into());

        for (name, columns) in &indexes {
            let cols_str = columns
                .iter()
                .map(|c| format!("\"{}\"", c))
                .collect::<Vec<_>>()
                .join(", ");
            if let Some(idx_name) = name {
                lines.push(format!("        Index(\"{}\", {}),", idx_name, cols_str));
            } else {
                lines.push(format!("        Index(None, {}),", cols_str));
            }
        }

        for (name, columns) in &composite_uniques {
            let cols_str = columns
                .iter()
                .map(|c| format!("\"{}\"", c))
                .collect::<Vec<_>>()
                .join(", ");
            if let Some(uq_name) = name {
                lines.push(format!(
                    "        UniqueConstraint({}, name=\"{}\"),",
                    cols_str, uq_name
                ));
            } else {
                lines.push(format!("        UniqueConstraint({}),", cols_str));
            }
        }

        lines.push("    )".into());
    }

    lines.push("".into());

    Ok(lines.join("\n"))
}

fn render_enum(lines: &mut Vec<String>, name: &str, values: &EnumValues) {
    let class_name = to_pascal_case(name);

    match values {
        EnumValues::String(vals) => {
            lines.push(format!("class {}(str, enum.Enum):", class_name));
            for val in vals {
                let variant_name = to_screaming_snake_case(val);
                lines.push(format!("    {} = \"{}\"", variant_name, val));
            }
        }
        EnumValues::Integer(vals) => {
            lines.push(format!("class {}(enum.IntEnum):", class_name));
            for val in vals {
                lines.push(format!("    {} = {}", val.name, val.value));
            }
        }
    }
}

fn render_column(
    lines: &mut Vec<String>,
    col: &ColumnDef,
    is_pk: bool,
    is_unique: bool,
    fk_info: Option<&(String, String)>,
) {
    // Add column comment
    if let Some(ref comment) = col.comment {
        lines.push(format!("    # {}", comment.replace('\n', " ")));
    }

    let python_type = column_type_to_python(&col.r#type, col.nullable);
    let sa_type = column_type_to_sqlalchemy(&col.r#type);

    let mut attrs: Vec<String> = Vec::new();

    // Add SQLAlchemy type
    attrs.push(sa_type);

    // Foreign key
    if let Some((ref_table, ref_col)) = fk_info {
        attrs.push(format!("ForeignKey(\"{}.{}\")", ref_table, ref_col));
    }

    // Primary key
    if is_pk {
        attrs.push("primary_key=True".into());
    }

    // Nullable
    if !is_pk {
        attrs.push(format!(
            "nullable={}",
            if col.nullable { "True" } else { "False" }
        ));
    }

    // Unique
    if is_unique && !is_pk {
        attrs.push("unique=True".into());
    }

    // Default value
    if let Some(ref default) = col.default {
        let default_str = default.to_sql();
        // Check if it's a function call or literal
        if default_str.contains('(') {
            attrs.push(format!("server_default=text(\"{}\")", default_str));
        } else if default_str.starts_with('\'') || default_str.starts_with('"') {
            attrs.push(format!("server_default={}", default_str));
        } else {
            attrs.push(format!("server_default=\"{}\"", default_str));
        }
    }

    let attrs_str = attrs.join(", ");
    lines.push(format!(
        "    {}: Mapped[{}] = mapped_column({})",
        col.name, python_type, attrs_str
    ));
}

fn column_type_to_python(col_type: &ColumnType, nullable: bool) -> String {
    let base = match col_type {
        ColumnType::Simple(ty) => match ty {
            SimpleColumnType::SmallInt => "int",
            SimpleColumnType::Integer => "int",
            SimpleColumnType::BigInt => "int",
            SimpleColumnType::Real => "float",
            SimpleColumnType::DoublePrecision => "float",
            SimpleColumnType::Text => "str",
            SimpleColumnType::Boolean => "bool",
            SimpleColumnType::Date => "date",
            SimpleColumnType::Time => "time",
            SimpleColumnType::Timestamp => "datetime",
            SimpleColumnType::Timestamptz => "datetime",
            SimpleColumnType::Interval => "str",
            SimpleColumnType::Bytea => "bytes",
            SimpleColumnType::Uuid => "UUID",
            SimpleColumnType::Json | SimpleColumnType::Jsonb => "dict",
            SimpleColumnType::Inet | SimpleColumnType::Cidr => "str",
            SimpleColumnType::Macaddr => "str",
            SimpleColumnType::Xml => "str",
        },
        ColumnType::Complex(ty) => match ty {
            ComplexColumnType::Varchar { .. } => "str",
            ComplexColumnType::Numeric { .. } => "Decimal",
            ComplexColumnType::Char { .. } => "str",
            ComplexColumnType::Custom { .. } => "str",
            ComplexColumnType::Enum { name, .. } => {
                return if nullable {
                    format!("Optional[{}]", to_pascal_case(name))
                } else {
                    to_pascal_case(name)
                };
            }
        },
    };

    if nullable {
        format!("Optional[{}]", base)
    } else {
        base.to_string()
    }
}

fn column_type_to_sqlalchemy(col_type: &ColumnType) -> String {
    match col_type {
        ColumnType::Simple(ty) => match ty {
            SimpleColumnType::SmallInt => "SmallInteger".into(),
            SimpleColumnType::Integer => "Integer".into(),
            SimpleColumnType::BigInt => "BigInteger".into(),
            SimpleColumnType::Real => "Float".into(),
            SimpleColumnType::DoublePrecision => "Float".into(),
            SimpleColumnType::Text => "Text".into(),
            SimpleColumnType::Boolean => "Boolean".into(),
            SimpleColumnType::Date => "Date".into(),
            SimpleColumnType::Time => "Time".into(),
            SimpleColumnType::Timestamp => "DateTime".into(),
            SimpleColumnType::Timestamptz => "DateTime(timezone=True)".into(),
            SimpleColumnType::Interval => "Interval".into(),
            SimpleColumnType::Bytea => "LargeBinary".into(),
            SimpleColumnType::Uuid => "Uuid".into(),
            SimpleColumnType::Json | SimpleColumnType::Jsonb => "JSON".into(),
            SimpleColumnType::Inet | SimpleColumnType::Cidr | SimpleColumnType::Macaddr => {
                "String(255)".into()
            }
            SimpleColumnType::Xml => "Text".into(),
        },
        ColumnType::Complex(ty) => match ty {
            ComplexColumnType::Varchar { length } => format!("String({})", length),
            ComplexColumnType::Numeric { precision, scale } => {
                format!("Numeric({}, {})", precision, scale)
            }
            ComplexColumnType::Char { length } => format!("String({})", length),
            ComplexColumnType::Custom { custom_type } => format!("\"{}\"", custom_type),
            ComplexColumnType::Enum { name, values } => {
                let class_name = to_pascal_case(name);
                match values {
                    EnumValues::String(_) => format!("Enum({})", class_name),
                    EnumValues::Integer(_) => "Integer".into(), // Integer enums stored as INTEGER
                }
            }
        },
    }
}

fn to_pascal_case(s: &str) -> String {
    s.split('_')
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                None => String::new(),
                Some(first) => first.to_uppercase().chain(chars).collect(),
            }
        })
        .collect()
}

fn to_screaming_snake_case(s: &str) -> String {
    let mut result = String::new();
    for (i, ch) in s.chars().enumerate() {
        if ch.is_uppercase() && i > 0 {
            result.push('_');
        }
        result.push(ch.to_ascii_uppercase());
    }
    // Replace any non-alphanumeric with underscore
    result
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use insta::assert_snapshot;
    use rstest::rstest;
    use vespertide_core::schema::column::NumValue;

    fn col(name: &str, ty: ColumnType) -> ColumnDef {
        ColumnDef {
            name: name.to_string(),
            r#type: ty,
            nullable: false,
            default: None,
            comment: None,
            primary_key: None,
            unique: None,
            index: None,
            foreign_key: None,
        }
    }

    #[test]
    fn test_basic_table() {
        let table = TableDef {
            name: "users".into(),
            description: Some("User accounts table".into()),
            columns: vec![
                ColumnDef {
                    name: "id".into(),
                    r#type: ColumnType::Simple(SimpleColumnType::Integer),
                    nullable: false,
                    default: None,
                    comment: Some("Primary key".into()),
                    primary_key: None,
                    unique: None,
                    index: None,
                    foreign_key: None,
                },
                ColumnDef {
                    name: "email".into(),
                    r#type: ColumnType::Simple(SimpleColumnType::Text),
                    nullable: false,
                    default: None,
                    comment: Some("User email address".into()),
                    primary_key: None,
                    unique: None,
                    index: None,
                    foreign_key: None,
                },
                ColumnDef {
                    name: "name".into(),
                    r#type: ColumnType::Simple(SimpleColumnType::Text),
                    nullable: true,
                    default: None,
                    comment: None,
                    primary_key: None,
                    unique: None,
                    index: None,
                    foreign_key: None,
                },
            ],
            constraints: vec![
                TableConstraint::PrimaryKey {
                    auto_increment: true,
                    columns: vec!["id".into()],
                },
                TableConstraint::Unique {
                    name: None,
                    columns: vec!["email".into()],
                },
            ],
        };

        let result = render_entity(&table).unwrap();
        assert_snapshot!(result);
    }

    #[test]
    fn test_table_with_enum() {
        let table = TableDef {
            name: "orders".into(),
            description: None,
            columns: vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                ColumnDef {
                    name: "status".into(),
                    r#type: ColumnType::Complex(ComplexColumnType::Enum {
                        name: "order_status".into(),
                        values: EnumValues::String(vec![
                            "pending".into(),
                            "shipped".into(),
                            "delivered".into(),
                        ]),
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
            constraints: vec![TableConstraint::PrimaryKey {
                auto_increment: false,
                columns: vec!["id".into()],
            }],
        };

        let result = render_entity(&table).unwrap();
        assert_snapshot!(result);
    }

    #[test]
    fn test_table_with_integer_enum() {
        let table = TableDef {
            name: "tasks".into(),
            description: None,
            columns: vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                ColumnDef {
                    name: "priority".into(),
                    r#type: ColumnType::Complex(ComplexColumnType::Enum {
                        name: "priority_level".into(),
                        values: EnumValues::Integer(vec![
                            NumValue {
                                name: "Low".into(),
                                value: 0,
                            },
                            NumValue {
                                name: "Medium".into(),
                                value: 1,
                            },
                            NumValue {
                                name: "High".into(),
                                value: 2,
                            },
                        ]),
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
            constraints: vec![TableConstraint::PrimaryKey {
                auto_increment: false,
                columns: vec!["id".into()],
            }],
        };

        let result = render_entity(&table).unwrap();
        assert_snapshot!(result);
    }

    #[test]
    fn test_table_with_foreign_key() {
        let table = TableDef {
            name: "posts".into(),
            description: None,
            columns: vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                col("user_id", ColumnType::Simple(SimpleColumnType::Integer)),
                col("title", ColumnType::Simple(SimpleColumnType::Text)),
            ],
            constraints: vec![
                TableConstraint::PrimaryKey {
                    auto_increment: false,
                    columns: vec!["id".into()],
                },
                TableConstraint::ForeignKey {
                    name: None,
                    columns: vec!["user_id".into()],
                    ref_table: "users".into(),
                    ref_columns: vec!["id".into()],
                    on_delete: None,
                    on_update: None,
                },
            ],
        };

        let result = render_entity(&table).unwrap();
        assert_snapshot!(result);
    }

    #[test]
    fn test_table_with_indexes() {
        let table = TableDef {
            name: "articles".into(),
            description: None,
            columns: vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                col("title", ColumnType::Simple(SimpleColumnType::Text)),
                col(
                    "created_at",
                    ColumnType::Simple(SimpleColumnType::Timestamptz),
                ),
            ],
            constraints: vec![
                TableConstraint::PrimaryKey {
                    auto_increment: false,
                    columns: vec!["id".into()],
                },
                TableConstraint::Index {
                    name: Some("idx_articles_created_at".into()),
                    columns: vec!["created_at".into()],
                },
                TableConstraint::Index {
                    name: None,
                    columns: vec!["title".into()],
                },
            ],
        };

        let result = render_entity(&table).unwrap();
        assert_snapshot!(result);
    }

    #[rstest]
    #[case("hello_world", "HelloWorld")]
    #[case("user_id", "UserId")]
    #[case("simple", "Simple")]
    fn test_to_pascal_case(#[case] input: &str, #[case] expected: &str) {
        assert_eq!(to_pascal_case(input), expected);
    }

    #[rstest]
    #[case("pending", "PENDING")]
    #[case("inProgress", "IN_PROGRESS")]
    #[case("order-status", "ORDER_STATUS")]
    fn test_to_screaming_snake_case(#[case] input: &str, #[case] expected: &str) {
        assert_eq!(to_screaming_snake_case(input), expected);
    }

    #[test]
    fn test_all_simple_column_types() {
        let table = TableDef {
            name: "all_types".into(),
            description: None,
            columns: vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                col("small", ColumnType::Simple(SimpleColumnType::SmallInt)),
                col("big", ColumnType::Simple(SimpleColumnType::BigInt)),
                col("real_num", ColumnType::Simple(SimpleColumnType::Real)),
                col(
                    "double_num",
                    ColumnType::Simple(SimpleColumnType::DoublePrecision),
                ),
                col("text_col", ColumnType::Simple(SimpleColumnType::Text)),
                col("bool_col", ColumnType::Simple(SimpleColumnType::Boolean)),
                col("date_col", ColumnType::Simple(SimpleColumnType::Date)),
                col("time_col", ColumnType::Simple(SimpleColumnType::Time)),
                col("ts_col", ColumnType::Simple(SimpleColumnType::Timestamp)),
                col(
                    "tstz_col",
                    ColumnType::Simple(SimpleColumnType::Timestamptz),
                ),
                col(
                    "interval_col",
                    ColumnType::Simple(SimpleColumnType::Interval),
                ),
                col("bytea_col", ColumnType::Simple(SimpleColumnType::Bytea)),
                col("uuid_col", ColumnType::Simple(SimpleColumnType::Uuid)),
                col("json_col", ColumnType::Simple(SimpleColumnType::Json)),
                col("jsonb_col", ColumnType::Simple(SimpleColumnType::Jsonb)),
                col("inet_col", ColumnType::Simple(SimpleColumnType::Inet)),
                col("cidr_col", ColumnType::Simple(SimpleColumnType::Cidr)),
                col("macaddr_col", ColumnType::Simple(SimpleColumnType::Macaddr)),
                col("xml_col", ColumnType::Simple(SimpleColumnType::Xml)),
            ],
            constraints: vec![TableConstraint::PrimaryKey {
                auto_increment: false,
                columns: vec!["id".into()],
            }],
        };

        let result = render_entity(&table).unwrap();
        assert!(result.contains("SmallInteger"));
        assert!(result.contains("BigInteger"));
        assert!(result.contains("Float")); // Real and DoublePrecision
        assert!(result.contains("Boolean"));
        assert!(result.contains("Date"));
        assert!(result.contains("Time"));
        assert!(result.contains("DateTime"));
        assert!(result.contains("Interval"));
        assert!(result.contains("LargeBinary"));
        assert!(result.contains("Uuid"));
        assert!(result.contains("JSON"));
        assert!(result.contains("String(255)")); // Inet, Cidr, Macaddr
        assert!(result.contains("from datetime import"));
        assert!(result.contains("date"));
        assert!(result.contains("time"));
        assert!(result.contains("datetime"));
        assert!(result.contains("from uuid import UUID"));
    }

    #[test]
    fn test_complex_column_types() {
        let table = TableDef {
            name: "complex_types".into(),
            description: None,
            columns: vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                ColumnDef {
                    name: "varchar_col".into(),
                    r#type: ColumnType::Complex(ComplexColumnType::Varchar { length: 100 }),
                    nullable: false,
                    default: None,
                    comment: None,
                    primary_key: None,
                    unique: None,
                    index: None,
                    foreign_key: None,
                },
                ColumnDef {
                    name: "char_col".into(),
                    r#type: ColumnType::Complex(ComplexColumnType::Char { length: 10 }),
                    nullable: false,
                    default: None,
                    comment: None,
                    primary_key: None,
                    unique: None,
                    index: None,
                    foreign_key: None,
                },
                ColumnDef {
                    name: "numeric_col".into(),
                    r#type: ColumnType::Complex(ComplexColumnType::Numeric {
                        precision: 10,
                        scale: 2,
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
                    name: "custom_col".into(),
                    r#type: ColumnType::Complex(ComplexColumnType::Custom {
                        custom_type: "CUSTOM_TYPE".into(),
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
            constraints: vec![TableConstraint::PrimaryKey {
                auto_increment: false,
                columns: vec!["id".into()],
            }],
        };

        let result = render_entity(&table).unwrap();
        assert!(result.contains("String(100)")); // Varchar
        assert!(result.contains("String(10)")); // Char
        assert!(result.contains("Numeric(10, 2)"));
        assert!(result.contains("\"CUSTOM_TYPE\""));
        assert!(result.contains("from decimal import Decimal"));
    }

    #[test]
    fn test_table_with_composite_unique() {
        let table = TableDef {
            name: "composite_unique".into(),
            description: None,
            columns: vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                col("tenant_id", ColumnType::Simple(SimpleColumnType::Integer)),
                col("name", ColumnType::Simple(SimpleColumnType::Text)),
            ],
            constraints: vec![
                TableConstraint::PrimaryKey {
                    auto_increment: false,
                    columns: vec!["id".into()],
                },
                TableConstraint::Unique {
                    name: Some("uq_tenant_name".into()),
                    columns: vec!["tenant_id".into(), "name".into()],
                },
            ],
        };

        let result = render_entity(&table).unwrap();
        assert!(result.contains("UniqueConstraint"));
        assert!(result.contains("uq_tenant_name"));
    }

    #[test]
    fn test_table_with_server_default() {
        let table = TableDef {
            name: "with_defaults".into(),
            description: None,
            columns: vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
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
                ColumnDef {
                    name: "status".into(),
                    r#type: ColumnType::Simple(SimpleColumnType::Text),
                    nullable: false,
                    default: Some("'active'".into()),
                    comment: None,
                    primary_key: None,
                    unique: None,
                    index: None,
                    foreign_key: None,
                },
                ColumnDef {
                    name: "count".into(),
                    r#type: ColumnType::Simple(SimpleColumnType::Integer),
                    nullable: false,
                    default: Some("0".into()),
                    comment: None,
                    primary_key: None,
                    unique: None,
                    index: None,
                    foreign_key: None,
                },
            ],
            constraints: vec![TableConstraint::PrimaryKey {
                auto_increment: false,
                columns: vec!["id".into()],
            }],
        };

        let result = render_entity(&table).unwrap();
        assert!(result.contains("server_default=text(\"now()\")"));
        assert!(result.contains("server_default='active'"));
        assert!(result.contains("server_default=\"0\""));
        assert!(result.contains("from sqlalchemy import")); // Should include text
    }

    #[test]
    fn test_nullable_enum() {
        let table = TableDef {
            name: "nullable_enum".into(),
            description: None,
            columns: vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                ColumnDef {
                    name: "status".into(),
                    r#type: ColumnType::Complex(ComplexColumnType::Enum {
                        name: "status_type".into(),
                        values: EnumValues::String(vec!["active".into(), "inactive".into()]),
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
            constraints: vec![TableConstraint::PrimaryKey {
                auto_increment: false,
                columns: vec!["id".into()],
            }],
        };

        let result = render_entity(&table).unwrap();
        assert!(result.contains("Optional[StatusType]"));
    }

    #[test]
    fn test_table_without_description() {
        let table = TableDef {
            name: "no_desc".into(),
            description: None,
            columns: vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
            constraints: vec![TableConstraint::PrimaryKey {
                auto_increment: false,
                columns: vec!["id".into()],
            }],
        };

        let result = render_entity(&table).unwrap();
        assert!(result.contains("class NoDesc(DeclarativeBase):"));
        assert!(!result.contains("\"\"\""));
    }

    #[test]
    fn test_to_pascal_case_empty_segment() {
        // Test case with consecutive underscores creating empty segments
        assert_eq!(to_pascal_case("a__b"), "AB");
        assert_eq!(to_pascal_case(""), "");
    }

    #[test]
    fn test_composite_foreign_key_ignored() {
        // Composite FK (multiple columns) should be ignored in fk_info
        let table = TableDef {
            name: "composite_fk".into(),
            description: None,
            columns: vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                col("ref_id1", ColumnType::Simple(SimpleColumnType::Integer)),
                col("ref_id2", ColumnType::Simple(SimpleColumnType::Integer)),
            ],
            constraints: vec![
                TableConstraint::PrimaryKey {
                    auto_increment: false,
                    columns: vec!["id".into()],
                },
                TableConstraint::ForeignKey {
                    name: None,
                    columns: vec!["ref_id1".into(), "ref_id2".into()],
                    ref_table: "other".into(),
                    ref_columns: vec!["id1".into(), "id2".into()],
                    on_delete: None,
                    on_update: None,
                },
            ],
        };

        let result = render_entity(&table).unwrap();
        // Composite FK should not generate ForeignKey() for individual columns
        assert!(!result.contains("ForeignKey(\"other.id1\")"));
        assert!(!result.contains("ForeignKey(\"other.id2\")"));
    }

    #[test]
    fn test_unnamed_composite_unique() {
        let table = TableDef {
            name: "unnamed_unique".into(),
            description: None,
            columns: vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                col("col_a", ColumnType::Simple(SimpleColumnType::Integer)),
                col("col_b", ColumnType::Simple(SimpleColumnType::Integer)),
            ],
            constraints: vec![
                TableConstraint::PrimaryKey {
                    auto_increment: false,
                    columns: vec!["id".into()],
                },
                TableConstraint::Unique {
                    name: None,
                    columns: vec!["col_a".into(), "col_b".into()],
                },
            ],
        };

        let result = render_entity(&table).unwrap();
        assert!(result.contains("UniqueConstraint(\"col_a\", \"col_b\"),"));
    }

    #[test]
    fn test_used_types_smallint() {
        let mut used = UsedTypes::default();
        used.add_column_type(&ColumnType::Simple(SimpleColumnType::SmallInt), false);
        assert!(used.sa_types.contains("SmallInteger"));
    }

    #[test]
    fn test_used_types_integer() {
        let mut used = UsedTypes::default();
        used.add_column_type(&ColumnType::Simple(SimpleColumnType::Integer), false);
        assert!(used.sa_types.contains("Integer"));
    }

    #[test]
    fn test_used_types_bigint() {
        let mut used = UsedTypes::default();
        used.add_column_type(&ColumnType::Simple(SimpleColumnType::BigInt), false);
        assert!(used.sa_types.contains("BigInteger"));
    }

    #[test]
    fn test_used_types_real() {
        let mut used = UsedTypes::default();
        used.add_column_type(&ColumnType::Simple(SimpleColumnType::Real), false);
        assert!(used.sa_types.contains("Float"));
    }

    #[test]
    fn test_used_types_double_precision() {
        let mut used = UsedTypes::default();
        used.add_column_type(
            &ColumnType::Simple(SimpleColumnType::DoublePrecision),
            false,
        );
        assert!(used.sa_types.contains("Float"));
    }

    #[test]
    fn test_used_types_text() {
        let mut used = UsedTypes::default();
        used.add_column_type(&ColumnType::Simple(SimpleColumnType::Text), false);
        assert!(used.sa_types.contains("Text"));
    }

    #[test]
    fn test_used_types_boolean() {
        let mut used = UsedTypes::default();
        used.add_column_type(&ColumnType::Simple(SimpleColumnType::Boolean), false);
        assert!(used.sa_types.contains("Boolean"));
    }

    #[test]
    fn test_used_types_date() {
        let mut used = UsedTypes::default();
        used.add_column_type(&ColumnType::Simple(SimpleColumnType::Date), false);
        assert!(used.sa_types.contains("Date"));
        assert!(used.datetime_types.contains("date"));
    }

    #[test]
    fn test_used_types_time() {
        let mut used = UsedTypes::default();
        used.add_column_type(&ColumnType::Simple(SimpleColumnType::Time), false);
        assert!(used.sa_types.contains("Time"));
        assert!(used.datetime_types.contains("time"));
    }

    #[test]
    fn test_used_types_timestamp() {
        let mut used = UsedTypes::default();
        used.add_column_type(&ColumnType::Simple(SimpleColumnType::Timestamp), false);
        assert!(used.sa_types.contains("DateTime"));
        assert!(used.datetime_types.contains("datetime"));
    }

    #[test]
    fn test_used_types_timestamptz() {
        let mut used = UsedTypes::default();
        used.add_column_type(&ColumnType::Simple(SimpleColumnType::Timestamptz), false);
        assert!(used.sa_types.contains("DateTime"));
        assert!(used.datetime_types.contains("datetime"));
    }

    #[test]
    fn test_used_types_interval() {
        let mut used = UsedTypes::default();
        used.add_column_type(&ColumnType::Simple(SimpleColumnType::Interval), false);
        assert!(used.sa_types.contains("Interval"));
    }

    #[test]
    fn test_used_types_bytea() {
        let mut used = UsedTypes::default();
        used.add_column_type(&ColumnType::Simple(SimpleColumnType::Bytea), false);
        assert!(used.sa_types.contains("LargeBinary"));
    }

    #[test]
    fn test_used_types_uuid() {
        let mut used = UsedTypes::default();
        used.add_column_type(&ColumnType::Simple(SimpleColumnType::Uuid), false);
        assert!(used.sa_types.contains("Uuid"));
        assert!(used.needs_uuid);
    }

    #[test]
    fn test_used_types_json() {
        let mut used = UsedTypes::default();
        used.add_column_type(&ColumnType::Simple(SimpleColumnType::Json), false);
        assert!(used.sa_types.contains("JSON"));
    }

    #[test]
    fn test_used_types_jsonb() {
        let mut used = UsedTypes::default();
        used.add_column_type(&ColumnType::Simple(SimpleColumnType::Jsonb), false);
        assert!(used.sa_types.contains("JSON"));
    }

    #[test]
    fn test_used_types_inet() {
        let mut used = UsedTypes::default();
        used.add_column_type(&ColumnType::Simple(SimpleColumnType::Inet), false);
        assert!(used.sa_types.contains("String"));
    }

    #[test]
    fn test_used_types_cidr() {
        let mut used = UsedTypes::default();
        used.add_column_type(&ColumnType::Simple(SimpleColumnType::Cidr), false);
        assert!(used.sa_types.contains("String"));
    }

    #[test]
    fn test_used_types_macaddr() {
        let mut used = UsedTypes::default();
        used.add_column_type(&ColumnType::Simple(SimpleColumnType::Macaddr), false);
        assert!(used.sa_types.contains("String"));
    }

    #[test]
    fn test_used_types_xml() {
        let mut used = UsedTypes::default();
        used.add_column_type(&ColumnType::Simple(SimpleColumnType::Xml), false);
        assert!(used.sa_types.contains("Text"));
    }

    #[test]
    fn test_used_types_varchar() {
        let mut used = UsedTypes::default();
        used.add_column_type(
            &ColumnType::Complex(ComplexColumnType::Varchar { length: 100 }),
            false,
        );
        assert!(used.sa_types.contains("String"));
    }

    #[test]
    fn test_used_types_char() {
        let mut used = UsedTypes::default();
        used.add_column_type(
            &ColumnType::Complex(ComplexColumnType::Char { length: 10 }),
            false,
        );
        assert!(used.sa_types.contains("String"));
    }

    #[test]
    fn test_used_types_numeric() {
        let mut used = UsedTypes::default();
        used.add_column_type(
            &ColumnType::Complex(ComplexColumnType::Numeric {
                precision: 10,
                scale: 2,
            }),
            false,
        );
        assert!(used.sa_types.contains("Numeric"));
        assert!(used.needs_decimal);
    }

    #[test]
    fn test_used_types_custom() {
        let mut used = UsedTypes::default();
        let initial_count = used.sa_types.len();
        used.add_column_type(
            &ColumnType::Complex(ComplexColumnType::Custom {
                custom_type: "FOO".into(),
            }),
            false,
        );
        // Custom type doesn't add any sa_types - verify count unchanged
        assert_eq!(used.sa_types.len(), initial_count);
    }

    #[test]
    fn test_used_types_enum_string() {
        let mut used = UsedTypes::default();
        used.add_column_type(
            &ColumnType::Complex(ComplexColumnType::Enum {
                name: "status".into(),
                values: EnumValues::String(vec!["a".into()]),
            }),
            false,
        );
        assert!(used.sa_types.contains("Enum"));
    }

    #[test]
    fn test_used_types_enum_integer() {
        let mut used = UsedTypes::default();
        used.add_column_type(
            &ColumnType::Complex(ComplexColumnType::Enum {
                name: "priority".into(),
                values: EnumValues::Integer(vec![NumValue {
                    name: "Low".into(),
                    value: 0,
                }]),
            }),
            false,
        );
        assert!(used.sa_types.contains("Integer"));
    }

    #[test]
    fn test_used_types_nullable_sets_optional() {
        let mut used = UsedTypes::default();
        assert!(!used.needs_optional);
        used.add_column_type(&ColumnType::Simple(SimpleColumnType::Integer), true);
        assert!(used.needs_optional);
    }
}
