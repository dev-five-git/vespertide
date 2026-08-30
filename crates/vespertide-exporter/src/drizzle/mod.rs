//! Drizzle ORM (TypeScript) schema generation.
//!
//! Drizzle has no backend-neutral output: `pgTable`, `mysqlTable` and
//! `sqliteTable` live in three different packages that fork at the `import`
//! line, so — unlike Prisma, which stays neutral by omitting native type
//! attributes — one schema renders to one file *per dialect*. The CLI writes
//! all three; [`render_schema`] renders one.

mod bindings;
mod enums;
mod render;
mod types;

use std::collections::HashSet;

use vespertide_core::TableDef;
use vespertide_core::schema::column::EnumValues;

use crate::orm::OrmExporter;
use crate::utils::typescript::ts_binding;
use bindings::FileBindings;
use enums::render_enum_decl;
use render::{render_relations_block, render_table};
use types::{custom_column, render_custom_type_decl};
use vespertide_naming::{build_enum_type_name, to_camel_case};

/// A database name as a TypeScript binding or object key.
///
/// Bindings, object keys and property accesses all have to agree, so every
/// site derives them from here rather than casing the name itself.
fn js_name(db_name: &str) -> String {
    ts_binding(&to_camel_case(db_name))
}

pub struct DrizzleExporter;

impl OrmExporter for DrizzleExporter {
    fn render_entity(&self, table: &TableDef) -> Result<String, String> {
        // A lone table is its own schema: a self-referential FK needs both
        // relation ends in scope for its `relationName` pair to be emitted.
        Ok(render_entity_with_schema(
            table,
            std::slice::from_ref(table),
        ))
    }

    fn render_entity_with_schema(
        &self,
        table: &TableDef,
        schema: &[TableDef],
    ) -> Result<String, String> {
        Ok(render_entity_with_schema(table, schema))
    }
}

/// The Drizzle package family a rendered file targets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DrizzleDialect {
    Pg,
    Mysql,
    Sqlite,
}

impl DrizzleDialect {
    /// Every dialect, in the order the CLI writes their files.
    pub const ALL: [DrizzleDialect; 3] = [
        DrizzleDialect::Pg,
        DrizzleDialect::Mysql,
        DrizzleDialect::Sqlite,
    ];

    /// Segment naming the dialect's output file (`models.pg.ts`).
    #[must_use]
    pub fn file_suffix(self) -> &'static str {
        match self {
            DrizzleDialect::Pg => "pg",
            DrizzleDialect::Mysql => "mysql",
            DrizzleDialect::Sqlite => "sqlite",
        }
    }

    /// The table-declaration function, which also identifies the dialect in
    /// the import header's rank ordering.
    fn table_fn(self) -> &'static str {
        match self {
            DrizzleDialect::Pg => "pgTable",
            DrizzleDialect::Mysql => "mysqlTable",
            DrizzleDialect::Sqlite => "sqliteTable",
        }
    }

    /// The package the dialect's constructors are imported from.
    fn import_path(self) -> &'static str {
        match self {
            DrizzleDialect::Pg => "drizzle-orm/pg-core",
            DrizzleDialect::Mysql => "drizzle-orm/mysql-core",
            DrizzleDialect::Sqlite => "drizzle-orm/sqlite-core",
        }
    }
}

/// Import requirements collected while rendering.
///
/// `render_table` records each decision — constructor symbols, constraint
/// helpers, whether a default reached for the `sql` tag — as it makes it, so
/// the header assembled afterwards cannot disagree with the body.
#[derive(Default)]
struct Imports {
    /// Dialect-core symbols; `header()` imposes the final order. Locally
    /// declared consts (`pgEnum`, `customType`) never enter this set.
    symbols: HashSet<String>,
    /// A default rendered through the `sql` tagged template.
    needs_sql: bool,
    /// At least one `relations(...)` block was emitted.
    needs_relations: bool,
}

