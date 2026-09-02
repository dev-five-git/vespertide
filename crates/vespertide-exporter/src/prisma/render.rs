use std::collections::{HashMap, HashSet};

use vespertide_core::TableDef;
use vespertide_core::schema::column::{ColumnType, ComplexColumnType, EnumValues};
use vespertide_core::schema::constraint::TableConstraint;
use vespertide_core::schema::names::ColumnName;
use vespertide_core::schema::reference::ReferenceAction;
use vespertide_naming::{
    IdentifierStart, build_index_name, build_unique_constraint_name, infer_relation_field_name,
    sanitize_identifier, to_pascal_case,
};

use super::enums::enum_variant;
use super::types::column_type_to_prisma;
use crate::constraint_scan::{BackRelation, collect_back_relations, fk_relation_names};
use crate::utils::common::{claim_field_name, unquote};

pub(super) fn back_rel_field(br: &BackRelation) -> (String, String) {
    let source_pascal = prisma_model_name(&br.source_table);
    let rel_type = if br.is_one_to_one {
        format!("{source_pascal}?")
    } else {
        format!("{source_pascal}[]")
    };

    // Derived from a column and a table name, so it needs the same escaping a
    // declared field gets.
    let field_name = prisma_field_name(&if br.relation_name.is_some() {
        format!("{}_{}", br.rel_segment, br.source_table)
    } else {
        br.source_table.clone()
    });

    (field_name, rel_type)
}

