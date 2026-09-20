use std::collections::{HashMap, HashSet};

use super::enums::{const_name, render_enum};
use super::types::{UsedImports, go_type_for_column_mapped, is_go_string};
use crate::constraint_scan::{
    BackRelation, FkDetails, collect_back_relations, primary_key_columns, single_column_fk_details,
    single_column_uniques,
};
use crate::enum_scan::{collect_table_enums, variant_names};
use crate::scope_names::ScopeNames;
use crate::utils::common::{
    CompositeFk, claim_binding, collect_composite_fks, integer_enum_variant_value, string_literal,
    unquote,
};
use vespertide_core::schema::column::{
    ColumnType, ComplexColumnType, EnumValues, SimpleColumnType,
};
use vespertide_core::schema::constraint::TableConstraint;
use vespertide_core::{ColumnDef, DefaultValue, ReferenceAction, TableDef};
use vespertide_naming::{
    IdentifierStart, build_index_name, build_unique_constraint_name, pluralize, sanitize_identifier,
};

/// The Go imports the columns of `tables` need.
pub(super) fn imports_for(tables: &[TableDef]) -> UsedImports {
    let mut used = UsedImports::default();
    for col in tables.iter().flat_map(|table| &table.columns) {
        used.add_column_type(&col.r#type);
    }
    used
}

/// The `package` clause and the import block, stdlib first.
pub(super) fn render_header(package_name: &str, used_imports: &UsedImports) -> Vec<String> {
    let mut lines = vec![format!("package {package_name}"), String::new()];

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
        if used_imports.needs_uuid {
            lines.push("    \"github.com/google/uuid\"".into());
        }
        if used_imports.needs_decimal {
            lines.push("    \"github.com/shopspring/decimal\"".into());
        }
        if used_imports.needs_datatypes {
            lines.push("    \"gorm.io/datatypes\"".into());
        }
        lines.push(")".into());
        lines.push(String::new());
    }
    lines
}

/// Lay rendered `lines` out the way `gofmt` does, so the file passes a
/// project's `gofmt -l` check untouched: tab indents, the name / type / rest
/// cells of consecutive struct fields or constants padded into columns, and
/// no doubled blank lines. A comment or an import path has no cells and ends
/// the run of lines being aligned, as it does in `gofmt`.
pub(super) fn gofmt_layout(lines: &[String]) -> String {
    fn flush(run: &mut Vec<[&str; 3]>, out: &mut Vec<String>) {
        let name_width = run.iter().map(|cells| cells[0].len()).max().unwrap_or(0);
        let type_width = run.iter().map(|cells| cells[1].len()).max().unwrap_or(0);
        for [name, ty, rest] in run.drain(..) {
            out.push(format!("\t{name:<name_width$} {ty:<type_width$} {rest}"));
        }
    }

    let mut out: Vec<String> = Vec::new();
    let mut run: Vec<[&str; 3]> = Vec::new();
    for line in lines {
        let cells = line
            .strip_prefix("    ")
            .filter(|body| !body.starts_with("//"))
            .and_then(|body| {
                let (name, rest) = body.split_once(' ')?;
                let (ty, rest) = rest.split_once(' ')?;
                Some([name, ty, rest])
            });
        if let Some(cells) = cells {
            run.push(cells);
            continue;
        }
        flush(&mut run, &mut out);
        if let Some(body) = line.strip_prefix("    ") {
            out.push(format!("\t{body}"));
        } else if !(line.is_empty() && out.last().is_some_and(String::is_empty)) {
            out.push(line.clone());
        }
    }
    out.join("\n")
}

/// Every name `tables` declare in their package: structs, enum types, then
/// enum constants, which Go also puts at package scope (`Status` + `code` is
/// `StatusCode`, and so is the struct of a `status_code` table).
pub(super) fn package_names(tables: &[TableDef]) -> ScopeNames {
    let mut names = ScopeNames::collect(tables, exported_go_name, exported_go_name);
    for table in tables {
        for (enum_name, values) in collect_table_enums(table) {
            let type_name = names.enum_type(&table.name, enum_name).to_string();
            for (index, variant) in variant_names(values).into_iter().enumerate() {
                names.claim_member(
                    &table.name,
                    enum_name,
                    index,
                    const_name(&type_name, variant),
                );
            }
        }
    }
    names
}

/// The struct a table is declared as; a table outside the package's schema —
/// a foreign key may point there — keeps its natural name.
fn struct_name_of(names: &ScopeNames, table: &str) -> String {
    names
        .table(table)
        .map_or_else(|| exported_go_name(table), str::to_string)
}

