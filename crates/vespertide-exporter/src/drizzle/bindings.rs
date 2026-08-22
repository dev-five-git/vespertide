//! File-scope binding names for one rendered Drizzle file.
//!
//! Four kinds of top-level `const`s share the module namespace with the
//! import symbols: `customType` helpers, `pgEnum` declarations, table
//! declarations and `relations` blocks. Distinct database names can collapse
//! onto one binding (`js_name` folds `_` and `-` alike), and a table can be
//! named after an import (`sql`, `integer`) — so every binding is claimed
//! here, in declaration order, and a later claimant takes a numeric suffix.
//! A schema with no collisions keeps every natural name, byte for byte.

use std::collections::{HashMap, HashSet};

use vespertide_core::TableDef;
use vespertide_core::schema::column::EnumValues;

use super::enums::enum_const_name;
use super::types::custom_column;
use super::{DrizzleDialect, js_name};
use crate::utils::common::claim_binding;

/// The generated callbacks' parameter names: the table callback is
/// `(t) => [...]` and the relations callback `({ one, many }) => ({...})`,
/// and claimed consts are interpolated *inside* those scopes — a table named
/// `t` would otherwise render foreign columns against the callback parameter
/// (compiling, silently wrong).
const CALLBACK_SCOPE_NAMES: [&str; 3] = ["t", "one", "many"];

/// Every symbol a rendered file can import for the dialect — from the
/// dialect-core line and the `drizzle-orm` line — so a binding never shadows
/// one. Guarded by `vocabulary_covers_every_import` below against new
/// constructors in `types.rs`.
fn import_vocabulary(dialect: DrizzleDialect) -> &'static [&'static str] {
    const PG: [&str; 29] = [
        "pgTable",
        "pgEnum",
        "customType",
        "primaryKey",
        "foreignKey",
        "uniqueIndex",
        "index",
        "check",
        "relations",
        "sql",
        "bigint",
        "boolean",
        "char",
        "cidr",
        "date",
        "doublePrecision",
        "inet",
        "integer",
        "interval",
        "json",
        "macaddr",
        "numeric",
        "real",
        "smallint",
        "text",
        "time",
        "timestamp",
        "uuid",
        "varchar",
    ];
    const MYSQL: [&str; 25] = [
        "mysqlTable",
        "mysqlEnum",
        "customType",
        "primaryKey",
        "foreignKey",
        "uniqueIndex",
        "index",
        "check",
        "relations",
        "sql",
        "bigint",
        "binary",
        "boolean",
        "char",
        "date",
        "decimal",
        "double",
        "float",
        "int",
        "json",
        "smallint",
        "text",
        "time",
        "timestamp",
        "varchar",
    ];
    const SQLITE: [&str; 14] = [
        "sqliteTable",
        "customType",
        "primaryKey",
        "foreignKey",
        "uniqueIndex",
        "index",
        "check",
        "relations",
        "sql",
        "blob",
        "integer",
        "numeric",
        "real",
        "text",
    ];
    match dialect {
        DrizzleDialect::Pg => &PG,
        DrizzleDialect::Mysql => &MYSQL,
        DrizzleDialect::Sqlite => &SQLITE,
    }
}

/// Binding names for one file, keyed by identity rather than by natural name
/// so colliding claimants each find their own (suffixed) binding.
pub(super) struct FileBindings {
    /// `customType` const per SQL data type.
    customs: HashMap<String, String>,
    /// `pgEnum` const per `(table, enum name)`.
    enums: HashMap<(String, String), String>,
    /// Table const per table name.
    tables: HashMap<String, String>,
    /// `relations` const per table name.
    relations: HashMap<String, String>,
}

impl FileBindings {
    /// Claim every binding the file will declare, in declaration order:
    /// `customType` consts, then `pgEnum` consts, then tables, then
    /// `relations` blocks — seeded with the import vocabulary and the
    /// callback parameter names so no binding shadows either.
    pub(super) fn collect(tables: &[TableDef], dialect: DrizzleDialect) -> Self {
        let mut taken: HashSet<String> = import_vocabulary(dialect)
            .iter()
            .chain(CALLBACK_SCOPE_NAMES.iter())
            .map(|s| (*s).to_string())
            .collect();

        let mut customs: HashMap<String, String> = HashMap::new();
        for table in tables {
            for col in &table.columns {
                if let Some(decl) = custom_column(&col.r#type, dialect)
                    && !customs.contains_key(&decl.data_type)
                {
                    let claimed = claim_binding(decl.const_name.clone(), &mut taken);
                    customs.insert(decl.data_type, claimed);
                }
            }
        }

        // Each map claims once per key: the export does not validate its
        // input, so a schema carrying two tables with one name (impossible in
        // a real database) renders them onto one binding rather than
        // suffixing names apart.
        let mut enums: HashMap<(String, String), String> = HashMap::new();
        if dialect == DrizzleDialect::Pg {
            for table in tables {
                for (name, values) in crate::enum_scan::collect_table_enums(table) {
                    let key = (table.name.to_string(), name.to_string());
                    // Integer enums stay plain integer columns and declare
                    // nothing.
                    if matches!(values, EnumValues::String(_)) && !enums.contains_key(&key) {
                        let claimed = claim_binding(enum_const_name(&table.name, name), &mut taken);
                        enums.insert(key, claimed);
                    }
                }
            }
        }

        let mut table_consts: HashMap<String, String> = HashMap::new();
        for table in tables {
            if !table_consts.contains_key(table.name.as_str()) {
                let claimed = claim_binding(js_name(&table.name), &mut taken);
                table_consts.insert(table.name.to_string(), claimed);
            }
        }

        let mut relations: HashMap<String, String> = HashMap::new();
        for table in tables {
            if !relations.contains_key(table.name.as_str()) {
                let preferred = format!("{}Relations", table_consts[table.name.as_str()]);
                let claimed = claim_binding(preferred, &mut taken);
                relations.insert(table.name.to_string(), claimed);
            }
        }

        Self {
            customs,
            enums,
            tables: table_consts,
            relations,
        }
    }

    /// The `customType` const for a data type; falls back to the natural name
    /// for a type the collect pass never saw.
    pub(super) fn custom_const(&self, data_type: &str) -> String {
        self.customs
            .get(data_type)
            .cloned()
            .unwrap_or_else(|| js_name(data_type))
    }

    /// The `pgEnum` const a column of `table` calls for `enum_name`.
    pub(super) fn enum_const(&self, table: &str, enum_name: &str) -> String {
        self.enums
            .get(&(table.to_string(), enum_name.to_string()))
            .cloned()
            .unwrap_or_else(|| enum_const_name(table, enum_name))
    }

    /// The table's const. The natural-name fallback is a live path: a foreign
    /// key may reference a table outside the schema slice (the export does not
    /// validate dangling references).
    pub(super) fn table_const(&self, table: &str) -> String {
        self.tables
            .get(table)
            .cloned()
            .unwrap_or_else(|| js_name(table))
    }

    /// The table's `relations` const.
    pub(super) fn relations_const(&self, table: &str) -> String {
        self.relations
            .get(table)
            .cloned()
            .unwrap_or_else(|| format!("{}Relations", js_name(table)))
    }
}
