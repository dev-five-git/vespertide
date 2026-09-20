//! Top-level names of a generated file that holds a whole schema in one scope.
//!
//! GORM writes every struct, enum type and enum constant into one Go package;
//! Django writes every model and choices class into one module. A table and an
//! enum that share a name (`role` and `user.role`), two names that fold onto
//! one identifier, or an enum constant that spells a struct (`Status` + `code`
//! next to `status_code`) would otherwise be declared twice. Names are claimed
//! here once for the whole schema and looked up by what they name, so every
//! reference agrees with its declaration.
//!
//! Drizzle's `drizzle::bindings` is a different claim order over a different
//! scope: it starts from the dialect's import symbols and the callback
//! parameters, claims `customType` helpers and `relations` consts as well, and
//! always qualifies an enum with its table.

use std::collections::{HashMap, HashSet};

use vespertide_core::TableDef;

use crate::enum_scan::collect_table_enums;
use crate::utils::common::claim_binding;

#[derive(PartialEq, Eq, Hash)]
enum Named {
    Table(String),
    Enum(String, String),
    Member(String, String, usize),
}

#[derive(Default)]
pub(crate) struct ScopeNames {
    names: HashMap<Named, String>,
    taken: HashSet<String>,
}

impl ScopeNames {
    /// Claim a name for every table, then for every table's enums. Tables go
    /// first so a table always keeps its natural name. An enum keeps its bare
    /// identifier only while no other table declares the same one and nothing
    /// holds it yet; otherwise it is qualified with the name its table claimed.
    pub(crate) fn collect(
        schema: &[TableDef],
        table_identifier: impl Fn(&str) -> String,
        enum_identifier: impl Fn(&str) -> String,
    ) -> Self {
        let mut scope = Self::default();
        for table in schema {
            scope.claim(
                Named::Table(table.name.to_string()),
                table_identifier(&table.name),
            );
        }

        let shared = identifiers_shared_across_tables(schema, &enum_identifier);
        for table in schema {
            for (enum_name, _) in collect_table_enums(table) {
                let bare = enum_identifier(enum_name);
                let natural = if shared.contains(&bare) || scope.taken.contains(&bare) {
                    format!(
                        "{}{bare}",
                        scope.names[&Named::Table(table.name.to_string())]
                    )
                } else {
                    bare
                };
                scope.claim(
                    Named::Enum(table.name.to_string(), enum_name.to_string()),
                    natural,
                );
            }
        }
        scope
    }

    /// Claim the `index`-th member of a table's enum, for a backend whose enum
    /// members share the file's scope.
    pub(crate) fn claim_member(
        &mut self,
        table: &str,
        enum_name: &str,
        index: usize,
        natural: String,
    ) {
        self.claim(
            Named::Member(table.to_string(), enum_name.to_string(), index),
            natural,
        );
    }

    /// The first claim for a key stands: two columns of one table may share an
    /// enum, and a schema may list a table twice.
    fn claim(&mut self, key: Named, natural: String) {
        if !self.names.contains_key(&key) {
            let name = claim_binding(natural, &mut self.taken);
            self.names.insert(key, name);
        }
    }

    /// `None` for a table outside the schema the names were collected from —
    /// a foreign key may point there — which callers answer with the natural
    /// name.
    pub(crate) fn table(&self, table: &str) -> Option<&str> {
        self.names
            .get(&Named::Table(table.to_string()))
            .map(String::as_str)
    }

    /// The type of an enum `table` declares. Only that table's own render asks,
    /// and every render collects its names from a slice that holds the table:
    /// the schema for a whole file, [`scope_of`] for a single table.
    pub(crate) fn enum_type(&self, table: &str, enum_name: &str) -> &str {
        &self.names[&Named::Enum(table.to_string(), enum_name.to_string())]
    }

    /// The `index`-th member of an enum `table` declares, as claimed through
    /// [`Self::claim_member`].
    pub(crate) fn member(&self, table: &str, enum_name: &str, index: usize) -> &str {
        &self.names[&Named::Member(table.to_string(), enum_name.to_string(), index)]
    }
}

/// The tables whose names share a file with `table`'s: `schema` when it holds
/// the table, the table alone otherwise — a render without schema context
/// still claims the table's own names against each other.
pub(crate) fn scope_of<'a>(table: &'a TableDef, schema: &'a [TableDef]) -> &'a [TableDef] {
    if schema.contains(table) {
        schema
    } else {
        std::slice::from_ref(table)
    }
}

