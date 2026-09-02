//! Table, relation and default-value rendering.

use std::collections::{HashMap, HashSet};

use vespertide_core::TableDef;
use vespertide_core::schema::column::{
    ColumnType, ComplexColumnType, EnumValues, SimpleColumnType,
};
use vespertide_core::schema::constraint::TableConstraint;
use vespertide_core::schema::names::ColumnName;
use vespertide_core::schema::reference::ReferenceAction;
use vespertide_naming::{build_foreign_key_name, build_index_name, build_unique_constraint_name};

use super::bindings::FileBindings;
use super::types::column_ctor;
use super::{DrizzleDialect, Imports, js_name};
use crate::constraint_scan::{collect_back_relations, fk_relation_names, relation_segment};
use crate::utils::common::{claim_field_name, unquote};
use crate::utils::typescript::ts_string;

// ─── Constraint lookups ──────────────────────────────────────────────────────

/// PK column names and whether the key carries a sequence.
fn pk_shape(constraints: &[TableConstraint]) -> (Vec<&str>, bool) {
    match crate::constraint_scan::primary_key(constraints) {
        Some(TableConstraint::PrimaryKey {
            columns,
            auto_increment,
            ..
        }) => (
            columns.iter().map(ColumnName::as_str).collect(),
            *auto_increment,
        ),
        _ => (Vec::new(), false),
    }
}

// ─── Table renderer ──────────────────────────────────────────────────────────