impl Imports {
    /// The import block: dialect-core line first, then `drizzle-orm` if needed.
    fn header(&self, dialect: DrizzleDialect) -> String {
        let table_fn = dialect.table_fn();
        let mut symbols: Vec<&str> = self.symbols.iter().map(String::as_str).collect();
        symbols.sort_by(|a, b| {
            symbol_rank(a, table_fn)
                .cmp(&symbol_rank(b, table_fn))
                .then(a.cmp(b))
        });

        let mut lines = vec![format!(
            "import {{ {} }} from \"{}\";",
            symbols.join(", "),
            dialect.import_path()
        )];

        let mut orm_symbols: Vec<&str> = Vec::new();
        if self.needs_relations {
            orm_symbols.push("relations");
        }
        if self.needs_sql {
            orm_symbols.push("sql");
        }
        if !orm_symbols.is_empty() {
            lines.push(format!(
                "import {{ {} }} from \"drizzle-orm\";",
                orm_symbols.join(", ")
            ));
        }
        lines.join("\n")
    }
}

/// Sort key for the dialect-core import list.
fn symbol_rank(symbol: &str, table_fn: &str) -> usize {
    // Type and constraint helpers in import order; the dialect's table
    // function precedes them and column constructors follow, alphabetical
    // within that tail.
    const RANKED_HELPERS: [&str; 7] = [
        "pgEnum",
        "customType",
        "primaryKey",
        "foreignKey",
        "uniqueIndex",
        "index",
        "check",
    ];
    if symbol == table_fn {
        0
    } else {
        let mut rank = RANKED_HELPERS.len() + 1;
        for (position, helper) in RANKED_HELPERS.iter().enumerate() {
            if *helper == symbol {
                rank = position + 1;
                break;
            }
        }
        rank
    }
}

/// Render every table into one Drizzle schema file for `dialect`.
///
/// Output order: imports → `customType` declarations → enum declarations
/// (PostgreSQL only — MySQL and SQLite inline their variants into the column)
/// → table declarations → relations declarations. Every enum type is table-prefixed
/// (`{table}_{enum}`) because that is the `CREATE TYPE` the SQL layer emits —
/// two tables sharing a model-level enum own two database types, and the file
/// declares one `pgEnum` per table accordingly.
pub fn render_schema(tables: &[TableDef], dialect: DrizzleDialect) -> String {
    let mut imports = Imports::default();
    let bindings = FileBindings::collect(tables, dialect);

    let custom_blocks = custom_type_decls(tables, dialect, &bindings);
    if !custom_blocks.is_empty() {
        imports.symbols.insert("customType".to_string());
    }

    let mut enum_blocks: Vec<String> = Vec::new();
    if dialect == DrizzleDialect::Pg {
        for table in tables {
            enum_blocks.extend(table_enum_decls(table, &bindings));
        }
        if !enum_blocks.is_empty() {
            imports.symbols.insert("pgEnum".to_string());
        }
    }

    let table_blocks: Vec<String> = tables
        .iter()
        .map(|table| render_table(table, dialect, &mut imports, &bindings))
        .collect();

    let mut relation_blocks: Vec<String> = Vec::new();
    for table in tables {
        if let Some(block) = render_relations_block(table, tables, &bindings) {
            imports.needs_relations = true;
            relation_blocks.push(block);
        }
    }

    let mut parts = vec![imports.header(dialect)];
    parts.extend(custom_blocks);
    parts.extend(enum_blocks);
    parts.extend(table_blocks);
    parts.extend(relation_blocks);
    parts.join("\n\n") + "\n"
}

