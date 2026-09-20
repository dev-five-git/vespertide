use std::collections::{HashMap, HashSet};

use super::enums::render_enum;
use super::types::{
    UsedImports, build_default, build_field_kwargs, django_field_type, on_delete_for,
};
use crate::constraint_scan::{
    FkDetails, junction_targets, primary_key, primary_key_columns, single_column_fk_details,
    single_column_uniques,
};
use crate::python_naming::to_pascal_case;
use crate::scope_names::{ScopeNames, scope_of};
use crate::utils::common::{claim_binding, collect_composite_fks, string_literal};
use crate::utils::python::{is_python_keyword, unmangled};
use vespertide_core::schema::column::{ColumnType, ComplexColumnType};
use vespertide_core::schema::constraint::TableConstraint;
use vespertide_core::{ReferenceAction, TableDef};
use vespertide_naming::{
    IdentifierStart, build_index_name, build_unique_constraint_name, pluralize, sanitize_identifier,
};

pub fn render_entity(table: &TableDef) -> Result<String, String> {
    let mut used = UsedImports::default();
    let names = module_names(std::slice::from_ref(table));
    let body = render_entity_part(table, &[], &mut used, &names, None);
    Ok(assemble_with_imports(&used, &[body]))
}

/// Render a single table with full schema context so many-to-many junction
/// tables can be recognized and exposed as `ManyToManyField(..., through=...)`.
pub fn render_entity_with_schema(table: &TableDef, schema: &[TableDef]) -> Result<String, String> {
    let mut used = UsedImports::default();
    let names = module_names(scope_of(table, schema));
    let body = render_entity_part(table, schema, &mut used, &names, None);
    Ok(assemble_with_imports(&used, &[body]))
}

pub fn export(schema: &[TableDef]) -> Result<String, String> {
    export_with_config(schema, None)
}

/// Same as [`export`], but with an optional `app_label` (from
/// `vespertide.json`'s `django` config) written into every model's `Meta`
/// class.
pub fn export_with_config(schema: &[TableDef], app_label: Option<&str>) -> Result<String, String> {
    let mut used = UsedImports::default();
    let names = module_names(schema);
    let parts: Vec<String> = schema
        .iter()
        .map(|t| render_entity_part(t, schema, &mut used, &names, app_label))
        .collect();
    Ok(assemble_with_imports(&used, &parts))
}

/// The other side of every many-to-many junction that links `table`: each
/// `(target, junction)` pair whose target `schema` also knows, in schema
/// order. Purely self-referential junctions yield no pairs, and neither does
/// a junction that reaches either end by a composite key: that key renders as
/// a comment, and a `through` model needs a real `ForeignKey` to both ends
/// (fields.E336) — nor can Django relate to the composite-key model such a
/// key points at (fields.E347).
fn many_to_many_targets<'a>(table: &TableDef, schema: &'a [TableDef]) -> Vec<(&'a str, &'a str)> {
    let mut pairs = Vec::new();
    for junction in schema {
        if junction.name == table.name {
            continue;
        }
        let junction_pk = primary_key_columns(&junction.constraints);
        let Some(targets) = junction_targets(table, junction, &junction_pk) else {
            continue;
        };
        let reached_by_foreign_key: HashSet<&str> = single_column_fk_details(&junction.constraints)
            .values()
            .filter(|fk| is_relatable(fk, schema))
            .map(|fk| fk.ref_table)
            .collect();
        if !reached_by_foreign_key.contains(table.name.as_str()) {
            continue;
        }
        for target in targets {
            if reached_by_foreign_key.contains(target.as_str())
                && schema.iter().any(|t| t.name == *target)
            {
                pairs.push((target.as_str(), junction.name.as_str()));
            }
        }
    }
    pairs
}