/// Render one table declaration.
///
/// Everything the file's import header depends on — the constructors a column
/// resolved to, whether a default reached for the `sql` tag, which constraint
/// helpers the callback used — is recorded into `imports` at the moment the
/// decision is made, so the header can never disagree with the body.
pub(super) fn render_table(
    table: &TableDef,
    dialect: DrizzleDialect,
    imports: &mut Imports,
    bindings: &FileBindings,
) -> String {
    let (pk_columns, pk_auto_increment) = pk_shape(&table.constraints);
    let pk_cols: HashSet<&str> = pk_columns.iter().copied().collect();
    let is_composite_pk = pk_columns.len() > 1;

    // ── Column lines ────────────────────────────────────────────────────────
    let mut col_lines: Vec<String> = Vec::new();

    for col in &table.columns {
        let col_db = col.name.as_str();
        let key = js_name(col_db);
        let in_pk = pk_cols.contains(col_db);
        let is_single_pk = in_pk && !is_composite_pk;
        let auto_inc = is_single_pk && pk_auto_increment;

        if let Some(comment) = &col.comment {
            for line in comment.lines() {
                col_lines.push(format!("  // {line}"));
            }
        }

        let ctor = column_ctor(&col.r#type, dialect, &table.name, bindings);
        if !ctor.local {
            imports.symbols.insert(ctor.symbol.clone());
        }
        let mut chain: Vec<String> = Vec::new();

        if is_single_pk {
            chain.push(primary_key_chain(dialect, auto_inc));
        }

        // A primary key already implies NOT NULL in every dialect.
        if !col.nullable && !is_single_pk {
            chain.push(".notNull()".to_string());
        }

        // A sequence supplies the value, so a default alongside it would be
        // dead weight at best and contradictory at worst.
        if !auto_inc && let Some(default) = &col.default {
            let rendered = default_chain(&default.to_sql(), &col.r#type, dialect);
            imports.needs_sql |= rendered.needs_sql;
            chain.push(rendered.text);
        }

        col_lines.push(format!("  {key}: {}{},", ctor.call(col_db), chain.concat()));
    }

    // ── Table-level constraints (array-form callback) ────────────────────────
    let mut constraint_lines: Vec<String> = Vec::new();

    if is_composite_pk {
        imports.symbols.insert("primaryKey".to_string());
        // The name `drizzle-kit` expects back, or it drops and re-adds the
        // key: on PostgreSQL the constraint its inline `PRIMARY KEY (…)`
        // syntax creates is `{table}_pkey`; on MySQL no name is stored at all,
        // but the kit snapshot books the introspected key under
        // `{table}_{columns}` (measured against drizzle-kit 0.31). SQLite
        // stores no name and its kit compares by columns alone.
        let name_field = match dialect {
            DrizzleDialect::Pg => {
                format!("name: {}, ", ts_string(&format!("{}_pkey", table.name)))
            }
            DrizzleDialect::Mysql => {
                let joined = pk_columns.join("_");
                format!("name: {}, ", ts_string(&format!("{}_{joined}", table.name)))
            }
            DrizzleDialect::Sqlite => String::new(),
        };
        constraint_lines.push(format!(
            "  primaryKey({{ {name_field}columns: [{}] }}),",
            column_refs(&pk_columns)
        ));
    }

    for c in &table.constraints {
        match c {
            // `uniqueIndex`, not `unique`: the SQL layer creates every unique
            // rule as `CREATE UNIQUE INDEX`, and PostgreSQL introspection
            // tells an index from a table constraint — `unique(…)` here would
            // read as "drop the index, add a constraint" to `drizzle-kit`.
            // The name always comes from the naming builder (a user-supplied
            // name is a key inside the convention, not the final name), so the
            // model names the exact index vespertide created.
            TableConstraint::Unique { name, columns, .. } => {
                imports.symbols.insert("uniqueIndex".to_string());
                let n = build_unique_constraint_name(&table.name, columns, name.as_deref());
                constraint_lines.push(table_level_entry("uniqueIndex", &n, columns));
            }
            TableConstraint::Index { name, columns } => {
                imports.symbols.insert("index".to_string());
                let n = build_index_name(&table.name, columns, name.as_deref());
                constraint_lines.push(table_level_entry("index", &n, columns));
            }
            TableConstraint::ForeignKey {
                name,
                columns,
                ref_table,
                ref_columns,
                on_delete,
                on_update,
                ..
            } => {
                imports.symbols.insert("foreignKey".to_string());
                // SQLite stores no foreign-key constraint names — the SQL
                // layer emits them inline and unnamed there — so a named key
                // would read as permanent drift to `drizzle-kit` introspection.
                let n = (dialect != DrizzleDialect::Sqlite)
                    .then(|| build_foreign_key_name(&table.name, columns, name.as_deref()));
                constraint_lines.push(foreign_key_entry(
                    n.as_deref(),
                    columns,
                    ref_table.as_str(),
                    ref_columns,
                    on_delete.as_ref(),
                    on_update.as_ref(),
                    &table.name,
                    bindings,
                ));
            }
            TableConstraint::Check { name, expr, .. } => {
                imports.symbols.insert("check".to_string());
                imports.needs_sql = true;
                // The SQL layer emits the check's name verbatim (no builder),
                // so the model does too.
                constraint_lines.push(format!(
                    "  check({}, sql`{}`),",
                    ts_string(name),
                    escape_backtick(expr)
                ));
            }
            // The primary key is rendered from `pk_shape` above.
            // `TableConstraint` is `#[non_exhaustive]`, so future variants
            // also land here — the table still renders, minus the constraint
            // this version cannot know about.
            _ => {}
        }
    }

    // ── Assemble the table call ──────────────────────────────────────────────
    let mut lines: Vec<String> = Vec::new();

    if let Some(desc) = &table.description {
        for line in desc.lines() {
            lines.push(format!("// {line}"));
        }
    }

    imports.symbols.insert(dialect.table_fn().to_string());
    lines.push(format!(
        "export const {} = {}({}, {{",
        bindings.table_const(&table.name),
        dialect.table_fn(),
        ts_string(&table.name)
    ));
    lines.extend(col_lines);
    if constraint_lines.is_empty() {
        lines.push("});".to_string());
    } else {
        lines.push("}, (t) => [".to_string());
        lines.extend(constraint_lines);
        lines.push("]);".to_string());
    }

    lines.join("\n")
}

/// How each dialect spells a single-column primary key.
///
/// Each dialect spells the sequence differently — and on PostgreSQL it must be
/// the identity chain, not `serial`: the SQL layer emits `GENERATED BY DEFAULT
/// AS IDENTITY`, and a `serial` model column reads as "drop the identity" to
/// `drizzle-kit`.
fn primary_key_chain(dialect: DrizzleDialect, auto_increment: bool) -> String {
    match (dialect, auto_increment) {
        (DrizzleDialect::Pg, true) => ".primaryKey().generatedByDefaultAsIdentity()".to_string(),
        (DrizzleDialect::Mysql, true) => ".autoincrement().primaryKey()".to_string(),
        (DrizzleDialect::Sqlite, true) => ".primaryKey({ autoIncrement: true })".to_string(),
        _ => ".primaryKey()".to_string(),
    }
}

/// `t.first, t.second` — the column list a table-level constraint builds on.
fn column_refs<T: AsRef<str>>(columns: &[T]) -> String {
    columns
        .iter()
        .map(|c| format!("t.{}", js_name(c.as_ref())))
        .collect::<Vec<_>>()
        .join(", ")
}

/// One entry of the table-level constraint callback.
///
/// `builder` is the Drizzle helper (`uniqueIndex` / `index`) and `name` is the
/// already-resolved database name (see the naming-builder comment at the
/// single-column unique site). The name is mandatory: `mysql-core` and
/// `sqlite-core` reject a nameless call.
fn table_level_entry(builder: &str, name: &str, columns: &[ColumnName]) -> String {
    format!(
        "  {builder}({}).on({}),",
        ts_string(name),
        column_refs(columns)
    )
}

/// `foreignKey({ columns: […], foreignColumns: […], name: "fk_…" })` plus the
/// `.onDelete(…)`/`.onUpdate(…)` chain.
///
/// The operator form rather than a `.references()` chain, for three reasons it
/// covers and the chain cannot: it carries the constraint's *name* — the SQL
/// layer names PostgreSQL/MySQL foreign keys via `build_foreign_key_name`, and
/// a differently-named key reads as drift to `drizzle-kit` (`None` on SQLite,
/// which stores no FK names) — it spells composite keys, and a self-referential
/// key can take its foreign columns from the callback's `t` (the key targets
/// this very table), which keeps the table const out of its own initializer's
/// type inference.
#[expect(
    clippy::too_many_arguments,
    reason = "one call site; the args are the FK's own fields plus the two naming contexts"
)]
fn foreign_key_entry(
    name: Option<&str>,
    columns: &[ColumnName],
    ref_table: &str,
    ref_columns: &[ColumnName],
    on_delete: Option<&ReferenceAction>,
    on_update: Option<&ReferenceAction>,
    table: &str,
    bindings: &FileBindings,
) -> String {
    let foreign_owner = if ref_table == table {
        "t".to_string()
    } else {
        bindings.table_const(ref_table)
    };
    // A foreign key with no explicit target column references the parent's
    // primary key, which vespertide names `id` by convention.
    let foreign_cols = if ref_columns.is_empty() {
        format!("{foreign_owner}.id")
    } else {
        ref_columns
            .iter()
            .map(|c| format!("{foreign_owner}.{}", js_name(c)))
            .collect::<Vec<_>>()
            .join(", ")
    };

    let name_field = name.map_or_else(String::new, |n| format!(", name: {}", ts_string(n)));
    let mut parts = vec![format!(
        "  foreignKey({{ columns: [{}], foreignColumns: [{foreign_cols}]{name_field} }})",
        column_refs(columns)
    )];
    if let Some(action) = on_delete {
        parts.push(format!(
            ".onDelete({})",
            ts_string(reference_action_to_drizzle(action))
        ));
    }
    if let Some(action) = on_update {
        parts.push(format!(
            ".onUpdate({})",
            ts_string(reference_action_to_drizzle(action))
        ));
    }
    parts.push(",".to_string());
    parts.concat()
}

