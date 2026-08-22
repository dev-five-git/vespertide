//! Column type mapping: `ColumnType` → a Drizzle column constructor.
//!
//! Each dialect exposes its own set of constructors, so the mapping forks
//! three ways. Every arm produces a [`ColumnCtor`], which carries both the
//! import symbol and the call text — the import header and the column body are
//! derived from the same decision rather than from two parallel matches.

use vespertide_core::schema::column::{
    ColumnType, ComplexColumnType, EnumValues, SimpleColumnType,
};

use super::bindings::FileBindings;
use super::{DrizzleDialect, js_name};
use crate::utils::typescript::ts_string;

/// One resolved Drizzle column constructor.
pub(super) struct ColumnCtor {
    /// Bare constructor name — also the symbol to import, unless `local`.
    pub(super) symbol: String,
    /// Everything after the column-name argument, e.g. `, { length: 255 }`.
    args: String,
    /// Trailing comment naming the source type when the dialect has no exact
    /// counterpart and the column is widened onto another constructor.
    note: String,
    /// `symbol` names a `const` declared in this file (a `pgEnum` or
    /// `customType` helper), so it must be kept out of the dialect-core
    /// import list.
    pub(super) local: bool,
}

impl ColumnCtor {
    /// The full constructor call, e.g. `varchar("email", { length: 255 })`.
    pub(super) fn call(&self, col_db: &str) -> String {
        format!(
            "{}({}{}){}",
            self.symbol,
            ts_string(col_db),
            self.args,
            self.note
        )
    }
}

fn ctor(symbol: &str) -> ColumnCtor {
    ColumnCtor {
        symbol: symbol.to_string(),
        args: String::new(),
        note: String::new(),
        local: false,
    }
}

fn ctor_args(symbol: &str, args: String) -> ColumnCtor {
    ColumnCtor {
        args,
        ..ctor(symbol)
    }
}

/// A column the dialect cannot represent exactly, mapped onto a wider
/// constructor with the source type recorded in a comment.
fn widened(symbol: &str, args: String, source: &str) -> ColumnCtor {
    ColumnCtor {
        args,
        note: format!(" /* {source} */"),
        ..ctor(symbol)
    }
}

/// `["draft", "published"]` — the variant list MySQL and SQLite inline into the
/// column, since neither declares an enum type separately.
fn enum_value_list(values: &[String]) -> String {
    let items: Vec<String> = values.iter().map(|v| ts_string(v)).collect();
    format!("[{}]", items.join(", "))
}

// ─── customType helpers ──────────────────────────────────────────────────────

/// A `customType` helper const: the column resolves to it, and the file has to
/// declare it. One source feeds both, so the declaration and its call sites
/// cannot disagree.
pub(super) struct CustomTypeDecl {
    /// The natural binding (`js_name` of the data type); the final one comes
    /// from `FileBindings`, which suffixes it on a file-scope collision.
    pub(super) const_name: String,
    pub(super) data_type: String,
    /// The TypeScript type query results carry for the column.
    ts_data: &'static str,
}

impl CustomTypeDecl {
    fn new(data_type: &str, ts_data: &'static str) -> Self {
        Self {
            const_name: js_name(data_type),
            data_type: data_type.to_string(),
            ts_data,
        }
    }
}

/// The call side of a `customType` column: the (claimed) const it invokes.
fn custom_ctor(decl: &CustomTypeDecl, bindings: &FileBindings) -> ColumnCtor {
    ColumnCtor {
        symbol: bindings.custom_const(&decl.data_type),
        local: true,
        ..ctor("")
    }
}

/// `const bytea = customType<{ data: Uint8Array }>({ dataType() { return "bytea"; } });`
///
/// `const_name` is the claimed binding — possibly suffixed — not necessarily
/// the decl's natural one.
pub(super) fn render_custom_type_decl(decl: &CustomTypeDecl, const_name: &str) -> String {
    format!(
        "const {const_name} = customType<{{ data: {} }}>({{ dataType() {{ return {}; }} }});",
        decl.ts_data,
        ts_string(&decl.data_type)
    )
}

// `Uint8Array` rather than `Buffer`: the `pg` driver hands back a `Buffer`,
// which *is* a `Uint8Array` — and the model file keeps compiling without
// `@types/node`.
fn pg_bytea() -> CustomTypeDecl {
    CustomTypeDecl::new("bytea", "Uint8Array")
}

fn pg_xml() -> CustomTypeDecl {
    CustomTypeDecl::new("xml", "string")
}

/// The SQL layer passes a `Custom` type's name to every backend verbatim, so
/// every dialect's model spells it back verbatim through `customType`.
fn custom_passthrough(custom_type: &str) -> CustomTypeDecl {
    CustomTypeDecl::new(custom_type, "string")
}

