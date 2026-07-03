use std::collections::{HashMap, HashSet};

use super::enums::render_enum;
use super::types::{UsedTypes, column_type_to_python, column_type_to_sqlalchemy};
use crate::parallel_config::{
    PYTHON_EXPORT_PAR_TABLE_MIN_LEN, SQLALCHEMY_EXPORT_PAR_TABLE_THRESHOLD,
};
use crate::utils::python::collect_composite_fks;
use rayon::prelude::*;
use vespertide_core::schema::column::{ColumnType, ComplexColumnType, EnumValues};
use vespertide_core::schema::constraint::TableConstraint;
use vespertide_core::{ColumnDef, TableDef};

pub fn render_entity(table: &TableDef) -> Result<String, String> {
    let mut used_types = UsedTypes::default();
    let part = render_entity_part(table, &mut used_types);

    Ok(assemble_with_imports(&used_types, &[part]))
}

pub fn export(schema: &[TableDef]) -> Result<String, String> {
    let (parts, used_types): (Vec<String>, UsedTypes<'static>) =
        if schema.len() < SQLALCHEMY_EXPORT_PAR_TABLE_THRESHOLD {
            let mut used_types = UsedTypes::default();
            let parts = schema
                .iter()
                .map(|table| render_entity_part(table, &mut used_types))
                .collect::<Vec<_>>();
            (parts, used_types)
        } else {
            schema
                .par_iter()
                .with_min_len(PYTHON_EXPORT_PAR_TABLE_MIN_LEN)
                .map(|table| {
                    let mut local_used = UsedTypes::default();
                    let rendered = render_entity_part(table, &mut local_used);
                    (rendered, local_used)
                })
                .collect::<Vec<_>>()
                .into_iter()
                .fold(
                    (Vec::new(), UsedTypes::default()),
                    |(mut parts, mut acc_used), (part, local_used)| {
                        parts.push(part);
                        acc_used.merge(local_used);
                        (parts, acc_used)
                    },
                )
        };

    Ok(assemble_with_imports(&used_types, &parts))
}

fn render_entity_part(table: &TableDef, used_types: &mut UsedTypes<'static>) -> String {
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
    for col in &table.columns {
        used_types.add_column_type(&col.r#type, col.nullable);
    }

    let composite_fks = collect_composite_fks(table);

    // Check for single-column foreign keys
    let has_single_fk = table.constraints.iter().any(|c| matches!(c, TableConstraint::ForeignKey { columns, ref_columns, .. } if columns.len() == 1 && ref_columns.len() == 1));
    if has_single_fk {
        used_types.sa_types.insert("ForeignKey");
    }

    if !composite_fks.is_empty() {
        used_types.sa_types.insert("ForeignKeyConstraint");
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

    // Render enum classes
    for (enum_name, values) in &enums {
        render_enum(&mut lines, enum_name, values);
        lines.push(String::new());
    }

    // Class definition
    let class_name = to_pascal_case(&table.name);

    // Add table description as docstring
    if let Some(ref desc) = table.description {
        lines.push(format!("class {class_name}(DeclarativeBase):"));
        lines.push(format!("    \"\"\"{}\"\"\"", desc.replace('\n', " ")));
    } else {
        lines.push(format!("class {class_name}(DeclarativeBase):"));
    }

    lines.push(format!("    __tablename__ = \"{}\"", table.name));
    lines.push(String::new());

    // Collect primary key columns; lookup-only, ordering unused.
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

    // Collect unique columns (single-column unique constraints); lookup-only, ordering unused.
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

    // Collect foreign key info; lookup-only, ordering unused.
    let fk_info: HashMap<String, (String, String)> = table
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
                        columns[0].to_string(),
                        (ref_table.to_string(), ref_columns[0].to_string()),
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
            pk_columns.contains(col.name.as_str()),
            unique_columns.contains(col.name.as_str()),
            fk_info.get(col.name.as_str()),
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

    if !indexes.is_empty() || !composite_uniques.is_empty() || !composite_fks.is_empty() {
        lines.push(String::new());
        lines.push("    __table_args__ = (".into());

        for (name, columns) in &indexes {
            let cols_str = columns
                .iter()
                .map(|c| format!("\"{c}\""))
                .collect::<Vec<_>>()
                .join(", ");
            if let Some(idx_name) = name {
                lines.push(format!("        Index(\"{idx_name}\", {cols_str}),"));
            } else {
                lines.push(format!("        Index(None, {cols_str}),"));
            }
        }

        for (name, columns) in &composite_uniques {
            let cols_str = columns
                .iter()
                .map(|c| format!("\"{c}\""))
                .collect::<Vec<_>>()
                .join(", ");
            if let Some(uq_name) = name {
                lines.push(format!(
                    "        UniqueConstraint({cols_str}, name=\"{uq_name}\"),"
                ));
            } else {
                lines.push(format!("        UniqueConstraint({cols_str}),"));
            }
        }

        for fk in &composite_fks {
            let local_cols = fk
                .local_cols
                .iter()
                .map(|col| format!("\"{col}\""))
                .collect::<Vec<_>>()
                .join(", ");
            let ref_cols = fk
                .ref_cols
                .iter()
                .map(|col| format!("\"{}.{}\"", fk.ref_table, col))
                .collect::<Vec<_>>()
                .join(", ");
            lines.push(format!(
                "        ForeignKeyConstraint([{local_cols}], [{ref_cols}]),"
            ));
        }

        lines.push("    )".into());
    }

    lines.push(String::new());

    lines.join("\n")
}

fn assemble_with_imports(used_types: &UsedTypes<'_>, parts: &[String]) -> String {
    let mut lines: Vec<String> = Vec::new();

    lines.push("from __future__ import annotations".into());
    lines.push(String::new());
    if parts.iter().any(|part| part.contains("enum.")) {
        lines.push("import enum".into());
    }

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

    lines.push(String::new());

    let mut sa_imports: Vec<&str> = used_types.sa_types.iter().copied().collect();
    sa_imports.sort_unstable();
    if !sa_imports.is_empty() {
        lines.push(format!("from sqlalchemy import {}", sa_imports.join(", ")));
    }
    lines.push("from sqlalchemy.orm import DeclarativeBase, Mapped, mapped_column".into());
    lines.push(String::new());
    lines.push(String::new());

    lines.push(parts.join("\n"));
    lines.join("\n")
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
        attrs.push(format!("ForeignKey(\"{ref_table}.{ref_col}\")"));
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
        // Escape double quotes for embedding in Python strings
        let escaped = default_str.replace('"', "\\\"");
        // Check if it's a function call or literal
        if default_str.contains('(') {
            attrs.push(format!("server_default=text(\"{escaped}\")"));
        } else if default_str.starts_with('\'') || default_str.starts_with('"') {
            attrs.push(format!("server_default={default_str}"));
        } else {
            attrs.push(format!("server_default=\"{escaped}\""));
        }
    }

    let attrs_str = attrs.join(", ");
    lines.push(format!(
        "    {}: Mapped[{}] = mapped_column({})",
        col.name, python_type, attrs_str
    ));
}

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

pub(super) fn to_screaming_snake_case(s: &str) -> String {
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