/// Identifiers more than one table of `schema` declares an enum under. Names
/// are compared after `identifier` has converted them, since distinct names
/// can collapse onto the same one (`doc_status` and `docStatus`).
fn identifiers_shared_across_tables(
    schema: &[TableDef],
    identifier: impl Fn(&str) -> String,
) -> HashSet<String> {
    let mut tables_declaring: HashMap<String, usize> = HashMap::new();
    for table in schema {
        let declared: HashSet<String> = collect_table_enums(table)
            .into_iter()
            .map(|(name, _)| identifier(name))
            .collect();
        for ident in declared {
            *tables_declaring.entry(ident).or_default() += 1;
        }
    }
    tables_declaring
        .into_iter()
        .filter(|(_, tables)| *tables > 1)
        .map(|(ident, _)| ident)
        .collect()
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use vespertide_core::schema::column::{ColumnType, ComplexColumnType, EnumValues};
    use vespertide_core::{ColumnDef, TableDef};

    use super::{ScopeNames, scope_of};
    use crate::python_naming::to_pascal_case;

    fn table(name: &str, enums: &[&str]) -> TableDef {
        TableDef {
            name: name.into(),
            description: None,
            columns: enums
                .iter()
                .map(|enum_name| {
                    let ty = ColumnType::Complex(ComplexColumnType::Enum {
                        name: (*enum_name).to_string(),
                        values: EnumValues::String(vec!["a".into()]),
                    });
                    ColumnDef::new(*enum_name, ty, false)
                })
                .collect(),
            constraints: vec![],
        }
    }

    fn collect(schema: &[TableDef]) -> ScopeNames {
        ScopeNames::collect(schema, to_pascal_case, to_pascal_case)
    }

    #[rstest]
    // Nothing else wants the identifier: the enum keeps it bare.
    #[case::unique(&[("orders", &["status"][..])], "orders", "status", "Status")]
    // A table of the same name holds it, whichever is declared first.
    #[case::taken_by_a_later_table(&[("user", &["role"][..]), ("role", &[])], "user", "role", "UserRole")]
    #[case::taken_by_an_earlier_table(&[("role", &[][..]), ("user", &["role"])], "user", "role", "UserRole")]
    // Two tables declare it: both are qualified.
    #[case::shared(&[("orders", &["status"][..]), ("tasks", &["status"])], "tasks", "status", "TasksStatus")]
    // The qualified name is itself a table: numbered.
    #[case::qualified_name_taken(
        &[("orders", &["status"][..]), ("tasks", &["status"]), ("tasks_status", &[])],
        "tasks",
        "status",
        "TasksStatus2"
    )]
    // Two names of one table fold onto one identifier: the second finds it
    // held, so each still gets a type of its own.
    #[case::folds_within_a_table(&[("docs", &["doc_status", "docStatus"][..])], "docs", "docStatus", "DocsDocStatus")]
    // The qualifier is the name the table claimed, not the one it wanted.
    #[case::qualified_by_a_numbered_table(
        &[("user_data", &["status"][..]), ("userData", &["status"])],
        "userData",
        "status",
        "UserData2Status"
    )]
    fn enums_are_named_clear_of_everything_else_in_the_scope(
        #[case] schema: &[(&str, &[&str])],
        #[case] table_name: &str,
        #[case] enum_name: &str,
        #[case] expected: &str,
    ) {
        let schema: Vec<TableDef> = schema
            .iter()
            .map(|(name, enums)| table(name, enums))
            .collect();
        assert_eq!(collect(&schema).enum_type(table_name, enum_name), expected);
    }

    #[test]
    fn tables_that_fold_onto_one_identifier_are_numbered() {
        let names = collect(&[table("user_data", &[]), table("userData", &[])]);
        assert_eq!(names.table("user_data"), Some("UserData"));
        assert_eq!(names.table("userData"), Some("UserData2"));
    }

    #[test]
    fn members_share_the_scope_with_tables_and_enums() {
        let mut names = collect(&[table("status_code", &[]), table("ticket", &["status"])]);
        names.claim_member("ticket", "status", 0, "StatusCode".into());
        names.claim_member("ticket", "status", 1, "Status".into());
        assert_eq!(names.member("ticket", "status", 0), "StatusCode2");
        assert_eq!(names.member("ticket", "status", 1), "Status2");
    }

    #[test]
    fn a_table_outside_the_schema_is_unknown() {
        let names = collect(&[table("orders", &["status"])]);
        assert_eq!(names.table("users"), None);
    }

    /// A schema that does not hold the table is no scope for it.
    #[test]
    fn a_table_is_its_own_scope_outside_its_schema() {
        let orders = table("orders", &["status"]);
        let schema = [table("users", &[]), orders.clone()];
        let alone = std::slice::from_ref(&orders);
        assert_eq!(scope_of(&orders, &schema), schema);
        assert_eq!(scope_of(&orders, &schema[..1]), alone);
        assert_eq!(scope_of(&orders, &[]), alone);
    }
}
