//! PostgreSQL enum declarations and the naming they share with their columns.
//!
//! Only PostgreSQL declares an enum type of its own. MySQL inlines the variant
//! list into `mysqlEnum(…)` and SQLite has no enum at all, so both spell the
//! variants directly on the column — see `types::complex_ctor`.

use vespertide_naming::build_enum_type_name;

use crate::utils::typescript::ts_string;

/// The enum type's database name: `{table}_{enum}`.
///
/// The SQL layer runs **every** PostgreSQL enum through
/// [`build_enum_type_name`] — the `CREATE TYPE` is table-prefixed even when no
/// other table declares the same enum — so the model does too, or `drizzle-kit`
/// sees a type the database never had. This also means two tables sharing an
/// enum declaration in the model own two separate database types, and the file
/// declares one `pgEnum` per table accordingly.
pub(super) fn enum_db_name(table: &str, enum_name: &str) -> String {
    build_enum_type_name(table, enum_name)
}

/// The natural `const` binding for an enum declaration, derived from the
/// database type name so the two stay recognisably paired. The final binding
/// comes from `FileBindings`, which suffixes this name on a file-scope
/// collision.
pub(super) fn enum_const_name(table: &str, enum_name: &str) -> String {
    super::js_name(&enum_db_name(table, enum_name))
}

/// Render a `pgEnum` declaration:
/// `export const ordersStatus = pgEnum("orders_status", ["draft", "published"]);`
///
/// Takes the string values directly — integer enums render as plain integer
/// columns and never declare a type. The values stay verbatim: Drizzle sends
/// them to PostgreSQL as written, so there is no variant-name normalization
/// and nothing that would need a `@map` equivalent.
pub(super) fn render_enum_decl(const_name: &str, db_name: &str, values: &[String]) -> String {
    let variants: Vec<String> = values.iter().map(|v| ts_string(v)).collect();
    format!(
        "export const {const_name} = pgEnum({}, [{}]);",
        ts_string(db_name),
        variants.join(", ")
    )
}
