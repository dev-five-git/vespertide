use std::collections::{HashMap, HashSet};

use crate::orm::OrmExporter;
use vespertide_config::DEFAULT_GORM_PACKAGE_NAME;
use vespertide_core::schema::column::{
    ColumnType, ComplexColumnType, EnumValues, SimpleColumnKind, SimpleColumnType,
};
use vespertide_core::schema::constraint::TableConstraint;
use vespertide_core::schema::names::ColumnName;
use vespertide_core::{ColumnDef, DefaultValue, ReferenceAction, ReferenceActionKind, TableDef};
use vespertide_naming::{IdentifierStart, sanitize_identifier};

/// Track which Go imports are actually used to generate minimal import statements.
#[expect(
    clippy::struct_excessive_bools,
    reason = "four independent import-presence flags; enum would add verbosity without clarity"
)]
#[derive(Default)]
struct UsedImports {
    needs_time: bool,
    needs_uuid: bool,
    needs_datatypes: bool,
    needs_decimal: bool,
}

impl UsedImports {
    fn add_column_type(&mut self, col_type: &ColumnType) {
        match col_type {
            ColumnType::Simple(ty) => match ty {
                SimpleColumnType::Date
                | SimpleColumnType::Time
                | SimpleColumnType::Timestamp
                | SimpleColumnType::Timestamptz => {
                    self.needs_time = true;
                }
                SimpleColumnType::Uuid => {
                    self.needs_uuid = true;
                }
                SimpleColumnType::Json => {
                    self.needs_datatypes = true;
                }
                _ => {}
            },
            ColumnType::Complex(ty) => {
                if let ComplexColumnType::Numeric { .. } = ty {
                    self.needs_decimal = true;
                }
                if let ComplexColumnType::Custom { custom_type } = ty
                    && custom_type.to_uppercase() == "JSONB"
                {
                    self.needs_datatypes = true;
                }
            }
        }
    }
}

pub struct GormExporter;

impl OrmExporter for GormExporter {
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

/// GORM exporter that honors `vespertide.json`'s `gorm` config section
/// (currently the effective Go package name — see
/// `VespertideConfig::gorm_package_name`, which resolves an explicit
/// `gorm.package_name` or infers one from the actual export directory —
/// emitted at the top of every file). Mirrors `seaorm::SeaOrmExporterWithConfig`.
pub struct GormExporterWithConfig<'a> {
    package_name: &'a str,
}

impl<'a> GormExporterWithConfig<'a> {
    /// `package_name` is the already-resolved effective package name (see
    /// `VespertideConfig::gorm_package_name`), not the raw `GormConfig`
    /// field — resolving requires the actual export directory, which the
    /// `GormConfig` alone doesn't know.
    pub fn new(package_name: &'a str) -> Self {
        Self { package_name }
    }

    pub fn render_entity(&self, table: &TableDef) -> Result<String, String> {
        Ok(render_entity_inner_with_package(
            table,
            &[],
            self.package_name,
        ))
    }

    pub fn render_entity_with_schema(
        &self,
        table: &TableDef,
        schema: &[TableDef],
    ) -> Result<String, String> {
        Ok(render_entity_inner_with_package(
            table,
            schema,
            self.package_name,
        ))
    }
}

/// Render a GORM entity for the given table definition.
pub fn render_entity(table: &TableDef) -> Result<String, String> {
    Ok(render_entity_inner(table, &[]))
}

/// Render a GORM entity with full schema context for reverse-relation (HasMany) generation.
pub fn render_entity_with_schema(table: &TableDef, schema: &[TableDef]) -> Result<String, String> {
    Ok(render_entity_inner(table, schema))
}

#[cfg(test)]
pub(crate) fn to_pascal_case_for_tests(s: &str) -> String {
    to_pascal_case(s)
}

fn render_entity_inner(table: &TableDef, schema: &[TableDef]) -> String {
    render_entity_inner_with_package(table, schema, DEFAULT_GORM_PACKAGE_NAME)
}