pub(super) fn render_model(
    table: &TableDef,
    schema: &[TableDef],
    ambiguous: &HashSet<String>,
) -> String {
    let mut lines: Vec<String> = Vec::new();

    if let Some(desc) = &table.description {
        for line in desc.lines() {
            lines.push(format!("/// {line}"));
        }
    }

    let model_name = prisma_model_name(&table.name);
    lines.push(format!("model {model_name} {{"));

    let pk = crate::constraint_scan::primary_key(&table.constraints);
    let pk_cols = pk.map(TableConstraint::columns).unwrap_or_default();
    let pk_auto_increment = matches!(
        pk,
        Some(TableConstraint::PrimaryKey {
            auto_increment: true,
            ..
        })
    );
    let pk_columns: HashSet<&str> = pk_cols.iter().map(ColumnName::as_str).collect();
    let is_composite_pk = pk_cols.len() > 1;

    let unique_single: HashMap<&str, Option<&str>> = table
        .constraints
        .iter()
        .filter_map(|c| {
            if let TableConstraint::Unique { name, columns, .. } = c {
                if columns.len() == 1 {
                    Some((columns[0].as_str(), name.as_deref()))
                } else {
                    None
                }
            } else {
                None
            }
        })
        .collect();

    // FK lookup by column, carrying the constraint position that keys `relation_names`.
    let fk_by_col: HashMap<&str, (usize, &TableConstraint)> = table
        .constraints
        .iter()
        .enumerate()
        .filter_map(|(constraint_idx, c)| match c {
            TableConstraint::ForeignKey { columns, .. } if columns.len() == 1 => {
                Some((columns[0].as_str(), (constraint_idx, c)))
            }
            _ => None,
        })
        .collect();

    // Count FKs per ref_table for disambiguation detection. Counted over every
    // FK constraint — a single-column and a composite FK to the same target
    // are still two relations, and Prisma requires both to be named.
    let mut ref_table_fk_count: HashMap<&str, usize> = HashMap::new();
    for c in &table.constraints {
        if let TableConstraint::ForeignKey { ref_table, .. } = c {
            *ref_table_fk_count.entry(ref_table.as_str()).or_default() += 1;
        }
    }

    let relation_names = fk_relation_names(table);

    // Prisma rejects a model with two fields of the same name, and relation field
    // names are derived from column/table names. Every column is claimed up front
    // so a relation derived from one column cannot take a later column's name.
    let mut field_names: HashSet<String> = table
        .columns
        .iter()
        .map(|col| prisma_field_name(col.name.as_str()))
        .collect();

    // Render scalar fields + inline relation fields
    for col in &table.columns {
        let db_name = col.name.as_str();
        let col_name = prisma_field_name(db_name);
        let in_pk = pk_columns.contains(db_name);
        let is_single_pk = in_pk && !is_composite_pk;
        let auto_inc = is_single_pk && pk_auto_increment;
        let is_unique = unique_single.get(db_name).copied();

        if let Some(ref comment) = col.comment {
            let comment = comment.replace('\n', " ");
            lines.push(format!("  /// {comment}"));
        }

        let type_str =
            column_type_to_prisma(&col.r#type, col.nullable, table.name.as_str(), ambiguous);
        let mut attrs: Vec<String> = Vec::new();

        if is_single_pk {
            attrs.push("@id".into());
            if auto_inc {
                attrs.push("@default(autoincrement())".into());
            }
        }

        if !auto_inc && let Some(ref default) = col.default {
            attrs.push(prisma_default_attr(&default.to_sql(), &col.r#type));
        }

        if let Some(unique_name) = is_unique
            && !is_single_pk
        {
            // The SQL layer always names the index (a user-supplied name is a
            // key inside the convention, not the final name), so the map has
            // to go through the same builder or `prisma migrate` sees a
            // different index than the one vespertide created.
            let n = build_unique_constraint_name(table.name.as_str(), &[db_name], unique_name);
            attrs.push(format!("@unique(map: \"{n}\")"));
        }

        // A renamed field no longer points at its column by name.
        if col_name != db_name {
            attrs.push(format!("@map(\"{db_name}\")"));
        }

        let attrs_str = if attrs.is_empty() {
            String::new()
        } else {
            format!(" {}", attrs.join(" "))
        };

        lines.push(format!("  {col_name} {type_str}{attrs_str}"));

        // Emit inline relation field for FK columns
        if let Some(&(constraint_idx, fk)) = fk_by_col.get(db_name)
            && let TableConstraint::ForeignKey {
                ref_table,
                ref_columns,
                on_delete,
                on_update,
                ..
            } = fk
        {
            let rel_field_name = claim_field_name(
                prisma_field_name(infer_relation_field_name(db_name)),
                &mut field_names,
            );
            let rel_model = prisma_model_name(ref_table);
            let rel_type = if col.nullable {
                format!("{rel_model}?")
            } else {
                rel_model
            };

            let multi_fk = ref_table_fk_count
                .get(ref_table.as_str())
                .copied()
                .unwrap_or(0)
                > 1;
            let is_self_ref = ref_table == &table.name;

            let mut rel_args: Vec<String> = Vec::new();
            if (multi_fk || is_self_ref)
                && let Some(name) = relation_names.get(&constraint_idx)
            {
                rel_args.push(format!("\"{name}\""));
            }
            rel_args.push(format!("fields: [{col_name}]"));
            rel_args.push(format!("references: [{}]", field_list(ref_columns)));
            push_referential_actions(&mut rel_args, on_delete.as_ref(), on_update.as_ref());

            let rel_args_str = rel_args.join(", ");
            lines.push(format!(
                "  {rel_field_name} {rel_type} @relation({rel_args_str})"
            ));
        }
    }

    // Composite FKs span several columns, so their relation fields follow the
    // scalar fields instead of sitting inline with one column.
    for (constraint_idx, c) in table.constraints.iter().enumerate() {
        let TableConstraint::ForeignKey {
            columns,
            ref_table,
            ref_columns,
            on_delete,
            on_update,
            ..
        } = c
        else {
            continue;
        };
        if columns.len() < 2 {
            continue;
        }

        let rel_field_name = claim_field_name(prisma_field_name(ref_table), &mut field_names);
        let rel_model = prisma_model_name(ref_table);
        // Prisma requires the relation to be optional when any of its scalar
        // fields is optional.
        let any_nullable = table
            .columns
            .iter()
            .any(|col| col.nullable && columns.contains(&col.name));
        let rel_type = if any_nullable {
            format!("{rel_model}?")
        } else {
            rel_model
        };

        let multi_fk = ref_table_fk_count
            .get(ref_table.as_str())
            .is_some_and(|count| *count > 1);
        let is_self_ref = *ref_table == table.name;

        let mut rel_args: Vec<String> = Vec::new();
        if (multi_fk || is_self_ref)
            && let Some(name) = relation_names.get(&constraint_idx)
        {
            rel_args.push(format!("\"{name}\""));
        }
        rel_args.push(format!("fields: [{}]", field_list(columns)));
        rel_args.push(format!("references: [{}]", field_list(ref_columns)));
        push_referential_actions(&mut rel_args, on_delete.as_ref(), on_update.as_ref());

        let rel_args_str = rel_args.join(", ");
        lines.push(format!(
            "  {rel_field_name} {rel_type} @relation({rel_args_str})"
        ));
    }

    // Back-relations from schema context
    if !schema.is_empty() {
        let back_rels = collect_back_relations(&table.name, schema);
        for br in &back_rels {
            let (base_name, rel_type) = back_rel_field(br);
            let field_name = claim_field_name(base_name, &mut field_names);
            let rel_attr = match &br.relation_name {
                Some(name) => format!(" @relation(\"{name}\")"),
                None => String::new(),
            };
            lines.push(format!("  {field_name} {rel_type}{rel_attr}"));
        }
    }

    // Blank line before model-level attributes
    lines.push(String::new());

    // Composite PK
    if is_composite_pk {
        let pk_list = field_list(pk_cols);
        lines.push(format!("  @@id([{pk_list}])"));
    }

    // Composite unique constraints. Like the field-level `@unique`, the map
    // carries the name the SQL layer actually gives the index.
    for c in &table.constraints {
        if let TableConstraint::Unique { name, columns, .. } = c
            && columns.len() > 1
        {
            let cols = field_list(columns);
            let n = build_unique_constraint_name(table.name.as_str(), columns, name.as_deref());
            lines.push(format!("  @@unique([{cols}], map: \"{n}\")"));
        }
    }

    // All index constraints
    for c in &table.constraints {
        if let TableConstraint::Index { name, columns } = c {
            let cols = field_list(columns);
            let n = build_index_name(table.name.as_str(), columns, name.as_deref());
            lines.push(format!("  @@index([{cols}], map: \"{n}\")"));
        }
    }

    // @@map (always present since model is PascalCase but table is snake_case)
    let table_name = table.name.as_str();
    lines.push(format!("  @@map(\"{table_name}\")"));
    lines.push("}".into());

    lines.join("\n")
}

fn prisma_default_attr(default_sql: &str, col_type: &ColumnType) -> String {
    // Integer-backed enum: resolve to a variant identifier (SCREAMING_SNAKE), never a bare int.
    if let ColumnType::Complex(ComplexColumnType::Enum {
        values: EnumValues::Integer(int_values),
        ..
    }) = col_type
    {
        let key = unquote(default_sql);
        // 1) numeric value match → variant name
        if let Ok(n) = key.parse::<i64>()
            && let Some(v) = int_values.iter().find(|v| v.value == n)
        {
            return format!("@default({})", enum_variant(&v.name));
        }
        // 2) exact variant-name match → variant name
        if let Some(v) = int_values.iter().find(|v| v.name == key) {
            return format!("@default({})", enum_variant(&v.name));
        }
        // 3) no match → dbgenerated fallback (valid PSL; avoids bare-int type error)
        let escaped = key.replace('"', "\\\"");
        return format!("@default(dbgenerated(\"{escaped}\"))");
    }

    if default_sql == "true" {
        return "@default(true)".into();
    }
    if default_sql == "false" {
        return "@default(false)".into();
    }

    let lower = default_sql.to_lowercase();
    if lower.contains("now()") || lower.starts_with("current_timestamp") {
        return "@default(now())".into();
    }
    if lower.contains("gen_random_uuid()")
        || lower.contains("uuid_generate_v4()")
        || lower.contains("newid()")
    {
        return "@default(uuid())".into();
    }

    // Any remaining function call → dbgenerated
    if default_sql.contains('(') {
        let escaped = default_sql.replace('"', "\\\"");
        return format!("@default(dbgenerated(\"{escaped}\"))");
    }

    // String literal with quotes — may be an enum value
    if default_sql.starts_with('\'') || default_sql.starts_with('"') {
        let stripped = unquote(default_sql);
        if let ColumnType::Complex(ComplexColumnType::Enum {
            values: EnumValues::String(variants),
            ..
        }) = col_type
            && variants.iter().any(|v| v.as_str() == stripped)
        {
            let variant = enum_variant(stripped);
            return format!("@default({variant})");
        }
        let s = stripped.replace('\\', "\\\\").replace('"', "\\\"");
        return format!("@default(\"{s}\")");
    }

    // Numeric
    if default_sql.parse::<f64>().is_ok() {
        return format!("@default({default_sql})");
    }

    // Fallback
    let escaped = default_sql.replace('"', "\\\"");
    format!("@default(dbgenerated(\"{escaped}\"))")
}

/// Append `onDelete` / `onUpdate` to a `@relation`, spelling out `NoAction`
/// when the model leaves them unset. The SQL layer omits the clauses, which
/// means `NO ACTION` in every backend — while Prisma's implicit defaults are
/// `SetNull`/`Restrict` + `Cascade`, so an attribute-less relation would make
/// `prisma migrate` rewrite the FK vespertide created.
fn push_referential_actions(
    rel_args: &mut Vec<String>,
    on_delete: Option<&ReferenceAction>,
    on_update: Option<&ReferenceAction>,
) {
    let od = on_delete.unwrap_or(&ReferenceAction::NoAction);
    let ou = on_update.unwrap_or(&ReferenceAction::NoAction);
    rel_args.push(format!("onDelete: {}", reference_action_to_prisma(od)));
    rel_args.push(format!("onUpdate: {}", reference_action_to_prisma(ou)));
}

fn reference_action_to_prisma(action: &ReferenceAction) -> &'static str {
    match action {
        ReferenceAction::Cascade => "Cascade",
        ReferenceAction::Restrict => "Restrict",
        ReferenceAction::SetNull => "SetNull",
        ReferenceAction::SetDefault => "SetDefault",
        // Includes NoAction and unknown/future referential actions.
        _ => "NoAction",
    }
}

/// Render a column list for a model-level attribute (`@@id`, `@@unique`,
/// `@@index`).
///
/// These name fields of the model, not database columns, so an escaped column
/// has to appear under its Prisma name. The `map:` argument alongside them is
/// what still carries the constraint's database name.
fn field_list<T: AsRef<str>>(columns: &[T]) -> String {
    columns
        .iter()
        .map(|column| prisma_field_name(column.as_ref()))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Prisma model name for a table. `@@map` carries the table name itself, so the
/// identifier only has to be valid.
fn prisma_model_name(table: &str) -> String {
    sanitize_identifier(&to_pascal_case(table), IdentifierStart::Letter)
}

/// Prisma field name for a column. `@map` carries the column name itself, so the
/// identifier only has to be valid.
fn prisma_field_name(column: &str) -> String {
    sanitize_identifier(column, IdentifierStart::Letter)
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use vespertide_core::ColumnDef;
    use vespertide_core::schema::column::{NumValue, SimpleColumnType};
    use vespertide_core::schema::primary_key::PrimaryKeySyntax;

    use super::*;

    #[rstest]
    #[case::cascade(ReferenceAction::Cascade, "Cascade")]
    #[case::restrict(ReferenceAction::Restrict, "Restrict")]
    #[case::set_null(ReferenceAction::SetNull, "SetNull")]
    #[case::set_default(ReferenceAction::SetDefault, "SetDefault")]
    #[case::no_action(ReferenceAction::NoAction, "NoAction")]
    fn reference_actions_map_to_prisma(#[case] action: ReferenceAction, #[case] expected: &str) {
        assert_eq!(reference_action_to_prisma(&action), expected);
    }

    fn string_enum() -> ColumnType {
        ColumnType::Complex(ComplexColumnType::Enum {
            name: "doc_status".into(),
            values: EnumValues::String(vec!["draft".into(), "in progress".into()]),
        })
    }

    fn integer_enum() -> ColumnType {
        ColumnType::Complex(ComplexColumnType::Enum {
            name: "priority".into(),
            values: EnumValues::Integer(vec![
                NumValue {
                    name: "low".into(),
                    value: 100,
                },
                NumValue {
                    name: "high".into(),
                    value: 200,
                },
            ]),
        })
    }

    #[rstest]
    #[case::bool_true("true", "@default(true)")]
    #[case::bool_false("false", "@default(false)")]
    #[case::now("now()", "@default(now())")]
    #[case::current_timestamp("CURRENT_TIMESTAMP", "@default(now())")]
    #[case::uuid_postgres("gen_random_uuid()", "@default(uuid())")]
    #[case::uuid_generate_v4("uuid_generate_v4()", "@default(uuid())")]
    #[case::uuid_mssql("NEWID()", "@default(uuid())")]
    #[case::other_function("gen_code()", "@default(dbgenerated(\"gen_code()\"))")]
    #[case::quoted_literal("'active'", "@default(\"active\")")]
    #[case::quoted_literal_with_inner_quotes("'say \"hi\"'", "@default(\"say \\\"hi\\\"\")")]
    #[case::numeric("0", "@default(0)")]
    #[case::bare_word("SOME_CONSTANT", "@default(dbgenerated(\"SOME_CONSTANT\"))")]
    fn default_attr_maps_scalar_forms(#[case] default_sql: &str, #[case] expected: &str) {
        let non_enum = ColumnType::Simple(SimpleColumnType::Text);
        assert_eq!(prisma_default_attr(default_sql, &non_enum), expected);
    }

    #[rstest]
    #[case::string_variant("'draft'", string_enum(), "@default(DRAFT)")]
    #[case::string_variant_normalized("'in progress'", string_enum(), "@default(IN_PROGRESS)")]
    // Emitting `ARCHIVED` would reference a value the enum does not define.
    #[case::string_value_not_declared("'archived'", string_enum(), "@default(\"archived\")")]
    #[case::integer_by_value("100", integer_enum(), "@default(LOW)")]
    #[case::integer_by_name("high", integer_enum(), "@default(HIGH)")]
    #[case::integer_by_quoted_name("'high'", integer_enum(), "@default(HIGH)")]
    #[case::integer_value_not_declared("999", integer_enum(), "@default(dbgenerated(\"999\"))")]
    fn default_attr_resolves_enum_defaults(
        #[case] default_sql: &str,
        #[case] col_type: ColumnType,
        #[case] expected: &str,
    ) {
        assert_eq!(prisma_default_attr(default_sql, &col_type), expected);
    }

    #[test]
    fn fk_on_update_action_is_rendered() {
        let mut table = crate::tests::fixtures::table_with_fk();
        for c in &mut table.constraints {
            if let TableConstraint::ForeignKey { on_update, .. } = c {
                *on_update = Some(ReferenceAction::Cascade);
            }
        }
        let rendered = render_model(&table, std::slice::from_ref(&table), &HashSet::new());
        assert!(rendered.contains("onUpdate: Cascade"));
    }

    /// Source table with a composite FK `[a, b]` to `target`, plus the given
    /// extra constraints deciding whether one source row can repeat a target key.
    fn composite_fk_source(extra: Vec<TableConstraint>) -> TableDef {
        let mut constraints = vec![TableConstraint::ForeignKey {
            name: None,
            columns: vec!["a".into(), "b".into()],
            ref_table: "target".into(),
            ref_columns: vec!["a".into(), "b".into()],
            on_delete: None,
            on_update: None,
            orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
        }];
        constraints.extend(extra);
        TableDef {
            name: "src".into(),
            description: None,
            columns: vec![
                ColumnDef::new("a", ColumnType::Simple(SimpleColumnType::Integer), false),
                ColumnDef::new("b", ColumnType::Simple(SimpleColumnType::Integer), false),
            ],
            constraints,
        }
    }

    fn pk_of(columns: &[&str]) -> TableConstraint {
        TableConstraint::PrimaryKey {
            auto_increment: false,
            columns: columns.iter().copied().map(Into::into).collect(),
            strategy: vespertide_core::PrimaryKeyAdditionStrategy::default(),
        }
    }

    fn unique_of(columns: &[&str]) -> TableConstraint {
        TableConstraint::Unique {
            name: None,
            columns: columns.iter().copied().map(Into::into).collect(),
            strategy: vespertide_core::UniqueConstraintStrategy::default(),
        }
    }

    /// A composite relation carries `onDelete` / `onUpdate` like a single-column
    /// one, and turns optional as soon as one of its scalar fields is nullable.
    #[test]
    fn composite_fk_renders_actions_and_optionality() {
        let mut table = crate::tests::fixtures::table_with_composite_fk();
        for c in &mut table.constraints {
            if let TableConstraint::ForeignKey {
                on_delete,
                on_update,
                ..
            } = c
            {
                *on_delete = Some(ReferenceAction::Cascade);
                *on_update = Some(ReferenceAction::Restrict);
            }
        }
        for col in &mut table.columns {
            if col.name == "order_version" {
                col.nullable = true;
            }
        }
        let rendered = render_model(&table, std::slice::from_ref(&table), &HashSet::new());
        assert!(rendered.contains(
            "  orders Orders? @relation(fields: [order_id, order_version], \
             references: [id, version], onDelete: Cascade, onUpdate: Restrict)"
        ));
    }

    /// One-to-one only when the source is bounded to one row per target key —
    /// its PK or a unique must cover the FK columns exactly, not merely
    /// overlap them.
    #[rstest]
    #[case::pk_equals_fk(vec![pk_of(&["a", "b"])], true)]
    #[case::pk_is_subset_of_fk(vec![pk_of(&["a"])], false)]
    #[case::unique_equals_fk(vec![pk_of(&["a"]), unique_of(&["a", "b"])], true)]
    #[case::unique_is_subset_of_fk(vec![unique_of(&["a"])], false)]
    fn composite_back_relation_one_to_one_needs_exact_cover(
        #[case] extra: Vec<TableConstraint>,
        #[case] expected: bool,
    ) {
        let source = composite_fk_source(extra);
        let rels = collect_back_relations("target", std::slice::from_ref(&source));
        assert_eq!(rels.len(), 1);
        assert_eq!(rels[0].is_one_to_one, expected);
    }

    /// A back-relation is named after the source table, so it can land on a name
    /// the target model already uses; Prisma rejects duplicate field names.
    #[rstest]
    #[case::free(&[], "book")]
    #[case::column_holds_the_name(&["book"], "book_rel")]
    #[case::suffixed_name_also_held(&["book", "book_rel"], "book_rel2")]
    #[case::numbered_name_also_held(&["book", "book_rel", "book_rel2"], "book_rel3")]
    fn back_relation_field_name_avoids_names_already_in_the_model(
        #[case] existing_columns: &[&str],
        #[case] expected_field: &str,
    ) {
        let author = author_table(existing_columns);
        let schema = vec![author.clone(), book_table(&[])];

        let rendered = render_model(&author, &schema, &HashSet::new());

        assert!(rendered.contains(&format!("  {expected_field} Book[]")));
    }

    /// The relation field for an FK column is derived from that column's name,
    /// so it can land on a column declared further down the table.
    #[test]
    fn forward_relation_field_name_avoids_a_column_declared_later() {
        let book = book_table(&["author"]);

        let rendered = render_model(&book, std::slice::from_ref(&book), &HashSet::new());

        assert!(rendered.contains("  author String?"));
        assert!(rendered.contains("  author_rel Author @relation(fields: [author_id]"));
    }

    /// `author` with a primary key, then one nullable text column per extra name.
    fn author_table(extra_columns: &[&str]) -> TableDef {
        TableDef {
            name: "author".into(),
            description: None,
            columns: with_text_columns(
                vec![
                    ColumnDef::new("id", ColumnType::Simple(SimpleColumnType::Integer), false)
                        .primary_key(PrimaryKeySyntax::Bool(true)),
                ],
                extra_columns,
            ),
            constraints: vec![],
        }
        .normalize()
        .expect("author normalizes")
    }

    /// `book` with a single-column foreign key back to `author`, then one nullable
    /// text column per extra name — declared *after* the FK column on purpose.
    fn book_table(extra_columns: &[&str]) -> TableDef {
        TableDef {
            name: "book".into(),
            description: None,
            columns: with_text_columns(
                vec![
                    ColumnDef::new("id", ColumnType::Simple(SimpleColumnType::Integer), false)
                        .primary_key(PrimaryKeySyntax::Bool(true)),
                    ColumnDef::new(
                        "author_id",
                        ColumnType::Simple(SimpleColumnType::Integer),
                        false,
                    ),
                ],
                extra_columns,
            ),
            constraints: vec![TableConstraint::ForeignKey {
                name: None,
                columns: vec!["author_id".into()],
                ref_table: "author".into(),
                ref_columns: vec!["id".into()],
                on_delete: None,
                on_update: None,
                orphan_strategy: vespertide_core::ForeignKeyOrphanStrategy::default(),
            }],
        }
        .normalize()
        .expect("book normalizes")
    }

    fn with_text_columns(mut columns: Vec<ColumnDef>, names: &[&str]) -> Vec<ColumnDef> {
        columns.extend(
            names.iter().map(|name| {
                ColumnDef::new(*name, ColumnType::Simple(SimpleColumnType::Text), true)
            }),
        );
        columns
    }

    /// The map has to carry the name the SQL layer actually creates: a
    /// user-supplied name is a key inside the `ix_{table}__{key}` convention,
    /// and an unnamed index still gets a conventional name.
    #[test]
    fn index_maps_carry_the_sql_layer_names() {
        let table = crate::tests::fixtures::table_with_indexes();
        let rendered = render_model(&table, &[], &HashSet::new());
        assert!(
            rendered
                .contains("@@index([created_at], map: \"ix_articles__idx_articles_created_at\")")
        );
        assert!(rendered.contains("@@index([title], map: \"ix_articles__title\")"));
    }
}