/// The `customType` declaration `ty` needs under `dialect`, if any.
///
/// PostgreSQL adds `bytea` and `xml` here because `pg-core` has no constructor
/// for either and widening them onto `text` reads as a column-type change to
/// `drizzle-kit`; on MySQL and SQLite those two map onto real constructors.
pub(super) fn custom_column(ty: &ColumnType, dialect: DrizzleDialect) -> Option<CustomTypeDecl> {
    match ty {
        ColumnType::Simple(SimpleColumnType::Bytea) if dialect == DrizzleDialect::Pg => {
            Some(pg_bytea())
        }
        ColumnType::Simple(SimpleColumnType::Xml) if dialect == DrizzleDialect::Pg => {
            Some(pg_xml())
        }
        ColumnType::Complex(ComplexColumnType::Custom { custom_type }) => {
            Some(custom_passthrough(custom_type))
        }
        _ => None,
    }
}

/// Resolve the constructor for `ty` under `dialect`.
pub(super) fn column_ctor(
    ty: &ColumnType,
    dialect: DrizzleDialect,
    table: &str,
    bindings: &FileBindings,
) -> ColumnCtor {
    match dialect {
        DrizzleDialect::Pg => pg_ctor(ty, table, bindings),
        DrizzleDialect::Mysql => mysql_ctor(ty, table, bindings),
        DrizzleDialect::Sqlite => sqlite_ctor(ty, table, bindings),
    }
}

fn pg_ctor(ty: &ColumnType, table: &str, bindings: &FileBindings) -> ColumnCtor {
    match ty {
        ColumnType::Simple(s) => match s {
            SimpleColumnType::SmallInt => ctor("smallint"),
            SimpleColumnType::Integer => ctor("integer"),
            // `bigint` is arbitrary-precision in PostgreSQL but a JS `number`
            // loses precision past 2^53, so the mode has to be explicit.
            SimpleColumnType::BigInt => ctor_args("bigint", ", { mode: \"number\" }".to_string()),
            SimpleColumnType::Real => ctor("real"),
            SimpleColumnType::DoublePrecision => ctor("doublePrecision"),
            SimpleColumnType::Boolean => ctor("boolean"),
            SimpleColumnType::Date => ctor("date"),
            SimpleColumnType::Time => ctor("time"),
            SimpleColumnType::Timestamp => ctor("timestamp"),
            SimpleColumnType::Timestamptz => {
                ctor_args("timestamp", ", { withTimezone: true }".to_string())
            }
            SimpleColumnType::Uuid => ctor("uuid"),
            // The SQL layer creates `json`, not `jsonb` — the model has to
            // match the column the migration actually made.
            SimpleColumnType::Json => ctor("json"),
            SimpleColumnType::Interval => ctor("interval"),
            SimpleColumnType::Inet => ctor("inet"),
            SimpleColumnType::Cidr => ctor("cidr"),
            SimpleColumnType::Macaddr => ctor("macaddr"),
            // `pg-core` exports no `bytea` or `xml` constructor; both go
            // through a `customType` helper so the type name reaches
            // `drizzle-kit` unchanged.
            SimpleColumnType::Bytea => custom_ctor(&pg_bytea(), bindings),
            SimpleColumnType::Xml => custom_ctor(&pg_xml(), bindings),
            SimpleColumnType::Text => ctor("text"),
            _ => unreachable!(
                "SimpleColumnType is #[non_exhaustive]; all variants are matched above"
            ),
        },
        ColumnType::Complex(c) => complex_ctor(c, DrizzleDialect::Pg, table, bindings),
    }
}

fn mysql_ctor(ty: &ColumnType, table: &str, bindings: &FileBindings) -> ColumnCtor {
    match ty {
        ColumnType::Simple(s) => match s {
            SimpleColumnType::SmallInt => ctor("smallint"),
            SimpleColumnType::Integer => ctor("int"),
            SimpleColumnType::BigInt => ctor_args("bigint", ", { mode: \"number\" }".to_string()),
            SimpleColumnType::Real => ctor("float"),
            SimpleColumnType::DoublePrecision => ctor("double"),
            SimpleColumnType::Boolean => ctor("boolean"),
            SimpleColumnType::Date => ctor("date"),
            SimpleColumnType::Time => ctor("time"),
            // vespertide maps both timestamp types onto MySQL `TIMESTAMP`, so
            // the generated model follows the SQL layer rather than splitting
            // them across `datetime` and `timestamp`.
            SimpleColumnType::Timestamp | SimpleColumnType::Timestamptz => ctor("timestamp"),
            // The SQL layer stores MySQL uuids as `binary(16)`.
            SimpleColumnType::Uuid => widened("binary", ", { length: 16 }".to_string(), "uuid"),
            SimpleColumnType::Json => ctor("json"),
            // MySQL has no counterpart for the PostgreSQL-specific types.
            SimpleColumnType::Interval => widened("text", String::new(), "interval"),
            // `binary(1)` mirrors the SQL layer's MySQL bytea column.
            SimpleColumnType::Bytea => widened("binary", ", { length: 1 }".to_string(), "bytea"),
            SimpleColumnType::Inet => widened("text", String::new(), "inet"),
            SimpleColumnType::Cidr => widened("text", String::new(), "cidr"),
            SimpleColumnType::Macaddr => widened("text", String::new(), "macaddr"),
            SimpleColumnType::Xml => widened("text", String::new(), "xml"),
            SimpleColumnType::Text => ctor("text"),
            _ => unreachable!(
                "SimpleColumnType is #[non_exhaustive]; all variants are matched above"
            ),
        },
        ColumnType::Complex(c) => complex_ctor(c, DrizzleDialect::Mysql, table, bindings),
    }
}

