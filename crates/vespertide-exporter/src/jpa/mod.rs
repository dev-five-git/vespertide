use std::collections::{HashMap, HashSet};

use crate::orm::OrmExporter;
use vespertide_core::schema::column::{
    ColumnType, ComplexColumnType, EnumValues, SimpleColumnType,
};
use vespertide_core::schema::constraint::TableConstraint;
use vespertide_core::{ColumnDef, TableDef};

/// Track which Java imports are actually used to generate minimal import statements.
#[derive(Default)]
struct UsedImports {
    java_time_types: HashSet<&'static str>,
    needs_uuid: bool,
    needs_big_decimal: bool,
}

impl UsedImports {
    fn add_column_type(&mut self, col_type: &ColumnType) {
        match col_type {
            ColumnType::Simple(ty) => match ty {
                SimpleColumnType::Date => {
                    self.java_time_types.insert("LocalDate");
                }
                SimpleColumnType::Time => {
                    self.java_time_types.insert("LocalTime");
                }
                SimpleColumnType::Timestamp => {
                    self.java_time_types.insert("LocalDateTime");
                }
                SimpleColumnType::Timestamptz => {
                    self.java_time_types.insert("OffsetDateTime");
                }
                SimpleColumnType::Uuid => {
                    self.needs_uuid = true;
                }
                _ => {}
            },
            ColumnType::Complex(ty) => {
                if let ComplexColumnType::Numeric { .. } = ty {
                    self.needs_big_decimal = true;
                }
            }
        }
    }
}

pub struct JpaExporter;

impl OrmExporter for JpaExporter {
    fn render_entity(&self, table: &TableDef) -> Result<String, String> {
        render_entity(table)
    }

    fn render_entity_with_schema(
        &self,
        table: &TableDef,
        schema: &[TableDef],
    ) -> Result<String, String> {
        render_entity_with_schema(table, schema)
    }
}

/// Render a JPA entity for the given table definition.
pub fn render_entity(table: &TableDef) -> Result<String, String> {
    render_entity_inner(table)
}

/// Render a JPA entity with full schema context for FK chain resolution.
pub fn render_entity_with_schema(table: &TableDef, _schema: &[TableDef]) -> Result<String, String> {
    // FK target types are inferred from ref_table in constraints,
    // so schema context is not needed for basic JPA entity generation.
    render_entity_inner(table)
}

