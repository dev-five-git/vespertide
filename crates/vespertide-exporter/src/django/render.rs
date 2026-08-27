use std::collections::{HashMap, HashSet};

use super::enums::render_enum;
use super::types::{UsedImports, build_field_kwargs, django_field_type, reference_action_str};
use crate::utils::python::collect_composite_fks;
use vespertide_core::schema::column::{ColumnType, ComplexColumnType};
use vespertide_core::schema::constraint::TableConstraint;
use vespertide_core::{ReferenceAction, TableDef};
use vespertide_naming::{IdentifierStart, sanitize_identifier};

pub fn render_entity(table: &TableDef) -> Result<String, String> {
    let mut used = UsedImports::default();
    let body = render_entity_part(table, &mut used, &[], None);
    Ok(assemble_with_imports(&used, &[body]))
}

/// Render a single table with full schema context so many-to-many junction
/// tables can be recognized and exposed as `ManyToManyField(..., through=...)`.
pub fn render_entity_with_schema(table: &TableDef, schema: &[TableDef]) -> Result<String, String> {
    render_entity_with_schema_and_config(table, schema, None)
}

/// Same as [`render_entity_with_schema`], but with an optional `app_label`
/// (from `vespertide.json`'s `django` config) written into every model's
/// `Meta` class.
pub fn render_entity_with_schema_and_config(
    table: &TableDef,
    schema: &[TableDef],
    app_label: Option<&str>,
) -> Result<String, String> {
    let mut used = UsedImports::default();
    let m2m_fields = find_many_to_many_fields(table, schema);
    let body = render_entity_part(table, &mut used, &m2m_fields, app_label);
    Ok(assemble_with_imports(&used, &[body]))
}

pub fn export(schema: &[TableDef]) -> Result<String, String> {
    export_with_config(schema, None)
}

/// Same as [`export`], but with an optional `app_label` written into every
/// model's `Meta` class.
pub fn export_with_config(schema: &[TableDef], app_label: Option<&str>) -> Result<String, String> {
    let mut used = UsedImports::default();
    let parts: Vec<String> = schema
        .iter()
        .map(|t| {
            let m2m_fields = find_many_to_many_fields(t, schema);
            render_entity_part(t, &mut used, &m2m_fields, app_label)
        })
        .collect();
    Ok(assemble_with_imports(&used, &parts))
}

/// Recognize many-to-many junction tables (composite PK, 2+ FKs, all FK
/// columns part of the PK) that reference `table`, and render the
/// corresponding `ManyToManyField` lines for the *other* side of each
/// junction. Purely self-referential junctions (every FK pointing back at
/// `table`) are skipped rather than guessed at.
fn find_many_to_many_fields(table: &TableDef, schema: &[TableDef]) -> Vec<String> {
    let mut matches: Vec<(String, String)> = Vec::new(); // (target_table, junction_table)

    for other in schema {
        if other.name == table.name {
            continue;
        }

        let other_pk: HashSet<String> = other
            .constraints
            .iter()
            .filter_map(|c| {
                if let TableConstraint::PrimaryKey { columns, .. } = c {
                    Some(
                        columns
                            .iter()
                            .map(|c| c.as_str().to_owned())
                            .collect::<Vec<_>>(),
                    )
                } else {
                    None
                }
            })
            .flatten()
            .collect();
        if other_pk.len() < 2 {
            continue;
        }

        let fks: Vec<(Vec<String>, String)> = other
            .constraints
            .iter()
            .filter_map(|c| {
                if let TableConstraint::ForeignKey {
                    columns, ref_table, ..
                } = c
                {
                    Some((
                        columns.iter().map(|c| c.as_str().to_owned()).collect(),
                        ref_table.as_str().to_owned(),
                    ))
                } else {
                    None
                }
            })
            .collect();
        if fks.len() < 2 {
            continue;
        }

        let all_fk_cols_in_pk = fks
            .iter()
            .all(|(cols, _)| cols.iter().all(|c| other_pk.contains(c.as_str())));
        if !all_fk_cols_in_pk {
            continue;
        }

        if !fks
            .iter()
            .any(|(_, ref_table)| ref_table.as_str() == table.name.as_str())
        {
            continue;
        }
        if fks
            .iter()
            .all(|(_, ref_table)| ref_table.as_str() == table.name.as_str())
        {
            continue;
        }

        for (_, ref_table) in &fks {
            if ref_table.as_str() == table.name.as_str() {
                continue;
            }
            if schema.iter().any(|t| t.name.as_str() == ref_table.as_str()) {
                matches.push((ref_table.clone(), other.name.as_str().to_owned()));
            }
        }
    }

    let mut target_counts: HashMap<String, usize> = HashMap::new();
    for (target, _) in &matches {
        *target_counts.entry(target.clone()).or_default() += 1;
    }

    let mut used_names: HashSet<String> = HashSet::new();
    matches
        .iter()
        .map(|(target, junction)| {
            let base = pluralize(target);
            let field_name = if target_counts.get(target).copied().unwrap_or(0) > 1 {
                unique_name(&format!("{base}_via_{junction}"), &mut used_names)
            } else {
                unique_name(&base, &mut used_names)
            };
            let target_class = sanitize_identifier(&to_pascal_case(target), IdentifierStart::Underscore);
            let junction_class =
                sanitize_identifier(&to_pascal_case(junction), IdentifierStart::Underscore);
            format!(
                "    {field_name} = models.ManyToManyField(\"{target_class}\", through=\"{junction_class}\", related_name=\"+\")"
            )
        })
        .collect()
}