fn render_entity_inner_with_package(
    table: &TableDef,
    schema: &[TableDef],
    package_name: &str,
) -> String {
    let mut lines: Vec<String> = Vec::new();

    let struct_name =
        sanitize_identifier(&to_pascal_case(&table.name), IdentifierStart::Underscore);

    // Find enum names that appear in multiple schema tables (need qualified Go type names)
    let conflicting_enums: HashSet<String> = {
        let mut counts: HashMap<String, usize> = HashMap::new();
        for col in &table.columns {
            if let ColumnType::Complex(ComplexColumnType::Enum { name, .. }) = &col.r#type {
                counts
                    .entry(sanitize_identifier(
                        &to_pascal_case(name),
                        IdentifierStart::Underscore,
                    ))
                    .or_insert(1);
            }
        }
        for other in schema {
            if other.name == table.name {
                continue;
            }
            let mut seen = HashSet::new();
            for col in &other.columns {
                if let ColumnType::Complex(ComplexColumnType::Enum { name, .. }) = &col.r#type {
                    let pascal =
                        sanitize_identifier(&to_pascal_case(name), IdentifierStart::Underscore);
                    if seen.insert(pascal.clone()) {
                        *counts.entry(pascal).or_default() += 1;
                    }
                }
            }
        }
        counts
            .into_iter()
            .filter(|(_, c)| *c > 1)
            .map(|(n, _)| n)
            .collect()
    };

    // Collect enums defined in this table's columns, with qualified names where needed
    let enums: Vec<(&str, &EnumValues, String)> = table
        .columns
        .iter()
        .filter_map(|col| {
            if let ColumnType::Complex(ComplexColumnType::Enum { name, values }) = &col.r#type {
                let pascal =
                    sanitize_identifier(&to_pascal_case(name), IdentifierStart::Underscore);
                let qualified = if conflicting_enums.contains(&pascal) {
                    format!("{struct_name}{pascal}")
                } else {
                    pascal
                };
                Some((name.as_str(), values, qualified))
            } else {
                None
            }
        })
        .collect();
    let enum_name_map: HashMap<&str, String> = enums
        .iter()
        .map(|(name, _, qualified)| (*name, qualified.clone()))
        .collect();

    let fk_by_column = collect_fk_info(&table.constraints);

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
        .map(|c| c.as_str().to_owned())
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

    let single_unique_columns: HashSet<String> = table
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

    let index_map = collect_index_info(&table.constraints);
    let composite_unique_map = collect_composite_unique_info(&table.constraints);

    let mut used_imports = UsedImports::default();
    for col in &table.columns {
        used_imports.add_column_type(&col.r#type);
    }

    let reverse_relations = find_reverse_relations(&table.name, schema);

    // --- Package declaration ---
    lines.push(format!("package {package_name}"));
    lines.push(String::new());

    // --- Imports ---
    let has_stdlib = used_imports.needs_time;
    let has_external =
        used_imports.needs_uuid || used_imports.needs_datatypes || used_imports.needs_decimal;

    if has_stdlib || has_external {
        lines.push("import (".into());
        if has_stdlib {
            lines.push("    \"time\"".into());
        }
        if has_stdlib && has_external {
            lines.push(String::new());
        }
        if used_imports.needs_datatypes {
            lines.push("    \"gorm.io/datatypes\"".into());
        }
        if used_imports.needs_uuid {
            lines.push("    \"github.com/google/uuid\"".into());
        }
        if used_imports.needs_decimal {
            lines.push("    \"github.com/shopspring/decimal\"".into());
        }
        lines.push(")".into());
        lines.push(String::new());
    }

    // --- Enum type declarations ---
    for (_, values, qualified_name) in &enums {
        render_enum(&mut lines, qualified_name, values);
        lines.push(String::new());
    }

    // --- Struct definition ---
    if let Some(ref desc) = table.description {
        lines.push(format!("// {}", desc.replace('\n', " ")));
    }

    lines.push(format!("type {struct_name} struct {{"));

    // Every real column's field name is reserved up front so belongs-to
    // relation fields (single-column and composite) can detect a collision
    // regardless of which column — FK or plain — happens to come first in
    // the table definition.
    let used_field_names: HashSet<String> = table
        .columns
        .iter()
        .map(|c| to_go_field_name(&c.name))
        .collect();
    let mut used_relation_names = used_field_names.clone();

    for col in &table.columns {
        let is_pk = pk_columns.contains(col.name.as_str());
        let is_unique = single_unique_columns.contains(col.name.as_str());
        let indexes = index_map
            .get(col.name.as_str())
            .map_or(&[][..], Vec::as_slice);
        let composite_unique_name = composite_unique_map.get(col.name.as_str());

        if let Some(ref comment) = col.comment {
            lines.push(format!("    // {}", comment.replace('\n', " ")));
        }

        render_column_field(
            &mut lines,
            col,
            is_pk,
            auto_increment && !is_composite_pk,
            is_unique,
            indexes,
            composite_unique_name,
            &enum_name_map,
        );

        if let Some(fk) = fk_by_column.get(col.name.as_str()) {
            render_fk_relation_field(&mut lines, col, fk, &mut used_relation_names);
        }
    }

    // Composite (multi-column) FK relation fields. GORM supports composite
    // associations via comma-separated `foreignKey`/`references` tags, unlike
    // Django which has no native equivalent.
    for fk in collect_composite_fk_info(&table.constraints) {
        render_composite_fk_relation_field(&mut lines, &fk, &mut used_relation_names);
    }

    // Reverse relation fields (HasMany) derived from schema context
    for rel in &reverse_relations {
        let mut constraint_parts: Vec<String> = Vec::new();
        if let Some(ref action) = rel.on_delete {
            constraint_parts.push(format!("OnDelete:{}", reference_action_str(action)));
        }
        if let Some(ref action) = rel.on_update {
            constraint_parts.push(format!("OnUpdate:{}", reference_action_str(action)));
        }
        let fk_field = to_go_field_name(&rel.fk_column);
        let gorm_tag = if constraint_parts.is_empty() {
            format!("foreignKey:{fk_field}")
        } else {
            format!(
                "foreignKey:{fk_field};constraint:{}",
                constraint_parts.join(",")
            )
        };
        lines.push(format!(
            "    {field_name} []{ref_struct} `gorm:\"{gorm_tag}\" json:\"-\"`",
            field_name = rel.field_name,
            ref_struct =
                sanitize_identifier(&to_pascal_case(&rel.ref_table), IdentifierStart::Underscore),
        ));
    }

    lines.push("}".into());
    lines.push(String::new());

    // --- TableName() method ---
    if needs_table_name_method(&table.name, &struct_name) {
        lines.push(format!(
            "func ({struct_name}) TableName() string {{ return \"{name}\" }}",
            name = table.name,
        ));
        lines.push(String::new());
    }

    lines.join("\n")
}