fn render_entity_inner(table: &TableDef) -> Result<String, String> {
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

    // Collect FK info
    let fk_info = collect_fk_info(&table.constraints);

    // Track used imports (skip FK columns — they render as entity references)
    let mut used_imports = UsedImports::default();
    for col in &table.columns {
        if !fk_info.contains_key(&col.name) {
            used_imports.add_column_type(&col.r#type);
        }
    }

    // --- Generate imports ---
    lines.push("import jakarta.persistence.*;".into());

    if used_imports.needs_big_decimal {
        lines.push("import java.math.BigDecimal;".into());
    }

    let mut time_types: Vec<&str> = used_imports.java_time_types.iter().copied().collect();
    time_types.sort();
    for time_type in &time_types {
        lines.push(format!("import java.time.{time_type};"));
    }

    if used_imports.needs_uuid {
        lines.push("import java.util.UUID;".into());
    }

    lines.push(String::new());

    // --- Render enum classes ---
    for (enum_name, values) in &enums {
        render_enum(&mut lines, enum_name, values);
        lines.push(String::new());
    }

    // --- Class definition ---
    let class_name = to_pascal_case(&table.name);

    // Javadoc from table description
    if let Some(ref desc) = table.description {
        lines.push(format!("/** {} */", desc.replace('\n', " ")));
    }

    lines.push("@Entity".into());
    render_table_annotation(&mut lines, &table.name, &table.constraints);
    lines.push(format!("public class {class_name} {{"));
    lines.push(String::new());

    // Collect primary key columns
    let pk_columns: HashSet<String> = table
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

    let auto_increment = table.constraints.iter().any(|c| {
        matches!(
            c,
            TableConstraint::PrimaryKey {
                auto_increment: true,
                ..
            }
        )
    });

    // Collect single-column unique constraints
    let unique_columns: HashSet<String> = table
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

    // --- Render fields ---
    for col in &table.columns {
        let is_pk = pk_columns.contains(&col.name);
        let is_unique = unique_columns.contains(&col.name);

        if let Some(fk) = fk_info.get(&col.name) {
            render_fk_field(&mut lines, col, is_pk, auto_increment, fk);
        } else {
            render_field(&mut lines, col, is_pk, auto_increment, is_unique);
        }
        lines.push(String::new());
    }

    // --- Protected no-arg constructor ---
    lines.push(format!("    protected {class_name}() {{"));
    lines.push("    }".into());

    lines.push("}".into());
    lines.push(String::new());

    Ok(lines.join("\n"))
}

// ---------------------------------------------------------------------------
// FK info collection
// ---------------------------------------------------------------------------

struct FkInfo {
    ref_table: String,
}

fn collect_fk_info(constraints: &[TableConstraint]) -> HashMap<String, FkInfo> {
    constraints
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
                        FkInfo {
                            ref_table: ref_table.clone(),
                        },
                    ))
                } else {
                    None
                }
            } else {
                None
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// @Table annotation
// ---------------------------------------------------------------------------

fn render_table_annotation(
    lines: &mut Vec<String>,
    table_name: &str,
    constraints: &[TableConstraint],
) {
    let indexes: Vec<_> = constraints
        .iter()
        .filter_map(|c| {
            if let TableConstraint::Index { name, columns } = c {
                Some((name.clone(), columns.clone()))
            } else {
                None
            }
        })
        .collect();

    let unique_constraints: Vec<_> = constraints
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

    if indexes.is_empty() && unique_constraints.is_empty() {
        lines.push(format!("@Table(name = \"{table_name}\")"));
        return;
    }

    let mut annotation = format!("@Table(name = \"{table_name}\"");

    if !indexes.is_empty() {
        annotation.push_str(", indexes = {\n");
        for (i, (name, columns)) in indexes.iter().enumerate() {
            let col_list = columns.join(", ");
            let comma = if i < indexes.len() - 1 { "," } else { "" };
            if let Some(idx_name) = name {
                annotation.push_str(&format!(
                    "    @Index(name = \"{idx_name}\", columnList = \"{col_list}\"){comma}\n"
                ));
            } else {
                annotation.push_str(&format!("    @Index(columnList = \"{col_list}\"){comma}\n"));
            }
        }
        annotation.push('}');
    }

    if !unique_constraints.is_empty() {
        annotation.push_str(", uniqueConstraints = {\n");
        for (i, (name, columns)) in unique_constraints.iter().enumerate() {
            let cols = columns
                .iter()
                .map(|c| format!("\"{c}\""))
                .collect::<Vec<_>>()
                .join(", ");
            let comma = if i < unique_constraints.len() - 1 {
                ","
            } else {
                ""
            };
            if let Some(uq_name) = name {
                annotation.push_str(&format!(
                    "    @UniqueConstraint(name = \"{uq_name}\", columnNames = {{{cols}}}){comma}\n"
                ));
            } else {
                annotation.push_str(&format!(
                    "    @UniqueConstraint(columnNames = {{{cols}}}){comma}\n"
                ));
            }
        }
        annotation.push('}');
    }

    annotation.push(')');
    lines.push(annotation);
}

// ---------------------------------------------------------------------------
// Enum rendering
// ---------------------------------------------------------------------------

fn render_enum(lines: &mut Vec<String>, name: &str, values: &EnumValues) {
    let class_name = to_pascal_case(name);

    match values {
        EnumValues::String(vals) => {
            // Use lowercase constants to match DB values with @Enumerated(EnumType.STRING)
            lines.push(format!("enum {class_name} {{"));
            let last_idx = vals.len().saturating_sub(1);
            for (i, val) in vals.iter().enumerate() {
                let sep = if i < last_idx { "," } else { ";" };
                lines.push(format!("    {val}{sep}"));
            }
            lines.push("}".into());
        }
        EnumValues::Integer(vals) => {
            lines.push(format!("enum {class_name} {{"));
            let last_idx = vals.len().saturating_sub(1);
            for (i, val) in vals.iter().enumerate() {
                let name_upper = val.name.to_uppercase();
                let sep = if i < last_idx { "," } else { ";" };
                lines.push(format!("    {name_upper}({}){sep}", val.value));
            }
            lines.push(String::new());
            lines.push("    private final int value;".into());
            lines.push(String::new());
            lines.push(format!("    {class_name}(int value) {{"));
            lines.push("        this.value = value;".into());
            lines.push("    }".into());
            lines.push(String::new());
            lines.push("    public int getValue() {".into());
            lines.push("        return value;".into());
            lines.push("    }".into());
            lines.push("}".into());
        }
    }
}

// ---------------------------------------------------------------------------
// Field rendering
// ---------------------------------------------------------------------------

fn render_field(
    lines: &mut Vec<String>,
    col: &ColumnDef,
    is_pk: bool,
    auto_increment: bool,
    is_unique: bool,
) {
    let java_type = java_type_for_column(col);
    let field_name = to_camel_case(&col.name);

    // Javadoc comment
    if let Some(ref comment) = col.comment {
        lines.push(format!("    /** {} */", comment.replace('\n', " ")));
    }

    // @Id + @GeneratedValue
    if is_pk {
        lines.push("    @Id".into());
        if auto_increment {
            lines.push("    @GeneratedValue(strategy = GenerationType.IDENTITY)".into());
        }
    }

    // @Enumerated for string enum types
    if let ColumnType::Complex(ComplexColumnType::Enum {
        values: EnumValues::String(_),
        ..
    }) = &col.r#type
    {
        lines.push("    @Enumerated(EnumType.STRING)".into());
    }

    // @Column annotation
    let column_attrs = build_column_attrs(col, is_pk, is_unique);
    lines.push(format!("    @Column({column_attrs})"));

    // Field declaration with optional default initializer
    let default_init = build_default_initializer(col);
    if let Some(ref init) = default_init {
        lines.push(format!("    private {java_type} {field_name} = {init};"));
    } else {
        lines.push(format!("    private {java_type} {field_name};"));
    }
}

fn render_fk_field(
    lines: &mut Vec<String>,
    col: &ColumnDef,
    is_pk: bool,
    auto_increment: bool,
    fk: &FkInfo,
) {
    let entity_type = to_pascal_case(&fk.ref_table);
    let field_name = infer_fk_field_name(&col.name);

    // Javadoc comment
    if let Some(ref comment) = col.comment {
        lines.push(format!("    /** {} */", comment.replace('\n', " ")));
    }

    // @Id + @GeneratedValue (rare for FK columns, but handle composite PK+FK)
    if is_pk {
        lines.push("    @Id".into());
        if auto_increment {
            lines.push("    @GeneratedValue(strategy = GenerationType.IDENTITY)".into());
        }
    }

    // @ManyToOne
    lines.push("    @ManyToOne(fetch = FetchType.LAZY)".into());

    // @JoinColumn
    let mut join_attrs: Vec<String> = vec![format!("name = \"{}\"", col.name)];
    if !col.nullable {
        join_attrs.push("nullable = false".into());
    }
    lines.push(format!("    @JoinColumn({})", join_attrs.join(", ")));

    // Field declaration
    lines.push(format!("    private {entity_type} {field_name};"));
}

// ---------------------------------------------------------------------------
// @Column attribute building
// ---------------------------------------------------------------------------

fn build_column_attrs(col: &ColumnDef, is_pk: bool, is_unique: bool) -> String {
    let mut attrs: Vec<String> = vec![format!("name = \"{}\"", col.name)];

    // nullable (skip for PK — always not-null)
    if !is_pk && !col.nullable {
        attrs.push("nullable = false".into());
    }

    // unique (skip for PK)
    if is_unique && !is_pk {
        attrs.push("unique = true".into());
    }

    // Type-specific attributes
    match &col.r#type {
        ColumnType::Complex(ComplexColumnType::Varchar { length }) => {
            attrs.push(format!("length = {length}"));
        }
        ColumnType::Complex(ComplexColumnType::Char { length }) => {
            attrs.push(format!("length = {length}"));
        }
        ColumnType::Complex(ComplexColumnType::Numeric { precision, scale }) => {
            attrs.push(format!("precision = {precision}"));
            attrs.push(format!("scale = {scale}"));
        }
        ColumnType::Simple(SimpleColumnType::Text | SimpleColumnType::Xml) => {
            attrs.push("columnDefinition = \"TEXT\"".into());
        }
        ColumnType::Simple(SimpleColumnType::Json) => {
            attrs.push("columnDefinition = \"JSON\"".into());
        }
        ColumnType::Simple(SimpleColumnType::Bytea) => {
            attrs.push("columnDefinition = \"BYTEA\"".into());
        }
        ColumnType::Simple(SimpleColumnType::Interval) => {
            attrs.push("columnDefinition = \"INTERVAL\"".into());
        }
        ColumnType::Complex(ComplexColumnType::Custom { custom_type }) => {
            attrs.push(format!("columnDefinition = \"{custom_type}\""));
        }
        _ => {}
    }

    attrs.join(", ")
}