/// Everything below the header for one table: enum types, the struct, and
/// its methods.
pub(super) fn render_table_body(
    table: &TableDef,
    schema: &[TableDef],
    names: &ScopeNames,
) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();

    let struct_name = struct_name_of(names, &table.name);

    let enums = collect_table_enums(table);
    let enum_name_map: HashMap<&str, String> = enums
        .iter()
        .map(|(name, _)| (*name, names.enum_type(&table.name, name).to_string()))
        .collect();

    let fk_by_column = single_column_fk_details(&table.constraints);
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

    let single_unique_columns = single_column_uniques(&table.constraints);

    let index_map = collect_index_names(table);
    let composite_unique_map = collect_composite_unique_names(table);

    // --- Enum type declarations ---
    for (enum_name, values) in &enums {
        let const_names: Vec<String> = (0..variant_names(values).len())
            .map(|index| names.member(&table.name, enum_name, index).to_string())
            .collect();
        render_enum(&mut lines, &enum_name_map[enum_name], &const_names, values);
        lines.push(String::new());
    }

    // --- Struct definition ---
    if let Some(ref desc) = table.description {
        lines.push(format!("// {}", desc.replace('\n', " ")));
    }

    lines.push(format!("type {struct_name} struct {{"));

    // One set of taken names for the whole struct, columns first: no relation
    // field — belongs-to, composite or has-many — may take a column's name,
    // whichever order the table declares them in.
    let (field_names, mut taken) = column_field_names(table);

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
            &field_names[col.name.as_str()],
            is_pk,
            auto_increment && !is_composite_pk,
            is_unique,
            indexes,
            composite_unique_name,
            &enum_name_map,
        );

        if let Some(fk) = fk_by_column.get(col.name.as_str()) {
            render_fk_relation_field(
                &mut lines,
                col,
                &field_names[col.name.as_str()],
                fk,
                schema,
                names,
                &mut taken,
            );
        }
    }

    // Composite (multi-column) FK relation fields, through GORM's
    // comma-separated `foreignKey`/`references` tags.
    for fk in collect_composite_fks(table) {
        render_composite_fk_relation_field(
            &mut lines,
            &fk,
            &field_names,
            schema,
            names,
            &mut taken,
        );
    }

    // Reverse relation fields (has-one / has-many) derived from schema context
    let back_relations = collect_back_relations(&table.name, schema);
    let reverse_names = reverse_field_names(&table.name, &back_relations);
    for (rel, field_name) in back_relations.iter().zip(reverse_names) {
        let foreign_key: Vec<String> = rel
            .fk_columns
            .iter()
            .map(|c| field_name_in(schema, &rel.source_table, c))
            .collect();
        let ref_columns: Vec<&str> = rel.ref_columns.iter().map(String::as_str).collect();
        let gorm_tag = relation_tag(
            &foreign_key,
            &reference_fields(schema, &table.name, &ref_columns),
            rel.on_delete.as_ref(),
            rel.on_update.as_ref(),
        );
        let source_struct = struct_name_of(names, &rel.source_table);
        let go_type = if rel.is_one_to_one {
            format!("*{source_struct}")
        } else {
            format!("[]{source_struct}")
        };
        lines.push(format!(
            "    {field_name} {go_type} {tag}",
            field_name = claim_binding(field_name, &mut taken),
            tag = struct_tag(&gorm_tag, "-"),
        ));
    }

    lines.push("}".into());
    lines.push(String::new());

    // GORM would otherwise derive the table name by pluralizing the struct
    // name, which does not reproduce an arbitrary database name.
    lines.push(format!(
        "func ({struct_name}) TableName() string {{ return {name} }}",
        name = string_literal(&table.name),
    ));
    lines.push(String::new());

    lines
}

// ---------------------------------------------------------------------------
// Index / unique names
// ---------------------------------------------------------------------------

/// Index names per column, spelled as the SQL layer spells them so
/// `AutoMigrate` finds the index the migration created instead of adding a
/// second one. Every column of a composite index carries the same name, which
/// is how GORM groups them.
fn collect_index_names(table: &TableDef) -> HashMap<&str, Vec<String>> {
    let mut map: HashMap<&str, Vec<String>> = HashMap::new();
    for c in &table.constraints {
        if let TableConstraint::Index { name, columns } = c {
            let index_name = build_index_name(&table.name, columns, name.as_deref());
            for col in columns {
                map.entry(col.as_str())
                    .or_default()
                    .push(index_name.clone());
            }
        }
    }
    map
}