// ─── Relations renderer ──────────────────────────────────────────────────────

/// Render a `relations(...)` export block, or `None` when the table has none.
pub(super) fn render_relations_block(
    table: &TableDef,
    schema: &[TableDef],
    bindings: &FileBindings,
) -> Option<String> {
    let table_js = bindings.table_const(&table.name);
    let relation_names = fk_relation_names(table);

    let mut ref_table_fk_count: HashMap<&str, usize> = HashMap::new();
    for c in &table.constraints {
        if let TableConstraint::ForeignKey { ref_table, .. } = c {
            *ref_table_fk_count.entry(ref_table.as_str()).or_default() += 1;
        }
    }

    let back_rels = collect_back_relations(&table.name, schema);

    // Drizzle merges columns and relations into one namespace in query
    // results, and the object literal itself rejects a repeated key, so every
    // relation field is claimed against the column names and one another.
    let mut field_names: HashSet<String> = table
        .columns
        .iter()
        .map(|col| js_name(col.name.as_str()))
        .collect();

    let mut rel_lines: Vec<String> = Vec::new();

    // Forward relations, in constraint order.
    for (constraint_idx, c) in table.constraints.iter().enumerate() {
        let TableConstraint::ForeignKey {
            columns,
            ref_table,
            ref_columns,
            ..
        } = c
        else {
            continue;
        };

        let segment = relation_segment(columns);
        let mut preferred = js_name(&segment);
        // A segment that is already a column's name (an FK column without an
        // `_id` suffix, or any other column) reads better with the target
        // table appended than with the generic `_rel` suffix.
        if field_names.contains(&preferred) {
            preferred = js_name(&format!("{segment}_{ref_table}"));
        }
        let field = claim_field_name(preferred, &mut field_names);
        let target = bindings.table_const(ref_table.as_str());

        let fields_list = columns
            .iter()
            .map(|c| format!("{table_js}.{}", js_name(c)))
            .collect::<Vec<_>>()
            .join(", ");
        let refs_list = if ref_columns.is_empty() {
            format!("{target}.id")
        } else {
            ref_columns
                .iter()
                .map(|c| format!("{target}.{}", js_name(c)))
                .collect::<Vec<_>>()
                .join(", ")
        };

        let mut opts: Vec<String> = vec![
            format!("fields: [{fields_list}]"),
            format!("references: [{refs_list}]"),
        ];
        let ambiguous = ref_table_fk_count
            .get(ref_table.as_str())
            .is_some_and(|n| *n > 1)
            || ref_table.as_str() == table.name.as_str();
        if ambiguous && let Some(name) = relation_names.get(&constraint_idx) {
            opts.push(format!("relationName: {}", ts_string(name)));
        }

        rel_lines.push(format!(
            "  {field}: one({target}, {{ {} }}),",
            opts.join(", ")
        ));
    }

    // Back relations.
    for br in &back_rels {
        let source = bindings.table_const(&br.source_table);
        let preferred = match &br.relation_name {
            Some(_) => js_name(&format!("{}_{}", br.rel_segment, br.source_table)),
            None => source.clone(),
        };
        let field = claim_field_name(preferred, &mut field_names);
        let opts = br.relation_name.as_ref().map_or_else(String::new, |n| {
            format!(", {{ relationName: {} }}", ts_string(n))
        });
        let builder = if br.is_one_to_one { "one" } else { "many" };
        rel_lines.push(format!("  {field}: {builder}({source}{opts}),"));
    }

    if rel_lines.is_empty() {
        return None;
    }

    let mut lines: Vec<String> = vec![format!(
        "export const {} = relations({table_js}, ({{ one, many }}) => ({{",
        bindings.relations_const(&table.name)
    )];
    lines.extend(rel_lines);
    lines.push("}));".to_string());

    Some(lines.join("\n"))
}