/// One `customType` helper const per distinct data type the dialect has no
/// constructor for, in first-appearance order across the file's columns.
fn custom_type_decls(
    tables: &[TableDef],
    dialect: DrizzleDialect,
    bindings: &FileBindings,
) -> Vec<String> {
    let mut seen: HashSet<String> = HashSet::new();
    let mut decls = Vec::new();
    for table in tables {
        for col in &table.columns {
            if let Some(decl) = custom_column(&col.r#type, dialect)
                && seen.insert(decl.data_type.clone())
            {
                decls.push(render_custom_type_decl(
                    &decl,
                    &bindings.custom_const(&decl.data_type),
                ));
            }
        }
    }
    decls
}

/// Render enum + table + relations blocks with full schema context.
///
/// No import header: this is the cross-ORM harness's per-table view, which —
/// like the other backends' single-entity output — carries declarations only.
/// The trait has a single `String` result and so no dialect axis; PostgreSQL
/// is the canonical dialect for cross-ORM comparison, matching the enum-typed
/// output the other backends produce.
pub fn render_entity_with_schema(table: &TableDef, schema: &[TableDef]) -> String {
    // Discarded: header-free output has no use for the collected symbols.
    let mut imports = Imports::default();
    // Bindings over the whole schema, so this fragment names things exactly
    // as the full file would.
    let bindings = FileBindings::collect(schema, DrizzleDialect::Pg);

    let mut parts = custom_type_decls(std::slice::from_ref(table), DrizzleDialect::Pg, &bindings);
    parts.extend(table_enum_decls(table, &bindings));
    parts.push(render_table(
        table,
        DrizzleDialect::Pg,
        &mut imports,
        &bindings,
    ));
    if let Some(block) = render_relations_block(table, schema, &bindings) {
        parts.push(block);
    }
    parts.join("\n\n")
}

/// One `pgEnum` declaration per string enum the table's columns use, under the
/// table-prefixed name the SQL layer's `CREATE TYPE` carries. Integer enums
/// are plain integer columns in every dialect and declare nothing.
fn table_enum_decls(table: &TableDef, bindings: &FileBindings) -> Vec<String> {
    let mut decls = Vec::new();
    for (name, values) in crate::enum_scan::collect_table_enums(table) {
        let EnumValues::String(vals) = values else {
            continue;
        };
        let db_name = build_enum_type_name(table.name.as_str(), name);
        let const_name = bindings.enum_const(table.name.as_str(), name);
        decls.push(render_enum_decl(&const_name, &db_name, vals));
    }
    decls
}

/// Multi-table entry point: render every table (enum + table + relations
/// blocks) with full schema context and join them. Mirrors the other ORMs'
/// `export` so the cross-ORM test harness can dispatch Drizzle through a
/// single call.
pub fn export(schema: &[TableDef]) -> Result<String, String> {
    Ok(schema
        .iter()
        .map(|table| render_entity_with_schema(table, schema))
        .collect::<Vec<_>>()
        .join("\n\n"))
}

#[cfg(test)]
mod tests {
    use insta::{assert_snapshot, with_settings};
    use rstest::rstest;

    use super::*;
    use crate::tests::fixtures::{
        binding_collisions, composite_pk, composite_unique, enum_shared, small_multi_schema,
        table_with_check, table_with_integer_enum, unique_and_indexed,
    };

    /// The complete per-dialect file: import header (rank-ordered), enum
    /// declarations (PostgreSQL only), table declarations, relations. One
    /// schema, three files — this is the dialect fork the cross-ORM harness
    /// cannot see, since the trait path renders PostgreSQL only.
    #[rstest]
    #[case::pg(DrizzleDialect::Pg)]
    #[case::mysql(DrizzleDialect::Mysql)]
    #[case::sqlite(DrizzleDialect::Sqlite)]
    fn render_schema_full_file_per_dialect(#[case] dialect: DrizzleDialect) {
        // Every constraint family is present so the import header exercises
        // its full rank ordering — primaryKey (composite), foreignKey,
        // unique, index, check — plus the `sql` import the check forces.
        let mut tables = small_multi_schema();
        tables.push(enum_shared());
        tables.push(composite_pk());
        tables.push(composite_unique());
        tables.push(unique_and_indexed());
        tables.push(table_with_check());
        // An integer enum stays a plain integer column and must not add a
        // `pgEnum` declaration.
        tables.push(table_with_integer_enum());
        let rendered = render_schema(&tables, dialect);
        with_settings!(
            { snapshot_path => "../tests/snapshots", snapshot_suffix => dialect.file_suffix() },
            { assert_snapshot!(rendered); }
        );
    }

    /// The import header lists the table function first, the type and
    /// constraint helpers next, and column constructors last (alphabetical
    /// within the tail rank).
    #[rstest]
    #[case::table_fn("pgTable", 0)]
    #[case::pg_enum("pgEnum", 1)]
    #[case::custom_type("customType", 2)]
    #[case::primary_key("primaryKey", 3)]
    #[case::foreign_key("foreignKey", 4)]
    #[case::unique_index("uniqueIndex", 5)]
    #[case::index("index", 6)]
    #[case::check("check", 7)]
    #[case::constructor("integer", 8)]
    fn symbol_rank_orders_helpers_before_constructors(
        #[case] symbol: &str,
        #[case] expected: usize,
    ) {
        assert_eq!(symbol_rank(symbol, "pgTable"), expected);
    }

    /// Two tables declaring the same enum name. PostgreSQL gives each its own
    /// table-prefixed type — that is the `CREATE TYPE` the SQL layer emits —
    /// while MySQL and SQLite declare nothing and inline the variants on the
    /// column instead.
    #[rstest]
    #[case::pg(DrizzleDialect::Pg)]
    #[case::mysql(DrizzleDialect::Mysql)]
    #[case::sqlite(DrizzleDialect::Sqlite)]
    fn render_schema_enum_declarations_per_dialect(#[case] dialect: DrizzleDialect) {
        let orders = table_with_enum("orders", "status", &["new", "paid"]);
        let tickets = table_with_enum("tickets", "status", &["new", "paid"]);
        let rendered = render_schema(&[orders, tickets], dialect);
        with_settings!(
            { snapshot_path => "../tests/snapshots", snapshot_suffix => dialect.file_suffix() },
            { assert_snapshot!(rendered); }
        );
    }

    fn table_with_enum(name: &str, enum_name: &str, values: &[&str]) -> TableDef {
        use vespertide_core::schema::column::{ColumnType, ComplexColumnType, EnumValues};
        use vespertide_core::schema::primary_key::PrimaryKeySyntax;
        use vespertide_core::{ColumnDef, SimpleColumnType};

        TableDef {
            name: name.into(),
            description: None,
            columns: vec![
                ColumnDef::new("id", ColumnType::Simple(SimpleColumnType::Integer), false)
                    .primary_key(PrimaryKeySyntax::Bool(true)),
                ColumnDef::new(
                    "st",
                    ColumnType::Complex(ComplexColumnType::Enum {
                        name: enum_name.into(),
                        values: EnumValues::String(
                            values.iter().copied().map(Into::into).collect(),
                        ),
                    }),
                    false,
                ),
            ],
            constraints: vec![],
        }
        .normalize()
        .expect("fixture table normalizes")
    }

    /// File-scope bindings suffix their way around collisions: a table
    /// claiming another table's would-be `relations` const, a table named
    /// after an import, and a custom type named after a column constructor —
    /// and every reference follows the suffixed name. Per dialect, because
    /// the import vocabulary a binding can collide with differs (MySQL has no
    /// `integer` constructor, so that custom type keeps its natural name).
    #[rstest]
    #[case::pg(DrizzleDialect::Pg)]
    #[case::mysql(DrizzleDialect::Mysql)]
    #[case::sqlite(DrizzleDialect::Sqlite)]
    fn render_schema_suffixes_colliding_bindings(#[case] dialect: DrizzleDialect) {
        let rendered = render_schema(&binding_collisions(), dialect);
        with_settings!(
            { snapshot_path => "../tests/snapshots", snapshot_suffix => dialect.file_suffix() },
            { assert_snapshot!(rendered); }
        );
    }

    /// A model-level shared enum still becomes one database type per table —
    /// mirroring the per-table `CREATE TYPE` the SQL layer emits.
    #[test]
    fn render_schema_declares_one_enum_type_per_table() {
        let t1 = enum_shared();
        let mut t2 = enum_shared();
        t2.name = "archived_documents".into();
        let rendered = render_schema(&[t1, t2], DrizzleDialect::Pg);
        assert_eq!(rendered.matches("pgEnum(").count(), 2);
    }
}