// ---------------------------------------------------------------------------
// Default value handling
// ---------------------------------------------------------------------------

fn build_default_initializer(col: &ColumnDef) -> Option<String> {
    let default = col.default.as_ref()?;
    let default_str = default.to_sql();

    // Skip server-side defaults (function calls like NOW())
    if default_str.contains('(') {
        return None;
    }

    // Boolean defaults
    if default_str == "true" {
        return Some("true".into());
    }
    if default_str == "false" {
        return Some("false".into());
    }

    // String literal defaults
    if default_str.starts_with('\'') || default_str.starts_with('"') {
        let stripped = default_str.trim_matches(|c| c == '\'' || c == '"');
        return Some(format!("\"{}\"", stripped.replace('"', "\\\"")));
    }

    // Numeric defaults
    if default_str.parse::<i64>().is_ok() || default_str.parse::<f64>().is_ok() {
        return Some(default_str);
    }

    None
}

// ---------------------------------------------------------------------------
// Type mapping
// ---------------------------------------------------------------------------

fn column_type_to_java(col_type: &ColumnType) -> &'static str {
    match col_type {
        ColumnType::Simple(ty) => match ty {
            SimpleColumnType::SmallInt => "Short",
            SimpleColumnType::Integer => "Integer",
            SimpleColumnType::BigInt => "Long",
            SimpleColumnType::Real => "Float",
            SimpleColumnType::DoublePrecision => "Double",
            SimpleColumnType::Boolean => "Boolean",
            SimpleColumnType::Text | SimpleColumnType::Xml => "String",
            SimpleColumnType::Date => "LocalDate",
            SimpleColumnType::Time => "LocalTime",
            SimpleColumnType::Timestamp => "LocalDateTime",
            SimpleColumnType::Timestamptz => "OffsetDateTime",
            SimpleColumnType::Interval => "String",
            SimpleColumnType::Bytea => "byte[]",
            SimpleColumnType::Uuid => "UUID",
            SimpleColumnType::Json => "String",
            SimpleColumnType::Inet | SimpleColumnType::Cidr | SimpleColumnType::Macaddr => "String",
        },
        ColumnType::Complex(ty) => match ty {
            ComplexColumnType::Varchar { .. } | ComplexColumnType::Char { .. } => "String",
            ComplexColumnType::Numeric { .. } => "BigDecimal",
            ComplexColumnType::Custom { .. } => "String",
            // Integer enums are stored as Integer in JPA
            ComplexColumnType::Enum {
                values: EnumValues::Integer(_),
                ..
            } => "Integer",
            // String enums use the generated enum class — handled separately
            ComplexColumnType::Enum {
                values: EnumValues::String(_),
                ..
            } => "String", // placeholder, overridden in render_field
        },
    }
}