fn pluralize(name: &str) -> String {
    if name.ends_with('s') {
        name.to_string()
    } else {
        format!("{name}s")
    }
}

fn unique_name(base: &str, used: &mut HashSet<String>) -> String {
    if used.insert(base.to_string()) {
        return base.to_string();
    }
    let mut n = 2;
    loop {
        let candidate = format!("{base}_{n}");
        if used.insert(candidate.clone()) {
            return candidate;
        }
        n += 1;
    }
}

fn render_entity_part(
    table: &TableDef,
    used: &mut UsedImports,
    extra_fields: &[String],
    app_label: Option<&str>,
) -> String {
    let mut lines: Vec<String> = Vec::new();

    // --- Constraint lookups ---
    let pk_columns: HashSet<String> = table
        .constraints
        .iter()
        .filter_map(|c| {
            if let TableConstraint::PrimaryKey { columns, .. } = c {
                Some(
                    columns
                        .iter()
                        .map(|c| c.as_str().to_owned())
                        .collect::<Vec<_>>(),
                )
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

    let is_composite_pk = pk_columns.len() > 1;

    // Column order (not just membership) matters for CompositePrimaryKey's
    // positional args, so capture it separately from the `pk_columns` set.
    let pk_columns_ordered: Vec<String> = table
        .constraints
        .iter()
        .find_map(|c| {
            if let TableConstraint::PrimaryKey { columns, .. } = c {
                Some(columns.iter().map(|c| c.as_str().to_owned()).collect())
            } else {
                None
            }
        })
        .unwrap_or_default();

    let single_unique_cols: HashSet<String> = table
        .constraints
        .iter()
        .filter_map(|c| {
            if let TableConstraint::Unique { columns, .. } = c {
                if columns.len() == 1 {
                    Some(columns[0].as_str().to_owned())
                } else {
                    None
                }
            } else {
                None
            }
        })
        .collect();

    // single-column FK info: col_name → (ref_table, on_delete, on_update)
    let fk_map: HashMap<String, (&str, Option<&ReferenceAction>, Option<&ReferenceAction>)> = table
        .constraints
        .iter()
        .filter_map(|c| {
            if let TableConstraint::ForeignKey {
                columns,
                ref_table,
                ref_columns,
                on_delete,
                on_update,
                ..
            } = c
                && columns.len() == 1
                && ref_columns.len() == 1
            {
                return Some((
                    columns[0].as_str().to_owned(),
                    (ref_table.as_str(), on_delete.as_ref(), on_update.as_ref()),
                ));
            }
            None
        })
        .collect();

    // Enum class names for this table's columns
    let enum_class_map: HashMap<&str, String> = table
        .columns
        .iter()
        .filter_map(|col| {
            if let ColumnType::Complex(ComplexColumnType::Enum { name, .. }) = &col.r#type {
                Some((
                    col.name.as_str(),
                    sanitize_identifier(&to_pascal_case(name), IdentifierStart::Underscore),
                ))
            } else {
                None
            }
        })
        .collect();

    // --- Enum class definitions ---
    let mut seen_enums: HashSet<String> = HashSet::new();
    for col in &table.columns {
        if let ColumnType::Complex(ComplexColumnType::Enum { name, values }) = &col.r#type {
            let class_name =
                sanitize_identifier(&to_pascal_case(name), IdentifierStart::Underscore);
            if seen_enums.insert(class_name.clone()) {
                render_enum(&mut lines, &class_name, values);
                lines.push(String::new());
            }
        }
    }

    // --- Class declaration ---
    let class_name = sanitize_identifier(&to_pascal_case(&table.name), IdentifierStart::Underscore);
    if let Some(ref desc) = table.description {
        lines.push(format!("class {class_name}(models.Model):"));
        lines.push(format!("    \"\"\"{}\"\"\"", desc.replace('\n', " ")));
        lines.push(String::new());
    } else {
        lines.push(format!("class {class_name}(models.Model):"));
    }

    // Composite PK: Django (5.2+) represents this natively via
    // `pk = models.CompositePrimaryKey(...)`, referencing each column by its
    // attname (a ForeignKey's attname is always `{field_name}_id`, regardless
    // of any `db_column` override). Without this, Django would fall back to
    // adding its own implicit auto `id` PK, which doesn't correspond to any
    // real uniqueness constraint on the actual table.
    if is_composite_pk {
        let attnames: Vec<String> = pk_columns_ordered
            .iter()
            .map(|col| {
                if fk_map.contains_key(col.as_str()) {
                    let (field_name, _) = fk_field_name(col);
                    format!("{field_name}_id")
                } else {
                    col.clone()
                }
            })
            .collect();
        let args = attnames
            .iter()
            .map(|a| format!("\"{a}\""))
            .collect::<Vec<_>>()
            .join(", ");
        lines.push(format!("    pk = models.CompositePrimaryKey({args})"));
    }

    // --- Fields ---
    // Sanitizing distinct column names (e.g. `a_id` -> `a`, `a` -> `a`) can
    // collapse two originally-distinct columns onto the same Python
    // attribute name; disambiguate with a numeric suffix rather than
    // silently emitting a duplicate class attribute.
    let mut used_field_names: HashSet<String> = HashSet::new();
    for col in &table.columns {
        let is_pk = pk_columns.contains(col.name.as_str());
        let is_unique = single_unique_cols.contains(col.name.as_str());

        if let Some(ref comment) = col.comment {
            lines.push(format!("    # {}", comment.replace('\n', " ")));
        }

        if let Some(&(ref_table, on_delete, on_update)) = fk_map.get(col.name.as_str()) {
            render_fk_field(
                &mut lines,
                &col.name,
                ref_table,
                on_delete,
                on_update,
                col.nullable,
                &mut used_field_names,
            );
        } else {
            let effective_pk = is_pk && !is_composite_pk;
            let field_type = django_field_type(
                &col.r#type,
                effective_pk,
                auto_increment && !is_composite_pk,
            );
            let field_name = unique_name(
                &sanitize_identifier(col.name.as_str(), IdentifierStart::Underscore),
                &mut used_field_names,
            );
            let db_column = if field_name == col.name.as_str() {
                None
            } else {
                Some(col.name.as_str())
            };
            let kwargs = build_field_kwargs(
                &col.r#type,
                effective_pk,
                is_unique,
                col.nullable,
                col.default.as_ref(),
                enum_class_map.get(col.name.as_str()).map(String::as_str),
                db_column,
                used,
            );
            let kwargs_str = kwargs.join(", ");
            if kwargs_str.is_empty() {
                lines.push(format!("    {field_name} = {field_type}()"));
            } else {
                lines.push(format!("    {field_name} = {field_type}({kwargs_str})"));
            }
        }
    }

    for line in extra_fields {
        lines.push(line.clone());
    }

    // Composite (multi-column) FKs have no native Django ORM field — surface
    // them as a comment rather than silently dropping the relationship info.
    // The individual columns still render above as plain scalar fields, and
    // referential integrity is enforced by the generated database schema.
    for fk in collect_composite_fks(table) {
        let local = fk.local_cols.join(", ");
        let refs = fk.ref_cols.join(", ");
        lines.push(format!(
            "    # composite foreign key: ({local}) -> {}({refs})",
            fk.ref_table
        ));
    }

    // --- Meta class ---
    let indexes: Vec<_> = table
        .constraints
        .iter()
        .filter_map(|c| {
            if let TableConstraint::Index { name, columns } = c {
                Some((name.as_deref(), columns.as_slice()))
            } else {
                None
            }
        })
        .collect();

    let composite_uniques: Vec<_> = table
        .constraints
        .iter()
        .filter_map(|c| {
            if let TableConstraint::Unique { name, columns, .. } = c {
                if columns.len() > 1 {
                    Some((name.as_deref(), columns.as_slice()))
                } else {
                    None
                }
            } else {
                None
            }
        })
        .collect();

    lines.push(String::new());
    lines.push("    class Meta:".into());
    lines.push(format!("        db_table = \"{}\"", table.name));
    if let Some(label) = app_label {
        lines.push(format!("        app_label = \"{label}\""));
    }

    if !indexes.is_empty() {
        lines.push("        indexes = [".into());
        for (name, cols) in &indexes {
            let fields = cols
                .iter()
                .map(|c| format!("\"{c}\""))
                .collect::<Vec<_>>()
                .join(", ");
            if let Some(n) = name {
                lines.push(format!(
                    "            models.Index(fields=[{fields}], name=\"{n}\"),"
                ));
            } else {
                lines.push(format!("            models.Index(fields=[{fields}]),"));
            }
        }
        lines.push("        ]".into());
    }

    if !composite_uniques.is_empty() {
        lines.push("        constraints = [".into());
        for (name, cols) in &composite_uniques {
            let fields = cols
                .iter()
                .map(|c| format!("\"{c}\""))
                .collect::<Vec<_>>()
                .join(", ");
            if let Some(n) = name {
                lines.push(format!(
                    "            models.UniqueConstraint(fields=[{fields}], name=\"{n}\"),"
                ));
            } else {
                lines.push(format!(
                    "            models.UniqueConstraint(fields=[{fields}]),"
                ));
            }
        }
        lines.push("        ]".into());
    }

    lines.push(String::new());
    lines.join("\n")
}

fn render_fk_field(
    lines: &mut Vec<String>,
    col_name: &str,
    ref_table: &str,
    on_delete: Option<&ReferenceAction>,
    on_update: Option<&ReferenceAction>,
    nullable: bool,
    used_field_names: &mut HashSet<String>,
) {
    let (field_name, db_column) = fk_field_name(col_name);
    // The `_id` strip can collapse two distinct columns onto the same
    // attribute name (e.g. `a_id` -> `a` colliding with a real column `a`).
    let deduped_field_name = unique_name(&field_name, used_field_names);
    let db_column =
        db_column.or_else(|| (deduped_field_name != field_name).then(|| col_name.to_string()));
    let field_name = deduped_field_name;
    let ref_class = sanitize_identifier(&to_pascal_case(ref_table), IdentifierStart::Underscore);
    let on_delete_str = on_delete.map_or("models.RESTRICT", reference_action_str);

    let _ = on_update; // Django ForeignKey has no on_update param; silently ignored

    let mut kwargs = vec![
        format!("\"{ref_class}\""),
        format!("on_delete={on_delete_str}"),
    ];
    if let Some(db_col) = db_column {
        kwargs.push(format!("db_column=\"{db_col}\""));
    }
    kwargs.push("related_name=\"+\"".into());
    if nullable {
        kwargs.push("null=True".into());
        kwargs.push("blank=True".into());
    }

    let kwargs_str = kwargs.join(", ");
    lines.push(format!(
        "    {field_name} = models.ForeignKey({kwargs_str})"
    ));
}

/// Returns (field_name, Option<db_column>).
/// If col_name ends with `_id`, strip it — Django automatically appends `_id`.
/// Otherwise, emit db_column explicitly so Django uses the raw column name.
/// Either way, `field_name` is sanitized into a valid Python identifier; if
/// that sanitization (or the `_id` strip) changes anything, `db_column` is
/// set to the original column name so the DB mapping isn't lost.
fn fk_field_name(col_name: &str) -> (String, Option<String>) {
    if let Some(base) = col_name.strip_suffix("_id") {
        let sanitized = sanitize_identifier(base, IdentifierStart::Underscore);
        if sanitized == base {
            (sanitized, None)
        } else {
            (sanitized, Some(col_name.to_string()))
        }
    } else {
        (
            sanitize_identifier(col_name, IdentifierStart::Underscore),
            Some(col_name.to_string()),
        )
    }
}

fn assemble_with_imports(used: &UsedImports, parts: &[String]) -> String {
    let mut lines: Vec<String> = Vec::new();

    lines.push("from __future__ import annotations".into());
    lines.push(String::new());

    if used.needs_timezone {
        lines.push("from django.utils import timezone".into());
    }
    if used.needs_uuid_default {
        lines.push("import uuid".into());
    }

    lines.push("from django.db import models".into());
    lines.push(String::new());
    lines.push(String::new());

    lines.push(parts.join("\n"));
    lines.join("\n")
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

pub(super) fn to_upper_snake_case(s: &str) -> String {
    let mut result = String::new();
    let chars: Vec<char> = s.chars().collect();
    for (i, &c) in chars.iter().enumerate() {
        if c == '-' || c == ' ' {
            if !result.ends_with('_') {
                result.push('_');
            }
        } else if c == '_' {
            result.push('_');
        } else if c.is_uppercase() && i > 0 && !result.ends_with('_') {
            // Only split on camelCase transitions (lowercase/digit → uppercase).
            // Adjacent uppercase letters (e.g. "ERROR") are not split.
            let prev = chars[i - 1];
            if prev.is_lowercase() || prev.is_ascii_digit() {
                result.push('_');
            }
            result.push(c);
        } else {
            result.push(c.to_ascii_uppercase());
        }
    }
    // Python identifiers cannot start with a digit
    if result.starts_with(|c: char| c.is_ascii_digit()) {
        result.insert(0, '_');
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[rstest::rstest]
    #[case("pending", "PENDING")]
    #[case("in_progress", "IN_PROGRESS")]
    #[case("inProgress", "IN_PROGRESS")]
    #[case("ERROR_LEVEL", "ERROR_LEVEL")]
    #[case("info-level", "INFO_LEVEL")]
    #[case("1critical", "_1CRITICAL")]
    fn test_to_upper_snake_case(#[case] input: &str, #[case] expected: &str) {
        assert_eq!(to_upper_snake_case(input), expected);
    }

    #[rstest::rstest]
    #[case("author_id", "author", None)]
    #[case("user_id", "user", None)]
    #[case("parent", "parent", Some("parent"))]
    #[case("ref", "ref", Some("ref"))]
    fn test_fk_field_name(
        #[case] col: &str,
        #[case] expected_field: &str,
        #[case] expected_db_col: Option<&str>,
    ) {
        let (field, db_col) = fk_field_name(col);
        assert_eq!(field, expected_field);
        assert_eq!(db_col.as_deref(), expected_db_col);
    }

    #[test]
    fn test_to_pascal_case_double_underscore() {
        // Double underscore produces an empty word, triggering the None arm in to_pascal_case
        assert_eq!(to_pascal_case("order__item"), "OrderItem");
        assert_eq!(to_pascal_case("_leading"), "Leading");
        assert_eq!(to_pascal_case("trailing_"), "Trailing");
    }

    #[test]
    fn test_unique_name_double_collision_appends_incrementing_suffix() {
        let mut used = HashSet::new();
        used.insert("tag".to_string());
        used.insert("tag_2".to_string());
        assert_eq!(unique_name("tag", &mut used), "tag_3");
    }
}