// ─── Default value rendering ─────────────────────────────────────────────────

/// A rendered `.default(...)` chain and whether it reached for the `sql` tag.
///
/// The import header needs to know about `sql` before any column is written, so
/// both answers come from one pass — deciding twice would let the header and
/// the body disagree.
pub(super) struct DefaultChain {
    pub(super) text: String,
    pub(super) needs_sql: bool,
}

impl DefaultChain {
    fn literal(text: String) -> Self {
        Self {
            text,
            needs_sql: false,
        }
    }

    /// `.default(sql`…`)` — a server-side expression Drizzle passes through.
    fn tagged(expr: &str) -> Self {
        Self {
            text: format!(".default(sql`{}`)", escape_backtick(expr)),
            needs_sql: true,
        }
    }
}

/// Backticks and `${` would end the tagged template or open an interpolation.
fn escape_backtick(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('`', "\\`")
        .replace("${", "\\${")
}

/// Render the `.default(...)` chain for a column default.
///
/// A `DefaultValue` string is a SQL expression rather than a literal, so most
/// of the work is recognising the handful of expressions Drizzle has a helper
/// for and passing everything else through the `sql` tag untouched.
pub(super) fn default_chain(
    default_sql: &str,
    col_type: &ColumnType,
    dialect: DrizzleDialect,
) -> DefaultChain {
    if default_sql == "true" || default_sql == "false" {
        return DefaultChain::literal(format!(".default({default_sql})"));
    }

    let lower = default_sql.to_lowercase();

    // The SQL layer normalizes both spellings to `DEFAULT CURRENT_TIMESTAMP`
    // in the DDL it runs, and each dialect has exactly one model spelling
    // `drizzle-kit` reads back as equal (measured on live PostgreSQL 17 and
    // MySQL 8 round-trips): PostgreSQL deparses `CURRENT_TIMESTAMP` as itself
    // — `defaultNow()` would write `now()` and read as a default change —
    // while MySQL introspects to `(now())`, which is precisely `defaultNow()`,
    // and SQLite requires the parentheses around an expression default.
    if lower.starts_with("current_timestamp") || lower.contains("now()") {
        return match dialect {
            DrizzleDialect::Pg => DefaultChain::tagged("CURRENT_TIMESTAMP"),
            DrizzleDialect::Mysql => DefaultChain::literal(".defaultNow()".to_string()),
            DrizzleDialect::Sqlite => DefaultChain::tagged("(CURRENT_TIMESTAMP)"),
        };
    }

    // Only `gen_random_uuid()` is normalized per backend by the SQL layer;
    // other generator spellings (`uuid_generate_v4()`, `newid()`) pass through
    // verbatim everywhere, so they fall to the generic call branch below.
    if lower.contains("gen_random_uuid()") {
        return match dialect {
            // `defaultRandom()` is a `pg-core` column method emitting
            // `gen_random_uuid()` — the same call the SQL layer uses.
            DrizzleDialect::Pg => DefaultChain::literal(".defaultRandom()".to_string()),
            // The other dialects mirror the generator the SQL layer actually
            // put on the column, not the PostgreSQL spelling of the model.
            // Lowercase on MySQL: `information_schema` stores `(uuid())`, and
            // the kit's comparison is case-sensitive (measured on MySQL 8).
            DrizzleDialect::Mysql => DefaultChain::tagged("(uuid())"),
            DrizzleDialect::Sqlite => DefaultChain::tagged("(lower(hex(randomblob(16))))"),
        };
    }

    // A JSON object or array literal has to reach the database as a JSON
    // literal, not as a bare SQL fragment.
    if matches!(col_type, ColumnType::Simple(SimpleColumnType::Json)) {
        let trimmed = default_sql.trim();
        if trimmed.starts_with('{') || trimmed.starts_with('[') {
            let quoted = trimmed.replace('\'', "''");
            return match dialect {
                DrizzleDialect::Pg => DefaultChain::tagged(&format!("'{quoted}'::json")),
                _ => DefaultChain::tagged(&format!("'{quoted}'")),
            };
        }
    }

    // Anything else that looks like a call is a server-side expression.
    if default_sql.contains('(') {
        return DefaultChain::tagged(default_sql);
    }

    if default_sql.starts_with('\'') || default_sql.starts_with('"') {
        // `unquote` keeps the doubled SQL escape (its other consumers re-emit
        // into SQL); a TS string wants the actual value, so undouble here.
        let inner = unquote(default_sql);
        let value = if default_sql.starts_with('\'') {
            inner.replace("''", "'")
        } else {
            inner.to_string()
        };
        return DefaultChain::literal(format!(".default({})", ts_string(&value)));
    }

    if default_sql.parse::<f64>().is_ok() {
        // Drizzle types a numeric/decimal default as a string — arbitrary
        // precision exceeds a JS number — so those keep the literal quoted.
        let numeric_col = matches!(
            col_type,
            ColumnType::Complex(ComplexColumnType::Numeric { .. })
        );
        return DefaultChain::literal(if numeric_col {
            format!(".default({})", ts_string(default_sql))
        } else {
            format!(".default({default_sql})")
        });
    }

    // An integer enum's default names a variant; the column stores its value.
    if let ColumnType::Complex(ComplexColumnType::Enum {
        values: EnumValues::Integer(variants),
        ..
    }) = col_type
        && let Some(variant) = variants.iter().find(|v| v.name == default_sql)
    {
        return DefaultChain::literal(format!(".default({})", variant.value));
    }

    // A bare keyword such as `CURRENT_USER`.
    DefaultChain::tagged(default_sql)
}