// ---------------------------------------------------------------------------
// FK info collection
// ---------------------------------------------------------------------------

struct FkInfo {
    ref_table: String,
    on_delete: Option<ReferenceAction>,
    on_update: Option<ReferenceAction>,
}

struct CompositeFkInfo {
    local_cols: Vec<String>,
    ref_table: String,
    ref_cols: Vec<String>,
    on_delete: Option<ReferenceAction>,
    on_update: Option<ReferenceAction>,
}

fn collect_composite_fk_info(constraints: &[TableConstraint]) -> Vec<CompositeFkInfo> {
    constraints
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
                && columns.len() > 1
                && columns.len() == ref_columns.len()
            {
                return Some(CompositeFkInfo {
                    local_cols: columns.iter().map(|c| c.as_str().to_owned()).collect(),
                    ref_table: ref_table.as_str().to_owned(),
                    ref_cols: ref_columns.iter().map(|c| c.as_str().to_owned()).collect(),
                    on_delete: on_delete.clone(),
                    on_update: on_update.clone(),
                });
            }
            None
        })
        .collect()
}

fn collect_fk_info(constraints: &[TableConstraint]) -> HashMap<String, FkInfo> {
    constraints
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
            {
                if columns.len() == 1 && ref_columns.len() == 1 {
                    Some((
                        columns[0].as_str().to_owned(),
                        FkInfo {
                            ref_table: ref_table.as_str().to_owned(),
                            on_delete: on_delete.clone(),
                            on_update: on_update.clone(),
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
// Index info collection
// ---------------------------------------------------------------------------

struct IndexInfo {
    name: Option<String>,
}

fn collect_index_info(constraints: &[TableConstraint]) -> HashMap<String, Vec<IndexInfo>> {
    let mut map: HashMap<String, Vec<IndexInfo>> = HashMap::new();
    for c in constraints {
        if let TableConstraint::Index { name, columns } = c {
            for col in columns {
                map.entry(col.as_str().to_owned())
                    .or_default()
                    .push(IndexInfo {
                        name: name.as_ref().map(|n| n.as_str().to_owned()),
                    });
            }
        }
    }
    map
}

fn collect_composite_unique_info(constraints: &[TableConstraint]) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for c in constraints {
        if let TableConstraint::Unique { name, columns, .. } = c
            && columns.len() > 1
        {
            let uq_name = name.as_ref().map_or_else(
                || {
                    let parts: Vec<&str> = columns.iter().map(ColumnName::as_str).collect();
                    format!("uq_{}", parts.join("_"))
                },
                |n| n.as_str().to_owned(),
            );
            for col in columns {
                map.insert(col.as_str().to_owned(), uq_name.clone());
            }
        }
    }
    map
}

// ---------------------------------------------------------------------------
// Reverse relation discovery
// ---------------------------------------------------------------------------

struct ReverseRelation {
    field_name: String,
    ref_table: String,
    fk_column: String,
    on_delete: Option<ReferenceAction>,
    on_update: Option<ReferenceAction>,
}

fn find_reverse_relations(table_name: &str, schema: &[TableDef]) -> Vec<ReverseRelation> {
    type RawRelation = (
        String,
        String,
        String,
        Option<ReferenceAction>,
        Option<ReferenceAction>,
    );
    let mut raw: Vec<RawRelation> = Vec::new();
    for other in schema {
        // Note: self-referencing tables (other.name == table_name) are NOT
        // skipped here — a table's own FK column pointing back at itself
        // (e.g. categories.parent_id -> categories.id) must still produce a
        // reverse has-many ("Children") relation on the same struct.
        for c in &other.constraints {
            if let TableConstraint::ForeignKey {
                columns,
                ref_table,
                on_delete,
                on_update,
                ..
            } = c
                && ref_table.as_str() == table_name
                && columns.len() == 1
            {
                let fk_col = columns[0].as_str().to_owned();
                let is_self_ref = other.name.as_str() == table_name;
                let base_name = if is_self_ref {
                    "Children".to_string()
                } else {
                    let pascal = sanitize_identifier(
                        &to_pascal_case(other.name.as_str()),
                        IdentifierStart::Underscore,
                    );
                    if pascal.ends_with('s') {
                        pascal
                    } else {
                        format!("{pascal}s")
                    }
                };
                raw.push((
                    other.name.as_str().to_owned(),
                    fk_col,
                    base_name,
                    on_delete.clone(),
                    on_update.clone(),
                ));
            }
        }
    }

    let mut name_count: HashMap<String, usize> = HashMap::new();
    for (_, _, base_name, _, _) in &raw {
        *name_count.entry(base_name.clone()).or_default() += 1;
    }

    raw.into_iter()
        .map(|(ref_table, fk_col, base_name, on_delete, on_update)| {
            let field_name = if *name_count.get(&base_name).unwrap_or(&0) > 1 {
                format!("{}By{}", base_name, to_go_field_name(&fk_col))
            } else {
                base_name
            };
            ReverseRelation {
                field_name,
                ref_table,
                fk_column: fk_col,
                on_delete,
                on_update,
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Enum rendering
// ---------------------------------------------------------------------------

fn render_enum(lines: &mut Vec<String>, name: &str, values: &EnumValues) {
    // `name` is already the sanitized, PascalCased (and possibly struct-qualified)
    // identifier built by the caller — re-running `to_pascal_case` here would
    // split on the `_` a leading-digit escape (e.g. `_1users`) introduces and
    // silently drop it.
    let type_name = name;

    let mut rendered = match values {
        EnumValues::String(_) => {
            vec![
                format!("type {type_name} string"),
                String::new(),
                "const (".into(),
            ]
        }
        EnumValues::Integer(_) => {
            vec![
                format!("type {type_name} int"),
                String::new(),
                "const (".into(),
            ]
        }
    };

    match values {
        EnumValues::String(vals) => {
            for val in vals {
                let const_name = format!("{type_name}{}", to_pascal_case(val));
                rendered.push(format!("    {const_name} {type_name} = \"{val}\""));
            }
        }
        EnumValues::Integer(vals) => {
            for val in vals {
                let const_name = format!("{type_name}{}", to_pascal_case(&val.name));
                rendered.push(format!("    {const_name} {type_name} = {}", val.value));
            }
        }
    }

    rendered.push(")".into());
    lines.extend(rendered);
}

// ---------------------------------------------------------------------------
// Field rendering
// ---------------------------------------------------------------------------

#[expect(
    clippy::too_many_arguments,
    reason = "all params are independent field-rendering inputs; a context struct would add noise without reducing coupling"
)]
fn render_column_field(
    lines: &mut Vec<String>,
    col: &ColumnDef,
    is_pk: bool,
    auto_increment: bool,
    is_unique: bool,
    indexes: &[IndexInfo],
    composite_unique_name: Option<&String>,
    enum_name_map: &HashMap<&str, String>,
) {
    let go_type = go_type_for_column_mapped(&col.r#type, col.nullable, enum_name_map);
    let field_name = to_go_field_name(&col.name);
    let gorm_tag = build_gorm_tag(
        col,
        is_pk,
        auto_increment,
        is_unique,
        indexes,
        composite_unique_name,
    );

    lines.push(format!(
        "    {field_name} {go_type} `gorm:\"{gorm_tag}\" json:\"{json_name}\"`",
        json_name = col.name,
    ));
}

fn render_fk_relation_field(
    lines: &mut Vec<String>,
    col: &ColumnDef,
    fk: &FkInfo,
    used_relation_names: &mut HashSet<String>,
) {
    let ref_struct =
        sanitize_identifier(&to_pascal_case(&fk.ref_table), IdentifierStart::Underscore);
    let fk_field_name = to_go_field_name(&col.name);
    let mut relation_field_name = infer_relation_field_name(&col.name);
    if relation_field_name == fk_field_name {
        relation_field_name = format!("{relation_field_name}{ref_struct}");
    }
    // The name above only rules out colliding with this FK's own scalar
    // field; it can still collide with an unrelated real column (or another
    // relation) elsewhere in the table, so fall back to a numbered suffix.
    if used_relation_names.contains(&relation_field_name) {
        let mut n = 2;
        loop {
            let candidate = format!("{relation_field_name}{n}");
            if !used_relation_names.contains(&candidate) {
                relation_field_name = candidate;
                break;
            }
            n += 1;
        }
    }
    used_relation_names.insert(relation_field_name.clone());

    let mut constraint_parts: Vec<String> = Vec::new();
    if let Some(ref action) = fk.on_delete {
        constraint_parts.push(format!("OnDelete:{}", reference_action_str(action)));
    }
    if let Some(ref action) = fk.on_update {
        constraint_parts.push(format!("OnUpdate:{}", reference_action_str(action)));
    }

    let gorm_tag = if constraint_parts.is_empty() {
        format!("foreignKey:{fk_field_name}")
    } else {
        format!(
            "foreignKey:{fk_field_name};constraint:{}",
            constraint_parts.join(",")
        )
    };

    let type_expr = if col.nullable {
        format!("*{ref_struct}")
    } else {
        ref_struct
    };

    lines.push(format!(
        "    {relation_field_name} {type_expr} `gorm:\"{gorm_tag}\" json:\"-\"`"
    ));
}

/// Render a belongs-to relation field for a composite (multi-column) FK,
/// using GORM's comma-separated `foreignKey`/`references` tag syntax.
fn render_composite_fk_relation_field(
    lines: &mut Vec<String>,
    fk: &CompositeFkInfo,
    used_relation_names: &mut HashSet<String>,
) {
    let ref_struct =
        sanitize_identifier(&to_pascal_case(&fk.ref_table), IdentifierStart::Underscore);

    let mut relation_field_name = ref_struct.clone();
    if used_relation_names.contains(&relation_field_name) {
        let mut n = 2;
        loop {
            let candidate = format!("{relation_field_name}{n}");
            if !used_relation_names.contains(&candidate) {
                relation_field_name = candidate;
                break;
            }
            n += 1;
        }
    }
    used_relation_names.insert(relation_field_name.clone());

    let fk_fields: Vec<String> = fk.local_cols.iter().map(|c| to_go_field_name(c)).collect();
    let ref_fields: Vec<String> = fk.ref_cols.iter().map(|c| to_go_field_name(c)).collect();

    let mut constraint_parts: Vec<String> = Vec::new();
    if let Some(ref action) = fk.on_delete {
        constraint_parts.push(format!("OnDelete:{}", reference_action_str(action)));
    }
    if let Some(ref action) = fk.on_update {
        constraint_parts.push(format!("OnUpdate:{}", reference_action_str(action)));
    }

    let gorm_tag = if constraint_parts.is_empty() {
        format!(
            "foreignKey:{};references:{}",
            fk_fields.join(","),
            ref_fields.join(",")
        )
    } else {
        format!(
            "foreignKey:{};references:{};constraint:{}",
            fk_fields.join(","),
            ref_fields.join(","),
            constraint_parts.join(",")
        )
    };

    lines.push(format!(
        "    {relation_field_name} {ref_struct} `gorm:\"{gorm_tag}\" json:\"-\"`"
    ));
}

// ---------------------------------------------------------------------------
// GORM tag building
// ---------------------------------------------------------------------------

fn build_gorm_tag(
    col: &ColumnDef,
    is_pk: bool,
    auto_increment: bool,
    is_unique: bool,
    indexes: &[IndexInfo],
    composite_unique_name: Option<&String>,
) -> String {
    let mut parts: Vec<String> = vec![format!("column:{}", col.name)];

    if is_pk {
        parts.push("primaryKey".into());
    }
    if is_pk && auto_increment {
        parts.push("autoIncrement".into());
    }
    if !col.nullable && !is_pk {
        parts.push("not null".into());
    }
    if is_unique && !is_pk {
        parts.push("unique".into());
    }

    match &col.r#type {
        ColumnType::Simple(SimpleColumnType::Text) => parts.push("type:text".into()),
        ColumnType::Simple(SimpleColumnType::Xml) => parts.push("type:xml".into()),
        ColumnType::Simple(SimpleColumnType::Interval) => parts.push("type:interval".into()),
        ColumnType::Simple(SimpleColumnType::Date) => parts.push("type:date".into()),
        ColumnType::Simple(SimpleColumnType::Time) => parts.push("type:time".into()),
        ColumnType::Simple(SimpleColumnType::Uuid) => parts.push("type:uuid".into()),
        ColumnType::Complex(ComplexColumnType::Varchar { length }) => {
            parts.push(format!("size:{length}"));
        }
        ColumnType::Complex(ComplexColumnType::Char { length }) => {
            parts.push(format!("size:{length}"));
            parts.push("type:char".into());
        }
        ColumnType::Complex(ComplexColumnType::Numeric { precision, scale }) => {
            parts.push(format!("type:numeric({precision},{scale})"));
        }
        ColumnType::Complex(ComplexColumnType::Custom { custom_type }) => {
            parts.push(format!("type:{custom_type}"));
        }
        _ => {}
    }

    if let Some(ref default) = col.default
        && let Some(tag) = build_default_tag(default)
    {
        parts.push(tag);
    }

    for idx in indexes {
        if let Some(ref name) = idx.name {
            parts.push(format!("index:{name}"));
        } else {
            parts.push("index".into());
        }
    }

    if let Some(uq_name) = composite_unique_name {
        parts.push(format!("uniqueIndex:{uq_name}"));
    }

    parts.join(";")
}

fn build_default_tag(default: &DefaultValue) -> Option<String> {
    let sql = default.to_sql();
    if sql.contains('(') {
        return None; // Skip server-side function calls like NOW()
    }
    Some(format!("default:{sql}"))
}

// ---------------------------------------------------------------------------
// Type mapping
// ---------------------------------------------------------------------------

pub(super) fn go_type_for_column_mapped(
    col_type: &ColumnType,
    nullable: bool,
    enum_map: &HashMap<&str, String>,
) -> String {
    let base = match col_type {
        ColumnType::Complex(ComplexColumnType::Enum { name, .. }) => {
            enum_map.get(name.as_str()).cloned().unwrap_or_else(|| {
                sanitize_identifier(&to_pascal_case(name), IdentifierStart::Underscore)
            })
        }
        _ => go_base_type(col_type),
    };
    if nullable { format!("*{base}") } else { base }
}

fn go_base_type(col_type: &ColumnType) -> String {
    match col_type {
        ColumnType::Simple(ty) => match SimpleColumnKind::from(*ty) {
            SimpleColumnKind::SmallInt => "int16".to_string(),
            SimpleColumnKind::Integer => "int32".to_string(),
            SimpleColumnKind::BigInt => "int64".to_string(),
            SimpleColumnKind::Real => "float32".to_string(),
            SimpleColumnKind::DoublePrecision => "float64".to_string(),
            SimpleColumnKind::Text
            | SimpleColumnKind::Xml
            | SimpleColumnKind::Inet
            | SimpleColumnKind::Cidr
            | SimpleColumnKind::Macaddr
            | SimpleColumnKind::Interval => "string".to_string(),
            SimpleColumnKind::Boolean => "bool".to_string(),
            SimpleColumnKind::Date
            | SimpleColumnKind::Time
            | SimpleColumnKind::Timestamp
            | SimpleColumnKind::Timestamptz => "time.Time".to_string(),
            SimpleColumnKind::Bytea => "[]byte".to_string(),
            SimpleColumnKind::Uuid => "uuid.UUID".to_string(),
            SimpleColumnKind::Json => "datatypes.JSON".to_string(),
        },
        ColumnType::Complex(ty) => match ty {
            ComplexColumnType::Varchar { .. } | ComplexColumnType::Char { .. } => {
                "string".to_string()
            }
            ComplexColumnType::Custom { custom_type } => {
                if custom_type.to_uppercase() == "JSONB" {
                    "datatypes.JSON".to_string()
                } else {
                    "string".to_string()
                }
            }
            ComplexColumnType::Numeric { .. } => "decimal.Decimal".to_string(),
            // `#[non_exhaustive]` future-variant guard; unreachable today.
            #[cfg(not(tarpaulin_include))]
            _ => {
                unreachable!("ComplexColumnType is #[non_exhaustive]; all variants matched")
            }
        },
    }
}

fn reference_action_str(action: &ReferenceAction) -> &'static str {
    match ReferenceActionKind::from(action) {
        ReferenceActionKind::Cascade => "CASCADE",
        ReferenceActionKind::Restrict => "RESTRICT",
        ReferenceActionKind::SetNull => "SET NULL",
        ReferenceActionKind::SetDefault => "SET DEFAULT",
        ReferenceActionKind::NoAction => "NO ACTION",
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

pub(super) fn to_go_field_name(s: &str) -> String {
    let pascal = to_pascal_case(s);
    // Apply Go conventions for common abbreviations
    let pascal = pascal.replace("Id", "ID");
    // Go identifiers can't start with a digit or contain non-alphanumeric
    // characters; a leading `_` is legal (matches Rust module / Java field
    // escaping elsewhere in the exporter).
    sanitize_identifier(&pascal, IdentifierStart::Underscore)
}

pub(super) fn infer_relation_field_name(fk_column: &str) -> String {
    let base = fk_column.strip_suffix("_id").unwrap_or(fk_column);
    sanitize_identifier(&to_pascal_case(base), IdentifierStart::Underscore)
}

fn pascal_to_snake(s: &str) -> String {
    let mut result = String::new();
    for c in s.chars() {
        if c.is_uppercase() && !result.is_empty() {
            result.push('_');
        }
        result.extend(c.to_lowercase());
    }
    result
}

pub(super) fn needs_table_name_method(table_name: &str, struct_name: &str) -> bool {
    let snake = pascal_to_snake(struct_name);
    let gorm_default = format!("{snake}s");
    gorm_default != table_name
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests;