fn render_entity_part(
    table: &TableDef,
    schema: &[TableDef],
    used: &mut UsedImports,
    names: &ScopeNames,
    app_label: Option<&str>,
) -> String {
    let mut lines: Vec<String> = Vec::new();
    let m2m = many_to_many_targets(table, schema);

    // --- Constraint lookups ---
    let pk_columns = primary_key_columns(&table.constraints);

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
    let pk_columns_ordered = primary_key(&table.constraints)
        .map(TableConstraint::columns)
        .unwrap_or_default();

    let single_unique_cols = single_column_uniques(&table.constraints);
    let fk_map = single_column_fk_details(&table.constraints);

    let class_name = class_of(names, &table.name);

    // Enum class names for this table's columns, as claimed in the module.
    let enum_class_map: HashMap<&str, String> = table
        .columns
        .iter()
        .filter_map(|col| {
            if let ColumnType::Complex(ComplexColumnType::Enum { name, .. }) = &col.r#type {
                let class = names.enum_type(&table.name, name).to_string();
                Some((col.name.as_str(), class))
            } else {
                None
            }
        })
        .collect();

    // --- Enum class definitions ---
    let mut seen_enums: HashSet<&str> = HashSet::new();
    for col in &table.columns {
        if let ColumnType::Complex(ComplexColumnType::Enum { values, .. }) = &col.r#type {
            let enum_class = enum_class_map[col.name.as_str()].as_str();
            if seen_enums.insert(enum_class) {
                render_enum(&mut lines, enum_class, values);
                lines.push(String::new());
            }
        }
    }

    // --- Class declaration ---
    if let Some(ref desc) = table.description {
        lines.push(format!("class {class_name}(models.Model):"));
        // A docstring keeps its triple quotes: `string_literal` supplies the
        // inner pair and escapes every `\` and `"` of the text, the two
        // characters that could end it early.
        let docstring = string_literal(&desc.replace('\n', " "));
        lines.push(format!("    \"\"{docstring}\"\""));
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
    // Rendered after the fields, which is where the attnames come from,
    // but emitted here at the top of the class body.
    let composite_pk_at = lines.len();

    // --- Fields ---
    // Sanitizing distinct column names (e.g. `a_id` -> `a`, `a` -> `a`) can
    // collapse two originally-distinct columns onto the same Python
    // attribute name; disambiguate with a numeric suffix rather than
    // silently emitting a duplicate class attribute.
    let (field_names, mut used_field_names) = column_field_names(table, schema);
    let mut attnames: HashMap<&str, String> = HashMap::new();
    let mut unrelatable: Vec<String> = Vec::new();
    for col in &table.columns {
        let field_name = field_names[col.name.as_str()].as_str();
        let is_pk = pk_columns.contains(col.name.as_str());
        let is_unique = single_unique_cols.contains(col.name.as_str());

        if let Some(ref comment) = col.comment {
            lines.push(format!("    # {}", comment.replace('\n', " ")));
        }

        let effective_pk = is_pk && !is_composite_pk;
        let fk = fk_map.get(col.name.as_str());
        if let Some(fk) = fk.filter(|fk| !is_relatable(fk, schema)) {
            unrelatable.push(format!(
                "    # foreign key: ({}) -> {}({})",
                col.name, fk.ref_table, fk.ref_column
            ));
        }
        let attname = if let Some(fk) = fk.filter(|fk| is_relatable(fk, schema)) {
            let field = ForeignKeyField {
                column: &col.name,
                name: field_name,
                target_class: class_of(names, fk.ref_table),
                to_field: to_field(fk, schema),
                on_delete: fk.on_delete,
                default: col
                    .default
                    .as_ref()
                    .and_then(|dv| build_default(&col.r#type, &dv.to_sql(), used)),
                is_pk: effective_pk,
                is_unique,
                nullable: col.nullable,
            };
            lines.push(field.render());
            // A ForeignKey's attname is `{field}_id` whatever `db_column` says.
            format!("{field_name}_id")
        } else {
            let field_type = django_field_type(
                &col.r#type,
                effective_pk,
                auto_increment && !is_composite_pk,
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
            field_name.to_string()
        };
        attnames.insert(col.name.as_str(), attname);
    }

    if is_composite_pk {
        let args = pk_columns_ordered
            .iter()
            .map(|col| format!("\"{}\"", attname_of(&attnames, col.as_str())))
            .collect::<Vec<_>>()
            .join(", ");
        lines.insert(
            composite_pk_at,
            format!("    pk = models.CompositePrimaryKey({args})"),
        );
    }

    // --- Many-to-many fields: the other side of each junction linking this
    // table. Named after the pluralized target, or `{target}_via_{junction}`
    // when two junctions reach one target; claimed after the columns so a
    // field never shadows a scalar of the same name.
    let mut target_counts: HashMap<&str, usize> = HashMap::new();
    for (target, _) in &m2m {
        *target_counts.entry(target).or_default() += 1;
    }
    for (target, junction) in &m2m {
        let base = pluralize(target);
        let raw = if target_counts[target] > 1 {
            format!("{base}_via_{junction}")
        } else {
            base
        };
        let field_name = django_field_name(&raw, &mut used_field_names);
        let target_class = class_of(names, target);
        let junction_class = class_of(names, junction);
        lines.push(format!(
            "    {field_name} = models.ManyToManyField(\"{target_class}\", through=\"{junction_class}\", related_name=\"+\")"
        ));
    }

    // Composite (multi-column) FKs have no native Django ORM field, and neither
    // has a key into a composite-key model — surface them as a comment rather
    // than silently dropping the relationship info. The individual columns
    // still render above as plain scalar fields, and referential integrity is
    // enforced by the generated database schema.
    lines.extend(unrelatable);
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
    // vespertide owns the DDL; `makemigrations` must not try to create or
    // alter these tables.
    lines.push("        managed = False".into());
    lines.push(format!(
        "        db_table = {}",
        string_literal(&table.name)
    ));
    if let Some(label) = app_label {
        lines.push(format!("        app_label = {}", string_literal(label)));
    }

    if !indexes.is_empty() {
        lines.push("        indexes = [".into());
        for (name, cols) in &indexes {
            let fields = cols
                .iter()
                .map(|c| format!("\"{}\"", attname_of(&attnames, c)))
                .collect::<Vec<_>>()
                .join(", ");
            // The name the SQL layer gave the index — a source name is the
            // builder's key, not the final name — while it fits Django's
            // 30-character cap (models.E034). Past the cap the index goes
            // unnamed: Django never creates the index of an unmanaged model,
            // so the name it makes up is never used.
            let n = build_index_name(&table.name, cols, *name);
            if n.len() <= 30 {
                lines.push(format!(
                    "            models.Index(fields=[{fields}], name={}),",
                    string_literal(&n)
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
                .map(|c| format!("\"{}\"", attname_of(&attnames, c)))
                .collect::<Vec<_>>()
                .join(", ");
            // Likewise the database's name. A constraint must carry one, and
            // Django puts no cap on it.
            let n = build_unique_constraint_name(&table.name, cols, *name);
            lines.push(format!(
                "            models.UniqueConstraint(fields=[{fields}], name={}),",
                string_literal(&n)
            ));
        }
        lines.push("        ]".into());
    }

    lines.push(String::new());
    lines.join("\n")
}

/// A single-column foreign key Django can express, as the field it renders.
struct ForeignKeyField<'a> {
    column: &'a str,
    name: &'a str,
    target_class: String,
    to_field: Option<String>,
    on_delete: Option<&'a ReferenceAction>,
    default: Option<String>,
    is_pk: bool,
    is_unique: bool,
    nullable: bool,
}

impl ForeignKeyField<'_> {
    fn render(&self) -> String {
        // Django reads a ForeignKey through `{field}_id`, so the column keeps
        // its database name exactly when the stripped base survives every
        // rename.
        let db_column = (format!("{}_id", self.name) != self.column).then_some(self.column);
        let null = self.nullable && !self.is_pk;
        // `ON UPDATE` has no counterpart on a Django ForeignKey.
        let on_delete = on_delete_for(self.on_delete, self.default.is_some(), null);

        let mut kwargs = vec![
            format!("\"{}\"", self.target_class),
            format!("on_delete={on_delete}"),
        ];
        if let Some(to_field) = &self.to_field {
            kwargs.push(format!("to_field={}", string_literal(to_field)));
        }
        if self.is_pk {
            kwargs.push("primary_key=True".into());
        }
        if let Some(default) = &self.default {
            kwargs.push(format!("default={default}"));
        }
        if let Some(db_column) = db_column {
            kwargs.push(format!("db_column={}", string_literal(db_column)));
        }
        kwargs.push("related_name=\"+\"".into());
        if null {
            kwargs.push("null=True".into());
            kwargs.push("blank=True".into());
        }

        // A FK that is the PK or unique holds at most one row per target:
        // Django's one-to-one. `ForeignKey(unique=True)` only draws fields.W342
        // pointing here.
        let field_class = if self.is_pk || self.is_unique {
            "models.OneToOneField"
        } else {
            "models.ForeignKey"
        };
        format!("    {} = {field_class}({})", self.name, kwargs.join(", "))
    }
}

/// Whether Django can express `fk` as a relation. It cannot relate to a model
/// with a composite primary key (fields.E347), and the field a key references
/// must be unique (fields.E311): the target's primary key, or a column with a
/// unique of its own. Such a key stays a plain column. A target outside
/// `schema` is taken at its word.
fn is_relatable(fk: &FkDetails, schema: &[TableDef]) -> bool {
    schema
        .iter()
        .find(|t| t.name.as_str() == fk.ref_table)
        .is_none_or(|target| {
            let pk = primary_key_columns(&target.constraints);
            pk.len() < 2
                && (pk.contains(fk.ref_column)
                    || single_column_uniques(&target.constraints).contains(fk.ref_column))
        })
}

/// The `to_field` a foreign key needs: the target's field for the referenced
/// column, whenever that column is not the target's primary key — which is
/// what Django would otherwise join on.
fn to_field(fk: &FkDetails, schema: &[TableDef]) -> Option<String> {
    let target = schema.iter().find(|t| t.name.as_str() == fk.ref_table)?;
    if primary_key_columns(&target.constraints).contains(fk.ref_column) {
        return None;
    }
    let (target_fields, _) = column_field_names(target, schema);
    Some(
        target_fields
            .get(fk.ref_column)
            .map_or(fk.ref_column, String::as_str)
            .to_string(),
    )
}

/// The Django field name of every column of `table`, claimed in declaration
/// order, with the set those claims filled. A foreign key Django can express
/// is named after its relation (`user_id` -> `user`), every other column after
/// itself. Sanitizing distinct columns (`a_id` -> `a`, `a` -> `a`) can land two
/// of them on one attribute; the later one is numbered.
fn column_field_names<'a>(
    table: &'a TableDef,
    schema: &[TableDef],
) -> (HashMap<&'a str, String>, HashSet<String>) {
    let fk_map = single_column_fk_details(&table.constraints);
    let mut taken = HashSet::new();
    let names = table
        .columns
        .iter()
        .map(|col| {
            let is_relation = fk_map
                .get(col.name.as_str())
                .is_some_and(|fk| is_relatable(fk, schema));
            let name = if is_relation {
                let base = vespertide_naming::infer_relation_field_name(&col.name);
                claim_relation_field_name(&django_identifier(base), &mut taken)
            } else {
                django_field_name(&col.name, &mut taken)
            };
            (col.name.as_str(), name)
        })
        .collect();
    (names, taken)
}

/// Claim a relation's field name together with its attname: Django stores the
/// key under `{field}_id`, so a plain `owner_id` column next to an `owner` key
/// would share that attribute with it (models.E006). The first numbered name
/// with both free wins.
fn claim_relation_field_name(preferred: &str, taken: &mut HashSet<String>) -> String {
    let mut name = preferred.to_string();
    let mut n = 2usize;
    while taken.contains(&name) || taken.contains(&format!("{name}_id")) {
        name = format!("{preferred}{n}");
        n += 1;
    }
    taken.insert(format!("{name}_id"));
    taken.insert(name.clone());
    name
}

/// Every class `tables` declare in their module: models, then choices classes.
fn module_names(tables: &[TableDef]) -> ScopeNames {
    ScopeNames::collect(tables, model_class_name, enum_class_name)
}

/// The class a table is declared as; a table outside the module's schema — a
/// foreign key may point there — keeps its natural name.
fn class_of(names: &ScopeNames, table: &str) -> String {
    names
        .table(table)
        .map_or_else(|| model_class_name(table), str::to_string)
}

/// A table's model class. Django rejects a model name that starts with `_`
/// (models.E023), so a name that cannot lead with its own first character
/// gains a letter instead.
fn model_class_name(table: &str) -> String {
    sanitize_identifier(&to_pascal_case(table), IdentifierStart::Letter)
}

/// An enum's choices class. A model names it from inside its own class body,
/// where Python would mangle a `__`-led name.
fn enum_class_name(name: &str) -> String {
    unmangled(sanitize_identifier(
        &to_pascal_case(name),
        IdentifierStart::Underscore,
    ))
}

/// What Django calls a column inside `Meta.indexes`, `Meta.constraints` and
/// `CompositePrimaryKey`: the declared field name, or a ForeignKey's attname.
/// Those three resolve against field names only — never `db_column` — so a
/// column whose name had to be escaped is unreachable under its database
/// spelling.
fn attname_of<'a>(attnames: &'a HashMap<&str, String>, column: &'a str) -> &'a str {
    attnames.get(column).map_or(column, String::as_str)
}

/// Attributes every Django model already has: `Model`'s public API and what its
/// metaclass adds. A field of the same name replaces the method (`save`,
/// `clean`: a `TypeError` at the first call), fails the checks (`pk`:
/// fields.E003, `check`: models.E020), stops the module importing (`objects`)
/// or is rebound by the `class Meta` written below the fields (`Meta`).
const MODEL_ATTRIBUTES: &[&str] = &[
    "DoesNotExist",
    "Meta",
    "MultipleObjectsReturned",
    "NotUpdated",
    "adelete",
    "arefresh_from_db",
    "asave",
    "check",
    "clean",
    "clean_fields",
    "date_error_message",
    "delete",
    "from_db",
    "full_clean",
    "get_constraints",
    "get_deferred_fields",
    "objects",
    "pk",
    "prepare_database_save",
    "refresh_from_db",
    "save",
    "save_base",
    "serializable_value",
    "unique_error_message",
    "validate_constraints",
    "validate_unique",
];

/// A column's Django field name: its [`django_identifier`], claimed against
/// `taken`. Callers emit `db_column` whenever the result differs from the
/// column.
fn django_field_name(column: &str, taken: &mut HashSet<String>) -> String {
    claim_binding(django_identifier(column), taken)
}

/// A Python identifier that also passes Django's field checks — no `__` (the
/// lookup separator, fields.E002), no trailing `_` (fields.E001), not a
/// keyword and not one of the model's own attributes. The repairs are
/// `inspectdb`'s, so a renamed field reads the way Django's own tooling would
/// spell it.
fn django_identifier(column: &str) -> String {
    let mut name = sanitize_identifier(column, IdentifierStart::Underscore);
    while name.contains("__") {
        name = name.replace("__", "_");
    }
    if name.ends_with('_') {
        name.push_str("field");
    }
    if MODEL_ATTRIBUTES.contains(&name.as_str()) || is_python_keyword(&name) {
        name.push_str("_field");
    }
    name
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

#[cfg(test)]
mod tests {
    use vespertide_core::schema::column::SimpleColumnType;

    use super::*;
    use crate::tests::fixtures::{fk, pk, simple};

    #[rstest::rstest]
    #[case::plain("author", "author")]
    #[case::keyword("from", "from_field")]
    #[case::reserved_pk("pk", "pk_field")]
    #[case::model_method("save", "save_field")]
    #[case::model_check("check", "check_field")]
    #[case::default_manager("objects", "objects_field")]
    #[case::options_class("Meta", "Meta_field")]
    #[case::lookup_separator("user__name", "user_name")]
    #[case::trailing_underscore("total_", "total_field")]
    #[case::separator_from_sanitizing("a--b", "a_b")]
    #[case::digit_led("1st", "_1st")]
    fn django_field_name_passes_the_field_checks(#[case] column: &str, #[case] expected: &str) {
        let mut taken = HashSet::new();
        assert_eq!(django_field_name(column, &mut taken), expected);
    }

    fn owners(constraints: Vec<TableConstraint>) -> TableDef {
        TableDef {
            name: "owners".into(),
            description: None,
            columns: vec![
                simple("id", SimpleColumnType::Integer),
                simple("region", SimpleColumnType::Integer),
                simple("code", SimpleColumnType::Integer),
            ],
            constraints,
        }
    }

    fn unique(column: &str) -> TableConstraint {
        TableConstraint::Unique {
            name: None,
            columns: vec![column.into()],
            strategy: vespertide_core::UniqueConstraintStrategy::DeleteDuplicates {
                keep: vespertide_core::KeepPolicy::First,
            },
        }
    }

    /// A key's attname is `{field}_id`, so the key and a column of that name
    /// cannot both keep theirs, whichever the table declares first.
    #[rstest::rstest]
    #[case::key_first(&["owner", "owner_id"], &["owner", "owner_id2"])]
    #[case::column_first(&["owner_id", "owner"], &["owner_id", "owner2"])]
    fn a_key_claims_its_attname_with_its_field_name(
        #[case] columns: &[&str],
        #[case] expected: &[&str],
    ) {
        let table = TableDef {
            name: "pets".into(),
            description: None,
            columns: columns
                .iter()
                .map(|name| simple(name, SimpleColumnType::Integer))
                .collect(),
            constraints: vec![fk(&["owner"], "owners", &["id"])],
        };
        let (names, _) = column_field_names(&table, &[]);
        let names: Vec<&str> = columns.iter().map(|c| names[c].as_str()).collect();
        assert_eq!(names, expected);
    }

    #[rstest::rstest]
    #[case::primary_key(vec![pk(&["id"])], "id", true)]
    #[case::unique_column(vec![pk(&["id"]), unique("code")], "code", true)]
    #[case::column_that_is_not_unique(vec![pk(&["id"])], "code", false)]
    #[case::part_of_a_composite_key(vec![pk(&["id", "region"])], "id", false)]
    fn a_key_is_a_relation_only_onto_a_unique_field_of_a_single_key_model(
        #[case] target_constraints: Vec<TableConstraint>,
        #[case] ref_column: &str,
        #[case] expected: bool,
    ) {
        let schema = [owners(target_constraints)];
        let key = fk(&["owner_id"], "owners", &[ref_column]);
        let fk_map = single_column_fk_details(std::slice::from_ref(&key));
        assert_eq!(is_relatable(&fk_map["owner_id"], &schema), expected);
        // A target the schema does not hold is taken at its word.
        assert!(is_relatable(&fk_map["owner_id"], &[]));
    }

    #[rstest::rstest]
    #[case::plain("order_status", "OrderStatus")]
    #[case::digit_led("1st", "_1st")]
    #[case::leading_run_python_would_mangle("--kind", "_kind")]
    fn enum_class_name_can_be_named_from_a_model(#[case] name: &str, #[case] expected: &str) {
        assert_eq!(enum_class_name(name), expected);
    }
}
