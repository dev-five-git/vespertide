use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;

use vespertide_core::schema::column::{
    ColumnType, ComplexColumnType, EnumValues, SimpleColumnType,
};
use vespertide_core::schema::constraint::TableConstraint;
use vespertide_core::{ColumnDef, TableDef};

use crate::jpa::types::{UsedImports, java_type_for_column};

pub(super) fn render_entity_inner(table: &TableDef) -> String {
    render_entity_with_imports(table).0
}

pub(super) fn render_entity_with_imports(table: &TableDef) -> (String, UsedImports) {
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
        if !fk_info.contains_key(col.name.as_str()) {
            used_imports.add_column_type(&col.r#type);
        }
    }

    // --- Generate imports ---
    lines.push("import jakarta.persistence.*;".into());

    if used_imports.needs_big_decimal {
        lines.push("import java.math.BigDecimal;".into());
    }

    let mut time_types: Vec<&str> = used_imports.java_time_types.iter().copied().collect();
    time_types.sort_unstable();
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
        .map(|col| col.to_string())
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
                    Some(columns[0].to_string())
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
        let is_pk = pk_columns.contains(col.name.as_str());
        let is_unique = unique_columns.contains(col.name.as_str());

        if let Some(fk) = fk_info.get(col.name.as_str()) {
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

    (lines.join("\n"), used_imports)
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
                        columns[0].to_string(),
                        FkInfo {
                            ref_table: ref_table.to_string(),
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
            if let TableConstraint::Unique { name, columns, .. } = c {
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
                let _ = writeln!(
                    annotation,
                    "    @Index(name = \"{idx_name}\", columnList = \"{col_list}\"){comma}"
                );
            } else {
                let _ = writeln!(annotation, "    @Index(columnList = \"{col_list}\"){comma}");
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
                let _ = writeln!(
                    annotation,
                    "    @UniqueConstraint(name = \"{uq_name}\", columnNames = {{{cols}}}){comma}"
                );
            } else {
                let _ = writeln!(
                    annotation,
                    "    @UniqueConstraint(columnNames = {{{cols}}}){comma}"
                );
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
        ColumnType::Complex(
            ComplexColumnType::Varchar { length } | ComplexColumnType::Char { length },
        ) => {
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
// Naming utilities
// ---------------------------------------------------------------------------

pub(super) fn to_pascal_case(s: &str) -> String {
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

pub(super) fn to_camel_case(s: &str) -> String {
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

pub(super) fn infer_fk_field_name(column_name: &str) -> String {
    let base = column_name.strip_suffix("_id").unwrap_or(column_name);
    to_camel_case(base)
}