/// Get the Java type for a column, including enum class names.
fn java_type_for_column(col: &ColumnDef) -> String {
    if let ColumnType::Complex(ComplexColumnType::Enum {
        name,
        values: EnumValues::String(_),
    }) = &col.r#type
    {
        to_pascal_case(name)
    } else {
        column_type_to_java(&col.r#type).to_string()
    }
}

// ---------------------------------------------------------------------------
// Naming utilities
// ---------------------------------------------------------------------------

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

fn to_camel_case(s: &str) -> String {
    let pascal = to_pascal_case(s);
    let mut chars = pascal.chars();
    match chars.next() {
        None => String::new(),
        Some(first) => {
            let lower: String = first.to_lowercase().collect();
            format!("{lower}{}", chars.collect::<String>())
        }
    }
}

fn infer_fk_field_name(column_name: &str) -> String {
    let base = column_name.strip_suffix("_id").unwrap_or(column_name);
    to_camel_case(base)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use insta::assert_snapshot;
    use rstest::rstest;
    use vespertide_core::{DefaultValue, NumValue};

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
                auto_increment: true,
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
                                name: "low".into(),
                                value: 0,
                            },
                            NumValue {
                                name: "medium".into(),
                                value: 10,
                            },
                            NumValue {
                                name: "high".into(),
                                value: 20,
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
                ColumnDef {
                    name: "author_id".into(),
                    r#type: ColumnType::Simple(SimpleColumnType::Integer),
                    nullable: false,
                    default: None,
                    comment: None,
                    primary_key: None,
                    unique: None,
                    index: None,
                    foreign_key: None,
                },
                col("title", ColumnType::Simple(SimpleColumnType::Text)),
            ],
            constraints: vec![
                TableConstraint::PrimaryKey {
                    auto_increment: true,
                    columns: vec!["id".into()],
                },
                TableConstraint::ForeignKey {
                    name: None,
                    columns: vec!["author_id".into()],
                    ref_table: "users".into(),
                    ref_columns: vec!["id".into()],
                    on_delete: None,
                    on_update: None,
                },
                TableConstraint::Index {
                    name: Some("ix_posts__author_id".into()),
                    columns: vec!["author_id".into()],
                },
            ],
        };

        let result = render_entity(&table).unwrap();
        assert_snapshot!(result);
    }

    #[test]
    fn test_table_with_all_simple_types() {
        let table = TableDef {
            name: "type_test".into(),
            description: None,
            columns: vec![
                col(
                    "col_smallint",
                    ColumnType::Simple(SimpleColumnType::SmallInt),
                ),
                col("col_integer", ColumnType::Simple(SimpleColumnType::Integer)),
                col("col_bigint", ColumnType::Simple(SimpleColumnType::BigInt)),
                col("col_real", ColumnType::Simple(SimpleColumnType::Real)),
                col(
                    "col_double",
                    ColumnType::Simple(SimpleColumnType::DoublePrecision),
                ),
                col("col_text", ColumnType::Simple(SimpleColumnType::Text)),
                col("col_boolean", ColumnType::Simple(SimpleColumnType::Boolean)),
                col("col_date", ColumnType::Simple(SimpleColumnType::Date)),
                col("col_time", ColumnType::Simple(SimpleColumnType::Time)),
                col(
                    "col_timestamp",
                    ColumnType::Simple(SimpleColumnType::Timestamp),
                ),
                col(
                    "col_timestamptz",
                    ColumnType::Simple(SimpleColumnType::Timestamptz),
                ),
                col(
                    "col_interval",
                    ColumnType::Simple(SimpleColumnType::Interval),
                ),
                col("col_bytea", ColumnType::Simple(SimpleColumnType::Bytea)),
                col("col_uuid", ColumnType::Simple(SimpleColumnType::Uuid)),
                col("col_json", ColumnType::Simple(SimpleColumnType::Json)),
                col("col_inet", ColumnType::Simple(SimpleColumnType::Inet)),
                col("col_cidr", ColumnType::Simple(SimpleColumnType::Cidr)),
                col("col_macaddr", ColumnType::Simple(SimpleColumnType::Macaddr)),
                col("col_xml", ColumnType::Simple(SimpleColumnType::Xml)),
            ],
            constraints: vec![TableConstraint::PrimaryKey {
                auto_increment: false,
                columns: vec!["col_integer".into()],
            }],
        };

        let result = render_entity(&table).unwrap();
        assert_snapshot!(result);
    }

    #[test]
    fn test_table_with_complex_types() {
        let table = TableDef {
            name: "products".into(),
            description: None,
            columns: vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                col(
                    "name",
                    ColumnType::Complex(ComplexColumnType::Varchar { length: 200 }),
                ),
                col(
                    "price",
                    ColumnType::Complex(ComplexColumnType::Numeric {
                        precision: 12,
                        scale: 2,
                    }),
                ),
                col(
                    "code",
                    ColumnType::Complex(ComplexColumnType::Char { length: 10 }),
                ),
                col(
                    "metadata",
                    ColumnType::Complex(ComplexColumnType::Custom {
                        custom_type: "JSONB".into(),
                    }),
                ),
            ],
            constraints: vec![TableConstraint::PrimaryKey {
                auto_increment: true,
                columns: vec!["id".into()],
            }],
        };

        let result = render_entity(&table).unwrap();
        assert_snapshot!(result);
    }

    #[test]
    fn test_table_with_defaults() {
        let table = TableDef {
            name: "articles".into(),
            description: None,
            columns: vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                ColumnDef {
                    name: "published".into(),
                    r#type: ColumnType::Simple(SimpleColumnType::Boolean),
                    nullable: false,
                    default: Some(DefaultValue::Bool(false)),
                    comment: None,
                    primary_key: None,
                    unique: None,
                    index: None,
                    foreign_key: None,
                },
                ColumnDef {
                    name: "view_count".into(),
                    r#type: ColumnType::Simple(SimpleColumnType::Integer),
                    nullable: false,
                    default: Some(DefaultValue::Integer(0)),
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
                    default: Some(DefaultValue::String("'draft'".into())),
                    comment: None,
                    primary_key: None,
                    unique: None,
                    index: None,
                    foreign_key: None,
                },
            ],
            constraints: vec![TableConstraint::PrimaryKey {
                auto_increment: true,
                columns: vec!["id".into()],
            }],
        };

        let result = render_entity(&table).unwrap();
        assert_snapshot!(result);
    }

    #[test]
    fn test_table_with_composite_constraints() {
        let table = TableDef {
            name: "order_items".into(),
            description: None,
            columns: vec![
                col("order_id", ColumnType::Simple(SimpleColumnType::Integer)),
                col("product_id", ColumnType::Simple(SimpleColumnType::Integer)),
                col("quantity", ColumnType::Simple(SimpleColumnType::Integer)),
            ],
            constraints: vec![
                TableConstraint::PrimaryKey {
                    auto_increment: false,
                    columns: vec!["order_id".into(), "product_id".into()],
                },
                TableConstraint::ForeignKey {
                    name: None,
                    columns: vec!["order_id".into()],
                    ref_table: "orders".into(),
                    ref_columns: vec!["id".into()],
                    on_delete: None,
                    on_update: None,
                },
                TableConstraint::ForeignKey {
                    name: None,
                    columns: vec!["product_id".into()],
                    ref_table: "products".into(),
                    ref_columns: vec!["id".into()],
                    on_delete: None,
                    on_update: None,
                },
                TableConstraint::Unique {
                    name: Some("uq_order_items__order_product".into()),
                    columns: vec!["order_id".into(), "product_id".into()],
                },
                TableConstraint::Index {
                    name: Some("ix_order_items__order_id".into()),
                    columns: vec!["order_id".into()],
                },
            ],
        };

        let result = render_entity(&table).unwrap();
        assert_snapshot!(result);
    }

    #[test]
    fn test_nullable_columns() {
        let table = TableDef {
            name: "profiles".into(),
            description: None,
            columns: vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                ColumnDef {
                    name: "bio".into(),
                    r#type: ColumnType::Simple(SimpleColumnType::Text),
                    nullable: true,
                    default: None,
                    comment: None,
                    primary_key: None,
                    unique: None,
                    index: None,
                    foreign_key: None,
                },
                ColumnDef {
                    name: "avatar_url".into(),
                    r#type: ColumnType::Complex(ComplexColumnType::Varchar { length: 500 }),
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
                auto_increment: true,
                columns: vec!["id".into()],
            }],
        };

        let result = render_entity(&table).unwrap();
        assert_snapshot!(result);
    }

    #[test]
    fn test_unnamed_index_and_unique() {
        let table = TableDef {
            name: "events".into(),
            description: None,
            columns: vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                col("venue_id", ColumnType::Simple(SimpleColumnType::Integer)),
                col("date", ColumnType::Simple(SimpleColumnType::Date)),
            ],
            constraints: vec![
                TableConstraint::PrimaryKey {
                    auto_increment: false,
                    columns: vec!["id".into()],
                },
                TableConstraint::Index {
                    name: None,
                    columns: vec!["venue_id".into(), "date".into()],
                },
                TableConstraint::Unique {
                    name: None,
                    columns: vec!["venue_id".into(), "date".into()],
                },
            ],
        };

        let result = render_entity(&table).unwrap();
        assert_snapshot!(result);
    }

    #[test]
    fn test_fk_with_comment_and_auto_increment() {
        let table = TableDef {
            name: "child".into(),
            description: None,
            columns: vec![
                ColumnDef {
                    name: "parent_id".into(),
                    r#type: ColumnType::Simple(SimpleColumnType::Integer),
                    nullable: false,
                    default: None,
                    comment: Some("References parent table".into()),
                    primary_key: None,
                    unique: None,
                    index: None,
                    foreign_key: None,
                },
                col("value", ColumnType::Simple(SimpleColumnType::Text)),
            ],
            constraints: vec![
                TableConstraint::PrimaryKey {
                    auto_increment: true,
                    columns: vec!["parent_id".into()],
                },
                TableConstraint::ForeignKey {
                    name: None,
                    columns: vec!["parent_id".into()],
                    ref_table: "parent".into(),
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
    fn test_server_default_and_true_boolean() {
        let table = TableDef {
            name: "logs".into(),
            description: None,
            columns: vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                ColumnDef {
                    name: "active".into(),
                    r#type: ColumnType::Simple(SimpleColumnType::Boolean),
                    nullable: false,
                    default: Some(DefaultValue::Bool(true)),
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
                    default: Some(DefaultValue::String("NOW()".into())),
                    comment: None,
                    primary_key: None,
                    unique: None,
                    index: None,
                    foreign_key: None,
                },
                ColumnDef {
                    name: "score".into(),
                    r#type: ColumnType::Simple(SimpleColumnType::Real),
                    nullable: false,
                    default: Some(DefaultValue::Float(1.5)),
                    comment: None,
                    primary_key: None,
                    unique: None,
                    index: None,
                    foreign_key: None,
                },
                ColumnDef {
                    name: "tag".into(),
                    r#type: ColumnType::Simple(SimpleColumnType::Text),
                    nullable: false,
                    default: Some(DefaultValue::String("UNKNOWN_EXPR".into())),
                    comment: None,
                    primary_key: None,
                    unique: None,
                    index: None,
                    foreign_key: None,
                },
            ],
            constraints: vec![TableConstraint::PrimaryKey {
                auto_increment: true,
                columns: vec!["id".into()],
            }],
        };

        let result = render_entity(&table).unwrap();
        assert_snapshot!(result);
    }

    #[test]
    fn test_column_type_to_java_string_enum() {
        // Exercises the string enum branch in column_type_to_java
        let ty = ColumnType::Complex(ComplexColumnType::Enum {
            name: "status".into(),
            values: EnumValues::String(vec!["a".into()]),
        });
        assert_eq!(column_type_to_java(&ty), "String");
    }

    #[rstest]
    #[case("order_item", "OrderItem")]
    #[case("users", "Users")]
    #[case("a", "A")]
    #[case("user_profile_image", "UserProfileImage")]
    #[case("", "")]
    fn test_to_pascal_case(#[case] input: &str, #[case] expected: &str) {
        assert_eq!(to_pascal_case(input), expected);
    }

    #[rstest]
    #[case("created_at", "createdAt")]
    #[case("id", "id")]
    #[case("user_profile_image", "userProfileImage")]
    #[case("", "")]
    fn test_to_camel_case(#[case] input: &str, #[case] expected: &str) {
        assert_eq!(to_camel_case(input), expected);
    }

    #[rstest]
    #[case("customer_id", "customer")]
    #[case("author_user_id", "authorUser")]
    #[case("parent", "parent")]
    fn test_infer_fk_field_name(#[case] input: &str, #[case] expected: &str) {
        assert_eq!(infer_fk_field_name(input), expected);
    }
}