/// Composite unique-index name per column, spelled as the SQL layer spells it.
fn collect_composite_unique_names(table: &TableDef) -> HashMap<&str, String> {
    let mut map = HashMap::new();
    for c in &table.constraints {
        if let TableConstraint::Unique { name, columns, .. } = c
            && columns.len() > 1
        {
            let uq_name = build_unique_constraint_name(&table.name, columns, name.as_deref());
            for col in columns {
                map.insert(col.as_str(), uq_name.clone());
            }
        }
    }
    map
}

// ---------------------------------------------------------------------------
// Reverse relation naming
// ---------------------------------------------------------------------------

/// Go field names for `rels`, in order: `Children` for a self-reference,
/// otherwise the source struct — as is for a has-one, pluralized for a
/// has-many. A name more than one relation would take is told apart by the
/// key it hangs on (`SettingsByCreatedByUserID`).
fn reverse_field_names(target: &str, rels: &[BackRelation]) -> Vec<String> {
    let bases: Vec<String> = rels
        .iter()
        .map(|rel| {
            if rel.source_table == target {
                "Children".to_string()
            } else if rel.is_one_to_one {
                exported_go_name(&rel.source_table)
            } else {
                exported_go_name(&pluralize(&rel.source_table))
            }
        })
        .collect();

    rels.iter()
        .zip(&bases)
        .map(|(rel, base)| {
            if bases.iter().filter(|other| *other == base).count() > 1 {
                let key: String = rel.fk_columns.iter().map(|c| to_go_field_name(c)).collect();
                format!("{base}By{key}")
            } else {
                base.clone()
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Field rendering
// ---------------------------------------------------------------------------

#[expect(
    clippy::too_many_arguments,
    reason = "independent field-rendering inputs, read once at a single call site"
)]
fn render_column_field(
    lines: &mut Vec<String>,
    col: &ColumnDef,
    field_name: &str,
    is_pk: bool,
    auto_increment: bool,
    is_unique: bool,
    indexes: &[String],
    composite_unique_name: Option<&String>,
    enum_name_map: &HashMap<&str, String>,
) {
    let go_type = go_type_for_column_mapped(&col.r#type, col.nullable, enum_name_map);
    let gorm_tag = build_gorm_tag(
        col,
        is_pk,
        auto_increment,
        is_unique,
        indexes,
        composite_unique_name,
    );

    lines.push(format!(
        "    {field_name} {go_type} {tag}",
        tag = struct_tag(&gorm_tag, &col.name),
    ));
}

fn render_fk_relation_field(
    lines: &mut Vec<String>,
    col: &ColumnDef,
    fk_field_name: &str,
    fk: &FkDetails,
    schema: &[TableDef],
    names: &ScopeNames,
    taken: &mut HashSet<String>,
) {
    let ref_struct = struct_name_of(names, fk.ref_table);
    let mut relation_field_name = go_relation_field_name(&col.name);
    if relation_field_name == fk_field_name {
        relation_field_name = format!("{relation_field_name}{ref_struct}");
    }
    // The name above only rules out colliding with this FK's own scalar
    // field; it can still collide with an unrelated real column (or another
    // relation) elsewhere in the table.
    let relation_field_name = claim_binding(relation_field_name, taken);

    let gorm_tag = relation_tag(
        &[fk_field_name.to_string()],
        &reference_fields(schema, fk.ref_table, &[fk.ref_column]),
        fk.on_delete,
        fk.on_update,
    );

    // Always a pointer, nullable or not: a struct that held its target by
    // value could not hold itself (`parent_id NOT NULL`), nor a target that
    // holds it back, and Go rejects both as an invalid recursive type.
    lines.push(format!(
        "    {relation_field_name} *{ref_struct} {tag}",
        tag = struct_tag(&gorm_tag, "-"),
    ));
}

/// Render a belongs-to relation field for a composite (multi-column) FK,
/// using GORM's comma-separated `foreignKey`/`references` tag syntax.
fn render_composite_fk_relation_field(
    lines: &mut Vec<String>,
    fk: &CompositeFk,
    field_names: &HashMap<&str, String>,
    schema: &[TableDef],
    names: &ScopeNames,
    taken: &mut HashSet<String>,
) {
    let ref_struct = struct_name_of(names, fk.ref_table);

    let relation_field_name = claim_binding(ref_struct.clone(), taken);

    let fk_fields: Vec<String> = fk
        .local_cols
        .iter()
        .map(|c| {
            field_names
                .get(*c)
                .map_or_else(|| to_go_field_name(c), Clone::clone)
        })
        .collect();
    let gorm_tag = relation_tag(
        &fk_fields,
        &reference_fields(schema, fk.ref_table, &fk.ref_cols),
        fk.on_delete,
        fk.on_update,
    );

    lines.push(format!(
        "    {relation_field_name} *{ref_struct} {tag}",
        tag = struct_tag(&gorm_tag, "-"),
    ));
}

// ---------------------------------------------------------------------------
// GORM tag building
// ---------------------------------------------------------------------------

/// A field's struct tag. `reflect.StructTag` reads each value as a quoted Go
/// string, so a `\` or `"` from a column name or default is escaped there; the
/// tag as a whole is a raw string unless a value holds a backtick, the one
/// character a raw string cannot.
fn struct_tag(gorm: &str, json: &str) -> String {
    let tag = format!(
        "gorm:{} json:{}",
        string_literal(gorm),
        string_literal(json)
    );
    if tag.contains('`') {
        string_literal(&tag)
    } else {
        format!("`{tag}`")
    }
}

/// The `references` fields of a relation on `ref_table`: none when the key is
/// the target's primary key, which GORM assumes. A composite key is always
/// spelled out, since GORM pairs its fields by position.
fn reference_fields(schema: &[TableDef], ref_table: &str, ref_columns: &[&str]) -> Vec<String> {
    if let [column] = ref_columns {
        let is_primary_key = schema
            .iter()
            .find(|t| t.name.as_str() == ref_table)
            .is_none_or(|target| {
                let pk = primary_key_columns(&target.constraints);
                pk.len() == 1 && pk.contains(column)
            });
        if is_primary_key {
            return Vec::new();
        }
    }
    ref_columns
        .iter()
        .map(|c| field_name_in(schema, ref_table, c))
        .collect()
}

/// The `gorm:"..."` tag of a relation field: the fields on the foreign-key
/// side, the fields they reference when GORM could not infer them, and the
/// referential actions.
fn relation_tag(
    foreign_key: &[String],
    references: &[String],
    on_delete: Option<&ReferenceAction>,
    on_update: Option<&ReferenceAction>,
) -> String {
    let mut parts = vec![format!("foreignKey:{}", foreign_key.join(","))];
    if !references.is_empty() {
        parts.push(format!("references:{}", references.join(",")));
    }
    let actions: Vec<String> = [("OnDelete", on_delete), ("OnUpdate", on_update)]
        .into_iter()
        .filter_map(|(key, action)| Some(format!("{key}:{}", action?.to_sql_keyword())))
        .collect();
    if !actions.is_empty() {
        parts.push(format!("constraint:{}", actions.join(",")));
    }
    parts.join(";")
}

fn build_gorm_tag(
    col: &ColumnDef,
    is_pk: bool,
    auto_increment: bool,
    is_unique: bool,
    indexes: &[String],
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
        ColumnType::Simple(SimpleColumnType::Inet) => parts.push("type:inet".into()),
        ColumnType::Simple(SimpleColumnType::Cidr) => parts.push("type:cidr".into()),
        ColumnType::Simple(SimpleColumnType::Macaddr) => parts.push("type:macaddr".into()),
        ColumnType::Complex(ComplexColumnType::Varchar { length }) => {
            parts.push(format!("size:{length}"));
        }
        // GORM only applies `size` to its built-in string type; a bare `type:char`
        // is `char(1)` on every database.
        ColumnType::Complex(ComplexColumnType::Char { length }) => {
            parts.push(format!("type:char({length})"));
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
        && let Some(tag) = build_default_tag(default, &col.r#type)
    {
        parts.push(tag);
    }

    for name in indexes {
        parts.push(format!("index:{name}"));
    }

    if let Some(uq_name) = composite_unique_name {
        parts.push(format!("uniqueIndex:{uq_name}"));
    }

    parts.join(";")
}

fn build_default_tag(default: &DefaultValue, col_type: &ColumnType) -> Option<String> {
    let sql = default.to_sql();
    // A function call has no literal to pin, and `;` would end the gorm
    // setting early, taking every later setting with it.
    if sql.contains(['(', ';']) {
        return None;
    }
    // An integer enum's default may name a variant; the column stores its value.
    if let ColumnType::Complex(ComplexColumnType::Enum {
        values: EnumValues::Integer(variants),
        ..
    }) = col_type
        && let Some(value) = integer_enum_variant_value(variants, unquote(&sql))
    {
        return Some(format!("default:{value}"));
    }
    // GORM takes a string field's default for the value itself, so the doubled
    // SQL escape must not reach it. It also trims every quote off both ends
    // rather than one pair, so a value that starts or ends with a quote has no
    // spelling. Every other kind of field keeps the tag as the SQL it was
    // written in.
    if is_go_string(col_type) && sql.len() >= 2 && sql.starts_with('\'') && sql.ends_with('\'') {
        let value = unquote(&sql).replace("''", "'");
        if value.starts_with(['\'', '"']) || value.ends_with(['\'', '"']) {
            return None;
        }
        return Some(format!("default:'{value}'"));
    }
    Some(format!("default:{sql}"))
}

// ---------------------------------------------------------------------------
// Naming utilities
// ---------------------------------------------------------------------------

pub(super) use crate::python_naming::to_pascal_case;

/// Exported Go identifier for a database name: PascalCase, with a digit-led
/// start given an upper-case letter prefix. GORM skips unexported struct
/// fields, and a `_`-led type is unreachable from other packages.
pub(super) fn exported_go_name(s: &str) -> String {
    exported(&to_pascal_case(s))
}

/// Go field name for a column: [`exported_go_name`] with Go's `ID` initialism.
fn to_go_field_name(s: &str) -> String {
    exported(&go_initialisms(&to_pascal_case(s)))
}

/// Go field name for every column of `table`, claimed in declaration order so
/// two columns that map to one Go name (`user_id`, `userId`) get distinct
/// fields, with the set those claims filled — the struct's relation fields
/// claim against the same one.
fn column_field_names(table: &TableDef) -> (HashMap<&str, String>, HashSet<String>) {
    // Every struct gets a `TableName` method, and Go rejects a field of the
    // same name.
    let mut taken = HashSet::from(["TableName".to_string()]);
    let names = table
        .columns
        .iter()
        .map(|col| {
            (
                col.name.as_str(),
                claim_binding(to_go_field_name(&col.name), &mut taken),
            )
        })
        .collect();
    (names, taken)
}

/// The field name `column` has in `table_name`'s struct: its claimed name when
/// that table is part of `schema`, the plain derivation otherwise (a
/// single-table render knows nothing about its FK targets).
fn field_name_in(schema: &[TableDef], table_name: &str, column: &str) -> String {
    schema
        .iter()
        .find(|t| t.name.as_str() == table_name)
        .and_then(|t| column_field_names(t).0.remove(column))
        .unwrap_or_else(|| to_go_field_name(column))
}

/// Go field name for a belongs-to relation: the FK column without its `_id`
/// suffix, in PascalCase.
fn go_relation_field_name(fk_column: &str) -> String {
    exported_go_name(vespertide_naming::infer_relation_field_name(fk_column))
}

fn exported(pascal: &str) -> String {
    let mut name = sanitize_identifier(pascal, IdentifierStart::Letter);
    // `Letter` copies the case of the first letter it finds (`x1users`), and Go
    // exports by case. The first byte is always an ASCII letter after
    // sanitizing, so the slice cannot split a character.
    name[..1].make_ascii_uppercase();
    name
}

/// Every `Id` that ends a PascalCase word becomes `ID`, as Go spells the
/// initialism; `Identity` and `Idx` keep their words.
fn go_initialisms(pascal: &str) -> String {
    let chars: Vec<char> = pascal.chars().collect();
    let mut out = String::with_capacity(pascal.len());
    let mut i = 0;
    while i < chars.len() {
        let ends_word = chars.get(i + 2).is_none_or(|c| !c.is_ascii_lowercase());
        if chars[i] == 'I' && chars.get(i + 1) == Some(&'d') && ends_word {
            out.push_str("ID");
            i += 2;
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use vespertide_core::schema::column::{
        ColumnType, ComplexColumnType, EnumValues, SimpleColumnType,
    };
    use vespertide_core::{ColumnDef, TableDef};

    use super::{
        build_default_tag, column_field_names, go_relation_field_name, package_names, struct_tag,
        to_go_field_name,
    };

    #[rstest]
    #[case("user_id", "UserID")]
    #[case("id", "ID")]
    #[case("created_at", "CreatedAt")]
    #[case("profile_image", "ProfileImage")]
    #[case("media_id", "MediaID")]
    #[case("identity", "Identity")]
    #[case("idx", "Idx")]
    #[case("1st_place", "X1stPlace")]
    fn column_names_become_exported_go_fields(#[case] input: &str, #[case] expected: &str) {
        assert_eq!(to_go_field_name(input), expected);
    }

    /// Two columns that map to one Go name get distinct fields, in declaration
    /// order, and none takes the name of the struct's own `TableName` method.
    #[test]
    fn column_field_names_disambiguate_go_collisions() {
        let integer = || ColumnType::Simple(SimpleColumnType::Integer);
        let table = TableDef {
            name: "sessions".into(),
            description: None,
            columns: vec![
                ColumnDef::new("user_id", integer(), false),
                ColumnDef::new("userId", integer(), false),
                ColumnDef::new("table_name", integer(), false),
            ],
            constraints: vec![],
        };
        let (names, _) = column_field_names(&table);
        assert_eq!(names["user_id"], "UserID");
        assert_eq!(names["userId"], "UserID2");
        assert_eq!(names["table_name"], "TableName2");
    }

    /// Constants sit at package scope next to the types: values that fold onto
    /// one name are numbered, and an empty value does not spell its own type.
    #[test]
    fn enum_constants_are_claimed_in_the_package_scope() {
        let state = ColumnType::Complex(ComplexColumnType::Enum {
            name: "state".into(),
            values: EnumValues::String(vec![
                "in progress".into(),
                "in-progress".into(),
                String::new(),
            ]),
        });
        let table = TableDef {
            name: "ticket".into(),
            description: None,
            columns: vec![ColumnDef::new("state", state, false)],
            constraints: vec![],
        };
        let names = package_names(std::slice::from_ref(&table));
        assert_eq!(names.member("ticket", "state", 0), "StateIn_progress");
        assert_eq!(names.member("ticket", "state", 1), "StateIn_progress2");
        assert_eq!(names.member("ticket", "state", 2), "State2");
    }

    #[rstest]
    #[case::plain(
        "column:id;primaryKey",
        "id",
        r#"`gorm:"column:id;primaryKey" json:"id"`"#
    )]
    #[case::quote_and_backslash(
        r#"column:a"b\c"#,
        r#"a"b\c"#,
        r#"`gorm:"column:a\"b\\c" json:"a\"b\\c"`"#
    )]
    #[case::backtick("column:a`b", "a`b", r#""gorm:\"column:a`b\" json:\"a`b\"""#)]
    fn struct_tags_escape_what_their_literal_cannot_hold(
        #[case] gorm: &str,
        #[case] json: &str,
        #[case] expected: &str,
    ) {
        assert_eq!(struct_tag(gorm, json), expected);
    }

    /// GORM reads a string field's default as the value and every other
    /// field's as SQL, so only the former loses the doubled quote — and, as
    /// GORM trims every quote off its ends, a value that ends in one.
    #[rstest]
    #[case::string(SimpleColumnType::Text, "'draft'", Some("default:'draft'"))]
    #[case::doubled_quote_in_a_string(SimpleColumnType::Text, "'it''s'", Some("default:'it's'"))]
    #[case::quotes_inside_a_string(
        SimpleColumnType::Text,
        r#"'a "b" c'"#,
        Some(r#"default:'a "b" c'"#)
    )]
    #[case::string_ending_in_a_quote(SimpleColumnType::Text, r#"'say "hi"'"#, None)]
    #[case::string_starting_with_a_quote(SimpleColumnType::Text, "'''tis'", None)]
    #[case::doubled_quote_in_json(
        SimpleColumnType::Json,
        r#"'{"a": "it''s"}'"#,
        Some(r#"default:'{"a": "it''s"}'"#)
    )]
    #[case::number(SimpleColumnType::Integer, "0", Some("default:0"))]
    #[case::function_call(SimpleColumnType::Timestamp, "now()", None)]
    #[case::setting_separator(SimpleColumnType::Text, "'a;b'", None)]
    fn defaults_become_gorm_default_tags(
        #[case] ty: SimpleColumnType,
        #[case] sql: &str,
        #[case] expected: Option<&str>,
    ) {
        let tag = build_default_tag(&sql.into(), &ColumnType::Simple(ty));
        assert_eq!(tag.as_deref(), expected);
    }

    #[rstest]
    #[case("user_id", "User")]
    #[case("author_id", "Author")]
    #[case("parent_id", "Parent")]
    #[case("node", "Node")]
    fn fk_columns_name_their_relation_field(#[case] input: &str, #[case] expected: &str) {
        assert_eq!(go_relation_field_name(input), expected);
    }
}