// ─── Reference action ────────────────────────────────────────────────────────

fn reference_action_to_drizzle(action: &ReferenceAction) -> &'static str {
    match action {
        ReferenceAction::Cascade => "cascade",
        ReferenceAction::Restrict => "restrict",
        ReferenceAction::SetNull => "set null",
        ReferenceAction::SetDefault => "set default",
        // `NoAction`, plus — `ReferenceAction` is `#[non_exhaustive]` — any
        // action added later, which falls back to the SQL default rather than
        // to a keyword Drizzle cannot parse.
        _ => "no action",
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use vespertide_core::schema::column::NumValue;

    use super::*;
    use crate::drizzle::DrizzleDialect::{Mysql, Pg, Sqlite};

    fn text_type() -> ColumnType {
        ColumnType::Simple(SimpleColumnType::Text)
    }

    // ── default_chain ────────────────────────────────────────────────────────

    #[rstest]
    #[case::bool_true("true", ".default(true)")]
    #[case::bool_false("false", ".default(false)")]
    #[case::number("42", ".default(42)")]
    #[case::float("1.5", ".default(1.5)")]
    #[case::quoted_string("'draft'", r#".default("draft")"#)]
    // SQL doubles quotes inside a single-quoted literal; the TS string wants
    // the actual value back.
    #[case::undoubled_quote("'it''s'", r#".default("it's")"#)]
    #[case::double_quoted("\"draft\"", r#".default("draft")"#)]
    fn literal_defaults_stay_literals(
        #[case] input: &str,
        #[case] expected: &str,
        #[values(Pg, Mysql, Sqlite)] dialect: DrizzleDialect,
    ) {
        let chain = default_chain(input, &text_type(), dialect);
        assert_eq!(chain.text, expected);
        assert!(!chain.needs_sql);
    }

    /// Both timestamp spellings collapse to each dialect's one drift-free
    /// spelling (measured on live round-trips): PostgreSQL deparses
    /// `CURRENT_TIMESTAMP` as itself, MySQL introspects to `defaultNow()`'s
    /// own `(now())`, SQLite needs the parenthesized expression.
    #[rstest]
    #[case::now_pg("now()", Pg, ".default(sql`CURRENT_TIMESTAMP`)", true)]
    #[case::now_mysql("now()", Mysql, ".defaultNow()", false)]
    #[case::now_sqlite("now()", Sqlite, ".default(sql`(CURRENT_TIMESTAMP)`)", true)]
    #[case::ct_pg("CURRENT_TIMESTAMP", Pg, ".default(sql`CURRENT_TIMESTAMP`)", true)]
    #[case::ct_mysql("CURRENT_TIMESTAMP", Mysql, ".defaultNow()", false)]
    #[case::ct_sqlite(
        "CURRENT_TIMESTAMP",
        Sqlite,
        ".default(sql`(CURRENT_TIMESTAMP)`)",
        true
    )]
    fn timestamp_defaults_fork_per_dialect(
        #[case] input: &str,
        #[case] dialect: DrizzleDialect,
        #[case] expected: &str,
        #[case] needs_sql: bool,
    ) {
        let chain = default_chain(input, &text_type(), dialect);
        assert_eq!(chain.text, expected);
        assert_eq!(chain.needs_sql, needs_sql);
    }

    /// `defaultRandom()` is a `pg-core` column method; the other dialects pass
    /// the generator expression through.
    #[rstest]
    #[case::pg(Pg, ".defaultRandom()", false)]
    #[case::mysql(Mysql, ".default(sql`(uuid())`)", true)]
    #[case::sqlite(Sqlite, ".default(sql`(lower(hex(randomblob(16))))`)", true)]
    fn uuid_defaults_fork_on_postgres(
        #[case] dialect: DrizzleDialect,
        #[case] expected: &str,
        #[case] needs_sql: bool,
    ) {
        let chain = default_chain("gen_random_uuid()", &text_type(), dialect);
        assert_eq!(chain.text, expected);
        assert_eq!(chain.needs_sql, needs_sql);
    }

    /// The SQL layer only normalizes `gen_random_uuid()`; other generator
    /// spellings reach every backend verbatim, and so does the model —
    /// `defaultRandom()` here would put `gen_random_uuid()` on a column whose
    /// database default is `uuid_generate_v4()`.
    #[rstest]
    #[case::pg(Pg)]
    #[case::mysql(Mysql)]
    #[case::sqlite(Sqlite)]
    fn other_uuid_generators_pass_through_verbatim(#[case] dialect: DrizzleDialect) {
        let chain = default_chain("uuid_generate_v4()", &text_type(), dialect);
        assert_eq!(chain.text, ".default(sql`uuid_generate_v4()`)");
        assert!(chain.needs_sql);
    }

    /// A JSON literal reaches the database as a JSON literal — `::json` on
    /// PostgreSQL because that is the type the SQL layer creates.
    #[rstest]
    #[case::pg(Pg, ".default(sql`'{\"a\": 1}'::json`)")]
    #[case::mysql(Mysql, ".default(sql`'{\"a\": 1}'`)")]
    fn json_literal_defaults_are_tagged(#[case] dialect: DrizzleDialect, #[case] expected: &str) {
        let ty = ColumnType::Simple(SimpleColumnType::Json);
        let chain = default_chain("{\"a\": 1}", &ty, dialect);
        assert_eq!(chain.text, expected);
        assert!(chain.needs_sql);
    }

    #[test]
    fn unknown_function_passes_through_the_sql_tag() {
        let chain = default_chain("gen_code()", &text_type(), Pg);
        assert_eq!(chain.text, ".default(sql`gen_code()`)");
        assert!(chain.needs_sql);
    }

    #[test]
    fn bare_keyword_passes_through_the_sql_tag() {
        let chain = default_chain("CURRENT_USER", &text_type(), Pg);
        assert_eq!(chain.text, ".default(sql`CURRENT_USER`)");
        assert!(chain.needs_sql);
    }

    /// Drizzle types a numeric/decimal default as a string — arbitrary
    /// precision exceeds a JS number.
    #[test]
    fn numeric_column_defaults_keep_the_literal_quoted() {
        let ty = ColumnType::Complex(ComplexColumnType::Numeric {
            precision: 10,
            scale: 2,
        });
        assert_eq!(default_chain("0.00", &ty, Pg).text, r#".default("0.00")"#);
    }

    /// An integer enum's default names a variant; the column stores its value.
    #[test]
    fn integer_enum_variant_default_resolves_to_its_value() {
        let ty = ColumnType::Complex(ComplexColumnType::Enum {
            name: "prio".to_string(),
            values: EnumValues::Integer(vec![NumValue {
                name: "low".to_string(),
                value: 7,
            }]),
        });
        assert_eq!(default_chain("low", &ty, Pg).text, ".default(7)");
    }

    #[test]
    fn escape_backtick_guards_template_syntax() {
        assert_eq!(escape_backtick("a`b${c}\\d"), "a\\`b\\${c}\\\\d");
    }

    // ── constraint entries ──────────────────────────────────────────────────

    #[rstest]
    #[case::pg_plain(Pg, false, ".primaryKey()")]
    #[case::pg_auto(Pg, true, ".primaryKey().generatedByDefaultAsIdentity()")]
    #[case::mysql_plain(Mysql, false, ".primaryKey()")]
    #[case::mysql_auto(Mysql, true, ".autoincrement().primaryKey()")]
    #[case::sqlite_plain(Sqlite, false, ".primaryKey()")]
    #[case::sqlite_auto(Sqlite, true, ".primaryKey({ autoIncrement: true })")]
    fn primary_key_chain_forks_per_dialect(
        #[case] dialect: DrizzleDialect,
        #[case] auto_inc: bool,
        #[case] expected: &str,
    ) {
        assert_eq!(primary_key_chain(dialect, auto_inc), expected);
    }

    #[test]
    fn table_level_entry_names_the_builder_call() {
        let columns: Vec<ColumnName> = vec!["a_col".into(), "b".into()];
        assert_eq!(
            table_level_entry("uniqueIndex", "uq_t__a_col_b", &columns),
            r#"  uniqueIndex("uq_t__a_col_b").on(t.aCol, t.b),"#
        );
    }

    /// Bindings over an empty schema: every lookup falls back to the natural
    /// name, which is exactly what these entries assert.
    fn empty_bindings() -> FileBindings {
        FileBindings::collect(&[], Pg)
    }

    fn fk_entry_for(
        columns: &[&str],
        ref_table: &str,
        ref_columns: &[&str],
        on_delete: Option<&ReferenceAction>,
        on_update: Option<&ReferenceAction>,
    ) -> String {
        let columns: Vec<ColumnName> = columns.iter().map(|c| (*c).into()).collect();
        let ref_columns: Vec<ColumnName> = ref_columns.iter().map(|c| (*c).into()).collect();
        foreign_key_entry(
            Some("fk_posts__x"),
            &columns,
            ref_table,
            &ref_columns,
            on_delete,
            on_update,
            "posts",
            &empty_bindings(),
        )
    }

    /// SQLite stores no FK constraint names, so its entries omit the field.
    #[test]
    fn unnamed_foreign_key_entry_omits_the_name_field() {
        let columns: Vec<ColumnName> = vec!["user_id".into()];
        let entry = foreign_key_entry(
            None,
            &columns,
            "users",
            &[],
            None,
            None,
            "posts",
            &empty_bindings(),
        );
        assert_eq!(
            entry,
            r"  foreignKey({ columns: [t.userId], foreignColumns: [users.id] }),"
        );
    }

    #[test]
    fn foreign_key_entry_spells_the_operator_form() {
        assert_eq!(
            fk_entry_for(&["user_id"], "users", &["id"], None, None),
            r#"  foreignKey({ columns: [t.userId], foreignColumns: [users.id], name: "fk_posts__x" }),"#
        );
    }

    #[test]
    fn composite_foreign_key_lists_every_column_in_order() {
        assert_eq!(
            fk_entry_for(&["a", "b"], "pair", &["x", "y"], None, None),
            r#"  foreignKey({ columns: [t.a, t.b], foreignColumns: [pair.x, pair.y], name: "fk_posts__x" }),"#
        );
    }

    /// A self-referential key takes its foreign columns from the callback's
    /// `t`, which keeps the table const out of its own initializer.
    #[test]
    fn self_referential_foreign_key_uses_the_callback_columns() {
        assert_eq!(
            fk_entry_for(&["parent_id"], "posts", &["id"], None, None),
            r#"  foreignKey({ columns: [t.parentId], foreignColumns: [t.id], name: "fk_posts__x" }),"#
        );
    }

    /// A foreign key with no explicit target column references the parent's
    /// primary key, which vespertide names `id` by convention.
    #[test]
    fn empty_ref_columns_fall_back_to_id() {
        assert_eq!(
            fk_entry_for(&["user_id"], "users", &[], None, None),
            r#"  foreignKey({ columns: [t.userId], foreignColumns: [users.id], name: "fk_posts__x" }),"#
        );
    }

    #[test]
    fn referential_actions_chain_after_the_operator() {
        assert_eq!(
            fk_entry_for(
                &["user_id"],
                "users",
                &["id"],
                Some(&ReferenceAction::Cascade),
                Some(&ReferenceAction::Restrict),
            ),
            r#"  foreignKey({ columns: [t.userId], foreignColumns: [users.id], name: "fk_posts__x" }).onDelete("cascade").onUpdate("restrict"),"#
        );
    }

    #[rstest]
    #[case::cascade(ReferenceAction::Cascade, "cascade")]
    #[case::restrict(ReferenceAction::Restrict, "restrict")]
    #[case::set_null(ReferenceAction::SetNull, "set null")]
    #[case::set_default(ReferenceAction::SetDefault, "set default")]
    #[case::no_action(ReferenceAction::NoAction, "no action")]
    fn reference_actions_map_to_drizzle_keywords(
        #[case] action: ReferenceAction,
        #[case] expected: &str,
    ) {
        assert_eq!(reference_action_to_drizzle(&action), expected);
    }

    #[test]
    fn js_name_escapes_digits_and_reserved_words() {
        assert_eq!(js_name("user_id"), "userId");
        assert_eq!(js_name("1st_place"), "x1stPlace");
        assert_eq!(js_name("default"), "default_");
    }
}