fn sqlite_ctor(ty: &ColumnType, table: &str, bindings: &FileBindings) -> ColumnCtor {
    match ty {
        ColumnType::Simple(s) => match s {
            // SQLite stores every integer as a 64-bit INTEGER; drizzle's
            // `bigint` mode lives on `blob`, which would change the storage
            // class, so `big_int` stays an integer column like the SQL layer's.
            SimpleColumnType::SmallInt | SimpleColumnType::Integer | SimpleColumnType::BigInt => {
                ctor("integer")
            }
            SimpleColumnType::Real | SimpleColumnType::DoublePrecision => ctor("real"),
            SimpleColumnType::Boolean => {
                ctor_args("integer", ", { mode: \"boolean\" }".to_string())
            }
            // SQLite has no date/time storage class; vespertide's own SQLite
            // SQL stores these as TEXT, and the model matches it.
            SimpleColumnType::Date => widened("text", String::new(), "date"),
            SimpleColumnType::Time => widened("text", String::new(), "time"),
            SimpleColumnType::Timestamp => widened("text", String::new(), "timestamp"),
            SimpleColumnType::Timestamptz => widened("text", String::new(), "timestamptz"),
            SimpleColumnType::Uuid => widened("text", String::new(), "uuid"),
            SimpleColumnType::Json => ctor_args("text", ", { mode: \"json\" }".to_string()),
            // SQLite has no counterpart for the PostgreSQL-specific types.
            SimpleColumnType::Interval => widened("text", String::new(), "interval"),
            SimpleColumnType::Bytea => widened("blob", String::new(), "bytea"),
            SimpleColumnType::Inet => widened("text", String::new(), "inet"),
            SimpleColumnType::Cidr => widened("text", String::new(), "cidr"),
            SimpleColumnType::Macaddr => widened("text", String::new(), "macaddr"),
            SimpleColumnType::Xml => widened("text", String::new(), "xml"),
            SimpleColumnType::Text => ctor("text"),
            _ => unreachable!(
                "SimpleColumnType is #[non_exhaustive]; all variants are matched above"
            ),
        },
        ColumnType::Complex(c) => complex_ctor(c, DrizzleDialect::Sqlite, table, bindings),
    }
}

fn complex_ctor(
    c: &ComplexColumnType,
    dialect: DrizzleDialect,
    table: &str,
    bindings: &FileBindings,
) -> ColumnCtor {
    match c {
        ComplexColumnType::Varchar { length } => match dialect {
            DrizzleDialect::Pg | DrizzleDialect::Mysql => {
                ctor_args("varchar", format!(", {{ length: {length} }}"))
            }
            DrizzleDialect::Sqlite => ctor_args("text", format!(", {{ length: {length} }}")),
        },
        ComplexColumnType::Char { length } => match dialect {
            DrizzleDialect::Pg | DrizzleDialect::Mysql => {
                ctor_args("char", format!(", {{ length: {length} }}"))
            }
            DrizzleDialect::Sqlite => widened("text", format!(", {{ length: {length} }}"), "char"),
        },
        ComplexColumnType::Numeric { precision, scale } => match dialect {
            DrizzleDialect::Pg => ctor_args(
                "numeric",
                format!(", {{ precision: {precision}, scale: {scale} }}"),
            ),
            DrizzleDialect::Mysql => ctor_args(
                "decimal",
                format!(", {{ precision: {precision}, scale: {scale} }}"),
            ),
            // SQLite's `numeric` takes no precision or scale.
            DrizzleDialect::Sqlite => ctor("numeric"),
        },
        ComplexColumnType::Custom { custom_type } => {
            custom_ctor(&custom_passthrough(custom_type), bindings)
        }
        ComplexColumnType::Enum { name, values } => match values {
            // Integer enums are stored as their numeric value, so they stay
            // plain integer columns in every dialect.
            EnumValues::Integer(_) => match dialect {
                DrizzleDialect::Pg | DrizzleDialect::Sqlite => ctor("integer"),
                DrizzleDialect::Mysql => ctor("int"),
            },
            EnumValues::String(vals) => match dialect {
                // PostgreSQL declares the enum as its own type; the column
                // calls that const.
                DrizzleDialect::Pg => ColumnCtor {
                    symbol: bindings.enum_const(table, name),
                    local: true,
                    ..ctor("")
                },
                DrizzleDialect::Mysql => {
                    ctor_args("mysqlEnum", format!(", {}", enum_value_list(vals)))
                }
                DrizzleDialect::Sqlite => {
                    ctor_args("text", format!(", {{ enum: {} }}", enum_value_list(vals)))
                }
            },
        },
        _ => unreachable!("ComplexColumnType is #[non_exhaustive]; all variants are matched above"),
    }
}
