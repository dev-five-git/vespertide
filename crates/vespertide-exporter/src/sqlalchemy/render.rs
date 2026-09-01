use super::enums::render_enum;
use super::types::{UsedTypes, column_type_to_python, column_type_to_sqlalchemy};
use crate::parallel_config::{
    PYTHON_EXPORT_PAR_TABLE_MIN_LEN, SQLALCHEMY_EXPORT_PAR_TABLE_THRESHOLD,
};
use crate::utils::common::{join_qualified_refs, join_quoted, push_attr};
use crate::utils::python::collect_composite_fks;
use rayon::prelude::*;
use vespertide_core::schema::column::{ColumnType, ComplexColumnType, EnumValues};
use vespertide_core::schema::constraint::TableConstraint;
use vespertide_core::{ColumnDef, TableDef};
use vespertide_naming::{IdentifierStart, sanitize_identifier};

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

    // Collect single-column foreign key targets once; the import flag below and
    // the per-column render lookups both read from this single scan.
    let fk_info = crate::constraint_scan::single_column_fk_targets(&table.constraints);

    // Check for single-column foreign keys
    if !fk_info.is_empty() {
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
    // `__tablename__` carries the table name, so the class name only has to be
    // valid Python.
    let class_name = sanitize_identifier(&to_pascal_case(&table.name), IdentifierStart::Underscore);

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
    let pk_columns = crate::constraint_scan::primary_key_columns(&table.constraints);

    // Collect unique columns (single-column unique constraints); lookup-only, ordering unused.
    let unique_columns = crate::constraint_scan::single_column_uniques(&table.constraints);

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
                Some((name, columns))
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
                    Some((name, columns))
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

        for &(name, columns) in &indexes {
            let cols_str = join_quoted(columns);
            if let Some(idx_name) = name {
                lines.push(format!("        Index(\"{idx_name}\", {cols_str}),"));
            } else {
                lines.push(format!("        Index(None, {cols_str}),"));
            }
        }

        for &(name, columns) in &composite_uniques {
            let cols_str = join_quoted(columns);
            if let Some(uq_name) = name {
                lines.push(format!(
                    "        UniqueConstraint({cols_str}, name=\"{uq_name}\"),"
                ));
            } else {
                lines.push(format!("        UniqueConstraint({cols_str}),"));
            }
        }

        for fk in &composite_fks {
            let local_cols = join_quoted(&fk.local_cols);
            let ref_cols = join_qualified_refs(fk.ref_table, &fk.ref_cols);
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
    fk_info: Option<&(&str, &str)>,
) {
    // Add column comment
    if let Some(ref comment) = col.comment {
        lines.push(format!("    # {}", comment.replace('\n', " ")));
    }

    let python_type = column_type_to_python(&col.r#type, col.nullable);
    let sa_type = column_type_to_sqlalchemy(&col.r#type);

    // Build the comma-separated attribute list directly into one buffer
    // (preserving the exact fragment order) instead of collecting a
    // `Vec<String>` + `.join(", ")`.
    let mut attrs = String::new();

    // Add SQLAlchemy type
    push_attr(&mut attrs, &sa_type);

    // Foreign key
    if let Some((ref_table, ref_col)) = fk_info {
        push_attr(
            &mut attrs,
            &format!("ForeignKey(\"{ref_table}.{ref_col}\")"),
        );
    }

    // Primary key
    if is_pk {
        push_attr(&mut attrs, "primary_key=True");
    }

    // Nullable
    if !is_pk {
        push_attr(
            &mut attrs,
            if col.nullable {
                "nullable=True"
            } else {
                "nullable=False"
            },
        );
    }

    // Unique
    if is_unique && !is_pk {
        push_attr(&mut attrs, "unique=True");
    }

    // Default value
    if let Some(ref default) = col.default {
        let default_str = default.to_sql();
        // Escape double quotes for embedding in Python strings
        let escaped = default_str.replace('"', "\\\"");
        // Check if it's a function call or literal
        if default_str.contains('(') {
            push_attr(&mut attrs, &format!("server_default=text(\"{escaped}\")"));
        } else if default_str.starts_with('\'') || default_str.starts_with('"') {
            push_attr(&mut attrs, &format!("server_default={default_str}"));
        } else {
            push_attr(&mut attrs, &format!("server_default=\"{escaped}\""));
        }
    }

    // A renamed attribute no longer points at its column by name, so pass the
    // column name positionally whenever the two differ. `attrs` is a single
    // buffer (see `push_attr`), so the positional name is spliced in at the
    // front instead of `Vec::insert(0, ..)`; output is byte-identical to
    // prepending the fragment and re-joining with ", ".
    let attr_name = sanitize_identifier(col.name.as_str(), IdentifierStart::Underscore);
    if attr_name != col.name.as_str() {
        attrs.insert_str(0, &format!("\"{}\", ", col.name));
    }

    lines.push(format!(
        "    {attr_name}: Mapped[{python_type}] = mapped_column({attrs})"
    ));
}

// Naming helpers shared with the `SQLModel` exporter — both Python ORMs
// produce identical PascalCase class names, so the implementation lives in
// `crate::python_naming` and we re-export it here to keep every existing
// `super::render::to_*` path working without churn.
pub(super) use crate::python_naming::to_pascal_case;
